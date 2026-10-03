import { readFile, rm, writeFile } from "node:fs/promises";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";
import { lifecycleScenario } from "./lifecycle-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function scenario(backend: "pi" | "codex") {
	const f = await lifecycleScenario(backend);
	roots.push(f.root);
	f.control.allowPlanning = true;
	return f;
}
it.each(["pi", "codex"] as const)(
	"T13 %s abandons a valid old saved plan and advances new planning after JSONL reopen",
	async (backend) => {
		const f = await scenario(backend);
		const plan = await f.mainAgent.planStage(f.job);
		const original = f.job.state.mainAgentCalls![plan.decisionRef];
		const bytes = await readFile(original.manifestRef);
		await f.job.reopenStage(plan.stageId, "new_revision", "legal new planning basis");
		f.job = (await ResearchJob.open(f.store, plan.jobId))!;
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state.mainAgentCalls![original.id]).toMatchObject({
			...JSON.parse(JSON.stringify(original)),
			abandoned: true,
			completed: false,
		});
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		expect(await readFile(original.manifestRef)).toEqual(bytes);
		await f.job.updateBudget({ maxTurns: 2 });
		await new ResearchSupervisor(f.job, f.store, f).tick();
		expect(f.calls.filter((call) => call.role === "main-agent")).toHaveLength(2);
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(1);
		expect(f.job.state.stagePlans[plan.id]).toBeUndefined();
		f.job = (await ResearchJob.open(f.store, plan.jobId))!;
		await f.job.recoverMainAgentDeliveries();
		expect(f.calls.filter((call) => call.role === "main-agent")).toHaveLength(2);
	},
);
it.each(["pi", "codex"] as const)(
	"T13 %s yields a returned old plan without applying it or installing infrastructure pause",
	async (backend) => {
		const f = await scenario(backend);
		await f.job.updateBudget({ maxTurns: 3 });
		f.control.beforeMainReturn = async () => {
			f.control.beforeMainReturn = undefined;
			await f.job.reopenStage("validation", "during_call", "legal basis change during runner");
		};
		await new ResearchSupervisor(f.job, f.store, f).tick();
		const call = Object.values(f.job.state.mainAgentCalls!)[0];
		expect(call).toMatchObject({ abandoned: true, completed: false });
		expect(call.applied).not.toBe(true);
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		expect(f.job.state.paused).toBe(false);
		expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
		await new ResearchSupervisor(f.job, f.store, f).tick();
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(1);
		expect(f.calls.filter((entry) => entry.role === "main-agent")).toHaveLength(2);
	},
);
it.each(["append", "snapshot"] as const)(
	"T13 abandon %s failure retains durable identity and never repeats the old runner",
	async (fault) => {
		const f = await scenario("pi");
		const plan = await f.mainAgent.planStage(f.job);
		await f.job.reopenStage(plan.stageId, "new_revision", "legal basis change");
		const append = f.store.append.bind(f.store);
		let fired = false;
		vi.spyOn(f.store, "append").mockImplementation(async (jobId, event) => {
			if (event.type === "main_agent_call_finished" && event.abandoned && !fired) {
				fired = true;
				if (fault === "append") throw new Error("injected abandon append");
				vi.spyOn(f.store, "writeSnapshot").mockRejectedValueOnce(new Error("injected abandon snapshot"));
			}
			return append(jobId, event);
		});
		await expect(f.job.recoverMainAgentDeliveries()).rejects.toThrow(`injected abandon ${fault}`);
		expect(fired).toBe(true);
		f.job = (await ResearchJob.open(f.store, plan.jobId))!;
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state.mainAgentCalls![plan.decisionRef]).toMatchObject({ abandoned: true, completed: false });
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		expect(f.calls).toHaveLength(1);
	},
);
it.each(["identity", "digest", "missing", "prefix"] as const)(
	"T13 stale basis never hides invalid %s",
	async (fault) => {
		const f = await scenario("pi");
		const plan = await f.mainAgent.planStage(f.job);
		const call = f.job.state.mainAgentCalls![plan.decisionRef];
		await f.job.reopenStage(plan.stageId, "new_revision", "legal basis change");
		if (fault === "missing") await rm(call.manifestRef);
		else if (fault === "prefix") vi.spyOn(f.store, "readEvents").mockResolvedValue([]);
		else {
			const manifest = JSON.parse(await readFile(call.manifestRef, "utf8"));
			if (fault === "identity") manifest.jobId = "foreign_job";
			else manifest.rationale = "changed registered bytes";
			await writeFile(call.manifestRef, JSON.stringify(manifest));
		}
		await expect(f.job.recoverMainAgentDeliveries()).rejects.toThrow(
			fault === "identity"
				? /identity|schema/
				: fault === "digest"
					? /digest/
					: fault === "prefix"
						? /prefix/
						: /ENOENT/,
		);
		expect(f.job.state.mainAgentCalls![call.id].abandoned).not.toBe(true);
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		expect(f.calls).toHaveLength(1);
	},
);

it.each(["ordinary", "research", "stage", "budget"] as const)(
	"T13 %s pause preserves exact permission before abandoning old work",
	async (gate) => {
		const f = await scenario("pi");
		const plan = await f.mainAgent.planStage(f.job);
		await f.job.reopenStage(plan.stageId, "new_revision", "legal basis change");
		if (gate === "ordinary") await f.job.pause("exact original ordinary pause");
		else if (gate === "research")
			await f.job.requireUserGate({
				kind: "research",
				stageId: plan.stageId,
				question: "exact question",
				reason: "exact reason",
			});
		else if (gate === "stage")
			await f.job.requireUserGate({ kind: "stage", stageId: plan.stageId, phase: "route", reason: "exact reason" });
		else
			await f.job.requireUserGate({
				kind: "budget",
				stageId: plan.stageId,
				limit: "maxTurns",
				reason: "exact reason",
			});
		f.job = (await ResearchJob.open(f.store, plan.jobId))!;
		const before = f.job.state.frame;
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state.frame).toEqual(before);
		expect(f.job.state.mainAgentCalls![plan.decisionRef].abandoned).not.toBe(true);
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(0);
		if (gate === "research") await f.job.resumeWithGuidance("exact answer to original gate");
		else await f.job.resume();
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state.mainAgentCalls![plan.decisionRef].abandoned).toBe(true);
		expect(f.calls).toHaveLength(1);
	},
);

it.each(["pi", "codex"] as const)(
	"T13 %s completes the applied call before abandoning a later pending call",
	async (backend) => {
		const f = await scenario(backend);
		const first = await f.mainAgent.planStage(f.job);
		const second = await f.mainAgent.planStage(f.job);
		await f.job.recordStagePlan(first);
		f.job = (await ResearchJob.open(f.store, first.jobId))!;
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state.mainAgentCalls![first.decisionRef]).toMatchObject({ applied: true, completed: true });
		expect(f.job.state.mainAgentCalls![first.decisionRef].abandoned).not.toBe(true);
		expect(f.job.state.mainAgentCalls![second.decisionRef]).toMatchObject({ abandoned: true, completed: false });
		expect(f.job.state.mainAgentCalls![second.decisionRef].applied).not.toBe(true);
		expect(Object.values(f.job.state.stagePlans)).toHaveLength(1);
		expect(f.calls).toHaveLength(2);
		const seq = f.job.state.eventSeq;
		await f.job.recoverMainAgentDeliveries();
		expect(f.job.state.eventSeq).toBe(seq);
	},
);
