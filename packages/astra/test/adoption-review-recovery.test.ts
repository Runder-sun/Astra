import { chmod, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import {
	canonicalArtifactPath,
	canonicalReceiptPath,
	readJson,
	reviewerManifestPath,
	reviewPacketPath,
	reviewSnapshotPath,
	taskDir,
	writeReviewerOutputManifest,
} from "../src/contracts.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import type {
	AstraEvent,
	Evidence,
	ReviewerOutputManifest,
	ReviewPacket,
	ReviewVerdict,
	TaskPacket,
} from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function setup(separated = false, requiredReviews?: number) {
	const root = await mkdtemp(join(tmpdir(), "astra-adoption-review-"));
	roots.push(root);
	const data = separated ? await mkdtemp(join(tmpdir(), "astra-adoption-data-")) : root;
	if (separated) roots.push(data);
	const store = new JsonlAstraStore(data);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Offline recovery",
		automation: "full",
		...(requiredReviews
			? {
					definitions: DEFAULT_STAGES.map((definition) => ({
						...definition,
						qualityPolicy: { ...definition.qualityPolicy!, minPassingReviews: requiredReviews },
					})),
				}
			: {}),
	});
	return { root, store, job };
}

async function delivery(
	job: ResearchJob,
	key = "original",
	fields: Partial<TaskPacket> = {},
	content: unknown = { content: key },
) {
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: key,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		successCriteria: ["complete"],
		failureSignals: ["missing"],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		...fields,
	});
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content,
		refs: [],
	});
	return evidence;
}

async function accept(job: ResearchJob, evidence: Evidence) {
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
}

function piReviewer(root: string, verdict: ReviewVerdict = "pass") {
	const runner = new PiChildSessionRunner();
	const run = vi
		.spyOn(runner, "run")
		.mockImplementation(async (_cwd, jobId, taskId, _attempt, role, _prompt, env = {}) => {
			expect(role).toBe("reviewer");
			const refs = [`evidence:${env.ASTRA_EVIDENCE_ID}`];
			await writeReviewerOutputManifest(
				{
					schemaVersion: "astra.reviewer_output_manifest.v1",
					manifestId: `manifest-${taskId}`,
					jobId,
					taskId,
					evidenceId: env.ASTRA_EVIDENCE_ID!,
					verdict,
					score: verdict === "pass" ? 1 : 0,
					findings: verdict === "pass" ? [] : ["Missing verified result"],
					verifiedRefs: refs,
					criteria: (JSON.parse(env.ASTRA_REVIEW_CRITERIA!) as string[]).map((criterion) => ({
						criterion,
						passed: verdict === "pass",
						score: verdict === "pass" ? 1 : 0,
						evidenceRefs: refs,
						rationale: "Offline fixture assessment",
					})),
					sessionRef: "offline",
					createdAt: new Date().toISOString(),
				},
				root,
			);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
		});
	return { adapter: new PiReviewerAdapter(runner), run };
}

function supervisor(job: ResearchJob, store: JsonlAstraStore, reviewer: PiReviewerAdapter | CodexResearchAdapters) {
	const forbidden = vi.fn(async () => {
		throw new NonRetryableResearchError("offline guard: no additional model calls");
	});
	return new ResearchSupervisor(job, store, {
		worker: { run: forbidden },
		reviewer,
		mainAgent: {
			planStage: forbidden,
			decideEvidence: forbidden,
			decideAdoption: forbidden,
			decideSearch: forbidden,
			decideRoute: forbidden,
		},
	});
}

function interrupt(
	store: JsonlAstraStore,
	matches: (event: AstraEvent) => boolean,
	snapshot = false,
	persistent = false,
) {
	const append = store.append.bind(store);
	let hit = false;
	return vi.spyOn(store, "append").mockImplementation(async (id, event) => {
		if ((!hit || persistent) && matches(event)) {
			hit = true;
			if (!snapshot) throw new Error("injected registration append failure");
			vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("injected registration snapshot failure"));
		}
		return append(id, event);
	});
}

it.each(["ENOENT", "EACCES"] as const)(
	"A1 refuses real %s before activation and recovers the same artifact",
	async (fault) => {
		const { root, job, store } = await setup(fault === "ENOENT");
		const evidence = await delivery(job);
		await accept(job, evidence);
		const directory = join(root, ".astra/jobs", job.state.frame.jobId, "canonical");
		if (fault === "ENOENT") await rm(root, { recursive: true });
		else {
			await mkdir(directory, { recursive: true });
			await chmod(directory, 0o500);
			await expect(writeFile(join(directory, "permission-probe"), "offline")).rejects.toMatchObject({
				code: "EACCES",
			});
		}
		try {
			await expect(job.adoptEvidence(evidence.id)).rejects.toMatchObject({ code: fault });
			const artifact = Object.values(job.state.canonical)[0];
			expect(artifact.status).toBe("adoption_requested");
			expect(artifact.adoptionCompletedAt).toBeUndefined();
			expect(job.state.canonicalRoute.stageArtifactIds.validation).toBeUndefined();
			expect(job.state.frame.scientificOutcome).toBe("pending");
		} finally {
			if (fault === "ENOENT") await mkdir(root, { recursive: true });
			else await chmod(directory, 0o700);
		}
		const artifactId = Object.keys(job.state.canonical)[0];
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.recoverPendingOperations();
		expect(Object.keys(reopened.state.canonical)).toEqual([artifactId]);
		expect(reopened.state.canonical[artifactId].status).toBe("active");
		expect(await readJson(canonicalArtifactPath(root, job.state.frame.jobId, artifactId))).toEqual(evidence.content);
		const done = reopened.state;
		await reopened.recoverPendingOperations();
		expect(reopened.state).toEqual(done);
	},
);

it("A2 recovers a Pi review append interruption in the same tick", async () => {
	const { root, job, store } = await setup();
	const evidence = await delivery(job);
	const { adapter, run } = piReviewer(root);
	interrupt(store, (event) => event.type === "review_recorded");
	await supervisor(job, store, adapter).tick();
	expect(run).toHaveBeenCalledOnce();
	expect(Object.values(job.state.reviews)).toHaveLength(1);
	expect(Object.values(job.state.tasks).find((task) => task.role === "reviewer")?.status).toBe("succeeded");
	expect(Object.values(job.state.reviews)[0].evidenceId).toBe(evidence.id);
});

async function reviewTarget(job: ResearchJob, kind: "plan" | "evidence") {
	if (kind === "evidence") return delivery(job);
	const plan = await job.recordStagePlan(
		{
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "recovery-plan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "recovery-plan",
			mode: "decompose",
			tasks: [
				{
					key: "work",
					deliveryKind: "local",
					objective: "bounded output",
					inputArtifactRefs: [],
					requiredOutputFields: ["content"],
					acceptanceChecks: ["verified"],
					successCriteria: [],
					failureSignals: ["missing"],
				},
			],
			rationale: "offline",
			sessionRef: "offline",
			createdAt: new Date().toISOString(),
		},
		job.state,
	);
	return preparePlanEvidence(job, plan);
}

function codexReviewer(root: string) {
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
	vi.stubEnv("ASTRA_CODEX_MODEL", "offline-fixture");
	vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "model-calls.jsonl"));
	const adapter = new CodexResearchAdapters(
		new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		}),
	);
	return { adapter, run: vi.spyOn(adapter, "review") };
}

const reviewCases = (["pi", "codex"] as const).flatMap((backend) =>
	(["plan", "evidence"] as const).flatMap((kind) =>
		(["session", "review", "task"] as const).flatMap((point) =>
			[false, true].map((snapshot) => ({ backend, kind, point, snapshot })),
		),
	),
);
it.each(reviewCases)(
	"A2 $backend/$kind recovers $point failure (snapshot=$snapshot) in one tick without another model turn",
	async ({ backend, kind, point, snapshot }) => {
		const { root, job, store } = await setup();
		const evidence = await reviewTarget(job, kind);
		await job.updateBudget({ maxTurns: 1 });
		const { adapter, run } = backend === "pi" ? piReviewer(root) : codexReviewer(root);
		interrupt(
			store,
			(event) =>
				point === "session"
					? event.type === "child_session_recorded" &&
						event.session.role === "reviewer" &&
						event.session.status === "completed"
					: point === "review"
						? event.type === "review_recorded"
						: event.type === "task_status" &&
							event.status === "succeeded" &&
							job.state.tasks[event.taskId]?.role === "reviewer",
			snapshot,
		);
		await supervisor(job, store, adapter).tick();
		expect(run).toHaveBeenCalledOnce();
		const tasks = Object.values(job.state.tasks).filter((task) => task.role === "reviewer");
		expect(tasks).toHaveLength(1);
		expect(tasks[0].status).toBe("succeeded");
		expect(Object.values(job.state.reviews)).toHaveLength(1);
		expect(Object.values(job.state.reviews)[0]).toMatchObject({
			evidenceId: evidence.id,
			reviewerTaskId: tasks[0].id,
			blocking: kind === "evidence",
		});
		expect(job.state.budgetUsage?.turnsUsed).toBe(1);
		expect(job.state.frame.nextAction).not.toContain("infrastructure failure");
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		const before = reopened.state;
		await reopened.recoverPendingOperations();
		expect(reopened.state).toEqual(before);
		if (backend === "codex") {
			const requests = (await readFile(join(root, "model-calls.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line));
			expect(requests.filter((request) => request.method === "turn/start")).toHaveLength(1);
		}
	},
);

it.each(
	(["plan", "evidence"] as const).flatMap((kind) =>
		(["fail", "partial", "blocked"] as const).map((verdict) => ({ kind, verdict })),
	),
)(
	"A2 restarts a complete $kind $verdict once while keeping the question and exhausted budget",
	async ({ kind, verdict }) => {
		const { root, job, store } = await setup();
		const evidence = await reviewTarget(job, kind);
		const { adapter, run } = piReviewer(root, verdict);
		await job.consumeTurns(1);
		await job.updateBudget({ maxTurns: 1 });
		const output = await adapter.review(evidence, job);
		interrupt(store, (event) => event.type === "review_recorded");
		await expect(
			job.recordReview({ ...output, evidenceId: evidence.id, blocking: kind === "evidence" }),
		).rejects.toThrow(/injected/);
		await job.requireUserGate({
			kind: "research",
			stageId: "validation",
			question: "Keep the original result?",
			reason: "explicit user gate",
		});
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		const gate = reopened.state.frame.userGate;
		const nextAction = reopened.state.frame.nextAction;
		await reopened.recoverPendingOperations();
		expect(Object.values(reopened.state.reviews)).toHaveLength(1);
		expect(Object.values(reopened.state.reviews)[0].verdict).toBe(verdict);
		expect(Object.values(reopened.state.obligations)).toHaveLength(kind === "evidence" ? 1 : 0);
		expect(reopened.state.tasks[output.reviewerTaskId!].status).toBe("succeeded");
		expect(reopened.state.budgetUsage?.turnsUsed).toBe(1);
		expect(reopened.state.frame.userGate).toEqual(gate);
		expect(reopened.state.frame.nextAction).toBe(nextAction);
		expect(reopened.state.paused).toBe(true);
		const done = reopened.state;
		await supervisor(reopened, store, adapter).tick();
		expect(run).toHaveBeenCalledOnce();
		expect(Object.keys(reopened.state.reviews)).toEqual(Object.keys(done.reviews));
		expect(Object.keys(reopened.state.obligations)).toEqual(Object.keys(done.obligations));
	},
);

it("A2 recovers a real Pi retry after an attempt=1 failure without output", async () => {
	const { root, job, store } = await setup();
	const evidence = await delivery(job);
	const { adapter, run } = piReviewer(root);
	run.mockResolvedValueOnce({ exitCode: 1, stdout: "", stderr: "no delivery", jsonEvents: [], costUsd: 0 });
	await expect(adapter.review(evidence, job)).rejects.toThrow("no delivery");
	const firstTask = Object.values(job.state.tasks).find((task) => task.role === "reviewer")!;
	expect(firstTask.status).toBe("failed");
	const second = await adapter.review(evidence, job);
	const secondTask = job.state.tasks[second.reviewerTaskId!];
	expect(firstTask.id).not.toBe(secondTask.id);
	expect(firstTask.attempt).toBe(1);
	expect(secondTask.attempt).toBe(1);
	expect(firstTask.replayKey).toBe(secondTask.replayKey);
	await job.consumeTurns(2);
	await job.updateBudget({ maxTurns: 2 });
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	await reopened.recoverPendingOperations();
	expect(Object.values(reopened.state.reviews)).toEqual([expect.objectContaining({ reviewerTaskId: secondTask.id })]);
	expect(reopened.state.tasks[firstTask.id].status).toBe("failed");
	expect(reopened.state.tasks[secondTask.id].status).toBe("succeeded");
	const done = reopened.state;
	await reopened.recoverPendingOperations();
	expect(reopened.state).toEqual(done);
	expect(run).toHaveBeenCalledTimes(2);
	expect(reopened.state.budgetUsage?.turnsUsed).toBe(2);
});

it.each([1, 2])("A2 supervisor recovers search reviews using independent ordinals (N=%s)", async (required) => {
	const { root, job, store } = await setup(false, required);
	const plan = await job.recordStagePlan({
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: "recovery-search",
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: "recovery-search",
		mode: "search",
		tasks: ["first", "second"].map((key) => ({
			key,
			objective: key,
			hypothesis: key,
			inputArtifactRefs: [],
			requiredOutputFields: job.definitions.validation.requiredOutputFields,
			acceptanceChecks: [],
			failureSignals: [],
			successCriteria: [],
		})),
		rationale: "offline search",
		sessionRef: "offline",
		createdAt: new Date().toISOString(),
	});
	const pe = await preparePlanEvidence(job, plan);
	await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
	const batch = Object.values(job.state.searchBatches)[0];
	const candidate = Object.values(batch.candidates)[0];
	const contract = buildEffectiveTaskContract(job, plan, plan.tasks[0]);
	await delivery(job, "candidate", {
		...contract,
		effectiveContractHash: semanticContractHash(contract),
		replayKey: `stage-plan:${plan.id}:${candidate.key}`,
	});
	const second = buildEffectiveTaskContract(job, plan, plan.tasks[1]);
	const blocked = await job.dispatchTask({
		...second,
		effectiveContractHash: semanticContractHash(second),
		replayKey: `stage-plan:${plan.id}:${plan.tasks[1].key}`,
	});
	await job.setTaskStatus(blocked.id, "blocked");
	await job.updateBudget({ maxTurns: required });
	const { adapter, run } = piReviewer(root, "fail");
	interrupt(store, (event) => event.type === "review_recorded");
	await supervisor(job, store, adapter).tick();
	expect(Object.values(job.state.reviews).filter((review) => review.evidenceId !== pe.id)).toEqual([
		expect.objectContaining({ verdict: "fail", blocking: false }),
	]);
	expect(Object.values(job.state.candidateEvaluations)).toHaveLength(1);
	expect(Object.values(job.state.obligations)).toHaveLength(0);
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	await supervisor(reopened, store, adapter).tick();
	expect(Object.values(reopened.state.candidateEvaluations)).toHaveLength(required);
	const reviews = Object.values(reopened.state.reviews).filter((review) => review.evidenceId !== pe.id);
	expect(reviews).toHaveLength(required);
	expect(new Set(reviews.map((review) => reopened.state.tasks[review.reviewerTaskId!].replayKey)).size).toBe(required);
	expect(run).toHaveBeenCalledTimes(required);
	expect(reopened.state.budgetUsage?.turnsUsed).toBe(required);
	await supervisor(reopened, store, adapter).tick();
	expect(run).toHaveBeenCalledTimes(required);
});

it.each([false, true])(
	"A2 rejects parallel Pi deliveries but accepts independent ordinals (independent=%s)",
	async (independent) => {
		const { root, job, store } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		await job.consumeTurns(2);
		await job.updateBudget({ maxTurns: 2 });
		const first = await adapter.review(evidence, job);
		if (independent) await job.recordReview({ ...first, evidenceId: evidence.id });
		const second = await adapter.review(evidence, job);
		const firstTask = job.state.tasks[first.reviewerTaskId!];
		const secondTask = job.state.tasks[second.reviewerTaskId!];
		expect(firstTask.id).not.toBe(secondTask.id);
		expect(firstTask.attempt).toBe(1);
		expect(secondTask.attempt).toBe(1);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		if (independent) {
			expect(firstTask.replayKey).not.toBe(secondTask.replayKey);
			await reopened.recoverPendingOperations();
			expect(Object.values(reopened.state.reviews)).toHaveLength(2);
			expect(reopened.state.tasks[firstTask.id].status).toBe("succeeded");
			expect(reopened.state.tasks[secondTask.id].status).toBe("succeeded");
			const done = reopened.state;
			await reopened.recoverPendingOperations();
			expect(reopened.state).toEqual(done);
		} else {
			expect(firstTask.replayKey).toBe(secondTask.replayKey);
			await expect(supervisor(reopened, store, adapter).tick()).rejects.toThrow("ambiguous");
			expect(Object.values(reopened.state.reviews)).toHaveLength(0);
			expect(Object.values(reopened.state.obligations)).toHaveLength(0);
			expect(reopened.state.paused).toBe(false);
		}
		expect(run).toHaveBeenCalledTimes(2);
		expect(reopened.state.budgetUsage?.turnsUsed).toBe(2);
	},
);

it.each(["pi", "codex"] as const)(
	"A2 persistent %s registration failure stops and restarts without execution or an infrastructure pause",
	async (backend) => {
		const { root, job, store } = await setup();
		await delivery(job);
		const { adapter, run } = backend === "pi" ? piReviewer(root) : codexReviewer(root);
		const fault = interrupt(store, (event) => event.type === "review_recorded", false, true);
		await expect(supervisor(job, store, adapter).tick()).rejects.toThrow(/injected/);
		expect(job.state.paused).toBe(false);
		expect(run).toHaveBeenCalledOnce();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		const turns = reopened.state.budgetUsage?.turnsUsed;
		await expect(supervisor(reopened, store, adapter).tick()).rejects.toThrow(/injected/);
		expect(run).toHaveBeenCalledOnce();
		expect(reopened.state.budgetUsage?.turnsUsed).toBe(turns);
		fault.mockRestore();
		await reopened.recoverPendingOperations();
		expect(Object.values(reopened.state.reviews)).toHaveLength(1);
		expect(Object.values(reopened.state.tasks).filter((task) => task.role === "reviewer")).toHaveLength(1);
	},
);

it.each([
	"materialization-event",
	"receipt-missing",
	"artifact-missing",
	"artifact-corrupt",
	"receipt-corrupt",
	"active-gap",
] as const)("A1 validates existing files before finishing interrupted adoption (%s)", async (fault) => {
	const { root, job, store } = await setup();
	const evidence = await delivery(job);
	await accept(job, evidence);
	interrupt(store, (event) =>
		fault === "materialization-event"
			? event.type === "canonical_artifact_materialized"
			: event.type === "canonical_artifact_status" && event.status === "active",
	);
	await expect(job.adoptEvidence(evidence.id)).rejects.toThrow(/injected/);
	const artifact = Object.values(job.state.canonical)[0];
	const path = canonicalArtifactPath(root, job.state.frame.jobId, artifact.id);
	const receipt = canonicalReceiptPath(root, job.state.frame.jobId, artifact.id);
	if (fault === "receipt-missing") await rm(receipt);
	if (fault === "artifact-missing") await rm(path);
	if (fault === "artifact-corrupt") await writeFile(path, "tampered");
	if (fault === "receipt-corrupt") {
		const saved = await readJson<Record<string, unknown>>(receipt);
		await writeFile(receipt, JSON.stringify({ ...saved, targetPath: join(root, "unrelated.json") }));
	}
	if (fault === "active-gap") {
		const state = job.state;
		state.canonical[artifact.id].status = "active";
		state.canonicalRoute.stageArtifactIds.validation = artifact.id;
		await store.writeSnapshot(state);
		await writeFile(path, "tampered active");
	}
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	if (["artifact-corrupt", "receipt-corrupt", "active-gap"].includes(fault)) {
		const bytes = await readFile(fault === "receipt-corrupt" ? receipt : path, "utf8");
		await expect(reopened.recoverPendingOperations()).rejects.toThrow(/integrity/);
		expect(reopened.state.canonical[artifact.id].adoptionCompletedAt).toBeUndefined();
		expect(await readFile(fault === "receipt-corrupt" ? receipt : path, "utf8")).toBe(bytes);
		return;
	}
	await reopened.recoverPendingOperations();
	expect(reopened.state.canonical[artifact.id].status).toBe("active");
	expect(await readJson(path)).toEqual(evidence.content);
	expect((await readJson<Record<string, unknown>>(receipt)).artifactId).toBe(artifact.id);
	const done = reopened.state;
	await reopened.recoverPendingOperations();
	expect(reopened.state).toEqual(done);
});

it.each([
	"wrong-task",
	"wrong-job",
	"wrong-evidence",
	"wrong-version",
	"task-attempt",
	"wrong-role",
	"worker-contract",
	"stage-contract",
	"duplicate",
	"missing-criterion",
	"foreign-ref",
	"corrupt-manifest",
	"missing-packet",
	"corrupt-snapshot",
	"unsafe-path",
	"symlink-manifest",
	"symlink-directory",
] as const)("A2 refuses %s without registering or executing again", async (fault) => {
	const { root, job, store } = await setup();
	const evidence = await delivery(job);
	const { adapter, run } = piReviewer(root);
	const output = await adapter.review(evidence, job);
	const taskId = output.reviewerTaskId!;
	const path = reviewerManifestPath(root, job.state.frame.jobId, taskId);
	const packetPath = reviewPacketPath(root, job.state.frame.jobId, taskId);
	const snapshotPath = reviewSnapshotPath(root, job.state.frame.jobId, taskId);
	const manifest = await readJson<ReviewerOutputManifest>(path);
	const packet = await readJson<ReviewPacket>(packetPath);
	if (fault === "wrong-task") manifest.taskId = "foreign-task";
	else if (fault === "wrong-job") manifest.jobId = "foreign-job";
	else if (fault === "wrong-evidence") manifest.evidenceId = "foreign-evidence";
	else if (fault === "duplicate") manifest.criteria.push(manifest.criteria[0]);
	else if (fault === "missing-criterion") manifest.criteria.pop();
	else if (fault === "foreign-ref") manifest.verifiedRefs = ["../private-result.json"];
	else if (fault === "wrong-version") packet.targetSnapshotHash = "wrong-version";
	else if (fault === "wrong-role") packet.reviewerRole = "worker" as "reviewer";
	else if (fault === "worker-contract") packet.workerContract.successCriteria = [];
	else if (fault === "stage-contract") packet.stageContract.outputArtifactType = "other-stage";
	else if (fault === "unsafe-path") packet.targetSnapshotRef = join(root, "foreign-snapshot.json");
	await writeFile(path, JSON.stringify(manifest));
	await writeFile(packetPath, JSON.stringify(packet));
	if (fault === "corrupt-manifest") await writeFile(path, "{broken");
	else if (fault === "missing-packet") await rm(packetPath);
	else if (fault === "corrupt-snapshot") await writeFile(snapshotPath, "{}");
	else if (fault === "task-attempt") {
		const taskPath = join(taskDir(root, job.state.frame.jobId, taskId), "task-packet.json");
		const task = await readJson<TaskPacket>(taskPath);
		task.attempt += 1;
		await writeFile(taskPath, JSON.stringify(task));
	} else if (fault === "symlink-manifest") {
		const foreign = join(root, "outside-manifest.json");
		await writeFile(foreign, JSON.stringify(manifest));
		await rm(path);
		await symlink(foreign, path);
	} else if (fault === "symlink-directory") {
		const directory = taskDir(root, job.state.frame.jobId, taskId);
		await rm(directory, { recursive: true });
		await symlink(root, directory);
	}
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	const turns = reopened.state.budgetUsage?.turnsUsed;
	await expect(supervisor(reopened, store, adapter).tick()).rejects.toThrow();
	expect(Object.keys(reopened.state.reviews)).toHaveLength(0);
	expect(run).toHaveBeenCalledOnce();
	expect(reopened.state.budgetUsage?.turnsUsed).toBe(turns);
});

it.each(["content", "refs", "taskVersion", "checksum", "versionHash"] as const)(
	"A2 checks persisted immutable snapshot %s against registered evidence",
	async (field) => {
		const { root, job, store } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		const output = await adapter.review(evidence, job);
		const path = reviewSnapshotPath(root, job.state.frame.jobId, output.reviewerTaskId!);
		const snapshot = await readJson<{ evidence: Record<string, unknown> }>(path);
		snapshot.evidence[field] =
			field === "refs" ? ["foreign"] : field === "content" ? { content: "tampered" } : "foreign";
		await writeFile(path, JSON.stringify(snapshot));
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await expect(reopened.recoverPendingOperations()).rejects.toThrow(/frozen target/);
		expect(Object.keys(reopened.state.reviews)).toHaveLength(0);
		expect(run).toHaveBeenCalledOnce();
	},
);

it.each(["blocked", "superseded", "older-attempt", "older-stage"] as const)(
	"A2 excludes known %s reviewers without reading their invalid manifest",
	async (fault) => {
		const { root, job, store } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		const output = await adapter.review(evidence, job);
		const task = job.state.tasks[output.reviewerTaskId!];
		await writeFile(reviewerManifestPath(root, job.state.frame.jobId, task.id), "{broken");
		const snapshot = job.state;
		if (fault === "blocked") snapshot.tasks[task.id].status = "blocked";
		else if (fault === "older-stage") snapshot.stages.validation.revision = 2;
		else
			snapshot.tasks.replacement = {
				...task,
				id: "replacement",
				attempt: fault === "older-attempt" ? 2 : 1,
				status: "blocked",
				...(fault === "superseded" ? { supersedesTaskId: task.id } : {}),
			};
		await store.writeSnapshot(snapshot);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.pause("explicit user pause");
		await reopened.recoverPendingOperations();
		expect(Object.keys(reopened.state.reviews)).toHaveLength(0);
		expect(run).toHaveBeenCalledOnce();
	},
);

it.each(["missing-incomplete", "missing-completed", "missing-succeeded", "registered-historical"] as const)(
	"A2 handles %s without guessing a substitute delivery",
	async (fault) => {
		const { root, job, store } = await setup();
		const evidence = await delivery(job);
		const { adapter, run } = piReviewer(root);
		const output = await adapter.review(evidence, job);
		const taskId = output.reviewerTaskId!;
		if (fault === "registered-historical") await job.recordReview({ ...output, evidenceId: evidence.id });
		await rm(reviewerManifestPath(root, job.state.frame.jobId, taskId));
		const snapshot = job.state;
		if (fault === "missing-incomplete")
			for (const session of Object.values(snapshot.sessions))
				if (session.taskId === taskId) session.status = "failed";
		if (fault === "missing-succeeded") snapshot.tasks[taskId].status = "succeeded";
		if (fault === "registered-historical") {
			snapshot.stages.validation.revision = 2;
			snapshot.evidence[evidence.id].status = "rejected";
		}
		await store.writeSnapshot(snapshot);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await reopened.pause("explicit user pause");
		if (fault === "missing-completed" || fault === "missing-succeeded")
			await expect(reopened.recoverPendingOperations()).rejects.toThrow(/completion gap/);
		else await reopened.recoverPendingOperations();
		expect(Object.values(reopened.state.reviews)).toHaveLength(fault === "registered-historical" ? 1 : 0);
		if (fault === "registered-historical") expect(reopened.state.tasks[taskId].status).toBe("succeeded");
		expect(reopened.state.paused).toBe(true);
		expect(run).toHaveBeenCalledOnce();
	},
);

it("A1 supervisor stops failed adoption before routing or retirement and recovery spends no model turn", async () => {
	const { root, job, store } = await setup();
	const old = await delivery(
		job,
		"old",
		{ requiredOutputType: "result-to-claim" },
		{ scientificOutcome: "inconclusive", missionCoverage: "insufficient", conclusion: "old conclusion" },
	);
	await accept(job, old);
	const original = await job.adoptEvidence(old.id);
	const replacement = await delivery(
		job,
		"new",
		{ requiredOutputType: "result-to-claim" },
		{ scientificOutcome: "refuted", missionCoverage: "sufficient", conclusion: "new conclusion" },
	);
	await accept(job, replacement);
	const directory = join(root, ".astra/jobs", job.state.frame.jobId, "canonical");
	await chmod(directory, 0o500);
	const route = vi.fn(async () => {
		throw new Error("unexpected route");
	});
	const adopt = vi.fn(async () => ({
		schemaVersion: "astra.main_agent_decision_manifest.v1" as const,
		manifestId: "adopt",
		jobId: job.state.frame.jobId,
		decisionType: "adoption" as const,
		decisionRef: "adopt",
		adopt: true,
		replacementOf: original.id,
		rationale: "offline",
		sessionRef: "offline",
		createdAt: new Date().toISOString(),
	}));
	const options = {
		worker: { run: route },
		reviewer: { review: route },
		mainAgent: {
			planStage: route,
			decideEvidence: route,
			decideAdoption: adopt,
			decideSearch: route,
			decideRoute: route,
		},
	};
	try {
		await expect(new ResearchSupervisor(job, store, options).tick()).rejects.toMatchObject({ code: "EACCES" });
		expect(route).not.toHaveBeenCalled();
		expect(adopt).toHaveBeenCalledOnce();
		expect(job.state.canonical[original.id].status).toBe("active");
		expect(Object.keys(job.state.retiredArtifacts)).toHaveLength(0);
		expect(job.state.frame.scientificOutcome).toBe("inconclusive");
		expect(job.state.frame.scientificOutcomeReason).toBe("old conclusion");
		const turns = job.state.budgetUsage?.turnsUsed;
		await expect(new ResearchSupervisor(job, store, options).tick()).rejects.toMatchObject({ code: "EACCES" });
		expect(job.state.budgetUsage?.turnsUsed).toBe(turns);
		expect(adopt).toHaveBeenCalledOnce();
	} finally {
		await chmod(directory, 0o700);
	}
	await job.recoverPendingOperations();
	expect(job.state.retiredArtifacts[original.id].replacementId).toBe(Object.values(job.state.canonical)[0].id);
	expect(job.state.frame.scientificOutcome).toBe("refuted");
	expect(route).not.toHaveBeenCalled();
});

it("A1 opening active without completion cannot infer a scientific outcome before file verification", async () => {
	const { root, job, store } = await setup();
	const evidence = await delivery(
		job,
		"result",
		{ requiredOutputType: "result-to-claim" },
		{ scientificOutcome: "refuted", missionCoverage: "sufficient" },
	);
	await accept(job, evidence);
	interrupt(store, (event) => event.type === "canonical_artifact_status" && event.status === "active");
	await expect(job.adoptEvidence(evidence.id)).rejects.toThrow(/injected/);
	const state = job.state;
	const artifact = Object.values(state.canonical)[0];
	artifact.status = "active";
	state.canonicalRoute.stageArtifactIds.validation = artifact.id;
	await store.writeSnapshot(state);
	await writeFile(canonicalArtifactPath(root, job.state.frame.jobId, artifact.id), "tampered");
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	expect(reopened.state.frame.scientificOutcome).toBe("pending");
	await expect(reopened.recoverPendingOperations()).rejects.toThrow(/integrity/);
	expect(reopened.state.frame.scientificOutcome).toBe("pending");
});
