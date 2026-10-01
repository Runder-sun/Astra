import { randomUUID } from "node:crypto";
import {
	appendFile,
	lstat,
	mkdir,
	mkdtemp,
	open,
	readdir,
	readFile,
	rename,
	rm,
	rmdir,
	stat,
	unlink,
	writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import type { AstraEvent, JobSnapshot, StoredEvent } from "./types.ts";

export class ResearchJobLockedError extends Error {}

export interface AstraStore {
	loadSnapshot(jobId: string): Promise<JobSnapshot | undefined>;
	append(jobId: string, event: AstraEvent): Promise<StoredEvent>;
	writeSnapshot(snapshot: JobSnapshot): Promise<void>;
	readEvents(jobId: string): Promise<StoredEvent[]>;
	readEventSeq(jobId: string): Promise<number>;
	withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T>;
	withWriteLock<T>(jobId: string, operation: () => Promise<T>): Promise<T>;
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

	private async readJobLock(path: string): Promise<JobLockRecord | undefined> {
		try {
			const value: unknown = JSON.parse(await readFile(path, "utf8"));
			return isJobLockRecord(value) ? value : undefined;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT" || error instanceof SyntaxError) return undefined;
			throw error;
		}
	}

	async withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		return this.withFileLock(jobId, this.lockPath(jobId), owner, operation);
	}

	async withWriteLock<T>(jobId: string, operation: () => Promise<T>): Promise<T> {
		await mkdir(this.jobDir(jobId), { recursive: true });
		const path = join(this.jobDir(jobId), "journal.lock");
		const temp = await mkdtemp(`${path}.tmp-`);
		const lock: JobLockRecord = {
			owner: "journal writer",
			pid: process.pid,
			token: randomUUID(),
			createdAt: new Date().toISOString(),
		};
		try {
			await writeFile(join(temp, `owner-${lock.token}.json`), `${JSON.stringify(lock)}\n`, { mode: 0o600 });
			let acquired = false;
			for (let attempt = 0; attempt < 4; attempt++) {
				try {
					await rename(temp, path);
					acquired = true;
					break;
				} catch (error) {
					const code = (error as NodeJS.ErrnoException).code;
					if (!["EEXIST", "ENOTEMPTY", "ENOTDIR", "EPERM", "EACCES"].includes(code ?? "")) throw error;
					if (code === "EPERM" || code === "EACCES") {
						const target = await lstat(path).catch((inspectionError: unknown) => {
							if ((inspectionError as NodeJS.ErrnoException).code === "ENOENT") throw error;
							throw inspectionError;
						});
						if (!target.isDirectory()) throw error;
					}
					try {
						const holder = await this.readJournalLock(path);
						if (holder && processIsAlive(holder.pid))
							throw new ResearchJobLockedError(
								`research journal lock held by ${holder.owner} (pid ${holder.pid})`,
							);
						await this.removeJournalOwner(path, holder?.token);
					} catch (inspectionError) {
						if ((inspectionError as NodeJS.ErrnoException).code !== "ENOENT") throw inspectionError;
					}
				}
			}
			if (!acquired) throw new ResearchJobLockedError("research journal lock changed during recovery");
			try {
				return await operation();
			} finally {
				await this.removeJournalOwner(path, lock.token);
			}
		} finally {
			await rm(temp, { recursive: true, force: true });
		}
	}

	private async readJournalLock(path: string): Promise<JobLockRecord | undefined> {
		if (!(await lstat(path)).isDirectory())
			throw new ResearchJobLockedError("research journal lock is not a directory");
		const names = await readdir(path);
		if (names.length === 0) return undefined;
		if (
			names.length !== 1 ||
			!/^owner-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.json$/.test(names[0])
		)
			throw new ResearchJobLockedError("research journal lock has an unknown owner");
		const ownerPath = join(path, names[0]);
		if (!(await lstat(ownerPath)).isFile()) throw new ResearchJobLockedError("research journal owner is not a file");
		let value: unknown;
		try {
			value = JSON.parse(await readFile(ownerPath, "utf8"));
		} catch (error) {
			if (error instanceof SyntaxError) throw new ResearchJobLockedError("research journal owner is invalid");
			throw error;
		}
		if (
			!isJobLockRecord(value) ||
			names[0] !== `owner-${value.token}.json` ||
			!value.owner ||
			!Number.isInteger(value.pid) ||
			value.pid <= 0 ||
			!Number.isFinite(Date.parse(value.createdAt))
		)
			throw new ResearchJobLockedError("research journal owner is invalid");
		return value;
	}

	private async removeJournalOwner(path: string, token?: string): Promise<void> {
		if (token) {
			try {
				await unlink(join(path, `owner-${token}.json`));
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			}
		}
		try {
			await rmdir(path);
		} catch (error) {
			if (!["ENOENT", "ENOTEMPTY", "EEXIST"].includes((error as NodeJS.ErrnoException).code ?? "")) throw error;
		}
	}

	private async withFileLock<T>(jobId: string, path: string, owner: string, operation: () => Promise<T>): Promise<T> {
		await mkdir(this.jobDir(jobId), { recursive: true });
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
				const holder = await this.readJobLock(path);
				if (holder && processIsAlive(holder.pid)) {
					throw new ResearchJobLockedError(`research supervisor lock held by ${holder.owner} (pid ${holder.pid})`);
				}
				if (!holder) {
					const lockStat = await stat(path).catch(() => undefined);
					if (lockStat && Date.now() - lockStat.mtimeMs < 30_000) {
						throw new ResearchJobLockedError("research supervisor lock held by an initializing process");
					}
				}
				await rm(path, { force: true });
			}
		}
		try {
			return await operation();
		} finally {
			const holder = await this.readJobLock(path);
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
		const temp = `${target}.tmp-${process.pid}-${randomUUID()}`;
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

	async readEventSeq(jobId: string): Promise<number> {
		return (await this.readEvents(jobId)).at(-1)?.seq ?? 0;
	}
}

export class MemoryAstraStore implements AstraStore {
	private readonly snapshots = new Map<string, JobSnapshot>();
	private readonly events = new Map<string, StoredEvent[]>();
	private readonly jobLocks = new Map<string, string>();
	private readonly writeLocks = new Set<string>();

	async withWriteLock<T>(jobId: string, operation: () => Promise<T>): Promise<T> {
		if (this.writeLocks.has(jobId)) throw new ResearchJobLockedError("research journal writer lock held");
		this.writeLocks.add(jobId);
		try {
			return await operation();
		} finally {
			this.writeLocks.delete(jobId);
		}
	}

	async withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		const holder = this.jobLocks.get(jobId);
		if (holder) throw new ResearchJobLockedError(`research supervisor lock held by ${holder}`);
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

	async readEventSeq(jobId: string): Promise<number> {
		return this.events.get(jobId)?.length ?? 0;
	}
}
