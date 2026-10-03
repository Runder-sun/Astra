import { execFile } from "node:child_process";
import * as fs from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { afterEach, expect, it, vi } from "vitest";
import { publishImmutableFile } from "../src/contracts.ts";
import { writeSourceReceipt } from "../src/literature.ts";
import { captureGitVersion } from "../src/project-version.ts";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import { prepareReviewEvidenceBundle, prepareTaskWorkspace } from "../src/task-workspace.ts";
import { lifecycleScenario } from "./lifecycle-fixture.ts";

vi.mock("node:fs/promises", async (importOriginal) => ({ ...(await importOriginal<typeof fs>()) }));

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => fs.rm(root, { recursive: true, force: true })));
});

async function scenario(stageId = "validation") {
	const root = await fs.mkdtemp(join(tmpdir(), "astra-publication-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "offline file publication regression",
		automation: "full",
		definitions: [DEFAULT_STAGES.find((stage) => stage.id === stageId)!],
	});
	const stage = job.definitions[stageId];
	const task = await job.dispatchTask({
		stageId,
		stageExecutionId: stageId,
		role: "worker",
		objective: "retain complete bytes",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: stage.outputArtifactType,
		requiredOutputFields: stage.requiredOutputFields,
		acceptanceChecks: stage.acceptanceChecks,
		successCriteria: stage.acceptanceChecks,
		failureSignals: stage.failureSignals,
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: stage.workerTools,
		writeAuthority: stage.workspaceWrite ? "workspace-write" : "none",
		budget: stage.workerBudget!,
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	return { root, store, job, task };
}

async function evidenceScenario() {
	const fixture = await scenario();
	const { job, task } = fixture;
	const cwd = await prepareTaskWorkspace(task, job);
	await fs.writeFile(join(cwd, "result.txt"), "complete original evidence\n");
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: task.requiredOutputType,
		content: { content: "offline evidence" },
		refs: [`.astra/jobs/${task.jobId}/workspaces/${task.id}/result.txt`],
	});
	return { ...fixture, cwd, evidence };
}

it("F01 re-prepares a readonly whole-research summary atomically", async () => {
	const { job, task } = await scenario("research-review");
	const cwd = await prepareTaskWorkspace(task, job);
	const path = join(cwd, "review-summary.json");
	const original = await fs.readFile(path, "utf8");
	expect((await fs.stat(path)).mode & 0o777).toBe(0o444);
	await prepareTaskWorkspace(task, job);
	expect(await fs.readFile(path, "utf8")).toBe(original);
	expect((await fs.stat(path)).mode & 0o777).toBe(0o444);
});

it("F04 reuses an existing shared frozen blob without rewriting registered evidence", async () => {
	const { root, job, task, cwd, evidence } = await evidenceScenario();
	const blob = join(root, ".astra", "jobs", task.jobId, "versions", "files", evidence.files![0].sha256);
	const original = await fs.readFile(blob);
	const second = await job.dispatchTask({ ...task, id: "second", replayKey: "second", objective: "second output" });
	const nextCwd = await prepareTaskWorkspace(second, job);
	await fs.writeFile(join(nextCwd, "result.txt"), await fs.readFile(join(cwd, "result.txt")));
	await job.setTaskStatus(second.id, "succeeded");
	const write = fs.writeFile;
	vi.spyOn(fs, "writeFile").mockImplementation(async (path, ...args) => {
		if (String(path).startsWith(blob)) throw Object.assign(new Error("offline ENOSPC"), { code: "ENOSPC" });
		return write(path, ...args);
	});
	await job.recordEvidence({
		taskId: second.id,
		stageId: second.stageId,
		type: second.requiredOutputType,
		content: { content: "same bytes" },
		refs: [`.astra/jobs/${task.jobId}/workspaces/${second.id}/result.txt`],
	});
	expect(await fs.readFile(blob)).toEqual(original);
	expect((await fs.stat(blob)).mode & 0o777).toBe(0o444);
	expect(Object.values(job.state.evidence)).toHaveLength(2);
});

it.each(["inputs", "review-bundle"])("F08 keeps the prior %s copy after partial write failure", async (kind) => {
	const { root, job, task, evidence } = await evidenceScenario();
	const consumer = await job.dispatchTask({
		...task,
		id: "consumer",
		replayKey: "consumer",
		inputArtifactRefs: [evidence.id],
	});
	const prepare = async () =>
		kind === "inputs"
			? prepareTaskWorkspace(consumer, job)
			: prepareReviewEvidenceBundle({ ...consumer, id: "reviewer" }, evidence, job);
	await prepare();
	const destination =
		kind === "inputs"
			? join(root, ".astra", "jobs", task.jobId, "workspaces", consumer.id, "inputs", evidence.id, "result.txt")
			: join(root, ".astra", "jobs", task.jobId, "tasks", "reviewer", "evidence", evidence.id, "result.txt");
	const original = await fs.readFile(destination);
	const open = fs.open;
	vi.spyOn(fs, "open").mockImplementation(async (path, ...args) => {
		const handle = await open(path, ...args);
		if (String(path).startsWith(destination)) {
			const write = handle.writeFile.bind(handle);
			vi.spyOn(handle, "writeFile").mockImplementation(async () => {
				await write("partial");
				throw Object.assign(new Error("offline partial write"), { code: "ENOSPC" });
			});
		}
		return handle;
	});
	await expect(prepare()).rejects.toThrow(/offline partial write/);
	expect(await fs.readFile(destination)).toEqual(original);
	vi.restoreAllMocks();
	await prepare();
	expect(await fs.readFile(destination)).toEqual(original);
});

it("F09 repeated Git capture does not rewrite shared snapshots or the user's index", async () => {
	const { root, job, task } = await scenario();
	const git = (...args: string[]) => promisify(execFile)("git", ["-C", root, ...args]);
	await git("init");
	await fs.writeFile(join(root, "tracked.txt"), "initial\n");
	await git("add", "tracked.txt");
	await git("-c", "user.name=Astra test", "-c", "user.email=astra@example.invalid", "commit", "-m", "initial");
	await fs.writeFile(join(root, "untracked.txt"), "original snapshot\n");
	await prepareTaskWorkspace(task, job);
	const version = job.state.tasks[task.id].version!;
	if (version.git.status !== "captured") throw new Error("fixture Git capture unavailable");
	const sha = version.git.untracked![0].sha256;
	const blob = join(root, ".astra", "jobs", task.jobId, "versions", "git", sha);
	const original = await fs.readFile(blob);
	const index = (await git("diff", "--cached")).stdout;
	const write = fs.writeFile;
	vi.spyOn(fs, "writeFile").mockImplementation(async (path, ...args) => {
		if (String(path).startsWith(blob)) {
			await write(path, "bad");
			throw new Error("offline partial Git snapshot write");
		}
		return write(path, ...args);
	});
	await prepareTaskWorkspace(task, job);
	expect(await fs.readFile(blob)).toEqual(original);
	expect((await fs.stat(blob)).mode & 0o777).toBe(0o600);
	expect((await git("diff", "--cached")).stdout).toBe(index);
	expect(await fs.readFile(join(root, "untracked.txt"), "utf8")).toBe("original snapshot\n");
});

it.each([0o444, 0o600])(
	"F05/F10 preserves old content after new content write or publication failures (%i)",
	async (mode) => {
		const { root } = await scenario();
		const contentRoot = join(root, "immutable");
		const original = Buffer.from("old registered bytes");
		const sha = await publishImmutableFile(contentRoot, original, mode);
		for (const operation of ["write", "publish"]) {
			const fresh = Buffer.from(`new ${operation} bytes`);
			let fired = false;
			if (operation === "write") {
				const open = fs.open;
				vi.spyOn(fs, "open").mockImplementation(async (path, ...args) => {
					const handle = await open(path, ...args);
					if (String(path).startsWith(contentRoot)) {
						const write = handle.writeFile.bind(handle);
						vi.spyOn(handle, "writeFile").mockImplementation(async () => {
							fired = true;
							await write("partial");
							throw new Error("offline partial CAS write");
						});
					}
					return handle;
				});
			} else {
				vi.spyOn(fs, "link").mockImplementation(async () => {
					fired = true;
					throw new Error("offline CAS publication failure");
				});
			}
			await expect(publishImmutableFile(contentRoot, fresh, mode)).rejects.toThrow(/offline/);
			expect(fired).toBe(true);
			expect(await fs.readdir(contentRoot)).toEqual([sha]);
			expect(await fs.readFile(join(contentRoot, sha))).toEqual(original);
			vi.restoreAllMocks();
		}
	},
);

it.each(["damaged", "symlink"])("F06 refuses a %s shared blob without repairing it", async (kind) => {
	const { root } = await scenario();
	const contentRoot = join(root, "immutable");
	const bytes = Buffer.from("original");
	const sha = await publishImmutableFile(contentRoot, bytes);
	const path = join(contentRoot, sha);
	await fs.rm(path);
	const outside = join(root, "outside.txt");
	await fs.writeFile(outside, "outside bytes");
	if (kind === "damaged") await fs.writeFile(path, "damaged");
	else await fs.symlink(outside, path);
	await expect(publishImmutableFile(contentRoot, bytes)).rejects.toThrow(/integrity/);
	expect(await fs.readFile(path, "utf8")).toBe(kind === "damaged" ? "damaged" : "outside bytes");
	expect(await fs.readFile(outside, "utf8")).toBe("outside bytes");
	expect(await fs.readdir(contentRoot)).toEqual([sha]);
});

it("F07 concurrent same-content publications expose one complete shared blob", async () => {
	const { root } = await scenario();
	const contentRoot = join(root, "immutable");
	const bytes = Buffer.from("same complete immutable content");
	const hashes = await Promise.all(Array.from({ length: 8 }, () => publishImmutableFile(contentRoot, bytes)));
	expect(new Set(hashes).size).toBe(1);
	expect(await fs.readdir(contentRoot)).toEqual([hashes[0]]);
	expect(await fs.readFile(join(contentRoot, hashes[0]))).toEqual(bytes);
	expect((await fs.stat(join(contentRoot, hashes[0]))).mode & 0o777).toBe(0o444);
});

it.each(["write", "publish"])(
	"F10 actual new Git capture %s failure preserves registered blobs and the user's source/index",
	async (operation) => {
		const { root, job, task } = await scenario();
		const git = (...args: string[]) => promisify(execFile)("git", ["-C", root, ...args]);
		await git("init");
		await fs.writeFile(join(root, "tracked.txt"), "initial\n");
		await git("add", "tracked.txt");
		await git("-c", "user.name=Astra test", "-c", "user.email=astra@example.invalid", "commit", "-m", "initial");
		await fs.writeFile(join(root, "old.txt"), "registered original\n");
		await prepareTaskWorkspace(task, job);
		const snapshotRoot = join(root, ".astra", "jobs", task.jobId, "versions", "git");
		const original = new Map(
			await Promise.all(
				(await fs.readdir(snapshotRoot)).map(
					async (name) => [name, await fs.readFile(join(snapshotRoot, name))] as const,
				),
			),
		);
		const index = await fs.readFile(join(root, ".git", "index"));
		await fs.writeFile(join(root, "new.txt"), "new unregistered bytes\n");
		let fired = false;
		if (operation === "write") {
			const open = fs.open;
			vi.spyOn(fs, "open").mockImplementation(async (path, ...args) => {
				const handle = await open(path, ...args);
				if (String(path).startsWith(snapshotRoot)) {
					const write = handle.writeFile.bind(handle);
					vi.spyOn(handle, "writeFile").mockImplementation(async () => {
						fired = true;
						await write("partial");
						throw new Error("offline new Git write failure");
					});
				}
				return handle;
			});
		} else
			vi.spyOn(fs, "link").mockImplementation(async () => {
				fired = true;
				throw new Error("offline new Git publish failure");
			});
		await expect(captureGitVersion(root, snapshotRoot)).rejects.toThrow(/offline new Git/);
		expect(fired).toBe(true);
		expect(await fs.readdir(snapshotRoot)).toEqual([...original.keys()]);
		for (const [name, bytes] of original) expect(await fs.readFile(join(snapshotRoot, name))).toEqual(bytes);
		expect(await fs.readFile(join(root, ".git", "index"))).toEqual(index);
		expect(await fs.readFile(join(root, "new.txt"), "utf8")).toBe("new unregistered bytes\n");
		vi.restoreAllMocks();
		await captureGitVersion(root, snapshotRoot);
		const gitVersion = job.state.tasks[task.id].version!.git;
		if (gitVersion.status !== "captured") throw new Error("fixture Git capture unavailable");
		const oldSha = gitVersion.untracked[0].sha256;
		await fs.writeFile(join(snapshotRoot, oldSha), "corrupted registered bytes");
		await expect(captureGitVersion(root, snapshotRoot)).rejects.toThrow(/integrity/);
		expect(await fs.readFile(join(snapshotRoot, oldSha), "utf8")).toBe("corrupted registered bytes");
	},
);

it.each(["pi", "codex"] as const)("F02/F03 %s actual capacity retry reuses the whole-review task", async (backend) => {
	const f = await lifecycleScenario(backend, DEFAULT_STAGES.find((stage) => stage.id === "research-review")!);
	roots.push(f.root);
	const sources = ["doi:10.1000/review-one", "doi:10.1000/review-two", "doi:10.1000/review-three"];
	for (const sourceRef of sources)
		await writeSourceReceipt(
			{ workspaceRoot: f.root, jobId: f.job.state.frame.jobId, query: "offline review input", limit: 1 },
			{ sourceRef, title: "Recorded offline metadata", authors: [] },
			"offline",
			"2026-10-02T00:00:00Z",
		);
	const upstream = await f.task("declare recorded review inputs");
	await f.job.setTaskStatus(upstream.id, "succeeded");
	const evidence = await f.job.recordEvidence({
		taskId: upstream.id,
		stageId: upstream.stageId,
		type: upstream.requiredOutputType,
		content: {
			verdict: "blocked",
			scientificOutcome: "insufficient-evidence",
			missionCoverage: "insufficient",
			strengths: [],
			weaknesses: [],
			claimAudit: [],
			requiredRepairs: [],
		},
		refs: sources,
	});
	const task = await f.task("whole-research review", [evidence.id]);
	f.control.sources = sources;
	f.control.capacityFailures = 1;
	await f.job.updateBudget({ maxTurns: 1 });
	await new ResearchSupervisor(f.job, f.store, f).tick();
	expect(f.job.state.tasks[task.id].status).toBe("ready");
	expect(f.job.state.budgetUsage!.turnsUsed).toBe(0);
	expect(f.job.state.providerBackoff).toBeDefined();
	f.job = (await ResearchJob.open(f.store, task.jobId))!;
	await f.job.clearProviderBackoff();
	await new ResearchSupervisor(f.job, f.store, f).tick();
	expect(
		f.job.state.tasks[task.id].status,
		JSON.stringify({
			sessions: f.job.state.sessions,
			frame: f.job.state.frame,
			task: f.job.state.tasks[task.id],
			calls: f.calls,
		}),
	).toBe("succeeded");
	expect(f.calls).toEqual([
		{ role: "worker", taskId: task.id },
		{ role: "worker", taskId: task.id },
	]);
	expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
	expect(Object.values(f.job.state.evidence)).toHaveLength(2);
	expect(f.job.state.frame.userGate?.kind).toBe("budget");
});
