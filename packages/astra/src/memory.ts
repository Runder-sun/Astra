import { appendFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
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

function memoryRoot(workspaceRoot: string, jobId: string): string {
	return join(workspaceRoot, ".astra", "jobs", jobId, "memory");
}

export async function appendJobMemory(
	workspaceRoot: string,
	jobId: string,
	entry: Omit<AstraMemoryEntry, "schemaVersion" | "id" | "scope" | "jobId" | "createdAt">,
): Promise<string> {
	const id = `memory_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
	const value: AstraMemoryEntry = {
		schemaVersion: "astra.memory_entry.v1",
		id,
		scope: "job",
		jobId,
		...entry,
		createdAt: new Date().toISOString(),
	};
	const root = memoryRoot(workspaceRoot, jobId);
	await mkdir(root, { recursive: true });
	await appendFile(join(root, "entries.jsonl"), `${JSON.stringify(value)}\n`, "utf8");
	return id;
}

export async function readJobMemory(workspaceRoot: string, jobId: string, limit = 24): Promise<AstraMemoryEntry[]> {
	try {
		const content = await readFile(join(memoryRoot(workspaceRoot, jobId), "entries.jsonl"), "utf8");
		return content
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line) as AstraMemoryEntry)
			.slice(-limit);
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
		throw error;
	}
}

export async function loadStageSkills(workspaceRoot: string, stageId: string, role: Role): Promise<string[]> {
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
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
	}
	return loaded;
}
