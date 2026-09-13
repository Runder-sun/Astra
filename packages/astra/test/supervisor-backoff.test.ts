import { describe, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ProviderCapacityError, ResearchSupervisor } from "../src/supervisor.ts";

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
});
