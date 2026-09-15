import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore, MemoryAstraStore } from "../src/store.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

for (const disk of [false, true]) {
	it(`recovers persisted events after snapshot failure (${disk ? "disk" : "memory"})`, async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-recovery-"));
		roots.push(root);
		const store = disk ? new JsonlAstraStore(root) : new MemoryAstraStore();
		const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "recovery" });
		vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("snapshot unavailable"));
		await expect(job.consumeTurns(2)).rejects.toThrow("snapshot unavailable");
		const reopened = await ResearchJob.open(store, job.state.frame.jobId);
		expect(reopened?.state.budgetUsage?.turnsUsed).toBe(2);
		await reopened!.reload();
		expect(reopened?.state.budgetUsage?.turnsUsed).toBe(2);
		await reopened!.consumeTurns(1);
		expect((await ResearchJob.open(store, job.state.frame.jobId))?.state.budgetUsage?.turnsUsed).toBe(3);
	});
}

it("does not apply an event when appending it fails", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, { workspaceRoot: "/tmp", objective: "append failure" });
	const before = job.state;
	vi.spyOn(store, "append").mockRejectedValueOnce(new Error("append unavailable"));
	await expect(job.consumeTurns(2)).rejects.toThrow("append unavailable");
	expect(job.state).toEqual(before);
	await job.consumeTurns(1);
	expect((await ResearchJob.open(store, before.frame.jobId))?.state.budgetUsage?.turnsUsed).toBe(1);
});

it("recovers creation when its first snapshot was never written", async () => {
	const store = new MemoryAstraStore();
	vi.spyOn(store, "writeSnapshot").mockRejectedValueOnce(new Error("snapshot unavailable"));
	await expect(
		ResearchJob.create(store, {
			jobId: "job_recover_creation",
			workspaceRoot: "/tmp",
			objective: "recover creation",
		}),
	).rejects.toThrow("snapshot unavailable");
	const job = await ResearchJob.open(store, "job_recover_creation");
	expect(job?.state.frame.objective).toBe("recover creation");
	expect(job?.state.eventSeq).toBe(1);
});

it("rejects an event sequence gap instead of silently skipping state", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, { workspaceRoot: "/tmp", objective: "sequence gap" });
	const events = await store.readEvents(job.state.frame.jobId);
	vi.spyOn(store, "readEvents").mockResolvedValue([
		...events,
		{
			seq: 3,
			jobId: job.state.frame.jobId,
			timestamp: new Date().toISOString(),
			event: { type: "budget_usage_recorded", turns: 2, costUsd: 0 },
		},
	]);
	await expect(ResearchJob.open(store, job.state.frame.jobId)).rejects.toThrow(/sequence/);
});
