import { createHash } from "node:crypto";
import type { Stats } from "node:fs";
import { link, lstat, mkdir, open, readFile, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import type {
	MainAgentCall,
	MainAgentDecisionManifest,
	ReviewerOutputManifest,
	ReviewPacket,
	ReviewTrace,
	StageDefinition,
	StagePlanManifest,
	TaskPacket,
	WorkerOutputManifest,
} from "./types.ts";

const ID_PATTERN = /^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$/;

/** Local work is reviewed against its own contract; stage policy still supplies budgets and review thresholds. */
export const TASK_DELIVERY_INSTRUCTIONS =
	"Use deliveryKind=stage for a complete capability result, local for a bounded subtask with its own fields and checks, and synthesis for one complete capability result combining all accepted local evidence IDs. Local tasks are independently reviewed against their task contract, which overrides full-stage field requirements. Never mix local and complete deliveries in one plan. For a stage-plan review, audit the proposed plan against its frozen planning criteria, not the unexecuted scientific results. The planner must address prior plan-review findings before submitting a new plan. Search candidates must use stage. Repairs preserve the failed task's delivery kind, fields, checks and inputs. Once local work is accepted, plan one synthesis after all local reviews and repairs finish; synthesis must satisfy the full capability contract and is independently reviewed before adoption.";

export function taskStageContract(definition: StageDefinition, task: TaskPacket): StageDefinition {
	return task.deliveryKind === "local" || task.requiredOutputType === "stage-plan"
		? {
				...definition,
				outputArtifactType: task.requiredOutputType,
				requiredOutputFields: task.requiredOutputFields,
				acceptanceChecks: task.acceptanceChecks,
				failureSignals: task.failureSignals,
				minSourceRefs: 0,
			}
		: definition;
}

export function assertAstraId(value: string, label: string): void {
	if (!ID_PATTERN.test(value)) throw new Error(`${label} contains unsafe characters: ${value}`);
}

function safeFileSegment(value: string, label: string): string {
	if (!value || value.includes("/") || value.includes("\\") || value.includes(".."))
		throw new Error(`${label} contains unsafe path characters`);
	return value.replace(/[^A-Za-z0-9._-]/g, "_");
}

export function taskDir(workspaceRoot: string, jobId: string, taskId: string): string {
	assertAstraId(jobId, "job id");
	assertAstraId(taskId, "task id");
	return join(resolve(workspaceRoot), ".astra", "jobs", jobId, "tasks", taskId);
}

export function taskPacketPath(workspaceRoot: string, jobId: string, taskId: string): string {
	return join(taskDir(workspaceRoot, jobId, taskId), "task-packet.json");
}

export function workerManifestPath(workspaceRoot: string, jobId: string, taskId: string): string {
	return join(taskDir(workspaceRoot, jobId, taskId), "output-manifest.json");
}

export function reviewerManifestPath(workspaceRoot: string, jobId: string, taskId: string): string {
	return join(taskDir(workspaceRoot, jobId, taskId), "review-manifest.json");
}

export function reviewPacketPath(workspaceRoot: string, jobId: string, taskId: string): string {
	return join(taskDir(workspaceRoot, jobId, taskId), "review-packet.json");
}

export function reviewTracePath(workspaceRoot: string, jobId: string, taskId: string): string {
	return join(taskDir(workspaceRoot, jobId, taskId), "review-trace.json");
}

export function reviewSnapshotPath(workspaceRoot: string, jobId: string, taskId: string): string {
	return join(taskDir(workspaceRoot, jobId, taskId), "review-target-snapshot.json");
}

export function canonicalArtifactPath(workspaceRoot: string, jobId: string, artifactId: string): string {
	assertAstraId(jobId, "job id");
	assertAstraId(artifactId, "artifact id");
	return join(resolve(workspaceRoot), ".astra", "jobs", jobId, "canonical", `${artifactId}.json`);
}

export function canonicalReceiptPath(workspaceRoot: string, jobId: string, artifactId: string): string {
	return `${canonicalArtifactPath(workspaceRoot, jobId, artifactId)}.receipt.json`;
}

export function mainDecisionManifestPath(
	workspaceRoot: string,
	jobId: string,
	decisionType: string,
	decisionRef?: string,
): string {
	assertAstraId(jobId, "job id");
	assertAstraId(decisionType, "decision type");
	const fileRef = decisionRef ? safeFileSegment(decisionRef, "decision ref") : undefined;
	return join(
		resolve(workspaceRoot),
		".astra",
		"jobs",
		jobId,
		"main-agent",
		"decisions",
		fileRef ? `${fileRef}.json` : `${decisionType}-latest.json`,
	);
}

export function stagePlanManifestPath(workspaceRoot: string, jobId: string, planId: string): string {
	assertAstraId(jobId, "job id");
	assertAstraId(planId, "stage plan id");
	return join(resolve(workspaceRoot), ".astra", "jobs", jobId, "main-agent", "plans", `${planId}.json`);
}

export async function atomicWriteJson(path: string, value: unknown): Promise<void> {
	await mkdir(dirname(path), { recursive: true });
	const temp = `${path}.tmp-${process.pid}-${Math.random().toString(36).slice(2)}`;
	await writeFile(temp, `${JSON.stringify(value, null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
	await rename(temp, path);
}

/** Publish complete content once; existing shared bytes must never be replaced. */
export async function publishImmutableFile(root: string, content: Buffer | string, mode = 0o444): Promise<string> {
	root = resolve(root);
	await mkdir(root, { recursive: true });
	for (let parent = root; ; parent = dirname(parent)) {
		const metadata = await lstat(parent);
		if (!metadata.isDirectory() || metadata.isSymbolicLink())
			throw new Error(`immutable content directory is unsafe: ${parent}`);
		if (parent === dirname(parent)) break;
	}
	const hash = createHash("sha256").update(content).digest("hex");
	const destination = join(root, hash);
	const exists = async (): Promise<boolean> => {
		let metadata: Stats;
		try {
			metadata = await lstat(destination);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
			throw error;
		}
		if (
			!metadata.isFile() ||
			metadata.isSymbolicLink() ||
			createHash("sha256")
				.update(await readFile(destination))
				.digest("hex") !== hash
		)
			throw new Error(`immutable content integrity failure: ${destination}`);
		return true;
	};
	if (await exists()) return hash;
	const temp = `${destination}.tmp-${process.pid}-${Math.random().toString(36).slice(2)}`;
	const handle = await open(temp, "wx", mode);
	try {
		await handle.writeFile(content);
		try {
			await link(temp, destination);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
			if (!(await exists())) throw new Error(`immutable content disappeared during publication: ${destination}`);
		}
	} finally {
		await handle.close();
		await rm(temp, { force: true });
	}
	return hash;
}

export async function readJson<T>(path: string): Promise<T> {
	return JSON.parse(await readFile(path, "utf8")) as T;
}

export function sha256(value: string | Buffer): string {
	return createHash("sha256").update(value).digest("hex");
}

export async function writeTaskPacket(packet: TaskPacket): Promise<string> {
	const path = taskPacketPath(packet.scope.workspaceRoot, packet.jobId, packet.id);
	await atomicWriteJson(path, packet);
	return path;
}

export async function writeWorkerOutputManifest(
	manifest: WorkerOutputManifest,
	workspaceRoot: string,
): Promise<string> {
	const path = workerManifestPath(workspaceRoot, manifest.jobId, manifest.taskId);
	await atomicWriteJson(path, manifest);
	return path;
}

export async function writeReviewerOutputManifest(
	manifest: ReviewerOutputManifest,
	workspaceRoot: string,
): Promise<string> {
	const path = reviewerManifestPath(workspaceRoot, manifest.jobId, manifest.taskId);
	await atomicWriteJson(path, manifest);
	return path;
}

export async function writeReviewPacket(packet: ReviewPacket, workspaceRoot: string): Promise<string> {
	const path = reviewPacketPath(workspaceRoot, packet.jobId, packet.taskId);
	await atomicWriteJson(path, packet);
	return path;
}

export async function writeReviewSnapshot(
	snapshot: unknown,
	workspaceRoot: string,
	jobId: string,
	taskId: string,
): Promise<string> {
	const path = reviewSnapshotPath(workspaceRoot, jobId, taskId);
	await atomicWriteJson(path, snapshot);
	return path;
}

export async function writeReviewTrace(trace: ReviewTrace, workspaceRoot: string): Promise<string> {
	const path = reviewTracePath(workspaceRoot, trace.jobId, trace.taskId);
	await atomicWriteJson(path, trace);
	return path;
}

export async function writeMainDecisionManifest(
	manifest: MainAgentDecisionManifest,
	workspaceRoot: string,
): Promise<string> {
	const historyPath = mainDecisionManifestPath(
		workspaceRoot,
		manifest.jobId,
		manifest.decisionType,
		manifest.decisionRef,
	);
	const latestPath = mainDecisionManifestPath(workspaceRoot, manifest.jobId, manifest.decisionType);
	await atomicWriteJson(historyPath, manifest);
	await atomicWriteJson(latestPath, manifest);
	return historyPath;
}

export async function writeStagePlanManifest(manifest: StagePlanManifest, workspaceRoot: string): Promise<string> {
	const path = stagePlanManifestPath(workspaceRoot, manifest.jobId, manifest.id);
	await atomicWriteJson(path, manifest);
	return path;
}

export async function readWorkerOutputManifest(path: string): Promise<WorkerOutputManifest> {
	const manifest = await readJson<WorkerOutputManifest>(path);
	if (manifest.schemaVersion !== "astra.worker_output_manifest.v1")
		throw new Error("unsupported worker output manifest schema");
	if (manifest.status !== "completed" || manifest.validationStatus !== "passed")
		throw new Error("worker output manifest is not a passed completion");
	if (manifest.outputRefs.length === 0) throw new Error("worker output manifest must include output refs");
	return manifest;
}

export async function readReviewerOutputManifest(path: string): Promise<ReviewerOutputManifest> {
	const manifest = await readJson<ReviewerOutputManifest>(path);
	if (manifest.schemaVersion !== "astra.reviewer_output_manifest.v1")
		throw new Error("unsupported reviewer output manifest schema");
	if (["pass", "fail", "partial", "blocked"].includes(manifest.verdict) === false)
		throw new Error("invalid reviewer verdict");
	if (!Number.isFinite(manifest.score) || manifest.score < 0 || manifest.score > 1) {
		throw new Error("reviewer score must be between zero and one");
	}
	if (manifest.criteria.length === 0) throw new Error("reviewer manifest requires criterion-level assessments");
	if (manifest.verifiedRefs.length === 0) throw new Error("reviewer manifest requires verified refs");
	return manifest;
}

export async function readTaskPacket(path: string): Promise<TaskPacket> {
	const packet = await readJson<TaskPacket>(path);
	if (packet.schemaVersion !== "astra.task_packet.v1") throw new Error("unsupported TaskPacket schema");
	assertAstraId(packet.jobId, "job id");
	assertAstraId(packet.id, "task id");
	if (resolve(path) !== resolve(taskPacketPath(packet.scope.workspaceRoot, packet.jobId, packet.id))) {
		throw new Error("TaskPacket path does not match its durable identity");
	}
	if (packet.outputManifestRequired !== true) throw new Error("TaskPacket must require an output manifest");
	if (packet.scope.allowedPaths.length === 0) throw new Error("TaskPacket allowed paths cannot be empty");
	if (packet.requiredOutputFields.length === 0) throw new Error("TaskPacket required output fields cannot be empty");
	if (packet.acceptanceChecks.length === 0) throw new Error("TaskPacket acceptance checks cannot be empty");
	if (packet.failureSignals.length === 0) throw new Error("TaskPacket failure signals cannot be empty");
	return packet;
}

export async function readStagePlanManifest(path: string): Promise<StagePlanManifest> {
	const manifest = await readJson<StagePlanManifest>(path);
	if (manifest.schemaVersion !== "astra.stage_plan_manifest.v1") {
		throw new Error("unsupported stage plan manifest schema");
	}
	assertAstraId(manifest.id, "stage plan id");
	assertAstraId(manifest.jobId, "job id");
	assertAstraId(manifest.stageId, "stage id");
	const maxTasks = manifest.mode === "search" ? 4 : 2;
	if (manifest.tasks.length === 0 || manifest.tasks.length > maxTasks) {
		throw new Error(`stage plan must contain one to ${maxTasks} tasks`);
	}
	if (manifest.mode === "search" && manifest.tasks.length < 2) {
		throw new Error("search stage plan must contain at least two candidates");
	}
	if (manifest.obligationId && manifest.tasks.length !== 1) {
		throw new Error("repair stage plan must contain exactly one task");
	}
	for (const task of manifest.tasks) {
		assertAstraId(task.key, "planned task key");
		if (!task.objective.trim()) throw new Error("planned task objective cannot be empty");
		if (task.requiredOutputFields.length === 0) throw new Error("planned task requires output fields");
		if (task.acceptanceChecks.length === 0) throw new Error("planned task requires acceptance checks");
	}
	return manifest;
}

export type ChildManifest =
	| WorkerOutputManifest
	| ReviewerOutputManifest
	| MainAgentDecisionManifest
	| StagePlanManifest;

/** Read only the registered historical path, rejecting symlinks in it or its parents. */
export async function readMainAgentDelivery(
	root: string,
	call: MainAgentCall,
): Promise<StagePlanManifest | MainAgentDecisionManifest> {
	const expected =
		call.type === "plan"
			? stagePlanManifestPath(root, call.jobId, call.planId!)
			: mainDecisionManifestPath(root, call.jobId, call.type, call.id);
	if (resolve(call.manifestRef) !== expected)
		throw new Error("main-agent delivery path does not match registered identity");
	let path = resolve(root);
	for (const part of relative(path, expected).split(sep)) {
		path = join(path, part);
		if ((await lstat(path)).isSymbolicLink()) throw new Error(`main-agent delivery symlink rejected: ${path}`);
	}
	const manifest =
		call.type === "plan"
			? await readStagePlanManifest(expected)
			: await readJson<MainAgentDecisionManifest>(expected);
	if (
		manifest.jobId !== call.jobId ||
		manifest.decisionRef !== call.id ||
		((call.type === "plan" || call.type === "route" || manifest.stageId !== undefined) &&
			manifest.stageId !== call.stageId) ||
		typeof manifest.rationale !== "string" ||
		!manifest.rationale.trim() ||
		typeof manifest.sessionRef !== "string" ||
		!manifest.sessionRef ||
		typeof manifest.createdAt !== "string" ||
		!Number.isFinite(Date.parse(manifest.createdAt))
	)
		throw new Error("main-agent delivery common identity/schema mismatch");
	if (call.type === "plan") {
		if (!("tasks" in manifest)) throw new Error("main-agent plan schema mismatch");
		if (manifest.id !== call.planId || manifest.mode !== call.mode || manifest.obligationId !== call.obligationId)
			throw new Error("main-agent plan id/mode/obligation mismatch");
		for (const task of manifest.tasks)
			for (const field of [
				task.inputArtifactRefs,
				task.requiredOutputFields,
				task.acceptanceChecks,
				task.failureSignals,
				task.successCriteria,
			])
				if (!Array.isArray(field) || field.some((value) => typeof value !== "string"))
					throw new Error("main-agent plan task schema mismatch");
	} else {
		if ("tasks" in manifest) throw new Error("main-agent decision cannot contain plan tasks");
		if (
			manifest.schemaVersion !== "astra.main_agent_decision_manifest.v1" ||
			manifest.manifestId !== call.manifestId ||
			manifest.decisionType !== call.type ||
			manifest.evidenceId !== call.evidenceId ||
			manifest.searchBatchId !== call.searchBatchId
		)
			throw new Error("main-agent decision identity/schema mismatch");
		if (call.type === "evidence" && !["accept", "reject", "defer"].includes(manifest.decision ?? ""))
			throw new Error("invalid main-agent evidence decision");
		if (
			call.type === "adoption" &&
			(typeof manifest.adopt !== "boolean" ||
				(manifest.replacementOf !== undefined && typeof manifest.replacementOf !== "string"))
		)
			throw new Error("invalid main-agent adoption decision");
		if (
			call.type === "search-selection" &&
			(Boolean(manifest.selectedCandidateId) === Boolean(manifest.continueSearch) ||
				(manifest.selectedCandidateId !== undefined && typeof manifest.selectedCandidateId !== "string") ||
				(manifest.continueSearch !== undefined && typeof manifest.continueSearch !== "boolean"))
		)
			throw new Error("invalid main-agent search decision");
		if (
			call.type === "route" &&
			(!["continue", "search", "advance", "backtrack", "ask-user", "complete"].includes(
				manifest.routeAction ?? "",
			) ||
				(manifest.evidenceRefs !== undefined &&
					(!Array.isArray(manifest.evidenceRefs) ||
						manifest.evidenceRefs.some((ref) => typeof ref !== "string"))) ||
				(manifest.newQuestions !== undefined &&
					(!Array.isArray(manifest.newQuestions) ||
						manifest.newQuestions.some((question) => typeof question !== "string"))) ||
				(manifest.question !== undefined && typeof manifest.question !== "string") ||
				(manifest.targetStageId !== undefined && typeof manifest.targetStageId !== "string"))
		)
			throw new Error("invalid main-agent route decision");
	}
	return manifest;
}
