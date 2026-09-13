import { createHash } from "node:crypto";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import type {
	MainAgentDecisionManifest,
	ReviewerOutputManifest,
	ReviewPacket,
	ReviewTrace,
	StagePlanManifest,
	TaskPacket,
	WorkerOutputManifest,
} from "./types.ts";

const ID_PATTERN = /^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$/;

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

export async function readJson<T>(path: string): Promise<T> {
	return JSON.parse(await readFile(path, "utf8")) as T;
}

export function sha256(value: string): string {
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
