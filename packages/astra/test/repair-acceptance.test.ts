import { describe, expect, it } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type { MainAgentDecisionManifest, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

function decision(job: ResearchJob, fields: Partial<MainAgentDecisionManifest>): MainAgentDecisionManifest {
	return {
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: `decision_${job.state.eventSeq}`,
		jobId: job.state.frame.jobId,
		decisionType: "evidence",
		decisionRef: `decision_${job.state.eventSeq}`,
		stageId: "validation",
		rationale: "Verify repair acceptance",
		sessionRef: "fixture:main",
		createdAt: new Date().toISOString(),
		...fields,
	};
}

async function candidate(job: ResearchJob, key: string, currentEvidenceSetId?: string) {
	const repairChecks = currentEvidenceSetId
		? Object.values(job.state.obligations).flatMap((issue) =>
				(issue.items ?? []).map((item) => ({ issueId: item.id, criterion: `[${item.id}] ${item.criterion}` })),
			)
		: [];
	const task = await job.dispatchTask({
		repairChecks,
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: key,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["result"],
		acceptanceChecks: ["result is verified", ...repairChecks.map((check) => check.criterion)],
		failureSignals: ["unverified result"],
		dependencies: [],
		scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["result is verified"],
	});
	await job.setTaskStatus(task.id, "succeeded");
	return job.recordEvidence({
		taskId: task.id,
		stageId: "validation",
		type: "validation",
		content: { result: key },
		refs: [`fixture:${key}`],
		currentEvidenceSetId,
	});
}

describe("repair acceptance", () => {
	it.each(["ask-user", "advance", "complete"] as const)(
		"keeps unresolved issues blocking when repair strategy is %s",
		async (routeAction) => {
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				objective: "Keep open issues blocking",
				workspaceRoot: "/workspace",
				automation: "full",
			});
			const evidence = await candidate(job, "failed");
			await job.recordReview(
				reviewFixture(job, { evidenceId: evidence.id, verdict: "fail", findings: ["choose an acceptable scope"] }),
			);
			let plans = 0;
			const unexpected = async (): Promise<never> => {
				throw new Error("Unexpected execution before repair strategy");
			};
			const supervisor = new ResearchSupervisor(job, store, {
				worker: { run: unexpected },
				reviewer: { review: unexpected },
				mainAgent: {
					planStage: async () => {
						plans++;
						return unexpected();
					},
					decideEvidence: unexpected,
					decideAdoption: unexpected,
					decideSearch: unexpected,
					decideRoute: async () =>
						decision(job, {
							decisionType: "route",
							routeAction,
							question: "Which research boundary should apply?",
						}),
				},
			});
			await supervisor.tick();
			expect(plans).toBe(0);
			expect(job.state.paused).toBe(true);
			expect(job.state.frame.openObligationIds).toHaveLength(1);
			expect(job.state.frame.status).not.toBe("completed");
			if (routeAction === "ask-user") expect(job.state.frame.userGate?.kind).toBe("research");
			else expect(job.state.frame.nextAction).toContain("Open repair obligations");
		},
	);
	it("routes an open repair upstream and rebinds its retired input before closing the original issues", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			objective: "Repair upstream without losing obligations",
			workspaceRoot: "/workspace",
			automation: "full",
		});
		const original = await candidate(job, "upstream");
		await job.recordReview(reviewFixture(job, { evidenceId: original.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(original.id, true);
		const oldArtifact = await job.adoptEvidence(original.id);
		await job.applyRouteDecision(
			decision(job, { decisionType: "route", routeAction: "advance", targetStageId: "literature" }),
		);
		const downstream = await job.dispatchTask({
			...job.state.tasks[original.taskId],
			id: "downstream",
			replayKey: "downstream",
			stageId: "literature",
			requiredOutputType: "literature",
			inputArtifactRefs: [oldArtifact.id],
			requiredCanonicalArtifacts: [oldArtifact.id],
		});
		await job.setTaskStatus(downstream.id, "succeeded");
		const failed = await job.recordEvidence({
			taskId: downstream.id,
			stageId: "literature",
			type: "literature",
			content: { result: "bad input" },
			refs: [],
		});
		await job.recordReview(
			reviewFixture(job, { evidenceId: failed.id, verdict: "fail", findings: ["repair upstream scope"] }),
		);
		const obligation = Object.values(job.state.obligations)[0];
		let newArtifactId = "";
		let repairRuns = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => {
					repairRuns++;
					expect(task.inputArtifactRefs).toContain(newArtifactId);
					expect(task.inputArtifactRefs).not.toContain(oldArtifact.id);
					expect(task.repairOfEvidenceId).toBe(failed.id);
					return {
						artifactType: task.requiredOutputType,
						content: { result: "rechecked with new input" },
						refs: [],
					};
				},
			},
			reviewer: {
				review: async (evidence) => reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				planStage: async (_job, issue) => ({
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: "plan_rebound",
					jobId: job.state.frame.jobId,
					stageId: "literature",
					decisionRef: "plan_rebound",
					obligationId: issue?.id,
					mode: "repair",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
					rationale: "Verify original issues against repaired upstream",
					tasks: [
						{
							key: "repair",
							objective: "Recheck the downstream result",
							inputArtifactRefs: [newArtifactId],
							requiredOutputFields: job.definitions.literature.requiredOutputFields,
							acceptanceChecks: ["repaired inputs checked"],
							failureSignals: ["old inputs reused"],
							successCriteria: ["all findings verified"],
						},
					],
				}),
				decideEvidence: async (evidence) => decision(job, { evidenceId: evidence.id, decision: "accept" }),
				decideAdoption: async (evidence) =>
					decision(job, { decisionType: "adoption", evidenceId: evidence.id, adopt: true }),
				decideSearch: async () => {
					throw new Error("No search expected");
				},
				decideRoute: async () =>
					decision(job, {
						decisionType: "route",
						stageId: "literature",
						routeAction: newArtifactId ? "continue" : "backtrack",
						targetStageId: newArtifactId ? undefined : "validation",
						evidenceRefs: [failed.id],
					}),
			},
		});
		await supervisor.tick();
		expect(job.state.frame.activeStageId).toBe("validation");
		expect(repairRuns).toBe(0);
		expect(job.state.obligations[obligation.id].status).toBe("open");
		const replacement = await candidate(job, "repaired-upstream");
		await job.recordReview(reviewFixture(job, { evidenceId: replacement.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(replacement.id, true);
		newArtifactId = (await job.adoptEvidence(replacement.id)).id;
		await expect(
			job.dispatchTask({
				...downstream,
				id: "unbound_history",
				replayKey: "unbound_history",
				inputArtifactRefs: [failed.id],
				requiredCanonicalArtifacts: [],
				repairOfEvidenceId: failed.id,
			}),
		).rejects.toThrow(/omits current input/);
		await expect(
			job.dispatchTask({
				...downstream,
				id: "stale_history",
				replayKey: "stale_history",
				inputArtifactRefs: [failed.id],
				requiredCanonicalArtifacts: [],
			}),
		).rejects.toThrow(/stale/);
		await job.applyRouteDecision(
			decision(job, { decisionType: "route", routeAction: "advance", targetStageId: "literature" }),
		);
		await supervisor.tick();
		expect(repairRuns).toBe(1);
		expect(job.state.obligations[obligation.id].status).toBe("resolved");
		await job.reload();
		expect(job.state.obligations[obligation.id].items?.every((item) => item.status === "resolved")).toBe(true);
	});
	it.each([false, true])("rejects a repair without faithful issue checks (renamed: %s)", async (renamed) => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			objective: "Do not close unverified issues",
			workspaceRoot: "/workspace",
		});
		const original = await candidate(job, "broken");
		await job.recordReview(
			reviewFixture(job, {
				evidenceId: original.id,
				verdict: "fail",
				findings: ["missing control", "unverified data"],
			}),
		);
		const task = await job.dispatchTask({
			...job.state.tasks[original.taskId],
			repairChecks: renamed
				? Object.values(job.state.obligations)[0].items?.map((item) => ({
						issueId: item.id,
						criterion: "Everything is fine",
					}))
				: [],
			acceptanceChecks: renamed ? ["result is verified", "Everything is fine"] : ["result is verified"],
			id: "unbound_repair",
			replayKey: "unbound_repair",
		});
		await job.setTaskStatus(task.id, "succeeded");
		const repair = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { result: "looks fixed" },
			refs: [],
			currentEvidenceSetId: original.currentEvidenceSetId,
		});
		await job.recordReview(reviewFixture(job, { evidenceId: repair.id, verdict: "pass", findings: [] }));
		await expect(job.decideEvidence(repair.id, true)).rejects.toThrow(/explicit verified closure/);
		expect(Object.values(job.state.obligations)[0].items?.every((item) => item.status === "open")).toBe(true);
	});
	it("includes the active canonical chain in a whole-research review even when its plan omits inputs", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			objective: "Review the actual evidence chain",
			workspaceRoot: "/workspace",
			automation: "full",
		});
		const original = await candidate(job, "raw-result");
		await job.recordReview(reviewFixture(job, { evidenceId: original.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(original.id, true);
		const artifact = await job.adoptEvidence(original.id);
		await job.applyRouteDecision(
			decision(job, { decisionType: "route", routeAction: "advance", targetStageId: "research-review" }),
		);
		const contract = job.definitions["research-review"];
		let receivedInputs: string[] = [];
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => {
					receivedInputs = task.inputArtifactRefs;
					return { artifactType: task.requiredOutputType, content: { verdict: "blocked" }, refs: [] };
				},
			},
			reviewer: {
				review: async (evidence, currentJob) =>
					reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			},
			mainAgent: {
				planStage: async () => ({
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: "plan_full_chain",
					jobId: job.state.frame.jobId,
					stageId: "research-review",
					decisionRef: "plan_full_chain",
					sessionRef: "fixture:main",
					rationale: "Whole-research review",
					createdAt: new Date().toISOString(),
					tasks: [
						{
							key: "audit",
							objective: "Audit the complete research",
							inputArtifactRefs: [],
							requiredOutputFields: contract.requiredOutputFields,
							acceptanceChecks: contract.acceptanceChecks,
							failureSignals: contract.failureSignals,
							successCriteria: ["all required evidence is available"],
						},
					],
				}),
				decideEvidence: async () => {
					throw new Error("Two reviews required");
				},
				decideAdoption: async () => {
					throw new Error("Two reviews required");
				},
				decideSearch: async () => {
					throw new Error("No search expected");
				},
				decideRoute: async () => {
					throw new Error("No route expected");
				},
			},
		});
		await supervisor.tick();
		expect(receivedInputs).toEqual([artifact.id]);
	});
	it("keeps obligations open until the configured reviews and evidence acceptance complete", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			objective: "Require both repair reviews",
			workspaceRoot: "/workspace",
			definitions: [
				{
					...DEFAULT_STAGES[0],
					qualityPolicy: { minPassingReviews: 2, minScore: 0.8, requireResolvableArtifacts: true },
				},
			],
		});
		const original = await candidate(job, "original");
		await job.recordReview(
			reviewFixture(job, { evidenceId: original.id, verdict: "fail", findings: ["repair the result"] }),
		);
		const repair = await candidate(job, "repair", original.currentEvidenceSetId);
		await job.recordReview(reviewFixture(job, { evidenceId: repair.id, verdict: "pass", findings: [] }));
		expect(job.state.frame.openObligationIds).toHaveLength(1);
		await expect(job.decideEvidence(repair.id, true)).rejects.toThrow("passing reviews");
		await job.recordReview(reviewFixture(job, { evidenceId: repair.id, verdict: "pass", findings: [] }));
		expect(job.state.frame.openObligationIds).toHaveLength(1);
		await job.decideEvidence(repair.id, true);
		expect(job.state.frame.openObligationIds).toEqual([]);
		expect(job.state.graph.unresolvedObjectionIds).toEqual([]);
	});

	it("does not accept conflicting reviews or close their obligations with later passing votes", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			objective: "Preserve a failed review on unchanged evidence",
			workspaceRoot: "/workspace",
		});
		const original = await candidate(job, "conflicted");
		await job.recordReview(
			reviewFixture(job, { evidenceId: original.id, verdict: "fail", findings: ["missing control"] }),
		);
		await job.recordReview(reviewFixture(job, { evidenceId: original.id, verdict: "pass", findings: [] }));
		await expect(job.decideEvidence(original.id, true)).rejects.toThrow("non-passing review");
		expect(job.state.frame.openObligationIds).toHaveLength(1);
	});

	it("carries stage checks and the backtrack defect into repair review, then closes only that defect", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			objective: "Finish a governed backtrack",
			workspaceRoot: "/workspace",
			automation: "full",
		});
		await job.reopenStage("literature", "backtrack_sources", "repair missing source receipts");
		const unrelatedObjection = job.state.graph.unresolvedObjectionIds[0];
		await job.reopenStage("validation", "backtrack_layout", "repair clipped content and verify page bounds");
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_backtrack",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan_backtrack",
			tasks: [
				{
					key: "repair",
					objective: "Repair the identified defect",
					inputArtifactRefs: [],
					requiredOutputFields: DEFAULT_STAGES[0].requiredOutputFields,
					acceptanceChecks: ["model-authored extra check"],
					failureSignals: ["model-authored failure"],
					successCriteria: ["new result delivered"],
				},
			],
			rationale: "Apply the review finding",
			sessionRef: "fixture:main",
			createdAt: new Date().toISOString(),
		};
		const supervisor = new ResearchSupervisor(job, store, {
			worker: {
				run: async (task) => ({ artifactType: task.requiredOutputType, content: { repaired: true }, refs: [] }),
			},
			reviewer: {
				review: async (evidence, currentJob) => {
					if (evidence.type === "stage-plan")
						return reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] });
					const task = job.state.tasks[evidence.taskId];
					expect(task.acceptanceChecks).toEqual(
						expect.arrayContaining([
							...DEFAULT_STAGES[0].acceptanceChecks,
							"model-authored extra check",
							"repair clipped content and verify page bounds",
						]),
					);
					expect(task.failureSignals).toEqual(expect.arrayContaining(DEFAULT_STAGES[0].failureSignals));
					return reviewFixture(currentJob, { evidenceId: evidence.id, ...{ verdict: "pass", findings: [] } });
				},
			},
			mainAgent: {
				planStage: async () => plan,
				decideEvidence: async (evidence) => decision(job, { evidenceId: evidence.id, decision: "accept" }),
				decideAdoption: async (evidence) =>
					decision(job, { decisionType: "adoption", evidenceId: evidence.id, adopt: true }),
				decideSearch: async () => {
					throw new Error("No search expected");
				},
				decideRoute: async () =>
					decision(job, { decisionType: "route", routeAction: "advance", targetStageId: "literature" }),
			},
		});
		await supervisor.tick();
		expect(job.state.graph.unresolvedObjectionIds).toEqual([unrelatedObjection]);
		expect(job.state.canonicalRoute.stageArtifactIds.validation).toBeDefined();
		expect(job.state.frame.activeStageId).toBe("literature");
		await job.reload();
		expect(job.state.graph.unresolvedObjectionIds).toEqual([unrelatedObjection]);
	});
});
