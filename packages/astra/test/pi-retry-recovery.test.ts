import { access, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { taskDir, workerManifestPath } from "../src/contracts.ts";
import { PiChildSessionRunner, PiWorkerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { prepareTaskWorkspace, taskRecoveryMaterials, taskWorkspacePath } from "../src/task-workspace.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it("retains partial Pi work and a readable failure log for the next attempt", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-pi-retry-"));
	roots.push(root);
	const job = await ResearchJob.create(new MemoryAstraStore(), {
		workspaceRoot: root,
		objective: "resume partial work",
	});
	const first = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "deliver partial result",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		successCriteria: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read", "write"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	const runner = new PiChildSessionRunner();
	let firstRoot = "";
	let context = "";
	let prompt = "";
	let recoveryRoots: string[] = [];
	vi.spyOn(runner, "runTask").mockImplementation(async (task, _role, text, env = {}) => {
		const cwd = env.ASTRA_EXECUTION_ROOT!;
		if (task.id === first.id) {
			firstRoot = cwd;
			await writeFile(join(cwd, "partial.json"), '{"verified":42}');
		} else {
			context = await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8");
			prompt = text;
			recoveryRoots = JSON.parse(env.ASTRA_RECOVERY_READ_ROOTS ?? "[]");
		}
		return { exitCode: 124, stdout: "partial tool completed", stderr: "fixture timeout", jsonEvents: [], costUsd: 0 };
	});
	const adapter = new PiWorkerAdapter(runner);
	await expect(adapter.run(first, job)).rejects.toThrow("124");
	await job.setTaskStatus(first.id, "failed");
	const second = await job.dispatchTask({ ...first, id: "task_retry", attempt: 2, supersedesTaskId: first.id });
	await expect(adapter.run(second, job)).rejects.toThrow("124");
	await expect(access(join(firstRoot, "partial.json"))).resolves.toBeUndefined();
	expect(context).toContain(firstRoot);
	expect(prompt).toContain("unreviewed recovery");
	expect(recoveryRoots).toContain(firstRoot);
	const failed = Object.values(job.state.sessions).find((session) => session.taskId === first.id)!;
	expect(failed.sessionFile).toBeDefined();
	await expect(readFile(failed.sessionFile!, "utf8")).resolves.toContain("fixture timeout");
});

async function recoveryFixture() {
	const root = await mkdtemp(join(tmpdir(), "astra-recovery-validation-"));
	roots.push(root);
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "validate retry lineage" });
	const prior = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "same bounded task",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		successCriteria: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 100 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	await prepareTaskWorkspace(prior, job);
	await job.setTaskStatus(prior.id, "failed");
	const retry = await job.dispatchTask({ ...prior, id: "retry_validation", attempt: 2, supersedesTaskId: prior.id });
	return { root, store, job, prior, retry };
}

it.each(["wrong-job", "wrong-predecessor", "forged-log", "symlink-root", "symlink-log", "changed-contract"] as const)(
	"rejects %s retry recovery",
	async (change) => {
		const { root, store, job, prior, retry } = await recoveryFixture();
		const snapshot = job.state;
		const previousRoot = taskWorkspacePath(root, prior.jobId, prior.id);
		if (change === "wrong-job") snapshot.tasks[prior.id].jobId = "another-job";
		if (change === "wrong-predecessor") snapshot.tasks[retry.id].supersedesTaskId = "unrelated-task";
		if (change === "changed-contract") snapshot.tasks[prior.id].acceptanceChecks = ["different criterion"];
		if (change === "symlink-root") {
			await rm(previousRoot, { recursive: true });
			await symlink(root, previousRoot);
		}
		if (change === "forged-log" || change === "symlink-log") {
			const log = join(taskDir(root, prior.jobId, prior.id), "failure-log.json");
			if (change === "symlink-log") await symlink(join(root, "secret"), log);
			snapshot.sessions.fixture = {
				sessionId: "fixture",
				taskId: prior.id,
				role: "worker",
				attempt: prior.attempt,
				status: "failed",
				sessionFile: change === "forged-log" ? join(root, "other-job-log.jsonl") : log,
				updatedAt: "now",
			};
		}
		await store.writeSnapshot(snapshot);
		const changed = (await ResearchJob.open(store, snapshot.frame.jobId))!;
		await expect(taskRecoveryMaterials(retry, changed)).rejects.toThrow(/same-job|same job|symbolic/);
	},
);

it("treats a missing old workspace and log as unavailable recovery material", async () => {
	const { root, job, prior, retry } = await recoveryFixture();
	await rm(taskWorkspacePath(root, prior.jobId, prior.id), { recursive: true });
	await job.recordChildSession({
		sessionId: "fixture",
		taskId: prior.id,
		role: "worker",
		attempt: prior.attempt,
		status: "failed",
		sessionFile: join(taskDir(root, prior.jobId, prior.id), "failure-log.json"),
		updatedAt: "now",
	});
	expect((await taskRecoveryMaterials(retry, job))?.readRoots).toEqual([]);
	await expect(prepareTaskWorkspace(retry, job)).resolves.toBe(taskWorkspacePath(root, retry.jobId, retry.id));
});

it("clears the current output manifest before a retry session starts", async () => {
	const { root, retry } = await recoveryFixture();
	const runner = new PiChildSessionRunner();
	const manifest = workerManifestPath(root, retry.jobId, retry.id);
	await mkdir(taskDir(root, retry.jobId, retry.id), { recursive: true });
	await writeFile(manifest, "old-manifest");
	vi.spyOn(runner, "run").mockImplementation(async () => {
		await expect(access(manifest)).rejects.toThrow();
		return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
	});
	await runner.runTask(retry, "worker", "fixture");
});

it("rejects a retained predecessor manifest submitted as the current retry result", async () => {
	const { root, job, prior, retry } = await recoveryFixture();
	const runner = new PiChildSessionRunner();
	vi.spyOn(runner, "runTask").mockImplementation(async () => {
		const manifest = workerManifestPath(root, retry.jobId, retry.id);
		await writeFile(
			manifest,
			JSON.stringify({
				schemaVersion: "astra.worker_output_manifest.v1",
				manifestId: "old",
				jobId: prior.jobId,
				taskId: prior.id,
				agentId: prior.agentId,
				status: "completed",
				artifactType: prior.requiredOutputType,
				content: { content: "old" },
				outputRefs: [{ kind: "log", ref: "session:fixture" }],
				validationStatus: "passed",
				validationErrors: [],
				sessionRef: "fixture",
				createdAt: "now",
			}),
		);
		return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
	});
	await expect(new PiWorkerAdapter(runner).run(retry, job)).rejects.toThrow("identity");
});

it("reuses a real Pi session file instead of replacing it with a fallback log", async () => {
	const { root, job, retry: prior } = await recoveryFixture();
	const sessionId = `astra-${prior.jobId}-${prior.id}-${prior.attempt}`;
	const sessionDir = join(root, ".astra", "jobs", prior.jobId, "sessions");
	await mkdir(sessionDir, { recursive: true });
	const transcript = join(sessionDir, `fixture_${sessionId}.jsonl`);
	await writeFile(transcript, '{"type":"tool-result","value":42}');
	const runner = new PiChildSessionRunner();
	vi.spyOn(runner, "runTask").mockResolvedValue({
		exitCode: 124,
		stdout: "",
		stderr: "timeout",
		jsonEvents: [],
		costUsd: 0,
	});
	await expect(new PiWorkerAdapter(runner).run(prior, job)).rejects.toThrow("124");
	expect(Object.values(job.state.sessions).find((session) => session.taskId === prior.id)?.sessionFile).toBe(
		transcript,
	);
	await expect(readFile(transcript, "utf8")).resolves.toContain('"value":42');
});
