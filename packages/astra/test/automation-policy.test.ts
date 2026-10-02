import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { costUsdFromJsonEvents } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import type { AstraStore } from "../src/store.ts";
import { JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";
import {
	NonRetryableResearchError,
	ProviderCapacityError,
	type ResearchMainAgentAdapter,
	ResearchSupervisor,
	type ResearchWorkerAdapter,
} from "../src/supervisor.ts";
import type { MainAgentDecisionManifest, StageDefinition } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

let workspaceRoot: string;
beforeEach(async () => {
	workspaceRoot = await mkdtemp(join(tmpdir(), "astra-flow-fixture-"));
});
afterEach(async () => {
	await rm(workspaceRoot, { recursive: true, force: true });
});

const validationStage: StageDefinition = {
	id: "validation",
	label: "Validate",
	suggestedInputArtifactTypes: [],
	outputArtifactType: "validation",
	requiredOutputFields: ["content"],
	acceptanceChecks: ["structured"],
	failureSignals: ["missing content"],
	workerTaskFamily: "validation",
	workerTools: ["read"],
	workspaceWrite: false,
	minSourceRefs: 0,
	gate: "main-agent",
};

function mainAgent() {
	return {
		planStage: vi.fn<ResearchMainAgentAdapter["planStage"]>(async (job, obligation) => {
			const stageId = job.state.frame.activeStageId;
			const definition = job.definitions[stageId];
			return {
				schemaVersion: "astra.stage_plan_manifest.v1" as const,
				id: `plan-${stageId}-${job.state.eventSeq}`,
				jobId: job.state.frame.jobId,
				stageId,
				decisionRef: `plan-${stageId}-${job.state.eventSeq}`,
				tasks: [
					{
						key: obligation ? "repair" : "primary",
						objective: obligation ? `Repair ${obligation.description}` : `Produce ${stageId} evidence`,
						inputArtifactRefs: [
							...Object.values(job.state.canonical)
								.filter((artifact) => artifact.status === "active")
								.map((artifact) => artifact.id),
							...(obligation
								? Object.values(job.state.evidence)
										.filter((evidence) => evidence.stageId === stageId)
										.map((evidence) => evidence.id)
								: []),
						],
						requiredOutputFields: definition.requiredOutputFields,
						acceptanceChecks: definition.acceptanceChecks,
						failureSignals: definition.failureSignals,
						successCriteria: definition.acceptanceChecks,
					},
				],
				rationale: "fixture stage plan",
				sessionRef: "fixture:main",
				...(obligation ? { obligationId: obligation.id } : {}),
				createdAt: new Date().toISOString(),
			};
		}),
		decideEvidence: vi.fn(
			async (evidence, job) =>
				({
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `accept-${evidence.id}`,
					jobId: job.state.frame.jobId,
					decisionType: "evidence",
					decisionRef: `accept-${evidence.id}`,
					evidenceId: evidence.id,
					decision: "accept",
					rationale: "fixture acceptance",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				}) satisfies MainAgentDecisionManifest,
		),
		decideAdoption: vi.fn(
			async (evidence, job) =>
				({
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `adopt-${evidence.id}`,
					jobId: job.state.frame.jobId,
					decisionType: "adoption",
					decisionRef: `adopt-${evidence.id}`,
					evidenceId: evidence.id,
					adopt: true,
					rationale: "fixture adoption",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				}) satisfies MainAgentDecisionManifest,
		),
		decideSearch: vi.fn(
			async (batch, evaluations, job) =>
				({
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `select-${batch.id}`,
					jobId: job.state.frame.jobId,
					decisionType: "search-selection",
					decisionRef: `select-${batch.id}`,
					searchBatchId: batch.id,
					selectedCandidateId: evaluations[0]?.candidateId,
					rationale: "fixture selection",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				}) satisfies MainAgentDecisionManifest,
		),
		decideRoute: vi.fn<ResearchMainAgentAdapter["decideRoute"]>(
			async (job) =>
				({
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `route-${job.state.frame.activeStageId}`,
					jobId: job.state.frame.jobId,
					decisionType: "route",
					decisionRef: `route-${job.state.frame.activeStageId}`,
					stageId: job.state.frame.activeStageId,
					routeAction:
						job.state.frame.activeStageId === "validation" && job.definitions["research-review"]
							? "advance"
							: "continue",
					targetStageId:
						job.state.frame.activeStageId === "validation" && job.definitions["research-review"]
							? "research-review"
							: undefined,
					evidenceRefs: Object.values(job.state.canonicalRoute.stageArtifactIds),
					rationale: "fixture route",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				}) satisfies MainAgentDecisionManifest,
		),
	};
}

function createSupervisor(
	job: ResearchJob,
	store: AstraStore,
	workerRun?: ResearchWorkerAdapter["run"],
	maxParallel = 1,
) {
	const decisions = mainAgent();
	const run = vi.fn<ResearchWorkerAdapter["run"]>(
		workerRun ??
			(async (task) => ({
				artifactType: task.requiredOutputType,
				content: { taskId: task.id },
				refs: [`fixture:${task.id}`],
			})),
	);
	const worker = {
		run,
	};
	const reviewer = {
		review: vi.fn(async (evidence) => reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] })),
	};
	return {
		decisions,
		worker,
		reviewer,
		supervisor: new ResearchSupervisor(job, store, {
			owner: `supervisor-${job.state.frame.jobId}`,
			maxParallel,
			worker,
			reviewer,
			mainAgent: decisions,
		}),
	};
}

describe("research automation policy", () => {
	it("counts only authoritative assistant message_end costs from Pi JSON output", () => {
		expect(
			costUsdFromJsonEvents([
				{ type: "message_end", message: { role: "assistant", usage: { cost: { total: 0.125 } } } },
				{ type: "turn_end", message: { role: "assistant", usage: { cost: { total: 0.125 } } } },
				{ type: "message_end", message: { role: "toolResult", usage: { cost: { total: 9 } } } },
				{ type: "message_end", message: { role: "assistant", usage: { cost: { total: 0.375 } } } },
			]),
		).toBe(0.5);
	});

	it("continues one tied search round and converges by selecting from the final round", async () => {
		const searchStage: StageDefinition = {
			...validationStage,
			searchPolicy: {
				strategy: "diverse-candidates",
				minCandidates: 2,
				maxCandidates: 2,
				maxRounds: 2,
				criteria: ["structured"],
			},
			qualityPolicy: { minPassingReviews: 1, minScore: 0.8, requireResolvableArtifacts: true },
		};
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_bounded_search",
			objective: "select a reviewed candidate",
			workspaceRoot,
			automation: "full",
			definitions: [searchStage],
		});
		const decisions = mainAgent();
		decisions.planStage.mockImplementation(async (currentJob, _obligation, requestedMode) => {
			const round = Object.keys(currentJob.state.searchBatches).length + 1;
			return {
				schemaVersion: "astra.stage_plan_manifest.v1",
				id: `plan-search-${round}`,
				jobId: currentJob.state.frame.jobId,
				stageId: "validation",
				decisionRef: `plan-search-${round}`,
				mode: requestedMode,
				tasks: [1, 2].map((index) => ({
					key: `round-${round}-candidate-${index}`,
					objective: `evaluate round ${round} candidate ${index}`,
					hypothesis: `round ${round} candidate ${index} uses discriminator ${round}-${index}`,
					inputArtifactRefs: [],
					requiredOutputFields: searchStage.requiredOutputFields,
					acceptanceChecks: searchStage.acceptanceChecks,
					failureSignals: searchStage.failureSignals,
					successCriteria: searchStage.acceptanceChecks,
				})),
				rationale: `bounded search round ${round}`,
				sessionRef: "fixture:main",
				createdAt: new Date().toISOString(),
			};
		});
		const decideSearch = vi
			.fn<ResearchMainAgentAdapter["decideSearch"]>()
			.mockImplementationOnce(async (batch, _evaluations, currentJob) => ({
				schemaVersion: "astra.main_agent_decision_manifest.v1",
				manifestId: `continue-${batch.id}`,
				jobId: currentJob.state.frame.jobId,
				decisionType: "search-selection",
				decisionRef: `continue-${batch.id}`,
				searchBatchId: batch.id,
				continueSearch: true,
				rationale: "round one remains tied",
				sessionRef: "fixture:main",
				createdAt: new Date().toISOString(),
			}))
			.mockImplementationOnce(async (batch, evaluations, currentJob) => ({
				schemaVersion: "astra.main_agent_decision_manifest.v1",
				manifestId: `select-${batch.id}`,
				jobId: currentJob.state.frame.jobId,
				decisionType: "search-selection",
				decisionRef: `select-${batch.id}`,
				searchBatchId: batch.id,
				selectedCandidateId: evaluations[0]?.candidateId,
				rationale: "final-round deterministic tie break",
				sessionRef: "fixture:main",
				createdAt: new Date().toISOString(),
			}));
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "supervisor-bounded-search",
			maxParallel: 2,
			worker: {
				run: async (task) => ({ artifactType: task.requiredOutputType, content: { content: task.id }, refs: [] }),
			},
			reviewer: {
				review: async (evidence) =>
					evidence.type === "stage-plan"
						? reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] })
						: {
								verdict: "pass",
								findings: [],
								score: 1,
								criteria: [
									{
										criterion: "structured",
										passed: true,
										score: 1,
										evidenceRefs: [`evidence:${evidence.id}`],
										rationale: "verified",
									},
								],
								verifiedRefs: [`evidence:${evidence.id}`],
							},
			},
			mainAgent: { ...decisions, decideSearch },
		});

		await supervisor.tick();
		expect(Object.values(job.state.searchBatches)).toEqual([
			expect.objectContaining({ round: 1, maxRounds: 2, status: "exhausted" }),
		]);

		await supervisor.tick();

		const batches = Object.values(job.state.searchBatches).sort((left, right) => left.round - right.round);
		expect(batches).toHaveLength(2);
		expect(batches[0]).toMatchObject({ round: 1, status: "exhausted" });
		expect(batches[1]).toMatchObject({ round: 2, status: "selected", previousBatchId: batches[0]?.id });
		expect(decisions.planStage).toHaveBeenCalledTimes(2);
		expect(decideSearch).toHaveBeenCalledTimes(2);
		expect(job.state.canonicalRoute.selectedCandidateIds.validation).toBe(batches[1]?.selectedCandidateId);
	});

	it("runs routine collaborative work and pauses only for a main-agent research question", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_collaborative",
			objective: "collaborative fixture",
			workspaceRoot,
			automation: "collaborative",
			definitions: [validationStage],
		});
		const fixture = createSupervisor(job, store);
		fixture.decisions.decideRoute.mockImplementation(async (currentJob) => ({
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: "ask-user-route",
			jobId: currentJob.state.frame.jobId,
			decisionType: "route",
			decisionRef: "ask-user-route",
			stageId: "validation",
			routeAction: "ask-user",
			question: "Should reproducibility or peak accuracy dominate?",
			evidenceRefs: Object.values(currentJob.state.canonicalRoute.stageArtifactIds),
			rationale: "the preference changes candidate ranking",
			sessionRef: "fixture:main",
			createdAt: new Date().toISOString(),
		}));

		await fixture.supervisor.tick();

		expect(fixture.worker.run).toHaveBeenCalledOnce();
		expect(fixture.decisions.decideRoute).toHaveBeenCalledOnce();
		expect(job.status().paused).toBe(true);
		expect(job.status().userGate).toMatchObject({
			kind: "research",
			question: "Should reproducibility or peak accuracy dominate?",
		});
		await expect(job.resume()).rejects.toThrow("requires user guidance");
		await job.recordUserGuidance("Prioritize reproducibility on one GPU");
		expect(job.status().paused).toBe(false);
		expect(job.state.stages.validation.lastRouteAction).toBe("ask-user");
	});

	it("persists a user gate and its approval across job reopen", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-policy-gate-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				jobId: "job_durable_gate",
				objective: "durable collaborative gate",
				workspaceRoot: root,
				automation: "collaborative",
			});
			await job.requireUserGate({
				kind: "research",
				stageId: "validation",
				question: "Which risk should dominate?",
				reason: "the route depends on user preference",
			});

			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			expect(reopened?.status().automation).toBe("collaborative");
			expect(reopened?.status().userGate).toMatchObject({ kind: "research", stageId: "validation" });

			await expect(reopened?.resume()).rejects.toThrow("requires user guidance");
			await reopened?.recordUserGuidance("Prefer safety over speed");
			const approved = await ResearchJob.open(store, job.state.frame.jobId);
			expect(approved?.status().userGate).toBeUndefined();
			expect(approved?.status().paused).toBe(false);
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("stops autonomous mode at a user-gated stage while full mode completes it", async () => {
		const definitions: StageDefinition[] = [
			validationStage,
			{
				...validationStage,
				id: "research-review",
				label: "Research review",
				suggestedInputArtifactTypes: ["validation"],
				outputArtifactType: "research-review",
				workerTaskFamily: "research-review",
				gate: "user",
			},
		];
		for (const automation of ["autonomous", "full"] as const) {
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				jobId: `job_${automation}`,
				objective: `${automation} fixture`,
				workspaceRoot,
				automation,
				definitions,
			});
			const fixture = createSupervisor(job, store, undefined, 2);

			await fixture.supervisor.tick();
			await fixture.supervisor.tick();

			if (automation === "autonomous") {
				expect(job.status().userGate).toMatchObject({
					stageId: "research-review",
					phase: "route",
				});
				expect(job.state.stages["research-review"].status).toBe("running");
				const canonicalCount = Object.keys(job.state.canonical).length;
				await job.resume();
				await fixture.supervisor.tick();
				expect(Object.keys(job.state.canonical)).toHaveLength(canonicalCount);
				expect(job.state.stages["research-review"].lastRouteAction).toBe("continue");
			} else {
				expect(job.status().userGate).toBeUndefined();
				expect(job.state.stages["research-review"].lastRouteAction).toBe("continue");
			}
		}
	});

	it("enforces the global turn budget and resumes only after the budget is raised", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_turn_budget",
			objective: "turn budget fixture",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
			maxTurns: 2,
		});
		const fixture = createSupervisor(job, store);

		await fixture.supervisor.tick();

		expect(fixture.worker.run).not.toHaveBeenCalled();
		expect(fixture.decisions.decideEvidence).not.toHaveBeenCalled();
		expect(job.status().budget.turnsUsed).toBe(2);
		expect(job.status().userGate).toMatchObject({ kind: "budget", limit: "maxTurns" });
		await expect(job.resume()).rejects.toThrow("increase maxTurns");

		await job.updateBudget({ maxTurns: 7 });
		await job.resume();
		await fixture.supervisor.tick();

		expect(job.state.stages.validation.lastRouteAction).toBe("continue");
		expect(job.status().budget.turnsUsed).toBe(7);
	});

	it("does not create a reviewer TaskPacket after the global task budget is exhausted", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_task_budget",
			objective: "task budget fixture",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
			maxTasks: 1,
		});
		const fixture = createSupervisor(job, store);

		await fixture.supervisor.tick();

		expect(Object.keys(job.state.tasks)).toHaveLength(0);
		expect(fixture.reviewer.review).not.toHaveBeenCalled();
		expect(job.status().userGate).toMatchObject({ kind: "budget", limit: "maxTasks" });

		await job.updateBudget({ maxTasks: 3 });
		await job.resume();
		await fixture.supervisor.tick();

		expect(fixture.reviewer.review).toHaveBeenCalledTimes(2);
		expect(Object.keys(job.state.tasks)).toHaveLength(2);
		expect(job.state.stages.validation.lastRouteAction).toBe("continue");
	});

	it.each([false, true])(
		"retries only an incomplete reviewer without a manifest (completed=%s)",
		async (completed) => {
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				jobId: "job_review_commit_retry",
				objective: "retry a review that the durable ledger rejects",
				workspaceRoot,
				automation: "full",
				definitions: [validationStage],
			});
			const workerTask = await job.dispatchTask({
				stageId: "validation",
				stageExecutionId: "stage_exec_validation",
				role: "worker",
				objective: "produce validation evidence",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: ["content"],
				acceptanceChecks: ["structured"],
				failureSignals: ["missing content"],
				dependencies: [],
				scope: { workspaceRoot, allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 30_000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["structured"],
			});
			await job.setTaskStatus(workerTask.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: workerTask.id,
				stageId: "validation",
				type: "validation",
				content: { content: "complete" },
				refs: [],
			});
			const reviewerTaskIds: string[] = [];
			const reviewer = {
				review: vi.fn(async () => {
					const reviewerTask = await job.dispatchTask({
						stageId: "validation",
						stageExecutionId: "stage_exec_validation",
						role: "reviewer",
						objective: `review ${evidence.id}`,
						inputArtifactRefs: [evidence.id],
						requiredCanonicalArtifacts: [],
						requiredOutputType: "review",
						requiredOutputFields: ["verdict", "findings"],
						acceptanceChecks: ["review manifest written"],
						failureSignals: ["missing review manifest"],
						dependencies: [],
						scope: { workspaceRoot, allowedPaths: ["."] },
						allowedTools: ["read"],
						writeAuthority: "none",
						budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 30_000 },
						reviewGateRequired: false,
						resumePolicy: "resume-session",
						successCriteria: ["review manifest written"],
					});
					await job.setTaskStatus(
						reviewerTask.id,
						completed || reviewerTaskIds.length > 0 ? "succeeded" : "running",
					);
					reviewerTaskIds.push(reviewerTask.id);
					const exact = reviewerTaskIds.length > 1;
					return {
						verdict: "pass" as const,
						findings: [],
						reviewerTaskId: reviewerTask.id,
						score: 1,
						criteria: [
							{
								criterion: exact ? "structured" : "Structured",
								passed: true,
								score: 1,
								evidenceRefs: [`evidence:${evidence.id}`],
								rationale: "verified",
							},
						],
						verifiedRefs: [`evidence:${evidence.id}`],
					};
				}),
			};
			const supervisor = new ResearchSupervisor(job, store, {
				owner: "supervisor-review-commit-retry",
				maxParallel: 1,
				worker: { run: vi.fn() },
				reviewer,
				mainAgent: mainAgent(),
			});

			if (completed) {
				await expect(supervisor.tick()).rejects.toThrow("completion gap: no review manifest");
				expect(job.state.tasks[reviewerTaskIds[0]]?.status).toBe("succeeded");
				expect(Object.keys(job.state.reviews)).toHaveLength(0);
				const turns = job.status().budget.turnsUsed;
				await expect(supervisor.tick()).rejects.toThrow("completion gap: no review manifest");
				expect(reviewer.review).toHaveBeenCalledOnce();
				expect(reviewerTaskIds).toHaveLength(1);
				expect(job.status().budget.turnsUsed).toBe(turns);
				expect(job.state.paused).toBe(false);
			} else {
				await supervisor.tick();
				expect(job.state.tasks[reviewerTaskIds[0]]?.status).toBe("failed");
				expect(Object.keys(job.state.reviews)).toHaveLength(0);
				await supervisor.tick();
				expect(reviewerTaskIds[1]).not.toBe(reviewerTaskIds[0]);
				expect(job.state.tasks[reviewerTaskIds[1]]?.status).toBe("succeeded");
				expect(Object.values(job.state.reviews)).toHaveLength(1);
				await expect(job.failUncommittedReviewerTask(reviewerTaskIds[1])).rejects.toThrow("committed");
			}
			await expect(job.failUncommittedReviewerTask(workerTask.id)).rejects.toThrow("only reviewer tasks");
		},
	);

	it("resumes an already-dispatched worker TaskPacket before creating another wave", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_ready_recovery",
			objective: "ready task recovery",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
		});
		await job.consumeTurns(1);
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "stage_exec_validation",
			role: "worker",
			objective: "validation worker wave 1",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content", "outputRefs"],
			acceptanceChecks: ["structured"],
			failureSignals: ["missing output manifest"],
			dependencies: [],
			scope: { workspaceRoot, allowedPaths: [".astra/jobs/job_ready_recovery/tasks"] },
			allowedTools: ["read"],
			writeAuthority: "workspace-write",
			budget: { maxTurns: 8, maxToolCalls: 16, maxRuntimeMs: 300_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["structured"],
		});
		const fixture = createSupervisor(job, store);

		const tick = await fixture.supervisor.tick();

		expect(tick.dispatchedTaskIds).toEqual([]);
		expect(fixture.worker.run).toHaveBeenCalledWith(expect.objectContaining({ id: task.id }), job);
		expect(Object.keys(job.state.tasks)).toHaveLength(1);
		expect(job.state.stages.validation.lastRouteAction).toBe("continue");
	});

	it("keeps repair evidence in the failed review evidence set when the plan uses only canonical refs", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_repair_evidence_set",
			objective: "repair a failed review",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
		});
		const decisions = mainAgent();
		decisions.planStage.mockImplementation(async (currentJob, obligation) => ({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: `plan-validation-${currentJob.state.eventSeq}`,
			jobId: currentJob.state.frame.jobId,
			stageId: "validation",
			decisionRef: `plan-validation-${currentJob.state.eventSeq}`,
			tasks: [
				{
					key: obligation ? "repair" : "primary",
					objective: obligation ? "Repair the failed validation evidence" : "Produce validation evidence",
					inputArtifactRefs: [],
					requiredOutputFields: validationStage.requiredOutputFields,
					acceptanceChecks: validationStage.acceptanceChecks,
					failureSignals: validationStage.failureSignals,
					successCriteria: validationStage.acceptanceChecks,
				},
			],
			rationale: "fixture repair plan without the failed evidence ref",
			sessionRef: "fixture:main",
			...(obligation ? { obligationId: obligation.id } : {}),
			createdAt: new Date().toISOString(),
		}));
		const reviewer = {
			review: vi.fn(async (evidence) => {
				const fail = evidence.type !== "stage-plan" && Object.keys(job.state.obligations).length === 0;
				return reviewFixture(job, {
					evidenceId: evidence.id,
					verdict: fail ? "fail" : "pass",
					findings: fail ? ["repair this evidence"] : [],
				});
			}),
		};
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "supervisor-repair-evidence-set",
			maxParallel: 1,
			worker: {
				run: async (task) => ({ artifactType: task.requiredOutputType, content: { content: task.id }, refs: [] }),
			},
			reviewer,
			mainAgent: decisions,
		});

		await supervisor.tick();
		expect(job.state.frame.openObligationIds).toHaveLength(1);

		await supervisor.tick();

		expect(reviewer.review).toHaveBeenCalledTimes(4);
		expect(job.state.frame.openObligationIds).toHaveLength(0);
		expect(job.state.stages.validation.lastRouteAction).toBe("continue");
	});

	it("pauses after a Pi child call crosses the global cost budget", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_cost_budget",
			objective: "cost budget fixture",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
			maxCostUsd: 0.1,
		});
		const workerRun = vi.fn(async (task) => {
			await job.recordCost(0.25);
			return {
				artifactType: task.requiredOutputType,
				content: { taskId: task.id },
				refs: [`fixture:${task.id}`],
			};
		});
		const fixture = createSupervisor(job, store, workerRun);

		await fixture.supervisor.tick();

		expect(job.status().budget.costUsdUsed).toBe(0.25);
		expect(job.status().userGate).toMatchObject({ kind: "budget", limit: "maxCostUsd" });
		expect(fixture.decisions.decideEvidence).not.toHaveBeenCalled();
	});

	it("pauses instead of retrying a non-retryable provider configuration failure", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_provider_auth_failure",
			objective: "surface provider authentication failure",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
		});
		const fixture = createSupervisor(job, store, async () => {
			throw new NonRetryableResearchError("provider authentication failed with 401 invalid_api_key");
		});

		await fixture.supervisor.tick();

		expect(job.status().paused).toBe(true);
		expect(job.status().nextAction).toContain("provider authentication failed");
		await fixture.supervisor.tick();
		expect(fixture.worker.run).toHaveBeenCalledOnce();
	});

	it("persists provider backoff and retries the same scientific task after capacity recovers", async () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date("2026-08-21T00:00:00.000Z"));
		const root = await mkdtemp(join(tmpdir(), "astra-provider-backoff-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				jobId: "job_provider_capacity",
				objective: "wait for temporary provider capacity",
				workspaceRoot: root,
				automation: "full",
				definitions: [validationStage],
			});
			const workerRun = vi
				.fn<ResearchWorkerAdapter["run"]>()
				.mockRejectedValueOnce(new ProviderCapacityError("HTTP 429 rate limit exceeded"))
				.mockRejectedValueOnce(new ProviderCapacityError("servers are currently overloaded"))
				.mockImplementation(async (task) => ({
					artifactType: task.requiredOutputType,
					content: { taskId: task.id },
					refs: [`fixture:${task.id}`],
				}));
			const first = createSupervisor(job, store, workerRun);

			await first.supervisor.tick();

			const originalTask = Object.values(job.state.tasks).find((task) => task.role === "worker")!;
			const firstBackoff = job.state.providerBackoff;
			expect(job.status().paused).toBe(false);
			expect(firstBackoff).toMatchObject({ attempt: 1, reason: "HTTP 429 rate limit exceeded" });
			expect(originalTask).toMatchObject({ status: "ready", attempt: 1 });
			expect(workerRun).toHaveBeenCalledOnce();

			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			expect(reopened?.state.providerBackoff).toEqual(firstBackoff);
			if (!reopened || !firstBackoff) throw new Error("expected persisted provider backoff");
			const resumed = createSupervisor(reopened, store, workerRun);

			await resumed.supervisor.tick();
			expect(workerRun).toHaveBeenCalledOnce();

			const firstDelay = Date.parse(firstBackoff.retryAt) - Date.now();
			vi.setSystemTime(new Date(firstBackoff.retryAt));
			await resumed.supervisor.tick();
			const secondBackoff = reopened.state.providerBackoff;
			expect(secondBackoff?.attempt).toBe(2);
			expect(Date.parse(secondBackoff?.retryAt ?? "") - Date.now()).toBe(firstDelay * 2);
			expect(reopened.state.tasks[originalTask.id]).toMatchObject({ status: "ready", attempt: 1 });

			vi.setSystemTime(new Date(secondBackoff?.retryAt ?? ""));
			await resumed.supervisor.tick();

			expect(workerRun).toHaveBeenCalledTimes(3);
			expect(Object.values(reopened.state.tasks).filter((task) => task.role === "worker")).toEqual([
				expect.objectContaining({ id: originalTask.id, status: "succeeded", attempt: 1 }),
			]);
			expect(reopened.state.providerBackoff).toBeUndefined();
			expect(reopened.status().paused).toBe(false);
		} finally {
			vi.useRealTimers();
			await rm(root, { recursive: true, force: true });
		}
	});

	it("recovers legacy capacity-paused tasks without spending a scientific attempt", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_legacy_capacity",
			objective: "recover old provider failure semantics",
			workspaceRoot,
			automation: "full",
			definitions: [validationStage],
		});
		const failed = await job.dispatchTask({
			id: "task_legacy_failed",
			attempt: 2,
			replayKey: "legacy:failed",
			stageId: "validation",
			stageExecutionId: "stage_exec_validation",
			role: "worker",
			objective: "capacity interrupted worker",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["structured"],
			failureSignals: ["missing content"],
			dependencies: [],
			scope: { workspaceRoot, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["structured"],
		});
		const running = await job.dispatchTask({
			id: "task_legacy_running",
			attempt: 2,
			replayKey: "legacy:running",
			stageId: "validation",
			stageExecutionId: "stage_exec_validation",
			role: "worker",
			objective: "concurrent interrupted worker",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["structured"],
			failureSignals: ["missing content"],
			dependencies: [],
			scope: { workspaceRoot, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["structured"],
		});
		await job.consumeTurns(2);
		await job.setTaskStatus(failed.id, "failed");
		await job.setTaskStatus(running.id, "running");
		await job.recordChildSession({
			sessionId: "legacy-capacity-session",
			role: "worker",
			taskId: failed.id,
			status: "failed",
			attempt: 2,
			error: "Our servers are currently overloaded. Please try again later.",
			updatedAt: new Date().toISOString(),
		});
		await job.pause("provider capacity gate: HTTP 429 rate limit exceeded");

		await job.resume();

		expect(job.state.tasks[failed.id]).toMatchObject({ status: "ready", attempt: 2 });
		expect(job.state.tasks[running.id]).toMatchObject({ status: "ready", attempt: 2 });
		expect(job.state.sessions["legacy-capacity-session"]?.status).toBe("interrupted");
		expect(job.status().budget.turnsUsed).toBe(0);
		expect(job.status().paused).toBe(false);
	});

	it("supersedes a frozen active search when resuming with new user guidance", async () => {
		const searchStage: StageDefinition = {
			...validationStage,
			searchPolicy: {
				strategy: "best-first",
				minCandidates: 2,
				maxCandidates: 2,
				criteria: ["structured"],
			},
		};
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_guided_replan",
			objective: "replan implementation after user correction",
			workspaceRoot,
			automation: "full",
			definitions: [searchStage],
		});
		await job.recordStagePlan({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_guided_replan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan-guided-replan",
			mode: "search",
			tasks: ["local-only", "no-download"].map((key) => ({
				key,
				objective: `old ${key} contract`,
				hypothesis: key,
				inputArtifactRefs: [],
				requiredOutputFields: ["content"],
				acceptanceChecks: ["structured"],
				failureSignals: ["missing content"],
				successCriteria: ["structured"],
			})),
			rationale: "old local-only plan",
			sessionRef: "fixture:main",
			createdAt: new Date().toISOString(),
		});
		const batch = Object.values(job.state.searchBatches)[0];
		if (!batch) throw new Error("expected active search batch");
		for (const candidate of Object.values(batch.candidates)) {
			await job.dispatchTask({
				id: `task_${candidate.key.replaceAll("-", "_")}`,
				replayKey: `guided:${candidate.key}`,
				stageId: "validation",
				stageExecutionId: "stage_exec_validation",
				role: "worker",
				objective: candidate.hypothesis,
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: ["content"],
				acceptanceChecks: ["structured"],
				failureSignals: ["missing content"],
				dependencies: [],
				scope: { workspaceRoot, allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 30_000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["structured"],
				searchBatchId: batch.id,
				searchCandidateId: candidate.id,
			});
		}
		await job.pause("provider capacity gate: HTTP 429");

		await job.resumeWithGuidance("Install the required environment and assets inside the implementation stage");

		expect(job.state.searchBatches[batch.id]?.status).toBe("superseded");
		expect(
			Object.values(job.state.tasks)
				.filter((task) => task.searchBatchId === batch.id)
				.map((task) => task.status),
		).toEqual(["blocked", "blocked"]);
		expect(Object.values(job.state.graph.nodes).some((node) => node.actor === "user")).toBe(true);
		expect(job.status().nextAction).toContain("replan validation");
		expect(job.status().paused).toBe(false);
	});
});
