import { randomUUID } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { atomicWriteJson } from "./contracts.ts";
import type { Role } from "./types.ts";

export interface AstraMemoryEntry {
	schemaVersion: "astra.memory_entry.v1";
	id: string;
	scope: "job" | "project";
	jobId?: string;
	stageId?: string;
	role?: Role;
	kind: "checkpoint" | "decision" | "failure-lesson" | "evidence-index" | "note";
	content: string;
	sourceRefs: string[];
	createdAt: string;
}

export interface AuxiliaryDiagnostic {
	path: string;
	line?: number;
	reason: string;
}

function reportDiagnostic(
	report: ((diagnostic: AuxiliaryDiagnostic) => void) | undefined,
	path: string,
	error: unknown,
	line?: number,
): void {
	try {
		report?.({ path, line, reason: error instanceof Error ? error.message : String(error) });
	} catch {
		// Auxiliary diagnostics must not discard healthy context or checkpoints.
	}
}

function validMemory(value: unknown, jobId: string): value is AstraMemoryEntry {
	if (!value || typeof value !== "object" || Array.isArray(value)) return false;
	const entry = value as Record<string, unknown>;
	return (
		entry.schemaVersion === "astra.memory_entry.v1" &&
		typeof entry.id === "string" &&
		entry.id.length > 0 &&
		(entry.scope === "job" ? entry.jobId === jobId : entry.scope === "project" && entry.jobId === undefined) &&
		typeof entry.kind === "string" &&
		["checkpoint", "decision", "failure-lesson", "evidence-index", "note"].includes(entry.kind) &&
		typeof entry.content === "string" &&
		entry.content.trim().length > 0 &&
		Array.isArray(entry.sourceRefs) &&
		entry.sourceRefs.every((ref) => typeof ref === "string" && ref.trim().length > 0) &&
		typeof entry.createdAt === "string" &&
		Number.isFinite(Date.parse(entry.createdAt)) &&
		(entry.stageId === undefined || typeof entry.stageId === "string") &&
		(entry.role === undefined ||
			(typeof entry.role === "string" && ["main-agent", "worker", "reviewer", "supervisor"].includes(entry.role)))
	);
}

function memoryRoot(workspaceRoot: string, jobId: string): string {
	return join(workspaceRoot, ".astra", "jobs", jobId, "memory");
}

export async function appendJobMemory(
	workspaceRoot: string,
	jobId: string,
	entry: Omit<AstraMemoryEntry, "schemaVersion" | "id" | "scope" | "jobId" | "createdAt">,
): Promise<string> {
	const id = `memory_${randomUUID()}`;
	const value: AstraMemoryEntry = {
		schemaVersion: "astra.memory_entry.v1",
		id,
		scope: "job",
		jobId,
		...entry,
		createdAt: new Date().toISOString(),
	};
	const root = memoryRoot(workspaceRoot, jobId);
	if (!validMemory(value, jobId)) throw new Error("invalid Astra memory entry");
	await atomicWriteJson(join(root, `${id}.json`), value);
	return id;
}

export async function readJobMemory(
	workspaceRoot: string,
	jobId: string,
	limit = 24,
	report?: (diagnostic: AuxiliaryDiagnostic) => void,
): Promise<AstraMemoryEntry[]> {
	const root = memoryRoot(workspaceRoot, jobId);
	const entries = new Map<string, AstraMemoryEntry>();
	const parse = (bytes: string, path: string, line?: number) => {
		try {
			const value: unknown = JSON.parse(bytes);
			if (!validMemory(value, jobId)) throw new Error("invalid Astra memory entry schema or job binding");
			if (entries.has(value.id)) throw new Error(`duplicate Astra memory id: ${value.id}`);
			entries.set(value.id, value);
		} catch (error) {
			reportDiagnostic(report, path, error, line);
		}
	};
	const legacy = join(root, "entries.jsonl");
	try {
		for (const [index, line] of (await readFile(legacy, "utf8")).split("\n").entries())
			if (line.trim()) parse(line, legacy, index + 1);
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") reportDiagnostic(report, legacy, error);
	}
	let names: string[] = [];
	try {
		names = await readdir(root);
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") reportDiagnostic(report, root, error);
	}
	for (const name of names.sort().filter((name) => name.endsWith(".json"))) {
		const path = join(root, name);
		try {
			parse(await readFile(path, "utf8"), path);
		} catch (error) {
			reportDiagnostic(report, path, error);
		}
	}
	return limit <= 0
		? []
		: [...entries.values()]
				.sort(
					(left, right) =>
						Date.parse(left.createdAt) - Date.parse(right.createdAt) || left.id.localeCompare(right.id),
				)
				.slice(-limit);
}

export async function loadStageSkills(
	workspaceRoot: string,
	stageId: string,
	role: Role,
	report?: (diagnostic: AuxiliaryDiagnostic) => void,
): Promise<string[]> {
	const packageSkillsRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "skills", "astra");
	const paths = [
		join(workspaceRoot, ".pi", "skills", "astra", `${stageId}.md`),
		join(workspaceRoot, ".pi", "skills", "astra", `${role}.md`),
		join(workspaceRoot, ".pi", "skills", "astra", `${stageId}.${role}.md`),
		join(packageSkillsRoot, `${stageId}.md`),
		join(packageSkillsRoot, `${role}.md`),
	];
	const loaded: string[] = [];
	for (const path of paths) {
		try {
			loaded.push(await readFile(path, "utf8"));
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") reportDiagnostic(report, path, error);
		}
	}
	return loaded;
}
