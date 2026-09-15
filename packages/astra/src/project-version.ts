import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import type { GitVersion } from "./types.ts";

/** Observes Git only: never changes HEAD, the index, or the user's worktree. */
export async function captureGitVersion(projectRoot: string, snapshotRoot: string): Promise<GitVersion> {
	const git = async (...args: string[]) =>
		(
			await promisify(execFile)("git", ["-C", projectRoot, ...args], {
				env: { ...process.env, LC_ALL: "C", GIT_OPTIONAL_LOCKS: "0" },
				maxBuffer: 32 * 1024 * 1024,
				timeout: 30_000,
			})
		).stdout;
	let root: string;
	try {
		root = (await git("rev-parse", "--show-toplevel")).trim();
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT")
			return { status: "unavailable", reason: "Git executable or project directory unavailable" };
		if (error instanceof Error && error.message.includes("not a git repository"))
			return { status: "unavailable", reason: "Project is not a Git repository" };
		throw error;
	}
	let head: string | null = null;
	try {
		head = (await git("rev-parse", "--verify", "HEAD")).trim();
	} catch {
		throw new Error("Git version capture requires an initial commit");
	}
	const branch = (await git("rev-parse", "--abbrev-ref", "HEAD")).trim();
	const paths = ["--", ".", ":(exclude)**/.astra/**", ":(exclude).astra/**"];
	const diffArgs = ["diff", "--binary", "--no-ext-diff", "--no-textconv", "HEAD", ...paths];
	const patch = await git(...diffArgs);
	const indexArgs = ["diff", "--cached", "--binary", "--no-ext-diff", "--no-textconv", "HEAD", ...paths];
	const indexPatch = await git(...indexArgs);
	const untrackedArgs = ["ls-files", "--others", "--exclude-standard", "-z", ...paths];
	const untrackedPaths = await git(...untrackedArgs);
	const untracked: Array<{ path: string; sha256: string; mode: number }> = [];
	await mkdir(snapshotRoot, { recursive: true });
	for (const path of untrackedPaths.split("\0").filter(Boolean)) {
		const metadata = await lstat(join(projectRoot, path));
		if (!metadata.isFile()) throw new Error(`Git version cannot snapshot an untracked non-file: ${path}`);
		const content = await readFile(join(projectRoot, path));
		const sha256 = createHash("sha256").update(content).digest("hex");
		await writeFile(join(snapshotRoot, sha256), content, { mode: 0o600 });
		untracked.push({ path, sha256, mode: metadata.mode & 0o777 });
	}
	if (
		(await git(...diffArgs)) !== patch ||
		(await git(...indexArgs)) !== indexPatch ||
		(await git(...untrackedArgs)) !== untrackedPaths ||
		(await git("rev-parse", "HEAD")).trim() !== head
	) {
		throw new Error("Git project changed during version capture");
	}
	for (const file of untracked) {
		if (
			createHash("sha256")
				.update(await readFile(join(projectRoot, file.path)))
				.digest("hex") !== file.sha256
		)
			throw new Error(`Git project changed during version capture: ${file.path}`);
	}
	const patchSha256 = createHash("sha256").update(patch).digest("hex");
	const indexPatchSha256 = createHash("sha256").update(indexPatch).digest("hex");
	await writeFile(join(snapshotRoot, patchSha256), patch, { mode: 0o600 });
	await writeFile(join(snapshotRoot, indexPatchSha256), indexPatch, { mode: 0o600 });
	return {
		status: "captured",
		root,
		head,
		branch,
		dirty: patch.length > 0 || indexPatch.length > 0 || untracked.length > 0,
		patchSha256,
		indexPatchSha256,
		untracked,
	};
}
