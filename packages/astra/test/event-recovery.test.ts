import { fork } from "node:child_process";
import { appendFile, mkdtemp, open, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore, MemoryAstraStore, ResearchJobLockedError } from "../src/store.ts";

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

it("checks memory journal currency without cloning the whole journal on every commit", async () => {
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, { workspaceRoot: "/tmp", objective: "bounded sequence reads" });
	const read = vi.spyOn(store, "readEvents");
	for (let index = 0; index < 30; index++) await job.consumeTurns(1);
	expect(read).not.toHaveBeenCalled();
	expect((await ResearchJob.open(store, job.state.frame.jobId))?.state.budgetUsage?.turnsUsed).toBe(30);
});

it("rejects competing real processes opened at the same journal sequence", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-writer-process-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "concurrent writers" });
	const children = [0, 1].map(() =>
		fork(fileURLToPath(new URL("./fixtures/journal-writer.mjs", import.meta.url)), [root, job.state.frame.jobId], {
			execArgv: ["--experimental-strip-types"],
			stdio: ["ignore", "ignore", "pipe", "ipc"],
		}),
	);
	try {
		await Promise.all(
			children.map(
				(child) =>
					new Promise<void>((resolve, reject) => {
						child.once("message", () => resolve());
						child.once("error", reject);
						child.once("exit", () => reject(new Error("writer exited before barrier")));
					}),
			),
		);
		const results = children.map(
			(child) =>
				new Promise<{ committed: boolean }>((resolve, reject) => {
					child.once("message", (message) => resolve(message as { committed: boolean }));
					child.once("error", reject);
				}),
		);
		for (const child of children) child.send("go");
		expect((await Promise.all(results)).filter((result) => result.committed)).toHaveLength(1);
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(reopened.state.budgetUsage?.turnsUsed).toBe(1);
		const events = await store.readEvents(job.state.frame.jobId);
		expect(events.map((event) => event.seq)).toEqual(events.map((_, index) => index + 1));
		expect((await store.loadSnapshot(job.state.frame.jobId))?.eventSeq).toBe(events.length);
	} finally {
		for (const child of children) if (child.exitCode === null) child.kill("SIGKILL");
	}
});

async function tornJournal() {
	const root = await mkdtemp(join(tmpdir(), "astra-tail-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "中文恢复" });
	const id = job.state.frame.jobId;
	const path = join(root, ".astra", "jobs", id, "events.jsonl");
	return { root, store, job, id, path, prefix: await readFile(path) };
}

it.each(['{"seq":2,"event":{"text":"中文', '{"seq":2,"event":{', '{"seq":2,"event":{"text":"\\u12'])(
	"F02-01/02 reads incomplete tail %s without mutation then repairs under the write lock",
	async (tail) => {
		const { store, job, id, path, prefix } = await tornJournal();
		await appendFile(path, tail);
		const before = await readFile(path);
		for (let i = 0; i < 2; i++) {
			expect(await store.readEvents(id)).toHaveLength(1);
			expect((await ResearchJob.open(store, id))?.state.eventSeq).toBe(1);
			expect(await readFile(path)).toEqual(before);
		}
		await job.consumeTurns(1);
		await job.consumeTurns(2);
		const reopened = (await ResearchJob.open(store, id))!;
		await reopened.reload();
		expect(reopened.state.budgetUsage?.turnsUsed).toBe(3);
		expect((await store.readEvents(id)).map((event) => event.seq)).toEqual([1, 2, 3]);
		expect((await readFile(path)).subarray(0, prefix.length)).toEqual(prefix);
	},
);

it("F02-01/02 does not fabricate a job from a partial creation and permits a new locked creation", async () => {
	const { store, id, path } = await tornJournal();
	await rm(join(path, "..", "job.json"));
	await writeFile(path, '{"seq":1,"event":{"text":"中文');
	expect(await ResearchJob.open(store, id)).toBeUndefined();
	const job = await ResearchJob.create(store, { jobId: id, workspaceRoot: "/tmp", objective: "new creation" });
	expect(job.state.eventSeq).toBe(1);
	expect((await store.readEvents(id))[0].event.type).toBe("job_created");
});

it("F02-03 retains a complete event without newline and adds only the separator before committing", async () => {
	const { store, job, id, path, prefix } = await tornJournal();
	await writeFile(path, prefix.subarray(0, prefix.length - 1));
	expect(await store.readEvents(id)).toHaveLength(1);
	await store.withWriteLock(id, async () => expect(await readFile(path)).toEqual(prefix));
	await job.consumeTurns(1);
	expect((await store.readEvents(id)).map((event) => event.seq)).toEqual([1, 2]);
	expect((await readFile(path)).subarray(0, prefix.length)).toEqual(prefix);
});

it.each([
	'{"seq":}\n',
	'{"seq":}',
	'{"text":"\\q"}',
	"{}garbage",
	'{"text":"\\uZZZZ',
	'{"seq":3}',
	'{"seq":2,"jobId":"other"}',
	'{"seq":3}\n{}\n',
])("F02-04 rejects complete corruption %s without mutating bytes", async (tail) => {
	const { store, id, path } = await tornJournal();
	await appendFile(path, tail);
	const before = await readFile(path);
	await expect(store.readEvents(id)).rejects.toThrow();
	await expect(
		store.withWriteLock(id, () => store.append(id, { type: "budget_usage_recorded", turns: 1, costUsd: 0 })),
	).rejects.toThrow();
	expect(await readFile(path)).toEqual(before);
});

it("F02-05 refuses direct concatenation and refuses repairing a tail while another store owns the journal", async () => {
	const { root, store, id, path } = await tornJournal();
	const other = new JsonlAstraStore(root);
	const event = { type: "budget_usage_recorded", turns: 1, costUsd: 0 } as const;
	await store.append(id, event);
	await appendFile(path, '{"seq":3');
	const before = await readFile(path);
	await expect(store.append(id, event)).rejects.toThrow(/withWriteLock/);
	await store.withWriteLock(id, async () => {
		await appendFile(path, '{"seq":3');
		const held = await readFile(path);
		await expect(other.withWriteLock(id, async () => undefined)).rejects.toBeInstanceOf(ResearchJobLockedError);
		expect(await readFile(path)).toEqual(held);
	});
	expect((await readFile(path)).length).toBe(before.length);
	await store.withWriteLock(id, () => store.append(id, event));
	expect((await store.readEvents(id)).map((record) => record.seq)).toEqual([1, 2, 3]);
});

it("F02-06 checks only the last byte of a normal journal before running the operation", async () => {
	const { store, id, path, prefix } = await tornJournal();
	await writeFile(path, Buffer.concat(Array.from({ length: 1000 }, () => prefix)));
	const handle = await open(path, "r");
	const prototype = Object.getPrototypeOf(handle) as typeof handle;
	await handle.close();
	const read = vi.spyOn(prototype, "read");
	const fullRead = vi.spyOn(prototype, "readFile");
	await store.withWriteLock(id, async () => undefined);
	expect(read).toHaveBeenCalledTimes(1);
	expect(read.mock.calls[0].slice(1)).toEqual([0, 1, prefix.length * 1000 - 1]);
	expect(fullRead).not.toHaveBeenCalled();
});

it("F02-07 rejects a snapshot ahead of the committed prefix even when the tail is recoverable", async () => {
	const { store, job, id, path } = await tornJournal();
	await job.consumeTurns(1);
	const [creation] = (await readFile(path, "utf8")).split("\n");
	await writeFile(path, `${creation}\n{"seq":2`);
	await expect(ResearchJob.open(store, id)).rejects.toThrow(/ahead/);
});

it("F02-EOF-01 accepts every valid truncation without discarding a complete event", async () => {
	const { store, id, path, prefix } = await tornJournal();
	const text = JSON.stringify({
		seq: 2,
		jobId: id,
		timestamp: new Date().toISOString(),
		event: {
			type: "fixture",
			nested: [{ yes: true, no: false, nil: null }, -3, 0, 1.25, 1e30, 'quote" slash\\中文'],
		},
	}).replace("中文", "\\u4e2d\\u6587");
	for (let position = 1; position < text.length; position++) {
		const bytes = Buffer.concat([prefix, Buffer.from(text.slice(0, position))]);
		await writeFile(path, bytes);
		expect(await store.readEvents(id), `cut ${position}`).toHaveLength(1);
		expect(await readFile(path)).toEqual(bytes);
	}
	await writeFile(path, Buffer.concat([prefix, Buffer.from(text)]));
	expect(await store.readEvents(id)).toHaveLength(2);
	await store.withWriteLock(id, async () => undefined);
	expect(await readFile(path)).toEqual(Buffer.concat([prefix, Buffer.from(`${text}\n`)]));
}, 30000);

it.each([
	'{"x":"\\q',
	'{"x":"\\uG',
	'{"x":"a\t',
	'{"x" "unfinished',
	'{"x":1 "unfinished',
	'{"x":1,}',
	"[1,]",
	'{"x":[1}',
	'{"x":01,"tail":"',
	'{"x":1.,"tail":"',
	'{"x":1e+,"tail":"',
	'{"x":truX,"tail":"',
	'{} {"tail":"',
	"{}garbage",
	'{"x":falsee',
	'{"x":+1',
	'{"x":.1',
])("F02-EOF-02 rejects invalid prefix %s even before a later unfinished value", async (tail) => {
	const { store, id, path, prefix } = await tornJournal();
	const bytes = Buffer.concat([prefix, Buffer.from(tail)]);
	await writeFile(path, bytes);
	await expect(store.readEvents(id)).rejects.toThrow();
	await expect(store.withWriteLock(id, async () => undefined)).rejects.toThrow();
	await expect(store.append(id, { type: "budget_usage_recorded", turns: 1, costUsd: 0 })).rejects.toThrow();
	expect(await readFile(path)).toEqual(bytes);
});

it("F02-EOF-03 truncates an unfinished multibyte character at the original prefix byte boundary", async () => {
	const { store, id, path, prefix } = await tornJournal();
	const tail = Buffer.from('{"seq":2,"text":"中文');
	await writeFile(path, Buffer.concat([prefix, tail.subarray(0, tail.length - 1)]));
	expect(await store.readEvents(id)).toHaveLength(1);
	await store.withWriteLock(id, async () => undefined);
	expect(await readFile(path)).toEqual(prefix);
});

it("F02-EOF-04 uses the same prefix classifier for read, repair and append, skipping it for newline journals", async () => {
	const { store, id, path } = await tornJournal();
	const classifier = vi.spyOn(store as unknown as { isIncompleteJson(text: string): boolean }, "isIncompleteJson");
	await store.readEvents(id);
	await store.withWriteLock(id, async () => undefined);
	expect(classifier).not.toHaveBeenCalled();
	await appendFile(path, '{"seq":2');
	await store.readEvents(id);
	await expect(store.append(id, { type: "budget_usage_recorded", turns: 1, costUsd: 0 })).rejects.toThrow(
		/withWriteLock/,
	);
	await store.withWriteLock(id, async () => undefined);
	expect(classifier.mock.results.map((result) => result.value)).toEqual([true, true, true]);
});
