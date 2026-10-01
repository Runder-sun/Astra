import { describe, expect, it } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

describe("repeated repair failures", () => {
	it("repairs the latest failed revision of the oldest open lineage and inherits its materials", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			objective: "Keep repair progress",
			workspaceRoot: "/workspace",
			automation: "full",
		});
		const revisions = [];
		for (const [key, lineage] of [
			["original", "A"],
			["latest", "A"],
			["unrelated", "B"],
		]) {
			const task = await job.dispatchTask({
				stageId: "validation",
				stageExecutionId: "validation",
				role: "worker",
				objective: key,
				inputArtifactRefs: key === "latest" ? [revisions[0].id] : [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: key === "latest" ? ["result", "recoveredRows"] : ["result"],
				acceptanceChecks: ["verify source"],
				successCriteria: [],
				failureSignals: [],
				dependencies: [],
				scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: { result: key },
				refs: [],
				currentEvidenceSetId: lineage,
			});
			revisions.push(evidence);
			await job.recordReview(
				reviewFixture(job, { evidenceId: evidence.id, verdict: "fail", findings: ["source missing"] }),
			);
		}
		// The same wording in another evidence lineage must create its own obligations.
		expect(Object.values(job.state.obligations).at(-1)?.items).toHaveLength(2);
		let repairedEvidence: string | undefined;
		let dispatched: TaskPacket | undefined;
		const unexpected = async (): Promise<never> => {
			throw new Error("unexpected decision");
		};
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => {
					dispatched = task;
					await job.pause("fixture stops after repair dispatch");
					return { artifactType: task.requiredOutputType, content: { result: "repair" }, refs: [] };
				},
			},
			reviewer: {
				review: async (evidence) => reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				decideRoute: async (_job, obligation) => {
					repairedEvidence = obligation?.evidenceId;
					return {
						schemaVersion: "astra.main_agent_decision_manifest.v1",
						manifestId: "continue",
						jobId: job.state.frame.jobId,
						decisionType: "route",
						decisionRef: "continue",
						stageId: "validation",
						routeAction: "continue",
						rationale: "repair",
						sessionRef: "fixture",
						createdAt: new Date().toISOString(),
					};
				},
				planStage: async (_job, obligation) => ({
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: "latest_revision",
					jobId: job.state.frame.jobId,
					stageId: "validation",
					decisionRef: "latest_revision",
					obligationId: obligation?.id,
					mode: "repair",
					sessionRef: "fixture",
					createdAt: new Date().toISOString(),
					rationale: "inherit progress",
					tasks: [
						{
							key: "repair",
							objective: "repair latest",
							inputArtifactRefs: [],
							requiredOutputFields: job.definitions.validation.requiredOutputFields,
							acceptanceChecks: ["verify source"],
							failureSignals: [],
							successCriteria: [],
						},
					],
				}),
				decideEvidence: unexpected,
				decideAdoption: unexpected,
				decideSearch: unexpected,
			},
		});
		await supervisor.tick();
		expect(repairedEvidence).toBe(revisions[1].id);
		expect(dispatched?.repairOfEvidenceId).toBe(revisions[1].id);
		expect(dispatched?.inputArtifactRefs).toEqual(expect.arrayContaining([revisions[0].id, revisions[1].id]));
		expect(dispatched?.requiredOutputFields).toContain("recoveredRows");
		expect(dispatched?.repairChecks).toHaveLength(2);
	});
	it("retains new findings without cloning existing issues across twelve failed revisions", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			objective: "Bound repair growth without losing obligations",
			workspaceRoot: "/workspace",
		});
		for (let round = 0; round < 12; round++) {
			const repairChecks = Object.values(job.state.obligations).flatMap((issue) =>
				(issue.items ?? []).map((item) => ({
					issueId: item.id,
					criterion: job.repairCriterion(`[${item.id}] ${item.criterion}`),
				})),
			);
			const task = await job.dispatchTask({
				stageId: "validation",
				stageExecutionId: "validation",
				role: "worker",
				objective: `revision ${round}`,
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: ["result"],
				acceptanceChecks: ["verify source", ...repairChecks.map((check) => check.criterion)],
				repairChecks,
				successCriteria: [],
				failureSignals: [],
				dependencies: [],
				scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: {},
				refs: [],
				currentEvidenceSetId: "one-lineage",
			});
			await job.recordReview(
				reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: "fail",
					findings: round < 6 ? ["source missing"] : ["source missing", "hash mismatched"],
				}),
			);
			const items = Object.values(job.state.obligations).flatMap((issue) => issue.items ?? []);
			expect(items).toHaveLength(round < 6 ? 2 : 3);
			expect(items.every((item) => item.status === "open")).toBe(true);
		}
		expect(Object.values(job.state.reviews)).toHaveLength(12);
		expect(job.state.graph.unresolvedObjectionIds).toHaveLength(12);
		const last = Object.values(job.state.tasks).at(-1)!;
		const repairChecks = Object.values(job.state.obligations).flatMap((issue) =>
			(issue.items ?? []).map((item) => ({
				issueId: item.id,
				criterion: job.normalizedRepairCriterion(item.criterion),
			})),
		);
		for (const variant of ["missing", "renamed", "complete", "separate-lineage"]) {
			const checks =
				variant === "missing"
					? repairChecks.slice(1)
					: repairChecks.map((check) => ({
							...check,
							criterion: variant === "renamed" ? "unrelated check" : check.criterion,
						}));
			const task = await job.dispatchTask({
				...last,
				id: variant,
				replayKey: variant,
				repairChecks: checks,
				acceptanceChecks: ["verify source", ...checks.map((check) => check.criterion)],
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: {},
				refs: [],
				currentEvidenceSetId: variant === "separate-lineage" ? "another-lineage" : "one-lineage",
			});
			await job.recordReview(
				reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: variant === "separate-lineage" ? "fail" : "pass",
					findings: variant === "separate-lineage" ? ["source missing"] : [],
				}),
			);
			if (variant === "missing" || variant === "renamed") {
				await expect(job.decideEvidence(evidence.id, true)).rejects.toThrow("explicit verified closure");
			} else if (variant === "complete") {
				await job.decideEvidence(evidence.id, true);
				expect(job.state.frame.openObligationIds).toEqual([]);
				expect(job.state.graph.unresolvedObjectionIds).toEqual([]);
				expect(
					Object.values(job.state.obligations)
						.flatMap((issue) => issue.items ?? [])
						.every((item) => item.status === "resolved"),
				).toBe(true);
			} else {
				expect(Object.values(job.state.obligations).at(-1)?.items?.length).toBeGreaterThan(0);
			}
		}
	});
});
