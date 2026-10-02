import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import {
	readJson,
	reviewPacketPath,
	taskDir,
	workerManifestPath,
	writeReviewerOutputManifest,
	writeWorkerOutputManifest,
} from "../src/contracts.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { writeSourceReceipt } from "../src/literature.ts";
import { PiChildSessionRunner, PiReviewerAdapter, PiWorkerAdapter } from "../src/pi-child-session.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { checksum, ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { type AstraStore, JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import { prepareReviewEvidenceBundle, taskWorkspacePath } from "../src/task-workspace.ts";
import type {
	AstraEvent,
	Evidence,
	MainAgentDecisionManifest,
	ReviewPacket,
	StageDefinition,
	StagePlanManifest,
	TaskPacket,
	WorkerOutputManifest,
} from "../src/types.ts";
import { validateWorkerSubmission, type WorkerSubmission } from "../src/worker-submission.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function setup(definitions?: StageDefinition[]) {
	const root = await mkdtemp(join(tmpdir(), "astra-completion-recovery-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Offline recovery",
		automation: "full",
		definitions,
	});
	return { root, job, store };
}

function input(job: ResearchJob, key: string) {
	return {
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker" as const,
		deliveryKind: "local" as const,
		objective: key,
		replayKey: key,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation:local",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		successCriteria: [],
		failureSignals: ["missing"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none" as const,
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session" as const,
	};
}

async function delivery(
	job: ResearchJob,
	key: string,
	fields: Partial<TaskPacket> = {},
	refs: string[] = [],
	lineage?: string,
) {
	const task = await job.dispatchTask({ ...input(job, key), ...fields });
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: key },
		refs,
		currentEvidenceSetId: lineage,
	});
	return { task, evidence };
}

async function accept(job: ResearchJob, evidence: Evidence) {
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
}

function route(
	job: ResearchJob,
	action: "backtrack" | "ask-user" | "continue",
	fields: Partial<MainAgentDecisionManifest> = {},
): MainAgentDecisionManifest {
	return {
		schemaVersion: "astra.main_agent_decision_manifest.v1",
		manifestId: `route-${action}`,
		jobId: job.state.frame.jobId,
		decisionType: "route",
		decisionRef: `route-${action}`,
		stageId: job.state.frame.activeStageId,
		routeAction: action,
		targetStageId: "validation",
		rationale: "offline route",
		sessionRef: "fixture",
		createdAt: new Date().toISOString(),
		...fields,
	};
}

function interrupt(store: JsonlAstraStore, matches: (event: AstraEvent) => boolean, after = false) {
	const append = store.append.bind(store);
	let hit = false;
	vi.spyOn(store, "append").mockImplementation(async (id, event) => {
		if (!hit && matches(event)) {
			hit = true;
			if (!after) throw new Error("injected completion append failure");
			vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("injected completion snapshot failure"));
		}
		return append(id, event);
	});
}

it.each([false, true])(
	"F06 recovers the prepared worker output while preserving a user pause and frozen bytes (snapshot=%s)",
	async (snapshot) => {
		const { root, job, store } = await setup();
		const sourceRef = "https://example.invalid/frozen-completion-source";
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: job.state.frame.jobId, query: "original source", limit: 1 },
			{ sourceRef, title: "original", authors: [] },
			"fixture",
			"2026-10-01T00:00:00.000Z",
		);
		let plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "completion-plan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "completion-plan",
			mode: "decompose",
			tasks: [
				{
					key: "work",
					deliveryKind: "local",
					objective: "produce output",
					inputArtifactRefs: [],
					requiredOutputFields: ["content"],
					acceptanceChecks: ["verified"],
					successCriteria: [],
					failureSignals: [],
				},
			],
			rationale: "offline",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		};
		plan = await job.recordStagePlan(plan, job.state);
		const pe = await preparePlanEvidence(job, plan);
		expect(Object.values(job.state.graph.nodes).some((node) => node.domainRef === pe.id)).toBe(false);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
		const forbidden = vi.fn(async () => {
			throw new Error("unexpected model call");
		});
		const worker = vi.fn(async (task: TaskPacket) => {
			const cwd = taskWorkspacePath(root, task.jobId, task.id);
			await mkdir(cwd, { recursive: true });
			await writeFile(join(cwd, "result.json"), "original output");
			return {
				artifactType: task.requiredOutputType,
				content: { content: "completed" },
				refs: ["result.json", sourceRef],
			};
		});
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: worker },
			reviewer: { review: forbidden },
			mainAgent: {
				planStage: forbidden,
				decideEvidence: forbidden,
				decideAdoption: forbidden,
				decideSearch: forbidden,
				decideRoute: forbidden,
			},
		});
		interrupt(
			store,
			(event) => event.type === "evidence_recorded" && event.evidence.type === "validation:local",
			snapshot,
		);
		await supervisor.tick();
		expect(worker).toHaveBeenCalledOnce();
		const task = Object.values(job.state.tasks).find((task) => task.role === "worker")!;
		expect(task.status).toBe(snapshot ? "succeeded" : "running");
		const prepared = JSON.parse(
			await readFile(join(root, ".astra/jobs", task.jobId, "tasks", task.id, "evidence-completion.json"), "utf8"),
		);
		await rm(join(taskWorkspacePath(root, task.jobId, task.id), "result.json"));
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: task.jobId, query: "changed source", limit: 1 },
			{ sourceRef, title: "changed", authors: [] },
			"fixture",
			"2026-10-02T00:00:00.000Z",
		);
		const current = (await ResearchJob.open(store, task.jobId))!;
		await current.pause("user explicitly paused completed output recovery");
		const paused = current.state.frame.nextAction;
		await current.recoverPendingOperations();
		expect(current.state.paused).toBe(true);
		expect(current.state.frame.nextAction).toBe(paused);
		expect(current.state.tasks[task.id].status).toBe("succeeded");
		expect(current.state.evidence[prepared.evidence.id]).toEqual(prepared.evidence);
		const frozenSource = current.state.evidence[prepared.evidence.id].files!.find(
			(file) => file.sourceRef === sourceRef,
		)!;
		expect(
			JSON.parse(
				await readFile(join(root, ".astra/jobs", task.jobId, "versions/files", frozenSource.sha256), "utf8"),
			).query,
		).toBe("original source");
		expect(current.state.graph.nodes[`research_evidence_${prepared.evidence.id}`]).toBeDefined();
		const done = current.state;
		await current.recoverPendingOperations();
		expect(current.state).toEqual(done);
		expect(worker).toHaveBeenCalledOnce();
	},
);

it.each([false, true])("F07 acceptance atomically closes reviewed repairs (snapshot failure=%s)", async (snapshot) => {
	const { job, store } = await setup();
	const failed = await delivery(job, "failed");
	await job.recordReview(
		reviewFixture(job, { evidenceId: failed.evidence.id, verdict: "fail", findings: ["verified"] }),
	);
	const issue = Object.values(job.state.obligations)[0];
	const checks = issue.items!.map((item) => ({
		issueId: item.id,
		criterion: job.normalizedRepairCriterion(item.criterion),
	}));
	const fixed = await delivery(
		job,
		"fixed",
		{
			repairOfEvidenceId: failed.evidence.id,
			inputArtifactRefs: [failed.evidence.id],
			acceptanceChecks: checks.map((check) => check.criterion),
			repairChecks: checks,
		},
		[],
		failed.evidence.currentEvidenceSetId,
	);
	await job.recordReview(reviewFixture(job, { evidenceId: fixed.evidence.id, verdict: "pass", findings: [] }));
	if (snapshot) {
		interrupt(store, (event) => event.type === "evidence_decided", true);
		await expect(job.decideEvidence(fixed.evidence.id, true)).rejects.toThrow(/snapshot/);
	} else {
		interrupt(store, (event) => event.type === "repair_item_resolved");
		await job.decideEvidence(fixed.evidence.id, true);
	}
	const current = (await ResearchJob.open(store, job.state.frame.jobId))!;
	expect(current.state.evidence[fixed.evidence.id].status).toBe("accepted");
	expect(current.state.obligations[issue.id].status).toBe("resolved");
	expect(current.state.graph.nodes[issue.graphObjectionId!].status).toBe("resolved");
});

it.each(["backtrack", "ask-user"] as const)(
	"F08 commits %s consequences together and does not duplicate them",
	async (action) => {
		const { job, store } = await setup();
		const decision = route(job, action, { question: "Which next step?", newQuestions: ["one", "one"] });
		interrupt(store, (event) => event.type === "route_decided", true);
		await expect(job.applyRouteDecision(decision)).rejects.toThrow(/snapshot/);
		const current = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(current.state.graph.openQuestionIds).toHaveLength(3);
		if (action === "backtrack") expect(current.state.stages.validation.revision).toBe(2);
		else expect(current.state.frame.userGate?.kind).toBe("research");
		await current.recoverPendingOperations();
		const done = current.state;
		await current.applyRouteDecision(decision);
		await current.recoverPendingOperations();
		expect(current.state).toEqual(done);
	},
);

it("F09 keeps three frozen source versions and reuses identical bytes across upstream inputs", async () => {
	const { root, job } = await setup();
	const ref = "https://example.invalid/stable";
	const versions: Evidence[] = [];
	for (const value of ["old", "middle", "fresh"]) {
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: job.state.frame.jobId, query: value, limit: 1 },
			{ sourceRef: ref, title: value, authors: [] },
			"fixture",
			"2026-10-01T00:00:00.000Z",
		);
		versions.push((await delivery(job, value, {}, [ref])).evidence);
	}
	const target = await delivery(job, "target", { inputArtifactRefs: versions.map((e) => e.id) }, [ref]);
	const task = await job.dispatchTask({ ...input(job, "review"), role: "reviewer" });
	const bundle = await prepareReviewEvidenceBundle(task, target.evidence, job);
	const entries = bundle.filter((entry) => entry.sourceRef === ref);
	expect(entries).toHaveLength(3);
	expect(new Set(entries.map((entry) => entry.path)).size).toBe(3);
	for (const entry of entries) {
		const bytes = await readFile(join(root, ".astra/jobs", task.jobId, "tasks", task.id, entry.path));
		expect(createHash("sha256").update(bytes).digest("hex")).toBe(entry.sha256);
		expect(["old", "middle", "fresh"]).toContain(JSON.parse(bytes.toString()).query);
	}
});

it.each([
	"all-open",
	"some-items-closed",
	"only-objection",
	"changed-evidence-version",
	"old-review-version",
	"conflicting-review",
	"changed-task-version",
] as const)("F07 repairs only the missing historical acceptance tail (%s)", async (partial) => {
	const { job, store } = await setup();
	const failed = await delivery(job, "historical-failed");
	await job.recordReview(
		reviewFixture(job, { evidenceId: failed.evidence.id, verdict: "fail", findings: ["verified", "second check"] }),
	);
	const issue = Object.values(job.state.obligations)[0];
	const checks = issue.items!.map((item) => ({
		issueId: item.id,
		criterion: job.normalizedRepairCriterion(item.criterion),
	}));
	const fixed = await delivery(
		job,
		"historical-fixed",
		{
			repairOfEvidenceId: failed.evidence.id,
			inputArtifactRefs: [failed.evidence.id],
			acceptanceChecks: checks.map((check) => check.criterion),
			repairChecks: checks,
		},
		[],
		failed.evidence.currentEvidenceSetId,
	);
	await accept(job, fixed.evidence);
	const newer = await delivery(job, "newer-candidate", {}, [], failed.evidence.currentEvidenceSetId);
	await job.applyRouteDecision(route(job, "continue", { newQuestions: ["Confirm repaired scope"] }));
	const legacyStore = new MemoryAstraStore();
	for (const saved of await store.readEvents(job.state.frame.jobId)) {
		const event = saved.event;
		if (["repair_item_resolved", "obligation_resolved", "research_node_status"].includes(event.type)) continue;
		await legacyStore.append(
			saved.jobId,
			event.type === "route_decided"
				? { type: event.type, decision: event.decision }
				: event.type === "evidence_decided"
					? {
							type: event.type,
							evidenceId: event.evidenceId,
							accepted: event.accepted,
							decisionRef: event.decisionRef,
						}
					: event,
		);
	}
	let legacy = (await ResearchJob.open(legacyStore, job.state.frame.jobId))!;
	if (partial !== "all-open") {
		const snapshot = legacy.state;
		const obligation = snapshot.obligations[issue.id];
		if (partial === "some-items-closed") obligation.items![0].status = "resolved";
		else if (partial === "only-objection") {
			for (const item of obligation.items!) item.status = "resolved";
			obligation.status = "resolved";
			snapshot.frame.openObligationIds = snapshot.frame.openObligationIds.filter((id) => id !== issue.id);
		} else if (partial === "changed-evidence-version")
			snapshot.evidence[fixed.evidence.id].versionHash = "a different evidence version";
		else if (partial === "changed-task-version")
			snapshot.tasks[fixed.task.id].version = await job.captureTaskVersion(fixed.task.id);
		else {
			const review = Object.values(snapshot.reviews).find((review) => review.evidenceId === fixed.evidence.id)!;
			if (partial === "old-review-version") review.targetVersionHash = "a different reviewed version";
			else review.verdict = "fail";
		}
		await legacyStore.writeSnapshot(snapshot);
		legacy = (await ResearchJob.open(legacyStore, job.state.frame.jobId))!;
	}
	const history = vi.spyOn(legacyStore, "readEvents");
	if (
		["changed-evidence-version", "old-review-version", "conflicting-review", "changed-task-version"].includes(partial)
	) {
		await expect(legacy.recoverPendingOperations()).rejects.toThrow();
		expect(legacy.state.obligations[issue.id].status).toBe("open");
		expect(legacy.state.evidence[newer.evidence.id].status).toBe("candidate");
		return;
	}
	await legacy.recoverPendingOperations();
	expect(history).toHaveBeenCalledOnce();
	expect(legacy.state.obligations[issue.id].status).toBe("resolved");
	expect(legacy.state.obligations[issue.id].items!.every((item) => item.status === "resolved")).toBe(true);
	expect(legacy.state.graph.nodes[issue.graphObjectionId!].status).toBe("resolved");
	expect(legacy.state.evidence[newer.evidence.id].status).toBe("candidate");
	const done = legacy.state;
	await legacy.recoverPendingOperations();
	expect(legacy.state).toEqual(done);
	expect(history).toHaveBeenCalledOnce();
});

it.each(["backtrack", "ask-user"] as const)(
	"F08 recovers only missing historical %s consequences and preserves explicit pause",
	async (action) => {
		const { job, store } = await setup();
		const decision = route(job, action, {
			question: "Which source should define the next step?",
			newQuestions: ["Scope question"],
		});
		await job.applyRouteDecision(decision);
		const legacyStore = new MemoryAstraStore();
		for (const saved of await store.readEvents(job.state.frame.jobId)) {
			const event = saved.event;
			await legacyStore.append(
				saved.jobId,
				event.type === "route_decided" ? { type: event.type, decision: event.decision } : event,
			);
		}
		const legacy = (await ResearchJob.open(legacyStore, job.state.frame.jobId))!;
		await legacy.pause("explicit user pause");
		await legacy.recoverPendingOperations();
		expect(legacy.state.paused).toBe(true);
		expect(legacy.state.frame.nextAction).toBe("paused: explicit user pause");
		if (action === "backtrack") {
			expect(legacy.state.stages.validation.revision).toBe(1);
			await legacy.resume();
			await legacy.recoverPendingOperations();
			expect(legacy.state.stages.validation.revision).toBe(2);
		} else {
			await legacy.recordUserGuidance("Use the existing source");
			await legacy.recoverPendingOperations();
			expect(legacy.state.frame.userGate).toBeUndefined();
			expect(legacy.state.paused).toBe(true);
			await legacy.resume();
			await legacy.recoverPendingOperations();
			expect(legacy.state.paused).toBe(false);
		}
		const done = legacy.state;
		await legacy.recoverPendingOperations();
		expect(legacy.state).toEqual(done);
	},
);

it.each(["later-route", "guidance"] as const)(
	"F08 does not replay an old backtrack after %s even with misleading timestamps",
	async (later) => {
		const { job, store } = await setup();
		await job.applyRouteDecision(
			route(job, "backtrack", { createdAt: "2099-01-01T00:00:00.000Z", newQuestions: ["Old question"] }),
		);
		const legacyStore = new MemoryAstraStore();
		for (const saved of await store.readEvents(job.state.frame.jobId)) {
			const event = saved.event;
			await legacyStore.append(
				saved.jobId,
				event.type === "route_decided" ? { type: event.type, decision: event.decision } : event,
			);
		}
		const legacy = (await ResearchJob.open(legacyStore, job.state.frame.jobId))!;
		if (later === "later-route")
			await legacy.applyRouteDecision(
				route(legacy, "continue", { decisionRef: "new-route", createdAt: "2001-01-01T00:00:00.000Z" }),
			);
		else await legacy.recordUserGuidance("The old question has been answered; keep the current work");
		const done = legacy.state;
		await legacy.recoverPendingOperations();
		const marked = legacy.state;
		expect(marked.routeDecisions["route-backtrack"].consequencesSupersededBy).toBeDefined();
		delete marked.routeDecisions["route-backtrack"].consequencesSupersededBy;
		expect({ ...marked, eventSeq: done.eventSeq, updatedAt: done.updatedAt }).toEqual(done);
		expect(legacy.state.stages.validation.revision).toBe(1);
		expect(Object.values(legacy.state.graph.nodes).some((node) => node.statement === "Old question")).toBe(false);
	},
);

it("F08 rejected event append leaves no questions or reopened stage, and reused decision ids reject different content", async () => {
	const { job, store } = await setup();
	const decision = route(job, "backtrack", { newQuestions: ["Only after save"] });
	const before = job.state;
	interrupt(store, (event) => event.type === "route_decided");
	await expect(job.applyRouteDecision(decision)).rejects.toThrow(/injected/);
	expect(job.state).toEqual(before);
	await job.applyRouteDecision(decision);
	const done = job.state;
	await expect(job.applyRouteDecision({ ...decision, rationale: "changed decision" })).rejects.toThrow(
		/different content/,
	);
	expect(job.state).toEqual(done);
});

it("F08 cross-stage historical backtrack makes the supervisor execute work in the recovered stage", async () => {
	const { job, store } = await setup();
	const task = await job.dispatchTask(input(job, "ready-in-reopened-stage"));
	const before = job.state;
	before.frame.activeStageId = "literature";
	before.stages.validation.status = "completed";
	before.stages.literature.status = "running";
	before.tasks[task.id].stageRevision = 2;
	await store.writeSnapshot(before);
	const source = (await ResearchJob.open(store, task.jobId))!;
	await source.applyRouteDecision(route(source, "backtrack", { targetStageId: "validation" }));
	const legacyStore = new MemoryAstraStore();
	for (const saved of await store.readEvents(task.jobId)) {
		const event = saved.event;
		await legacyStore.append(
			task.jobId,
			event.type === "route_decided" ? { type: event.type, decision: event.decision } : event,
		);
	}
	await legacyStore.writeSnapshot(before);
	const legacy = (await ResearchJob.open(legacyStore, task.jobId))!;
	expect(legacy.state.frame.activeStageId).toBe("literature");
	const worker = vi.fn(async (current: TaskPacket) => {
		expect(current.stageId).toBe("validation");
		expect(current.stageRevision).toBe(2);
		return {
			artifactType: current.requiredOutputType,
			content: { content: "actual offline work in recovered stage" },
			refs: [],
		};
	});
	const forbidden = vi.fn(async () => {
		throw new Error("no new planning or model call is authorized in this fixture");
	});
	const supervisor = new ResearchSupervisor(legacy, legacyStore, {
		worker: { run: worker },
		reviewer: { review: forbidden },
		mainAgent: {
			planStage: forbidden,
			decideEvidence: forbidden,
			decideAdoption: forbidden,
			decideSearch: forbidden,
			decideRoute: forbidden,
		},
	});
	const tick = await supervisor.tick();
	expect(worker).toHaveBeenCalledOnce();
	expect(tick.stageId).toBe("validation");
	expect(legacy.state.tasks[task.id].status).toBe("succeeded");
	expect(Object.values(legacy.state.evidence).find((evidence) => evidence.taskId === task.id)?.content).toEqual({
		content: "actual offline work in recovered stage",
	});
	expect(legacy.state.stages.validation.revision).toBe(2);
});

function codexFixture() {
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
	vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
	return new CodexResearchAdapters(
		new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		}),
	);
}

function piFixture(job: ResearchJob, submission?: WorkerSubmission) {
	const runner = new PiChildSessionRunner();
	const run = vi.spyOn(runner, "runTask").mockImplementation(async (task, _role, _text, env = {}) => {
		const cwd = env.ASTRA_EXECUTION_ROOT!;
		await writeFile(join(cwd, "result.json"), "frozen worker result");
		const validated = await validateWorkerSubmission(
			task,
			submission ?? {
				artifactType: task.requiredOutputType,
				content: { content: "saved" },
				refs: [{ kind: "artifact", ref: "result.json", summary: "offline result" }],
			},
			{ executionRoot: cwd, sessionRef: "pi-session:fixture", job },
		);
		await writeWorkerOutputManifest(
			{
				schemaVersion: "astra.worker_output_manifest.v1",
				manifestId: `manifest-${task.id}`,
				jobId: task.jobId,
				taskId: task.id,
				agentId: task.agentId,
				status: "completed",
				artifactType: task.requiredOutputType,
				content: validated.content,
				...(validated.incrementalRevision ? { incrementalRevision: validated.incrementalRevision } : {}),
				outputRefs: validated.outputRefs.map((ref) =>
					["artifact", "log"].includes(ref.kind)
						? { ...ref, ref: `.astra/jobs/${task.jobId}/workspaces/${task.id}/${ref.ref}` }
						: ref,
				),
				validationStatus: "passed",
				validationErrors: [],
				sessionRef: "pi-session:fixture",
				createdAt: new Date().toISOString(),
			},
			task.scope.workspaceRoot,
		);
		return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
	});
	return { adapter: new PiWorkerAdapter(runner), run };
}

it.each(["codex", "pi"] as const)(
	"F06 restores %s's real output manifest after session completion registration fails",
	async (backend) => {
		const { root, job, store } = await setup();
		const task = await job.dispatchTask(input(job, "manifest-completion"));
		await job.setTaskStatus(task.id, "running");
		const adapter = backend === "codex" ? codexFixture() : piFixture(job).adapter;
		const run = vi.spyOn(adapter, "run");
		interrupt(store, (event) => event.type === "child_session_recorded" && event.session.status === "completed");
		await expect(adapter.run(task, job)).rejects.toThrow(/injected/);
		expect(Object.keys(job.state.evidence)).toHaveLength(0);
		await job.requireUserGate({
			kind: "research",
			stageId: "validation",
			question: "Keep the completed result paused?",
			reason: "explicit user question",
		});
		const gate = job.state.frame.userGate;
		const originalVersion = job.state.tasks[task.id].version;
		const recovered = (await ResearchJob.open(store, task.jobId))!;
		const version = recovered.state.tasks[task.id].version!;
		const { hash, ...boundVersion } = version;
		expect(checksum(boundVersion)).toBe(hash);
		await recovered.recoverPendingOperations();
		expect(recovered.state.tasks[task.id].status).toBe("succeeded");
		expect(recovered.state.tasks[task.id].version).toEqual(originalVersion);
		expect(Object.values(recovered.state.evidence)).toHaveLength(1);
		expect(recovered.state.frame.userGate).toEqual(gate);
		expect(recovered.state.paused).toBe(true);
		const evidence = Object.values(recovered.state.evidence)[0];
		if (backend === "pi") {
			await rm(join(taskWorkspacePath(root, task.jobId, task.id), "result.json"));
			expect(
				evidence.files?.some(
					(file) => file.sha256 === createHash("sha256").update("frozen worker result").digest("hex"),
				),
			).toBe(true);
		}
		const done = recovered.state;
		await recovered.recoverPendingOperations();
		expect(recovered.state).toEqual(done);
		expect(run).toHaveBeenCalledOnce();
	},
);

it.each([
	"corrupt-prepare",
	"changed-prepare",
	"missing-frozen",
	"symlink-prepare",
	"wrong-manifest",
	"wrong-prefix",
	"changed-file",
	"symlink-file",
	"wrong-packet",
	"changed-version",
	"stale-stage",
] as const)("F06 rejects %s without another worker call or fallback", async (change) => {
	const { root, job, store } = await setup();
	const task = await job.dispatchTask(input(job, change));
	await job.setTaskStatus(task.id, "running");
	const { adapter, run } = piFixture(job);
	const output = await adapter.run(task, job);
	const path = workerManifestPath(root, task.jobId, task.id);
	if (["corrupt-prepare", "changed-prepare", "missing-frozen", "symlink-prepare"].includes(change)) {
		interrupt(store, (event) => event.type === "evidence_recorded");
		await expect(job.completeWorkerTask(task.id, output)).rejects.toThrow(/injected/);
		const preparedPath = join(taskDir(root, task.jobId, task.id), "evidence-completion.json");
		if (change === "corrupt-prepare") await writeFile(preparedPath, "{broken");
		else if (change === "symlink-prepare") {
			await rm(preparedPath);
			await symlink(path, preparedPath);
		} else if (change === "missing-frozen") {
			const prepared = await readJson<{ evidence: Evidence }>(preparedPath);
			await rm(join(root, ".astra/jobs", task.jobId, "versions/files", prepared.evidence.files![0].sha256));
		} else {
			const prepared = await readJson<{ evidence: Evidence }>(preparedPath);
			prepared.evidence.content = { content: "tampered" };
			await writeFile(preparedPath, JSON.stringify(prepared));
		}
	} else if (change === "wrong-manifest" || change === "wrong-prefix") {
		const manifest = await readJson<WorkerOutputManifest>(path);
		if (change === "wrong-manifest") manifest.agentId = "foreign-worker";
		else
			manifest.outputRefs.find((ref) => ref.kind === "artifact")!.ref =
				`.astra/jobs/another-job/workspaces/${task.id}/result.json`;
		await writeFile(path, JSON.stringify(manifest));
	} else if (change === "changed-file") {
		await writeFile(join(taskWorkspacePath(root, task.jobId, task.id), "result.json"), "altered result");
	} else if (change === "symlink-file") {
		const file = join(taskWorkspacePath(root, task.jobId, task.id), "result.json");
		await rm(file);
		await writeFile(join(root, "outside.json"), "frozen worker result");
		await symlink(join(root, "outside.json"), file);
	} else if (change === "wrong-packet") {
		const packetPath = join(taskDir(root, task.jobId, task.id), "task-packet.json");
		const packet = await readJson<TaskPacket>(packetPath);
		packet.attempt += 1;
		await writeFile(packetPath, JSON.stringify(packet));
	} else {
		const snapshot = job.state;
		if (change === "changed-version") snapshot.tasks[task.id].version!.hash = "bad-hash";
		else snapshot.stages.validation.revision = (snapshot.stages.validation.revision ?? 1) + 1;
		await store.writeSnapshot(snapshot);
	}
	const recovered = (await ResearchJob.open(store, task.jobId))!;
	await expect(recovered.recoverPendingOperations()).rejects.toThrow();
	expect(Object.keys(recovered.state.evidence)).toHaveLength(0);
	expect(recovered.state.tasks[task.id].status).toBe("running");
	expect(run).toHaveBeenCalledOnce();
});

it("F06 recovers incremental manifests using the declared base without merging twice", async () => {
	const { job } = await setup();
	const base = await delivery(job, "incremental-base", {
		stageId: "literature",
		requiredOutputType: "literature:local",
	});
	const task = await job.dispatchTask({
		...input(job, "incremental-repair"),
		stageId: "literature",
		requiredOutputType: "literature:local",
		repairOfEvidenceId: base.evidence.id,
		inputArtifactRefs: [base.evidence.id],
		repairChecks: [{ issueId: "offline-bound-issue", criterion: "verified" }],
	});
	await job.setTaskStatus(task.id, "running");
	const { adapter, run } = piFixture(job, {
		artifactType: task.requiredOutputType,
		content: {},
		refs: [],
		incrementalRevision: {
			baseEvidenceId: base.evidence.id,
			baseHash: checksum(base.evidence.content),
			operations: [
				{
					op: "set",
					path: ["added"],
					value: "one new result",
					issueId: "offline-bound-issue",
					sourceRefs: [],
					reason: "complete the bounded result",
				},
			],
			affectedCriteria: ["verified"],
			rationale: "offline additive repair",
		},
	});
	await adapter.run(task, job);
	await job.recoverPendingOperations();
	const fixed = Object.values(job.state.evidence).find((e) => e.taskId === task.id)!;
	expect(fixed.content).toEqual({ content: "incremental-base", added: "one new result" });
	expect(fixed.incrementalRevision?.resultHash).toBe(checksum(fixed.content));
	expect(fixed.currentEvidenceSetId).toBe(base.evidence.currentEvidenceSetId);
	const done = job.state;
	await job.recoverPendingOperations();
	expect(job.state).toEqual(done);
	expect(run).toHaveBeenCalledOnce();
});

it("F06 rejects succeeded orphan completions and ignores superseded attempts", async () => {
	const { job } = await setup();
	const old = await job.dispatchTask(input(job, "attempts"));
	await job.setTaskStatus(old.id, "succeeded");
	await expect(job.recoverPendingOperations()).rejects.toThrow(/stale completion gap/);
	const retry = await job.dispatchTask({
		...old,
		id: "task_new_attempt",
		replayKey: "new-completion-attempt",
		attempt: 2,
		supersedesTaskId: old.id,
		resumePolicy: "restart-attempt",
	});
	await job.setTaskStatus(retry.id, "ready");
	await job.recoverPendingOperations();
	expect(Object.keys(job.state.evidence)).toHaveLength(0);
	expect(job.state.tasks[old.id].status).toBe("succeeded");
});

it("F06 restores a historical evidence graph without creating another evidence record", async () => {
	const { job, store } = await setup();
	const result = await delivery(job, "historical-graph");
	const legacyStore = new MemoryAstraStore();
	for (const saved of await store.readEvents(result.task.jobId)) {
		const event = saved.event;
		await legacyStore.append(
			saved.jobId,
			event.type === "evidence_recorded" ? { type: event.type, evidence: event.evidence } : event,
		);
	}
	const legacy = (await ResearchJob.open(legacyStore, result.task.jobId))!;
	expect(legacy.state.graph.nodes[`research_evidence_${result.evidence.id}`]).toBeUndefined();
	await legacy.recoverPendingOperations();
	expect(legacy.state.graph.nodes[`research_evidence_${result.evidence.id}`]).toBeDefined();
	expect(Object.values(legacy.state.evidence)).toEqual([result.evidence]);
	const done = legacy.state;
	await legacy.recoverPendingOperations();
	expect(legacy.state).toEqual(done);
});

it("completed worker and route recovery adds no event-history scans beyond the existing write guard", async () => {
	const { job, store } = await setup();
	const result = await delivery(job, "completed");
	await job.applyRouteDecision(route(job, "continue"));
	const history = vi.spyOn(store, "readEvents");
	const done = job.state;
	await job.recoverPendingOperations();
	await job.recoverPendingOperations();
	expect(history).toHaveBeenCalledTimes(2);
	expect(job.state).toEqual(done);
	expect(job.state.graph.nodes[`research_evidence_${result.evidence.id}`]).toBeDefined();
});

it.each(["codex", "pi"] as const)(
	"F09 %s review packets expose every frozen source version with matching bytes",
	async (backend) => {
		const { root, job } = await setup();
		const ref = "https://example.invalid/versioned-source";
		const versions: Evidence[] = [];
		for (const value of ["first", "second", "third"]) {
			await writeSourceReceipt(
				{ workspaceRoot: root, jobId: job.state.frame.jobId, query: value, limit: 1 },
				{ sourceRef: ref, title: value, authors: [] },
				"fixture",
				"2026-10-01T00:00:00.000Z",
			);
			versions.push((await delivery(job, value, {}, [ref])).evidence);
		}
		const target = await delivery(job, "review-target", { inputArtifactRefs: versions.map((e) => e.id) }, [ref]);
		let packet: ReviewPacket;
		let directory: string;
		if (backend === "codex") {
			const reviewed = await codexFixture().review(target.evidence, job);
			directory = taskDir(root, job.state.frame.jobId, reviewed.reviewerTaskId!);
			packet = await readJson<ReviewPacket>(reviewPacketPath(root, job.state.frame.jobId, reviewed.reviewerTaskId!));
		} else {
			const runner = new PiChildSessionRunner();
			vi.spyOn(runner, "runTask").mockImplementation(async (task, _role, _prompt, env = {}) => {
				directory = env.ASTRA_EXECUTION_ROOT!;
				packet = await readJson<ReviewPacket>(join(directory, "review-packet.json"));
				const refs = [`evidence:${target.evidence.id}`, ...packet.resolvedEvidenceRefs.map((entry) => entry.path)];
				await writeReviewerOutputManifest(
					{
						schemaVersion: "astra.reviewer_output_manifest.v1",
						manifestId: "offline-review",
						jobId: task.jobId,
						taskId: task.id,
						evidenceId: target.evidence.id,
						verdict: "pass",
						findings: [],
						score: 1,
						criteria: ["verified"].map((criterion) => ({
							criterion,
							passed: true,
							score: 1,
							evidenceRefs: refs,
							rationale: "all fixture versions inspected",
						})),
						verifiedRefs: refs,
						sessionRef: "pi-session:offline-review",
						createdAt: new Date().toISOString(),
					},
					root,
				);
				return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
			});
			await new PiReviewerAdapter(runner).review(target.evidence, job);
		}
		const entries = packet!.resolvedEvidenceRefs.filter((entry) => entry.sourceRef === ref);
		expect(entries).toHaveLength(3);
		expect(new Set(entries.map((entry) => entry.path)).size).toBe(3);
		const queries: string[] = [];
		for (const entry of entries) {
			const bytes = await readFile(join(directory!, entry.path));
			expect(createHash("sha256").update(bytes).digest("hex")).toBe(entry.sha256);
			queries.push(JSON.parse(bytes.toString()).query);
		}
		expect(queries.sort()).toEqual(["first", "second", "third"]);
	},
);

it.each(["omitted", "undefined", "nested-undefined"] as const)(
	"F06 custom stage %s survives capture, disk storage and recapture unchanged",
	async (shape) => {
		const { searchPolicy: _searchPolicy, ...withoutSearch } = DEFAULT_STAGES[0];
		const stage: StageDefinition =
			shape === "omitted"
				? withoutSearch
				: {
						...withoutSearch,
						searchPolicy:
							shape === "undefined"
								? undefined
								: {
										strategy: "diverse-candidates",
										minCandidates: 2,
										maxCandidates: 2,
										maxRounds: undefined,
										criteria: ["verified"],
									},
					};
		const { job, store } = await setup([stage]);
		const task = await job.dispatchTask(input(job, "custom-version"));
		await job.setTaskStatus(task.id, "running");
		const { adapter } = piFixture(job);
		await adapter.run(task, job);
		const version = job.state.tasks[task.id].version;
		const reopened = (await ResearchJob.open(store, task.jobId))!;
		expect(await reopened.captureTaskVersion(task.id)).toEqual(version);
		await reopened.recoverPendingOperations();
		expect(reopened.state.tasks[task.id].version).toEqual(version);
		expect(reopened.state.tasks[task.id].status).toBe("succeeded");
	},
);

it.each(["default-undefined", "custom-omitted", "unrecoverable-nested-undefined"] as const)(
	"F06 historical %s version retains its exact registered hashes or explicitly refuses unavailable shape",
	async (shape) => {
		const { searchPolicy: _searchPolicy, ...withoutSearch } = DEFAULT_STAGES[0];
		const stage: StageDefinition =
			shape === "default-undefined"
				? DEFAULT_STAGES[0]
				: shape === "custom-omitted"
					? withoutSearch
					: {
							...withoutSearch,
							searchPolicy: {
								strategy: "best-first",
								minCandidates: 2,
								maxCandidates: 2,
								maxRounds: undefined,
								criteria: ["verified"],
							},
						};
		const { root, job, store } = await setup([stage]);
		const task = await job.dispatchTask(input(job, "old-version"));
		await job.setTaskStatus(task.id, "running");
		await piFixture(job).adapter.run(task, job);
		const snapshot = job.state;
		const currentTask = snapshot.tasks[task.id];
		const contractHash = checksum({
			planId: currentTask.planId,
			repairChecks: currentTask.repairChecks,
			deliveryKind: currentTask.deliveryKind,
			stageRevision: currentTask.stageRevision,
			repairOfEvidenceId: currentTask.repairOfEvidenceId,
			stage,
			objective: currentTask.objective,
			requiredOutputFields: currentTask.requiredOutputFields,
			acceptanceChecks: currentTask.acceptanceChecks,
			failureSignals: currentTask.failureSignals,
			successCriteria: currentTask.successCriteria,
		});
		const { hash: _hash, ...versionValue } = currentTask.version!;
		versionValue.contractHash = contractHash;
		const historical = { ...versionValue, hash: checksum(versionValue) };
		currentTask.version = historical;
		await store.writeSnapshot(snapshot);
		await writeFile(join(taskDir(root, task.jobId, task.id), "task-packet.json"), JSON.stringify(currentTask));
		const reopened = (await ResearchJob.open(store, task.jobId))!;
		if (shape === "unrecoverable-nested-undefined") {
			await expect(reopened.captureTaskVersion(task.id)).rejects.toThrow(/version changed/);
			await expect(reopened.recoverPendingOperations()).rejects.toThrow(/version.*stale/);
		} else {
			expect(await reopened.captureTaskVersion(task.id)).toEqual(historical);
			await reopened.recoverPendingOperations();
			expect(reopened.state.tasks[task.id].status).toBe("succeeded");
			expect(reopened.state.tasks[task.id].version).toEqual(historical);
		}
	},
);

it.each(["strategy", "candidate-count", "worker-budget", "objective", "acceptance"] as const)(
	"F06 normalization still refuses an actual %s contract change",
	async (change) => {
		const stage: StageDefinition = {
			...DEFAULT_STAGES[0],
			searchPolicy: {
				strategy: "diverse-candidates",
				minCandidates: 2,
				maxCandidates: 2,
				criteria: ["verified"],
			},
		};
		const { job, store } = await setup([stage]);
		const task = await job.dispatchTask(input(job, "changed-contract"));
		await job.setTaskStatus(task.id, "running");
		await piFixture(job).adapter.run(task, job);
		const snapshot = job.state;
		const definition = snapshot.stageDefinitions!.validation;
		if (change === "strategy") definition.searchPolicy!.strategy = "best-first";
		else if (change === "candidate-count") definition.searchPolicy!.maxCandidates += 1;
		else if (change === "worker-budget") definition.workerBudget!.maxTurns += 1;
		else if (change === "objective") snapshot.tasks[task.id].objective += " changed";
		else snapshot.tasks[task.id].acceptanceChecks.push("new acceptance criterion");
		await store.writeSnapshot(snapshot);
		const reopened = (await ResearchJob.open(store, task.jobId))!;
		await expect(reopened.captureTaskVersion(task.id)).rejects.toThrow(/version changed/);
		await expect(reopened.recoverPendingOperations()).rejects.toThrow();
		expect(reopened.state.tasks[task.id].status).toBe("running");
		expect(Object.keys(reopened.state.evidence)).toHaveLength(0);
	},
);

async function approvedCompletionPlan(job: ResearchJob, ready: boolean) {
	const plan = await job.recordStagePlan(
		{
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "v2-plan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "v2-plan",
			mode: "decompose",
			tasks: [
				{
					key: "work",
					deliveryKind: "local",
					objective: "one completed result",
					inputArtifactRefs: [],
					requiredOutputFields: ["content"],
					acceptanceChecks: ["verified"],
					successCriteria: [],
					failureSignals: [],
				},
			],
			rationale: "offline",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		},
		job.state,
	);
	const evidence = await preparePlanEvidence(job, plan);
	await job.recordReview(
		reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [], blocking: false }),
	);
	if (ready) {
		const contract = buildEffectiveTaskContract(job, plan, plan.tasks[0]);
		await job.dispatchTask({
			...contract,
			effectiveContractHash: semanticContractHash(contract),
			id: `task_${checksum({ planId: plan.id, taskKey: "work", attempt: 1 }).slice(0, 24)}`,
			planId: plan.id,
			replayKey: `stage-plan:${plan.id}:work`,
			attempt: 1,
			role: "worker",
			stageExecutionId: "stage_exec_validation",
			dependencies: [],
			scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		});
	}
}

function completionSupervisor(job: ResearchJob, store: AstraStore, worker: { run: PiWorkerAdapter["run"] }) {
	const forbidden = vi.fn(async () => {
		throw new Error("unexpected further agent invocation");
	});
	return new ResearchSupervisor(job, store, {
		worker,
		reviewer: { review: forbidden },
		mainAgent: {
			planStage: forbidden,
			decideEvidence: forbidden,
			decideAdoption: forbidden,
			decideSearch: forbidden,
			decideRoute: forbidden,
		},
	});
}

it.each(["codex", "pi"] as const)(
	"R1 supervisor keeps %s durable output before retry across ready/new dispatch and append/snapshot failures",
	async (backend) => {
		for (const ready of [false, true])
			for (const snapshot of [false, true]) {
				const { job, store } = await setup();
				await approvedCompletionPlan(job, ready);
				const adapter = backend === "codex" ? codexFixture() : piFixture(job).adapter;
				const run = vi.spyOn(adapter, "run");
				interrupt(
					store,
					(event) => event.type === "child_session_recorded" && event.session.status === "completed",
					snapshot,
				);
				const supervisor = completionSupervisor(job, store, adapter);
				const turns = job.state.budgetUsage!.turnsUsed;
				await supervisor.tick();
				expect(run).toHaveBeenCalledOnce();
				const workers = Object.values(job.state.tasks).filter((task) => task.role === "worker");
				expect(workers).toHaveLength(1);
				expect(workers[0].attempt).toBe(1);
				expect(workers[0].status).toBe("succeeded");
				expect(Object.values(job.state.evidence).filter((e) => e.type !== "stage-plan")).toHaveLength(1);
				expect(job.state.paused).toBe(true);
				expect(job.state.frame.nextAction).toContain("injected completion");
				expect(job.state.budgetUsage!.turnsUsed).toBe(turns + 1);
				await supervisor.tick();
				expect(run).toHaveBeenCalledOnce();
				expect(job.state.budgetUsage!.turnsUsed).toBe(turns + 1);
			}
	},
);

it.each(["registration-fails", "invalid", "stale"] as const)(
	"R1 supervisor preserves durable %s without another attempt",
	async (fault) => {
		const { root, job, store } = await setup();
		await approvedCompletionPlan(job, true);
		const adapter = piFixture(job).adapter;
		const originalRun = adapter.run.bind(adapter);
		const run = vi.spyOn(adapter, "run").mockImplementation(async (task, current) => {
			try {
				return await originalRun(task, current);
			} catch (error) {
				if (fault === "stale") await job.applyRouteDecision(route(job, "backtrack"));
				throw error;
			}
		});
		const append = store.append.bind(store);
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (event.type === "child_session_recorded" && event.session.status === "completed") {
				if (fault === "invalid") {
					const path = workerManifestPath(root, id, event.session.taskId);
					const manifest = await readJson<WorkerOutputManifest>(path);
					manifest.agentId = "foreign-worker";
					await writeFile(path, JSON.stringify(manifest));
				}
				throw new Error("injected session append failure");
			}
			if (
				fault === "registration-fails" &&
				event.type === "evidence_recorded" &&
				event.evidence.type !== "stage-plan"
			)
				throw new Error("injected evidence registration failure");
			return append(id, event);
		});
		const supervisor = completionSupervisor(job, store, adapter);
		await supervisor.tick();
		expect(run).toHaveBeenCalledOnce();
		expect(job.state.paused).toBe(true);
		expect(Object.values(job.state.tasks).filter((task) => task.role === "worker")).toHaveLength(1);
		expect(Object.values(job.state.evidence).filter((e) => e.type !== "stage-plan")).toHaveLength(0);
		const turns = job.state.budgetUsage!.turnsUsed;
		await expect(supervisor.tick()).rejects.toThrow();
		expect(run).toHaveBeenCalledOnce();
		expect(job.state.budgetUsage!.turnsUsed).toBe(turns);
		expect(Object.values(job.state.tasks).filter((task) => task.role === "worker")).toHaveLength(1);
	},
);

it("R1 supervisor still retries execution failure with no durable output", async () => {
	const { job, store } = await setup();
	await approvedCompletionPlan(job, true);
	const run = vi.fn(async () => {
		throw new Error("actual execution failure without result");
	});
	await completionSupervisor(job, store, { run }).tick();
	expect(run).toHaveBeenCalledTimes(2);
	expect(
		Object.values(job.state.tasks)
			.filter((task) => task.role === "worker")
			.map((task) => task.attempt),
	).toEqual([1, 2]);
	expect(job.state.paused).toBe(false);
});

it("R1 saves returned output before auxiliary accounting failure", async () => {
	const { job, store } = await setup();
	await approvedCompletionPlan(job, true);
	const run = vi.fn(async (task: TaskPacket) => ({
		artifactType: task.requiredOutputType,
		content: { content: "returned" },
		refs: [],
	}));
	vi.spyOn(job, "clearProviderBackoff").mockRejectedValueOnce(new Error("injected auxiliary bookkeeping failure"));
	await completionSupervisor(job, store, { run }).tick();
	expect(run).toHaveBeenCalledOnce();
	expect(job.state.paused).toBe(true);
	expect(Object.values(job.state.evidence).filter((e) => e.type !== "stage-plan")).toHaveLength(1);
});

it.each(["snapshot", "replay"] as const)(
	"R2 supervisor's ask-user returned during pause survives %s and installs its gate on resume",
	async (storage) => {
		const { job, store } = await setup();
		const result = await delivery(job, "canonical", { deliveryKind: "stage", requiredOutputType: "validation" });
		await accept(job, result.evidence);
		await job.adoptEvidence(result.evidence.id);
		const forbidden = vi.fn(async () => {
			throw new Error("unexpected agent invocation");
		});
		const decision = route(job, "ask-user", { question: "Which source?", newQuestions: ["Clarify source"] });
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: forbidden },
			reviewer: { review: forbidden },
			mainAgent: {
				planStage: forbidden,
				decideEvidence: forbidden,
				decideAdoption: forbidden,
				decideSearch: forbidden,
				decideRoute: async () => {
					await job.pause("explicit pause while deciding");
					return decision;
				},
			},
		});
		await supervisor.tick();
		expect(job.state.routeDecisions[decision.decisionRef].consequencesCompleted).not.toBe(true);
		expect(job.state.frame.nextAction).toBe("paused: explicit pause while deciding");
		const replayStore = storage === "snapshot" ? store : new MemoryAstraStore();
		if (storage === "replay")
			for (const event of await store.readEvents(job.state.frame.jobId))
				await replayStore.append(event.jobId, event.event);
		const reopened = (await ResearchJob.open(replayStore, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		expect(reopened.state.frame.nextAction).toBe("paused: explicit pause while deciding");
		await reopened.resume();
		expect(reopened.state.paused).toBe(true);
		await completionSupervisor(reopened, replayStore, { run: forbidden }).tick();
		expect(forbidden).not.toHaveBeenCalled();
		expect(reopened.state.frame.userGate).toEqual(
			expect.objectContaining({ kind: "research", question: "Which source?" }),
		);
		await expect(reopened.resume()).rejects.toThrow(/guidance/);
		await reopened.resumeWithGuidance("Use source A");
		await reopened.recoverPendingOperations();
		expect(reopened.state.paused).toBe(false);
		expect(reopened.state.frame.userGate).toBeUndefined();
	},
);

it("R2 approving another stage gate does not answer a pending research question", async () => {
	const { job } = await setup();
	await job.requireUserGate({
		kind: "stage",
		stageId: "validation",
		phase: "route",
		reason: "original stage permission",
	});
	const gate = job.state.frame.userGate;
	await job.applyRouteDecision(route(job, "ask-user", { question: "Still need research answer" }));
	await job.recoverPendingOperations();
	expect(job.state.frame.userGate).toEqual(gate);
	await job.resume();
	expect(job.state.paused).toBe(true);
	expect(job.state.frame.userGate?.kind).toBe("research");
	expect(job.state.frame.userGate).toEqual(expect.objectContaining({ question: "Still need research answer" }));
});

it.each(["route", "guidance"] as const)(
	"R3 records a journal-ordered %s supersession once and avoids scans after reopening",
	async (later) => {
		const { job, store } = await setup();
		await job.applyRouteDecision(
			route(job, "continue", { decisionRef: "old", createdAt: "2099-01-01T00:00:00.000Z" }),
		);
		const memory = new MemoryAstraStore();
		for (const saved of await store.readEvents(job.state.frame.jobId))
			await memory.append(
				saved.jobId,
				saved.event.type === "route_decided"
					? { type: saved.event.type, decision: saved.event.decision }
					: saved.event,
			);
		const recovered = (await ResearchJob.open(memory, job.state.frame.jobId))!;
		if (later === "route")
			await recovered.applyRouteDecision(
				route(recovered, "continue", { decisionRef: "new", createdAt: "2001-01-01T00:00:00.000Z" }),
			);
		else await recovered.recordUserGuidance("Use the current work");
		const read = vi.spyOn(memory, "readEvents");
		await recovered.recoverPendingOperations();
		expect(read).toHaveBeenCalledOnce();
		const old = recovered.state.routeDecisions.old;
		expect(old.consequencesCompleted).not.toBe(true);
		expect("consequencesSupersededBy" in old).toBe(true);
		read.mockClear();
		await recovered.recoverPendingOperations();
		expect(read).not.toHaveBeenCalled();
		const reopened = (await ResearchJob.open(memory, job.state.frame.jobId))!;
		read.mockClear();
		await reopened.recoverPendingOperations();
		expect(read).not.toHaveBeenCalled();
	},
);

it("R2 budget approval restores the pending research gate before scheduling", async () => {
	const { job } = await setup();
	await job.requireUserGate({ kind: "budget", stageId: "validation", limit: "maxTurns", reason: "budget permission" });
	const gate = job.state.frame.userGate;
	await job.applyRouteDecision(route(job, "ask-user", { question: "Which evidence should we use?" }));
	await job.recoverPendingOperations();
	expect(job.state.frame.userGate).toEqual(gate);
	await job.resume();
	expect(job.state.paused).toBe(true);
	expect(job.state.frame.userGate).toEqual(
		expect.objectContaining({ kind: "research", question: "Which evidence should we use?" }),
	);
});

it("R2 another research question keeps its gate and cannot approve the pending question", async () => {
	const { job } = await setup();
	await job.requireUserGate({
		kind: "research",
		stageId: "validation",
		question: "Original question",
		reason: "original research permission",
	});
	const gate = job.state.frame.userGate;
	await job.applyRouteDecision(route(job, "ask-user", { question: "Different pending question" }));
	await job.recoverPendingOperations();
	expect(job.state.frame.userGate).toEqual(gate);
	expect(job.state.routeDecisions["route-ask-user"].consequencesCompleted).not.toBe(true);
	await expect(job.resume()).rejects.toThrow(/guidance/);
	expect(job.state.frame.userGate).toEqual(gate);
});

it("R2 an already installed identical legacy question gate completes once and preserves the pause", async () => {
	const { job, store } = await setup();
	await job.applyRouteDecision(route(job, "ask-user", { question: "Existing same question" }));
	const gate = job.state.frame.userGate!;
	const memory = new MemoryAstraStore();
	for (const saved of await store.readEvents(job.state.frame.jobId))
		await memory.append(
			saved.jobId,
			saved.event.type === "route_decided"
				? { type: saved.event.type, decision: saved.event.decision }
				: saved.event,
		);
	await memory.append(job.state.frame.jobId, { type: "user_gate_required", gate });
	const reopened = (await ResearchJob.open(memory, job.state.frame.jobId))!;
	await reopened.pause("keep the existing question paused");
	const reason = reopened.state.frame.nextAction;
	await reopened.recoverPendingOperations();
	expect(reopened.state.routeDecisions["route-ask-user"].consequencesCompleted).toBe(true);
	expect(reopened.state.frame.userGate).toEqual(gate);
	expect(reopened.state.frame.nextAction).toBe(reason);
	const read = vi.spyOn(memory, "readEvents");
	const append = vi.spyOn(memory, "append");
	await reopened.recoverPendingOperations();
	await reopened.recoverPendingOperations();
	expect(read).not.toHaveBeenCalled();
	expect(append).not.toHaveBeenCalled();
});

it.each(["ask-user", "backtrack"] as const)(
	"R3 recovers a repeated legacy %s decision by its last journal position",
	async (action) => {
		const { job, store } = await setup();
		await job.applyRouteDecision(
			route(job, action, {
				decisionRef: "same",
				question: "Which required source?",
				newQuestions: ["Repeated legacy question"],
			}),
		);
		const memory = new MemoryAstraStore();
		for (const saved of await store.readEvents(job.state.frame.jobId)) {
			const event =
				saved.event.type === "route_decided"
					? { type: saved.event.type, decision: saved.event.decision }
					: saved.event;
			await memory.append(saved.jobId, event);
			if (event.type === "route_decided") await memory.append(saved.jobId, event);
		}
		const reopened = (await ResearchJob.open(memory, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		expect(reopened.state.routeDecisions.same.consequencesSupersededBy).toBeUndefined();
		expect(reopened.state.routeDecisions.same.consequencesCompleted).toBe(true);
		if (action === "ask-user") {
			expect(reopened.state.frame.userGate).toEqual(
				expect.objectContaining({ kind: "research", question: "Which required source?" }),
			);
			expect(reopened.state.paused).toBe(true);
		} else expect(reopened.state.stages.validation.revision).toBe(2);
		expect(
			Object.values(reopened.state.graph.nodes).filter(
				(node) => node.kind === "question" && node.domainRef === "same",
			),
		).toHaveLength(1);
		const done = reopened.state;
		await reopened.recoverPendingOperations();
		expect(reopened.state).toEqual(done);
		const again = (await ResearchJob.open(memory, job.state.frame.jobId))!;
		const reads = vi.spyOn(memory, "readEvents");
		await again.recoverPendingOperations();
		expect(again.state).toEqual(done);
		expect(reads).not.toHaveBeenCalled();
	},
);
