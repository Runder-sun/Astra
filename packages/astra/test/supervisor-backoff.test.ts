import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { writeReviewerOutputManifest } from "../src/contracts.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ProviderCapacityError, ResearchSupervisor } from "../src/supervisor.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("provider backoff across research phases", () => {
	it("stops provider calls in the same tick after a capacity failure", async () => {
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			workspaceRoot: "/tmp/astra-backoff-test",
			objective: "respect shared provider capacity",
			automation: "full",
		});
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "pending review",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["content verified"],
			failureSignals: ["missing content"],
			dependencies: [],
			scope: { workspaceRoot: "/tmp/astra-backoff-test", allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["content verified"],
		});
		await job.setTaskStatus(task.id, "succeeded");
		await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { content: "ready for review" },
			refs: ["codex-session:seed"],
		});
		const ready = await job.dispatchTask({ ...task, id: "capacity_worker", replayKey: "capacity_worker" });
		const worker = { run: vi.fn().mockRejectedValue(new ProviderCapacityError("temporarily overloaded")) };
		const reviewer = { review: vi.fn().mockResolvedValue({ verdict: "pass", findings: [] }) };
		const mainAgent = {
			planStage: vi.fn(),
			decideEvidence: vi.fn(),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn(),
		};
		const supervisor = new ResearchSupervisor(job, store, { worker, reviewer, mainAgent, maxParallel: 1 });
		await supervisor.tick();
		expect(reviewer.review).not.toHaveBeenCalled();
		expect(job.state.tasks[ready.id].status).toBe("ready");
		expect(job.state.providerBackoff?.reason).toBe("temporarily overloaded");
		expect(job.state.budgetUsage?.turnsUsed).toBe(0);
		await supervisor.tick();
		expect(worker.run).toHaveBeenCalledOnce();
		expect(reviewer.review).not.toHaveBeenCalled();
	});

	it.each([true, false])("checks the task limit only when the reviewer cannot be reused (reuse=%s)", async (reuse) => {
		const root = await mkdtemp(join(tmpdir(), "astra-backoff-review-"));
		roots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			workspaceRoot: root,
			objective: "reuse reviewer",
			maxTasks: 2,
			automation: "full",
		});
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "pending review",
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
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			refs: [],
			content: { content: "ready" },
		});
		const runner = new PiChildSessionRunner();
		const run = vi
			.spyOn(runner, "run")
			.mockImplementation(async (_cwd, jobId, taskId, _attempt, _role, _prompt, env = {}) => {
				const refs = [`evidence:${evidence.id}`];
				await writeReviewerOutputManifest(
					{
						schemaVersion: "astra.reviewer_output_manifest.v1",
						manifestId: `manifest-${taskId}`,
						jobId,
						taskId,
						evidenceId: evidence.id,
						verdict: "pass",
						score: 1,
						findings: [],
						criteria: (JSON.parse(env.ASTRA_REVIEW_CRITERIA!) as string[]).map((criterion) => ({
							criterion,
							passed: true,
							score: 1,
							evidenceRefs: refs,
							rationale: "offline",
						})),
						verifiedRefs: refs,
						sessionRef: "offline",
						createdAt: new Date().toISOString(),
					},
					root,
				);
				return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [], costUsd: 0 };
			});
		run.mockResolvedValueOnce({
			exitCode: 1,
			stdout: "",
			stderr: "first execution interrupted",
			jsonEvents: [],
			costUsd: 0,
			...(reuse ? { providerError: { kind: "capacity" as const, message: "temporarily overloaded" } } : {}),
		});
		const reviewer = new PiReviewerAdapter(runner);
		await expect(reviewer.review(evidence, job)).rejects.toThrow();
		const review = vi.spyOn(reviewer, "review");
		const mainAgent = {
			planStage: vi.fn(),
			decideEvidence: vi.fn().mockResolvedValue({ decision: "defer" }),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn(),
		};
		await new ResearchSupervisor(job, store, { worker: { run: vi.fn() }, reviewer, mainAgent }).tick();
		expect(review).toHaveBeenCalledOnce();
		expect(Object.values(job.state.reviews)).toHaveLength(reuse ? 1 : 0);
		if (reuse) expect(job.state.frame.userGate).toBeUndefined();
		else {
			expect(job.state.frame.userGate).toMatchObject({ kind: "budget", limit: "maxTasks" });
			expect(job.status().budget.turnsUsed).toBe(0);
		}
		expect(Object.keys(job.state.tasks)).toHaveLength(2);
	});
});
