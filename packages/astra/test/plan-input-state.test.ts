import { describe, expect, it } from "vitest";
import { type EffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { planReviewStatus, planReviewStatusFromSnapshot, preparePlanEvidence } from "../src/plan-review.ts";
import { researchMilestones } from "../src/progress.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { StageDefinition, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

describe("plan input governance snapshot", () => {
	it("keeps semantic contract hashes stable across JSON persistence", () => {
		const contract = {
			planId: "plan_json_roundtrip",
			inputVersions: [
				{
					inputRef: "legacy-canonical",
					evidenceId: null,
					versionHash: null,
					status: null,
					canonical: {
						id: "legacy-canonical",
						status: "active",
						adoptedAt: "now",
						evidenceSnapshotHash: undefined,
					},
				},
			],
		} as unknown as EffectiveTaskContract;
		const restored = JSON.parse(JSON.stringify(contract)) as EffectiveTaskContract;
		expect(semanticContractHash(restored)).toBe(semanticContractHash(contract));
	});

	it("marks a reviewed plan stale when an input version changes before dispatch", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			workspaceRoot: "/workspace",
			objective: "Bind review to the exact worker contract and input versions",
		});
		const sourceTask = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "deliver an input",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verified"],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 1000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		await job.setTaskStatus(sourceTask.id, "succeeded");
		const source = await job.recordEvidence({
			taskId: sourceTask.id,
			stageId: "validation",
			type: "validation",
			content: { content: "version one" },
			refs: [],
		});
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_frozen_input",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan_frozen_input",
			tasks: [
				{
					key: "use_input",
					objective: "use declared evidence",
					inputArtifactRefs: [source.id],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: ["verified"],
					failureSignals: [],
					successCriteria: [],
				},
			],
			rationale: "fixture",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(plan);
		const planEvidence = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: planEvidence.id, verdict: "pass", findings: [] }));
		expect(planReviewStatus(job, plan.id)).toBe("passed");
		await job.recordReview(
			reviewFixture(job, { evidenceId: source.id, verdict: "fail", findings: ["repair input"] }),
		);
		await job.decideEvidence(source.id, false);
		expect(planReviewStatus(job, plan.id)).toBe("stale");
		expect(
			researchMilestones(job.state)
				.flatMap((stage) => stage.plans)
				.find((item) => item.id === plan.id)?.status,
		).toBe("stale");
	});

	it.each(["unknown-stage", "programming-error"] as const)(
		"propagates %s when checking a reviewed plan",
		async (failure) => {
			const job = await ResearchJob.create(new MemoryAstraStore(), {
				workspaceRoot: "/workspace",
				objective: "preserve unexpected contract errors",
			});
			const value: StagePlanManifest = {
				schemaVersion: "astra.stage_plan_manifest.v1",
				id: "error-propagation",
				jobId: job.state.frame.jobId,
				stageId: "validation",
				decisionRef: "error-propagation",
				tasks: [
					{
						key: "bounded",
						objective: "deliver bounded result",
						inputArtifactRefs: [],
						requiredOutputFields: job.definitions.validation.requiredOutputFields,
						acceptanceChecks: ["verified"],
						failureSignals: [],
						successCriteria: [],
					},
				],
				rationale: "fixture",
				sessionRef: "fixture",
				createdAt: new Date().toISOString(),
			};
			await job.recordStagePlan(value);
			const evidence = await preparePlanEvidence(job, value);
			await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
			const definitions: Record<string, StageDefinition> = {
				...job.definitions,
				validation: { ...job.definitions.validation },
			};
			if (failure === "unknown-stage") {
				delete definitions.validation;
				expect(() => planReviewStatusFromSnapshot(job.state, definitions, value.id)).toThrow(
					"unknown stage validation",
				);
			} else {
				const error = new TypeError("unexpected contract programming error");
				Object.defineProperty(definitions.validation, "acceptanceChecks", {
					get: () => {
						throw error;
					},
				});
				expect(() => planReviewStatusFromSnapshot(job.state, definitions, value.id)).toThrow(error);
			}
		},
	);

	it("rejects dispatch criteria added after the reviewed effective contract", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			workspaceRoot: "/workspace",
			objective: "Do not extend a reviewed task during dispatch",
		});
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_exact_contract",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan_exact_contract",
			tasks: [
				{
					key: "bounded",
					objective: "deliver the bounded validation",
					inputArtifactRefs: [],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: ["verified"],
					failureSignals: [],
					successCriteria: [],
				},
			],
			rationale: "fixture",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(plan);
		const evidence = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		await expect(
			job.dispatchTask({
				planId: plan.id,
				effectiveContractHash: "unreviewed-contract",
				id: "task_extra_criteria",
				replayKey: `stage-plan:${plan.id}:bounded`,
				stageId: plan.stageId,
				stageExecutionId: "stage_exec_validation",
				role: "worker",
				deliveryKind: "stage",
				objective: plan.tasks[0]!.objective,
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: plan.tasks[0]!.requiredOutputFields,
				acceptanceChecks: ["verified", "unreviewed criterion"],
				failureSignals: [],
				dependencies: [],
				scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
				allowedTools: job.definitions.validation.workerTools,
				writeAuthority: job.definitions.validation.workspaceWrite ? "workspace-write" : "none",
				budget: job.definitions.validation.workerBudget!,
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: [],
			}),
		).rejects.toThrow(/frozen reviewed plan/);
	});

	it("blocks ready workers from legacy plans without frozen contracts", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			workspaceRoot: "/workspace",
			objective: "recover a legacy plan safely",
		});
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_legacy_ready",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan_legacy_ready",
			tasks: [
				{
					key: "bounded",
					objective: "deliver",
					inputArtifactRefs: [],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: ["verified"],
					failureSignals: [],
					successCriteria: [],
				},
			],
			rationale: "fixture",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(plan);
		const planEvidence = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: planEvidence.id, verdict: "pass", findings: [] }));
		const contract = (planEvidence.content as { effectiveContracts: Array<{ contract: EffectiveTaskContract }> })
			.effectiveContracts[0]!.contract;
		const task = await job.dispatchTask({
			planId: plan.id,
			effectiveContractHash: semanticContractHash(contract),
			replayKey: `stage-plan:${plan.id}:bounded`,
			stageId: "validation",
			stageExecutionId: job.state.stages.validation.executionId ?? "stage_exec_validation",
			role: "worker",
			deliveryKind: contract.deliveryKind,
			objective: contract.objective,
			inputArtifactRefs: contract.inputArtifactRefs,
			requiredCanonicalArtifacts: contract.requiredCanonicalArtifacts,
			requiredOutputType: contract.requiredOutputType,
			requiredOutputFields: contract.requiredOutputFields,
			acceptanceChecks: contract.acceptanceChecks,
			failureSignals: contract.failureSignals,
			dependencies: contract.dependencies,
			scope: contract.scope,
			allowedTools: contract.allowedTools,
			writeAuthority: contract.writeAuthority,
			budget: contract.budget,
			reviewGateRequired: contract.reviewGateRequired,
			resumePolicy: contract.resumePolicy,
			successCriteria: contract.successCriteria,
		});
		const legacySnapshot = job.state;
		delete (legacySnapshot.evidence[planEvidence.id]!.content as { effectiveContracts?: unknown }).effectiveContracts;
		await store.writeSnapshot(legacySnapshot);
		const legacyJob = (await ResearchJob.open(store, job.state.frame.jobId))!;
		let workerRuns = 0;
		const supervisor = new ResearchSupervisor(legacyJob, store, {
			worker: {
				run: async () => {
					workerRuns += 1;
					return { artifactType: "validation", content: {}, refs: [] };
				},
			},
			reviewer: {
				review: async (evidence, currentJob) =>
					reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				planStage: async () => {
					throw new Error("unexpected legacy plan replacement");
				},
				decideEvidence: async () => {
					throw new Error("unexpected evidence decision");
				},
				decideAdoption: async () => {
					throw new Error("unexpected adoption decision");
				},
				decideSearch: async () => {
					throw new Error("unexpected search decision");
				},
				decideRoute: async () => {
					throw new Error("unexpected route decision");
				},
			},
		});
		await supervisor.tick();
		expect(workerRuns).toBe(0);
		expect(legacyJob.state.tasks[task.id]?.status).toBe("blocked");
	});

	it("freezes adoption separately from immutable pre-adoption delivery text", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			workspaceRoot: "/workspace",
			objective: "Use reviewed results without rewriting historical delivery text",
		});
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "deliver bounded result",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verified"],
			failureSignals: ["incorrect"],
			dependencies: [],
			scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 1000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["verified"],
		});
		await job.setTaskStatus(task.id, "succeeded");
		const content = { content: "incomplete pending independent review and adoption" };
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: "validation",
			refs: [],
			content,
		});
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_before_adoption",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "decision_fixture",
			tasks: [
				{
					key: "use_result",
					objective: "inspect result",
					inputArtifactRefs: [evidence.id],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: ["verified"],
					failureSignals: ["incorrect"],
					successCriteria: ["verified"],
				},
			],
			rationale: "fixture",
			sessionRef: "fixture:main",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(plan);
		const before = await preparePlanEvidence(job, plan);
		const beforeContent = structuredClone(before.content);
		expect(before.content).toMatchObject({
			inputStates: [
				{ inputRef: evidence.id, evidenceId: evidence.id, evidenceStatus: "candidate", canonical: null },
			],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(evidence.id, true);
		const artifact = await job.adoptEvidence(evidence.id);
		const afterPlan = {
			...plan,
			id: "plan_after_adoption",
			tasks: [{ ...plan.tasks[0], inputArtifactRefs: [artifact.id] }],
		};
		await job.recordStagePlan(afterPlan);
		const after = await preparePlanEvidence(job, afterPlan);
		expect(after.content).toMatchObject({
			inputStates: [
				{
					inputRef: artifact.id,
					evidenceId: evidence.id,
					evidenceStatus: "accepted",
					canonical: {
						id: artifact.id,
						status: "active",
						adoptedAt: artifact.adoptedAt,
						evidenceSnapshotHash: artifact.evidenceSnapshotHash,
					},
				},
			],
		});
		expect(job.state.evidence[before.id].content).toEqual(beforeContent);
		expect(job.state.evidence[evidence.id].content).toEqual(content);
		const repairTask = await job.dispatchTask({ ...task, id: "repair-source", replayKey: "repair-source" });
		await job.setTaskStatus(repairTask.id, "succeeded");
		const repairEvidence = await job.recordEvidence({
			taskId: repairTask.id,
			stageId: "validation",
			type: "validation",
			content,
			refs: [],
		});
		await job.recordReview(
			reviewFixture(job, { evidenceId: repairEvidence.id, verdict: "fail", findings: ["missing proof"] }),
		);
		const obligation = Object.values(job.state.obligations).find((item) => item.evidenceId === repairEvidence.id)!;
		const repairPlan = { ...plan, id: "repair-plan", mode: "repair" as const, obligationId: obligation.id };
		const repairSnapshot = await preparePlanEvidence(job, repairPlan);
		expect(repairSnapshot.content).toMatchObject({
			dispatchAdditions: {
				inheritedTask: {
					id: repairTask.id,
					requiredOutputFields: ["content"],
					acceptanceChecks: ["verified"],
					successCriteria: ["verified"],
				},
				reviewFindings: ["missing proof"],
				backtrackChecks: [],
			},
		});
	});
});
