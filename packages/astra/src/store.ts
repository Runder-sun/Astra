import { randomUUID } from "node:crypto";
import {
	appendFile,
	type FileHandle,
	lstat,
	mkdir,
	mkdtemp,
	open,
	readdir,
	readFile,
	rename,
	rm,
	rmdir,
	unlink,
	writeFile,
} from "node:fs/promises";
import { join } from "node:path";
import type { AstraEvent, JobSnapshot, StoredEvent } from "./types.ts";

export class ResearchJobLockedError extends Error {}
export class ResearchJobBusyError extends ResearchJobLockedError {}

export interface ResearchExecutionOwner {
	jobId: string;
	owner: string;
	pid: number;
	token: string;
	createdAt: string;
}

export interface AstraStore {
	loadSnapshot(jobId: string): Promise<JobSnapshot | undefined>;
	/** Cross-process writers must hold withWriteLock; append does not acquire it again. */
	append(jobId: string, event: AstraEvent): Promise<StoredEvent>;
	writeSnapshot(snapshot: JobSnapshot): Promise<void>;
	readEvents(jobId: string): Promise<StoredEvent[]>;
	readEventSeq(jobId: string): Promise<number>;
	withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T>;
	withWriteLock<T>(jobId: string, operation: () => Promise<T>): Promise<T>;
	withExecutionLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T>;
	readExecutionOwner(jobId: string): Promise<ResearchExecutionOwner | undefined>;
}

interface JobLockRecord {
	jobId?: string;
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
		record.owner.length > 0 &&
		typeof record.pid === "number" &&
		Number.isInteger(record.pid) &&
		record.pid > 0 &&
		typeof record.token === "string" &&
		/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(record.token) &&
		typeof record.createdAt === "string" &&
		Number.isFinite(Date.parse(record.createdAt))
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

	async withJobLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		const legacy = join(this.jobDir(jobId), "supervisor.lock");
		return this.withDirectoryLock(jobId, `${legacy}.d`, "supervisor", owner, async () => {
			await this.assertLegacySupervisorAvailable(jobId);
			return operation();
		});
	}

	private async assertLegacySupervisorAvailable(jobId: string): Promise<void> {
		const legacy = join(this.jobDir(jobId), "supervisor.lock");
		try {
			if (!(await lstat(legacy)).isFile())
				throw new ResearchJobLockedError("research supervisor legacy lock is not a file");
			const holder: unknown = JSON.parse(await readFile(legacy, "utf8"));
			if (!isJobLockRecord(holder)) throw new ResearchJobLockedError("research supervisor legacy owner is invalid");
			if (holder.jobId !== undefined && holder.jobId !== jobId)
				throw new ResearchJobLockedError("research supervisor owner job identity is invalid");
			if (processIsAlive(holder.pid))
				throw new ResearchJobBusyError(`research supervisor lock held by ${holder.owner} (pid ${holder.pid})`);
		} catch (error) {
			if (error instanceof SyntaxError)
				throw new ResearchJobLockedError("research supervisor legacy owner is invalid");
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
	}

	async withWriteLock<T>(jobId: string, operation: () => Promise<T>): Promise<T> {
		return this.withDirectoryLock(
			jobId,
			join(this.jobDir(jobId), "journal.lock"),
			"journal",
			"journal writer",
			async () => {
				await this.repairEventTail(jobId);
				return operation();
			},
		);
	}

	async withExecutionLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		return this.withDirectoryLock(jobId, join(this.jobDir(jobId), "execution.lock"), "execution", owner, async () => {
			await this.withJobLock(jobId, owner, async () => {});
			return operation();
		});
	}

	async readExecutionOwner(jobId: string): Promise<ResearchExecutionOwner | undefined> {
		let holder: JobLockRecord | undefined;
		try {
			holder = await this.readJournalLock(join(this.jobDir(jobId), "execution.lock"), "execution");
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
		if (holder && holder.jobId !== jobId)
			throw new ResearchJobLockedError("research execution owner job identity is invalid");
		if (holder && processIsAlive(holder.pid)) return { ...holder, jobId };
		await this.assertLegacySupervisorAvailable(jobId);
		try {
			const supervisor = await this.readJournalLock(join(this.jobDir(jobId), "supervisor.lock.d"), "supervisor");
			if (supervisor && processIsAlive(supervisor.pid))
				throw new ResearchJobLockedError(
					`research supervisor lock held by ${supervisor.owner} (pid ${supervisor.pid})`,
				);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
		return undefined;
	}

	async withGuidanceLock<T>(jobId: string, operation: () => Promise<T>): Promise<T> {
		return this.withDirectoryLock(
			jobId,
			join(this.jobDir(jobId), "guidance.lock"),
			"guidance",
			"guidance inbox",
			operation,
		);
	}

	private async withDirectoryLock<T>(
		jobId: string,
		path: string,
		kind: "journal" | "supervisor" | "execution" | "guidance",
		owner: string,
		operation: () => Promise<T>,
	): Promise<T> {
		await mkdir(this.jobDir(jobId), { recursive: true });
		const temp = await mkdtemp(`${path}.tmp-`);
		const lock: JobLockRecord = {
			jobId,
			owner,
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
						const holder = await this.readJournalLock(path, kind);
						if (kind === "execution" && holder && holder.jobId !== jobId)
							throw new ResearchJobLockedError("research execution owner job identity is invalid");
						if (kind === "supervisor" && holder?.jobId !== undefined && holder.jobId !== jobId)
							throw new ResearchJobLockedError("research supervisor owner job identity is invalid");
						if (holder && processIsAlive(holder.pid))
							throw new (kind === "supervisor" ? ResearchJobBusyError : ResearchJobLockedError)(
								`research ${kind} lock held by ${holder.owner} (pid ${holder.pid})`,
							);
						await this.removeJournalOwner(path, holder?.token);
					} catch (inspectionError) {
						if ((inspectionError as NodeJS.ErrnoException).code !== "ENOENT") throw inspectionError;
					}
				}
			}
			if (!acquired) throw new ResearchJobLockedError(`research ${kind} lock changed during recovery`);
			try {
				return await operation();
			} finally {
				await this.removeJournalOwner(path, lock.token);
			}
		} finally {
			await rm(temp, { recursive: true, force: true });
		}
	}

	private async readJournalLock(
		path: string,
		kind: "journal" | "supervisor" | "execution" | "guidance" = "journal",
	): Promise<JobLockRecord | undefined> {
		if (!(await lstat(path)).isDirectory())
			throw new ResearchJobLockedError(`research ${kind} lock is not a directory`);
		const names = await readdir(path);
		if (names.length === 0) return undefined;
		if (
			names.length !== 1 ||
			!/^owner-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.json$/.test(names[0])
		)
			throw new ResearchJobLockedError(`research ${kind} lock has an unknown owner`);
		const ownerPath = join(path, names[0]);
		if (!(await lstat(ownerPath)).isFile()) throw new ResearchJobLockedError(`research ${kind} owner is not a file`);
		let value: unknown;
		try {
			value = JSON.parse(await readFile(ownerPath, "utf8"));
		} catch (error) {
			if (error instanceof SyntaxError) throw new ResearchJobLockedError(`research ${kind} owner is invalid`);
			throw error;
		}
		if (!isJobLockRecord(value) || names[0] !== `owner-${value.token}.json`)
			throw new ResearchJobLockedError(`research ${kind} owner is invalid`);
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
			const content = await this.readEventContent(jobId);
			const { events: current } = this.parseEvents(jobId, content);
			if (content && !content.endsWith("\n"))
				throw new Error("research journal tail requires withWriteLock before append");
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

	private async readEventContent(jobId: string): Promise<string> {
		try {
			return await readFile(this.eventsPath(jobId), "utf8");
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return "";
			throw error;
		}
	}

	// Recognizes only a valid JSON prefix interrupted at EOF. It never repairs or
	// produces a value; complete records still use JSON.parse on every runtime.
	private isIncompleteJson(text: string): boolean {
		let position = 0;
		const incomplete = Symbol("EOF");
		const invalid = Symbol("invalid JSON");
		const whitespace = () => {
			while (/[ \t\r\n]/.test(text[position] ?? "x")) position++;
		};
		const next = () => {
			if (position === text.length) throw incomplete;
			return text[position++];
		};
		const string = () => {
			if (next() !== '"') throw invalid;
			for (;;) {
				const char = next();
				if (char === '"') return;
				if (char.charCodeAt(0) < 0x20) throw invalid;
				if (char !== "\\") continue;
				const escaped = next();
				if ('"\\/bfnrt'.includes(escaped)) continue;
				if (escaped !== "u") throw invalid;
				for (let index = 0; index < 4; index++) if (!/[0-9a-fA-F]/.test(next())) throw invalid;
			}
		};
		const digits = () => {
			if (position === text.length) throw incomplete;
			if (!/[0-9]/.test(text[position])) throw invalid;
			while (/[0-9]/.test(text[position] ?? "x")) position++;
		};
		const value = (): void => {
			whitespace();
			if (position === text.length) throw incomplete;
			const char = text[position];
			if (char === '"') {
				string();
				return;
			}
			if (char === "{" || char === "[") {
				position++;
				const end = char === "{" ? "}" : "]";
				whitespace();
				if (text[position] === end) {
					position++;
					return;
				}
				for (;;) {
					if (char === "{") {
						whitespace();
						string();
						whitespace();
						if (next() !== ":") throw invalid;
					}
					value();
					whitespace();
					const separator = next();
					if (separator === end) return;
					if (separator !== ",") throw invalid;
				}
			}
			for (const literal of ["true", "false", "null"]) {
				if (char !== literal[0]) continue;
				for (const expected of literal) if (next() !== expected) throw invalid;
				return;
			}
			if (char === "-") position++;
			if (text[position] === "0") position++;
			else digits();
			if (text[position] === ".") {
				position++;
				digits();
			}
			if (text[position] === "e" || text[position] === "E") {
				position++;
				if (text[position] === "+" || text[position] === "-") position++;
				digits();
			}
		};
		try {
			value();
			whitespace();
			return false;
		} catch (error) {
			if (error === incomplete) return true;
			if (error === invalid) return false;
			throw error;
		}
	}

	private parseEvents(
		jobId: string,
		content: string,
	): { events: StoredEvent[]; completeBytes: number; incomplete: boolean } {
		const events: StoredEvent[] = [];
		const lines = content.split("\n");
		for (let index = 0; index < lines.length; index++) {
			const line = lines[index];
			if (!line) continue;
			let stored: StoredEvent;
			try {
				stored = JSON.parse(line) as StoredEvent;
			} catch (error) {
				if (error instanceof SyntaxError && index === lines.length - 1 && this.isIncompleteJson(line))
					return {
						events,
						completeBytes: Buffer.byteLength(content.slice(0, content.lastIndexOf("\n") + 1)),
						incomplete: true,
					};
				throw error;
			}
			if (!stored || stored.seq !== events.length + 1 || stored.jobId !== jobId)
				throw new Error(`research journal event sequence or job id mismatch for ${jobId}`);
			events.push(stored);
		}
		return { events, completeBytes: Buffer.byteLength(content), incomplete: false };
	}

	private async repairEventTail(jobId: string): Promise<void> {
		let handle: FileHandle;
		try {
			handle = await open(this.eventsPath(jobId), "r+");
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
			throw error;
		}
		try {
			const { size } = await handle.stat();
			if (size === 0) return;
			const last = Buffer.alloc(1);
			await handle.read(last, 0, 1, size - 1);
			if (last[0] === 10) return;
			const parsed = this.parseEvents(jobId, await handle.readFile("utf8"));
			if (parsed.incomplete) await handle.truncate(parsed.completeBytes);
			else await handle.write("\n", size, "utf8");
		} finally {
			await handle.close();
		}
	}

	async readEvents(jobId: string): Promise<StoredEvent[]> {
		return this.parseEvents(jobId, await this.readEventContent(jobId)).events;
	}

	async readEventSeq(jobId: string): Promise<number> {
		return (await this.readEvents(jobId)).at(-1)?.seq ?? 0;
	}
}

export class MemoryAstraStore implements AstraStore {
	private readonly executionOwners = new Map<string, ResearchExecutionOwner>();

	async withExecutionLock<T>(jobId: string, owner: string, operation: () => Promise<T>): Promise<T> {
		if (this.executionOwners.has(jobId)) throw new ResearchJobLockedError("research execution lock held");
		this.executionOwners.set(jobId, {
			jobId,
			owner,
			pid: process.pid,
			token: randomUUID(),
			createdAt: new Date().toISOString(),
		});
		try {
			await this.withJobLock(jobId, owner, async () => {});
			return await operation();
		} finally {
			this.executionOwners.delete(jobId);
		}
	}

	async readExecutionOwner(jobId: string): Promise<ResearchExecutionOwner | undefined> {
		const holder = this.executionOwners.get(jobId);
		if (!holder && this.jobLocks.has(jobId)) throw new ResearchJobLockedError("research supervisor lock held");
		return holder ? { ...holder } : undefined;
	}
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
		if (holder) throw new ResearchJobBusyError(`research supervisor lock held by ${holder}`);
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
