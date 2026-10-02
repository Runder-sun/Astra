import { execFileSync } from "node:child_process";
import type * as fs from "node:fs/promises";
import { lstat, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { researchMilestones } from "../src/progress.ts";
import { ResearchJob } from "../src/research.ts";
import type { AstraStore } from "../src/store.ts";
import { JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import type { AstraEvent, Evidence, MissionFrame, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
const fault = vi.hoisted(() => ({ operation: "", path: "", after: false, fired: false }));
vi.mock("node:fs/promises", async (importOriginal) => {
	const original = await importOriginal<typeof fs>();
	return {
		...original,
		cp: async (...args: Parameters<typeof fs.cp>) => {
			const fail = fault.operation === "cp" && String(args[0]).includes(fault.path) && !fault.fired;
			if (fail) {
				fault.fired = true;
				throw new Error("injected archive copy failure");
			}
			return original.cp(...args);
		},
		rename: async (...args: Parameters<typeof fs.rename>) => {
			const fail = fault.operation === "rename" && String(args[0]).includes(fault.path) && !fault.fired;
			if (fail) {
				fault.fired = true;
				if (fault.after) await original.rename(...args);
				throw new Error("injected resource rename interruption");
			}
			return original.rename(...args);
		},
		rm: async (...args: Parameters<typeof fs.rm>) => {
			const fail =
				["rm", "partial-rm"].includes(fault.operation) && String(args[0]).includes(fault.path) && !fault.fired;
			if (fail) {
				fault.fired = true;
				if (fault.operation === "partial-rm") await original.rm(join(String(args[0]), "first.txt"));
				if (fault.after) await original.rm(...args);
				throw new Error("injected source removal interruption");
			}
			return original.rm(...args);
		},
	};
});

it("T11 resumes a partially deleted source directory after its complete archive exists", async () => {
	const { root, job, store } = await setup(true);
	const e = await evidence(job, "partial directory removal");
	const artifact = await job.adoptEvidence(e.id);
	const source = join(root, ".astra/jobs", job.state.frame.jobId, "workspaces", e.taskId);
	await mkdir(source, { recursive: true });
	await writeFile(join(source, "first.txt"), "first original");
	await writeFile(join(source, "second.txt"), "second original");
	Object.assign(fault, { operation: "partial-rm", path: source, after: false, fired: false });
	await expect(job.reopenStage("validation", "partial-removal", "original reason")).rejects.toThrow(/injected/);
	fault.operation = "";
	const current = (await ResearchJob.open(store, job.state.frame.jobId))!;
	await current.recoverPendingOperations();
	const receipt = current.state.retiredArtifacts[artifact.id];
	expect(receipt.cleanupStatus).toBe("completed");
	const archive = join(receipt.archiveRefs![0], "workspace");
	expect(await readFile(join(archive, "first.txt"), "utf8")).toBe("first original");
	expect(await readFile(join(archive, "second.txt"), "utf8")).toBe("second original");
});

it.each(["changed", "additional", "symlink-type"])(
	"T11 rejects a conflicting source %s while preserving its archive",
	async (mode) => {
		const { root, job } = await setup();
		const e = await evidence(job, "workspace conflict");
		await job.adoptEvidence(e.id);
		const base = join(root, ".astra/jobs", job.state.frame.jobId);
		const source = join(base, "workspaces", e.taskId);
		const target = join(base, "archive/tasks", e.taskId, "workspace");
		await mkdir(source, { recursive: true });
		await mkdir(target, { recursive: true });
		await writeFile(join(target, "same.txt"), "original");
		if (mode === "symlink-type") await symlink(join(target, "same.txt"), join(source, "same.txt"));
		else await writeFile(join(source, "same.txt"), mode === "changed" ? "changed" : "original");
		if (mode === "additional") await writeFile(join(source, "additional.txt"), "additional");
		await expect(job.reopenStage("validation", "conflicting-copy", "reason")).rejects.toThrow(/conflicts/);
		expect(await readFile(join(target, "same.txt"), "utf8")).toBe("original");
		expect(await lstat(join(source, "same.txt"))).toBeDefined();
	},
);
afterEach(async () => {
	fault.operation = "";
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it.each([
	"complete",
	"first-pruned",
	"active-only",
	"off-route",
	"changed-stage",
	"changed-guidance",
	"pending",
	"changed-frozen",
	"foreign-prune",
	"lost-acceptance",
	"changed-current-version",
	"changed-pruned-version",
	"changed-current-id",
	"changed-current-task",
	"changed-current-stage",
	"changed-current-type",
	"changed-current-lineage",
	"changed-pruned-task",
])("T5/T14 restores genuine legacy repair facts without reopening completed work (%s)", async (mode) => {
	// Produced by the frozen pre-fix implementation, including its real frozen contracts and pruning.
	let events = (await readFile(new URL("./fixtures/followup-legacy-repair.jsonl", import.meta.url), "utf8"))
		.trim()
		.split("\n")
		.map((line) => JSON.parse(line) as AstraEvent);
	const originalCreated = events[0];
	if (originalCreated.type !== "job_created") throw new Error("invalid legacy fixture");
	const root = await mkdtemp(join(tmpdir(), "astra-legacy-replay-"));
	roots.push(root);
	// Relocate execution paths, preserving scientific version hashes and input-version identities.
	events = JSON.parse(
		JSON.stringify(events).replaceAll(originalCreated.snapshot.frame.permissions.workspaceRoot, root),
	);
	const contractHashes = new Map<string, string>();
	for (const event of events) {
		if (event.type !== "evidence_recorded" || event.evidence.type !== "stage-plan") continue;
		const content = event.evidence.content as {
			effectiveContracts: Array<{ hash: string; contract: Parameters<typeof semanticContractHash>[0] }>;
		};
		for (const frozen of content.effectiveContracts) {
			const previous = frozen.hash;
			frozen.hash = semanticContractHash(frozen.contract);
			contractHashes.set(previous, frozen.hash);
		}
	}
	for (const event of events) {
		if (event.type === "task_dispatched" && event.task.effectiveContractHash)
			event.task.effectiveContractHash =
				contractHashes.get(event.task.effectiveContractHash) ?? event.task.effectiveContractHash;
	}
	const winner = events.find((event) => event.type === "evidence_adopted");
	if (winner?.type !== "evidence_adopted") throw new Error("invalid legacy fixture");
	if (mode === "lost-acceptance")
		events = events.filter(
			(event) =>
				!(event.type === "evidence_decided" && event.evidenceId === winner.artifact.evidenceId && event.accepted),
		);
	if (mode === "foreign-prune")
		for (const event of events) if (event.type === "evidence_pruned") event.receipt.checksum = "foreign-revision";
	if (mode === "changed-frozen") {
		const frozenPlan = [...events]
			.reverse()
			.find((event) => event.type === "evidence_recorded" && event.evidence.type === "stage-plan");
		if (frozenPlan?.type !== "evidence_recorded") throw new Error("invalid legacy fixture");
		(
			frozenPlan.evidence.content as { effectiveContracts: Array<{ contract: { acceptanceChecks: string[] } }> }
		).effectiveContracts[0].contract.acceptanceChecks.push("unreviewed change");
	}
	const first = events.find((event) => event.type === "evidence_recorded" && event.evidence.type === "validation");
	const active = events.findIndex((event) => event.type === "canonical_artifact_status" && event.status === "active");
	const firstPruned = events.findIndex((event) => event.type === "evidence_pruned");
	const created = events[0];
	if (created.type !== "job_created" || first?.type !== "evidence_recorded") throw new Error("invalid legacy fixture");
	const store = new MemoryAstraStore();
	const limit =
		mode === "pending"
			? active
			: mode === "active-only" || mode.startsWith("changed-current-") || mode.startsWith("changed-pruned-")
				? active + 1
				: mode === "first-pruned"
					? firstPruned + 1
					: events.length;
	for (const event of events.slice(0, limit)) await store.append(created.snapshot.frame.jobId, event);
	const job = (await ResearchJob.open(store, created.snapshot.frame.jobId))!;
	const artifact = Object.values(job.state.canonical)[0];
	const before = job.state;
	if (
		["changed-current-version", "changed-pruned-version", "changed-current-task", "changed-current-lineage"].includes(
			mode,
		)
	) {
		const old = job.state.evidence[first.evidence.id];
		const winnerEvidence = job.state.evidence[artifact.evidenceId];
		let taskId = old.taskId;
		if (mode === "changed-current-task") {
			const { id: _id, agentId: _agentId, version: _version, ...otherInput } = job.state.tasks[old.taskId];
			const other = await job.dispatchTask({ ...otherInput, replayKey: "other-native-comparison" });
			await job.setTaskStatus(other.id, "succeeded");
			taskId = other.id;
		}
		const changedVersion = mode === "changed-current-version" || mode === "changed-pruned-version";
		const changed = await job.recordEvidence({
			id: old.id,
			taskId,
			stageId: old.stageId,
			type: old.type,
			content: old.content,
			refs: changedVersion ? ["https://example.invalid/changed-reference"] : old.refs,
			currentEvidenceSetId: mode === "changed-current-lineage" ? "unrelated-lineage" : old.currentEvidenceSetId,
			supersededByTaskId: winnerEvidence.taskId,
		});
		await job.decideEvidence(changed.id, false);
		expect(changed.checksum).toBe(old.checksum);
		if (changedVersion) expect(changed.versionHash).not.toBe(old.versionHash);
		else expect(changed.versionHash).toBe(old.versionHash);
	}
	if (["changed-pruned-version", "changed-pruned-task"].includes(mode)) {
		const prune = structuredClone(events[firstPruned]);
		if (prune.type !== "evidence_pruned") throw new Error("invalid legacy fixture");
		if (mode === "changed-pruned-task") prune.receipt.taskId = "unrelated-task";
		// Freeze a genuine legacy prune boundary after the public same-id update above.
		await store.append(job.state.frame.jobId, prune);
		await job.reload();
		expect(job.state.evidence[first.evidence.id]).toBeUndefined();
	}
	if (["changed-current-id", "changed-current-stage", "changed-current-type"].includes(mode)) {
		const invalid = job.state;
		const current = invalid.evidence[first.evidence.id];
		if (mode === "changed-current-id") current.id = "unrelated-evidence";
		if (mode === "changed-current-stage") current.stageId = "paper-write";
		if (mode === "changed-current-type") current.type = "unrelated-type";
		await store.writeSnapshot(invalid);
		await job.reload();
	}
	if (["off-route", "changed-stage"].includes(mode)) {
		const invalid = job.state;
		if (mode === "off-route") delete invalid.canonicalRoute.stageArtifactIds.validation;
		else invalid.stages.validation.revision = (invalid.stages.validation.revision ?? 1) + 1;
		await store.writeSnapshot(invalid);
		await job.reload();
	}
	if (mode === "changed-guidance") await job.recordUserGuidance("Change the current approved research scope");
	if (
		[
			"off-route",
			"changed-stage",
			"changed-guidance",
			"pending",
			"changed-frozen",
			"foreign-prune",
			"lost-acceptance",
			"changed-current-version",
			"changed-pruned-version",
			"changed-current-id",
			"changed-current-task",
			"changed-current-stage",
			"changed-current-type",
			"changed-current-lineage",
			"changed-pruned-task",
		].includes(mode)
	) {
		await expect(job.recoverPendingOperations()).rejects.toThrow(/stale|route/);
		expect(job.state.canonical[artifact.id].adoptionCompletedAt).toBeUndefined();
		return;
	}
	await job.recoverPendingOperations();
	expect(job.state.canonical[artifact.id].adoptionCompletedAt).toBeTruthy();
	expect(job.state.canonicalRoute.stageArtifactIds.validation).toBe(artifact.id);
	expect(job.state.evidence[first.evidence.id]).toBeUndefined();
	expect(Object.values(job.state.evidence).filter((entry) => entry.type === "validation")).toHaveLength(1);
	expect(job.state.frame.openObligationIds).toEqual([]);
	if (mode === "complete") {
		expect(job.state.graph).toEqual(before.graph);
		expect(job.state.canonicalRoute).toEqual(before.canonicalRoute);
		expect(job.state.obligations).toEqual(before.obligations);
	}
	const completed = job.state;
	await job.recoverPendingOperations();
	expect(job.state).toEqual(completed);
});

it("T14 audits moved Codex sessions by their exact completed cleanup mapping and rejects damaged archives", async () => {
	const { root, job, store } = await setup(true);
	const task = await job.dispatchTask(taskInput(job, "archive-native-session"));
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
	vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
	const adapters = new CodexResearchAdapters(
		new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		}),
	);
	const output = await adapters.run(task, job);
	await job.setTaskStatus(task.id, "succeeded");
	const e = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: output.content,
		refs: output.refs,
	});
	await job.recordReview(reviewFixture(job, { evidenceId: e.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(e.id, true);
	await job.adoptEvidence(e.id);
	const jobRoot = join(root, ".astra/jobs", job.state.frame.jobId);
	await writeFile(join(jobRoot, "backend.json"), JSON.stringify({ backend: "codex" }));
	const audit = () =>
		JSON.parse(
			execFileSync(
				process.execPath,
				[
					fileURLToPath(new URL("../../../scripts/audit-astra-run.mjs", import.meta.url)),
					root,
					job.state.frame.jobId,
				],
				{ encoding: "utf8" },
			),
		);
	expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(true);
	const original = Object.values(job.state.sessions).find((session) => session.taskId === task.id)!;
	const bytes = await readFile(original.sessionFile!, "utf8");
	await job.reopenStage("validation", "archive-native-reopen", "preserve audited history");
	const archived = job.state.sessions[original.sessionId].sessionFile!;
	expect(await readFile(archived, "utf8")).toBe(bytes);
	expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(true);
	await writeFile(archived, bytes.replaceAll('"modelProvider":"openai"', '"modelProvider":"untrusted"'));
	expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(false);
	await rm(archived);
	expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(false);
	await writeFile(archived, bytes);
	const completed = job.state;
	for (const mode of ["pending", "wrong-task", "wrong-source", "wrong-target"]) {
		const invalid = structuredClone(completed);
		const intent = Object.values(invalid.cleanupIntents!)[0];
		if (mode === "pending") intent.status = "pending";
		else if (mode === "wrong-task") intent.tasks[0].taskId = "unrelated";
		else if (mode === "wrong-source") intent.tasks[0].sessions[0].source += ".unrelated";
		else {
			intent.tasks[0].sessions[0].target += ".unrelated";
			await writeFile(intent.tasks[0].sessions[0].target, bytes);
		}
		await store.writeSnapshot(invalid);
		expect(audit().runtimeIntegrity.codexSessionsVerified, mode).toBe(false);
	}
	await store.writeSnapshot(completed);
	expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(true);
});

it("T11 preserves a reused task's archived session across a second cleanup and refuses a missing archive", async () => {
	const { root, job, store } = await setup(true);
	const first = await evidence(job, "reused delivery");
	const source = join(root, "external-session.jsonl");
	await writeFile(source, "preserved historical session\n");
	await job.recordChildSession({
		sessionId: "reused-thread",
		taskId: first.taskId,
		attempt: 1,
		role: "worker",
		status: "completed",
		sessionFile: source,
		updatedAt: new Date().toISOString(),
	});
	const artifact = await job.adoptEvidence(first.id);
	const second = await job.recordEvidence({
		taskId: first.taskId,
		stageId: first.stageId,
		type: first.type,
		content: { content: "new revision" },
		refs: [],
		currentEvidenceSetId: first.currentEvidenceSetId,
	});
	await job.recordReview(reviewFixture(job, { evidenceId: second.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(second.id, true);
	await job.adoptEvidence(second.id, artifact.id);
	const archived = job.state.sessions["reused-thread"].sessionFile!;
	expect(await readFile(archived, "utf8")).toBe("preserved historical session\n");
	await job.reopenStage("validation", "reused-reopen", "retain the existing archive");
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	await reopened.recoverPendingOperations();
	expect(reopened.state.sessions["reused-thread"].sessionFile).toBe(archived);
	expect(await readFile(archived, "utf8")).toBe("preserved historical session\n");
	const again = reopened.state;
	const intent = Object.values(again.cleanupIntents!).find(
		(entry) => entry.kind === "retirement" && entry.receipt.artifactId !== artifact.id,
	)!;
	intent.status = "pending";
	await store.writeSnapshot(again);
	await rm(archived);
	await reopened.reload();
	await expect(reopened.recoverPendingOperations()).rejects.toThrow(/missing/);
});

async function historical(store: AstraStore, jobId: string, edit: (events: AstraEvent[]) => AstraEvent[]) {
	const copy = new MemoryAstraStore();
	for (const event of edit((await store.readEvents(jobId)).map((entry) => structuredClone(entry.event))))
		await copy.append(jobId, event);
	return { store: copy, job: (await ResearchJob.open(copy, jobId))! };
}

async function frozenEvidence(job: ResearchJob, key: string) {
	let plan: StagePlanManifest = {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: key,
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: key,
		mode: "decompose",
		tasks: [
			{
				key: "work",
				objective: key,
				inputArtifactRefs: [],
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: [],
				failureSignals: [],
				successCriteria: [],
			},
		],
		rationale: "offline frozen plan",
		sessionRef: "fixture",
		createdAt: new Date().toISOString(),
	};
	plan = await job.recordStagePlan(plan, job.state);
	const pe = await preparePlanEvidence(job, plan);
	await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
	const contract = buildEffectiveTaskContract(job, plan, plan.tasks[0]);
	const task = await job.dispatchTask({
		...contract,
		effectiveContractHash: semanticContractHash(contract),
		replayKey: `stage-plan:${plan.id}:work`,
	});
	await job.setTaskStatus(task.id, "succeeded");
	const e = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: Object.fromEntries(task.requiredOutputFields.map((field) => [field, key])),
		refs: [],
	});
	await job.recordReview(reviewFixture(job, { evidenceId: e.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(e.id, true);
	return e;
}

function forbiddenSupervisor(job: ResearchJob, store: AstraStore) {
	const forbidden = vi.fn(async () => {
		throw new NonRetryableResearchError("unexpected model call");
	});
	return {
		forbidden,
		supervisor: new ResearchSupervisor(job, store, {
			worker: { run: forbidden },
			reviewer: { review: forbidden },
			mainAgent: {
				planStage: forbidden,
				decideEvidence: forbidden,
				decideAdoption: forbidden,
				decideSearch: forbidden,
				decideRoute: forbidden,
			},
		}),
	};
}

async function cleanupScenario(kind: "retirement" | "evidence" | "search-candidate") {
	const setupResult = await setup(true);
	const { job } = setupResult;
	if (kind === "retirement") {
		const old = await evidence(job, "retirement");
		const a = await job.adoptEvidence(old.id);
		return {
			...setupResult,
			old,
			canonicalPath: a.materializationRef,
			trigger: (current: ResearchJob) => current.reopenStage("validation", "cleanup-reopen", "preserved reason"),
		};
	}
	if (kind === "evidence") {
		const old = await evidence(job, "superseded", false);
		await job.recordReview(
			reviewFixture(job, { evidenceId: old.id, verdict: "fail", findings: ["historical failure"], blocking: false }),
		);
		await job.decideEvidence(old.id, false);
		const task = await job.dispatchTask(taskInput(job, "winner"));
		await job.setTaskStatus(task.id, "succeeded");
		const winner = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { content: "winner" },
			refs: [],
			currentEvidenceSetId: old.currentEvidenceSetId,
		});
		await job.recordReview(reviewFixture(job, { evidenceId: winner.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(winner.id, true);
		return {
			...setupResult,
			old,
			canonicalPath: undefined,
			trigger: (current: ResearchJob) => current.adoptEvidence(winner.id),
		};
	}
	const plan: StagePlanManifest = {
		schemaVersion: "astra.stage_plan_manifest.v1",
		id: "cleanup-search",
		jobId: job.state.frame.jobId,
		stageId: "validation",
		decisionRef: "cleanup-search",
		mode: "search",
		tasks: ["winner", "loser"].map((key) => ({
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
		sessionRef: "fixture",
		createdAt: "now",
	};
	await job.recordStagePlan(plan);
	const pe = await preparePlanEvidence(job, plan);
	await job.recordReview(reviewFixture(job, { evidenceId: pe.id, verdict: "pass", findings: [], blocking: false }));
	const batch = Object.values(job.state.searchBatches)[0];
	const candidates = Object.values(batch.candidates);
	let old: Evidence | undefined;
	for (const [index, candidate] of candidates.entries()) {
		const contract = buildEffectiveTaskContract(job, plan, plan.tasks[index]);
		const task = await job.dispatchTask({
			...contract,
			effectiveContractHash: semanticContractHash(contract),
			replayKey: `stage-plan:${plan.id}:${candidate.key}`,
		});
		await job.setTaskStatus(task.id, "succeeded");
		const item = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { content: candidate.key },
			refs: [],
		});
		const review = await job.recordReview(
			reviewFixture(job, {
				evidenceId: item.id,
				verdict: index === 0 ? "pass" : "fail",
				findings: index === 0 ? [] : ["preserve failed alternative"],
				criteria: [...new Set([...task.acceptanceChecks, ...batch.criteria])].map((criterion) => ({
					criterion,
					passed: index === 0,
					score: index === 0 ? 1 : 0,
					evidenceRefs: [item.id],
					rationale: "offline candidate assessment",
				})),
			}),
		);
		await job.recordCandidateEvaluationFromReview(review.id);
		if (index === 1) old = item;
	}
	return {
		...setupResult,
		old: old!,
		canonicalPath: undefined,
		trigger: (current: ResearchJob) => current.selectSearchCandidate(batch.id, candidates[0].id, "selection"),
	};
}

it.each(["retirement", "evidence", "search-candidate"] as const)(
	"T10 %s rejects failed initial intent and stale writer before file mutation",
	async (kind) => {
		const { root, job, store, old, canonicalPath, trigger } = await cleanupScenario(kind);
		const base = join(root, ".astra/jobs", job.state.frame.jobId);
		const paths = [
			...(canonicalPath ? [canonicalPath] : []),
			join(base, "workspaces", old.taskId, "result"),
			join(base, "tasks", old.taskId, "packet"),
			join(base, "resources", old.taskId, "weights"),
			join(root, "external-session"),
			join(base, "archive/tasks", old.taskId, "resources/prior"),
		];
		for (const path of paths) {
			await mkdir(dirname(path), { recursive: true });
			await writeFile(path, path);
		}
		await job.recordChildSession({
			sessionId: "external",
			role: "worker",
			taskId: old.taskId,
			status: "completed",
			attempt: 1,
			sessionFile: join(root, "external-session"),
			updatedAt: "now",
		});
		const append = store.append.bind(store);
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (
				event.type === "stage_reopened" ||
				event.type === "cleanup_requested" ||
				(event.type === "canonical_artifact_status" && event.status === "active")
			)
				throw new Error("intent append rejected");
			return append(id, event);
		});
		await expect(trigger(job)).rejects.toThrow(/intent append/);
		for (const path of paths) expect(await readFile(path, "utf8")).toBe(path);
		vi.restoreAllMocks();
		const stale = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await job.consumeTurns(1);
		await expect(trigger(stale)).rejects.toThrow(/stale/);
		for (const path of paths) expect(await readFile(path, "utf8")).toBe(path);
	},
);

it.each(["evidence", "search-candidate"] as const)(
	"T11 %s keeps cleanup identity/archive refs after completed append failure",
	async (kind) => {
		const { root, job, store, old, trigger } = await cleanupScenario(kind);
		const base = join(root, ".astra/jobs", job.state.frame.jobId);
		const paths = [
			join(base, "workspaces", old.taskId, "result"),
			join(base, "resources", old.taskId, "weights"),
			join(root, "external"),
		];
		for (const path of paths) {
			await mkdir(dirname(path), { recursive: true });
			await writeFile(path, path);
		}
		await job.recordChildSession({
			sessionId: "external",
			role: "worker",
			taskId: old.taskId,
			status: "completed",
			attempt: 1,
			sessionFile: paths[2],
			updatedAt: "now",
		});
		const append = store.append.bind(store);
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (event.type === "cleanup_completed") throw new Error("completion append rejected");
			return append(id, event);
		});
		await expect(trigger(job)).rejects.toThrow(/completion append/);
		const intent = Object.values(job.state.cleanupIntents!).find((pending) => pending.kind === kind)!;
		expect(intent.status).toBe("pending");
		vi.restoreAllMocks();
		const current = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await current.recoverPendingOperations();
		const done = current.state.cleanupIntents![intent.id];
		expect(done.status).toBe("completed");
		expect(done.archiveRefs).toHaveLength(1);
		expect(await readFile(join(done.archiveRefs![0], "workspace/result"), "utf8")).toBe(paths[0]);
		expect(await readFile(join(done.archiveRefs![0], "resources/weights"), "utf8")).toBe(paths[1]);
		expect(await readFile(current.state.sessions.external.sessionFile!, "utf8")).toBe(paths[2]);
		if (done.kind === "search-candidate") {
			const receipt = current.state.discardedCandidates[done.receipt.candidateId];
			expect(receipt.archivedObligations?.[0].status).toBe("open");
			const prior = current.state;
			await current.recoverPendingOperations();
			expect(current.state).toEqual(prior);
		}
	},
);

it("T3 serializes budget lowering with reservations and does not release a successor lease", async () => {
	const { job } = await setup(false, { maxTurns: 3 });
	const result = await Promise.allSettled([job.consumeTurns(2), job.updateBudget({ maxTurns: 1 })]);
	expect(result[0].status).toBe("fulfilled");
	expect(result[1].status).toBe("rejected");
	await job.acquireLease("expired", -1);
	await Promise.all([job.acquireLease("successor"), job.releaseLease("expired")]);
	expect(job.state.lease?.owner).toBe("successor");
});

it("T4 preserves the previous active bytes until the replacement activation commits", async () => {
	const { job, store } = await setup(true);
	const old = await job.adoptEvidence((await evidence(job, "old")).id);
	const bytes = await readFile(old.materializationRef!);
	const replacement = await evidence(job, "new");
	const append = store.append.bind(store);
	let failed = false;
	vi.spyOn(store, "append").mockImplementation(async (id, event) => {
		if (!failed && event.type === "canonical_artifact_status" && event.status === "active") {
			failed = true;
			throw new Error("activation append failed");
		}
		return append(id, event);
	});
	await expect(job.adoptEvidence(replacement.id, old.id)).rejects.toThrow(/activation append/);
	expect(await readFile(old.materializationRef!)).toEqual(bytes);
	expect(job.state.canonical[old.id].status).toBe("active");
	expect(job.state.canonicalRoute.stageArtifactIds.validation).toBe(old.id);
	const pending = Object.values(job.state.canonical).find((a) => a.evidenceId === replacement.id)!;
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	const next = await reopened.adoptEvidence(replacement.id);
	expect(next.id).toBe(pending.id);
	expect(next.replacementOf).toBe(old.id);
	expect(reopened.state.retiredArtifacts[old.id].replacementId).toBe(next.id);
});

it("T5 completes historical active tails and reuses partial random claim identities", async () => {
	const { job, store } = await setup();
	const e = await evidence(job, "historical claim", true, true);
	const a = await job.adoptEvidence(e.id);
	const legacy = await historical(store, job.state.frame.jobId, (events) =>
		events.map((event) => {
			if (event.type !== "canonical_artifact_status" || event.status !== "active") return event;
			return { type: event.type, artifactId: event.artifactId, status: event.status };
		}),
	);
	expect(researchMilestones(legacy.job.state).find((m) => m.stageId === "result-to-claim")?.status).not.toBe(
		"adopted",
	);
	const state = legacy.job.state;
	const claim = job.state.graph.nodes[job.state.graph.acceptedClaimIds[0]];
	claim.id = "historical_random_claim";
	state.graph.nodes[claim.id] = claim;
	state.graph.acceptedClaimIds = [claim.id];
	await legacy.store.writeSnapshot(state);
	await legacy.job.reload();
	await legacy.job.recoverPendingOperations();
	expect(legacy.job.state.graph.acceptedClaimIds).toEqual([claim.id]);
	expect(legacy.job.state.canonical[a.id].adoptionCompletedAt).toBeTruthy();
	expect(legacy.job.state.frame.scientificOutcome).toBe("supported");
	const completed = legacy.job.state;
	await legacy.job.recoverPendingOperations();
	expect(legacy.job.state).toEqual(completed);
	await expect(legacy.job.adoptEvidence(e.id, "conflicting")).rejects.toThrow(/repeated/);
	const stale = legacy.job.state;
	stale.canonical[a.id].status = "stale";
	delete stale.canonical[a.id].adoptionCompletedAt;
	await legacy.store.writeSnapshot(stale);
	await legacy.job.reload();
	await expect(legacy.job.adoptEvidence(e.id)).rejects.toThrow(/repeated/);
});

it.each(["paused", "budget", "failure"])("T6 supervisor recovers before new model calls (%s)", async (mode) => {
	const { job, store } = await setup(true);
	const e = await frozenEvidence(job, "frozen-recovery");
	failSnapshotAfter(store, (event) => event.type === "evidence_adopted");
	await expect(job.adoptEvidence(e.id)).rejects.toThrow(/snapshot/);
	if (mode === "paused") await job.pause("preserve pause");
	if (mode === "budget") {
		await job.consumeTurns(1);
		await job.updateBudget({ maxTurns: 1 });
	}
	if (mode === "failure") {
		vi.spyOn(store, "append").mockImplementationOnce(async () => {
			throw new Error("recovery append unavailable");
		});
	}
	const current = (await ResearchJob.open(store, job.state.frame.jobId))!;
	const { supervisor, forbidden } = forbiddenSupervisor(current, store);
	// Lease persistence precedes recovery; inject into the pending adoption rather than lease.
	if (mode === "failure") {
		vi.restoreAllMocks();
		const append = store.append.bind(store);
		vi.spyOn(store, "append").mockImplementation(async (id, event) => {
			if (event.type === "canonical_artifact_materialized") throw new Error("recovery append unavailable");
			return append(id, event);
		});
		await expect(supervisor.tick()).rejects.toThrow(/recovery append/);
	} else await supervisor.tick();
	expect(forbidden).not.toHaveBeenCalled();
	expect(current.state.budgetUsage?.turnsUsed).toBe(mode === "budget" ? 1 : 0);
	expect(Object.values(current.state.canonical)[0].status).toBe(mode === "failure" ? "adoption_requested" : "active");
	expect(current.state.paused).toBe(mode !== "failure");
	if (mode === "paused") expect(current.state.frame.nextAction).toBe("paused: preserve pause");
});

it.each(["review-only", "objection-only", "resolved", "missing-resolved-objection"])(
	"T8 restores only missing historical review consequences (%s)",
	async (mode) => {
		const { job, store } = await setup();
		const e = await evidence(job, "old failure", false);
		const review = await job.recordReview(
			reviewFixture(job, { evidenceId: e.id, verdict: "fail", findings: ["old finding"] }),
		);
		const issue = Object.values(job.state.obligations)[0];
		const objection = job.state.graph.nodes[issue.graphObjectionId!];
		const legacy = await historical(store, job.state.frame.jobId, (events) =>
			events.map((event) => (event.type === "review_recorded" ? { type: event.type, review: event.review } : event)),
		);
		if (mode !== "review-only") {
			const state = legacy.job.state;
			if (mode !== "missing-resolved-objection")
				state.graph.nodes[objection.id] = { ...objection, status: mode === "resolved" ? "resolved" : "open" };
			if (mode === "resolved" || mode === "missing-resolved-objection") {
				state.obligations[issue.id] = { ...issue, status: "resolved" };
				state.frame.openObligationIds = [];
			}
			await legacy.store.writeSnapshot(state);
			await legacy.job.reload();
		}
		await legacy.job.recoverPendingOperations();
		const recovered = Object.values(legacy.job.state.obligations).find((o) => o.sourceReviewId === review.id)!;
		expect(recovered.graphObjectionId).toBe(
			mode === "review-only" ? `research_objection_${review.id}` : objection.id,
		);
		expect(recovered.status).toBe(mode.includes("resolved") ? "resolved" : "open");
		expect(legacy.job.state.graph.nodes[recovered.graphObjectionId!].status).toBe(recovered.status);
		const done = legacy.job.state;
		await legacy.job.recoverPendingOperations();
		expect(legacy.job.state).toEqual(done);
	},
);

it("T7 deduplicates concurrent same-lineage repair criteria and keeps nonblocking review nonblocking", async () => {
	const { job } = await setup();
	const e = await evidence(job, "repeated review", false);
	await Promise.all(
		[1, 2].map(() =>
			job.recordReview(reviewFixture(job, { evidenceId: e.id, verdict: "fail", findings: ["same finding"] })),
		),
	);
	const items = Object.values(job.state.obligations).flatMap((o) => o.items ?? []);
	expect(
		items
			.map((item) => job.normalizedRepairCriterion(item.criterion))
			.filter((criterion) => criterion === job.normalizedRepairCriterion("same finding")),
	).toHaveLength(1);
	const before = job.state.frame.openObligationIds.length;
	await job.recordReview(
		reviewFixture(job, { evidenceId: e.id, verdict: "blocked", findings: ["nonblocking"], blocking: false }),
	);
	expect(job.state.frame.openObligationIds).toHaveLength(before);
});

it("T9 preserves a committed reviewer when its review snapshot fails", async () => {
	const { job, store } = await setup();
	const accepted = await frozenEvidence(job, "reviewer-snapshot");
	const snapshot = job.state;
	snapshot.evidence[accepted.id].status = "candidate";
	for (const [id, review] of Object.entries(snapshot.reviews))
		if (review.evidenceId === accepted.id) delete snapshot.reviews[id];
	await store.writeSnapshot(snapshot);
	await job.reload();
	const reviewer = await job.dispatchTask({ ...taskInput(job, "reviewer"), role: "reviewer" });
	await job.setTaskStatus(reviewer.id, "succeeded");
	failSnapshotAfter(store, (event) => event.type === "review_recorded" && event.review.reviewerTaskId === reviewer.id);
	const adapters = forbiddenSupervisor(job, store);
	const review = vi.fn(async () =>
		reviewFixture(job, {
			evidenceId: accepted.id,
			reviewerTaskId: reviewer.id,
			verdict: "fail",
			findings: ["missing evidence"],
		}),
	);
	const supervisor = new ResearchSupervisor(job, store, {
		worker: { run: adapters.forbidden },
		reviewer: { review },
		mainAgent: {
			planStage: adapters.forbidden,
			decideEvidence: adapters.forbidden,
			decideAdoption: adapters.forbidden,
			decideSearch: adapters.forbidden,
			decideRoute: adapters.forbidden,
		},
	});
	await supervisor.tick();
	expect(job.state.tasks[reviewer.id].status).toBe("succeeded");
	expect(Object.values(job.state.reviews).some((r) => r.reviewerTaskId === reviewer.id)).toBe(true);
	expect(Object.values(job.state.reviews).filter((r) => r.reviewerTaskId === reviewer.id)).toHaveLength(1);
	await expect(job.failUncommittedReviewerTask(reviewer.id)).rejects.toThrow("committed");
	await supervisor.tick();
	expect(review).toHaveBeenCalledOnce();
});

it.each(["intent-snapshot", "copy", "rename", "remove", "completion-append", "completion-snapshot"])(
	"T11 resumes archive cleanup after %s interruption",
	async (mode) => {
		const { root, job, store } = await setup(true);
		const e = await evidence(job, "cleanup interruption");
		const a = await job.adoptEvidence(e.id);
		const base = join(root, ".astra/jobs", job.state.frame.jobId);
		const sources = [
			join(base, "workspaces", e.taskId, "result"),
			join(base, "tasks", e.taskId, "packet"),
			join(base, "resources", e.taskId, "weights"),
			join(root, "session.jsonl"),
		];
		for (const path of sources) {
			await mkdir(dirname(path), { recursive: true });
			await writeFile(path, path);
		}
		await job.recordChildSession({
			sessionId: "registered",
			role: "worker",
			taskId: e.taskId,
			status: "completed",
			attempt: 1,
			sessionFile: sources[3],
			updatedAt: "now",
		});
		if (mode === "intent-snapshot" || mode === "completion-snapshot") {
			failSnapshotAfter(
				store,
				(event) => event.type === (mode === "intent-snapshot" ? "stage_reopened" : "cleanup_completed"),
			);
		} else if (mode === "completion-append") {
			const append = store.append.bind(store);
			let hit = false;
			vi.spyOn(store, "append").mockImplementation(async (id, event) => {
				if (!hit && event.type === "cleanup_completed") {
					hit = true;
					throw new Error("injected completion append failure");
				}
				return append(id, event);
			});
		} else {
			Object.assign(fault, {
				operation: mode === "copy" ? "cp" : mode === "rename" ? "rename" : "rm",
				path: mode === "copy" ? "workspaces" : mode === "rename" ? "resources" : `workspaces/${e.taskId}`,
				after: mode !== "copy",
				fired: false,
			});
		}
		await expect(job.reopenStage("validation", "saved-reopen", "original reason")).rejects.toThrow(/injected/);
		expect(job.state.canonical[a.id]).toBeUndefined();
		expect(job.state.frame.nextAction).toContain("original reason");
		fault.operation = "";
		vi.restoreAllMocks();
		const current = (await ResearchJob.open(store, job.state.frame.jobId))!;
		await current.recoverPendingOperations();
		const receipt = current.state.retiredArtifacts[a.id];
		expect(receipt.cleanupStatus).toBe("completed");
		const archive = receipt.archiveRefs![0];
		expect(await readFile(join(archive, "workspace/result"), "utf8")).toBe(sources[0]);
		expect(await readFile(join(archive, "task/packet"), "utf8")).toBe(sources[1]);
		expect(await readFile(join(archive, "resources/weights"), "utf8")).toBe(sources[2]);
		expect(await readFile(current.state.sessions.registered.sessionFile!, "utf8")).toBe(sources[3]);
		const done = current.state;
		await current.reopenStage("validation", "saved-reopen", "original reason");
		expect(current.state).toEqual(done);
	},
);

it("T11 preserves both conflicting resource trees and keeps a retryable intent", async () => {
	const { root, job } = await setup();
	const e = await evidence(job, "conflict");
	await job.adoptEvidence(e.id);
	const base = join(root, ".astra/jobs", job.state.frame.jobId);
	const source = join(base, "resources", e.taskId, "weights");
	const target = join(base, "archive/tasks", e.taskId, "resources/weights");
	for (const path of [source, target]) {
		await mkdir(dirname(path), { recursive: true });
		await writeFile(path, path);
	}
	await expect(job.reopenStage("validation", "conflict-reopen", "reason")).rejects.toThrow(/conflicts/);
	expect(await readFile(source, "utf8")).toBe(source);
	expect(await readFile(target, "utf8")).toBe(target);
	expect(Object.values(job.state.cleanupIntents!)[0].status).toBe("pending");
});

it("T13 records new scientific outcome after retiring old results and invalidates downstream", async () => {
	const { job, store } = await setup();
	const old = await job.adoptEvidence((await evidence(job, "old claim", true, true)).id);
	const task = await job.dispatchTask({
		...taskInput(job, "downstream"),
		stageId: "paper-write",
		requiredOutputType: "paper-write",
		inputArtifactRefs: [old.id],
	});
	await job.setTaskStatus(task.id, "succeeded");
	const e = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: "paper" },
		refs: [],
	});
	await job.recordReview(reviewFixture(job, { evidenceId: e.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(e.id, true);
	const downstream = await job.adoptEvidence(e.id);
	const replacement = await job.adoptEvidence((await evidence(job, "new claim", true, true)).id, old.id);
	expect(job.state.frame.scientificOutcome).toBe("supported");
	expect(job.state.graph.acceptedClaimIds.map((id) => job.state.graph.nodes[id].statement)).toEqual(["new claim"]);
	expect(job.state.canonical[downstream.id].status).toBe("stale");
	expect(job.state.canonicalRoute.stageArtifactIds["result-to-claim"]).toBe(replacement.id);
	const events = await store.readEvents(job.state.frame.jobId);
	expect(events.map((entry) => entry.seq)).toEqual(events.map((_, i) => i + 1));
	vi.spyOn(store, "loadSnapshot").mockResolvedValue(undefined);
	const replay = (await ResearchJob.open(store, job.state.frame.jobId))!;
	// T14 full journal replay and normal snapshot projection have the same durable facts.
	expect(JSON.parse(JSON.stringify(replay.state))).toEqual(JSON.parse(JSON.stringify(job.state)));
});
async function setup(disk = false, budget: Partial<MissionFrame["budget"]> = {}) {
	const root = await mkdtemp(join(tmpdir(), "astra-followup-recovery-"));
	roots.push(root);
	const store = disk ? new JsonlAstraStore(root) : new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "Offline recovery",
		automation: "full",
		...budget,
	});
	await job.reload();
	return { root, store, job };
}
function taskInput(job: ResearchJob, key: string) {
	return {
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker" as const,
		deliveryKind: "stage" as const,
		objective: key,
		replayKey: key,
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		successCriteria: [],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none" as const,
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session" as const,
	};
}
async function evidence(job: ResearchJob, key: string, accepted = true, result = false) {
	const task = await job.dispatchTask({
		...taskInput(job, key),
		...(result ? { stageId: "result-to-claim", requiredOutputType: "result-to-claim" } : {}),
	});
	await job.setTaskStatus(task.id, "succeeded");
	const item = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: result
			? {
					scientificOutcome: "supported",
					missionCoverage: "sufficient",
					claims: [{ statement: key, assessment: "supported" }],
					conclusion: key,
				}
			: { content: key },
		refs: [],
	});
	if (accepted) {
		await job.recordReview(reviewFixture(job, { evidenceId: item.id, verdict: "pass", findings: [] }));
		if (result) await job.recordReview(reviewFixture(job, { evidenceId: item.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(item.id, true);
	}
	return item;
}
function failSnapshotAfter(store: AstraStore, match: (event: AstraEvent) => boolean) {
	const append = store.append.bind(store);
	let failed = false;
	vi.spyOn(store, "append").mockImplementation(async (id, event) => {
		const saved = await append(id, event);
		if (!failed && match(event)) {
			failed = true;
			vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("injected snapshot failure"));
		}
		return saved;
	});
}
it.each([false, true])("T1 serializes turn reservations and refunds (disk=%s)", async (disk) => {
	const { job, store } = await setup(disk, { maxTurns: 1 });
	const reserved = await Promise.allSettled([job.consumeTurns(1), job.consumeTurns(1)]);
	expect(reserved.filter((r) => r.status === "fulfilled")).toHaveLength(1);
	expect((await ResearchJob.open(store, job.state.frame.jobId))!.state.budgetUsage!.turnsUsed).toBe(1);
	const refunded = await Promise.allSettled([job.refundTurns(1), job.refundTurns(1)]);
	expect(refunded.filter((r) => r.status === "fulfilled")).toHaveLength(1);
	await job.consumeTurns(1);
	await expect(job.consumeTurns(1)).rejects.toThrow(/turn budget/);
});
it("T2 serializes duplicate dispatch and failed retries", async () => {
	const { job, store } = await setup();
	const pair = await Promise.all([job.dispatchTask(taskInput(job, "same")), job.dispatchTask(taskInput(job, "same"))]);
	expect(pair[0].id).toBe(pair[1].id);
	expect(
		(await store.readEvents(job.state.frame.jobId)).filter((e) => e.event.type === "task_dispatched"),
	).toHaveLength(1);
	await job.setTaskStatus(pair[0].id, "failed");
	const retry = await job.dispatchTask({ ...taskInput(job, "same"), attempt: 2, supersedesTaskId: pair[0].id });
	expect(retry.id).not.toBe(pair[0].id);
	const limited = (await setup(false, { maxTasks: 1 })).job;
	expect(
		(
			await Promise.allSettled([
				limited.dispatchTask(taskInput(limited, "a")),
				limited.dispatchTask(taskInput(limited, "b")),
			])
		).filter((r) => r.status === "fulfilled"),
	).toHaveLength(1);
});
it("T3 merges partial budgets and serializes leases and terminal task status", async () => {
	const { job } = await setup();
	await Promise.all([job.updateBudget({ maxTurns: 3 }), job.updateBudget({ maxTasks: 4 })]);
	expect(job.state.frame.budget).toMatchObject({ maxTurns: 3, maxTasks: 4 });
	expect(
		(await Promise.allSettled([job.acquireLease("a"), job.acquireLease("b")])).filter(
			(r) => r.status === "fulfilled",
		),
	).toHaveLength(1);
	const task = await job.dispatchTask(taskInput(job, "terminal"));
	const updates = await Promise.allSettled([
		job.setTaskStatus(task.id, "succeeded"),
		job.setTaskStatus(task.id, "running"),
	]);
	expect(updates[1].status).toBe("rejected");
	expect(job.state.tasks[task.id].status).toBe("succeeded");
});
for (const stage of [
	"adoption_requested",
	"materialization",
	"materialized",
	"baseline_visible",
	"integration_verified",
	"active",
]) {
	it.each([false, true])(`T4 resumes ${stage} snapshot failure (reopen=%s)`, async (reopen) => {
		const { job, store } = await setup(true);
		const e = await evidence(job, "claim", true, true);
		let hit = false;
		failSnapshotAfter(store, (event) => {
			const match =
				stage === "adoption_requested"
					? event.type === "evidence_adopted"
					: stage === "materialization"
						? event.type === "canonical_artifact_materialized"
						: event.type === "canonical_artifact_status" && event.status === stage;
			if (!hit && match) {
				hit = true;
				return true;
			}
			return false;
		});
		await expect(job.adoptEvidence(e.id)).rejects.toThrow(/injected snapshot/);
		const candidate = Object.values(job.state.canonical)[0];
		const current = reopen ? (await ResearchJob.open(store, job.state.frame.jobId))! : job;
		const adopted = await current.adoptEvidence(e.id);
		expect(adopted.id).toBe(candidate.id);
		expect(adopted.status).toBe("active");
		expect(current.state.graph.acceptedClaimIds).toHaveLength(1);
		const done = current.state;
		await current.adoptEvidence(e.id);
		expect(current.state).toEqual(done);
		expect(JSON.parse(JSON.stringify((await ResearchJob.open(store, done.frame.jobId))!.state))).toEqual(
			JSON.parse(JSON.stringify(done)),
		);
	});
}
it.each(["fail", "partial", "blocked"] as const)(
	"T7 saves %s review and all required consequences together",
	async (verdict) => {
		const { job, store } = await setup(true);
		const e = await evidence(job, "failed", false);
		failSnapshotAfter(store, (event) => event.type === "review_recorded");
		await expect(
			job.recordReview(reviewFixture(job, { evidenceId: e.id, verdict, findings: ["missing control"] })),
		).rejects.toThrow(/snapshot/);
		const recovered = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(recovered.state.frame.openObligationIds).toHaveLength(1);
		expect(recovered.state.graph.unresolvedObjectionIds).toHaveLength(1);
		expect(Object.values(recovered.state.graph.edges).some((edge) => edge.kind === "contradicts")).toBe(true);
	},
);
it("T10 preserves all source and archive bytes when reopen intent append fails", async () => {
	const { root, job, store } = await setup(true);
	const e = await evidence(job, "retire");
	const a = await job.adoptEvidence(e.id);
	const base = join(root, ".astra/jobs", job.state.frame.jobId);
	const paths = [
		a.materializationRef!,
		join(base, "workspaces", e.taskId, "result"),
		join(base, "tasks", e.taskId, "packet"),
		join(base, "resources", e.taskId, "weights"),
		join(base, "archive/tasks", e.taskId, "resources", "old"),
		join(root, "external-session"),
	];
	for (const path of paths) {
		await mkdir(dirname(path), { recursive: true });
		await writeFile(path, "retained bytes");
	}
	await job.recordChildSession({
		sessionId: "session",
		role: "worker",
		taskId: e.taskId,
		status: "completed",
		attempt: 1,
		sessionFile: paths.at(-1),
		updatedAt: "now",
	});
	vi.spyOn(store, "append").mockRejectedValueOnce(new Error("intent append failed"));
	await expect(job.reopenStage("validation", "reopen", "original reason")).rejects.toThrow(/intent append/);
	for (const path of paths) expect(await readFile(path, "utf8")).toBe("retained bytes");
	expect((await ResearchJob.open(store, job.state.frame.jobId))!.state.canonical[a.id].status).toBe("active");
});
