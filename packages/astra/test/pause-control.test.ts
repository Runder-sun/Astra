import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { runResearchControl } from "../src/research-control.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";

it.each([1, 2])("queues pause during %s locked worker calls and stops before the next model call", async (parallel) => {
	const root = await mkdtemp(join(tmpdir(), "astra-pause-"));
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "pause running work",
		automation: "full",
	});
	await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "running call",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: [],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["verified"],
	});
	if (parallel === 2) await job.dispatchTask({ ...task, id: "task_parallel_pause", replayKey: "parallel_pause" });
	let markStarted!: () => void;
	const started = new Promise<void>((resolve) => {
		markStarted = resolve;
	});
	let finish!: () => void;
	const finished = new Promise<void>((resolve) => {
		finish = resolve;
	});
	let startedCalls = 0;
	const worker = {
		run: vi.fn(async () => {
			if (++startedCalls === parallel) markStarted();
			await finished;
			return { content: { content: "finished" }, refs: [], artifactType: "validation" };
		}),
	};
	const reviewer = { review: vi.fn() };
	const mainAgent = {
		planStage: vi.fn(),
		decideEvidence: vi.fn(),
		decideAdoption: vi.fn(),
		decideSearch: vi.fn(),
		decideRoute: vi.fn(),
	};
	const running = new ResearchSupervisor(job, store, { worker, reviewer, mainAgent }).tick();
	try {
		await started;
		const response = await runResearchControl({ action: "pause", reason: "operator stop" }, root);
		expect(response).toMatchObject({ pauseRequested: true });
		finish();
		await running;
		expect(reviewer.review).not.toHaveBeenCalled();
		expect((await ResearchJob.open(store, job.state.frame.jobId))?.status()).toMatchObject({
			paused: true,
			nextAction: "paused: operator stop",
		});
	} finally {
		finish();
		await running;
		await rm(root, { recursive: true, force: true });
	}
});
