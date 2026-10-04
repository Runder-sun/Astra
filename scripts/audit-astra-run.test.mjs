import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, readFile, readdir, readlink, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { mainDecisionManifestPath, workerManifestPath, writeReviewerOutputManifest } from "../packages/astra/src/contracts.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../packages/astra/src/pi-child-session.ts";
import { ResearchJob } from "../packages/astra/src/research.ts";
import { JsonlAstraStore } from "../packages/astra/src/store.ts";
import { reviewFixture } from "../packages/astra/test/review-fixture.ts";
import { SessionManager } from "../packages/coding-agent/src/core/session-manager.ts";

const auditScript = fileURLToPath(new URL("./audit-astra-run.mjs", import.meta.url));

async function registeredPiFixture(custom = false) {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-registered-pi-"));
	const store = new JsonlAstraStore(workspace);
	const job = await ResearchJob.create(store, { jobId: "job_registered_pi", workspaceRoot: workspace, objective: "Audit retained Pi history" });
	const task = await job.dispatchTask({ stageId: "validation", stageExecutionId: "validation", role: "worker", objective: "Produce offline validation", inputArtifactRefs: [], requiredCanonicalArtifacts: [], requiredOutputType: "validation", requiredOutputFields: ["content"], acceptanceChecks: ["verified"], failureSignals: [], successCriteria: [], dependencies: [], scope: { workspaceRoot: workspace, allowedPaths: ["."] }, allowedTools: ["read"], writeAuthority: "none", budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 }, reviewGateRequired: true, resumePolicy: "resume-session" });
	await job.setTaskStatus(task.id, "running");
	await job.setTaskStatus(task.id, "succeeded");
	const jobRoot = join(workspace, ".astra/jobs", task.jobId);
	const executionRoot = join(jobRoot, "workspaces", task.id);
	const sessionId = `astra-${task.jobId}-${task.id}-${task.attempt}`;
	await mkdir(executionRoot, { recursive: true });
	const manager = SessionManager.create(executionRoot, join(workspace, custom ? "custom-sessions" : `.astra/jobs/${task.jobId}/sessions`), { id: sessionId });
	manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "offline success" }], api: "openai-responses", provider: "fixture", model: "fixture", usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "toolUse", timestamp: Date.now() });
	const sessionFile = manager.getSessionFile();
	const manifestRef = workerManifestPath(workspace, task.jobId, task.id);
	await mkdir(join(jobRoot, "tasks", task.id), { recursive: true });
	await writeFile(manifestRef, JSON.stringify({ schemaVersion: "astra.worker_output_manifest.v1", manifestId: "fixture", jobId: task.jobId, taskId: task.id, agentId: task.agentId, status: "completed", artifactType: task.requiredOutputType, content: { content: "verified" }, outputRefs: [{ kind: "log", ref: `pi-session:${sessionId}` }], validationStatus: "passed", validationErrors: [], sessionRef: sessionId, createdAt: new Date().toISOString() }));
	await job.recordChildSession({ sessionId, taskId: task.id, role: "worker", attempt: task.attempt, status: "completed", sessionFile, manifestRef, updatedAt: new Date().toISOString() });
	const audit = async () => {
		const capture = async (root) => {
			const files = {};
			for (const entry of await readdir(root, { withFileTypes: true })) {
				const path = join(root, entry.name);
				if (entry.isDirectory()) Object.assign(files, await capture(path));
				else files[path] = entry.isSymbolicLink() ? await readlink(path) : (await readFile(path)).toString("base64");
			}
			return files;
		};
		const inputs = await capture(workspace);
		const snapshot = await readFile(join(jobRoot, "job.json"));
		const events = await readFile(join(jobRoot, "events.jsonl"));
		const result = spawnSync(process.execPath, [auditScript, workspace, task.jobId], { cwd: workspace, encoding: "utf8" });
		assert.equal(result.status, 0, result.stderr);
		assert.deepEqual(await readFile(join(jobRoot, "job.json")), snapshot);
		assert.deepEqual(await readFile(join(jobRoot, "events.jsonl")), events);
		assert.deepEqual(await capture(workspace), inputs);
		return JSON.parse(result.stdout);
	};
	return { workspace, store, job, task, jobRoot, sessionId, sessionFile, manifestRef, audit };
}

test("L2 ordinary real Pi header and manifest survive durable reopen and custom session directories", async () => {
	const f = await registeredPiFixture(true);
	try {
		const reopened = await ResearchJob.open(f.store, f.task.jobId);
		assert.equal(reopened.state.sessions[f.sessionId].taskId, f.task.id);
		const report = await f.audit();
		assert.equal(report.contractChecks.acceptedChildSessionsValid, true);
		assert.equal(report.sessions.files, 1);
		assert.equal(report.passed, false, "valid sessions do not complete research");
	} finally { await rm(f.workspace, { recursive: true, force: true }); }
});

async function registeredPiReviewerFixture() {
	const f = await registeredPiFixture();
	const evidence = await f.job.recordEvidence({ taskId: f.task.id, stageId: f.task.stageId, type: f.task.requiredOutputType, content: { content: "verified" }, refs: [] });
	let manifestPath;
	class OfflineRunner extends PiChildSessionRunner {
		async run(cwd, jobId, taskId, attempt, role, _prompt, env) {
			assert.equal(role, "reviewer");
			const sessionId = `astra-${jobId}-${taskId}-${attempt}`.replace(/[^A-Za-z0-9._-]/g, "-");
			const manager = SessionManager.create(cwd, join(f.jobRoot, "sessions"), { id: sessionId });
			manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "offline reviewed" }], api: "openai-responses", provider: "fixture", model: "fixture", usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "toolUse", timestamp: Date.now() });
			manifestPath = await writeReviewerOutputManifest({ schemaVersion: "astra.reviewer_output_manifest.v1", manifestId: `review_${taskId}`, jobId, taskId, evidenceId: env.ASTRA_EVIDENCE_ID, verdict: "pass", score: 1, findings: [], verifiedRefs: [`evidence:${evidence.id}`], criteria: JSON.parse(env.ASTRA_REVIEW_CRITERIA).map((criterion) => ({ criterion, passed: true, score: 1, evidenceRefs: [`evidence:${evidence.id}`], rationale: "offline checked" })), sessionRef: sessionId, createdAt: new Date().toISOString() }, f.workspace);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
		}
	}
	const reviewed = await new PiReviewerAdapter(new OfflineRunner()).review(evidence, f.job);
	await f.job.recordReview({ ...reviewed, evidenceId: evidence.id });
	await f.job.setTaskStatus(reviewed.reviewerTaskId, "succeeded");
	const reviewerTask = f.job.state.tasks[reviewed.reviewerTaskId];
	assert.deepEqual(reviewerTask.inputArtifactRefs, [evidence.id]);
	return { ...f, evidence, reviewerTask, reviewerManifest: manifestPath };
}

for (const change of ["wrong", "missing", "other-existing", "input-missing", "input-empty", "input-multiple", "input-other"]) {
	test(`L2 public Pi reviewer rejects target ${change} with read-only audit`, async () => {
		const f = await registeredPiReviewerFixture();
		try {
			assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
			const other = await f.job.recordEvidence({ taskId: f.task.id, stageId: f.task.stageId, type: f.task.requiredOutputType, content: { content: "other existing target" }, refs: [] });
			const manifest = JSON.parse(await readFile(f.reviewerManifest, "utf8"));
			if (change === "wrong") manifest.evidenceId = "evidence_not_registered";
			if (change === "missing") delete manifest.evidenceId;
			if (change === "other-existing") manifest.evidenceId = other.id;
			if (change.startsWith("input-")) {
				const snapshot = f.job.state;
				const task = snapshot.tasks[f.reviewerTask.id];
				if (change === "input-missing") delete task.inputArtifactRefs;
				if (change === "input-empty") task.inputArtifactRefs = [];
				if (change === "input-multiple") task.inputArtifactRefs = [f.evidence.id, other.id];
				if (change === "input-other") task.inputArtifactRefs = [other.id];
				await f.store.writeSnapshot(snapshot);
			} else await writeFile(f.reviewerManifest, JSON.stringify(manifest));
			const report = await f.audit();
			assert.equal(report.contractChecks.acceptedChildSessionsValid, false);
			assert.ok(report.sessions.audits.find((entry) => entry.taskId === f.reviewerTask.id).failures.some((reason) => reason.includes("identity")));
		} finally { await rm(f.workspace, { recursive: true, force: true }); }
	});
}

test("L2 public Pi reviewer target binding survives real retirement and rejects changed archive target", async () => {
	const f = await registeredPiReviewerFixture();
	try {
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
		await f.job.decideEvidence(f.evidence.id, true);
		await f.job.adoptEvidence(f.evidence.id);
		const original = await readFile(f.reviewerManifest);
		await f.job.reopenStage("validation", "public-pi-reviewer-retirement", "audit archived target identity");
		const session = Object.values(f.job.state.sessions).find((entry) => entry.taskId === f.reviewerTask.id);
		assert.ok(session.sessionFile.startsWith(join(f.jobRoot, "archive", "tasks", f.reviewerTask.id)));
		const archivedManifest = join(f.jobRoot, "archive", "tasks", f.reviewerTask.id, "task", "review-manifest.json");
		assert.deepEqual(await readFile(archivedManifest), original);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
		await writeFile(archivedManifest, JSON.stringify({ ...JSON.parse(original), evidenceId: "evidence_wrong_archive_target" }));
		const report = await f.audit();
		assert.equal(report.contractChecks.acceptedChildSessionsValid, false);
		assert.ok(report.sessions.audits.find((entry) => entry.taskId === f.reviewerTask.id).failures.some((reason) => reason.includes("identity")));
		await writeFile(archivedManifest, original);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
	} finally { await rm(f.workspace, { recursive: true, force: true }); }
});

for (const change of ["missing", "undefined", "directory", "leaf-link", "parent-link", "empty", "malformed", "truncated", "null", "array", "scalar", "wrong-header", "wrong-job", "wrong-task", "wrong-role", "wrong-attempt", "wrong-manifest"]) {
	test(`L2 registered Pi session rejects ${change} without modifying inputs`, async () => {
		const f = await registeredPiFixture();
		try {
			const bytes = await readFile(f.sessionFile);
			if (change === "missing") await rm(f.sessionFile);
			else if (change === "directory") { await rm(f.sessionFile); await mkdir(f.sessionFile); }
			else if (change === "leaf-link") { const outside = join(f.workspace, "outside.jsonl"); await writeFile(outside, bytes); await rm(f.sessionFile); await symlink(outside, f.sessionFile); }
			else if (change === "parent-link") { const dir = join(f.jobRoot, "sessions"); const outside = join(f.workspace, "outside-sessions"); await rename(dir, outside); await symlink(outside, dir); }
			else if (["empty", "malformed", "truncated", "null", "array", "scalar", "wrong-header"].includes(change)) await writeFile(f.sessionFile, change === "empty" ? "" : change === "malformed" ? "{bad}\n" : change === "truncated" ? `${bytes}{\"type\":` : change === "null" ? "null\n" : change === "array" ? "[]\n" : change === "scalar" ? "3\n" : bytes.toString().replace(f.sessionId, "wrong-session"));
			else if (change === "wrong-manifest") { const manifest = JSON.parse(await readFile(f.manifestRef, "utf8")); await writeFile(f.manifestRef, JSON.stringify({ ...manifest, taskId: "wrong" })); }
			else {
				const snapshot = f.job.state;
				const session = snapshot.sessions[f.sessionId];
				if (change === "undefined") delete session.sessionFile;
				if (change === "wrong-job") snapshot.tasks[f.task.id].jobId = "job_other";
				if (change === "wrong-task") session.taskId = "task_unknown";
				if (change === "wrong-role") session.role = "reviewer";
				if (change === "wrong-attempt") session.attempt++;
				await f.store.writeSnapshot(snapshot);
			}
			const report = await f.audit();
			assert.equal(report.contractChecks.acceptedChildSessionsValid, false);
			assert.ok(report.sessions.invalidFiles.length > 0 || report.sessions.audits.some((entry) => entry.failures.length > 0));
		} finally { await rm(f.workspace, { recursive: true, force: true }); }
	});
}

test("L2 public Pi retirement binds actual archive bytes and exact completed cleanup proof", async () => {
	const f = await registeredPiFixture();
	try {
		const evidence = await f.job.recordEvidence({ taskId: f.task.id, stageId: f.task.stageId, type: f.task.requiredOutputType, content: { content: "verified" }, refs: [] });
		await f.job.recordReview(reviewFixture(f.job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
		await f.job.decideEvidence(evidence.id, true);
		await f.job.adoptEvidence(evidence.id);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
		await f.job.reopenStage("validation", "public-pi-retirement", "retain audited history");
		const archived = f.job.state.sessions[f.sessionId].sessionFile;
		assert.notEqual(archived, f.sessionFile);
		const bytes = await readFile(archived);
		const retiredAudit = await f.audit();
		assert.equal(retiredAudit.contractChecks.acceptedChildSessionsValid, true, JSON.stringify(retiredAudit.sessions));
		await writeFile(archived, `${bytes}\n`);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false);
		await rm(archived);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false);
		await writeFile(archived, bytes);
		const valid = f.job.state;
		for (const mode of ["pending", "wrong-source", "wrong-target", "wrong-hash", "wrong-task", "not-present"]) {
			const snapshot = structuredClone(valid);
			const intent = Object.values(snapshot.cleanupIntents)[0];
			const moved = intent.tasks[0].sessions[0];
			if (mode === "pending") intent.status = "pending";
			if (mode === "wrong-source") moved.source += ".wrong";
			if (mode === "wrong-target") moved.target += ".wrong";
			if (mode === "wrong-hash") moved.expectedHash = sha256("wrong");
			if (mode === "wrong-task") intent.tasks[0].taskId = "wrong";
			if (mode === "not-present") moved.present = false;
			await f.store.writeSnapshot(snapshot);
			assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false, mode);
		}
		await f.store.writeSnapshot(valid);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
	} finally { await rm(f.workspace, { recursive: true, force: true }); }
});

test("L2 persistent main Pi history validates every registered call after snapshot overwrite", async () => {
	const f = await registeredPiFixture();
	try {
		const manager = SessionManager.create(f.workspace, join(f.jobRoot, "sessions"), { id: f.job.state.mainAgentSessionId });
		manager.appendCustomEntry("offline-call", {});
		const paths = [];
		for (const id of ["decision_first", "decision_second"]) {
			const path = mainDecisionManifestPath(f.workspace, f.task.jobId, "route", id);
			await f.job.registerMainAgentCall({ id, type: "route", manifestId: id, manifestRef: path });
			await mkdir(dirname(path), { recursive: true });
			await writeFile(path, JSON.stringify({ schemaVersion: "astra.main_agent_decision_manifest.v1", manifestId: id, jobId: f.task.jobId, decisionRef: id, decisionType: "route", stageId: "validation", routeAction: "continue", rationale: "offline fixture", sessionRef: manager.getSessionId(), createdAt: new Date().toISOString() }));
			await f.job.readMainAgentCallDelivery(id);
			await f.job.recordChildSession({ sessionId: manager.getSessionId(), taskId: id, role: "main-agent", status: "completed", attempt: 1, sessionFile: manager.getSessionFile(), manifestRef: path, updatedAt: new Date().toISOString() });
			paths.push(path);
		}
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
		const failedCall = "decision_failed";
		await f.job.registerMainAgentCall({ id: failedCall, type: "route", manifestId: failedCall, manifestRef: mainDecisionManifestPath(f.workspace, f.task.jobId, "route", failedCall) });
		manager.appendMessage({ role: "assistant", content: [], api: "openai-responses", provider: "fixture", model: "fixture", usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "error", errorMessage: "offline later call failed", timestamp: Date.now() });
		await f.job.recordChildSession({ sessionId: manager.getSessionId(), taskId: failedCall, role: "main-agent", status: "failed", attempt: 1, sessionFile: manager.getSessionFile(), updatedAt: new Date().toISOString() });
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true, "a later failed main call must not invalidate older successful deliveries");
		await rm(paths[0]);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false, "older call must not disappear behind latest snapshot");
	} finally { await rm(f.workspace, { recursive: true, force: true }); }
});

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

test("L2 failed Pi fallback and unavailable unowned history preserve recovery policy", async () => {
	const f = await registeredPiFixture();
	try {
		const snapshot = f.job.state;
		snapshot.tasks[f.task.id].status = "failed";
		const session = snapshot.sessions[f.sessionId];
		session.status = "failed";
		session.sessionFile = join(f.jobRoot, "tasks", f.task.id, "failure-log.json");
		await writeFile(session.sessionFile, JSON.stringify({ taskId: f.task.id, attempt: f.task.attempt, error: "offline fixture failure" }));
		await f.store.writeSnapshot(snapshot);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
		await writeFile(session.sessionFile, JSON.stringify({ taskId: "wrong", attempt: f.task.attempt, error: "failure" }));
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false);
		await rm(session.sessionFile);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, true);
		snapshot.tasks[f.task.id].status = "running";
		await f.store.writeSnapshot(snapshot);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false, "retained running owner cannot exempt a failed session");
		session.status = "completed";
		snapshot.tasks[f.task.id].status = "succeeded";
		await f.store.writeSnapshot(snapshot);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false);
	} finally { await rm(f.workspace, { recursive: true, force: true }); }
});

test("L2 completed Pi registration cannot inherit a later failed task's missing-file exemption", async () => {
	const f = await registeredPiFixture();
	try {
		const snapshot = f.job.state;
		snapshot.tasks[f.task.id].status = "failed";
		await f.store.writeSnapshot(snapshot);
		await rm(f.sessionFile);
		assert.equal((await f.audit()).contractChecks.acceptedChildSessionsValid, false);
	} finally { await rm(f.workspace, { recursive: true, force: true }); }
});

for (const line of ["null", "[]", "3"]) {
	test(`L2 unregistered Pi non-object ${line} returns a read-only negative report`, async () => {
		const f = await registeredPiFixture();
		try {
			const path = join(f.jobRoot, "sessions", "unregistered.jsonl");
			await writeFile(path, `${line}\n`);
			const report = await f.audit();
			assert.equal(report.contractChecks.inputFilesValid, false);
			assert.equal(await readFile(path, "utf8"), `${line}\n`);
		} finally { await rm(f.workspace, { recursive: true, force: true }); }
	});
}
function stableJson(value) {
	if (value === null || typeof value !== "object") return JSON.stringify(value);
	if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
	return `{${Object.entries(value).sort(([left], [right]) => left.localeCompare(right))
		.map(([key, entry]) => `${JSON.stringify(key)}:${stableJson(entry)}`).join(",")}}`;
}
const checksum = (value) => sha256(stableJson(value));
function sourceReceipt(sourceRef, fields = {}) {
	const receipt = { sourceRef, provider: "openalex", query: "fixture", retrievedAt: "2026-10-03T00:00:00.000Z",
		record: { sourceRef, title: "Fixture source", authors: [], ...fields } };
	return { ...receipt, sha256: sha256(JSON.stringify(receipt)) };
}

test("counts Pi assistant terminal errors as failed child sessions", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-"));
	const jobId = "job_fixture";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	try {
		await mkdir(join(jobRoot, "sessions"), { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { activeStageId: "research-lit" },
				stages: { "research-lit": { status: "completed" } },
				eventSeq: 1,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {},
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		await writeFile(
			join(jobRoot, "sessions", "child.jsonl"),
			`${JSON.stringify({
				type: "message",
				message: { role: "assistant", stopReason: "error", errorMessage: "fixture failure" },
			})}\n`,
		);

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		assert.equal(JSON.parse(result.stdout).sessions.failedFiles, 1);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("recognizes resumed completion checkpoints and optional compaction in parent Pi sessions", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-parent-"));
	const jobId = "job_resumed";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const parentSessions = join(workspace, "parent-sessions");
	try {
		await mkdir(join(jobRoot, "sessions"), { recursive: true });
		await mkdir(parentSessions, { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { activeStageId: "research-lit" },
				stages: { "research-lit": { status: "completed" } },
				eventSeq: 0,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {},
			}),
		);
		await writeFile(
			join(jobRoot, "events.jsonl"),
			`${JSON.stringify({ seq: 1, event: { type: "job_resumed" } })}\n`,
		);
		await writeFile(
			join(parentSessions, "resume.jsonl"),
			[
				{ type: "custom", customType: "astra_research_result", data: { action: "resume", jobId } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n"),
		);

		const missingCheckpoint = spawnSync(process.execPath, [auditScript, workspace, jobId, parentSessions], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(missingCheckpoint.status, 0, missingCheckpoint.stderr);
		assert.equal(JSON.parse(missingCheckpoint.stdout).contractChecks.recoveryCheckpointed, false);

		await writeFile(
			join(parentSessions, "resume.jsonl"),
			[
				{ type: "custom", customType: "astra_checkpoint", data: { jobId } },
				{ type: "custom", customType: "astra_research_result", data: { action: "resume", jobId } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n"),
		);
		const checkpointed = spawnSync(process.execPath, [auditScript, workspace, jobId, parentSessions], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(checkpointed.status, 0, checkpointed.stderr);
		const checkpointReport = JSON.parse(checkpointed.stdout);
		assert.equal(checkpointReport.contractChecks.recoveryCheckpointed, true);
		assert.equal(checkpointReport.parentSessions.checkpointEntries, 1);
		assert.equal(checkpointReport.parentSessions.compactionEntries, 0);

		await writeFile(
			join(parentSessions, "resume.jsonl"),
			[
				{ type: "custom", customType: "astra_checkpoint", data: { jobId } },
				{ type: "compaction", summary: "durable checkpoint" },
				{ type: "custom", customType: "astra_research_result", data: { action: "resume", jobId } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n"),
		);
		const result = spawnSync(process.execPath, [auditScript, workspace, jobId, parentSessions], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.equal(report.contractChecks.recoveryCheckpointed, true);
		assert.equal(report.policy.automation, null);
		assert.deepEqual(report.canonical.duplicateEvidenceIds, []);
		assert.equal(report.parentSessions.matchingFiles, 1);
		assert.deepEqual(report.parentSessions.actions, ["resume"]);
		assert.equal(report.parentSessions.compactionFiles, 1);
		assert.equal(report.parentSessions.compactionEntries, 1);
		assert.equal(report.parentSessions.checkpointEntries, 1);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("audits stage plans, task context, upstream artifacts, and source receipts", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-contracts-"));
	const jobId = "job_contracts";
	const taskId = "task_literature";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const sourceRefs = ["openalex:W1", "openalex:W2", "openalex:W3"];
	try {
		await mkdir(join(jobRoot, "tasks", taskId), { recursive: true });
		await mkdir(join(jobRoot, "workspaces", taskId), { recursive: true });
		await mkdir(join(jobRoot, "sources"), { recursive: true });
		const plan = {
			id: "plan_literature",
			stageId: "literature",
			tasks: [{ key: "primary", objective: "Survey the selected literature" }],
		};
		const task = {
			id: taskId,
			role: "worker",
			status: "succeeded",
			stageId: "literature",
			objective: "Survey the selected literature",
			replayKey: "stage-plan:plan_literature:primary",
			inputArtifactRefs: ["artifact_validation"],
		};
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "literature" },
				stages: { literature: { status: "completed" } },
				stagePlans: { [plan.id]: plan },
				eventSeq: 0,
				lease: null,
				tasks: { [task.id]: task },
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: { artifact_validation: { id: "artifact_validation", status: "retired" } },
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		await writeFile(
			join(jobRoot, "workspaces", taskId, "ASTRA_TASK_CONTEXT.json"),
			JSON.stringify({
				schemaVersion: "astra.task_context.v1",
				mission: { jobId },
				stage: { minSourceRefs: 3 },
				task: { id: taskId },
				inputs: { canonicalArtifacts: [{ id: "artifact_validation" }] },
			}),
		);
		await writeFile(
			join(jobRoot, "tasks", taskId, "output-manifest.json"),
			JSON.stringify({ outputRefs: sourceRefs.map((ref) => ({ kind: "source", ref })) }),
		);
		for (const [index, sourceRef] of sourceRefs.entries()) {
			await writeFile(join(jobRoot, "sources", `openalex-W${index + 1}.json`), JSON.stringify(sourceReceipt(sourceRef)));
		}

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.deepEqual(report.planning.missingStageIds, []);
		assert.deepEqual(report.planning.unplannedWorkerTaskIds, []);
		assert.deepEqual(report.planning.objectiveMismatchTaskIds, []);
		assert.deepEqual(report.planning.forbiddenGenericTaskIds, []);
		assert.deepEqual(report.workspaces.missingTaskIds, []);
		assert.deepEqual(report.workspaces.missingUpstreamTaskIds, []);
		assert.deepEqual(report.sources.insufficientTaskIds, []);
		assert.deepEqual(report.sources.missingReceiptRefs, []);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("audits the accepted chain without treating recovered attempts as active failures", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-recovered-"));
	const jobId = "job_recovered";
	const succeededTaskId = "task_succeeded";
	const failedTaskId = "task_failed";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const failedSessionPath = join(jobRoot, "sessions", "failed.jsonl");
	const mainSessionPath = join(jobRoot, "sessions", "main.jsonl");
	try {
		await mkdir(join(jobRoot, "tasks", succeededTaskId), { recursive: true });
		await mkdir(join(jobRoot, "workspaces", succeededTaskId), { recursive: true });
		await mkdir(join(jobRoot, "workspaces", failedTaskId), { recursive: true });
		await mkdir(join(jobRoot, "sources"), { recursive: true });
		await mkdir(join(jobRoot, "sessions"), { recursive: true });
		const plan = {
			id: "plan_review",
			stageId: "research-review",
			tasks: [
				{ key: "accepted", objective: "Audit the accepted chain" },
				{ key: "recovered", objective: "Historical failed attempt" },
			],
		};
		const succeededTask = {
			id: succeededTaskId,
			jobId,
			agentId: "worker_success",
			attempt: 1,
			requiredOutputType: "research-review",
			role: "worker",
			status: "succeeded",
			stageId: "research-review",
			objective: plan.tasks[0].objective,
			replayKey: `stage-plan:${plan.id}:accepted`,
			inputArtifactRefs: ["artifact_expanded", "artifact_indexed"],
		};
		const failedTask = {
			id: failedTaskId,
			jobId,
			attempt: 1,
			role: "worker",
			status: "failed",
			stageId: "research-review",
			objective: plan.tasks[1].objective,
			replayKey: `stage-plan:${plan.id}:recovered`,
			inputArtifactRefs: ["artifact_expanded"],
		};
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "research-review" },
				stages: { "research-review": { status: "completed" } },
				stagePlans: { [plan.id]: plan },
				eventSeq: 0,
				lease: null,
				tasks: { [succeededTaskId]: succeededTask, [failedTaskId]: failedTask },
				sessions: {
					failed: {
						sessionId: "failed",
						role: "worker",
						taskId: failedTaskId,
						status: "starting",
						attempt: 1,
						sessionFile: failedSessionPath,
					},
					main: {
						sessionId: "main",
						role: "worker",
						taskId: succeededTaskId,
						attempt: 1,
						sessionFile: mainSessionPath,
						manifestRef: join(jobRoot, "tasks", succeededTaskId, "output-manifest.json"),
						status: "completed",
					},
				},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {
					artifact_expanded: { id: "artifact_expanded", status: "active" },
					artifact_indexed: { id: "artifact_indexed", status: "retired" },
				},
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		for (const task of [succeededTask, failedTask]) {
			await writeFile(
				join(jobRoot, "workspaces", task.id, "ASTRA_TASK_CONTEXT.json"),
				JSON.stringify({
					schemaVersion: "astra.task_context.v1",
					mission: { jobId },
					stage: { minSourceRefs: 1 },
					task: { id: task.id },
					inputs: {
						canonicalArtifacts: [{ id: "artifact_expanded" }],
						omittedRefs: task.id === succeededTaskId ? ["artifact_indexed"] : [],
					},
				}),
			);
		}
		await writeFile(
			join(jobRoot, "tasks", succeededTaskId, "output-manifest.json"),
			JSON.stringify({ schemaVersion: "astra.worker_output_manifest.v1", jobId, taskId: succeededTaskId,
				agentId: succeededTask.agentId, artifactType: succeededTask.requiredOutputType, status: "completed", validationStatus: "passed", sessionRef: "main",
				outputRefs: [{ kind: "source", ref: "https://doi.org/10.1000/astra" }] }),
		);
		await writeFile(
			join(jobRoot, "sources", "openalex-W1.json"),
			JSON.stringify(sourceReceipt("openalex:W1", { doi: "10.1000/astra", landingPageUrl: "https://doi.org/10.1000/astra" })),
		);
		await writeFile(
			failedSessionPath,
			`${[
				{ type: "session", id: "failed", cwd: join(jobRoot, "workspaces", failedTaskId) },
				{
					type: "message",
					message: { role: "assistant", stopReason: "error", errorMessage: "recovered fixture failure" },
				},
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n")}\n`,
		);
		await writeFile(
			mainSessionPath,
			`${[
				{ type: "session", id: "main", cwd: join(jobRoot, "workspaces", succeededTaskId) },
				{ type: "message", message: { role: "assistant", stopReason: "error", errorMessage: "transient 502" } },
				{ type: "message", message: { role: "assistant", stopReason: "toolUse" } },
			]
				.map((entry) => JSON.stringify(entry))
				.join("\n")}\n`,
		);

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.equal(report.contractChecks.upstreamArtifactsBound, true);
		assert.equal(report.contractChecks.sourceMinimumsSatisfied, true);
		assert.equal(report.contractChecks.sourceReceiptsPresent, true);
		assert.equal(report.contractChecks.acceptedChildSessionsValid, true);
		assert.equal(report.sessions.failedFiles, 2);
		assert.equal(report.sessions.recoveredFailedFiles, 2);
		assert.equal(report.sessions.unrecoveredFailedFiles, 0);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("accepts a whole-research review that passes with nonblocking caveats", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-review-caveats-"));
	const jobId = "job_review_caveats";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	try {
		await mkdir(jobRoot, { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "research-review" },
				stages: { "research-review": { status: "completed" } },
				eventSeq: 0,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {
					artifact_review: {
						id: "artifact_review",
						type: "research-review",
						status: "active",
						content: { verdict: "pass_with_nonblocking_caveats", requiredRepairs: [] },
					},
				},
				graph: { acceptedClaimIds: [], unresolvedObjectionIds: [], openQuestionIds: [] },
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		assert.equal(JSON.parse(result.stdout).researchQuality.wholeResearchReviewPassed, true);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("reports process completion separately from insufficient scientific evidence", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-scientific-outcome-"));
	const jobId = "job_scientific_outcome";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	try {
		await mkdir(jobRoot, { recursive: true });
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: {
					jobId,
					status: "completed",
					activeStageId: "research-review",
					scientificOutcome: "inconclusive",
					missionCoverage: "insufficient",
					finalDecisionRef: "complete_inconclusive",
				},
				stages: { "research-review": { status: "completed" } },
				eventSeq: 0,
				lease: null,
				tasks: {},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {
					artifact_review: {
						id: "artifact_review",
						type: "research-review",
						status: "active",
						content: {
							verdict: "pass_with_nonblocking_caveats",
							scientificOutcome: "inconclusive",
							missionCoverage: "insufficient",
							requiredRepairs: [],
						},
					},
				},
				graph: { nodes: {}, acceptedClaimIds: [], unresolvedObjectionIds: [], openQuestionIds: [] },
				routeDecisions: {
					complete_inconclusive: { id: "complete_inconclusive", action: "complete" },
				},
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.equal(report.researchQuality.researchCompletedByDecision, true);
		assert.equal(report.researchQuality.scientificOutcomeRecorded, true);
		assert.equal(report.researchQuality.claimOutcomeConsistent, true);
		assert.deepEqual(report.scientificResult, {
			outcome: "inconclusive",
			missionCoverage: "insufficient",
			acceptedClaimCount: 0,
			processCompleted: true,
		});
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

test("does not require a pruned workspace for a worker recorded in a retired artifact receipt", async () => {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-retired-worker-"));
	const jobId = "job_retired_worker";
	const taskId = "task_retired_worker";
	const artifactId = "artifact_retired_review";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	const receiptPath = join(jobRoot, "canonical", `${artifactId}.json.receipt.json`);
	const retired = { artifactId, type: "research-review", evidenceId: "evidence_retired", taskId,
		reviewIds: ["review_retired"], checksum: sha256("retired content"), materializationReceiptRef: receiptPath,
		retiredAt: "2026-10-03T00:00:00.000Z", cleanupStatus: "completed", archiveRefs: [] };
	try {
		await mkdir(join(jobRoot, "canonical"), { recursive: true });
		const plan = {
			id: "plan_retired_review",
			stageId: "research-review",
			tasks: [{ key: "retired", objective: "Produce the superseded whole-research review" }],
		};
		await writeFile(
			join(jobRoot, "job.json"),
			JSON.stringify({
				frame: { jobId, activeStageId: "research-review" },
				stages: { "research-review": { status: "completed" } },
				stagePlans: { [plan.id]: plan },
				eventSeq: 0,
				lease: null,
				tasks: {
					[taskId]: {
						id: taskId,
						role: "worker",
						status: "succeeded",
						stageId: "research-review",
						objective: plan.tasks[0].objective,
						replayKey: `stage-plan:${plan.id}:retired`,
						inputArtifactRefs: ["artifact_upstream"],
					},
				},
				evidence: {},
				reviews: {},
				obligations: {},
				canonical: {},
				retiredArtifacts: {
					[artifactId]: retired,
				},
				graph: { acceptedClaimIds: [], unresolvedObjectionIds: [], openQuestionIds: [] },
			}),
		);
		await writeFile(join(jobRoot, "events.jsonl"), "");
		await writeFile(receiptPath, JSON.stringify({ schemaVersion: "astra.retired_artifact_receipt.v1", ...retired }));

		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], {
			cwd: workspace,
			encoding: "utf8",
		});
		assert.equal(result.status, 0, result.stderr);
		const report = JSON.parse(result.stdout);
		assert.deepEqual(report.workspaces.missingTaskIds, []);
		assert.deepEqual(report.workspaces.missingUpstreamTaskIds, []);
		assert.equal(report.contractChecks.workerContextsValid, true);
		assert.equal(report.contractChecks.upstreamArtifactsBound, true);
		assert.equal(report.contractChecks.retiredArtifactsPruned, true);
	} finally {
		await rm(workspace, { recursive: true, force: true });
	}
});

async function auditFixture() {
	const workspace = await mkdtemp(join(tmpdir(), "astra-audit-integrity-"));
	const jobId = "job_integrity";
	const jobRoot = join(workspace, ".astra", "jobs", jobId);
	await mkdir(join(jobRoot, "canonical"), { recursive: true });
	const content = { content: "accepted result" };
	const evidence = { id: "evidence_current", taskId: "task_current", stageId: "validation", type: "validation",
		content, refs: [], files: [], checksum: checksum(content), status: "accepted" };
	evidence.versionHash = checksum({ content, refs: [], files: [], taskVersion: undefined });
	const path = join(jobRoot, "canonical", "artifact_current.json");
	const bytes = `${JSON.stringify(content, null, 2)}\n`;
	const artifact = { id: "artifact_current", type: "validation", status: "active", evidenceId: evidence.id,
		content, checksum: evidence.checksum, sourceSha256: evidence.checksum, targetSha256: sha256(bytes),
		materializationRef: path, adoptedAt: "2026-10-03T00:00:00.000Z",
		evidenceSnapshotHash: checksum({ stageId: "validation", acceptedRevisionRefs: [evidence.id] }) };
	const receipt = { schemaVersion: "astra.materialization_receipt.v1", artifactId: artifact.id,
		sourceSha256: artifact.sourceSha256, targetSha256: artifact.targetSha256, targetPath: path, createdAt: artifact.adoptedAt };
	const snapshot = { frame: { jobId, activeStageId: "validation" }, stages: {}, stagePlans: {}, eventSeq: 0,
		tasks: {}, evidence: { [evidence.id]: evidence }, reviews: { review_current: { id: "review_current",
			evidenceId: evidence.id, verdict: "pass", score: 1, targetVersionHash: evidence.versionHash } },
		obligations: {}, canonical: { [artifact.id]: artifact }, canonicalRoute: { stageArtifactIds: { validation: artifact.id } } };
	const save = () => writeFile(join(jobRoot, "job.json"), JSON.stringify(snapshot));
	await save();
	await writeFile(join(jobRoot, "events.jsonl"), "");
	await writeFile(path, bytes);
	await writeFile(`${path}.receipt.json`, JSON.stringify(receipt));
	const audit = () => {
		const result = spawnSync(process.execPath, [auditScript, workspace, jobId], { encoding: "utf8" });
		assert.equal(result.status, 0, result.stderr);
		return JSON.parse(result.stdout);
	};
	return { workspace, jobRoot, snapshot, path, receipt, save, audit };
}

for (const owner of ["active", "candidate", "reviewer"]) {
	test(`audits a receipted source task still owned by ${owner}`, async () => {
		const fixture = await auditFixture();
		try {
			const source = { id: "task_current", jobId: "job_integrity", role: "worker", status: "succeeded",
				stageId: "validation", stageRevision: 1, attempt: 1, replayKey: "stage-plan:shared:source",
				objective: "shared source", inputArtifactRefs: [] };
			fixture.snapshot.stages.validation = { status: "running", revision: 1 };
			fixture.snapshot.tasks[source.id] = source;
			fixture.snapshot.stagePlans.shared = { id: "shared", stageId: "validation", tasks: [{ key: "source", objective: source.objective }] };
			fixture.snapshot.discardedEvidence = { historical: { evidenceId: "historical", taskId: source.id } };
			if (owner !== "active") {
				fixture.snapshot.canonical = {};
				fixture.snapshot.canonicalRoute.stageArtifactIds = {};
				fixture.snapshot.evidence.evidence_current.status = "candidate";
			}
			if (owner === "reviewer") {
				fixture.snapshot.tasks.reviewer = { ...source, id: "reviewer", role: "reviewer", status: "ready",
					replayKey: "review:current:1", inputArtifactRefs: ["evidence_current"] };
			}
			// Other receipt collections are intentionally absent, as allowed by the audit input contract.
			await fixture.save();
			const report = fixture.audit();
			assert.deepEqual(report.workspaces.missingTaskIds, [source.id]);
			assert.equal(report.contractChecks.workerContextsValid, false);
		} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
	});
}

test("canonical receipts bind actual bytes, schema, identity, path, and content", async () => {
	const fixture = await auditFixture();
	try {
		assert.equal(fixture.audit().runtimeIntegrity.canonicalReceiptsMatch, true);
		await writeFile(fixture.path, "changed bytes");
		assert.equal(fixture.audit().runtimeIntegrity.canonicalReceiptsMatch, false);
		await rm(fixture.path);
		assert.equal(fixture.audit().runtimeIntegrity.canonicalReceiptsMatch, false);
		await writeFile(fixture.path, `${JSON.stringify(fixture.snapshot.canonical.artifact_current.content, null, 2)}\n`);
		for (const changed of [{ schemaVersion: "wrong" }, { artifactId: "wrong" }, { targetPath: "wrong" }, { createdAt: "wrong" }]) {
			await writeFile(`${fixture.path}.receipt.json`, JSON.stringify({ ...fixture.receipt, ...changed }));
			assert.equal(fixture.audit().runtimeIntegrity.canonicalReceiptsMatch, false, JSON.stringify(changed));
		}
	} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
});

test("canonical history stays reviewed while current evidence or review versions fail", async () => {
	const fixture = await auditFixture();
	try {
		const healthy = fixture.audit();
		assert.equal(healthy.runtimeIntegrity.canonicalEvidenceVersionsMatch, true);
		assert.equal(healthy.researchQuality.canonicalReviewVersionsMatch, true);
		fixture.snapshot.evidence.evidence_current.content = { content: "overwritten current evidence" };
		fixture.snapshot.evidence.evidence_current.versionHash = sha256("different current version");
		await fixture.save();
		const changed = fixture.audit();
		assert.equal(changed.researchQuality.canonicalArtifactsReviewed, true);
		assert.equal(changed.runtimeIntegrity.canonicalEvidenceVersionsMatch, false);
		assert.equal(changed.researchQuality.canonicalReviewVersionsMatch, false);
		assert.ok(changed.canonical.versionParity[0].failures.length > 0);
	} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
});

test("audit reports missing, null, or array required snapshot collections without crashing", async () => {
	const fixture = await auditFixture();
	try {
		for (const key of ["evidence", "reviews", "obligations", "tasks", "canonical", "stages"]) {
			for (const value of [undefined, null, []]) {
				await writeFile(join(fixture.jobRoot, "job.json"), JSON.stringify({ ...fixture.snapshot, [key]: value }));
				const before = await readFile(join(fixture.jobRoot, "job.json"));
				const report = fixture.audit();
				assert.equal(report.passed, false);
				assert.equal(report.contractChecks.inputFilesValid, false);
				assert.ok(report.inputFailures.some((failure) => failure.reason.includes(key)));
				assert.deepEqual(await readFile(join(fixture.jobRoot, "job.json")), before);
			}
		}
	} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
});

test("retired receipt corruption is reported without modifying any input", async () => {
	const fixture = await auditFixture();
	const path = join(fixture.jobRoot, "canonical", "artifact_retired.json.receipt.json");
	const retired = { artifactId: "artifact_retired", type: "validation", evidenceId: "evidence_retired", taskId: "task_retired",
		reviewIds: ["review_retired"], checksum: sha256("retired"), materializationReceiptRef: path,
		retiredAt: "2026-10-03T00:00:00.000Z", cleanupStatus: "completed", archiveRefs: [] };
	try {
		fixture.snapshot.retiredArtifacts = { [retired.artifactId]: retired };
		await fixture.save();
		const receipt = { schemaVersion: "astra.retired_artifact_receipt.v1", ...retired };
		await writeFile(path, JSON.stringify(receipt));
		assert.equal(fixture.audit().runtimeIntegrity.retiredArtifactsPruned, true);
		for (const value of ["{bad json", JSON.stringify({ ...receipt, artifactId: "wrong" }),
			JSON.stringify({ ...receipt, evidenceId: "wrong" }), JSON.stringify({ ...receipt, checksum: sha256("wrong") })]) {
			await writeFile(path, value);
			const before = await readFile(path);
			const snapshotBefore = await readFile(join(fixture.jobRoot, "job.json"));
			const report = fixture.audit();
			assert.equal(report.runtimeIntegrity.retiredArtifactsPruned, false);
			assert.ok(report.canonical.retiredReceiptParity[0].failures.length > 0);
			assert.deepEqual(await readFile(path), before);
			assert.deepEqual(await readFile(join(fixture.jobRoot, "job.json")), snapshotBefore);
		}
	} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
});

test("source minimums count valid distinct canonical sources and respect frozen receipts", async () => {
	const fixture = await auditFixture();
	const taskId = "task_sources";
	const sourceRoot = join(fixture.jobRoot, "sources");
	const manifestPath = join(fixture.jobRoot, "tasks", taskId, "output-manifest.json");
	try {
		await mkdir(sourceRoot);
		await mkdir(join(fixture.jobRoot, "tasks", taskId), { recursive: true });
		await mkdir(join(fixture.jobRoot, "workspaces", taskId), { recursive: true });
		fixture.snapshot.tasks[taskId] = { id: taskId, role: "worker", status: "succeeded", stageId: "literature",
			objective: "Sources", inputArtifactRefs: [] };
		await fixture.save();
		await writeFile(join(fixture.jobRoot, "workspaces", taskId, "ASTRA_TASK_CONTEXT.json"), JSON.stringify({
			schemaVersion: "astra.task_context.v1", mission: { jobId: "job_integrity" }, task: { id: taskId },
			stage: { minSourceRefs: 3 }, inputs: {} }));
		const receipt = sourceReceipt("openalex:W1", { doi: "10.1000/astra", landingPageUrl: "https://example.invalid/paper", pdfUrl: "https://example.invalid/paper.pdf" });
		const sourcePath = join(sourceRoot, "openalex-W1.json");
		await writeFile(sourcePath, JSON.stringify(receipt));
		const saveRefs = (refs) => writeFile(manifestPath, JSON.stringify({ outputRefs: refs.map((ref) => typeof ref === "string" ? { kind: "source", ref } : ref) }));
		await saveRefs(["doi:10.1000/astra", "https://example.invalid/paper", "https://example.invalid/paper.pdf"]);
		assert.equal(fixture.audit().runtimeIntegrity.sourceMinimumsSatisfied, false);
		assert.equal(fixture.audit().runtimeIntegrity.sourceReceiptsPresent, true);
		await saveRefs(["openalex:W1", "openalex:W1", "openalex:W1"]);
		assert.equal(fixture.audit().runtimeIntegrity.sourceMinimumsSatisfied, false);
		await saveRefs(["openalex:W1", "openalex:W2", "openalex:W3"]);
		for (const ref of ["W2", "W3"]) await writeFile(join(sourceRoot, `openalex-${ref}.json`), JSON.stringify(sourceReceipt(`openalex:${ref}`)));
		assert.equal(fixture.audit().runtimeIntegrity.sourceMinimumsSatisfied, true);
		for (const corrupt of [{ ...receipt, sha256: sha256("wrong") }, sourceReceipt("openalex:W1", { sourceRef: "openalex:W9" }), sourceReceipt("openalex:W9"), null]) {
			await writeFile(sourcePath, JSON.stringify(corrupt));
			const report = fixture.audit();
			assert.equal(report.runtimeIntegrity.sourceReceiptsPresent, false);
			assert.equal(report.runtimeIntegrity.sourceMinimumsSatisfied, false);
		}
		await writeFile(sourcePath, "malformed");
		assert.equal(fixture.audit().runtimeIntegrity.sourceReceiptsPresent, false);
		await mkdir(join(fixture.jobRoot, "versions", "files"), { recursive: true });
		const frozen = JSON.stringify(receipt);
		const digest = sha256(frozen);
		await writeFile(join(fixture.jobRoot, "versions", "files", digest), frozen);
		await saveRefs([{ kind: "source", ref: "openalex:W1", sha256: digest }, "openalex:W2", "openalex:W3"]);
		assert.equal(fixture.audit().runtimeIntegrity.sourceReceiptsPresent, true);
		assert.equal(fixture.audit().runtimeIntegrity.sourceMinimumsSatisfied, true);
		await writeFile(join(fixture.jobRoot, "versions", "files", digest), "changed frozen receipt");
		const brokenFrozen = fixture.audit();
		assert.equal(brokenFrozen.runtimeIntegrity.sourceReceiptsPresent, false);
		assert.ok(brokenFrozen.sources.receiptFailures.some((entry) => entry.failures.includes("Frozen source digest mismatch")));
		assert.deepEqual((await readdir(sourceRoot)).sort(), ["openalex-W1.json", "openalex-W2.json", "openalex-W3.json"]);
	} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
});

for (const [name, outputRefs, invalid] of [
	["object outputRefs", {}, true],
	["null outputRefs entry", [null], true],
	["null outputRefs collection", null, true],
	["missing outputRefs collection", undefined, true],
	["array outputRefs entry", [[]], true],
	["scalar outputRefs entries", [false, 3, "source"], true],
	["non-string source ref", [{ kind: "source", ref: {} }], true],
	["null source ref", [{ kind: "source", ref: null }], true],
	["missing source ref", [{ kind: "source" }], true],
	["empty source ref", [{ kind: "source", ref: "" }], true],
	["valid empty outputRefs", [], false],
]) {
	test(`audit returns a read-only negative report for ${name}`, async () => {
		const fixture = await auditFixture();
		const taskId = "task_sources";
		const manifestPath = join(fixture.jobRoot, "tasks", taskId, "output-manifest.json");
		try {
			await mkdir(join(fixture.jobRoot, "tasks", taskId), { recursive: true });
			await mkdir(join(fixture.jobRoot, "workspaces", taskId), { recursive: true });
			fixture.snapshot.tasks[taskId] = { id: taskId, role: "worker", status: "succeeded", stageId: "literature",
				objective: "Sources", inputArtifactRefs: [] };
			await fixture.save();
			await writeFile(join(fixture.jobRoot, "workspaces", taskId, "ASTRA_TASK_CONTEXT.json"), JSON.stringify({
				schemaVersion: "astra.task_context.v1", mission: { jobId: "job_integrity" }, task: { id: taskId },
				stage: { minSourceRefs: 1 }, inputs: {} }));
			await writeFile(manifestPath, JSON.stringify({ outputRefs }));
			const beforeManifest = await readFile(manifestPath);
			const beforeSnapshot = await readFile(join(fixture.jobRoot, "job.json"));
			const report = fixture.audit();
			assert.equal(report.passed, false);
			assert.equal(report.runtimeIntegrity.sourceMinimumsSatisfied, false);
			assert.equal(report.contractChecks.inputFilesValid, !invalid);
			assert.equal(report.inputFailures.some((failure) => failure.path === manifestPath && failure.reason.includes("outputRefs")), invalid);
			assert.deepEqual(await readFile(manifestPath), beforeManifest);
			assert.deepEqual(await readFile(join(fixture.jobRoot, "job.json")), beforeSnapshot);
		} finally { await rm(fixture.workspace, { recursive: true, force: true }); }
	});
}
