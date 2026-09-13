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
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: key,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["result"],
		acceptanceChecks: ["result is verified"],
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
