#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const inputFailures = [];
function readJson(path) {
	try { return JSON.parse(readFileSync(path, "utf8")); }
	catch (error) { inputFailures.push({ path, reason: error.message }); return null; }
}

function stableJson(value) {
	if (value === null || typeof value !== "object") return JSON.stringify(value);
	if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
	return `{${Object.entries(value).sort(([left], [right]) => left.localeCompare(right))
		.map(([key, entry]) => `${JSON.stringify(key)}:${stableJson(entry)}`).join(",")}}`;
}
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const checksum = (value) => sha256(stableJson(value));
const isDigest = (value) => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);

function readAuditedBytes(path, failures) {
	try {
		if (!lstatSync(path).isFile()) throw new Error("not a regular file");
		return readFileSync(path);
	} catch (error) { failures.push(`Cannot read ${path}: ${error.message}`); return null; }
}

function readJsonIfPresent(path) {
	return existsSync(path) ? readJson(path) : null;
}

function readJsonl(path) {
	if (!existsSync(path)) return [];
	try {
		return readFileSync(path, "utf8").split("\n").filter(Boolean).flatMap((line, index) => {
			try { return [JSON.parse(line)]; }
			catch (error) { inputFailures.push({ path, line: index + 1, reason: error.message }); return []; }
		});
	} catch (error) { inputFailures.push({ path, reason: error.message }); return []; }
}

function collectJsonlFiles(root) {
	if (!root || !existsSync(root)) return [];
	const files = [];
	for (const entry of readdirSync(root, { withFileTypes: true })) {
		const path = join(root, entry.name);
		if (entry.isDirectory()) files.push(...collectJsonlFiles(path));
		else if (entry.isFile() && entry.name.endsWith(".jsonl")) files.push(path);
	}
	return files;
}

function usage() {
	console.error("Usage: node scripts/audit-astra-run.mjs <workspace> [job-id] [parent-session-dir]");
	process.exit(2);
}

const workspace = resolve(process.argv[2] ?? ".");
const activeJobPath = join(workspace, ".astra", "active-job.json");
const jobId = process.argv[3] ?? (existsSync(activeJobPath) ? readJson(activeJobPath)?.jobId : undefined);
const parentSessionRoot = process.argv[4] ? resolve(process.argv[4]) : process.env.PI_CODING_AGENT_SESSION_DIR;
if (!jobId) usage();

const jobRoot = join(workspace, ".astra", "jobs", jobId);
const snapshotPath = join(jobRoot, "job.json");
if (!existsSync(snapshotPath)) throw new Error(`Astra job snapshot not found: ${snapshotPath}`);

const snapshot = readJson(snapshotPath);
const invalidSnapshotFields = ["frame", "tasks", "evidence", "reviews", "obligations", "canonical", "stages"]
	.filter((key) => !snapshot?.[key] || typeof snapshot[key] !== "object" || Array.isArray(snapshot[key]));
if (invalidSnapshotFields.length > 0) {
	console.log(JSON.stringify({ schemaVersion: "astra.run_audit.v4", passed: false, workspace, jobId,
		contractChecks: { inputFilesValid: false }, inputFailures: [...inputFailures,
			{ path: snapshotPath, reason: `Invalid job snapshot collections: ${invalidSnapshotFields.join(", ")}` }] }, null, 2));
	process.exit(0);
}
const backend = readJsonIfPresent(join(jobRoot, "backend.json"))?.backend ?? "pi";
if (!["pi", "codex"].includes(backend)) throw new Error(`Unknown Astra backend: ${backend}`);
const events = readJsonl(join(jobRoot, "events.jsonl"));
const stages = Object.values(snapshot.stages);
const stagePlans = Object.values(snapshot.stagePlans ?? {});
const workerTasks = Object.values(snapshot.tasks).filter((task) => task.role === "worker");
const discardedCandidateReceipts = Object.values(snapshot.discardedCandidates ?? {});
const discardedEvidenceReceipts = Object.values(snapshot.discardedEvidence ?? {});
const retiredArtifacts = Object.values(snapshot.retiredArtifacts ?? {});
const discardedTaskIds = new Set(
	discardedCandidateReceipts
		.map((receipt) => receipt.taskId)
		.concat(discardedEvidenceReceipts.map((receipt) => receipt.taskId))
		.concat(retiredArtifacts.map((receipt) => receipt.taskId))
		.filter(Boolean),
);
const auditedWorkerTasks = workerTasks.filter((task) => !discardedTaskIds.has(task.id));
const plannedTasks = new Map(
	stagePlans.flatMap((plan) =>
		plan.tasks.map((task) => [`stage-plan:${plan.id}:${task.key}`, { plan, task }]),
	),
);
const executedStageIds = [...new Set(auditedWorkerTasks.map((task) => task.stageId))];
const missingStagePlanIds = executedStageIds.filter(
	(stageId) => !stagePlans.some((plan) => plan.stageId === stageId),
);
const unplannedWorkerTaskIds = auditedWorkerTasks
	.filter((task) => !plannedTasks.has(task.replayKey))
	.map((task) => task.id);
const objectiveMismatchTaskIds = auditedWorkerTasks
	.filter((task) => plannedTasks.get(task.replayKey)?.task.objective !== task.objective)
	.map((task) => task.id);
const forbiddenGenericTaskIds = auditedWorkerTasks
	.filter((task) => /worker wave/i.test(task.objective))
	.map((task) => task.id);
const workerContexts = auditedWorkerTasks.map((task) => {
	const path = join(jobRoot, "workspaces", task.id, "ASTRA_TASK_CONTEXT.json");
	const context = readJsonIfPresent(path);
	const expectedCanonicalRefs = task.inputArtifactRefs.filter((ref) => snapshot.canonical[ref]);
	const contextCanonicalRefs = new Set(
		(context?.inputs?.canonicalArtifacts ?? [])
			.map((artifact) => artifact.id)
			.concat(context?.inputs?.omittedRefs ?? []),
	);
	return {
		task,
		path,
		context,
		contextMatchesTask:
			context?.schemaVersion === "astra.task_context.v1" &&
			context?.mission?.jobId === snapshot.frame.jobId &&
			context?.task?.id === task.id,
		upstreamBound: expectedCanonicalRefs.every((ref) => contextCanonicalRefs.has(ref)),
	};
});
const missingContextTaskIds = workerContexts.filter((entry) => !entry.context).map((entry) => entry.task.id);
const invalidContextTaskIds = workerContexts
	.filter((entry) => entry.context && !entry.contextMatchesTask)
	.map((entry) => entry.task.id);
const missingUpstreamTaskIds = workerContexts
	.filter((entry) => !entry.upstreamBound)
	.map((entry) => entry.task.id);
const sourceTaskAudits = workerContexts.flatMap((entry) => {
	if (entry.task.status !== "succeeded") return [];
	const minSourceRefs = Number(entry.context?.stage?.minSourceRefs ?? 0);
	if (minSourceRefs <= 0) return [];
	const manifestPath = join(jobRoot, "tasks", entry.task.id, "output-manifest.json");
	const manifest = readJsonIfPresent(manifestPath);
	if (!Array.isArray(manifest?.outputRefs)) inputFailures.push({ path: manifestPath, reason: "outputRefs must be an array" });
	const sourceOutputs = (Array.isArray(manifest?.outputRefs) ? manifest.outputRefs : []).flatMap((output, index) => {
		if (!output || typeof output !== "object" || Array.isArray(output)) {
			inputFailures.push({ path: manifestPath, reason: `outputRefs[${index}] must be an object` });
			return [];
		}
		if (output.kind !== "source") return [];
		if (typeof output.ref !== "string" || !output.ref.trim()) {
			inputFailures.push({ path: manifestPath, reason: `outputRefs[${index}].ref must be a nonempty source string` });
			return [];
		}
		return [output];
	});
	const sourceRefs = sourceOutputs.map((output) => output.ref);
	return [{ taskId: entry.task.id, stageId: entry.task.stageId, minSourceRefs, sourceRefs,
		sourceOutputs, manifestPath }];
});
const sourceRoot = join(jobRoot, "sources");
function sourceFilename(sourceRef) {
	const id = /^openalex:(W\d+)$/.exec(sourceRef)?.[1];
	if (id) return `openalex-${id}.json`;
	return /^(?:doi:10\.\d{4,9}\/\S+|arxiv:(?:\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})|https:\/\/\S+)$/.test(sourceRef)
		? `source-${sha256(sourceRef)}.json` : undefined;
}
function auditSourceReceipt(path, expectedDigest) {
	const failures = [];
	const bytes = readAuditedBytes(path, failures);
	let receipt = null;
	if (bytes) {
		try { receipt = JSON.parse(bytes.toString("utf8")); }
		catch (error) { failures.push(`Invalid source receipt JSON: ${error.message}`); }
	}
	if (expectedDigest && (!isDigest(expectedDigest) || !bytes || sha256(bytes) !== expectedDigest)) failures.push("Frozen source digest mismatch");
	if (!receipt || typeof receipt !== "object" || Array.isArray(receipt)) failures.push("Invalid source receipt object");
	else {
		const { sha256: digest, ...body } = receipt;
		if (!isDigest(digest) || digest !== sha256(JSON.stringify(body))) failures.push("Source receipt internal sha256 mismatch");
		if (!sourceFilename(receipt.sourceRef) || receipt.record?.sourceRef !== receipt.sourceRef) failures.push("Source receipt identity mismatch");
		if (!expectedDigest && path !== join(sourceRoot, sourceFilename(receipt.sourceRef) ?? "invalid")) failures.push("Source receipt filename mismatch");
	}
	const doi = receipt?.record?.doi;
	const aliases = receipt ? [receipt.sourceRef, receipt.record?.sourceRef, receipt.record?.landingPageUrl,
		receipt.record?.pdfUrl, doi, doi ? `doi:${doi}` : undefined, doi ? `https://doi.org/${doi}` : undefined]
		.filter((ref) => typeof ref === "string") : [];
	return { path, receipt, failures, aliases, valid: failures.length === 0 };
}
const sourceReceipts = existsSync(sourceRoot)
	? readdirSync(sourceRoot)
			.filter((name) => name.endsWith(".json"))
			.map((name) => auditSourceReceipt(join(sourceRoot, name)))
	: [];
const frozenSourceReceipts = new Map();
const sourceRefAudits = sourceTaskAudits.flatMap((entry) => entry.sourceOutputs.map((output) => {
	let candidates;
	if (output.sha256 !== undefined) {
		if (!frozenSourceReceipts.has(output.sha256)) frozenSourceReceipts.set(output.sha256,
			auditSourceReceipt(join(jobRoot, "versions", "files", isDigest(output.sha256) ? output.sha256 : "invalid-digest"), output.sha256));
		candidates = [frozenSourceReceipts.get(output.sha256)];
	} else candidates = sourceReceipts;
	const matches = candidates.filter((candidate) => candidate.valid && candidate.aliases.includes(output.ref));
	const canonicalRefs = [...new Set(matches.map((candidate) => candidate.receipt.sourceRef))];
	return { taskId: entry.taskId, ref: output.ref, canonicalSourceRef: canonicalRefs.length === 1 ? canonicalRefs[0] : null,
		failures: canonicalRefs.length === 1 ? [] : [canonicalRefs.length ? "Ambiguous source alias" : "No intact matching source receipt"] };
}));
const insufficientSourceTaskIds = sourceTaskAudits
	.filter((entry) => new Set(sourceRefAudits.filter((source) => source.taskId === entry.taskId && source.canonicalSourceRef)
		.map((source) => source.canonicalSourceRef)).size < entry.minSourceRefs)
	.map((entry) => entry.taskId);
const missingSourceReceiptRefs = [
	...new Set(sourceRefAudits.filter((entry) => !entry.canonicalSourceRef).map((entry) => entry.ref)),
].sort();
const activeArtifacts = Object.values(snapshot.canonical).filter((artifact) => artifact.status === "active");
const canonicalEvidenceCounts = Object.values(snapshot.canonical).reduce((counts, artifact) => {
	counts[artifact.evidenceId] = (counts[artifact.evidenceId] ?? 0) + 1;
	return counts;
}, {});
const duplicateCanonicalEvidenceIds = Object.entries(canonicalEvidenceCounts)
	.filter(([, count]) => count > 1)
	.map(([evidenceId]) => evidenceId)
	.sort();
const artifactDir = join(jobRoot, "canonical");
const receiptFor = (artifactId) => join(artifactDir, `${artifactId}.json.receipt.json`);
const receiptParity = activeArtifacts.map((artifact) => {
	const failures = [];
	const receiptPath = receiptFor(artifact.id);
	const receipt = existsSync(receiptPath) ? readJson(receiptPath) : null;
	const expectedPath = join(artifactDir, `${artifact.id}.json`);
	const bytes = readAuditedBytes(expectedPath, failures);
	const expectedReceipt = { schemaVersion: "astra.materialization_receipt.v1", artifactId: artifact.id,
		sourceSha256: artifact.sourceSha256, targetSha256: artifact.targetSha256, targetPath: expectedPath, createdAt: artifact.adoptedAt };
	if (!receipt || checksum(receipt) !== checksum(expectedReceipt)) failures.push("Materialization receipt schema/content/state mismatch");
	if (artifact.materializationRef !== expectedPath) failures.push("Materialization path mismatch");
	if (!isDigest(artifact.sourceSha256) || artifact.sourceSha256 !== artifact.checksum) failures.push("Source checksum mismatch");
	if (!isDigest(artifact.targetSha256) || artifact.targetSha256 !== sha256(`${JSON.stringify(artifact.content, null, 2)}\n`)) failures.push("Canonical content digest mismatch");
	if (!bytes || sha256(bytes) !== artifact.targetSha256) failures.push("Materialized bytes digest mismatch");
	return {
		artifactId: artifact.id,
		status: artifact.status,
		evidenceId: artifact.evidenceId,
		materializationRef: artifact.materializationRef ?? null,
		receiptPath,
		receiptPresent: receipt !== null,
		receiptMatchesState: failures.length === 0,
		failures,
		sourceSha256: artifact.sourceSha256 ?? null,
		targetSha256: artifact.targetSha256 ?? null,
	};
});
const retiredReceiptParity = retiredArtifacts.map((state) => {
	const receiptPath = state.materializationReceiptRef ?? receiptFor(state.artifactId);
	const receipt = readJsonIfPresent(receiptPath);
	const failures = [];
	if (receiptPath !== receiptFor(state.artifactId)) failures.push("Retired receipt path mismatch");
	if (!isDigest(state.checksum) || !state.evidenceId || !state.taskId || !state.type || !Array.isArray(state.reviewIds) ||
		state.cleanupStatus !== "completed" || !state.retiredAt) failures.push("Retired receipt state is incomplete");
	if (!receipt || checksum(receipt) !== checksum({ schemaVersion: "astra.retired_artifact_receipt.v1", ...state })) failures.push("Retired receipt schema/identity/checksum/state mismatch");
	return { artifactId: state.artifactId, receiptPath, receiptPresent: receipt !== null,
		receiptMatchesState: failures.length === 0, failures,
		materializedArtifactRemoved: !existsSync(join(artifactDir, `${state.artifactId}.json`)) };
});
const versionParity = activeArtifacts.map((artifact) => {
	const evidence = snapshot.evidence[artifact.evidenceId];
	const failures = [];
	if (!evidence || evidence.status !== "accepted") failures.push("Current source evidence is missing or not accepted");
	if (evidence) {
		const expectedChecksum = checksum(evidence.incrementalRevision ? { content: evidence.content, incrementalRevision: evidence.incrementalRevision } : evidence.content);
		const expectedVersion = checksum({ content: evidence.content, refs: evidence.refs, files: evidence.files,
			taskVersion: evidence.taskVersion, ...(evidence.incrementalRevision ? { incrementalRevision: evidence.incrementalRevision } : {}) });
		if (evidence.checksum !== expectedChecksum || artifact.checksum !== evidence.checksum ||
			artifact.sourceSha256 !== evidence.checksum || checksum(artifact.content) !== checksum(evidence.content) || artifact.type !== evidence.type)
			failures.push("Canonical content/checksum does not belong to current source evidence");
		if (!isDigest(evidence.versionHash) || evidence.versionHash !== expectedVersion) failures.push("Current source evidence versionHash mismatch");
		const evidenceSetId = evidence.currentEvidenceSetId ?? evidence.taskId;
		const acceptedRevisionRefs = Object.values(snapshot.evidence).filter((entry) => entry.status === "accepted" &&
			(entry.currentEvidenceSetId ?? entry.taskId) === evidenceSetId).map((entry) => entry.id).sort();
		if (artifact.evidenceSnapshotHash !== checksum({ stageId: evidence.stageId, acceptedRevisionRefs })) failures.push("Canonical evidence snapshot mismatch");
	}
	const requiredPassingReviews = ["result-to-claim", "research-review"].includes(artifact.type) ? 2 : 1;
	const reviews = Object.values(snapshot.reviews ?? {}).filter((review) => review.evidenceId === artifact.evidenceId &&
		review.verdict === "pass" && Number(review.score ?? 0) >= 0.8);
	const currentReviewIds = reviews.filter((review) => evidence && isDigest(evidence.versionHash) && review.targetVersionHash === evidence.versionHash).map((review) => review.id);
	const evidenceMatches = failures.length === 0;
	const reviewsMatch = currentReviewIds.length >= requiredPassingReviews;
	if (!reviewsMatch) failures.push("Passing reviews target a different or missing current evidence version");
	return { artifactId: artifact.id, evidenceId: artifact.evidenceId, currentVersionHash: evidence?.versionHash ?? null,
		historicalPassingReviewIds: reviews.map((review) => review.id), currentPassingReviewIds: currentReviewIds,
		evidenceMatches, reviewsMatch, failures };
});
const eventTypes = {};
for (const stored of events) {
	const type = stored.event?.type ?? "unknown";
	eventTypes[type] = (eventTypes[type] ?? 0) + 1;
}
const sequenceContinuous = events.every((stored, index) => stored.seq === index + 1);
const jobWasResumed = events.some((stored) => ["job_resumed", "user_gate_approved"].includes(stored.event?.type));
const migrationPath = join(workspace, ".astra", "migrations", "pmcli-import-report.json");
const sessionsPath = join(jobRoot, "sessions");
const sessionFiles = existsSync(sessionsPath)
	? readdirSync(sessionsPath).filter((name) => name.endsWith(".jsonl")).map((name) => join(sessionsPath, name))
	: [];
const failedSessions = sessionFiles.filter((path) =>
	readJsonl(path).some(
		(entry) =>
			entry.status === "failed" ||
			entry.data?.status === "failed" ||
			(entry.message?.role === "assistant" && ["error", "aborted"].includes(entry.message.stopReason)),
	),
);
const sessionRecords = Object.values(snapshot.sessions ?? {});
const sessionRecordsById = new Map(sessionRecords.map((session) => [session.sessionId, session]));
const sessionRecordsByFile = new Map(
	(backend === "codex"
		? events.filter((entry) => entry.event?.type === "child_session_recorded").map((entry) => entry.event.session).concat(sessionRecords)
		: sessionRecords).flatMap((session) =>
		session.sessionFile ? [[resolve(session.sessionFile), session]] : [],
	),
);
// The persistent main thread has one current snapshot record but many decision
// logs. Audit the event ledger's per-file history, not just its latest file.
const archivedTaskRoots = new Set(
	[...discardedCandidateReceipts, ...discardedEvidenceReceipts, ...retiredArtifacts]
		.flatMap((receipt) => receipt.archiveRefs ?? []).filter((path) => existsSync(path)).map((path) => resolve(path)),
);
const completedSessionMoves = new Map();
for (const intent of Object.values(snapshot.cleanupIntents ?? {})) {
	if (intent.status !== "completed") continue;
	for (const task of intent.tasks) {
		const taskArchive = resolve(jobRoot, "archive", "tasks", task.taskId);
		if (!events.some((entry) => entry.event?.type === "cleanup_completed" &&
			entry.event.intentId === intent.id && entry.event.archiveRefs.some((path) => resolve(path) === taskArchive))) continue;
		for (const moved of task.sessions) {
			const target = resolve(taskArchive, "sessions", `${createHash("sha256").update(moved.sessionId).digest("hex")}.jsonl`);
			const current = sessionRecordsById.get(moved.sessionId);
			if (!moved.present || resolve(moved.target) !== target || current?.taskId !== task.taskId ||
				!current.sessionFile || resolve(current.sessionFile) !== target) continue;
			completedSessionMoves.set(JSON.stringify([task.taskId, moved.sessionId, resolve(moved.source)]), target);
		}
	}
}
const codexSessionAudits = backend === "codex" ? [...sessionRecordsByFile].map(([path, session]) => {
	const archivedPath = completedSessionMoves.get(JSON.stringify([session.taskId, session.sessionId, path]));
	const entries = readJsonl(archivedPath ?? path);
	const identities = entries.filter((entry) => entry.method === "astra/session");
	const current = entries.slice(entries.findLastIndex((entry) => entry.method === "astra/session"));
	const finalIndex = current.findLastIndex((entry) => entry.method === "item/completed" &&
		entry.params?.item?.type === "agentMessage" && ["final_answer", null, undefined].includes(entry.params.item.phase));
	const completedIndex = current.findLastIndex((entry) => entry.method === "turn/completed");
	const started = current.findLast((entry) => entry.method === "turn/started");
	const completed = current[completedIndex];
	let structuredOutput = false;
	try {
		const output = JSON.parse(current[finalIndex]?.params?.item?.text ?? "");
		structuredOutput = output !== null && typeof output === "object" && !Array.isArray(output);
	} catch { /* Missing or malformed final output is an audit failure. */ }
	const verified = identities.length > 0 && identities.every((entry) =>
		entry.params?.threadId === session.sessionId && entry.params.modelProvider === "openai" &&
		entry.params.permissionProfile === "astra" && typeof entry.params.model === "string" && entry.params.model.length > 0) &&
		Boolean(started) && finalIndex >= 0 && completedIndex > finalIndex && structuredOutput &&
		completed?.params?.threadId === session.sessionId && completed.params.turn?.status === "completed" &&
		completed.params.turn?.id === started?.params?.turn?.id;
	const failed = session.status === "failed" || entries.some((entry) =>
		entry.method === "turn/completed" && entry.params?.turn?.status !== "completed");
	const terminalFailure = ["failed", "aborted"].includes(session.status) &&
		!activeArtifacts.some((artifact) => snapshot.evidence[artifact.evidenceId]?.taskId === session.taskId);
	const pruned = !existsSync(path) && !sessionRecordsById.get(session.sessionId)?.sessionFile &&
		archivedTaskRoots.has(resolve(jobRoot, "archive", "tasks", session.taskId));
	return { path, sessionId: session.sessionId, status: session.status, verified, failed, pruned,
		recovered: pruned || terminalFailure || (session.status === "completed" && verified),
		models: identities.map((entry) => entry.params?.model).filter((model) => typeof model === "string") };
}) : [];
const codexFiles = backend === "codex" ? collectJsonlFiles(join(jobRoot, "codex-events")) : [];
const untrackedCodexFiles = codexFiles.filter((path) => !sessionRecordsByFile.has(resolve(path)));
const codexSessionsVerified = codexSessionAudits.length > 0 && untrackedCodexFiles.length === 0 &&
	codexSessionAudits.every((entry) => entry.recovered);
const failedSessionAudits = failedSessions.map((path) => {
	const entries = readJsonl(path);
	const sessionId = entries.find((entry) => entry.type === "session")?.id;
	const session = sessionRecordsByFile.get(resolve(path)) ?? sessionRecordsById.get(sessionId);
	const task = session?.taskId ? snapshot.tasks[session.taskId] : undefined;
	const lastFailureIndex = entries.findLastIndex(
		(entry) =>
			entry.status === "failed" ||
			entry.data?.status === "failed" ||
			(entry.message?.role === "assistant" && ["error", "aborted"].includes(entry.message.stopReason)),
	);
	const succeededAfterFailure = entries.some(
		(entry, index) =>
			index > lastFailureIndex &&
			entry.message?.role === "assistant" &&
			["stop", "toolUse"].includes(entry.message.stopReason),
	);
	return {
		path,
		recovered:
			Boolean(session) &&
			(["failed", "aborted"].includes(session.status) ||
				task?.status === "failed" ||
				(session.status === "completed" && succeededAfterFailure)),
	};
});
const recoveredFailedSessions = failedSessionAudits.filter((entry) => entry.recovered);
const unrecoveredFailedSessions = failedSessionAudits.filter((entry) => !entry.recovered);
const parentSessionFiles = collectJsonlFiles(parentSessionRoot);
const parentSessionAudits = parentSessionFiles.map((path) => {
	const entries = readJsonl(path);
	const results = entries.filter(
		(entry) =>
			(entry.type === "custom" || entry.type === "custom_message") &&
			entry.customType === "astra_research_result" &&
			["run", "resume"].includes(entry.data?.action) &&
			entry.data?.jobId === jobId,
	);
	const checkpoints = entries.filter(
		(entry) => entry.type === "custom" && entry.customType === "astra_checkpoint" && entry.data?.jobId === jobId,
	);
	return {
		path,
		results,
		checkpoints,
		compactions: entries.filter((entry) => entry.type === "compaction"),
	};
});
const matchingParentSessions = parentSessionAudits
	.filter((session) => session.results.length > 0)
	.map((session) => ({
		path: session.path,
		resultEntries: session.results.length,
		actions: [...new Set(session.results.map((entry) => entry.data.action))].sort(),
	}));
const parentCheckpointSessions = parentSessionAudits.filter((session) => session.checkpoints.length > 0);
const parentCompactionSessions = parentSessionAudits.filter((session) => session.compactions.length > 0);
const recoveryRequired =
	jobWasResumed || parentSessionAudits.some((session) => session.results.some((entry) => entry.data?.action === "resume"));
const recoveryCheckpointSessions = parentSessionAudits.filter(
	(session) =>
		session.results.some((entry) => entry.data?.action === "resume") &&
		session.checkpoints.length > 0,
);
const canonicalRouteArtifactIds = Object.values(snapshot.canonicalRoute?.stageArtifactIds ?? {});
const activeArtifactIds = new Set(activeArtifacts.map((artifact) => artifact.id));
const routeUsesOnlyActiveArtifacts =
	canonicalRouteArtifactIds.every((artifactId) => activeArtifactIds.has(artifactId)) &&
	activeArtifacts.every((artifact) => canonicalRouteArtifactIds.includes(artifact.id));
const selectedSearches = Object.values(snapshot.searchBatches ?? {}).filter((batch) => batch.status === "selected");
const candidateEvaluations = Object.values(snapshot.candidateEvaluations ?? {});
const selectedSearchesEvaluated = selectedSearches.every((batch) => {
	const evaluations = candidateEvaluations.filter((evaluation) => evaluation.batchId === batch.id);
	const selected = evaluations.find((evaluation) => evaluation.candidateId === batch.selectedCandidateId);
	const losers = Object.values(batch.candidates).filter((candidate) => candidate.id !== batch.selectedCandidateId);
	return (
		Boolean(selected && selected.verdict === "pass") &&
		Object.values(batch.candidates).every(
			(candidate) =>
				candidate.status === "failed" || evaluations.some((evaluation) => evaluation.candidateId === candidate.id),
		) &&
		losers.every((candidate) => snapshot.discardedCandidates?.[candidate.id])
	);
});
const canonicalArtifactsReviewed = activeArtifacts.every((artifact) => {
	const requiredPassingReviews = ["result-to-claim", "research-review"].includes(artifact.type) ? 2 : 1;
	return (
		Object.values(snapshot.reviews ?? {}).filter(
			(review) =>
				review.evidenceId === artifact.evidenceId &&
				review.verdict === "pass" &&
				Number(review.score ?? 0) >= 0.8,
		).length >= requiredPassingReviews
	);
});
const acceptedClaimIds = snapshot.graph?.acceptedClaimIds ?? [];
const acceptedClaimNodes = acceptedClaimIds.flatMap((id) => {
	const node = snapshot.graph?.nodes?.[id];
	return node ? [node] : [];
});
const unresolvedObjectionIds = snapshot.graph?.unresolvedObjectionIds ?? [];
const openQuestionIds = snapshot.graph?.openQuestionIds ?? [];
const finalReviewArtifact = activeArtifacts.find((artifact) => artifact.type === "research-review");
const finalReviewContent =
	finalReviewArtifact?.content && typeof finalReviewArtifact.content === "object" && !Array.isArray(finalReviewArtifact.content)
		? finalReviewArtifact.content
		: null;
const finalReviewVerdict = String(finalReviewContent?.verdict ?? "").toLowerCase();
const wholeResearchReviewPassed =
	["pass", "pass_with_nonblocking_caveats", "accept", "accepted"].includes(finalReviewVerdict) &&
	(!Array.isArray(finalReviewContent?.requiredRepairs) || finalReviewContent.requiredRepairs.length === 0);
const scientificOutcome = String(snapshot.frame.scientificOutcome ?? "pending");
const missionCoverage = String(snapshot.frame.missionCoverage ?? "pending");
const scientificOutcomeRecorded =
	["supported", "partially-supported", "refuted", "inconclusive", "insufficient-evidence"].includes(
		scientificOutcome,
	) && ["sufficient", "insufficient"].includes(missionCoverage);
const claimOutcomeConsistent =
	acceptedClaimNodes.length === acceptedClaimIds.length &&
	acceptedClaimNodes.every((node) => ["supported", "partially-supported"].includes(node.claimAssessment)) &&
	(!["supported", "partially-supported"].includes(scientificOutcome) || acceptedClaimIds.length > 0);
const wholeResearchReviewConfirmsOutcome =
	String(finalReviewContent?.scientificOutcome ?? "") === scientificOutcome &&
	String(finalReviewContent?.missionCoverage ?? "") === missionCoverage;
const completionDecision = Object.values(snapshot.routeDecisions ?? {}).find(
	(decision) => decision.id === snapshot.frame.finalDecisionRef && decision.action === "complete",
);
const mainAgentSessionStable = (backend === "codex" ? [...sessionRecordsByFile.values()] : sessionRecords)
	.filter((session) => session.role === "main-agent")
	.every((session) => session.sessionId === snapshot.mainAgentSessionId);
const runtimeIntegrity = {
	eventSequenceContinuous: sequenceContinuous && snapshot.eventSeq === events.length,
	stagePlansComplete: missingStagePlanIds.length === 0,
	workerTasksComeFromPlans: unplannedWorkerTaskIds.length === 0 && objectiveMismatchTaskIds.length === 0,
	noGenericWorkerWaves: forbiddenGenericTaskIds.length === 0,
	workerContextsValid: missingContextTaskIds.length === 0 && invalidContextTaskIds.length === 0,
	upstreamArtifactsBound: missingUpstreamTaskIds.length === 0,
	sourceMinimumsSatisfied: insufficientSourceTaskIds.length === 0,
	sourceReceiptsPresent: missingSourceReceiptRefs.length === 0,
	canonicalRouteIsUnique: routeUsesOnlyActiveArtifacts && new Set(canonicalRouteArtifactIds).size === canonicalRouteArtifactIds.length,
	canonicalReceiptsMatch: receiptParity.every((entry) => entry.receiptMatchesState) && versionParity.every((entry) => entry.evidenceMatches),
	canonicalEvidenceVersionsMatch: versionParity.every((entry) => entry.evidenceMatches),
	retiredArtifactsPruned: retiredReceiptParity.every(
		(entry) => entry.receiptMatchesState && entry.materializedArtifactRemoved,
	),
	mainAgentSessionStable,
	acceptedChildSessionsValid: backend === "codex" ? codexSessionsVerified : unrecoveredFailedSessions.length === 0,
	...(backend === "codex" ? { codexSessionsVerified } : {
		parentControlRecorded: !parentSessionRoot || matchingParentSessions.length > 0,
	}),
	recoveryCheckpointed: !recoveryRequired || (backend === "codex"
		? events.slice(events.findLastIndex((entry) => ["job_resumed", "user_gate_approved"].includes(entry.event?.type)) + 1)
			.some((entry) => entry.event?.type === "child_session_recorded" && entry.event.session.status === "completed")
		: recoveryCheckpointSessions.length > 0),
};
const researchQuality = {
	researchCompletedByDecision: snapshot.frame.status === "completed" && Boolean(completionDecision),
	canonicalArtifactsReviewed,
	canonicalReviewVersionsMatch: versionParity.every((entry) => entry.reviewsMatch),
	selectedSearchesEvaluated,
	scientificOutcomeRecorded,
	claimOutcomeConsistent,
	wholeResearchReviewPassed,
	wholeResearchReviewConfirmsOutcome,
	noOpenObligations: Object.values(snapshot.obligations ?? {}).every((obligation) => obligation.status !== "open"),
	noBlockingObjections: unresolvedObjectionIds.length === 0,
};
const unresolvedUncertainty = {
	openQuestionIds,
	unresolvedObjectionIds,
	openObligationIds: Object.values(snapshot.obligations ?? {})
		.filter((obligation) => obligation.status === "open")
		.map((obligation) => obligation.id),
};
const contractChecks = { ...runtimeIntegrity, ...researchQuality };

const report = {
	schemaVersion: "astra.run_audit.v4",
	backend,
	passed: Object.values(contractChecks).every(Boolean),
	contractChecks,
	runtimeIntegrity,
	researchQuality,
	unresolvedUncertainty,
	scientificResult: {
		outcome: scientificOutcome,
		missionCoverage,
		acceptedClaimCount: acceptedClaimIds.length,
		processCompleted: snapshot.frame.status === "completed",
	},
	workspace,
	jobId,
	policy: {
		automation: snapshot.frame.automation ?? null,
		paused: Boolean(snapshot.paused),
		userGate: snapshot.frame.userGate ?? null,
		budget: snapshot.frame.budget
			? {
					...snapshot.frame.budget,
					tasksUsed: Object.keys(snapshot.tasks).length,
					turnsUsed: snapshot.budgetUsage?.turnsUsed ?? 0,
					costUsdUsed: snapshot.budgetUsage?.costUsdUsed ?? 0,
				}
			: null,
	},
	stages: {
		total: stages.length,
		completed: stages.filter((stage) => stage.status === "completed").length,
		activeStageId: snapshot.frame.activeStageId,
	},
	planning: {
		manifests: stagePlans.length,
		stagesPlanned: [...new Set(stagePlans.map((plan) => plan.stageId))].sort(),
		missingStageIds: missingStagePlanIds,
		unplannedWorkerTaskIds,
		objectiveMismatchTaskIds,
		forbiddenGenericTaskIds,
	},
	eventLedger: { eventSeq: snapshot.eventSeq, lines: events.length, sequenceContinuous, eventTypes },
	leasePresent: Boolean(snapshot.lease),
	tasks: {
		total: Object.keys(snapshot.tasks).length,
		worker: workerTasks.length,
		repair: Object.values(snapshot.tasks).filter((task) => /repair/i.test(task.objective)).length,
	},
	workspaces: {
		contexts: workerContexts.filter((entry) => entry.context).length,
		missingTaskIds: missingContextTaskIds,
		invalidTaskIds: invalidContextTaskIds,
		upstreamBound: workerContexts.filter((entry) => entry.upstreamBound).length,
		missingUpstreamTaskIds,
	},
	sources: {
		requiredTasks: sourceTaskAudits.length,
		receipts: sourceReceipts.length,
		insufficientTaskIds: insufficientSourceTaskIds,
		missingReceiptRefs: missingSourceReceiptRefs,
		refAudits: sourceRefAudits,
		receiptFailures: [...sourceReceipts, ...frozenSourceReceipts.values()].filter((entry) => !entry.valid)
			.map(({ path, failures }) => ({ path, failures })),
	},
	evidence: { total: Object.keys(snapshot.evidence).length, reviews: Object.keys(snapshot.reviews).length },
	obligations: {
		total: Object.keys(snapshot.obligations).length,
		open: Object.values(snapshot.obligations).filter((obligation) => obligation.status === "open").length,
	},
	canonical: {
		active: activeArtifacts.length,
		retired: retiredArtifacts.length,
		duplicateEvidenceIds: duplicateCanonicalEvidenceIds,
		receiptParity,
		retiredReceiptParity,
		versionParity,
		routeArtifactIds: canonicalRouteArtifactIds,
		discardedCandidates: discardedCandidateReceipts.length,
		discardedEvidence: discardedEvidenceReceipts.length,
		adoptedArtifacts: Object.values(snapshot.canonical).length,
	},
	sessions: backend === "codex" ? {
		files: codexFiles.length,
		failedFiles: codexSessionAudits.filter((entry) => entry.failed).length,
		recoveredFailedFiles: codexSessionAudits.filter((entry) => entry.failed && entry.recovered).length,
		unrecoveredFailedFiles: codexSessionAudits.filter((entry) => entry.failed && !entry.recovered).length,
		models: [...new Set(codexSessionAudits.flatMap((entry) => entry.models))].sort(),
		invalidFiles: codexSessionAudits.filter((entry) => !entry.recovered).map((entry) => entry.path),
		untrackedFiles: untrackedCodexFiles,
		prunedFiles: codexSessionAudits.filter((entry) => entry.pruned).map((entry) => entry.path),
	} : {
		files: sessionFiles.length,
		failedFiles: failedSessions.length,
		recoveredFailedFiles: recoveredFailedSessions.length,
		unrecoveredFailedFiles: unrecoveredFailedSessions.length,
	},
	parentSessions: parentSessionRoot
		? {
				root: resolve(parentSessionRoot),
				files: parentSessionFiles.length,
				matchingFiles: matchingParentSessions.length,
				resultEntries: matchingParentSessions.reduce((total, session) => total + session.resultEntries, 0),
				actions: [...new Set(matchingParentSessions.flatMap((session) => session.actions))].sort(),
				checkpointFiles: parentCheckpointSessions.length,
				compactionFiles: parentCompactionSessions.length,
				compactionEntries: parentCompactionSessions.reduce(
					(total, session) => total + session.compactions.length,
					0,
				),
				checkpointEntries: parentCheckpointSessions.reduce(
					(total, session) => total + session.checkpoints.length,
					0,
				),
				recoveryRequired,
				recoveryCheckpointFiles: recoveryCheckpointSessions.length,
				matches: matchingParentSessions,
			}
		: null,
	migrationReport: existsSync(migrationPath) ? readJson(migrationPath) : null,
	inputFailures,
};
report.runtimeIntegrity.inputFilesValid = inputFailures.length === 0;
report.contractChecks.inputFilesValid = inputFailures.length === 0;
report.passed = Object.values(report.contractChecks).every(Boolean);

console.log(JSON.stringify(report, null, 2));
