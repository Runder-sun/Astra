import { createHash } from "node:crypto";
import type { ResearchJob } from "./research.ts";
import { groupRepairCriteria } from "./review-validation.ts";
import type {
	JobSnapshot,
	Obligation,
	PlannedTask,
	ResponsibilityTransfer,
	SearchBatch,
	StageDefinition,
	StagePlanManifest,
	TaskBudget,
	TaskPacket,
} from "./types.ts";

export class EffectiveContractUnavailableError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "EffectiveContractUnavailableError";
	}
}

export interface EffectiveTaskContract {
	planId: string;
	stageId: string;
	stageExecutionId: string;
	stageRevision: number;
	role: "worker";
	deliveryKind: "stage" | "local" | "synthesis";
	repairOfEvidenceId?: string;
	objective: string;
	inputArtifactRefs: string[];
	requiredCanonicalArtifacts: string[];
	requiredOutputType: string;
	requiredOutputFields: string[];
	acceptanceChecks: string[];
	failureSignals: string[];
	successCriteria: string[];
	repairChecks: Array<{ issueId: string; criterion: string }>;
	responsibilityBindings: Array<{ nodeId: string; stageId: string; phase: "stage" | "synthesis" }>;
	responsibilityTransfers: ResponsibilityTransfer[];
	dependencies: string[];
	scope: { workspaceRoot: string; allowedPaths: string[] };
	allowedTools: string[];
	writeAuthority: "workspace-write" | "none";
	budget: TaskBudget;
	reviewGateRequired: boolean;
	resumePolicy: "resume-session";
	searchBatchId?: string;
	searchCandidateId?: string;
	inputVersions: Array<{
		inputRef: string;
		evidenceId: string | null;
		versionHash: string | null;
		status: string | null;
		canonical?: {
			id: string;
			status: string;
			adoptedAt: string;
			evidenceSnapshotHash?: string;
			sourceSha256?: string;
			targetSha256?: string;
			materializationRef?: string;
		};
	}>;
}

export function semanticContractHash(contract: unknown): string {
	const stableValue = (value: unknown): string => {
		if (Array.isArray(value))
			return `[${value.map((entry) => (entry === undefined ? "null" : stableValue(entry))).join(",")}]`;
		if (value && typeof value === "object") {
			return `{${Object.entries(value)
				.filter(([, entry]) => entry !== undefined)
				.sort(([left], [right]) => left.localeCompare(right))
				.map(([key, entry]) => `${JSON.stringify(key)}:${stableValue(entry)}`)
				.join(",")}}`;
		}
		return JSON.stringify(value) ?? "undefined";
	};
	return createHash("sha256").update(stableValue(contract)).digest("hex");
}

export function sourceTaskContractHash(task: TaskPacket): string {
	return semanticContractHash({
		stageId: task.stageId,
		deliveryKind: task.deliveryKind,
		repairOfEvidenceId: task.repairOfEvidenceId,
		objective: task.objective,
		inputArtifactRefs: task.inputArtifactRefs,
		requiredCanonicalArtifacts: task.requiredCanonicalArtifacts,
		requiredOutputType: task.requiredOutputType,
		requiredOutputFields: task.requiredOutputFields,
		acceptanceChecks: task.acceptanceChecks,
		failureSignals: task.failureSignals,
		successCriteria: task.successCriteria,
		repairChecks: task.repairChecks ?? [],
		responsibilityBindings: task.responsibilityBindings ?? [],
		dependencies: task.dependencies,
		scope: task.scope,
		allowedTools: task.allowedTools,
		writeAuthority: task.writeAuthority,
		budget: task.budget,
		reviewGateRequired: task.reviewGateRequired,
		resumePolicy: task.resumePolicy,
	});
}

/** Per-call indexes; never retained across mutations or supervisor ticks. */
export function repairContext(snapshot: JobSnapshot) {
	const obligationsByLineage = new Map<string, Obligation[]>();
	const issueIds = new Set<string>();
	const findings = new Set<string>();
	for (const obligation of Object.values(snapshot.obligations)) {
		for (const item of obligation.items ?? []) issueIds.add(item.id);
		const review = snapshot.reviews[obligation.sourceReviewId];
		for (const finding of review?.findings ?? []) findings.add(finding);
		const evidence = review ? snapshot.evidence[review.evidenceId] : undefined;
		if (obligation.status !== "open" || !evidence) continue;
		const lineage = evidence.currentEvidenceSetId ?? evidence.taskId;
		const entries = obligationsByLineage.get(lineage) ?? [];
		entries.push(obligation);
		obligationsByLineage.set(lineage, entries);
	}
	return {
		obligationsByLineage,
		normalize(criterion: string): string {
			const plain = groupRepairCriteria([criterion], issueIds)[0].criterion;
			return !plain.startsWith("Verify that the reported issue is resolved:") && findings.has(plain)
				? `Verify that the reported issue is resolved: ${plain}`
				: plain;
		},
	};
}

export function backtrackChecksFromSnapshot(snapshot: JobSnapshot, stageId: string) {
	return snapshot.graph.unresolvedObjectionIds.flatMap((nodeId) => {
		const objection = snapshot.graph.nodes[nodeId];
		const route = objection.domainRef ? snapshot.routeDecisions[objection.domainRef] : undefined;
		if (
			objection.stageId !== stageId ||
			!(
				(route?.action === "backtrack" && route.targetStageId === stageId) ||
				objection.domainRef === snapshot.stages[stageId].lastRouteDecisionRef
			)
		)
			return [];
		return [{ nodeId, criterion: `[${nodeId}] Verify that the backtrack issue is resolved: ${objection.statement}` }];
	});
}

export function resolveRepairInputFromSnapshot(snapshot: JobSnapshot, ref: string): string {
	const retired =
		snapshot.retiredArtifacts[ref] ??
		Object.values(snapshot.retiredArtifacts).find((receipt) => receipt.evidenceId === ref);
	const artifact =
		snapshot.canonical[ref] ?? Object.values(snapshot.canonical).find((entry) => entry.evidenceId === ref);
	if (!retired && artifact?.status !== "stale") return ref;
	const taskId = retired?.taskId ?? (artifact ? snapshot.evidence[artifact.evidenceId]?.taskId : undefined);
	const stageId = taskId ? snapshot.tasks[taskId]?.stageId : undefined;
	const replacement = stageId ? snapshot.canonicalRoute.stageArtifactIds[stageId] : undefined;
	if (!replacement || snapshot.canonical[replacement]?.status !== "active")
		throw new EffectiveContractUnavailableError(
			`Repair input ${ref} requires a reviewed active replacement before retry`,
		);
	return replacement;
}

export function transferableResponsibilityCandidates(job: ResearchJob, stageId: string, obligationId?: string) {
	if (!obligationId) return [];
	return transferableResponsibilityCandidatesFromSnapshot(job.state, stageId, obligationId);
}

export function transferableResponsibilityCandidatesFromSnapshot(
	snapshot: JobSnapshot,
	stageId: string,
	obligationId?: string,
) {
	if (!obligationId) return [];
	const obligation = snapshot.obligations[obligationId];
	const review = obligation ? snapshot.reviews[obligation.sourceReviewId] : undefined;
	const evidence = review ? snapshot.evidence[review.evidenceId] : undefined;
	const source = evidence ? snapshot.tasks[evidence.taskId] : undefined;
	if (!obligation || obligation.status !== "open" || evidence?.stageId !== stageId || source?.status !== "succeeded")
		return [];
	const backtrackChecks = backtrackChecksFromSnapshot(snapshot, stageId);
	const candidates: Array<{
		sourceTaskId: string;
		sourceContractHash: string;
		sourceField: "acceptanceChecks";
		sourceIndex: number;
		exactCriterion: string;
		nodeId: string | null;
		issueId: string | null;
		destinationStageId: string;
		destinationPhase: "synthesis";
	}> = [];
	for (const check of backtrackChecks) {
		const sourceIndex = source.acceptanceChecks.indexOf(check.criterion);
		if (sourceIndex < 0) continue;
		candidates.push({
			sourceTaskId: source.id,
			sourceContractHash: sourceTaskContractHash(source),
			sourceField: "acceptanceChecks",
			sourceIndex,
			exactCriterion: check.criterion,
			nodeId: check.nodeId,
			issueId: null,
			destinationStageId: stageId,
			destinationPhase: "synthesis",
		});
	}
	for (const item of obligation.items ?? []) {
		if (item.status !== "open") continue;
		const sourceIndex = source.acceptanceChecks.indexOf(item.criterion);
		if (sourceIndex < 0) continue;
		candidates.push({
			sourceTaskId: source.id,
			sourceContractHash: sourceTaskContractHash(source),
			sourceField: "acceptanceChecks",
			sourceIndex,
			exactCriterion: item.criterion,
			nodeId: null,
			issueId: item.id,
			destinationStageId: stageId,
			destinationPhase: "synthesis",
		});
	}
	return candidates;
}

function openLineageObligations(snapshot: JobSnapshot, evidenceId: string, context = repairContext(snapshot)) {
	const evidence = snapshot.evidence[evidenceId];
	return evidence ? (context.obligationsByLineage.get(evidence.currentEvidenceSetId ?? evidence.taskId) ?? []) : [];
}

function inputVersions(snapshot: JobSnapshot, refs: string[]): EffectiveTaskContract["inputVersions"] {
	return refs.map((inputRef) => {
		const canonical = snapshot.canonical[inputRef];
		const evidence = snapshot.evidence[canonical?.evidenceId ?? inputRef];
		return {
			inputRef,
			evidenceId: evidence?.id ?? null,
			versionHash: evidence?.versionHash ?? canonical?.evidenceSnapshotHash ?? null,
			status: canonical?.status ?? evidence?.status ?? null,
			...(canonical
				? {
						canonical: {
							id: canonical.id,
							status: canonical.status,
							adoptedAt: canonical.adoptedAt,
							...(canonical.evidenceSnapshotHash
								? { evidenceSnapshotHash: canonical.evidenceSnapshotHash }
								: {}),
							...(canonical.sourceSha256 ? { sourceSha256: canonical.sourceSha256 } : {}),
							...(canonical.targetSha256 ? { targetSha256: canonical.targetSha256 } : {}),
							...(canonical.materializationRef ? { materializationRef: canonical.materializationRef } : {}),
						},
					}
				: {}),
		};
	});
}

export function buildEffectiveTaskContract(
	job: ResearchJob,
	plan: StagePlanManifest,
	planned: PlannedTask,
): EffectiveTaskContract {
	return buildEffectiveTaskContractFromSnapshot(job.state, job.definitions, plan, planned);
}

export function buildEffectiveTaskContractFromSnapshot(
	snapshot: JobSnapshot,
	definitions: Record<string, StageDefinition>,
	plan: StagePlanManifest,
	planned: PlannedTask,
	context = repairContext(snapshot),
): EffectiveTaskContract {
	const definition = definitions[plan.stageId];
	if (!definition) throw new Error(`unknown stage ${plan.stageId}`);
	const obligation = plan.obligationId ? snapshot.obligations[plan.obligationId] : undefined;
	const failedReview = obligation ? snapshot.reviews[obligation.sourceReviewId] : undefined;
	const failedEvidence = failedReview ? snapshot.evidence[failedReview.evidenceId] : undefined;
	const sourceTask = failedEvidence
		? snapshot.tasks[failedEvidence.taskId]
		: planned.responsibilityTransfers?.[0]
			? snapshot.tasks[planned.responsibilityTransfers[0].sourceTaskId]
			: undefined;
	const deliveryKind = planned.deliveryKind ?? "stage";
	const transferProposals = planned.responsibilityTransfers ?? [];
	const inheritedTransfers =
		failedEvidence && sourceTask?.deliveryKind === "local" && deliveryKind === "local"
			? (sourceTask.responsibilityTransfers ?? [])
			: [];
	const transferredIssueIds = new Set(
		[...transferProposals, ...inheritedTransfers].flatMap((transfer) => (transfer.issueId ? [transfer.issueId] : [])),
	);
	const repairChecks = failedEvidence
		? openLineageObligations(snapshot, failedEvidence.id, context).flatMap((item) =>
				(item.items ?? [])
					.filter((check) => check.status === "open" && !transferredIssueIds.has(check.id))
					.map((check) => ({ issueId: check.id, criterion: context.normalize(check.criterion) })),
			)
		: [];
	const ownTransfers: ResponsibilityTransfer[] = transferProposals.map((transfer, index) => {
		const sourceEvidence = sourceTask
			? Object.values(snapshot.evidence).find((evidence) => evidence.taskId === sourceTask.id)
			: undefined;
		const nodeBound = transfer.nodeId
			? backtrackChecksFromSnapshot(snapshot, plan.stageId).some(
					(check) => check.nodeId === transfer.nodeId && check.criterion === transfer.exactCriterion,
				) &&
				(!sourceTask?.responsibilityBindings?.length ||
					sourceTask.responsibilityBindings.some(
						(binding) => binding.nodeId === transfer.nodeId && binding.stageId === plan.stageId,
					))
			: false;
		const issueBound = transfer.issueId
			? Object.values(snapshot.obligations).some((obligation) =>
					(obligation.items ?? []).some(
						(item) =>
							item.id === transfer.issueId &&
							item.status === "open" &&
							item.criterion === transfer.exactCriterion &&
							snapshot.evidence[snapshot.reviews[obligation.sourceReviewId]?.evidenceId ?? ""]?.taskId ===
								sourceTask?.id,
					),
				)
			: false;
		if (
			deliveryKind !== "local" ||
			!sourceTask ||
			!sourceEvidence ||
			transfer.sourceTaskId !== sourceTask.id ||
			transfer.sourceContractHash !== sourceTaskContractHash(sourceTask) ||
			transfer.sourceField !== "acceptanceChecks" ||
			!Number.isInteger(transfer.sourceIndex) ||
			sourceTask.acceptanceChecks[transfer.sourceIndex] !== transfer.exactCriterion ||
			transfer.destinationStageId !== plan.stageId ||
			transfer.destinationPhase !== "synthesis" ||
			!transfer.rationale.trim() ||
			Boolean(transfer.nodeId) === Boolean(transfer.issueId) ||
			!(nodeBound || issueBound)
		)
			throw new EffectiveContractUnavailableError(
				"responsibility transfer must match one exact bound source criterion",
			);
		return { ...transfer, id: `${plan.id}:${planned.key}:transfer:${index}`, planId: plan.id };
	});
	const carriedTransfers =
		deliveryKind === "synthesis"
			? planned.inputArtifactRefs.flatMap((ref) => {
					const evidence = snapshot.evidence[ref];
					const task = evidence ? snapshot.tasks[evidence.taskId] : undefined;
					return evidence?.status === "accepted" && task?.deliveryKind === "local"
						? (task.responsibilityTransfers ?? [])
						: [];
				})
			: [];
	const responsibilityTransfers = [...ownTransfers, ...inheritedTransfers, ...carriedTransfers];
	const transferredCriteria = new Set(
		[...ownTransfers, ...inheritedTransfers].map((transfer) => transfer.exactCriterion),
	);
	const completeRepairChecks = [
		...repairChecks,
		...carriedTransfers.flatMap((transfer) =>
			transfer.issueId ? [{ issueId: transfer.issueId, criterion: transfer.exactCriterion }] : [],
		),
	];
	const responsible = planned.responsibilityBindings;
	const legacyBindings = responsible === undefined || (responsible.length === 0 && deliveryKind !== "local");
	const assignedNodeIds = new Set(responsible?.map((binding) => binding.nodeId) ?? []);
	const transferredNodeIds = new Set(
		[...ownTransfers, ...inheritedTransfers].flatMap((transfer) => (transfer.nodeId ? [transfer.nodeId] : [])),
	);
	const backtrack =
		legacyBindings || deliveryKind !== "local"
			? backtrackChecksFromSnapshot(snapshot, plan.stageId).filter(
					(check) =>
						!transferredNodeIds.has(check.nodeId) && (legacyBindings || assignedNodeIds.has(check.nodeId)),
				)
			: [];
	const responsibilityBindings = [
		...(responsible && (responsible.length > 0 || deliveryKind === "local")
			? responsible
			: deliveryKind === "local"
				? []
				: backtrack.map(({ nodeId }) => ({
						nodeId,
						stageId: plan.stageId,
						phase: deliveryKind === "synthesis" ? ("synthesis" as const) : ("stage" as const),
					}))),
		...carriedTransfers.flatMap((transfer) =>
			transfer.nodeId
				? [{ nodeId: transfer.nodeId, stageId: transfer.destinationStageId, phase: "synthesis" as const }]
				: [],
		),
	];
	const inputs = [
		...new Set([
			...planned.inputArtifactRefs,
			...(failedEvidence ? [failedEvidence.id] : []),
			...(failedEvidence && sourceTask
				? sourceTask.inputArtifactRefs.map((ref) => resolveRepairInputFromSnapshot(snapshot, ref))
				: []),
			...(plan.stageId === "research-review" ? Object.values(snapshot.canonicalRoute.stageArtifactIds) : []),
		]),
	];
	const batch = Object.values(snapshot.searchBatches).find((candidate) => candidate.planId === plan.id);
	const searchCandidate = batch
		? Object.values(batch.candidates).find((candidate) => candidate.key === planned.key)
		: undefined;
	const acceptanceChecks = [
		...(deliveryKind === "local" ? [] : definition.acceptanceChecks),
		...planned.acceptanceChecks,
		...(sourceTask
			? sourceTask.acceptanceChecks.filter(
					(_criterion, index) =>
						!ownTransfers.some(
							(transfer) => transfer.sourceField === "acceptanceChecks" && transfer.sourceIndex === index,
						),
				)
			: []),
		...(failedReview?.findings ?? []).filter((finding) => !transferredCriteria.has(finding)),
		...completeRepairChecks.map((check) => check.criterion),
		...backtrack.map((check) => check.criterion),
		...carriedTransfers.map((transfer) => transfer.exactCriterion),
		...(batch?.criteria ?? []),
	].map((criterion) => (failedEvidence ? context.normalize(criterion) : criterion));
	const successCriteria = [
		...planned.successCriteria,
		...(sourceTask?.successCriteria ?? []).filter((criterion) => !transferredCriteria.has(criterion)),
	].map((criterion) => (failedEvidence ? context.normalize(criterion) : criterion));
	return {
		planId: plan.id,
		stageId: plan.stageId,
		stageExecutionId: snapshot.stages[plan.stageId].executionId ?? `stage_exec_${plan.stageId}`,
		stageRevision: snapshot.stages[plan.stageId]?.revision ?? 1,
		role: "worker",
		deliveryKind,
		...(failedEvidence ? { repairOfEvidenceId: failedEvidence.id } : {}),
		objective: planned.objective,
		inputArtifactRefs: inputs,
		requiredCanonicalArtifacts: inputs.filter((ref) => snapshot.canonical[ref]?.status === "active"),
		requiredOutputType:
			deliveryKind === "local" ? `${definition.outputArtifactType}:local` : definition.outputArtifactType,
		requiredOutputFields: [
			...new Set([...planned.requiredOutputFields, ...(sourceTask?.requiredOutputFields ?? [])]),
		],
		acceptanceChecks: [...new Set(acceptanceChecks)],
		failureSignals: [
			...new Set([
				...(deliveryKind === "local" ? [] : definition.failureSignals),
				...planned.failureSignals,
				...(sourceTask?.failureSignals ?? []),
			]),
		],
		successCriteria: [...new Set(successCriteria)],
		repairChecks: completeRepairChecks,
		responsibilityTransfers,
		responsibilityBindings: [...new Map(responsibilityBindings.map((binding) => [binding.nodeId, binding])).values()],
		dependencies: [],
		scope: { workspaceRoot: snapshot.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: definition.workerTools,
		writeAuthority: definition.workspaceWrite ? "workspace-write" : "none",
		budget: definition.workerBudget
			? { ...definition.workerBudget }
			: plan.stageId === "research-review"
				? { maxTurns: 12, maxToolCalls: 32, maxRuntimeMs: 300_000 }
				: definition.workspaceWrite
					? { maxTurns: 16, maxToolCalls: 32, maxRuntimeMs: 300_000 }
					: { maxTurns: 8, maxToolCalls: 16, maxRuntimeMs: 300_000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		...(batch && searchCandidate ? { searchBatchId: batch.id, searchCandidateId: searchCandidate.id } : {}),
		inputVersions: inputVersions(snapshot, inputs),
	};
}

export function searchBatchForPlan(
	snapshot: JobSnapshot,
	definition: StageDefinition,
	plan: StagePlanManifest,
): SearchBatch {
	const existing = Object.values(snapshot.searchBatches).find((batch) => batch.planId === plan.id);
	if (existing) return existing;
	const policy = definition.searchPolicy ?? {
		strategy: "diverse-candidates" as const,
		minCandidates: 2,
		maxCandidates: 4,
		criteria: definition.acceptanceChecks,
	};
	const latest = Object.values(snapshot.searchBatches)
		.filter(
			(batch) =>
				batch.stageId === plan.stageId &&
				(batch.stageRevision ?? 1) === (snapshot.stages[plan.stageId].revision ?? 1),
		)
		.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
	const previous = latest && ["exhausted", "superseded"].includes(latest.status) ? latest : undefined;
	const maxRounds = previous?.maxRounds ?? policy.maxRounds ?? 2;
	const round = previous ? previous.round + 1 : 1;
	if (!Number.isInteger(maxRounds) || maxRounds <= 0) throw new Error("search maxRounds must be a positive integer");
	if (round > maxRounds) throw new Error(`search round ${round} exceeds the maximum of ${maxRounds}`);
	const id = `search_${semanticContractHash({ jobId: plan.jobId, planId: plan.id }).slice(0, 24)}`;
	return {
		id,
		stageId: plan.stageId,
		planId: plan.id,
		round,
		maxRounds,
		...(previous ? { previousBatchId: previous.id } : {}),
		objective: `Search alternatives for ${definition.label}`,
		strategy: policy.strategy,
		status: "planning",
		minCandidates: policy.minCandidates,
		maxCandidates: policy.maxCandidates,
		criteria: policy.criteria,
		candidates: Object.fromEntries(
			plan.tasks.map((task) => {
				const candidateId = `candidate_${semanticContractHash({ batchId: id, key: task.key }).slice(0, 24)}`;
				return [
					candidateId,
					{
						id: candidateId,
						key: task.key,
						hypothesis: task.hypothesis ?? task.objective,
						status: "planned" as const,
						graphNodeId: `hypothesis_${semanticContractHash({ batchId: id, key: task.key }).slice(0, 24)}`,
					},
				];
			}),
		),
		createdAt: plan.createdAt,
		updatedAt: plan.createdAt,
	};
}

/** Only inputs actually available to the planner, excluding pause/budget bookkeeping. */
export function planGenerationBasisHash(
	snapshot: JobSnapshot,
	definitions: Record<string, StageDefinition>,
	plan: StagePlanManifest,
): string {
	const batch = plan.mode === "search" ? searchBatchForPlan(snapshot, definitions[plan.stageId], plan) : undefined;
	const basis = batch ? { ...snapshot, searchBatches: { ...snapshot.searchBatches, [batch.id]: batch } } : snapshot;
	const context = repairContext(basis);
	return semanticContractHash({
		mission: {
			objective: basis.frame.objective,
			boundaries: basis.frame.boundaries,
			acceptance: basis.frame.acceptance,
			requiredArtifactTypes: basis.frame.requiredArtifactTypes,
			permissions: basis.frame.permissions,
		},
		activeStageId: basis.frame.activeStageId,
		stage: {
			definition: definitions[plan.stageId],
			revision: basis.stages[plan.stageId]?.revision ?? 1,
			executionId: basis.stages[plan.stageId]?.executionId,
		},
		guidance: Object.values(basis.graph.nodes)
			.filter((node) => node.actor === "user")
			.sort((left, right) => left.id.localeCompare(right.id)),
		canonicalInputs: Object.values(basis.canonical)
			.map((artifact) => ({
				id: artifact.id,
				evidenceId: artifact.evidenceId,
				status: artifact.status,
				checksum: artifact.checksum,
				evidenceSnapshotHash: artifact.evidenceSnapshotHash,
			}))
			.sort((left, right) => left.id.localeCompare(right.id)),
		localInputs: Object.values(basis.evidence)
			.filter(
				(evidence) =>
					evidence.stageId === plan.stageId &&
					evidence.status === "accepted" &&
					basis.tasks[evidence.taskId]?.deliveryKind === "local",
			)
			.map((evidence) => ({ id: evidence.id, versionHash: evidence.versionHash })),
		contracts: plan.tasks.map((planned) =>
			buildEffectiveTaskContractFromSnapshot(basis, definitions, plan, planned, context),
		),
	});
}

export function taskContractMatches(task: TaskPacket, contract: EffectiveTaskContract): boolean {
	return (
		task.planId === contract.planId &&
		task.stageRevision === contract.stageRevision &&
		task.effectiveContractHash === semanticContractHash(contract) &&
		task.stageId === contract.stageId &&
		task.stageExecutionId === contract.stageExecutionId &&
		task.role === contract.role &&
		task.deliveryKind === contract.deliveryKind &&
		task.repairOfEvidenceId === contract.repairOfEvidenceId &&
		task.objective === contract.objective &&
		JSON.stringify(task.inputArtifactRefs) === JSON.stringify(contract.inputArtifactRefs) &&
		JSON.stringify(task.requiredCanonicalArtifacts) === JSON.stringify(contract.requiredCanonicalArtifacts) &&
		task.requiredOutputType === contract.requiredOutputType &&
		JSON.stringify(task.requiredOutputFields) === JSON.stringify(contract.requiredOutputFields) &&
		JSON.stringify(task.acceptanceChecks) === JSON.stringify(contract.acceptanceChecks) &&
		JSON.stringify(task.failureSignals) === JSON.stringify(contract.failureSignals) &&
		JSON.stringify(task.successCriteria) === JSON.stringify(contract.successCriteria) &&
		JSON.stringify(task.repairChecks ?? []) === JSON.stringify(contract.repairChecks) &&
		JSON.stringify(task.responsibilityBindings ?? []) === JSON.stringify(contract.responsibilityBindings) &&
		JSON.stringify(task.responsibilityTransfers ?? []) === JSON.stringify(contract.responsibilityTransfers) &&
		JSON.stringify(task.dependencies) === JSON.stringify(contract.dependencies) &&
		JSON.stringify(task.scope) === JSON.stringify(contract.scope) &&
		JSON.stringify(task.allowedTools) === JSON.stringify(contract.allowedTools) &&
		task.writeAuthority === contract.writeAuthority &&
		JSON.stringify(task.budget) === JSON.stringify(contract.budget) &&
		task.reviewGateRequired === contract.reviewGateRequired &&
		task.resumePolicy === contract.resumePolicy &&
		task.searchBatchId === contract.searchBatchId &&
		task.searchCandidateId === contract.searchCandidateId
	);
}

export function obligationsForPlan(job: ResearchJob, plan: StagePlanManifest): Obligation[] {
	const snapshot = job.state;
	const obligation = plan.obligationId ? snapshot.obligations[plan.obligationId] : undefined;
	const evidenceId = obligation ? snapshot.reviews[obligation.sourceReviewId]?.evidenceId : undefined;
	return evidenceId ? openLineageObligations(snapshot, evidenceId) : [];
}
