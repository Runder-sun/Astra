import { randomUUID } from "node:crypto";
import { readdir, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { assertAstraId, atomicWriteJson } from "./contracts.ts";
import type { ResearchJob } from "./research.ts";

export async function requestResearchPause(root: string, jobId: string, reason: string): Promise<void> {
	assertAstraId(jobId, "job id");
	await atomicWriteJson(join(root, ".astra/jobs", jobId, "pause-requests", `${randomUUID()}.json`), { reason });
}

/** Called only by the holder of the job lock; acknowledge after the pause event is durable. */
export async function applyPendingPauses(job: ResearchJob): Promise<void> {
	const state = job.state;
	const root = join(state.frame.permissions.workspaceRoot, ".astra/jobs", state.frame.jobId, "pause-requests");
	let files: string[];
	try {
		files = await readdir(root);
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
		throw error;
	}
	for (const file of files.filter((name) => name.endsWith(".json")).sort()) {
		const path = join(root, file);
		const request = JSON.parse(await readFile(path, "utf8")) as { reason?: unknown };
		if (typeof request.reason !== "string" || !request.reason.trim()) throw new Error("invalid pause request");
		await job.pause(request.reason);
		await rm(path);
	}
}
