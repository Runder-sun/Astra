import { randomUUID } from "node:crypto";
import { mkdir, mkdtemp, readdir, readFile, rm, rmdir, symlink, unlink, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore, ResearchJobLockedError } from "../src/store.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function setup(supervisor = false) {
	const root = await mkdtemp(join(tmpdir(), "astra-journal-lock-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "safe journal locks" });
	const id = job.state.frame.jobId;
	return {
		root,
		store,
		job,
		id,
		path: join(root, ".astra", "jobs", id, supervisor ? "supervisor.lock.d" : "journal.lock"),
	};
}
async function owner(path: string, pid = 2147483646) {
	const token = randomUUID();
	await mkdir(path);
	await writeFile(
		join(path, `owner-${token}.json`),
		JSON.stringify({ owner: "fixture", pid, token, createdAt: new Date(0).toISOString() }),
	);
	return token;
}

it.each([false, true])(
	"F01-01 reclaiming a dead owner cannot remove a newer live lock (supervisor=%s)",
	async (supervisor) => {
		const { root, store: first, id, path } = await setup(supervisor);
		const second = new JsonlAstraStore(root);
		const acquire = (store: JsonlAstraStore, operation: () => Promise<void>) =>
			supervisor ? store.withJobLock(id, "fixture", operation) : store.withWriteLock(id, operation);
		await owner(path);
		type Reader = { readJournalLock(path: string): Promise<unknown> };
		const reader = second as unknown as Reader;
		const read = reader.readJournalLock.bind(reader);
		let captured = () => {};
		const observed = new Promise<void>((resolve) => {
			captured = resolve;
		});
		let entered = () => {};
		const firstEntered = new Promise<void>((resolve) => {
			entered = resolve;
		});
		let release = () => {};
		const held = new Promise<void>((resolve) => {
			release = resolve;
		});
		vi.spyOn(reader, "readJournalLock").mockImplementationOnce(async (current) => {
			const previous = await read(current);
			captured();
			await firstEntered;
			return previous;
		});
		let firstInside = false;
		let overlap = false;
		const competing = acquire(second, async () => {
			overlap = firstInside;
		}).catch((error: unknown) => error);
		await observed;
		const owning = acquire(first, async () => {
			firstInside = true;
			entered();
			await held;
			firstInside = false;
		});
		try {
			const error = await competing;
			expect(overlap).toBe(false);
			expect(error).toBeInstanceOf(ResearchJobLockedError);
			expect(await readdir(path)).toHaveLength(1);
		} finally {
			release();
			await owning;
		}
	},
);

it.each(["empty", "dead"])(
	"recovers an abandoned %s directory and leaves unpublished directories alone",
	async (kind) => {
		const { store, id, path } = await setup();
		const unpublished = `${path}.unpublished-fixture`;
		await owner(unpublished);
		if (kind === "dead") await owner(path);
		else await mkdir(path);
		await store.withWriteLock(id, async () => {
			const names = await readdir(path);
			expect(names).toHaveLength(1);
			const record = JSON.parse(await readFile(join(path, names[0]), "utf8")) as { pid: number; token: string };
			expect(record.pid).toBe(process.pid);
			expect(names[0]).toBe(`owner-${record.token}.json`);
		});
		await expect(readdir(path)).rejects.toMatchObject({ code: "ENOENT" });
		expect(await readdir(unpublished)).toHaveLength(1);
	},
);

it.each(["active", "unknown", "multiple", "mismatch", "file", "symlink", "owner-symlink"])(
	"rejects %s owners without changing their files",
	async (kind) => {
		const { root, store, id, path } = await setup();
		if (kind === "file") await writeFile(path, "legacy file");
		else if (kind === "symlink") {
			await mkdir(join(root, "other"));
			await symlink(join(root, "other"), path);
		} else {
			await mkdir(path);
			if (kind === "unknown") await writeFile(join(path, "unknown"), "retained");
			else {
				const token = randomUUID();
				const content = JSON.stringify({
					owner: "fixture",
					pid: process.pid,
					token: kind === "mismatch" ? randomUUID() : token,
					createdAt: new Date().toISOString(),
				});
				if (kind === "owner-symlink") {
					await writeFile(join(root, "owner"), content);
					await symlink(join(root, "owner"), join(path, `owner-${token}.json`));
				} else await writeFile(join(path, `owner-${token}.json`), content);
				if (kind === "multiple") await writeFile(join(path, "unexpected"), "retained");
			}
		}
		const operation = vi.fn(async () => undefined);
		await expect(store.withWriteLock(id, operation)).rejects.toBeInstanceOf(ResearchJobLockedError);
		expect(operation).not.toHaveBeenCalled();
		if (kind === "file") expect(await readFile(path, "utf8")).toBe("legacy file");
		else expect(await readdir(path)).toHaveLength(kind === "symlink" ? 0 : kind === "multiple" ? 2 : 1);
	},
);

it("release cannot remove a new owner published after the old token was removed", async () => {
	const { store, id, path } = await setup();
	let nextToken = "";
	await store.withWriteLock(id, async () => {
		const [name] = await readdir(path);
		await unlink(join(path, name));
		await rmdir(path);
		nextToken = await owner(path, process.pid);
	});
	expect(await readdir(path)).toEqual([`owner-${nextToken}.json`]);
	await expect(store.withWriteLock(id, async () => undefined)).rejects.toBeInstanceOf(ResearchJobLockedError);
});

it.each(["append", "snapshot"])("releases journal locks after %s failure inside the supervisor lock", async (kind) => {
	const { store, job, id, path } = await setup();
	vi.spyOn(store, kind === "append" ? "append" : "writeSnapshot").mockRejectedValueOnce(new Error("fixture failure"));
	await expect(store.withJobLock(id, "supervisor", () => job.consumeTurns(1))).rejects.toThrow("fixture failure");
	await expect(readdir(path)).rejects.toMatchObject({ code: "ENOENT" });
	const reopened = (await ResearchJob.open(store, id))!;
	await store.withJobLock(id, "supervisor", () => reopened.consumeTurns(1));
});

it.each(["dead", "active", "empty", "json", "owner", "pid", "token", "date", "directory", "symlink"])(
	"F01-04 preserves legacy supervisor %s locks and refuses unknown owners even with old mtime",
	async (kind) => {
		const { root, store, id, path } = await setup();
		const legacy = join(root, ".astra", "jobs", id, "supervisor.lock");
		const record: { owner: string; pid: number; token: string; createdAt: string } = {
			owner: "legacy",
			pid: kind === "active" ? process.pid : 2147483646,
			token: randomUUID(),
			createdAt: new Date(0).toISOString(),
		};
		if (kind === "owner") record.owner = "";
		if (kind === "pid") record.pid = -1;
		if (kind === "token") record.token = "bad";
		if (kind === "date") record.createdAt = "bad";
		const content = kind === "empty" ? "" : kind === "json" ? "{" : JSON.stringify(record);
		if (kind === "directory") await mkdir(legacy);
		else if (kind === "symlink") {
			await writeFile(join(root, "legacy-target"), content);
			await symlink(join(root, "legacy-target"), legacy);
		} else await writeFile(legacy, content);
		await utimes(legacy, new Date(0), new Date(0));
		const operation = vi.fn(async () => {
			expect(await readdir(`${legacy}.d`)).toHaveLength(1);
		});
		if (kind === "dead") await store.withJobLock(id, "supervisor", operation);
		else await expect(store.withJobLock(id, "supervisor", operation)).rejects.toBeInstanceOf(ResearchJobLockedError);
		expect(operation).toHaveBeenCalledTimes(kind === "dead" ? 1 : 0);
		if (kind === "directory") expect(await readdir(legacy)).toEqual([]);
		else expect(await readFile(legacy, "utf8")).toBe(content);
		await expect(readdir(path)).rejects.toMatchObject({ code: "ENOENT" });
	},
);

it.each(["empty", "dead", "active", "unknown", "invalid", "file"])(
	"F01-02 applies the existing directory protocol to supervisor %s locks",
	async (kind) => {
		const { root, store, id } = await setup();
		const path = join(root, ".astra", "jobs", id, "supervisor.lock.d");
		const unpublished = `${path}.tmp-fixture`;
		await owner(unpublished);
		if (kind === "file") await writeFile(path, "retained");
		else if (kind === "empty" || kind === "unknown") {
			await mkdir(path);
			if (kind === "unknown") await writeFile(join(path, "unknown"), "retained");
		} else {
			const token = await owner(path, kind === "active" ? process.pid : 2147483646);
			if (kind === "invalid") await writeFile(join(path, `owner-${token}.json`), "{");
		}
		const operation = vi.fn(async () => {
			expect(await readdir(path)).toHaveLength(1);
		});
		if (["empty", "dead"].includes(kind)) await store.withJobLock(id, "fixture", operation);
		else await expect(store.withJobLock(id, "fixture", operation)).rejects.toBeInstanceOf(ResearchJobLockedError);
		expect(operation).toHaveBeenCalledTimes(["empty", "dead"].includes(kind) ? 1 : 0);
		expect(await readdir(unpublished)).toHaveLength(1);
		if (kind === "file") expect(await readFile(path, "utf8")).toBe("retained");
		else if (!["empty", "dead"].includes(kind)) expect(await readdir(path)).toHaveLength(1);
	},
);

it("F01-03 supervisor release cannot delete a replaced owner and operation failure releases its own owner", async () => {
	const { root, store, id } = await setup();
	const path = join(root, ".astra", "jobs", id, "supervisor.lock.d");
	await expect(
		store.withJobLock(id, "fixture", async () => {
			throw new Error("operation failure");
		}),
	).rejects.toThrow("operation failure");
	let next = "";
	await store.withJobLock(id, "fixture", async () => {
		const [name] = await readdir(path);
		await unlink(join(path, name));
		await rmdir(path);
		next = await owner(path, process.pid);
	});
	expect(await readdir(path)).toEqual([`owner-${next}.json`]);
});

it.each(["journal", "supervisor"] as const)(
	"F01 reports the actual %s lock role in contention diagnostics",
	async (kind) => {
		const { store, id, path } = await setup(kind === "supervisor");
		await owner(path, process.pid);
		const operation = async () => undefined;
		await expect(
			kind === "journal" ? store.withWriteLock(id, operation) : store.withJobLock(id, "fixture", operation),
		).rejects.toThrow(`research ${kind} lock held`);
	},
);
