import { createHash } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it } from "vitest";
import {
	buildEffectiveTaskContract,
	EffectiveContractUnavailableError,
	semanticContractHash,
} from "../src/effective-contract.ts";
import {
	evidenceHasCurrentPlanApproval,
	planEvidence,
	planReviewStatus,
	preparePlanEvidence,
} from "../src/plan-review.ts";
import { researchMilestones } from "../src/progress.ts";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { PlannedTask, StagePlanManifest, TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true }))));

function stable(value: unknown): string {
	if (Array.isArray(value))
		return `[${value.map((entry) => (entry === undefined ? "null" : stable(entry))).join(",")}]`;
	if (value && typeof value === "object")
		return `{${Object.entries(value)
			.filter(([, entry]) => entry !== undefined)
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([key, entry]) => `${JSON.stringify(key)}:${stable(entry)}`)
			.join(",")}}`;
	return JSON.stringify(value) ?? "undefined";
}

function legacyTaskHash(task: TaskPacket): string {
	return createHash("sha256")
		.update(
			stable({
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
			}),
		)
		.digest("hex");
}

function plan(
	job: ResearchJob,
	id: string,
	stageId: string,
	planned: PlannedTask,
	obligationId?: string,
): StagePlanManifest {
	return {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id,
		jobId: job.state.frame.jobId,
		stageId,
		decisionRef: id,
		mode: obligationId ? "repair" : "decompose",
		tasks: [planned],
		rationale: "Transfer one exact legacy responsibility after independent plan review.",
		sessionRef: "fixture:main",
		createdAt: new Date().toISOString(),
		...(obligationId ? { obligationId } : {}),
	};
}

async function approvePlan(job: ResearchJob, value: StagePlanManifest): Promise<void> {
	await job.recordStagePlan(value);
	const evidence = await preparePlanEvidence(job, value);
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	expect(planReviewStatus(job, value.id)).toBe("passed");
}

it.each([false, true])(
	"reviews an exact legacy transfer, keeps it open through local acceptance, then closes it only after synthesis review (local repair: %s)",
	async (repairLocal) => {
		const root = await mkdtemp(join(tmpdir(), "astra-responsibility-transfer-"));
		roots.push(root);
		const literature = DEFAULT_STAGES.find((stage) => stage.id === "literature");
		if (!literature) throw new Error("literature stage definition missing");
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			objective: "Migrate an old mixed contract",
			workspaceRoot: root,
			definitions: [literature],
		});
		await job.reopenStage(
			"literature",
			"backtrack-route",
			"A method responsibility must be resolved before advancing.",
		);
		const nodeId = job.state.graph.unresolvedObjectionIds[0]!;
		const criterion = job.backtrackChecks("literature").find((check) => check.nodeId === nodeId)!.criterion;
		const sourceTask = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			deliveryKind: "stage",
			objective: "Deliver the legacy resource inventory and method comparison",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature",
			requiredOutputFields: literature.requiredOutputFields,
			acceptanceChecks: [criterion, "Record source limitations"],
			failureSignals: ["No source limitations"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["Inventory is traceable"],
		});
		expect(sourceTask.responsibilityBindings).toBeUndefined();
		await job.setTaskStatus(sourceTask.id, "succeeded");
		const sourceEvidence = await job.recordEvidence({
			taskId: sourceTask.id,
			stageId: "literature",
			type: "literature",
			content: Object.fromEntries(literature.requiredOutputFields.map((field) => [field, field])),
			refs: [],
			currentEvidenceSetId: "legacy-literature-lineage",
		});
		await job.recordReview(
			reviewFixture(job, {
				evidenceId: sourceEvidence.id,
				verdict: "fail",
				score: 0.5,
				findings: ["Record source limitations"],
				criteria: [
					{
						criterion,
						passed: true,
						score: 1,
						evidenceRefs: [sourceEvidence.id],
						rationale: "The previous resource comparison remains intact.",
					},
					{
						criterion: "Record source limitations",
						passed: false,
						score: 0,
						evidenceRefs: [sourceEvidence.id],
						rationale: "Needs a separate local repair.",
					},
					{
						criterion: "Inventory is traceable",
						passed: true,
						score: 1,
						evidenceRefs: [sourceEvidence.id],
						rationale: "The inventory points to its recorded sources.",
					},
				],
			}),
		);
		const obligationId = job.state.frame.openObligationIds[0]!;
		const transfer = {
			sourceTaskId: sourceTask.id,
			sourceContractHash: legacyTaskHash(sourceTask),
			sourceField: "acceptanceChecks" as const,
			sourceIndex: 0,
			exactCriterion: criterion,
			nodeId,
			destinationStageId: "literature",
			destinationPhase: "synthesis" as const,
			rationale: "Keep legacy method comparison in the complete synthesis delivery.",
		};
		const wrongNodeTransferPlan = plan(
			job,
			"plan-wrong-node-transfer",
			"literature",
			{
				key: "wrong-node-transfer",
				deliveryKind: "local",
				objective: "Try to transfer an unrelated source requirement",
				inputArtifactRefs: [sourceEvidence.id],
				requiredOutputFields: ["limitations"],
				acceptanceChecks: ["Limitations are explicit"],
				failureSignals: ["Limitations are missing"],
				successCriteria: ["Limitations are explicit"],
				responsibilityTransfers: [
					{
						...transfer,
						sourceIndex: 1,
						exactCriterion: "Record source limitations",
					},
				],
			} as unknown as PlannedTask,
			obligationId,
		);
		await expect(job.recordStagePlan(wrongNodeTransferPlan)).rejects.toThrow(
			/does not match a bound source requirement/,
		);
		const localPlan = plan(
			job,
			"plan-local-transfer",
			"literature",
			{
				key: "local-source-repair",
				deliveryKind: "local",
				objective: "Record source limitations in a focused local delivery",
				inputArtifactRefs: [sourceEvidence.id],
				requiredOutputFields: ["limitations"],
				acceptanceChecks: ["Limitations are explicit"],
				failureSignals: ["Limitations are missing"],
				successCriteria: ["Limitations are explicit"],
				responsibilityTransfers: [transfer],
			} as unknown as PlannedTask,
			obligationId,
		);
		await approvePlan(job, localPlan);
		const localContract = buildEffectiveTaskContract(
			job,
			job.state.stagePlans[localPlan.id],
			job.state.stagePlans[localPlan.id].tasks[0]!,
		);
		expect(localContract.acceptanceChecks).not.toContain(criterion);
		expect(localContract.responsibilityTransfers).toHaveLength(1);
		const localTask = await job.dispatchTask({
			...localContract,
			effectiveContractHash: semanticContractHash(localContract),
			replayKey: `stage-plan:${localPlan.id}:local-source-repair`,
		});
		await job.setTaskStatus(localTask.id, "succeeded");
		let localEvidence = await job.recordEvidence({
			taskId: localTask.id,
			stageId: "literature",
			type: localTask.requiredOutputType,
			content: { limitations: ["Full text was unavailable"] },
			refs: [],
			currentEvidenceSetId: sourceEvidence.currentEvidenceSetId,
		});
		if (repairLocal) {
			for (const repairRound of [1, 2]) {
				await job.recordReview(
					reviewFixture(job, {
						evidenceId: localEvidence.id,
						verdict: "fail",
						findings: ["Repair source limitations"],
					}),
				);
				const repairObligation = Object.values(job.state.obligations).find(
					(item) => item.evidenceId === localEvidence.id,
				)!;
				const repairPlan = plan(
					job,
					`plan-node-repair-local-${repairRound}`,
					"literature",
					{
						...localPlan.tasks[0]!,
						inputArtifactRefs: [localEvidence.id],
						responsibilityTransfers: [],
					},
					repairObligation.id,
				);
				await approvePlan(job, repairPlan);
				const repairContract = buildEffectiveTaskContract(job, repairPlan, repairPlan.tasks[0]!);
				expect(repairContract.responsibilityTransfers).toEqual(localTask.responsibilityTransfers);
				expect(repairContract.acceptanceChecks).not.toContain(criterion);
				const repairTask = await job.dispatchTask({
					...repairContract,
					effectiveContractHash: semanticContractHash(repairContract),
					replayKey: `stage-plan:${repairPlan.id}:${repairPlan.tasks[0]!.key}`,
				});
				await job.pause("check inherited node handoff persistence");
				await job.reload();
				await job.resume();
				expect(job.state.tasks[repairTask.id].responsibilityTransfers).toEqual(localTask.responsibilityTransfers);
				await job.setTaskStatus(repairTask.id, "succeeded");
				localEvidence = await job.recordEvidence({
					taskId: repairTask.id,
					stageId: "literature",
					type: repairTask.requiredOutputType,
					content: { limitations: ["Repaired source limitations"] },
					refs: [],
					currentEvidenceSetId: sourceEvidence.currentEvidenceSetId,
				});
			}
		}
		await job.recordReview(reviewFixture(job, { evidenceId: localEvidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(localEvidence.id, true, "accept-local-transfer");
		expect(job.state.graph.nodes[nodeId]?.status).toBe("open");
		await expect(
			job.applyRouteDecision({
				schemaVersion: "astra.main_agent_decision_manifest.v1",
				manifestId: "route-before-synthesis",
				jobId: job.state.frame.jobId,
				decisionType: "route",
				decisionRef: "route-before-synthesis",
				stageId: "literature",
				routeAction: "advance",
				targetStageId: "literature",
				evidenceRefs: [],
				rationale: "Try to advance before transferring responsibility.",
				sessionRef: "fixture:main",
				createdAt: new Date().toISOString(),
			}),
		).rejects.toThrow(/requires synthesis of accepted local evidence|bound responsibility nodes remain open/);
		const frozenLocalPlan = structuredClone(planEvidence(job, localPlan.id)!);
		const synthesisPlan = plan(job, "plan-synthesis-transfer", "literature", {
			key: "complete-synthesis",
			deliveryKind: "synthesis",
			objective: "Complete the literature synthesis and method comparison",
			inputArtifactRefs: [localEvidence.id],
			requiredOutputFields: literature.requiredOutputFields,
			acceptanceChecks: [],
			failureSignals: [],
			successCriteria: [],
			responsibilityTransfers: [],
		});
		await approvePlan(job, synthesisPlan);
		const synthesisContract = buildEffectiveTaskContract(
			job,
			job.state.stagePlans[synthesisPlan.id],
			job.state.stagePlans[synthesisPlan.id].tasks[0]!,
		);
		expect(synthesisContract.acceptanceChecks).toContain(criterion);
		const synthesisTask = await job.dispatchTask({
			...synthesisContract,
			effectiveContractHash: semanticContractHash(synthesisContract),
			replayKey: `stage-plan:${synthesisPlan.id}:complete-synthesis`,
		});
		await job.setTaskStatus(synthesisTask.id, "succeeded");
		const synthesisEvidence = await job.recordEvidence({
			taskId: synthesisTask.id,
			stageId: "literature",
			type: synthesisTask.requiredOutputType,
			content: Object.fromEntries(literature.requiredOutputFields.map((field) => [field, field])),
			refs: [],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: synthesisEvidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(synthesisEvidence.id, true, "accept-synthesis-transfer");
		await job.adoptEvidence(synthesisEvidence.id);
		expect(job.state.graph.nodes[nodeId]?.status).toBe("resolved");
		expect(
			researchMilestones(job.state)
				.flatMap((stage) => stage.plans)
				.find((entry) => entry.id === localPlan.id)?.status,
		).toBe("stale");
		expect(planReviewStatus(job, localPlan.id)).toBe("stale");
		expect(planEvidence(job, localPlan.id)).toEqual(frozenLocalPlan);
		expect(evidenceHasCurrentPlanApproval(job, localEvidence)).toBe(true);
		expect(() => buildEffectiveTaskContract(job, localPlan, localPlan.tasks[0]!)).toThrow(
			EffectiveContractUnavailableError,
		);
		await expect(
			job.dispatchTask({
				...localContract,
				id: "closed-transfer-redispatch",
				effectiveContractHash: semanticContractHash(localContract),
				replayKey: "closed-transfer-redispatch",
			}),
		).rejects.toThrow("worker dispatch requires an independently approved plan");
	},
);

it.each([false, true])(
	"transfers an exact issue-bound legacy criterion through local acceptance and synthesis (local repair: %s)",
	async (repairLocal) => {
		const root = await mkdtemp(join(tmpdir(), "astra-issue-transfer-"));
		roots.push(root);
		const store = new MemoryAstraStore();
		const literature = DEFAULT_STAGES.find((stage) => stage.id === "literature");
		if (!literature) throw new Error("literature stage definition missing");
		const job = await ResearchJob.create(store, {
			objective: "Migrate an old issue-bound requirement",
			workspaceRoot: root,
			definitions: [literature],
		});
		const sourceCriterion = "Record source limitations";
		const sourceTask = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			deliveryKind: "stage",
			objective: "Deliver the legacy resource inventory and limitations",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature",
			requiredOutputFields: literature.requiredOutputFields,
			acceptanceChecks: [sourceCriterion, "Record query limitations"],
			failureSignals: ["Limitations are missing"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["Inventory is traceable"],
		});
		expect(sourceTask.responsibilityBindings).toBeUndefined();
		await job.setTaskStatus(sourceTask.id, "succeeded");
		const sourceEvidence = await job.recordEvidence({
			taskId: sourceTask.id,
			stageId: "literature",
			type: "literature",
			content: Object.fromEntries(literature.requiredOutputFields.map((field) => [field, field])),
			refs: [],
			currentEvidenceSetId: "legacy-issue-lineage",
		});
		const sourceObjection = sourceTask.acceptanceChecks.find((criterion) => criterion === sourceCriterion)!;
		await job.recordReview(
			reviewFixture(job, {
				evidenceId: sourceEvidence.id,
				verdict: "fail",
				score: 0.5,
				findings: [sourceCriterion],
				criteria: [...new Set([...sourceTask.acceptanceChecks, ...sourceTask.successCriteria])].map(
					(criterion) => ({
						criterion,
						passed: criterion !== sourceCriterion,
						score: criterion === sourceCriterion ? 0 : 1,
						evidenceRefs: [sourceEvidence.id],
						rationale:
							criterion === sourceCriterion
								? "The source limitations were omitted."
								: "Verified in the legacy delivery.",
					}),
				),
			}),
		);
		const obligationId = job.state.frame.openObligationIds[0]!;
		const issueId = job.state.obligations[obligationId]!.items!.find(
			(item) => item.criterion === sourceObjection,
		)!.id;
		const transfer = {
			sourceTaskId: sourceTask.id,
			sourceContractHash: legacyTaskHash(sourceTask),
			sourceField: "acceptanceChecks" as const,
			sourceIndex: sourceTask.acceptanceChecks.indexOf(sourceObjection),
			exactCriterion: sourceObjection,
			issueId,
			destinationStageId: "literature",
			destinationPhase: "synthesis" as const,
			rationale: "Keep this exact legacy review issue for complete synthesis.",
		};
		const invalidTransferTask = (responsibilityTransfers: (typeof transfer)[], key: string) =>
			({
				key,
				deliveryKind: "local" as const,
				objective: "Reject an invalid responsibility transfer",
				inputArtifactRefs: [sourceEvidence.id],
				requiredOutputFields: ["limitations"],
				acceptanceChecks: [],
				failureSignals: [],
				successCriteria: [],
				responsibilityTransfers,
			}) as unknown as PlannedTask;
		await expect(
			job.recordStagePlan(
				plan(
					job,
					"plan-wrong-source-hash",
					"literature",
					invalidTransferTask([{ ...transfer, sourceContractHash: "0".repeat(64) }], "wrong-hash"),
					obligationId,
				),
			),
		).rejects.toThrow(/does not match a bound source requirement/);
		await expect(
			job.recordStagePlan(
				plan(
					job,
					"plan-duplicate-transfer",
					"literature",
					invalidTransferTask([transfer, transfer], "duplicate-transfer"),
					obligationId,
				),
			),
		).rejects.toThrow(/duplicates a responsibility transfer/);
		await expect(
			job.recordStagePlan(
				plan(
					job,
					"plan-cross-stage-transfer",
					"literature",
					invalidTransferTask([{ ...transfer, destinationStageId: "idea" }], "cross-stage-transfer"),
					obligationId,
				),
			),
		).rejects.toThrow(/does not match a bound source requirement/);
		const localPlan = plan(
			job,
			"plan-local-issue-transfer",
			"literature",
			{
				key: "local-query-repair",
				deliveryKind: "local",
				objective: "Record query limitations in a focused delivery",
				inputArtifactRefs: [sourceEvidence.id],
				requiredOutputFields: ["limitations"],
				acceptanceChecks: ["Query limitations are explicit"],
				failureSignals: ["Query limitations are missing"],
				successCriteria: ["Query limitations are explicit"],
				responsibilityTransfers: [transfer],
			} as unknown as PlannedTask,
			obligationId,
		);
		await approvePlan(job, localPlan);
		await job.reload();
		expect(planReviewStatus(job, localPlan.id)).toBe("passed");
		const localContract = buildEffectiveTaskContract(
			job,
			job.state.stagePlans[localPlan.id],
			job.state.stagePlans[localPlan.id]!.tasks[0]!,
		);
		expect(localContract.acceptanceChecks).not.toContain(sourceCriterion);
		expect(localContract.repairChecks.some((check) => check.issueId === issueId)).toBe(false);
		const localTask = await job.dispatchTask({
			...localContract,
			effectiveContractHash: semanticContractHash(localContract),
			replayKey: `stage-plan:${localPlan.id}:local-query-repair`,
		});
		expect(localTask.responsibilityTransfers?.[0]?.sourceContractHash).toBe(transfer.sourceContractHash);
		await job.setTaskStatus(localTask.id, "succeeded");
		let localEvidence = await job.recordEvidence({
			taskId: localTask.id,
			stageId: "literature",
			type: localTask.requiredOutputType,
			content: { limitations: ["The query scope is recorded"] },
			refs: [],
			currentEvidenceSetId: sourceEvidence.currentEvidenceSetId,
		});
		if (repairLocal) {
			for (const repairRound of [1, 2]) {
				await job.recordReview(
					reviewFixture(job, {
						evidenceId: localEvidence.id,
						verdict: "fail",
						findings: ["Repair query limitations"],
					}),
				);
				const repairObligation = Object.values(job.state.obligations).find(
					(item) => item.evidenceId === localEvidence.id,
				)!;
				const repairPlan = plan(
					job,
					`plan-repair-local-${repairRound}`,
					"literature",
					{
						...localPlan.tasks[0]!,
						inputArtifactRefs: [localEvidence.id],
						responsibilityTransfers: [],
					},
					repairObligation.id,
				);
				await approvePlan(job, repairPlan);
				const repairContract = buildEffectiveTaskContract(job, repairPlan, repairPlan.tasks[0]!);
				expect(repairContract.responsibilityTransfers).toEqual(localTask.responsibilityTransfers);
				expect(repairContract.repairChecks.some((check) => check.issueId === issueId)).toBe(false);
				expect(repairContract.acceptanceChecks).not.toContain(sourceCriterion);
				const repairTask = await job.dispatchTask({
					...repairContract,
					effectiveContractHash: semanticContractHash(repairContract),
					replayKey: `stage-plan:${repairPlan.id}:${repairPlan.tasks[0]!.key}`,
				});
				await job.pause("check inherited handoff persistence");
				await job.reload();
				await job.resume();
				expect(job.state.tasks[repairTask.id].responsibilityTransfers).toEqual(localTask.responsibilityTransfers);
				await job.setTaskStatus(repairTask.id, "succeeded");
				localEvidence = await job.recordEvidence({
					taskId: repairTask.id,
					stageId: "literature",
					type: repairTask.requiredOutputType,
					content: { limitations: ["The repaired query scope is recorded"] },
					refs: [],
					currentEvidenceSetId: sourceEvidence.currentEvidenceSetId,
				});
			}
		}
		await job.recordReview(reviewFixture(job, { evidenceId: localEvidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(localEvidence.id, true, "accept-local-issue-transfer");
		expect(job.state.obligations[obligationId]!.items!.find((item) => item.id === issueId)!.status).toBe("open");
		expect(job.state.graph.nodes[job.state.obligations[obligationId]!.graphObjectionId!]?.status).toBe("open");
		const frozenLocalPlan = structuredClone(planEvidence(job, localPlan.id)!);
		const synthesisPlan = plan(job, "plan-synthesis-issue-transfer", "literature", {
			key: "complete-issue-synthesis",
			deliveryKind: "synthesis",
			objective: "Complete the literature synthesis",
			inputArtifactRefs: [localEvidence.id],
			requiredOutputFields: literature.requiredOutputFields,
			acceptanceChecks: [],
			failureSignals: [],
			successCriteria: [],
			responsibilityTransfers: [],
		});
		let plannedMode: string | undefined;
		let plannedObligation: string | undefined;
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => ({
					content: Object.fromEntries(task.requiredOutputFields.map((field) => [field, field])),
					refs: [],
					artifactType: task.requiredOutputType,
				}),
			},
			reviewer: {
				review: async (evidence) => reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				planStage: async (_activeJob, obligation, requestedMode) => {
					plannedMode = requestedMode;
					plannedObligation = obligation?.id;
					return synthesisPlan;
				},
				decideEvidence: async (evidence) => ({
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: "accept-synthesis-issue",
					jobId: job.state.frame.jobId,
					decisionType: "evidence",
					decisionRef: "accept-synthesis-issue",
					evidenceId: evidence.id,
					decision: "accept",
					rationale: "The complete synthesis passed review.",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				}),
				decideAdoption: async (evidence) => ({
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: "adopt-synthesis-issue",
					jobId: job.state.frame.jobId,
					decisionType: "adoption",
					decisionRef: "adopt-synthesis-issue",
					evidenceId: evidence.id,
					adopt: true,
					rationale: "The complete synthesis is ready for adoption.",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				}),
				decideSearch: async () => {
					throw new Error("search decision was not expected");
				},
				decideRoute: async () => {
					throw new Error("stop after synthesis adoption");
				},
			},
		});
		await supervisor.tick();
		expect(plannedMode).toBe("decompose");
		expect(plannedObligation).toBeUndefined();
		expect(job.state.obligations[obligationId]!.items!.find((item) => item.id === issueId)!.status).toBe("resolved");
		const objectionId = job.state.obligations[obligationId]!.graphObjectionId!;
		expect(job.state.graph.nodes[objectionId]?.status).toBe("resolved");
		expect(job.state.graph.unresolvedObjectionIds).not.toContain(objectionId);
		expect(
			researchMilestones(job.state)
				.flatMap((stage) => stage.plans)
				.find((entry) => entry.id === localPlan.id)?.status,
		).toBe("stale");
		expect(planReviewStatus(job, localPlan.id)).toBe("stale");
		expect(planEvidence(job, localPlan.id)).toEqual(frozenLocalPlan);
		expect(evidenceHasCurrentPlanApproval(job, localEvidence)).toBe(true);
		expect(() => buildEffectiveTaskContract(job, localPlan, localPlan.tasks[0]!)).toThrow(
			EffectiveContractUnavailableError,
		);
		await expect(
			job.dispatchTask({
				...localContract,
				id: "closed-transfer-redispatch",
				effectiveContractHash: semanticContractHash(localContract),
				replayKey: "closed-transfer-redispatch",
			}),
		).rejects.toThrow("worker dispatch requires an independently approved plan");

		expect(job.completionBlockers()).not.toContain("unresolved research objections remain");
		await job.pause("check synthesis closure persistence");
		await job.reload();
		await job.resume();
		expect(job.state.graph.nodes[objectionId]?.status).toBe("resolved");
		expect(job.state.graph.unresolvedObjectionIds).not.toContain(objectionId);
	},
);
