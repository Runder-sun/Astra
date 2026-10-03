import { taskBindingIsCurrent, taskHasRetainedOwner } from "./task-ownership.ts";

export { taskHasRetainedOwner } from "./task-ownership.ts";

import { join } from "node:path";
import {
	backtrackChecksFromSnapshot,
	buildEffectiveTaskContractFromSnapshot,
	EffectiveContractUnavailableError,
	type EffectiveTaskContract,
	repairContext,
	resolveRepairInputFromSnapshot,
	semanticContractHash,
	taskContractMatches,
} from "./effective-contract.ts";
import type { ResearchJob } from "./research.ts";
import type { Evidence, JobSnapshot, StageDefinition, StagePlanManifest, TaskPacket } from "./types.ts";

export const PLAN_REVIEW_CHECKS = [
	"The plan advances the mission within its boundaries and addresses the current stage purpose",
	"Task scopes are actionable, feasible within budgets, and do not duplicate or depend on concurrent outputs",
	"Declared inputs provide the evidence needed for the proposed work and are current",
	"Output fields and acceptance criteria can falsify an inadequate result rather than merely confirm execution",
	"Local work has a coherent path to complete synthesis; complete deliveries cover the stage contract",
	"The plan addresses outstanding repair and previous plan-review findings without weakening requirements",
];

/** Planning must see the inherited requirements that dispatch adds to the proposed task. */
export function planDispatchAdditions(job: ResearchJob, stageId: string, obligationId?: string) {
	return planDispatchAdditionsFromSnapshot(job.state, stageId, obligationId);
}

export function planDispatchAdditionsFromSnapshot(
	snapshot: JobSnapshot,
	stageId: string,
	obligationId?: string,
	context = repairContext(snapshot),
) {
	const obligation = obligationId ? snapshot.obligations[obligationId] : undefined;
	const review = obligation ? snapshot.reviews[obligation.sourceReviewId] : undefined;
	const evidence = review ? snapshot.evidence[review.evidenceId] : undefined;
	const task = evidence ? snapshot.tasks[evidence.taskId] : undefined;
	return {
		inheritedTask: task
			? {
					id: task.id,
					requiredOutputFields: task.requiredOutputFields,
					acceptanceChecks: [...new Set(task.acceptanceChecks.map((criterion) => context.normalize(criterion)))],
					successCriteria: [...new Set(task.successCriteria.map((criterion) => context.normalize(criterion)))],
					failureSignals: task.failureSignals,
				}
			: null,
		reviewFindings: review?.findings ?? [],
		openRepairChecks: (evidence
			? (context.obligationsByLineage.get(evidence.currentEvidenceSetId ?? evidence.taskId) ?? [])
			: []
		).flatMap((item) =>
			(item.items ?? [])
				.filter((check) => check.status === "open")
				.map((check) => ({ issueId: check.id, criterion: context.normalize(check.criterion) })),
		),
		backtrackChecks: backtrackChecksFromSnapshot(snapshot, stageId),
	};
}

function planEvidenceFromSnapshot(snapshot: JobSnapshot, planId: string): Evidence | undefined {
	return Object.values(snapshot.evidence).find(
		(evidence) => evidence.type === "stage-plan" && snapshot.tasks[evidence.taskId]?.planId === planId,
	);
}

export function planEvidence(job: Pick<ResearchJob, "state">, planId: string): Evidence | undefined {
	return planEvidenceFromSnapshot(job.state, planId);
}

export function hasFrozenPlanContract(job: Pick<ResearchJob, "state">, planId: string): boolean {
	return hasFrozenPlanContractFromSnapshot(job.state, planId);
}

export function hasFrozenPlanContractFromSnapshot(snapshot: JobSnapshot, planId: string): boolean {
	const evidence = planEvidenceFromSnapshot(snapshot, planId);
	return Array.isArray((evidence?.content as { effectiveContracts?: unknown } | undefined)?.effectiveContracts);
}

function planBasisUnchanged(snapshot: JobSnapshot, planId: string, evidence: Evidence): boolean {
	const task = snapshot.tasks[evidence.taskId];
	const context = evidence.content as {
		plan: StagePlanManifest;
		guidanceRefs: string[];
		effectiveContracts?: Array<{ hash: string; contract: EffectiveTaskContract }>;
	};
	const guidanceRefs = Object.values(snapshot.graph.nodes)
		.filter((node) => node.actor === "user")
		.map((node) => node.id)
		.sort();
	return (
		task.stageRevision === (snapshot.stages[evidence.stageId]?.revision ?? 1) &&
		JSON.stringify(context.plan) === JSON.stringify(snapshot.stagePlans[planId]) &&
		JSON.stringify(context.guidanceRefs) === JSON.stringify(guidanceRefs) &&
		!task.inputArtifactRefs.some(
			(ref) => snapshot.retiredArtifacts[ref] || snapshot.canonical[ref]?.status === "stale",
		)
	);
}

function currentPlanBasis(snapshot: JobSnapshot, planId: string, evidence: Evidence): boolean {
	return (
		evidence.status !== "rejected" &&
		!snapshot.discardedEvidence[evidence.id] &&
		!Object.values(snapshot.discardedCandidates).some((receipt) => receipt.evidenceId === evidence.id) &&
		taskIdentityIsCurrent(snapshot, snapshot.tasks[evidence.taskId]) &&
		planBasisUnchanged(snapshot, planId, evidence)
	);
}

function repairAncestors(snapshot: JobSnapshot, contract: EffectiveTaskContract, workerTaskId?: string): Set<string> {
	const workerEvidence = workerTaskId
		? Object.values(snapshot.evidence).find((entry) => entry.taskId === workerTaskId)
		: undefined;
	const ancestors = new Set<string>();
	let ancestorId = contract.repairOfEvidenceId;
	while (ancestorId && !ancestors.has(ancestorId)) {
		const ancestor = snapshot.evidence[ancestorId];
		const source = ancestor ? snapshot.tasks[ancestor.taskId] : undefined;
		if (
			!ancestor ||
			!source ||
			source.jobId !== snapshot.frame.jobId ||
			source.stageId !== contract.stageId ||
			ancestor.stageId !== contract.stageId ||
			(workerTaskId && ancestor.currentEvidenceSetId !== workerEvidence?.currentEvidenceSetId)
		)
			break;
		ancestors.add(ancestorId);
		ancestorId = source.repairOfEvidenceId;
	}
	return ancestors;
}

function planInputsUnchanged(snapshot: JobSnapshot, evidence: Evidence, workerTaskId?: string): boolean {
	if (!Array.isArray((evidence.content as { effectiveContracts?: unknown }).effectiveContracts)) return true;
	const context = evidence.content as { effectiveContracts: Array<{ contract: EffectiveTaskContract }> };
	for (const frozen of context.effectiveContracts) {
		const ancestors = repairAncestors(snapshot, frozen.contract, workerTaskId);
		for (const input of frozen.contract.inputVersions) {
			const canonical = snapshot.canonical[input.inputRef];
			const currentEvidence = snapshot.evidence[canonical?.evidenceId ?? input.inputRef];
			if (
				!currentEvidence &&
				!canonical &&
				!input.canonical &&
				workerTaskId &&
				input.versionHash &&
				input.evidenceId === input.inputRef
			) {
				const worker = snapshot.tasks[workerTaskId];
				const winner = Object.values(snapshot.evidence).find((entry) => entry.taskId === workerTaskId);
				const adopted = winner
					? snapshot.canonical[snapshot.canonicalRoute.stageArtifactIds[winner.stageId]]
					: undefined;
				const receipt = snapshot.discardedEvidence[input.inputRef];
				const cleanup = snapshot.cleanupIntents?.[`evidence:${input.inputRef}`];
				const obligation = Object.values(snapshot.obligations).find(
					(item) => item.evidenceId === input.inputRef && item.targetVersionHash === input.versionHash,
				);
				const chain = new Set<string>();
				let ancestorId = worker.repairOfEvidenceId;
				while (ancestorId && !chain.has(ancestorId)) {
					const ancestor = snapshot.evidence[ancestorId];
					const sourceId = ancestor?.taskId ?? snapshot.discardedEvidence[ancestorId]?.taskId;
					const source = sourceId ? snapshot.tasks[sourceId] : undefined;
					if (
						!source ||
						source.jobId !== worker.jobId ||
						source.stageId !== worker.stageId ||
						(ancestor &&
							(ancestor.stageId !== worker.stageId ||
								ancestor.currentEvidenceSetId !== winner?.currentEvidenceSetId))
					)
						break;
					chain.add(ancestorId);
					ancestorId = source.repairOfEvidenceId;
				}
				// The original obligation proves the deleted version; the receipt alone cannot reconstruct its bytes.
				if (
					winner?.status === "accepted" &&
					adopted?.status === "active" &&
					adopted.adoptionCompletedAt &&
					adopted.evidenceId === winner.id &&
					adopted.type === winner.type &&
					adopted.checksum === winner.checksum &&
					adopted.sourceSha256 === winner.checksum &&
					taskContractMatches(worker, frozen.contract) &&
					chain.has(input.inputRef) &&
					!snapshot.retiredArtifacts[input.inputRef] &&
					!Object.values(snapshot.retiredArtifacts).some((entry) => entry.evidenceId === input.inputRef) &&
					!Object.values(snapshot.canonical).some((entry) => entry.evidenceId === input.inputRef) &&
					receipt?.evidenceId === input.inputRef &&
					receipt.reason === "superseded-repair" &&
					receipt.cleanupStatus === "completed" &&
					cleanup?.kind === "evidence" &&
					cleanup.status === "completed" &&
					cleanup.receipt.evidenceId === receipt.evidenceId &&
					cleanup.receipt.taskId === receipt.taskId &&
					cleanup.receipt.checksum === receipt.checksum &&
					obligation?.status === "resolved" &&
					receipt.reviewIds.includes(obligation.sourceReviewId) &&
					obligation.items?.length &&
					obligation.items.every((item) => {
						const review = item.reviewId ? snapshot.reviews[item.reviewId] : undefined;
						const criterion = frozen.contract.repairChecks.find((check) => check.issueId === item.id)?.criterion;
						return (
							item.status === "resolved" &&
							item.evidenceId === winner.id &&
							review?.evidenceId === winner.id &&
							review.targetVersionHash === winner.versionHash &&
							review.verdict === "pass" &&
							(review.score ?? 0) >= 0.8 &&
							Boolean(
								criterion && review.criteria?.some((check) => check.criterion === criterion && check.passed),
							)
						);
					})
				)
					continue;
			}
			if (input.evidenceId !== (currentEvidence?.id ?? null)) return false;
			if (input.versionHash !== (currentEvidence?.versionHash ?? canonical?.evidenceSnapshotHash ?? null))
				return false;
			if (
				(currentEvidence?.status === "rejected" &&
					!(
						input.inputRef === frozen.contract.repairOfEvidenceId &&
						(!workerTaskId ||
							!currentEvidence.supersededByTaskId ||
							currentEvidence.supersededByTaskId === workerTaskId)
					) &&
					!(
						!canonical &&
						workerTaskId &&
						input.versionHash !== null &&
						ancestors.has(input.inputRef) &&
						currentEvidence.supersededByTaskId === workerTaskId
					)) ||
				canonical?.status === "stale"
			)
				return false;
			if (
				input.canonical &&
				(!canonical ||
					input.canonical.id !== canonical.id ||
					input.canonical.evidenceSnapshotHash !== canonical.evidenceSnapshotHash ||
					input.canonical.sourceSha256 !== canonical.sourceSha256 ||
					input.canonical.targetSha256 !== canonical.targetSha256)
			)
				return false;
		}
	}
	return true;
}

export function evidenceHasCurrentPlanApproval(job: ResearchJob, evidence: Evidence): boolean {
	return evidenceHasCurrentPlanApprovalFromSnapshot(job.state, evidence);
}

export function evidenceHasCurrentPlanApprovalFromSnapshot(snapshot: JobSnapshot, evidence: Evidence): boolean {
	const worker = snapshot.tasks[evidence.taskId];
	if (
		!worker ||
		evidence.stageId !== worker.stageId ||
		evidence.type !== worker.requiredOutputType ||
		(worker.role !== "worker" && !(worker.role === "main-agent" && evidence.type === "stage-plan")) ||
		!taskIdentityIsCurrent(snapshot, worker)
	)
		return false;
	if (evidence.type === "stage-plan")
		return Boolean(
			worker.planId &&
				currentPlanBasis(snapshot, worker.planId, evidence) &&
				planInputsUnchanged(snapshot, evidence),
		);
	return taskIsCurrentFromSnapshot(snapshot, worker);
}

function taskIdentityIsCurrent(snapshot: JobSnapshot, task: TaskPacket): boolean {
	return (
		taskBindingIsCurrent(snapshot, task) &&
		!Object.values(snapshot.cleanupIntents ?? {}).some(
			(intent) =>
				intent.status === "completed" &&
				intent.archiveRefs?.includes(
					join(
						snapshot.frame.permissions.workspaceRoot,
						".astra",
						"jobs",
						task.jobId,
						"archive",
						"tasks",
						task.id,
					),
				),
		) &&
		(![
			...Object.values(snapshot.retiredArtifacts),
			...Object.values(snapshot.discardedEvidence),
			...Object.values(snapshot.discardedCandidates),
		].some((receipt) => receipt.taskId === task.id) ||
			taskHasRetainedOwner(snapshot, task.id))
	);
}

/** The original frozen input and exact winner, not a shared lineage label, authorize a repair comparison. */
export function taskHasBoundRepairAncestor(snapshot: JobSnapshot, task: TaskPacket, ref: string): boolean {
	const input = snapshot.evidence[ref];
	if (
		!task.planId ||
		!input ||
		input.status !== "rejected" ||
		input.supersededByTaskId !== task.id ||
		snapshot.canonical[ref] ||
		Object.values(snapshot.canonical).some((artifact) => artifact.evidenceId === ref) ||
		snapshot.retiredArtifacts[ref] ||
		Object.values(snapshot.retiredArtifacts).some((receipt) => receipt.evidenceId === ref)
	)
		return false;
	const plan = planEvidenceFromSnapshot(snapshot, task.planId);
	if (!plan || !matchesApprovedWorkerContract(snapshot, task, plan)) return false;
	const frozen = (
		plan.content as { effectiveContracts: Array<{ hash: string; contract: EffectiveTaskContract }> }
	).effectiveContracts.find((entry) => entry.hash === task.effectiveContractHash)!;
	return (
		repairAncestors(snapshot, frozen.contract, task.id).has(ref) &&
		frozen.contract.inputVersions.some(
			(version) =>
				version.inputRef === ref && version.evidenceId === input.id && version.versionHash === input.versionHash,
		)
	);
}

/** Current work and ordinary inputs share one identity rule; historical repair targets stay comparisons. */
export function taskIsCurrentFromSnapshot(
	snapshot: JobSnapshot,
	task: TaskPacket,
	checked = new Set<string>(),
): boolean {
	if (!taskIdentityIsCurrent(snapshot, task)) return false;
	if (checked.has(task.id)) return true;
	checked.add(task.id);
	for (const ref of task.inputArtifactRefs) {
		if (
			ref === task.repairOfEvidenceId &&
			Object.values(snapshot.obligations).some(
				(issue) => issue.evidenceId === ref || snapshot.reviews[issue.sourceReviewId]?.evidenceId === ref,
			)
		) {
			const historical = snapshot.evidence[ref];
			if (historical) {
				try {
					if (
						snapshot.tasks[historical.taskId].inputArtifactRefs.some(
							(oldRef) => !task.inputArtifactRefs.includes(resolveRepairInputFromSnapshot(snapshot, oldRef)),
						)
					)
						return false;
				} catch (error) {
					if (error instanceof EffectiveContractUnavailableError) return false;
					throw error;
				}
				continue;
			}
		}
		if (
			snapshot.retiredArtifacts[ref] ||
			snapshot.canonical[ref]?.status === "stale" ||
			Object.values(snapshot.retiredArtifacts).some((receipt) => receipt.evidenceId === ref) ||
			Object.values(snapshot.canonical).some(
				(artifact) => artifact.evidenceId === ref && artifact.status === "stale",
			)
		)
			return false;
		if (taskHasBoundRepairAncestor(snapshot, task, ref)) continue;
		const evidence = snapshot.evidence[snapshot.canonical[ref]?.evidenceId ?? ref];
		if (
			evidence &&
			(evidence.status === "rejected" ||
				evidence.supersededByTaskId ||
				!taskIsCurrentFromSnapshot(snapshot, snapshot.tasks[evidence.taskId], checked) ||
				!taskHasApprovedPlanFromSnapshot(snapshot, snapshot.tasks[evidence.taskId]))
		)
			return false;
	}
	return task.role !== "worker" || taskHasApprovedPlanFromSnapshot(snapshot, task);
}

function taskHasApprovedPlanFromSnapshot(snapshot: JobSnapshot, worker: TaskPacket): boolean {
	if (worker.role !== "worker") return true;
	const planId = worker?.planId;
	if (!planId) return true;
	const planEvidenceValue = planEvidenceFromSnapshot(snapshot, planId);
	if (!planEvidenceValue || !currentPlanBasis(snapshot, planId, planEvidenceValue)) return false;
	return matchesApprovedWorkerContract(snapshot, worker, planEvidenceValue);
}

/** Only for an already applied adoption whose original comparisons were proved from the journal. */
export function evidenceMatchesHistoricalPlanFromSnapshot(snapshot: JobSnapshot, evidence: Evidence): boolean {
	const worker = snapshot.tasks[evidence.taskId];
	if (!worker?.planId) return true;
	const plan = planEvidenceFromSnapshot(snapshot, worker.planId);
	return Boolean(
		plan &&
			planBasisUnchanged(snapshot, worker.planId, plan) &&
			matchesApprovedWorkerContract(snapshot, worker, plan),
	);
}

function matchesApprovedWorkerContract(
	snapshot: JobSnapshot,
	worker: TaskPacket,
	planEvidenceValue: Evidence,
): boolean {
	const planReviews = Object.values(snapshot.reviews).filter(
		(review) =>
			review.evidenceId === planEvidenceValue.id && review.targetVersionHash === planEvidenceValue.versionHash,
	);
	const approval =
		planReviews.length > 0 && planReviews.every((review) => review.verdict === "pass" && (review.score ?? 0) >= 0.8);
	if (!approval || !planInputsUnchanged(snapshot, planEvidenceValue, worker.id)) return false;
	if (!Array.isArray((planEvidenceValue.content as { effectiveContracts?: unknown }).effectiveContracts)) return false;
	const context = planEvidenceValue.content as {
		effectiveContracts: Array<{ hash: string; contract: EffectiveTaskContract }>;
	};
	const frozen = context.effectiveContracts.find((entry) => entry.hash === worker.effectiveContractHash);
	return Boolean(
		frozen && frozen.hash === semanticContractHash(frozen.contract) && taskContractMatches(worker, frozen.contract),
	);
}

export function planReviewStatus(job: ResearchJob, planId: string): "pending" | "passed" | "failed" | "stale" {
	return planReviewStatusFromSnapshot(job.state, job.definitions, planId);
}

export function planReviewStatusFromSnapshot(
	snapshot: JobSnapshot,
	definitions: Record<string, StageDefinition>,
	planId: string,
): "pending" | "passed" | "failed" | "stale" {
	const evidence = planEvidenceFromSnapshot(snapshot, planId);
	if (!evidence) {
		const source = Object.values(snapshot.tasks).find(
			(task) => task.planId === planId && task.requiredOutputType === "stage-plan" && task.role === "main-agent",
		);
		if (
			source &&
			(!taskIsCurrentFromSnapshot(snapshot, source) ||
				[...Object.values(snapshot.discardedEvidence), ...Object.values(snapshot.discardedCandidates)].some(
					(receipt) => receipt.taskId === source.id && !taskHasRetainedOwner(snapshot, source.id),
				))
		)
			return "stale";
		return "pending";
	}
	const context = evidence.content as {
		effectiveContracts?: Array<{ hash: string; contract: EffectiveTaskContract }>;
	};
	if (
		!currentPlanBasis(snapshot, planId, evidence) ||
		!taskIsCurrentFromSnapshot(snapshot, snapshot.tasks[evidence.taskId]) ||
		!planInputsUnchanged(snapshot, evidence)
	)
		return "stale";
	const repairs = repairContext(snapshot);
	try {
		if (
			context.effectiveContracts?.some((frozen, index) => {
				const plan = snapshot.stagePlans[planId];
				const planned = plan?.tasks[index];
				if (!planned) return true;
				const current = buildEffectiveTaskContractFromSnapshot(snapshot, definitions, plan, planned, repairs);
				return (
					frozen.hash !== semanticContractHash(frozen.contract) || frozen.hash !== semanticContractHash(current)
				);
			})
		)
			return "stale";
	} catch (error) {
		if (error instanceof EffectiveContractUnavailableError) return "stale";
		throw error;
	}
	const reviews = Object.values(snapshot.reviews).filter(
		(review) => review.evidenceId === evidence.id && review.targetVersionHash === evidence.versionHash,
	);
	if (reviews.some((review) => review.verdict !== "pass" || (review.score ?? 0) < 0.8)) return "failed";
	return reviews.length ? "passed" : "pending";
}

export async function preparePlanEvidence(job: ResearchJob, plan: StagePlanManifest): Promise<Evidence> {
	if (planReviewStatus(job, plan.id) === "stale") throw new Error("stage plan evidence is stale or was discarded");
	const existing = planEvidence(job, plan.id);
	if (existing) return existing;
	const snapshot = job.state;
	const repairs = repairContext(snapshot);
	const obligation = plan.obligationId ? snapshot.obligations[plan.obligationId] : undefined;
	const failed = obligation ? snapshot.evidence[snapshot.reviews[obligation.sourceReviewId]?.evidenceId] : undefined;
	const effectiveContracts = plan.tasks.map((planned) => {
		const contract = buildEffectiveTaskContractFromSnapshot(snapshot, job.definitions, plan, planned, repairs);
		return { hash: semanticContractHash(contract), contract };
	});
	const inputs = [...new Set(effectiveContracts.flatMap(({ contract }) => contract.inputArtifactRefs))];
	const task = await job.dispatchTask({
		repairOfEvidenceId: failed?.id,
		planId: plan.id,
		replayKey: `plan-review:${plan.id}`,
		stageId: plan.stageId,
		stageExecutionId: snapshot.stages[plan.stageId].executionId ?? plan.stageId,
		role: "main-agent",
		objective:
			"Independently audit this execution plan before any worker is dispatched. Assess planning adequacy, not whether the planned experiments have already succeeded.",
		inputArtifactRefs: inputs,
		requiredCanonicalArtifacts: inputs.filter((id) => Boolean(snapshot.canonical[id])),
		requiredOutputType: "stage-plan",
		requiredOutputFields: ["plan", "mission", "capability"],
		acceptanceChecks: PLAN_REVIEW_CHECKS,
		successCriteria: [],
		failureSignals: ["uncovered requirements", "unverifiable criteria", "ignored repairs"],
		dependencies: [],
		scope: { workspaceRoot: snapshot.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 8, maxToolCalls: 32, maxRuntimeMs: 180000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	await job.setTaskStatus(task.id, "succeeded");
	return job.recordEvidence({
		taskId: task.id,
		stageId: plan.stageId,
		type: "stage-plan",
		refs: [],
		content: {
			dispatchAdditions: planDispatchAdditionsFromSnapshot(snapshot, plan.stageId, plan.obligationId, repairs),
			inputStates: inputs.map((inputRef) => {
				const artifact = snapshot.canonical[inputRef];
				const evidence = snapshot.evidence[artifact?.evidenceId ?? inputRef];
				return {
					inputRef,
					evidenceId: evidence?.id ?? null,
					evidenceStatus: evidence?.status ?? null,
					canonical: artifact
						? {
								id: artifact.id,
								status: artifact.status,
								adoptedAt: artifact.adoptedAt,
								evidenceSnapshotHash: artifact.evidenceSnapshotHash,
							}
						: null,
				};
			}),
			userGuidance: Object.values(snapshot.graph.nodes)
				.filter((node) => node.actor === "user")
				.sort((left, right) => left.updatedAt.localeCompare(right.updatedAt)),
			historicalRepairArchives: job.historicalRepairArchives(plan.stageId),
			guidanceRefs: Object.values(snapshot.graph.nodes)
				.filter((node) => node.actor === "user")
				.map((node) => node.id)
				.sort(),
			plan,
			effectiveContracts,
			mission: snapshot.frame,
			capability: job.definitions[plan.stageId],
			obligation: plan.obligationId ? snapshot.obligations[plan.obligationId] : undefined,
			previousFindings: Object.values(snapshot.reviews)
				.filter(
					(review) =>
						snapshot.evidence[review.evidenceId]?.type === "stage-plan" &&
						snapshot.evidence[review.evidenceId]?.stageId === plan.stageId &&
						review.verdict !== "pass",
				)
				.map((review) => ({ reviewId: review.id, findings: review.findings, criteria: review.criteria })),
		},
	});
}
