import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { writeWorkerOutputManifest } from "../src/contracts.ts";
import { PiChildSessionRunner, PiWorkerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { taskResourcePath } from "../src/task-workspace.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("Pi worker resource environment", () => {
	it("keeps package and model caches under the task resource root", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-worker-resources-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_worker_resources",
			objective: "provision a reusable experiment environment",
			workspaceRoot: root,
			automation: "full",
		});
		const task = await job.dispatchTask({
			stageId: "implement-solution",
			stageExecutionId: "stage_exec_implement-solution",
			agentId: "worker_resources",
			role: "worker",
			objective: "install dependencies and implement the method",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "implement-solution",
			requiredOutputFields: job.definitions["implement-solution"].requiredOutputFields,
			acceptanceChecks: job.definitions["implement-solution"].acceptanceChecks,
			failureSignals: job.definitions["implement-solution"].failureSignals,
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: job.definitions["implement-solution"].workerTools,
			writeAuthority: "workspace-write",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: job.definitions["implement-solution"].acceptanceChecks,
		});
		const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
		let workerEnv: Record<string, string | undefined> = {};
		let workerPrompt = "";
		vi.spyOn(runner, "runTask").mockImplementation(async (runningTask, _role, prompt, env) => {
			workerEnv = env ?? {};
			workerPrompt = prompt;
			await writeWorkerOutputManifest(
				{
					schemaVersion: "astra.worker_output_manifest.v1",
					manifestId: `manifest_${runningTask.id}`,
					jobId: runningTask.jobId,
					taskId: runningTask.id,
					agentId: runningTask.agentId,
					status: "completed",
					artifactType: "implement-solution",
					content: {
						implementation: "implemented",
						files: [],
						tests: [],
						commands: [],
						limitations: [],
					},
					outputRefs: [{ kind: "session", ref: "pi-session:worker-resources", summary: "worker session" }],
					validationStatus: "passed",
					validationErrors: [],
					sessionRef: "pi-session:worker-resources",
					createdAt: new Date().toISOString(),
				},
				runningTask.scope.workspaceRoot,
			);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [] };
		});

		await new PiWorkerAdapter(runner).run(task, job);

		const resourceRoot = taskResourcePath(root, task.jobId, task.id);
		expect(workerEnv).toMatchObject({
			ASTRA_RESOURCE_ROOT: resourceRoot,
			PIP_CACHE_DIR: join(resourceRoot, "cache", "pip"),
			HF_HOME: join(resourceRoot, "cache", "huggingface"),
			TORCH_HOME: join(resourceRoot, "cache", "torch"),
			XDG_CACHE_HOME: join(resourceRoot, "cache"),
		});
		expect(workerPrompt).toContain(
			'create environments, datasets, checkpoints, caches, and other reusable runtime assets under "$ASTRA_RESOURCE_ROOT"',
		);
		expect(workerPrompt).toContain("Do not create a resources/ directory in the task workspace");
	});
});
