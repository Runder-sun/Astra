import { describe, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ProviderCapacityError, ResearchSupervisor } from "../src/supervisor.ts";
import { reviewFixture } from "./review-fixture.ts";

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
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			workspaceRoot: "/tmp",
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
			scope: { workspaceRoot: "/tmp", allowedPaths: ["."] },
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
		const reviewInput = {
			...task,
			id: "task_reused_reviewer",
			role: "reviewer" as const,
			replayKey: "retry_review",
			inputArtifactRefs: [evidence.id],
		};
		await job.dispatchTask(reviewInput);
		if (!reuse) await job.setTaskStatus(reviewInput.id, "failed");
		const reviewer = {
			review: vi.fn(async () => {
				const reused = await job.dispatchTask(reviewInput);
				return reviewFixture(job, {
					evidenceId: evidence.id,
					reviewerTaskId: reused.id,
					verdict: "pass",
					findings: [],
				});
			}),
		};
		const mainAgent = {
			planStage: vi.fn(),
			decideEvidence: vi.fn().mockResolvedValue({ decision: "defer" }),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn(),
		};
		await new ResearchSupervisor(job, store, { worker: { run: vi.fn() }, reviewer, mainAgent }).tick();
		expect(reviewer.review).toHaveBeenCalledOnce();
		expect(Object.values(job.state.reviews)).toHaveLength(reuse ? 1 : 0);
		if (reuse) expect(job.state.frame.userGate).toBeUndefined();
		else {
			expect(job.state.frame.userGate).toMatchObject({ kind: "budget", limit: "maxTasks" });
			expect(job.status().budget.turnsUsed).toBe(0);
		}
		expect(Object.keys(job.state.tasks)).toHaveLength(2);
	});
});
