import { mkdtemp, readFile, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import type { Static, TSchema } from "typebox";
import { Value } from "typebox/value";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner, type CodexRunOptions, type CodexRunResult } from "../src/codex-app-server.ts";
import {
	mainDecisionManifestPath,
	readMainAgentDelivery,
	writeMainDecisionManifest,
	writeStagePlanManifest,
} from "../src/contracts.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { requestResearchPause } from "../src/pause-control.ts";
import { PiChildSessionRunner, PiMainAgentAdapter } from "../src/pi-child-session.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import type {
	AstraEvent,
	Evidence,
	MainAgentDecisionManifest,
	PlannedTask,
	StageDefinition,
	StagePlanManifest,
	TaskPacket,
	UserGateRequest,
} from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

type Kind = "plan" | "evidence" | "adoption" | "search-selection" | "route";
const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
const definition: StageDefinition = {
	id: "validation",
	label: "Offline validation",
	suggestedInputArtifactTypes: [],
	outputArtifactType: "validation",
	requiredOutputFields: ["content"],
	acceptanceChecks: ["verified content"],
	failureSignals: ["missing content"],
	workerTaskFamily: "validation",
	workerTools: ["read"],
	workspaceWrite: false,
	minSourceRefs: 0,
	gate: "main-agent",
	qualityPolicy: { minPassingReviews: 1, minScore: 0.8, requireResolvableArtifacts: false },
};
function planned(key: string): PlannedTask {
	return {
		key,
		objective: `Evaluate ${key}`,
		hypothesis: key,
		deliveryKind: "stage",
		inputArtifactRefs: [],
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified content"],
		failureSignals: ["missing content"],
		successCriteria: ["verified content"],
		responsibilityBindings: [],
		responsibilityTransfers: [],
	};
}
async function seed(job: ResearchJob, task: TaskPacket) {
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: "Verified offline content" },
		refs: [],
	});
	const review = await job.recordReview(
		reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", score: 1, findings: [], blocking: false }),
	);
	if (task.searchBatchId) await job.recordCandidateEvaluationFromReview(review.id);
	return evidence;
}
async function setup(kind: Kind) {
	const root = await mkdtemp(join(tmpdir(), "astra-main-delivery-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const def =
		kind === "search-selection"
			? {
					...definition,
					searchPolicy: {
						strategy: "diverse-candidates" as const,
						minCandidates: 2,
						maxCandidates: 2,
						maxRounds: 1,
						criteria: ["verified content"],
					},
				}
			: definition;
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Durable offline decisions",
		definitions: [def],
		automation: "full",
		maxTurns: 1,
	});
	let evidence: Evidence | undefined;
	let batchId: string | undefined;
	if (kind === "search-selection") {
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "seed_plan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "seed_plan",
			mode: "search",
			tasks: [planned("a"), planned("b")],
			rationale: "Compare alternatives",
			sessionRef: "offline",
			createdAt: new Date().toISOString(),
		};
		await job.recordStagePlan(plan);
		const pe = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
		for (const task of plan.tasks) {
			const contract = buildEffectiveTaskContract(job, plan, task);
			evidence = await seed(
				job,
				await job.dispatchTask({
					...contract,
					effectiveContractHash: semanticContractHash(contract),
					replayKey: `stage-plan:${plan.id}:${task.key}`,
				}),
			);
		}
		batchId = Object.keys(job.state.searchBatches)[0];
	} else if (kind !== "plan") {
		evidence = await seed(
			job,
			await job.dispatchTask({
				stageId: "validation",
				stageExecutionId: "validation",
				role: "worker",
				objective: "Offline content",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: ["content"],
				acceptanceChecks: ["verified content"],
				failureSignals: ["missing content"],
				successCriteria: ["verified content"],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 1, maxToolCalls: 1, maxRuntimeMs: 1000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
			}),
		);
		if (kind === "adoption" || kind === "route") await job.decideEvidence(evidence.id, true, "seed_acceptance");
		if (kind === "route") await job.adoptEvidence(evidence.id);
	}
	return { root, store, job, evidence, batchId };
}
type Fixture = Awaited<ReturnType<typeof setup>>;
function output(f: Fixture, kind: Kind, negative = false): Record<string, unknown> {
	if (kind === "plan") return { tasks: [planned("planned")], rationale: "Focused offline plan" };
	if (kind === "evidence") return { decision: negative ? "defer" : "accept", rationale: "Reviewed content" };
	if (kind === "adoption") return { adopt: !negative, rationale: "Promote reviewed content" };
	if (kind === "search-selection")
		return {
			selectedCandidateId: Object.keys(f.job.state.searchBatches[f.batchId!].candidates)[0],
			continueSearch: false,
			rationale: "Qualified alternative",
		};
	return {
		routeAction: "ask-user",
		targetStageId: null,
		evidenceRefs: Object.values(f.job.state.canonicalRoute.stageArtifactIds),
		question: "Which research direction?",
		newQuestions: [],
		rationale: "Material preference required",
	};
}
interface RunnerControl {
	calls: number;
	afterDelivery?: (path: string) => Promise<void>;
	pause?: () => Promise<void>;
	negative?: boolean;
	routeAction?: MainAgentDecisionManifest["routeAction"];
}
class OfflinePiRunner extends PiChildSessionRunner {
	fixture: Fixture;
	control: RunnerControl;
	constructor(fixture: Fixture, control: RunnerControl) {
		super();
		this.fixture = fixture;
		this.control = control;
	}
	override async run(
		_cwd: string,
		jobId: string,
		taskId: string,
		_attempt: number,
		_role: "worker" | "reviewer" | "main-agent",
		_prompt: string,
		env: Record<string, string | undefined> = {},
	) {
		this.control.calls++;
		const kind = env.ASTRA_STAGE_PLAN_ID ? "plan" : (env.ASTRA_DECISION_TYPE as Kind);
		const body = output(this.fixture, kind, this.control.negative);
		if (kind === "route" && this.control.routeAction) body.routeAction = this.control.routeAction;
		if (kind === "route" && this.control.routeAction === "backtrack") body.targetStageId = "validation";
		let path: string;
		if (kind === "plan")
			path = await writeStagePlanManifest(
				{
					...body,
					schemaVersion: "astra.stage_plan_manifest.v1",
					id: env.ASTRA_STAGE_PLAN_ID!,
					jobId,
					stageId: env.ASTRA_STAGE_ID!,
					decisionRef: taskId,
					mode: env.ASTRA_PLAN_MODE as StagePlanManifest["mode"],
					...(env.ASTRA_OBLIGATION_ID ? { obligationId: env.ASTRA_OBLIGATION_ID } : {}),
					sessionRef: `pi-session:${env.ASTRA_SESSION_ID}`,
					createdAt: new Date().toISOString(),
				} as StagePlanManifest,
				this.fixture.root,
			);
		else
			path = await writeMainDecisionManifest(
				{
					...body,
					schemaVersion: "astra.main_agent_decision_manifest.v1",
					manifestId: `decision_${taskId}`,
					jobId,
					decisionRef: taskId,
					decisionType: kind,
					stageId: env.ASTRA_STAGE_ID!,
					...(env.ASTRA_EVIDENCE_ID ? { evidenceId: env.ASTRA_EVIDENCE_ID } : {}),
					...(env.ASTRA_SEARCH_BATCH_ID ? { searchBatchId: env.ASTRA_SEARCH_BATCH_ID } : {}),
					sessionRef: `pi-session:${env.ASTRA_SESSION_ID}`,
					createdAt: new Date().toISOString(),
				} as MainAgentDecisionManifest,
				this.fixture.root,
			);
		// Pi omits the inapplicable optional fields, while Codex's provider schema uses null.
		const saved = JSON.parse(await readFile(path, "utf8")) as Record<string, unknown>;
		for (const key of ["targetStageId", "question"]) if (saved[key] === null) delete saved[key];
		await writeFile(path, JSON.stringify(saved));
		await this.control.afterDelivery?.(path);
		await this.control.pause?.();
		return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
	}
	override async waitForManifest<T>(path: string, reader: (path: string) => Promise<T>): Promise<T> {
		return reader(path);
	}
}
class OfflineCodexRunner extends CodexAppServerRunner {
	fixture: Fixture;
	control: RunnerControl;
	constructor(fixture: Fixture, control: RunnerControl) {
		super();
		this.fixture = fixture;
		this.control = control;
	}
	override async run<S extends TSchema>(options: CodexRunOptions<S>): Promise<CodexRunResult<Static<S>>> {
		this.control.calls++;
		if (!("properties" in options.schema)) throw new Error("Offline main agent requires an object schema");
		const properties = options.schema.properties as Record<string, unknown>;
		const kind: Kind = properties.tasks
			? "plan"
			: properties.decision
				? "evidence"
				: properties.adopt
					? "adoption"
					: properties.selectedCandidateId
						? "search-selection"
						: "route";
		const value = output(this.fixture, kind, this.control.negative);
		if (kind === "route" && this.control.routeAction) value.routeAction = this.control.routeAction;
		if (kind === "route" && this.control.routeAction === "backtrack") value.targetStageId = "validation";
		expect(Value.Check(options.schema, value)).toBe(true);
		const threadId = `offline-${this.control.calls}`;
		await options.onThread(threadId);
		await this.control.pause?.();
		return { output: value as Static<S>, threadId, model: "offline", sessionFile: options.logPath };
	}
}
function supervisor(f: Fixture, backend: "pi" | "codex", control: RunnerControl) {
	const forbidden = vi.fn(async () => {
		throw new Error("Unexpected offline invocation");
	});
	const mainAgent =
		backend === "pi"
			? new PiMainAgentAdapter(new OfflinePiRunner(f, control), f.root)
			: new CodexResearchAdapters(new OfflineCodexRunner(f, control));
	return () =>
		new ResearchSupervisor(f.job, f.store, {
			mainAgent,
			worker: { run: forbidden },
			reviewer: { review: forbidden },
		}).tick();
}
function applied(f: Fixture, kind: Kind) {
	if (kind === "plan") return Object.keys(f.job.state.stagePlans).length === 1;
	if (kind === "evidence") return f.job.state.evidence[f.evidence!.id].status === "accepted";
	if (kind === "adoption")
		return Object.values(f.job.state.canonical).some((artifact) => artifact.adoptionCompletedAt);
	if (kind === "search-selection") return f.job.state.searchBatches[f.batchId!].status === "selected";
	return Object.keys(f.job.state.routeDecisions).length === 1;
}
const kinds: Kind[] = ["plan", "evidence", "adoption", "search-selection", "route"];
it("T3 model control returns an independent small object and immediately reflects later events", async () => {
	const f = await setup("plan");
	const backoff = {
		attempt: 1,
		reason: "Provider cooling down",
		startedAt: new Date().toISOString(),
		retryAt: new Date(Date.now() + 60_000).toISOString(),
	};
	await f.job.recordProviderBackoff(backoff);
	const before = f.job.state;
	const control = f.job.modelControl();
	expect(Object.keys(control).sort()).toEqual(["paused", "providerBackoff"]);
	control.paused = true;
	control.providerBackoff!.retryAt = new Date(0).toISOString();
	control.providerBackoff!.reason = "Changed outside the kernel";
	expect(f.job.state).toEqual(before);
	expect(f.job.modelControl()).toEqual({ paused: false, providerBackoff: backoff });
	await f.job.pause("Actual later pause");
	await f.job.clearProviderBackoff();
	expect(f.job.modelControl()).toEqual({ paused: true, providerBackoff: undefined });
});
it.each(
	(["pi", "codex"] as const).flatMap((backend) => (["before", "during"] as const).map((when) => ({ backend, when }))),
)("T1 $backend applies a real pause file $when the model call", async ({ backend, when }) => {
	const f = await setup("plan");
	const request = () =>
		requestResearchPause(f.root, f.job.state.frame.jobId, "Pause requested from a separate entry point");
	const control: RunnerControl = { calls: 0, ...(when === "during" ? { pause: request } : {}) };
	if (when === "before") await request();
	await supervisor(f, backend, control)();
	expect(f.job.modelControl().paused).toBe(true);
	expect(f.job.state.frame.nextAction).toContain("Pause requested from a separate entry point");
	expect(Object.keys(f.job.state.stagePlans)).toHaveLength(0);
	expect(control.calls).toBe(when === "before" ? 0 : 1);
	expect(f.job.state.budgetUsage?.turnsUsed ?? 0).toBe(when === "before" ? 0 : 1);
	if (when === "during") {
		expect(Object.values(f.job.state.mainAgentCalls!)[0]).toMatchObject({ deliveryHash: expect.any(String) });
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await f.job.resume();
		await f.job.recoverMainAgentDeliveries();
		expect(Object.keys(f.job.state.stagePlans)).toHaveLength(1);
		expect(control.calls).toBe(1);
		expect(f.job.state.budgetUsage?.turnsUsed).toBe(1);
	}
});
it.each(["pi", "codex"] as const)("T2 %s reads backoff expiry immediately without caching", async (backend) => {
	const f = await setup("route");
	const now = Date.now();
	await f.job.recordProviderBackoff({
		attempt: 1,
		reason: "Provider cooling down",
		startedAt: new Date(now - 1000).toISOString(),
		retryAt: new Date(now + 1000).toISOString(),
	});
	const clock = vi.spyOn(Date, "now").mockReturnValue(now);
	const control: RunnerControl = { calls: 0 };
	const tick = supervisor(f, backend, control);
	await tick();
	expect(control.calls).toBe(0);
	clock.mockReturnValue(now + 1000);
	await tick();
	expect(control.calls).toBe(1);
	expect(f.job.state.providerBackoff).toBeUndefined();
	expect(f.job.state.frame.userGate).toMatchObject({ kind: "research", question: "Which research direction?" });
	clock.mockReturnValue(now + 2000);
	await tick();
	expect(control.calls).toBe(1);
});
it.each(["pi", "codex"] as const)(
	"Q7 %s rejects a saved plan after real reopening of a synthetic legacy execution",
	async (backend) => {
		const f = await setup("plan");
		// Synthetic journal input uses the supported execution field; actual reopen clears it.
		const originalStore = f.store;
		f.store = new JsonlAstraStore(join(f.root, "legacy-state"));
		for (const saved of await originalStore.readEvents(f.job.state.frame.jobId)) {
			const event = structuredClone(saved.event);
			if (event.type === "job_created") event.snapshot.stages.validation.executionId = "legacy_execution";
			await f.store.withWriteLock(saved.jobId, () => f.store.append(saved.jobId, event));
		}
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		const control: RunnerControl = { calls: 0, pause: () => f.job.pause("Original planner pause") };
		const adapter =
			backend === "pi"
				? new PiMainAgentAdapter(new OfflinePiRunner(f, control), f.root)
				: new CodexResearchAdapters(new OfflineCodexRunner(f, control));
		await adapter.planStage(f.job, undefined, "decompose");
		await f.job.recoverMainAgentDeliveries();
		const call = Object.values(f.job.state.mainAgentCalls!)[0];
		const original = f.job.state.stages.validation;
		await f.job.reopenStage("validation", "real_scientific_reopen", "Reconsider the validation execution");
		expect(f.job.state.stages.validation.revision).toBe((original.revision ?? 1) + 1);
		expect(f.job.state.stages.validation.executionId).not.toBe(original.executionId);
		await f.job.resume();
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await expect(f.job.recoverMainAgentDeliveries()).rejects.toThrow(/stale/);
		expect(Object.keys(f.job.state.stagePlans)).toHaveLength(0);
		expect(Object.values(f.job.state.mainAgentCalls!)[0]).toEqual(call);
		expect(control.calls).toBe(1);
	},
);
it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		(["unapplied", "committed-tail"] as const).map((when) => ({ backend, when })),
	),
)("Q5 $backend preserves repair route permission after $when obligation changes", async ({ backend, when }) => {
	const f = await setup("evidence");
	await f.job.recordReview(
		reviewFixture(f.job, {
			evidenceId: f.evidence!.id,
			verdict: "fail",
			findings: ["Original repair criterion"],
			blocking: true,
		}),
	);
	const obligationId = f.job.state.frame.openObligationIds[0];
	const control: RunnerControl = {
		calls: 0,
		...(when === "unapplied" ? { pause: () => f.job.pause("Pause original repair route") } : {}),
	};
	const adapter =
		backend === "pi"
			? new PiMainAgentAdapter(new OfflinePiRunner(f, control), f.root)
			: new CodexResearchAdapters(new OfflineCodexRunner(f, control));
	await adapter.decideRoute(f.job, f.job.state.obligations[obligationId]);
	if (when === "unapplied") await f.job.recoverMainAgentDeliveries();
	else {
		const append = f.store.append.bind(f.store);
		let interrupted = false;
		vi.spyOn(f.store, "append").mockImplementation(async (id, event) => {
			if (!interrupted && event.type === "route_decided") {
				interrupted = true;
				vi.spyOn(f.store, "writeSnapshot").mockRejectedValueOnce(
					new Error("Original repair route snapshot failed"),
				);
			}
			return append(id, event);
		});
		await expect(f.job.recoverMainAgentDeliveries()).rejects.toThrow("Original repair route snapshot failed");
	}
	await f.job.recordReview(
		reviewFixture(f.job, {
			evidenceId: f.evidence!.id,
			verdict: "fail",
			findings: ["A materially different repair criterion"],
			blocking: true,
		}),
	);
	expect(f.job.state.frame.openObligationIds).toHaveLength(2);
	if (when === "unapplied") await f.job.resume();
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	if (when === "unapplied") {
		const before = f.job.state;
		await expect(f.job.recoverMainAgentDeliveries()).rejects.toThrow(/stale/);
		expect(f.job.state).toEqual(before);
		expect(Object.keys(f.job.state.routeDecisions)).toHaveLength(0);
	} else {
		await f.job.recoverMainAgentDeliveries();
		expect(Object.keys(f.job.state.routeDecisions)).toHaveLength(1);
		expect(Object.values(f.job.state.mainAgentCalls!)[0]).toMatchObject({
			obligationId,
			applied: true,
			completed: true,
		});
		expect(f.job.state.frame.userGate).toMatchObject({ kind: "research", question: "Which research direction?" });
		const done = f.job.state;
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state).toEqual(done);
	}
	expect(control.calls).toBe(1);
});
it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		(["unchanged", "guidance"] as const).map((change) => ({ backend, change })),
	),
)("Q7 $backend legacy acceptance completion preserves basis with $change inputs", async ({ backend, change }) => {
	const f = await setup("search-selection");
	const batch = f.job.state.searchBatches[f.batchId!];
	await f.job.selectSearchCandidate(batch.id, Object.keys(batch.candidates)[0], "original_selection");
	const legacy = f.job.state;
	delete legacy.searchBatches[batch.id].acceptanceCompleted;
	await f.store.writeSnapshot(legacy);
	f.job = (await ResearchJob.open(f.store, legacy.frame.jobId))!;
	const control: RunnerControl = { calls: 0, pause: () => f.job.pause("Pause saved planner before legacy tail") };
	const adapter =
		backend === "pi"
			? new PiMainAgentAdapter(new OfflinePiRunner(f, control), f.root)
			: new CodexResearchAdapters(new OfflineCodexRunner(f, control));
	const plan = await adapter.planStage(f.job, undefined, "decompose");
	await f.job.recoverMainAgentDeliveries();
	const before = f.job.state;
	const originalCall = Object.values(before.mainAgentCalls!)[0];
	const originalFile = await readFile(originalCall.manifestRef, "utf8");
	await f.job.recoverPendingOperations();
	expect(f.job.state.searchBatches[batch.id].acceptanceCompleted).toBe(true);
	const after = f.job.state;
	const { eventSeq: _beforeSeq, updatedAt: _beforeTime, ...beforeDomain } = before;
	const { eventSeq: _afterSeq, updatedAt: _afterTime, ...afterDomain } = structuredClone(after);
	delete beforeDomain.searchBatches[batch.id].acceptanceCompleted;
	delete afterDomain.searchBatches[batch.id].acceptanceCompleted;
	expect(afterDomain).toEqual(beforeDomain);
	expect(Object.values(after.mainAgentCalls!)[0]).toEqual(originalCall);
	expect(await readFile(originalCall.manifestRef, "utf8")).toBe(originalFile);
	await f.job.recoverPendingOperations();
	expect(f.job.state).toEqual(after);
	expect(
		(await f.store.readEvents(before.frame.jobId)).filter(
			(saved) =>
				saved.event.type === "evidence_acceptance_recovered" && saved.event.searchSelection?.batchId === batch.id,
		),
	).toHaveLength(1);
	f.job = (await ResearchJob.open(f.store, before.frame.jobId))!;
	if (change === "guidance") {
		await f.job.resumeWithGuidance("Change the scientific planning direction");
		await expect(f.job.recoverMainAgentDeliveries()).rejects.toThrow(/stale/);
		expect(f.job.state.stagePlans[plan.id]).toBeUndefined();
		expect(control.calls).toBe(1);
		return;
	}
	await f.job.resume();
	await f.job.recoverMainAgentDeliveries();
	expect(f.job.state.stagePlans[plan.id]).toBeDefined();
	expect(control.calls).toBe(1);
	expect(f.job.state.budgetUsage).toEqual(before.budgetUsage);
});
it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		kinds.flatMap((kind) => (["direct", "reopen", "reloaded"] as const).map((mode) => ({ backend, kind, mode }))),
	),
)("Q7 $backend newly created job preserves original $kind basis ($mode)", async ({ backend, kind, mode }) => {
	const f = await setup(kind);
	if (mode === "reloaded") await f.job.reload();
	const control: RunnerControl = { calls: 0 };
	if (mode === "reopen") control.pause = () => f.job.pause("Pause during first direct planner");
	const adapter =
		backend === "pi"
			? new PiMainAgentAdapter(new OfflinePiRunner(f, control), f.root)
			: new CodexResearchAdapters(new OfflineCodexRunner(f, control));
	if (kind === "plan") await adapter.planStage(f.job, undefined, "decompose");
	else if (kind === "evidence") await adapter.decideEvidence(f.evidence!, f.job);
	else if (kind === "adoption") await adapter.decideAdoption(f.evidence!, f.job);
	else if (kind === "search-selection")
		await adapter.decideSearch(
			f.job.state.searchBatches[f.batchId!],
			Object.values(f.job.state.candidateEvaluations),
			f.job,
		);
	else await adapter.decideRoute(f.job);
	if (mode === "reopen") {
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await f.job.resume();
	}
	await f.job.recoverMainAgentDeliveries();
	expect(applied(f, kind)).toBe(true);
	expect(control.calls).toBe(1);
	expect(f.job.state.budgetUsage?.turnsUsed ?? 0).toBe(0);
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	await f.job.recoverMainAgentDeliveries();
	expect(control.calls).toBe(1);
});
for (const backend of ["pi", "codex"] as const)
	for (const kind of kinds) {
		it.each([
			"session",
			"session-snapshot",
			"delivery-append",
			"delivery-snapshot",
			"domain-append",
			"domain-snapshot",
			"finished",
			"finished-snapshot",
		] as const)(`B5 ${backend} ${kind} recovers %s before exhausted budget`, async (phase) => {
			const f = await setup(kind);
			const control: RunnerControl = { calls: 0 };
			const tick = supervisor(f, backend, control);
			const append = f.store.append.bind(f.store);
			let hit = false;
			const domain: AstraEvent["type"] =
				kind === "plan"
					? "stage_plan_recorded"
					: kind === "evidence"
						? "evidence_decided"
						: kind === "adoption"
							? "evidence_adopted"
							: kind === "search-selection"
								? "search_batch_decided"
								: "route_decided";
			vi.spyOn(f.store, "append").mockImplementation(async (id, event) => {
				const target = phase.startsWith("session")
					? event.type === "child_session_recorded" &&
						event.session.role === "main-agent" &&
						event.session.status === "completed"
					: phase.startsWith("delivery")
						? event.type === "main_agent_delivery_recorded"
						: phase.startsWith("domain")
							? event.type === domain
							: event.type === "main_agent_call_finished";
				if (target && !hit) {
					hit = true;
					if (phase.endsWith("snapshot"))
						vi.spyOn(f.store, "writeSnapshot").mockRejectedValueOnce(new Error("Injected snapshot failure"));
					else throw new Error("Injected append failure");
				}
				return append(id, event);
			});
			await tick();
			expect(hit).toBe(true);
			f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
			await tick();
			expect(applied(f, kind)).toBe(true);
			expect(control.calls).toBe(1);
			expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
			expect(Object.values(f.job.state.mainAgentCalls!)[0].completed).toBe(true);
		});
		it.each(["ordinary", "research", "budget", "stage"] as const)(
			`S7 ${backend} ${kind} retains delivery under %s pause`,
			async (pause) => {
				const f = await setup(kind);
				const control: RunnerControl = {
					calls: 0,
					pause: async () => {
						if (pause === "ordinary") await f.job.pause("Pause during call");
						else {
							const gate: UserGateRequest =
								pause === "research"
									? {
											kind: "research",
											stageId: "validation",
											question: "Choose?",
											reason: "Saved research gate",
										}
									: pause === "budget"
										? {
												kind: "budget",
												stageId: "validation",
												limit: "maxTurns",
												reason: "Saved budget gate",
											}
										: { kind: "stage", stageId: "validation", phase: "route", reason: "Saved stage gate" };
							await f.job.requireUserGate(gate);
						}
					},
				};
				const tick = supervisor(f, backend, control);
				await tick();
				const gate = f.job.state.frame.userGate;
				const nextAction = f.job.state.frame.nextAction;
				expect(control.calls).toBe(1);
				expect(applied(f, kind)).toBe(false);
				expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
				const call = Object.values(f.job.state.mainAgentCalls!)[0];
				expect(call.deliveryHash).toBeTruthy();
				expect(call.applied).not.toBe(true);
				f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
				await tick();
				expect(f.job.state.frame.userGate).toEqual(gate);
				expect(f.job.state.frame.nextAction).toBe(nextAction);
				expect(control.calls).toBe(1);
				if (pause === "ordinary" || pause === "stage") {
					control.pause = undefined;
					await f.job.resume();
					await tick();
					expect(applied(f, kind)).toBe(true);
					expect(control.calls).toBe(1);
				}
			},
		);
	}
it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		(["evidence", "adoption"] as const).map((kind) => ({ backend, kind })),
	),
)("B5 $backend negative $kind remains a durable outcome", async ({ backend, kind }) => {
	const f = await setup(kind);
	const control: RunnerControl = { calls: 0, negative: true };
	const tick = supervisor(f, backend, control);
	const append = f.store.append.bind(f.store);
	let hit = false;
	vi.spyOn(f.store, "append").mockImplementation(async (id, event) => {
		if (!hit && event.type === "job_paused" && event.decisionRef) {
			hit = true;
			vi.spyOn(f.store, "writeSnapshot").mockRejectedValueOnce(new Error("Domain snapshot failed"));
		}
		return append(id, event);
	});
	await tick();
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	await tick();
	expect(control.calls).toBe(1);
	expect(f.job.state.paused).toBe(true);
	expect(Object.values(f.job.state.mainAgentCalls!)[0]).toMatchObject({ applied: true, completed: true });
});

it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		(["search", "advance", "complete"] as const).map((action) => ({ backend, action })),
	),
)("M1 $backend repair route cannot apply unauthorized $action", async ({ backend, action }) => {
	const f = await setup("evidence");
	await f.job.recordReview(
		reviewFixture(f.job, {
			evidenceId: f.evidence!.id,
			verdict: "fail",
			findings: ["Repair checked content"],
			blocking: true,
		}),
	);
	const obligationId = f.job.state.frame.openObligationIds[0];
	expect(obligationId).toBeTruthy();
	const control: RunnerControl = { calls: 0, routeAction: action };
	const tick = supervisor(f, backend, control);
	await tick();
	expect(control.calls).toBe(1);
	expect(Object.keys(f.job.state.routeDecisions)).toHaveLength(0);
	expect(Object.values(f.job.state.mainAgentCalls!)[0]).toMatchObject({ type: "route", obligationId });
	expect(f.job.state.stages.validation.lastRouteAction).toBeUndefined();
	expect(Object.values(f.job.state.mainAgentCalls!)[0].applied).not.toBe(true);
	expect(f.job.state.obligations[obligationId].status).toBe("open");
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	await expect(tick()).rejects.toThrow(/repair|obligation/);
	expect(control.calls).toBe(1);
	expect(Object.keys(f.job.state.routeDecisions)).toHaveLength(0);
});
it.each(
	(["pi", "codex"] as const).flatMap((backend) =>
		(["continue", "backtrack", "ask-user"] as const).map((action) => ({ backend, action })),
	),
)("Q4 $backend repair route allows $action once", async ({ backend, action }) => {
	const f = await setup("evidence");
	await f.job.recordReview(
		reviewFixture(f.job, {
			evidenceId: f.evidence!.id,
			verdict: "fail",
			findings: ["Repair checked content"],
			blocking: true,
		}),
	);
	const obligationId = f.job.state.frame.openObligationIds[0];
	const control: RunnerControl = { calls: 0, routeAction: action };
	await supervisor(f, backend, control)();
	expect(control.calls).toBe(1);
	expect(Object.values(f.job.state.routeDecisions)).toHaveLength(1);
	expect(Object.values(f.job.state.routeDecisions)[0].action).toBe(action);
	expect(Object.values(f.job.state.mainAgentCalls!)[0]).toMatchObject({
		type: "route",
		obligationId,
		applied: true,
		completed: true,
	});
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	const before = f.job.state;
	await f.job.recoverMainAgentDeliveries();
	await f.job.recoverPendingOperations();
	expect(f.job.state).toEqual(before);
	expect(control.calls).toBe(1);
});
it.each([
	"jobId",
	"decisionRef",
	"manifestId",
	"decisionType",
	"evidenceId",
	"searchBatchId",
	"stageId",
	"schemaVersion",
	"tasks",
	"symlink",
	"parent-symlink",
])("M1 rejects changed %s before domain effects", async (field) => {
	const f = await setup("evidence");
	const control: RunnerControl = {
		calls: 0,
		afterDelivery: async (path) => {
			if (field === "parent-symlink") {
				const outside = join(f.root, "outside-decisions");
				await rename(dirname(path), outside);
				await symlink(outside, dirname(path));
			} else if (field === "symlink") {
				const outside = join(f.root, "outside.json");
				await writeFile(outside, await readFile(path));
				await rm(path);
				await symlink(outside, path);
			} else {
				const value = JSON.parse(await readFile(path, "utf8")) as Record<string, unknown>;
				value[field] = "foreign";
				await writeFile(path, JSON.stringify(value));
			}
		},
	};
	await supervisor(f, "pi", control)();
	expect(f.job.state.evidence[f.evidence!.id].status).toBe("candidate");
	expect(control.calls).toBe(1);
	expect(Object.values(f.job.state.mainAgentCalls!)[0].applied).not.toBe(true);
});

it("M1 uses the fixed historical file and rejects a mismatched registered path", async () => {
	const f = await setup("evidence");
	const control: RunnerControl = {
		calls: 0,
		afterDelivery: async () => {
			await writeFile(
				mainDecisionManifestPath(f.root, f.job.state.frame.jobId, "evidence"),
				JSON.stringify({ jobId: "foreign-latest" }),
			);
		},
	};
	await supervisor(f, "pi", control)();
	expect(f.job.state.evidence[f.evidence!.id].status).toBe("accepted");
	const call = Object.values(f.job.state.mainAgentCalls!)[0];
	await expect(
		readMainAgentDelivery(f.root, { ...call, manifestRef: join(f.root, "..", "escape.json") }),
	).rejects.toThrow(/registered identity/);
	expect(control.calls).toBe(1);
});

it.each(["invalid-file", "persistent-session"] as const)(
	"B5 %s stops before deciding a second evidence",
	async (fault) => {
		const f = await setup("evidence");
		await seed(
			f.job,
			await f.job.dispatchTask({
				...f.job.state.tasks[f.evidence!.taskId],
				id: undefined,
				replayKey: "second-evidence",
			}),
		);
		await f.job.updateBudget({ maxTurns: 3 });
		const control: RunnerControl = {
			calls: 0,
			afterDelivery:
				fault === "invalid-file"
					? async (path) => {
							const manifest = JSON.parse(await readFile(path, "utf8")) as Record<string, unknown>;
							manifest.jobId = "foreign";
							await writeFile(path, JSON.stringify(manifest));
						}
					: undefined,
		};
		if (fault === "persistent-session") {
			const append = f.store.append.bind(f.store);
			vi.spyOn(f.store, "append").mockImplementation(async (id, event) => {
				if (
					event.type === "child_session_recorded" &&
					event.session.role === "main-agent" &&
					event.session.status === "completed"
				)
					throw new Error("Persistent completion unavailable");
				return append(id, event);
			});
		}
		await supervisor(f, "pi", control)();
		expect(control.calls).toBe(1);
		expect(Object.values(f.job.state.evidence).every((evidence) => evidence.status === "candidate")).toBe(true);
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await expect(supervisor(f, "pi", control)()).rejects.toThrow(
			fault === "invalid-file" ? /identity\/schema/ : /Persistent completion/,
		);
		expect(control.calls).toBe(1);
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
	},
);

it.each(["pi", "codex"] as const)("S7 %s budget increase resumes original saved plan", async (backend) => {
	const f = await setup("plan");
	const control: RunnerControl = {
		calls: 0,
		pause: () =>
			f.job.requireUserGate({
				kind: "budget",
				stageId: "validation",
				limit: "maxTurns",
				reason: "Original budget gate",
			}),
	};
	const tick = supervisor(f, backend, control);
	await tick();
	const call = Object.values(f.job.state.mainAgentCalls!)[0];
	expect(call.applied).not.toBe(true);
	f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
	await f.job.updateBudget({ maxTurns: 2 });
	await f.job.resume();
	control.pause = undefined;
	await tick();
	expect(f.job.state.stagePlans[call.planId!].decisionRef).toBe(call.id);
	expect(control.calls).toBe(1);
});

it.each(["id", "mode", "obligationId", "stageId", "tasks"] as const)(
	"M1 rejects plan %s mismatch before saving a valid delivery",
	async (field) => {
		const f = await setup("plan");
		const control: RunnerControl = {
			calls: 0,
			afterDelivery: async (path) => {
				const manifest = JSON.parse(await readFile(path, "utf8")) as Record<string, unknown>;
				manifest[field] = "foreign";
				await writeFile(path, JSON.stringify(manifest));
			},
		};
		await supervisor(f, "pi", control)();
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		expect(Object.values(f.job.state.mainAgentCalls!)[0].deliveryHash).toBeUndefined();
		expect(control.calls).toBe(1);
	},
);

it("M1 rejects changed bytes after digest registration and guidance-stale saved output", async () => {
	for (const change of ["bytes", "guidance"] as const) {
		const f = await setup("plan");
		const control: RunnerControl = { calls: 0, pause: () => f.job.pause("Original pause") };
		const tick = supervisor(f, "pi", control);
		await tick();
		const call = Object.values(f.job.state.mainAgentCalls!)[0];
		if (change === "bytes") {
			const manifest = JSON.parse(await readFile(call.manifestRef, "utf8")) as StagePlanManifest;
			manifest.rationale = "Changed delivery bytes";
			await writeFile(call.manifestRef, JSON.stringify(manifest));
		} else await f.job.recordUserGuidance("Change the scientific scope");
		await f.job.resume();
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await expect(tick()).rejects.toThrow(change === "bytes" ? /digest/ : /stale/);
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		expect(control.calls).toBe(1);
	}
});
it("B5 persistent completion failure stops with delivery pending and no refund", async () => {
	const f = await setup("plan");
	const control: RunnerControl = { calls: 0 };
	const tick = supervisor(f, "pi", control);
	const append = f.store.append.bind(f.store);
	vi.spyOn(f.store, "append").mockImplementation(async (id, event) => {
		if (
			event.type === "child_session_recorded" &&
			event.session.role === "main-agent" &&
			event.session.status === "completed"
		)
			throw new Error("Persistent session failure");
		return append(id, event);
	});
	await tick();
	expect(control.calls).toBe(1);
	expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
	expect(Object.values(f.job.state.mainAgentCalls!)[0].completed).not.toBe(true);
	await expect(tick()).rejects.toThrow("Persistent session failure");
	expect(control.calls).toBe(1);
	vi.restoreAllMocks();
	await tick();
	expect(applied(f, "plan")).toBe(true);
	expect(control.calls).toBe(1);
});

it.each(["snapshot", "prepared-append", "pause", "budget-pause", "stage-pause", "ordinary-pause"] as const)(
	"M3/S7 worker recovers %s without repeat or false pause",
	async (fault) => {
		const f = await setup("plan");
		const task = await f.job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "Worker tail",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verified content"],
			failureSignals: ["missing content"],
			successCriteria: ["verified content"],
			dependencies: [],
			scope: { workspaceRoot: f.root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 1, maxToolCalls: 1, maxRuntimeMs: 1000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		const append = f.store.append.bind(f.store);
		let hit = false;
		vi.spyOn(f.store, "append").mockImplementation(async (id, event) => {
			if (!hit && event.type === "evidence_recorded") {
				hit = true;
				if (fault === "snapshot")
					vi.spyOn(f.store, "writeSnapshot").mockRejectedValueOnce(new Error("Worker snapshot failed"));
				if (fault === "prepared-append") throw new Error("Worker append failed");
			}
			return append(id, event);
		});
		const forbidden = vi.fn(async () => {
			throw new Error("Unexpected later invocation");
		});
		const run = vi.fn(async () => {
			if (fault === "ordinary-pause") await f.job.pause("Explicit pause while executing");
			if (fault === "budget-pause")
				await f.job.requireUserGate({
					kind: "budget",
					stageId: "validation",
					limit: "maxTurns",
					reason: "Explicit budget gate while executing",
				});
			if (fault === "stage-pause")
				await f.job.requireUserGate({
					kind: "stage",
					stageId: "validation",
					phase: "route",
					reason: "Explicit stage gate while executing",
				});
			if (fault === "pause")
				await f.job.requireUserGate({
					kind: "research",
					stageId: "validation",
					question: "Preserve?",
					reason: "Explicit gate while executing",
				});
			return { artifactType: "validation", content: { content: "Original bytes" }, refs: [] };
		});
		const tick = () =>
			new ResearchSupervisor(f.job, f.store, {
				worker: { run },
				reviewer: { review: forbidden },
				mainAgent: {
					planStage: forbidden,
					decideEvidence: forbidden,
					decideAdoption: forbidden,
					decideSearch: forbidden,
					decideRoute: forbidden,
				},
			}).tick();
		await tick();
		expect(run).toHaveBeenCalledOnce();
		expect(forbidden).not.toHaveBeenCalled();
		expect(f.job.state.tasks[task.id].status).toBe("succeeded");
		expect(Object.values(f.job.state.evidence)).toHaveLength(1);
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
		expect(f.job.state.frame.nextAction).not.toContain("infrastructure failure");
		if (fault === "pause")
			expect(f.job.state.frame.userGate).toMatchObject({
				kind: "research",
				question: "Preserve?",
				reason: "Explicit gate while executing",
			});
		if (fault === "ordinary-pause")
			expect(f.job.state.frame.nextAction).toBe("paused: Explicit pause while executing");
		if (fault === "budget-pause" || fault === "stage-pause")
			expect(f.job.state.frame.userGate).toMatchObject({
				kind: fault === "budget-pause" ? "budget" : "stage",
				reason:
					fault === "budget-pause"
						? "Explicit budget gate while executing"
						: "Explicit stage gate while executing",
			});
		const evidence = Object.values(f.job.state.evidence)[0];
		f.job = (await ResearchJob.open(f.store, f.job.state.frame.jobId))!;
		await tick();
		await tick();
		expect(Object.values(f.job.state.evidence)).toEqual([evidence]);
		expect(run).toHaveBeenCalledOnce();
		expect(forbidden).not.toHaveBeenCalled();
	},
);
