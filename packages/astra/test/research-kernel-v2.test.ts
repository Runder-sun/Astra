import { access, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { PiChildSessionRunner } from "../src/pi-child-session.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { buildResearchBoard, formatResearchBoard } from "../src/research-board.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { taskResourcePath } from "../src/task-workspace.ts";
import { reviewFixture } from "./review-fixture.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function recordPassingArtifact(
	job: ResearchJob,
	stageId: string,
	content: unknown,
	inputArtifactRefs = Object.values(job.state.canonicalRoute.stageArtifactIds),
): Promise<string> {
	const definition = job.definitions[stageId];
	const task = await job.dispatchTask({
		stageId,
		stageExecutionId: `stage_exec_${stageId}`,
		role: "worker",
		objective: `produce ${stageId} evidence`,
		inputArtifactRefs,
		requiredCanonicalArtifacts: inputArtifactRefs,
		requiredOutputType: definition.outputArtifactType,
		requiredOutputFields: definition.requiredOutputFields,
		acceptanceChecks: definition.acceptanceChecks,
		failureSignals: definition.failureSignals,
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: definition.workerTools,
		writeAuthority: definition.workspaceWrite ? "workspace-write" : "none",
		budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30_000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: definition.acceptanceChecks,
	});
	await job.setTaskStatus(task.id, "running");
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId,
		type: definition.outputArtifactType,
		content,
		refs: [`pi-session:${task.id}`],
	});
	for (let index = 0; index < (definition.qualityPolicy?.minPassingReviews ?? 1); index++) {
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	}
	await job.decideEvidence(evidence.id, true, `accept-${evidence.id}`);
	return (await job.adoptEvidence(evidence.id)).id;
}

describe("Research Kernel v2", () => {
	it("stores the mission and user guidance in one canonical research graph", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_graph",
			objective: "find the most reliable method",
			workspaceRoot: "/workspace",
		});

		expect(Object.values(job.state.graph.nodes)).toContainEqual(
			expect.objectContaining({ kind: "question", statement: "find the most reliable method", status: "open" }),
		);
		await job.recordUserGuidance("Prefer methods that can be reproduced on one GPU");

		const board = buildResearchBoard(job.state);
		expect(board.questions).toHaveLength(1);
		expect(board.decisions[0]?.statement).toContain("one GPU");
		const formatted = formatResearchBoard(board);
		expect(formatted).toContain("Open questions");
		expect(formatted).toContain("Prefer methods that can be reproduced on one GPU");
	});

	it("records typed claim outcomes without accepting unsupported claims", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-typed-claims-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_typed_claims",
			objective: "determine whether the primary hypothesis holds",
			workspaceRoot: root,
		});
		await recordPassingArtifact(
			job,
			"result-to-claim",
			{
				scientificOutcome: "partially-supported",
				missionCoverage: "sufficient",
				claims: [
					{
						statement: "The bounded primary comparison supports the mechanism.",
						assessment: "supported",
					},
					{
						statement: "The mechanism generalizes to every embodied benchmark.",
						assessment: "unsupported",
					},
				],
				supportingResults: ["bounded comparison"],
				unsupportedClaims: ["universal generalization"],
				missingEvidence: [],
				conclusion: "The primary mechanism is supported only in the bounded comparison.",
			},
			[],
		);

		const claimNodes = Object.values(job.state.graph.nodes).filter((node) => node.kind === "claim");
		expect(job.state.frame.scientificOutcome).toBe("partially-supported");
		expect(job.state.frame.missionCoverage).toBe("sufficient");
		expect(claimNodes).toEqual(
			expect.arrayContaining([
				expect.objectContaining({ claimAssessment: "supported", status: "accepted" }),
				expect.objectContaining({ claimAssessment: "unsupported", status: "rejected" }),
			]),
		);
		expect(job.state.graph.acceptedClaimIds).toHaveLength(1);
	});

	it("separates completed research from an inconclusive scientific outcome", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-inconclusive-outcome-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_inconclusive_outcome",
			objective: "test a hypothesis without forcing a positive result",
			workspaceRoot: root,
		});
		await recordPassingArtifact(
			job,
			"result-to-claim",
			{
				scientificOutcome: "inconclusive",
				missionCoverage: "insufficient",
				claims: [
					{
						statement: "The available pilot establishes the primary hypothesis.",
						assessment: "unsupported",
					},
				],
				supportingResults: [],
				unsupportedClaims: ["primary hypothesis"],
				missingEvidence: ["independent reruns"],
				conclusion: "The process completed with insufficient evidence.",
			},
			[],
		);
		await recordPassingArtifact(job, "research-review", {
			verdict: "pass_with_nonblocking_caveats",
			scientificOutcome: "inconclusive",
			missionCoverage: "insufficient",
			strengths: ["unsupported claims remain withheld"],
			weaknesses: ["independent reruns are missing"],
			claimAudit: ["the primary hypothesis is not accepted"],
			requiredRepairs: [],
		});

		expect(job.state.graph.acceptedClaimIds).toHaveLength(0);
		expect(job.completionBlockers()).not.toContain("no accepted research claim");
		expect(job.completionBlockers()).toEqual([]);
		expect(job.state.frame.status).toBe("running");
		expect(job.state.frame.scientificOutcome).toBe("inconclusive");
	});

	it("blocks completion until explicitly required paper artifacts exist", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_required_paper",
			objective: "produce a reviewed paper",
			workspaceRoot: "/workspace",
			requiredArtifactTypes: ["paper-write", "paper-compile"],
		});

		expect(job.completionBlockers()).toEqual(
			expect.arrayContaining([
				"required canonical artifact paper-write is missing",
				"required canonical artifact paper-compile is missing",
			]),
		);
	});

	it("requires two independent passing reviews for result-to-claim evidence", async () => {
		const definition = DEFAULT_STAGES.find((stage) => stage.id === "result-to-claim");
		if (!definition) throw new Error("result-to-claim definition missing");
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_review_quorum",
			objective: "require independent claim review",
			workspaceRoot: "/workspace",
			definitions: [definition],
		});
		const task = await job.dispatchTask({
			stageId: definition.id,
			stageExecutionId: "stage_exec_result_to_claim",
			role: "worker",
			objective: "map the bounded results to claims",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: definition.outputArtifactType,
			requiredOutputFields: definition.requiredOutputFields,
			acceptanceChecks: definition.acceptanceChecks,
			failureSignals: definition.failureSignals,
			dependencies: [],
			scope: { workspaceRoot: "/workspace", allowedPaths: ["."] },
			allowedTools: definition.workerTools,
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: definition.acceptanceChecks,
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: definition.id,
			type: definition.outputArtifactType,
			content: {
				scientificOutcome: "supported",
				missionCoverage: "sufficient",
				claims: [{ statement: "bounded claim", assessment: "supported" }],
				supportingResults: ["result"],
				unsupportedClaims: [],
				missingEvidence: [],
				conclusion: "supported",
			},
			refs: ["pi-session:claim-worker"],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [], score: 1 }));
		await expect(job.decideEvidence(evidence.id, true, "one-review")).rejects.toThrow("configured passing reviews");
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [], score: 1 }));
		await expect(job.decideEvidence(evidence.id, true, "two-reviews")).resolves.toBeUndefined();
	});

	it("turns an explicit search plan into durable candidate lineage", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_search",
			objective: "compare candidate methods",
			workspaceRoot: "/workspace",
		});
		await job.recordStagePlan({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_search",
			jobId: "job_search",
			stageId: "validation",
			decisionRef: "decision_search",
			mode: "search",
			tasks: [
				{
					key: "candidate_a",
					objective: "test candidate A",
					hypothesis: "candidate A is more reliable",
					inputArtifactRefs: [],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: job.definitions.validation.acceptanceChecks,
					failureSignals: job.definitions.validation.failureSignals,
					successCriteria: job.definitions.validation.acceptanceChecks,
				},
				{
					key: "candidate_b",
					objective: "test candidate B",
					hypothesis: "candidate B is simpler",
					inputArtifactRefs: [],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: job.definitions.validation.acceptanceChecks,
					failureSignals: job.definitions.validation.failureSignals,
					successCriteria: job.definitions.validation.acceptanceChecks,
				},
			],
			rationale: "compare alternatives before choosing",
			sessionRef: "pi-session:main",
			createdAt: new Date().toISOString(),
		});

		const batch = Object.values(job.state.searchBatches)[0];
		expect(batch).toMatchObject({ stageId: "validation", status: "planning", strategy: "diverse-candidates" });
		expect(Object.values(batch?.candidates ?? {})).toHaveLength(2);
		expect(Object.values(job.state.graph.nodes).filter((node) => node.kind === "hypothesis")).toHaveLength(2);
	});

	it("exhausts a continued search round without discarding its evaluated lineage", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-continued-search-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_search_continue",
			objective: "break a tie between candidate methods",
			workspaceRoot: root,
		});
		const searchPlan = {
			schemaVersion: "astra.stage_plan_manifest.v1" as const,
			id: "plan_search_round_1",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "decision_search_round_1",
			mode: "search" as const,
			tasks: ["a", "b"].map((key) => ({
				key: `candidate_${key}`,
				objective: `test candidate ${key}`,
				hypothesis: `candidate ${key} is the most reliable`,
				inputArtifactRefs: [],
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: job.definitions.validation.acceptanceChecks,
				failureSignals: job.definitions.validation.failureSignals,
				successCriteria: job.definitions.validation.acceptanceChecks,
			})),
			rationale: "compare independent candidates",
			sessionRef: "pi-session:main",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(searchPlan);
		const pe = await preparePlanEvidence(job, searchPlan);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
		const firstBatch = Object.values(job.state.searchBatches)[0];
		if (!firstBatch) throw new Error("first search batch missing");

		for (const candidate of Object.values(firstBatch.candidates)) {
			const contract = buildEffectiveTaskContract(
				job,
				searchPlan,
				searchPlan.tasks.find((task) => task.key === candidate.key)!,
			);
			const task = await job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${searchPlan.id}:${candidate.key}`,
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: { candidate: candidate.id },
				refs: [`pi-session:${task.id}`],
			});
			const review = await job.recordReview(
				reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: "pass",
					findings: [],
					blocking: false,
					score: 1,
				}),
			);
			await job.recordCandidateEvaluation({
				batchId: firstBatch.id,
				candidateId: candidate.id,
				evidenceId: evidence.id,
				reviewId: review.id,
				verdict: "pass",
				score: 1,
				criteria: review.criteria ?? [],
				findings: [],
			});
		}

		await job.continueSearchBatch(firstBatch.id, "continue-round-1", "the frozen criteria remain tied");

		const exhausted = job.state.searchBatches[firstBatch.id];
		expect(exhausted).toMatchObject({
			status: "exhausted",
			round: 1,
			maxRounds: 2,
			decisionRef: "continue-round-1",
			continuationRationale: "the frozen criteria remain tied",
		});
		expect(Object.values(exhausted.candidates)).toHaveLength(2);
		expect(Object.values(job.state.evidence).filter((evidence) => evidence.type !== "stage-plan")).toHaveLength(2);
		expect(Object.values(job.state.reviews).filter((review) => review.evidenceId !== pe.id)).toHaveLength(2);
		expect(Object.values(job.state.candidateEvaluations)).toHaveLength(2);
		for (const candidate of Object.values(exhausted.candidates)) {
			expect(job.state.graph.nodes[candidate.graphNodeId]?.status).toBe("superseded");
		}
		expect(formatResearchBoard(buildResearchBoard(job.state))).toContain("[exhausted round 1/2]");

		await expect(
			job.recordStagePlan({
				...searchPlan,
				id: "plan_search_round_2_duplicate",
				decisionRef: "decision_search_round_2_duplicate",
			}),
		).rejects.toThrow("repeats a hypothesis");
		await job.recordStagePlan({
			...searchPlan,
			id: "plan_search_round_2",
			decisionRef: "decision_search_round_2",
			tasks: searchPlan.tasks.map((task, index) => ({
				...task,
				key: `orthogonal_${index + 1}`,
				hypothesis: `orthogonal discriminator ${index + 1} breaks the reliability tie`,
			})),
		});
		const secondBatch = Object.values(job.state.searchBatches).find((batch) => batch.id !== firstBatch.id);
		expect(secondBatch).toMatchObject({ round: 2, maxRounds: 2, previousBatchId: firstBatch.id });
	});

	it("recreates a missing search batch when a recorded plan is replayed after interruption", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_search_plan_replay",
			objective: "recover an interrupted search plan commit",
			workspaceRoot: "/workspace",
		});
		const plan = {
			schemaVersion: "astra.stage_plan_manifest.v1" as const,
			id: "plan_search_replay",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "decision_search_replay",
			mode: "search" as const,
			tasks: ["a", "b"].map((key) => ({
				key,
				objective: `test recovery candidate ${key}`,
				hypothesis: `recovery candidate ${key} is reliable`,
				inputArtifactRefs: [],
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: job.definitions.validation.acceptanceChecks,
				failureSignals: job.definitions.validation.failureSignals,
				successCriteria: job.definitions.validation.acceptanceChecks,
			})),
			rationale: "test durable plan replay",
			sessionRef: "pi-session:main",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(plan);
		const interrupted = job.state;
		interrupted.searchBatches = {};
		await store.writeSnapshot(interrupted);
		const reopened = await ResearchJob.open(store, job.state.frame.jobId);
		if (!reopened) throw new Error("reopened job missing");

		await reopened.recordStagePlan(plan);

		expect(Object.values(reopened.state.searchBatches)).toHaveLength(1);
		expect(Object.values(reopened.state.searchBatches)[0]).toMatchObject({ planId: plan.id, round: 1 });
	});

	it("reopens an upstream stage and invalidates every dependent downstream artifact", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_backtrack",
			objective: "reconsider invalid evidence",
			workspaceRoot: "/workspace",
		});
		for (const stageId of ["validation", "literature", "idea"]) {
			job.state.stages[stageId].status;
		}

		await job.reopenStage("validation", "route_invalid_evidence", "review found a broken premise");

		expect(job.state.stages.validation.status).toBe("running");
		expect(job.state.stages.literature.status).toBe("pending");
		expect(job.state.stages.idea.status).toBe("pending");
		expect(job.state.graph.unresolvedObjectionIds).toHaveLength(1);
		expect(job.state.frame.nextAction).toContain("revisit validation");
	});

	it("uses one durable Pi session id for every main-agent round", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-persistent-main-"));
		tempRoots.push(root);
		const launcherPath = join(root, "capture-args.mjs");
		await writeFile(
			launcherPath,
			'console.log(JSON.stringify({ type: "captured_args", args: process.argv.slice(2) }));\n',
			"utf8",
		);
		const runner = new PiChildSessionRunner({ launcherPath, sessionDir: join(root, "sessions") });

		const first = await runner.run(root, "job_main", "plan_validation", 1, "main-agent", "plan");
		const second = await runner.run(root, "job_main", "decision_validation", 1, "main-agent", "decide");
		const sessionId = (result: typeof first): string => {
			const event = result.jsonEvents[0] as { args: string[] };
			return event.args[event.args.indexOf("--session-id") + 1] ?? "";
		};

		expect(sessionId(first)).toBe("astra-job_main-main");
		expect(sessionId(second)).toBe(sessionId(first));
	});

	it("streams large Pi child prompts through stdin without truncation", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-prompt-stdin-"));
		tempRoots.push(root);
		const launcherPath = join(root, "capture-stdin.mjs");
		await writeFile(
			launcherPath,
			[
				'let input = "";',
				'process.stdin.setEncoding("utf8");',
				"for await (const chunk of process.stdin) input += chunk;",
				'console.log(JSON.stringify({ type: "captured_stdin", length: input.length, tail: input.slice(-32), args: process.argv.slice(2) }));',
				"",
			].join("\n"),
			"utf8",
		);
		const runner = new PiChildSessionRunner({ launcherPath, sessionDir: join(root, "sessions") });
		const prompt = `${"route-context ".repeat(16_000)}sentinel`;

		const result = await runner.run(root, "job_large_prompt", "route", 1, "main-agent", prompt);
		const captured = result.jsonEvents[0] as { length: number; tail: string; args: string[] };

		expect(result.exitCode).toBe(0);
		expect(captured.length).toBe(prompt.length);
		expect(captured.tail).toContain("sentinel");
		expect(captured.args).not.toContain(prompt);
	});

	it("keeps stages as capabilities and changes stage only through an explicit route decision", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-dynamic-route-"));
		tempRoots.push(root);
		expect(DEFAULT_STAGES.every((stage) => !("dependsOn" in stage))).toBe(true);
		expect(DEFAULT_STAGES.every((stage) => !("inputArtifactTypes" in stage))).toBe(true);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_dynamic_route",
			objective: "choose a route from evidence",
			workspaceRoot: root,
		});
		const artifactId = await recordPassingArtifact(job, "validation", {
			researchQuestion: "Which method is reliable?",
			scope: ["one benchmark"],
			nonGoals: [],
			acceptanceCriteria: ["reproduces"],
			falsifiableNextStep: "compare methods",
		});

		expect(job.state.frame.activeStageId).toBe("validation");
		await job.applyRouteDecision({
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: "route_validation_literature",
			jobId: job.state.frame.jobId,
			decisionType: "route",
			decisionRef: "route-validation-literature",
			stageId: "validation",
			routeAction: "advance",
			targetStageId: "literature",
			evidenceRefs: [artifactId],
			rationale: "literature evidence is the highest-value next action",
			sessionRef: "pi-session:main",
			createdAt: new Date().toISOString(),
		});

		expect(job.state.stages.validation.status).toBe("completed");
		expect(job.state.stages.literature.status).toBe("running");
		expect(job.state.frame.activeStageId).toBe("literature");
	});

	it("selects only an independently evaluated search winner and prunes loser workspaces", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-search-cleanup-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_search_cleanup",
			objective: "compare two methods",
			workspaceRoot: root,
		});
		await job.recordStagePlan({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_cleanup_search",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan-cleanup-search",
			mode: "search",
			tasks: ["a", "b"].map((key) => ({
				key,
				objective: `evaluate method ${key}`,
				hypothesis: `method ${key} is reliable`,
				inputArtifactRefs: [],
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: job.definitions.validation.acceptanceChecks,
				failureSignals: job.definitions.validation.failureSignals,
				successCriteria: job.definitions.validation.acceptanceChecks,
			})),
			rationale: "compare before promotion",
			sessionRef: "pi-session:main",
			createdAt: new Date().toISOString(),
		});
		const plan = job.state.stagePlans.plan_cleanup_search;
		const pe = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
		const batch = Object.values(job.state.searchBatches)[0];
		if (!batch) throw new Error("search batch missing");
		const candidates = Object.values(batch.candidates);
		for (const [index, candidate] of candidates.entries()) {
			const contract = buildEffectiveTaskContract(job, plan, plan.tasks.find((task) => task.key === candidate.key)!);
			const task = await job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:${candidate.key}`,
			});
			await job.setTaskStatus(task.id, "running");
			await job.setTaskStatus(task.id, "succeeded");
			const workspace = join(root, ".astra", "jobs", job.state.frame.jobId, "workspaces", task.id);
			await mkdir(workspace, { recursive: true });
			await writeFile(join(workspace, "candidate.txt"), candidate.hypothesis, "utf8");
			const resourceRoot = taskResourcePath(root, job.state.frame.jobId, task.id);
			await mkdir(resourceRoot, { recursive: true });
			await writeFile(join(resourceRoot, "environment.txt"), candidate.hypothesis, "utf8");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				content: { candidate: candidate.id },
				refs: ["candidate.txt"],
			});
			const review = await job.recordReview(
				reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: "pass",
					findings: [],
					blocking: false,
					score: index === 0 ? 0.95 : 0.85,
				}),
			);
			if (index === 0) {
				await expect(
					job.recordCandidateEvaluation({
						batchId: batch.id,
						candidateId: candidate.id,
						evidenceId: evidence.id,
						reviewId: review.id,
						verdict: "pass",
						score: 0.95,
						criteria: [],
						findings: [],
					}),
				).rejects.toThrow("frozen criteria");
			}
			await job.recordCandidateEvaluation({
				batchId: batch.id,
				candidateId: candidate.id,
				evidenceId: evidence.id,
				reviewId: review.id,
				verdict: "pass",
				score: index === 0 ? 0.95 : 0.85,
				criteria: review.criteria ?? [],
				findings: [],
			});
		}

		const loserTaskId = job.state.searchBatches[batch.id]?.candidates[candidates[1]?.id ?? ""]?.taskId;
		await job.selectSearchCandidate(batch.id, candidates[0]?.id ?? "", "select-best-candidate");

		expect(job.state.searchBatches[batch.id]?.selectedCandidateId).toBe(candidates[0]?.id);
		expect(Object.values(job.state.evidence).filter((evidence) => evidence.type !== "stage-plan")).toHaveLength(1);
		expect(Object.values(job.state.discardedCandidates)).toHaveLength(1);
		if (!loserTaskId) throw new Error("loser task missing");
		await expect(
			access(join(root, ".astra", "jobs", job.state.frame.jobId, "workspaces", loserTaskId)),
		).rejects.toThrow();
		await expect(access(taskResourcePath(root, job.state.frame.jobId, loserTaskId))).rejects.toThrow();
		await expect(
			access(join(root, ".astra", "jobs", job.state.frame.jobId, "archive", "tasks", loserTaskId)),
		).resolves.toBeUndefined();
		await expect(
			access(
				join(
					root,
					".astra",
					"jobs",
					job.state.frame.jobId,
					"archive",
					"tasks",
					loserTaskId,
					"resources",
					"environment.txt",
				),
			),
		).resolves.toBeUndefined();
		expect(Object.values(job.state.discardedCandidates)[0]?.archiveRefs).toEqual([
			join(root, ".astra", "jobs", job.state.frame.jobId, "archive", "tasks", loserTaskId),
		]);
	});

	it("does not allow a negative or incomplete final review to terminate research", async () => {
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_completion_gate",
			objective: "produce a defensible result",
			workspaceRoot: "/workspace",
		});

		expect(job.completionBlockers()).toEqual(
			expect.arrayContaining(["scientific outcome has not been recorded", "no passing whole-research review"]),
		);
	});

	it("accepts a final review that passes with nonblocking caveats and has no required repairs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-completion-caveats-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_completion_caveats",
			objective: "complete only after a bounded whole-research review",
			workspaceRoot: root,
		});
		await recordPassingArtifact(
			job,
			"result-to-claim",
			{
				scientificOutcome: "partially-supported",
				missionCoverage: "sufficient",
				claims: [
					{
						statement: "The bounded fixture supports one narrow traceability observation.",
						assessment: "supported",
					},
				],
				supportingResults: ["fixture result"],
				unsupportedClaims: ["broad reproducibility"],
				missingEvidence: ["independent rerun"],
				conclusion: "bounded evidence only",
			},
			[],
		);
		await recordPassingArtifact(job, "research-review", {
			verdict: "pass_with_nonblocking_caveats",
			scientificOutcome: "partially-supported",
			missionCoverage: "sufficient",
			strengths: ["claims remain bounded"],
			weaknesses: ["the pilot is small"],
			claimAudit: ["broad claims remain withheld"],
			requiredRepairs: [],
		});

		expect(job.completionBlockers()).not.toContain("no passing whole-research review");
	});

	it("backtracks by artifact lineage and physically prunes only obsolete route materializations", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-lineage-backtrack-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_lineage_backtrack",
			objective: "invalidate only conclusions that depend on a broken premise",
			workspaceRoot: root,
		});
		const validationId = await recordPassingArtifact(
			job,
			"validation",
			{
				researchQuestion: "Is the premise sound?",
				scope: ["fixture"],
				nonGoals: [],
				acceptanceCriteria: ["falsifiable"],
				falsifiableNextStep: "audit literature",
			},
			[],
		);
		await job.applyRouteDecision({
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: "route_to_literature",
			jobId: job.state.frame.jobId,
			decisionType: "route",
			decisionRef: "route-to-literature",
			stageId: "validation",
			routeAction: "advance",
			targetStageId: "literature",
			evidenceRefs: [validationId],
			rationale: "inspect prior work",
			sessionRef: "pi-session:main",
			createdAt: new Date().toISOString(),
		});
		const literatureId = await recordPassingArtifact(
			job,
			"literature",
			{
				queryStrategy: ["fixture"],
				sources: ["openalex:W1", "openalex:W2", "openalex:W3"],
				closestWork: [],
				gaps: ["gap"],
				synthesis: "depends on validation",
			},
			[validationId],
		);
		const independentId = await recordPassingArtifact(
			job,
			"monitor",
			{
				runStatus: "independent",
				metrics: [],
				anomalies: [],
				integrityChecks: [],
				decisions: [],
			},
			[],
		);
		const obsoletePaths = [validationId, literatureId].map(
			(id) => job.state.canonical[id]?.materializationRef ?? "missing",
		);

		await job.reopenStage("validation", "backtrack-broken-premise", "the premise failed reproduction");

		expect(job.state.canonical[validationId]).toBeUndefined();
		expect(job.state.canonical[literatureId]).toBeUndefined();
		expect(job.state.canonical[independentId]?.status).toBe("active");
		expect(Object.keys(job.state.retiredArtifacts)).toEqual(expect.arrayContaining([validationId, literatureId]));
		for (const path of obsoletePaths) await expect(access(path)).rejects.toThrow();
	});
});
