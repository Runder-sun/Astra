import { createHash } from "node:crypto";
import { cp, lstat, mkdir, readdir, readFile, readlink, rename, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import type { CleanupTaskFiles, JobSnapshot } from "./types.ts";

async function exists(path: string): Promise<boolean> {
	try {
		await lstat(path);
		return true;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
		throw error;
	}
}

/** Captured before the intent: absence after a restart must not mean archive success. */
export async function cleanupTaskFiles(snapshot: JobSnapshot, taskIds: string[]): Promise<CleanupTaskFiles[]> {
	const root = join(snapshot.frame.permissions.workspaceRoot, ".astra", "jobs", snapshot.frame.jobId);
	return Promise.all(
		[...new Set(taskIds)].map(async (taskId) => ({
			taskId,
			workspace: await exists(join(root, "workspaces", taskId)),
			task: await exists(join(root, "tasks", taskId)),
			resources: await exists(join(root, "resources", taskId)),
			sessions: await Promise.all(
				Object.values(snapshot.sessions)
					.filter((session) => session.taskId === taskId && session.sessionFile)
					.map(async (session) => ({
						sessionId: session.sessionId,
						source: session.sessionFile!,
						target: join(
							root,
							"archive",
							"tasks",
							taskId,
							"sessions",
							`${createHash("sha256").update(session.sessionId).digest("hex")}.jsonl`,
						),
						present: await exists(session.sessionFile!),
					})),
			),
		})),
	);
}

async function treeHash(path: string): Promise<string> {
	const metadata = await lstat(path);
	if (metadata.isSymbolicLink()) return `link:${await readlink(path)}`;
	if (!metadata.isDirectory())
		return createHash("sha256")
			.update(await readFile(path))
			.digest("hex");
	const entries = await Promise.all(
		(await readdir(path)).sort().map(async (name) => [name, await treeHash(join(path, name))]),
	);
	return createHash("sha256").update(JSON.stringify(entries)).digest("hex");
}

/** Recursive deletion can leave a byte-identical subset of an already complete archive. */
async function sourceMatchesArchive(source: string, target: string): Promise<boolean> {
	if (!(await exists(target))) return false;
	const [sourceMetadata, targetMetadata] = await Promise.all([lstat(source), lstat(target)]);
	if (
		sourceMetadata.isSymbolicLink() !== targetMetadata.isSymbolicLink() ||
		sourceMetadata.isDirectory() !== targetMetadata.isDirectory() ||
		sourceMetadata.isFile() !== targetMetadata.isFile()
	)
		return false;
	if (sourceMetadata.isSymbolicLink()) return (await readlink(source)) === (await readlink(target));
	if (!sourceMetadata.isDirectory()) return (await treeHash(source)) === (await treeHash(target));
	for (const name of await readdir(source))
		if (!(await sourceMatchesArchive(join(source, name), join(target, name)))) return false;
	return true;
}

async function archiveCopy(source: string, target: string, present: boolean): Promise<boolean> {
	if (resolve(source) === resolve(target)) {
		if (!(await exists(target))) throw new Error(`cleanup archive missing: ${target}`);
		return true;
	}
	if (!present) return false;
	const sourceExists = await exists(source);
	if (await exists(target)) {
		if (sourceExists && !(await sourceMatchesArchive(source, target)))
			throw new Error(`cleanup archive conflicts with source: ${target}`);
		return true;
	}
	if (!sourceExists) throw new Error(`cleanup source and archive both missing: ${source}`);
	await mkdir(dirname(target), { recursive: true });
	const staging = `${target}.cleanup-copy`;
	await rm(staging, { recursive: true, force: true });
	await cp(source, staging, { recursive: true, force: false, verbatimSymlinks: true });
	if ((await treeHash(source)) !== (await treeHash(staging)))
		throw new Error(`cleanup archive verification failed: ${source}`);
	await rename(staging, target);
	return true;
}

/** Only called under the job writer lock after a durable intent. */
export async function archiveAndPruneTasks(snapshot: JobSnapshot, tasks: CleanupTaskFiles[]): Promise<string[]> {
	const root = join(snapshot.frame.permissions.workspaceRoot, ".astra", "jobs", snapshot.frame.jobId);
	const archiveRefs: string[] = [];
	for (const task of tasks) {
		const archive = join(root, "archive", "tasks", task.taskId);
		let archived = false;
		for (const [folder, target, present] of [
			["workspaces", "workspace", task.workspace],
			["tasks", "task", task.task],
		] as const) {
			if (await archiveCopy(join(root, folder, task.taskId), join(archive, target), present)) archived = true;
		}
		for (const session of task.sessions)
			if (await archiveCopy(session.source, session.target, session.present)) archived = true;
		if (task.resources) {
			const source = join(root, "resources", task.taskId);
			const target = join(archive, "resources");
			if (await exists(source)) {
				if (await exists(target)) throw new Error(`cleanup resource archive conflicts with source: ${target}`);
				await mkdir(archive, { recursive: true });
				await rename(source, target);
			} else if (!(await exists(target))) {
				throw new Error(`cleanup resource source and archive both missing: ${source}`);
			}
			archived = true;
		}
		if (archived) archiveRefs.push(archive);
		// All archives for this task exist before deleting its source copies.
		if (task.workspace) await rm(join(root, "workspaces", task.taskId), { recursive: true, force: true });
		if (task.task) await rm(join(root, "tasks", task.taskId), { recursive: true, force: true });
		for (const session of task.sessions)
			if (session.present && resolve(session.source) !== resolve(session.target))
				await rm(session.source, { force: true });
	}
	return archiveRefs;
}
