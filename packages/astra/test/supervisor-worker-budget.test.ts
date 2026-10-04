import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { ProviderCapacityError, ResearchSupervisor } from "../src/supervisor.ts";
import type { TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function setup(approved: boolean, maxTurns: number, maxTasks = 20) {
	const root = await mkdtemp(join(tmpdir(), "astra-worker-budget-"));
	roots.push(root);
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "bounded workers",
		automation: "full",
		maxTasks: 20,
		maxTurns: 64,
	});
	if (approved) {
		const plan = await job.recordStagePlan({
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "budget-plan",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "offline-decision",
			mode: "decompose",
			tasks: ["a", "b"].map((key) => ({
				key,
				objective: key,
				inputArtifactRefs: [],
				requiredOutputFields: job.definitions.validation.requiredOutputFields,
				acceptanceChecks: [],
				failureSignals: [],
				successCriteria: [],
			})),
			rationale: "offline",
			sessionRef: "fixture",
			createdAt: new Date().toISOString(),
		});
		const evidence = await preparePlanEvidence(job, plan);
		await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	} else {
		for (const id of ["a", "b"])
			await job.dispatchTask({
				id,
				replayKey: id,
				stageId: "validation",
				stageExecutionId: "validation",
				role: "worker",
				objective: id,
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
	}
	await job.updateBudget({ maxTurns, maxTasks });
	const worker = {
		run: vi.fn(async (_task: TaskPacket) => ({ content: { content: "done" }, refs: [], artifactType: "validation" })),
	};
	const reviewer = {
		review: vi.fn(async () => {
			throw new Error("offline reviewer interruption");
		}),
	};
	const unexpected = vi.fn(async () => {
		throw new Error("unexpected main agent");
	});
	const supervisor = new ResearchSupervisor(job, store, {
		worker,
		reviewer,
		maxParallel: 2,
		mainAgent: {
			planStage: unexpected,
			decideEvidence: unexpected,
			decideAdoption: unexpected,
			decideSearch: unexpected,
			decideRoute: unexpected,
		},
	});
	return { job, store, worker, reviewer, supervisor };
}

it.each([false, true])("limits worker batch to one remaining turn (approved=%s)", async (approved) => {
	const f = await setup(approved, 1);
	await f.supervisor.tick();
	expect(f.worker.run).toHaveBeenCalledOnce();
	expect(f.job.status().budget.turnsUsed).toBe(1);
	expect(
		Object.values(f.job.state.tasks).filter((task) => task.role === "worker" && task.status === "ready"),
	).toHaveLength(1);
	expect(f.reviewer.review).not.toHaveBeenCalled();
});

it.each([false, true])("zero remaining turns gates the next single worker (approved=%s)", async (approved) => {
	const f = await setup(approved, 1);
	await f.job.consumeTurns(1);
	await f.supervisor.tick();
	expect(f.worker.run).not.toHaveBeenCalled();
	expect(f.job.state.frame.userGate).toMatchObject({ kind: "budget", limit: "maxTurns", requiredMinimum: 2 });
});

it.each([false, true])("retains two-worker parallelism with enough turns (approved=%s)", async (approved) => {
	const f = await setup(approved, 2);
	await f.supervisor.tick();
	expect(f.worker.run).toHaveBeenCalledTimes(2);
	expect(f.job.status().budget.turnsUsed).toBe(2);
	expect(f.job.state.frame.userGate).toMatchObject({ limit: "maxTurns", requiredMinimum: 3 });
});

it("runs existing ready tasks at the task limit but registers a new plan atomically", async () => {
	const ready = await setup(false, 2, 2);
	await ready.supervisor.tick();
	expect(ready.worker.run).toHaveBeenCalledTimes(2);
	const planned = await setup(true, 2, 2);
	await planned.supervisor.tick();
	expect(planned.worker.run).not.toHaveBeenCalled();
	expect(planned.job.status().budget.tasksUsed).toBe(1);
	expect(planned.job.state.frame.userGate).toMatchObject({ limit: "maxTasks", requiredMinimum: 3 });
});

it("refunds only the selected capacity failure and respects backoff", async () => {
	const f = await setup(false, 1);
	f.worker.run.mockRejectedValue(new ProviderCapacityError("offline capacity"));
	await f.supervisor.tick();
	expect(f.worker.run).toHaveBeenCalledOnce();
	expect(f.job.status().budget.turnsUsed).toBe(0);
	expect(f.job.state.providerBackoff).toBeDefined();
	await f.supervisor.tick();
	expect(f.worker.run).toHaveBeenCalledOnce();
});

it("recovers durable completion before gating without repeating or refunding its turn", async () => {
	const f = await setup(false, 1);
	await f.job.consumeTurns(1);
	const write = f.store.writeSnapshot.bind(f.store);
	let fail = true;
	f.store.writeSnapshot = async (snapshot) => {
		if (fail && Object.keys(snapshot.evidence).length) {
			fail = false;
			throw new Error("after durable append");
		}
		return write(snapshot);
	};
	await expect(
		f.job.completeWorkerTask("a", { content: { content: "done" }, refs: [], artifactType: "validation" }),
	).rejects.toThrow("after durable append");
	await f.supervisor.tick();
	expect(f.worker.run).not.toHaveBeenCalled();
	expect(f.job.state.tasks.a.status).toBe("succeeded");
	expect(Object.keys(f.job.state.evidence)).toHaveLength(1);
	expect(f.job.status().budget.turnsUsed).toBe(1);
});

it("preserves historical minimum and rejects a malformed budget gate", async () => {
	const f = await setup(false, 1);
	await f.job.requireUserGate({
		kind: "budget",
		stageId: "validation",
		limit: "maxTurns",
		requiredMinimum: 2,
		reason: "historical batch",
	});
	await expect(f.job.resume()).rejects.toThrow();
	expect(f.job.state.frame.userGate).toMatchObject({ kind: "budget", requiredMinimum: 2 });
	await f.job.updateBudget({ maxTurns: 2 });
	await f.job.resume();
	expect(f.job.state.paused).toBe(false);
	await f.job.requireUserGate({ kind: "budget", stageId: "validation", limit: "maxTurns", reason: "malformed" });
	await expect(f.job.resume()).rejects.toThrow();
});

it("keeps the exhausted cost gate ahead of worker dispatch", async () => {
	const f = await setup(false, 2);
	await f.job.updateBudget({ maxCostUsd: 0 });
	await f.supervisor.tick();
	expect(f.worker.run).not.toHaveBeenCalled();
	expect(f.job.state.frame.userGate).toMatchObject({ kind: "budget", limit: "maxCostUsd" });
	expect(f.job.status().budget.turnsUsed).toBe(0);
});
