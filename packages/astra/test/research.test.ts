import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "@earendil-works/pi-agent-core";
import { createFauxCore, fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai/providers/faux";
import { Type } from "typebox";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createHarness } from "../../coding-agent/test/suite/harness.ts";
import { mainDecisionManifestPath, readJson, writeMainDecisionManifest } from "../src/contracts.ts";
import { createAstraExtension } from "../src/extension.ts";
import { appendJobMemory, loadStageSkills, readJobMemory } from "../src/memory.ts";
import { migratePmcli } from "../src/migration.ts";
import { ResearchJob } from "../src/research.ts";
import { runResearchControl } from "../src/research-control.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";
import {
	type ResearchMainAgentAdapter,
	type ResearchReviewerAdapter,
	ResearchSupervisor,
	type WorkerRunResult,
} from "../src/supervisor.ts";
import { reviewFixture } from "./review-fixture.ts";

let workspaceRoot: string;
beforeEach(async () => {
	workspaceRoot = await mkdtemp(join(tmpdir(), "astra-flow-fixture-"));
});
afterEach(async () => {
	await rm(workspaceRoot, { recursive: true, force: true });
});

async function workerTask(job: ResearchJob, objective: string, id = `task-${objective}`) {
	const repairChecks = Object.values(job.state.obligations)
		.filter((issue) => issue.status === "open")
		.flatMap((issue) =>
			(issue.items ?? []).map((item) => ({
				issueId: item.id,
				criterion: job.repairCriterion(`[${item.id}] ${item.criterion}`),
			})),
		);
	const task = await job.dispatchTask({
		repairChecks,
		id,
		stageId: job.state.frame.activeStageId,
		stageExecutionId: "stage_exec_fixture",
		role: "worker",
		objective,
		inputArtifactRefs: [],
		requiredOutputType: "validation",
		acceptanceChecks: ["structured", ...repairChecks.map((check) => check.criterion)],
		dependencies: [],
		allowedTools: ["read"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
		requiredCanonicalArtifacts: [],
		requiredOutputFields: ["content", "outputRefs"],
		failureSignals: ["missing output manifest"],
		scope: {
			workspaceRoot: job.state.frame.permissions.workspaceRoot,
			allowedPaths: [`.astra/jobs/${job.state.frame.jobId}/tasks`],
		},
		reviewGateRequired: true,
		resumePolicy: "restart-attempt",
		successCriteria: ["structured"],
	});
	await job.setTaskStatus(task.id, "running");
	await job.setTaskStatus(task.id, "succeeded");
	return task;
}

async function runPiWorker(task: {
	id: string;
	requiredOutputType: string;
	requiredOutputFields: string[];
}): Promise<WorkerRunResult> {
	const core = createFauxCore({});
	let emitted = "";
	core.setResponses([
		fauxAssistantMessage(
			fauxToolCall("emit_evidence", { value: `${task.requiredOutputType}:${task.id}` }, { id: `emit-${task.id}` }),
		),
		fauxAssistantMessage("worker completed"),
	]);
	const agent = new Agent({
		streamFn: core.streamSimple,
		initialState: {
			model: core.getModel(),
			systemPrompt: "You are an Astra worker. Emit the requested evidence exactly once.",
			tools: [
				{
					name: "emit_evidence",
					label: "Emit evidence",
					description: "Emit structured evidence",
					parameters: Type.Object({ value: Type.String() }),
					execute: async (_id: string, params: unknown) => {
						const value = (params as { value: string }).value;
						emitted = value;
						return { content: [{ type: "text", text: `recorded ${value}` }], details: { value } };
					},
				},
			],
		},
	});
	await agent.prompt(`Complete TaskPacket ${task.id}.`);
	return {
		artifactType: task.requiredOutputType,
		content: {
			...Object.fromEntries(task.requiredOutputFields.map((field) => [field, `${field}:${task.id}`])),
			...(task.requiredOutputType === "result-to-claim"
				? {
						scientificOutcome: "supported",
						missionCoverage: "sufficient",
						claims: [{ statement: "fixture supported claim", assessment: "supported" }],
						supportingResults: ["fixture result"],
						unsupportedClaims: [],
						missingEvidence: [],
						conclusion: "fixture supported conclusion",
					}
				: {}),
			...(task.requiredOutputType === "research-review"
				? {
						verdict: "pass",
						scientificOutcome: "supported",
						missionCoverage: "sufficient",
						requiredRepairs: [],
						claimAudit: ["supported"],
					}
				: {}),
			emitted,
			assistantTurns: agent.state.messages.filter((message) => message.role === "assistant").length,
		},
		refs: [`pi-session:${task.id}`],
	};
}

describe("Pi-native Astra research state", () => {
	it("surfaces a corrupt active-job binding instead of treating it as missing", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-corrupt-active-job-"));
		await mkdir(join(root, ".astra"), { recursive: true });
		await writeFile(join(root, ".astra", "active-job.json"), "{broken", "utf8");

		await expect(runResearchControl({ action: "status" }, root)).rejects.toThrow(SyntaxError);
	});

	it("persists durable memory, package skills, and read-only pmcli migration report", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-memory-"));
		await appendJobMemory(root, "job-memory", {
			kind: "note",
			content: "keep source provenance",
			sourceRefs: ["source:test"],
			stageId: "literature",
			role: "worker",
		});
		expect((await readJobMemory(root, "job-memory"))[0]?.content).toBe("keep source provenance");
		expect((await loadStageSkills(root, "literature", "worker")).join("\n")).toContain("provenance");
		const report = await migratePmcli(root);
		expect(report.status).toBe("not-needed");
		expect(report.readonlySource).toBe(true);
	});

	it("keeps structured decision refs while safely encoding manifest filenames", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-decision-"));
		const manifest = {
			schemaVersion: "astra.main_agent_decision_manifest.v1" as const,
			manifestId: "decision_1",
			jobId: "job_1",
			decisionType: "evidence" as const,
			decisionRef: "main_agent_worker_artifact_decision::accept_1",
			decision: "accept" as const,
			evidenceId: "evidence_1",
			rationale: "fixture",
			sessionRef: "pi-session:test",
			createdAt: new Date().toISOString(),
		};
		const path = await writeMainDecisionManifest(manifest, root);
		expect(path).toContain("main_agent_worker_artifact_decision__accept_1.json");
		expect((await readJson<typeof manifest>(path)).decisionRef).toBe(manifest.decisionRef);
		expect(mainDecisionManifestPath(root, "job_1", "evidence")).toContain("evidence-latest.json");
	});
	function mainAgent(): ResearchMainAgentAdapter {
		return {
			async planStage(job, obligation, requestedMode = obligation ? "repair" : "decompose") {
				const stageId = job.state.frame.activeStageId;
				const definition = job.definitions[stageId];
				const taskCount = requestedMode === "search" ? (definition.searchPolicy?.minCandidates ?? 2) : 1;
				return {
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: `plan-${stageId}-${job.state.eventSeq}`,
					jobId: job.state.frame.jobId,
					stageId,
					decisionRef: `plan-${stageId}-${job.state.eventSeq}`,
					mode: requestedMode,
					tasks: Array.from({ length: taskCount }, (_, index) => ({
						key: obligation ? "repair" : `primary-${index + 1}`,
						objective: obligation ? `Repair ${obligation.description}` : `Produce ${stageId} evidence`,
						hypothesis: requestedMode === "search" ? `${stageId} candidate ${index + 1}` : undefined,
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
					})),
					rationale: "fixture stage plan",
					sessionRef: "fixture:main",
					...(obligation ? { obligationId: obligation.id } : {}),
					createdAt: new Date().toISOString(),
				};
			},
			async decideEvidence(evidence) {
				return {
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `decision-evidence-${evidence.id}`,
					jobId: evidence.id.split("_")[0] ?? "job",
					decisionType: "evidence",
					decisionRef: `accept-${evidence.id}`,
					evidenceId: evidence.id,
					decision: "accept",
					rationale: "fixture main-agent acceptance",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				};
			},
			async decideAdoption(evidence, job) {
				const active = Object.values(job.state.canonical).find(
					(artifact) => artifact.status === "active" && artifact.type === evidence.type,
				);
				return {
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `decision-adoption-${evidence.id}`,
					jobId: job.state.frame.jobId,
					decisionType: "adoption",
					decisionRef: `adopt-${evidence.id}`,
					evidenceId: evidence.id,
					adopt: true,
					replacementOf: active?.id,
					rationale: "fixture main-agent adoption",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				};
			},
			async decideSearch(batch, evaluations, job) {
				return {
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `decision-search-${batch.id}`,
					jobId: job.state.frame.jobId,
					decisionType: "search-selection",
					decisionRef: `select-${batch.id}`,
					searchBatchId: batch.id,
					selectedCandidateId: evaluations.sort((left, right) => right.score - left.score)[0]?.candidateId,
					rationale: "fixture candidate comparison",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				};
			},
			async decideRoute(job, obligation) {
				const stageIds = Object.keys(job.definitions);
				const stageIndex = stageIds.indexOf(job.state.frame.activeStageId);
				const targetStageId = obligation ? undefined : stageIds[stageIndex + 1];
				return {
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `decision-route-${job.state.eventSeq}`,
					jobId: job.state.frame.jobId,
					decisionType: "route",
					decisionRef: `route-${job.state.eventSeq}`,
					stageId: job.state.frame.activeStageId,
					routeAction: obligation ? "continue" : targetStageId ? "advance" : "complete",
					targetStageId,
					evidenceRefs: Object.values(job.state.canonicalRoute.stageArtifactIds),
					rationale: targetStageId ? `advance to ${targetStageId}` : "all quality gates passed",
					sessionRef: "fixture:main",
					createdAt: new Date().toISOString(),
				};
			},
		};
	}

	it("runs a bounded outer loop with review failure, repair, replacement, and stage advance", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_e2e",
			objective: "fixture auto research",
			workspaceRoot,
		});
		let reviewCount = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "fixture-supervisor",
			maxParallel: 2,
			worker: {
				async run(task) {
					return { artifactType: "validation", content: { task: task.objective }, refs: [`fixture:${task.id}`] };
				},
			},
			reviewer: {
				async review(evidence, currentJob) {
					if (evidence.type === "stage-plan")
						return reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] });
					reviewCount += 1;
					return reviewCount === 1
						? reviewFixture(currentJob, {
								evidenceId: evidence.id,
								...{ verdict: "fail", findings: ["repair required"] },
							})
						: reviewFixture(currentJob, { evidenceId: evidence.id, ...{ verdict: "pass", findings: [] } });
				},
			},
			mainAgent: mainAgent(),
		});
		const firstTick = await supervisor.tick();
		expect(firstTick.dispatchedTaskIds).toHaveLength(1);
		expect(Object.values(job.state.obligations)).toHaveLength(1);
		const secondTick = await supervisor.tick();
		expect(secondTick.dispatchedTaskIds).toHaveLength(1);
		expect(secondTick.completed).toBe(false);
		expect(secondTick.routeChanged).toBe(true);
		expect(job.state.frame.activeStageId).toBe("literature");
		expect(Object.values(job.state.canonical).filter((artifact) => artifact.status === "active")).toHaveLength(1);
		expect(Object.values(job.state.discardedEvidence)).toHaveLength(1);
		expect(Object.values(job.state.obligations).every((obligation) => obligation.status === "resolved")).toBe(true);
	});

	it("runs complete auto-research through explicit dynamic route decisions and Pi worker loops", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_full",
			objective: "fixture complete research",
			workspaceRoot,
			automation: "full",
		});
		let reviewCount = 0;
		let piWorkerRuns = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "full-fixture-supervisor",
			maxParallel: 2,
			worker: {
				async run(task) {
					piWorkerRuns += 1;
					return runPiWorker(task);
				},
			},
			reviewer: {
				async review(evidence, currentJob) {
					if (evidence.type === "stage-plan")
						return reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] });
					reviewCount += 1;
					return reviewCount === 1
						? reviewFixture(currentJob, {
								evidenceId: evidence.id,
								...{ verdict: "fail", findings: ["fixture repair obligation"] },
							})
						: reviewFixture(currentJob, { evidenceId: evidence.id, ...{ verdict: "pass", findings: [] } });
				},
			},
			mainAgent: mainAgent(),
		});
		let ticks = 0;
		while (job.state.frame.status !== "completed") {
			await supervisor.tick();
			ticks += 1;
			if (ticks > 40) throw new Error("full fixture research did not converge");
		}
		expect(Object.keys(job.state.stages)).toHaveLength(14);
		expect(piWorkerRuns).toBeGreaterThan(14);
		expect(reviewCount).toBeGreaterThan(14);
		expect(Object.values(job.state.canonical).filter((artifact) => artifact.status === "active")).toHaveLength(14);
		expect(Object.values(job.state.obligations).every((obligation) => obligation.status === "resolved")).toBe(true);
		expect((await store.readEvents("job_full")).some((event) => event.event.type === "route_decided")).toBe(true);
	});

	it("keeps candidate evidence behind main-agent decision and independent review", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_gate",
			objective: "fixture research",
			workspaceRoot,
		});
		const task = await workerTask(job, "initial validation");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: "validation",
			type: "validation",
			content: { ok: true },
			refs: ["fixture:validation"],
		});
		await expect(job.adoptEvidence(evidence.id)).rejects.toThrow("accepted");
		await expect(
			job.decideEvidence(evidence.id, true, "main_agent_worker_artifact_decision::accept_1"),
		).rejects.toThrow("passing reviews");
		const failed = await job.recordReview(
			reviewFixture(job, {
				evidenceId: evidence.id,
				verdict: "fail",
				findings: ["missing control"],
			}),
		);
		expect(Object.values(job.state.obligations)).toHaveLength(1);
		expect(Object.values(job.state.obligations)[0].sourceReviewId).toBe(failed.id);
		await expect(job.adoptEvidence(evidence.id)).rejects.toThrow("accepted before adoption");
	});

	it("reviews existing repair evidence before planning another repair", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_repair_review_priority",
			objective: "fixture repair review priority",
			workspaceRoot,
		});
		const firstTask = await workerTask(job, "first repair candidate", "task-repair-priority-first");
		const first = await job.recordEvidence({
			taskId: firstTask.id,
			stageId: "validation",
			type: "validation",
			content: { version: 1 },
			refs: ["fixture:repair-priority-v1"],
		});
		await job.recordReview(
			reviewFixture(job, { evidenceId: first.id, verdict: "fail", findings: ["repair the evidence"] }),
		);
		const repairTask = await workerTask(job, "second repair candidate", "task-repair-priority-second");
		const repair = await job.recordEvidence({
			taskId: repairTask.id,
			stageId: "validation",
			type: "validation",
			content: { version: 2 },
			refs: ["fixture:repair-priority-v2"],
			currentEvidenceSetId: first.currentEvidenceSetId,
		});
		let planCalls = 0;
		let reviewCalls = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "repair-priority-supervisor",
			worker: {
				async run() {
					throw new Error("worker should not run while repair evidence awaits review");
				},
			},
			reviewer: {
				async review(evidence, currentJob) {
					reviewCalls += 1;
					expect(evidence.id).toBe(repair.id);
					return reviewFixture(currentJob, { evidenceId: evidence.id, ...{ verdict: "pass", findings: [] } });
				},
			},
			mainAgent: {
				...mainAgent(),
				async planStage() {
					planCalls += 1;
					throw new Error("repair planning should not run before pending evidence review");
				},
			},
		});

		const result = await supervisor.tick();

		expect(planCalls).toBe(0);
		expect(reviewCalls).toBe(1);
		expect(result.routeChanged).toBe(true);
		expect(Object.values(job.state.obligations).every((obligation) => obligation.status === "resolved")).toBe(true);
	});

	it("turns a partial review into an explicit repair obligation", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_partial_review",
			objective: "fixture partial review",
			workspaceRoot,
		});
		const task = await workerTask(job, "partial candidate", "task-partial-review");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: "validation",
			type: "validation",
			content: { partial: true },
			refs: ["fixture:partial"],
		});
		await job.recordReview(
			reviewFixture(job, { evidenceId: evidence.id, verdict: "partial", findings: ["add a measurable bound"] }),
		);

		expect(job.state.frame.openObligationIds).toHaveLength(1);
		expect(Object.values(job.state.obligations)[0]).toMatchObject({
			description: "add a measurable bound",
			status: "open",
		});
	});

	it("releases the supervisor lease and survives a worker crash on the next tick", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_crash",
			objective: "fixture recovery",
			workspaceRoot,
		});
		let runs = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "crash-supervisor",
			worker: {
				async run(task) {
					runs += 1;
					if (runs === 1) throw new Error("fixture worker crash");
					return {
						artifactType: task.requiredOutputType,
						content: { recovered: true },
						refs: [`fixture:${task.id}`],
					};
				},
			},
			reviewer: {
				async review(evidence, currentJob) {
					return reviewFixture(currentJob, { evidenceId: evidence.id, ...{ verdict: "pass", findings: [] } });
				},
			},
			mainAgent: mainAgent(),
		});
		const first = await supervisor.tick();
		expect(first.recovered).toBe(true);
		expect(job.state.lease).toBeUndefined();
		const second = await supervisor.tick();
		expect(second.recovered).toBe(true);
		expect(Object.values(job.state.evidence).length).toBeGreaterThan(0);
	});

	it("bounds failed search candidates and routes terminal search failure", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_search_retry_limit",
			objective: "fixture search retry limit",
			workspaceRoot,
			definitions: [
				{
					...DEFAULT_STAGES[0],
					searchPolicy: {
						strategy: "diverse-candidates",
						minCandidates: 2,
						maxCandidates: 2,
						maxRounds: 1,
						criteria: ["candidate is executable"],
					},
				},
			],
		});
		const fixtureMain = mainAgent();
		fixtureMain.decideRoute = async (currentJob) => ({
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: `decision-route-${currentJob.state.eventSeq}`,
			jobId: currentJob.state.frame.jobId,
			decisionType: "route",
			decisionRef: `route-${currentJob.state.eventSeq}`,
			stageId: currentJob.state.frame.activeStageId,
			routeAction: "ask-user",
			question: "Which evidence source should replace the failed search candidates?",
			evidenceRefs: [],
			rationale: "all bounded search candidates failed",
			sessionRef: "fixture:main",
			createdAt: new Date().toISOString(),
		});
		const supervisor = new ResearchSupervisor(job, store, {
			owner: "search-retry-limit-supervisor",
			maxParallel: 2,
			worker: {
				async run() {
					throw new Error("fixture candidate manifest missing");
				},
			},
			reviewer: {
				async review(evidence, currentJob) {
					if (evidence.type === "stage-plan")
						return reviewFixture(currentJob, { evidenceId: evidence.id, verdict: "pass", findings: [] });
					throw new Error("failed candidates must not reach review");
				},
			},
			mainAgent: fixtureMain,
		});

		await supervisor.tick();
		await supervisor.tick();
		const terminalTick = await supervisor.tick();

		expect(terminalTick.paused).toBe(true);
		expect(job.state.frame.userGate?.kind).toBe("research");
		const batch = Object.values(job.state.searchBatches)[0];
		expect(batch?.status).toBe("exhausted");
		expect(Object.values(batch?.candidates ?? {}).every((candidate) => candidate.status === "failed")).toBe(true);
		expect(Object.values(job.state.tasks).filter((task) => task.role === "worker")).toHaveLength(6);
		expect(Math.max(...Object.values(job.state.tasks).map((task) => task.attempt))).toBe(3);
		for (const task of Object.values(job.state.tasks).filter((candidate) => candidate.attempt > 1)) {
			const prior = Object.values(job.state.tasks).find(
				(candidate) => candidate.replayKey === task.replayKey && candidate.attempt === task.attempt - 1,
			);
			expect(task.supersedesTaskId).toBe(prior?.id);
		}
	});

	it("serializes supervisors across stores and reloads the latest snapshot before each tick", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-supervisor-lock-"));
		try {
			const firstStore = new JsonlAstraStore(root);
			const created = await ResearchJob.create(firstStore, {
				jobId: "job_supervisor_lock",
				objective: "fixture supervisor serialization",
				workspaceRoot: root,
			});
			const secondStore = new JsonlAstraStore(root);
			const stale = await ResearchJob.open(secondStore, created.state.frame.jobId);
			if (!stale) throw new Error("fixture job did not reopen");
			let releaseFirstWorker!: () => void;
			let markFirstWorkerStarted!: () => void;
			const firstWorkerStarted = new Promise<void>((resolve) => {
				markFirstWorkerStarted = resolve;
			});
			const firstWorkerRelease = new Promise<void>((resolve) => {
				releaseFirstWorker = resolve;
			});
			const passingReviewer: ResearchReviewerAdapter = {
				async review(evidence, currentJob) {
					return reviewFixture(currentJob, {
						evidenceId: evidence.id,
						...{ verdict: "pass" as const, findings: [] },
					});
				},
			};
			const firstSupervisor = new ResearchSupervisor(created, firstStore, {
				owner: "first-supervisor",
				worker: {
					async run(task) {
						markFirstWorkerStarted();
						await firstWorkerRelease;
						return {
							artifactType: task.requiredOutputType,
							content: { stageId: task.stageId },
							refs: [`fixture:${task.id}`],
						};
					},
				},
				reviewer: passingReviewer,
				mainAgent: mainAgent(),
			});
			const secondWorkerStages: string[] = [];
			const secondSupervisor = new ResearchSupervisor(stale, secondStore, {
				owner: "second-supervisor",
				worker: {
					async run(task) {
						secondWorkerStages.push(task.stageId);
						return {
							artifactType: task.requiredOutputType,
							content: { stageId: task.stageId },
							refs: [`fixture:${task.id}`],
						};
					},
				},
				reviewer: passingReviewer,
				mainAgent: mainAgent(),
			});

			const firstTick = firstSupervisor.tick();
			await firstWorkerStarted;
			try {
				await expect(secondSupervisor.tick()).rejects.toThrow("supervisor lock held");
			} finally {
				releaseFirstWorker();
				await firstTick;
			}

			await secondSupervisor.tick();
			expect(secondWorkerStages).toEqual(["literature"]);
			const events = await secondStore.readEvents(created.state.frame.jobId);
			expect(events.map((event) => event.seq)).toEqual(events.map((_, index) => index + 1));
			const reopened = await ResearchJob.open(secondStore, created.state.frame.jobId);
			expect(reopened?.state.eventSeq).toBe(events.length);
			expect(reopened?.state.stages.validation.status).toBe("completed");
			expect(reopened?.state.stages.literature.status).toBe("completed");
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});

	it("supports repair review, canonical replacement, and restart recovery", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-"));
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			jobId: "job_recovery",
			objective: "fixture recovery",
			workspaceRoot: root,
		});
		await job.acquireLease("supervisor-a");
		const firstTask = await workerTask(job, "first candidate", "task-first");
		const first = await job.recordEvidence({
			taskId: firstTask.id,
			stageId: "validation",
			type: "validation",
			content: { version: 1 },
			refs: ["fixture:v1"],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: first.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(first.id, true, "main_agent_worker_artifact_decision::v1");
		const artifact = await job.adoptEvidence(first.id);
		expect(artifact.materializationRef).toContain("canonical");
		expect(artifact.targetSha256).toBeTruthy();

		const repairTask = await workerTask(job, "repair candidate", "task-repair");
		const repair = await job.recordEvidence({
			taskId: repairTask.id,
			stageId: "validation",
			type: "validation",
			content: { version: 2 },
			refs: ["fixture:v2"],
			currentEvidenceSetId: first.currentEvidenceSetId,
		});
		await job.recordReview(reviewFixture(job, { evidenceId: repair.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(repair.id, true, "main_agent_worker_artifact_decision::v2");
		const replacement = await job.adoptEvidence(repair.id, artifact.id);
		expect(job.state.canonical[artifact.id]).toBeUndefined();
		expect(job.state.retiredArtifacts[artifact.id]?.replacementId).toBe(replacement.id);
		expect(replacement.replacementOf).toBe(artifact.id);
		expect(replacement.materializationRef).toContain("canonical");
		expect(job.state.evidence[first.id]).toBeUndefined();

		const reopened = await ResearchJob.open(store, "job_recovery");
		expect(reopened?.state.canonical[replacement.id].status).toBe("active");
		expect((await store.readEvents("job_recovery")).length).toBeGreaterThan(8);
		expect(
			JSON.parse(await readFile(join(root, ".astra/jobs/job_recovery/job.json"), "utf8")).eventSeq,
		).toBeGreaterThan(8);
	});

	it("rejects lease conflicts, replay duplicates, and unsafe replacement refs", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_lease",
			objective: "fixture lease",
			workspaceRoot,
		});
		await job.acquireLease("owner-a");
		await expect(job.acquireLease("owner-b")).rejects.toThrow("lease held");
		const first = await workerTask(job, "same objective", "task-replay");
		const duplicate = await job.dispatchTask({
			id: "different-id",
			stageId: "validation",
			stageExecutionId: "stage_exec_fixture",
			role: "worker",
			objective: "same objective",
			inputArtifactRefs: [],
			requiredOutputType: "validation",
			acceptanceChecks: ["structured"],
			dependencies: [],
			allowedTools: ["read"],
			writeAuthority: "workspace-write",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			requiredCanonicalArtifacts: [],
			requiredOutputFields: ["content", "outputRefs"],
			failureSignals: ["missing output manifest"],
			scope: { workspaceRoot, allowedPaths: [`.astra/jobs/${job.state.frame.jobId}/tasks`] },
			reviewGateRequired: true,
			resumePolicy: "restart-attempt",
			successCriteria: ["structured"],
		});
		expect(duplicate.id).toBe(first.id);
		await expect(job.adoptEvidence("missing", "missing-artifact")).rejects.toThrow("unknown evidence");
	});

	it("requires a passing review before canonical adoption and keeps role policy explicit", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_role",
			objective: "fixture policy",
			workspaceRoot,
			allowDestructive: false,
		});
		const task = await workerTask(job, "policy candidate");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: "validation",
			type: "validation",
			content: "candidate",
			refs: ["fixture:policy"],
		});
		expect(evidence.status).toBe("candidate");
		expect(evidence.acceptanceAuthority).toBeUndefined();
		await expect(job.decideEvidence(evidence.id, true)).rejects.toThrow("passing reviews");
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(evidence.id, true);
		expect(job.state.evidence[evidence.id].acceptanceAuthority).toBe("main_agent");
	});

	it("writes an Astra checkpoint before Pi compaction and keeps the durable job resumable", async () => {
		const harness = await createHarness({
			settings: { compaction: { keepRecentTokens: 1 } },
			extensionFactories: [createAstraExtension({ jobId: "job_compact" })],
		});
		try {
			const store = new JsonlAstraStore(harness.tempDir);
			const job = await ResearchJob.create(store, {
				jobId: "job_compact",
				objective: "compaction recovery",
				workspaceRoot: harness.tempDir,
			});
			harness.setResponses(
				Array.from({ length: 8 }, (_, index) =>
					fauxAssistantMessage(index < 2 ? `${index + 1} response` : "compaction summary"),
				),
			);
			await harness.session.prompt("first durable research turn");
			await harness.session.prompt("second durable research turn");
			await harness.session.compact();

			const entries = harness.sessionManager.getEntries();
			expect(entries.some((entry) => entry.type === "compaction")).toBe(true);
			expect(entries.some((entry) => entry.type === "custom" && entry.customType === "astra_checkpoint")).toBe(true);
			expect(harness.session.messages.some((message) => message.role === "compactionSummary")).toBe(true);
			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			expect(reopened?.status().activeStageId).toBe("validation");
			expect(
				(await readJobMemory(harness.tempDir, "job_compact")).some((entry) => entry.kind === "checkpoint"),
			).toBe(true);
		} finally {
			harness.cleanup();
		}
	});
});
