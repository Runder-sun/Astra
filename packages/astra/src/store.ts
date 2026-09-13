import { randomUUID } from "node:crypto";
import { appendFile, mkdir, open, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { AstraEvent, JobSnapshot, StoredEvent } from "./types.ts";

export interface AstraStore {
	loadSnapshot(jobId: string): Promise<JobSnapshot | undefined>;
	append(jobId: string, event: AstraEvent): Promise<StoredEvent>;
	writeSnapshot(snapshot: JobSnapshot): Promise<void>;
	readEvents(jobId: string): Promise<StoredEvent[]>;
	withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T>;
}

interface JobLockRecord {
	owner: string;
	pid: number;
	token: string;
	createdAt: string;
}

function isJobLockRecord(value: unknown): value is JobLockRecord {
	if (value === null || typeof value !== "object") return false;
	const record = value as Record<string, unknown>;
	return (
		typeof record.owner === "string" &&
		typeof record.pid === "number" &&
		typeof record.token === "string" &&
		typeof record.createdAt === "string"
	);
}

function processIsAlive(pid: number): boolean {
	try {
		process.kill(pid, 0);
		return true;
	} catch (error) {
		return (error as NodeJS.ErrnoException).code !== "ESRCH";
	}
}

export class JsonlAstraStore implements AstraStore {
	readonly root: string;
	private readonly appendLocks = new Map<string, Promise<void>>();

	constructor(projectRoot: string) {
		this.root = join(projectRoot, ".astra");
	}

	private jobDir(jobId: string): string {
		return join(this.root, "jobs", jobId);
	}

	private snapshotPath(jobId: string): string {
		return join(this.jobDir(jobId), "job.json");
	}

	private eventsPath(jobId: string): string {
		return join(this.jobDir(jobId), "events.jsonl");
	}

	private lockPath(jobId: string): string {
		return join(this.jobDir(jobId), "supervisor.lock");
	}

	private async readJobLock(jobId: string): Promise<JobLockRecord | undefined> {
		try {
			const value: unknown = JSON.parse(await readFile(this.lockPath(jobId), "utf8"));
			return isJobLockRecord(value) ? value : undefined;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT" || error instanceof SyntaxError) return undefined;
			throw error;
		}
	}

	async withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		await mkdir(this.jobDir(jobId), { recursive: true });
		const path = this.lockPath(jobId);
		const lock: JobLockRecord = {
			owner,
			pid: process.pid,
			token: randomUUID(),
			createdAt: new Date().toISOString(),
		};
		for (;;) {
			try {
				const handle = await open(path, "wx", 0o600);
				try {
					await handle.writeFile(`${JSON.stringify(lock)}\n`, "utf8");
				} finally {
					await handle.close();
				}
				break;
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
				const holder = await this.readJobLock(jobId);
				if (holder && processIsAlive(holder.pid)) {
					throw new Error(`research supervisor lock held by ${holder.owner} (pid ${holder.pid})`);
				}
				if (!holder) {
					const lockStat = await stat(path).catch(() => undefined);
					if (lockStat && Date.now() - lockStat.mtimeMs < 30_000) {
						throw new Error("research supervisor lock held by an initializing process");
					}
				}
				await rm(path, { force: true });
			}
		}
		try {
			return await operation();
		} finally {
			const holder = await this.readJobLock(jobId);
			if (holder?.token === lock.token) await rm(path, { force: true });
		}
	}

	async loadSnapshot(jobId: string): Promise<JobSnapshot | undefined> {
		try {
			return JSON.parse(await readFile(this.snapshotPath(jobId), "utf8")) as JobSnapshot;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
			throw error;
		}
	}

	async append(jobId: string, event: AstraEvent): Promise<StoredEvent> {
		const previous = this.appendLocks.get(jobId) ?? Promise.resolve();
		let resolveLock!: () => void;
		const lock = new Promise<void>((resolve) => {
			resolveLock = resolve;
		});
		const queued = previous.catch(() => undefined).then(() => lock);
		this.appendLocks.set(jobId, queued);
		await previous;
		try {
			await mkdir(this.jobDir(jobId), { recursive: true });
			const current = await this.readEvents(jobId);
			const stored: StoredEvent = { seq: current.length + 1, timestamp: new Date().toISOString(), jobId, event };
			await appendFile(this.eventsPath(jobId), `${JSON.stringify(stored)}\n`, "utf8");
			return stored;
		} finally {
			resolveLock();
			if (this.appendLocks.get(jobId) === queued) this.appendLocks.delete(jobId);
		}
	}

	async writeSnapshot(snapshot: JobSnapshot): Promise<void> {
		await mkdir(this.jobDir(snapshot.frame.jobId), { recursive: true });
		const target = this.snapshotPath(snapshot.frame.jobId);
		const temp = `${target}.tmp-${process.pid}`;
		await writeFile(temp, `${JSON.stringify(snapshot, null, 2)}\n`, "utf8");
		await rename(temp, target);
	}

	async readEvents(jobId: string): Promise<StoredEvent[]> {
		try {
			const content = await readFile(this.eventsPath(jobId), "utf8");
			return content
				.split("\n")
				.filter(Boolean)
				.map((line) => JSON.parse(line) as StoredEvent);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
			throw error;
		}
	}
}

export class MemoryAstraStore implements AstraStore {
	private readonly snapshots = new Map<string, JobSnapshot>();
	private readonly events = new Map<string, StoredEvent[]>();
	private readonly jobLocks = new Map<string, string>();

	async withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		const holder = this.jobLocks.get(jobId);
		if (holder) throw new Error(`research supervisor lock held by ${holder}`);
		this.jobLocks.set(jobId, owner);
		try {
			return await operation();
		} finally {
			if (this.jobLocks.get(jobId) === owner) this.jobLocks.delete(jobId);
		}
	}

	async loadSnapshot(jobId: string): Promise<JobSnapshot | undefined> {
		const snapshot = this.snapshots.get(jobId);
		return snapshot ? structuredClone(snapshot) : undefined;
	}

	async append(jobId: string, event: AstraEvent): Promise<StoredEvent> {
		const events = this.events.get(jobId) ?? [];
		const stored: StoredEvent = { seq: events.length + 1, timestamp: new Date().toISOString(), jobId, event };
		events.push(structuredClone(stored));
		this.events.set(jobId, events);
		return structuredClone(stored);
	}

	async writeSnapshot(snapshot: JobSnapshot): Promise<void> {
		this.snapshots.set(snapshot.frame.jobId, structuredClone(snapshot));
	}

	async readEvents(jobId: string): Promise<StoredEvent[]> {
		return structuredClone(this.events.get(jobId) ?? []);
	}
}
