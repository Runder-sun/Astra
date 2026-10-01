import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { ResearchJob } from "../src/research.ts";
import { DEFAULT_STAGES } from "../src/stages.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it("continues planning after a main-agent runtime timeout without refunding consumed work", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-main-timeout-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, { objective: "Recover timed-out planning", workspaceRoot: root });
	const runner = new CodexAppServerRunner({
		executable: process.execPath,
		prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
	});
	const run = runner.run.bind(runner);
	vi.spyOn(runner, "run").mockImplementationOnce((options) =>
		run({ ...options, timeoutMs: 300, env: { ASTRA_FAKE_CODEX_MODE: "timeout" } }),
	);
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
	const adapters = new CodexResearchAdapters(runner);
	const supervisor = new ResearchSupervisor(job, store, { worker: adapters, reviewer: adapters, mainAgent: adapters });
	await supervisor.tick();
	expect(job.state.paused).toBe(false);
	expect(job.state.budgetUsage!.turnsUsed).toBe(1);
	expect(Object.values(job.state.stagePlans)).toHaveLength(0);
	await supervisor.tick();
	expect(Object.values(job.state.stagePlans)).toHaveLength(1);
	const focus = JSON.parse(
		await readFile(
			join(root, ".astra", "jobs", job.state.frame.jobId, "main-agent", "codex-context", "research-focus.json"),
			"utf8",
		),
	);
	expect(focus.fullIndexPath).toBe("research-summary.json");
	expect(focus.repairRequirementsPath).toBe("repair-requirements.json");
	expect(Object.values(job.state.evidence).some((evidence) => evidence.type !== "stage-plan")).toBe(true);
});

it.each([
	[false, false, "tool-budget"],
	[true, false, "tool-budget"],
	[true, true, "tool-budget"],
	[false, false, "timeout"],
	[true, false, "timeout"],
	[true, true, "timeout"],
])(
	"automatically recovers exhaustion with bounded attempts (persistent=%s, globalLimit=%s, mode=%s)",
	async (persistent, globalLimit, mode) => {
		const root = await mkdtemp(join(tmpdir(), "astra-budget-recovery-"));
		roots.push(root);
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			objective: "Recover work without weakening review",
			workspaceRoot: root,
			maxTurns: globalLimit ? 4 : 64,
			definitions: [{ ...DEFAULT_STAGES[0], workerBudget: { maxTurns: 4, maxToolCalls: 2, maxRuntimeMs: 30000 } }],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const runner = new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		});
		const run = runner.run.bind(runner);
		vi.spyOn(runner, "run").mockImplementation((options) =>
			run({ ...options, timeoutMs: process.env.ASTRA_FAKE_CODEX_MODE === "timeout" ? 300 : options.timeoutMs }),
		);
		const adapters = new CodexResearchAdapters(runner);
		let runs = 0;
		const supervisor = new ResearchSupervisor(job, store, {
			mainAgent: adapters,
			reviewer: adapters,
			worker: {
				async run(task, currentJob) {
					vi.stubEnv("ASTRA_FAKE_CODEX_MODE", persistent || runs++ === 0 ? String(mode) : "research");
					try {
						return await adapters.run(task, currentJob);
					} finally {
						vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
					}
				},
			},
		});
		await supervisor.tick();
		expect(job.state.paused).toBe(false);
		const first = Object.values(job.state.tasks).find((task) => task.role === "worker")!;
		expect(first.status).toBe("failed");
		expect(Object.values(job.state.evidence).filter((evidence) => evidence.type !== "stage-plan")).toHaveLength(0);
		const used = job.state.budgetUsage!.turnsUsed;
		const ledger = (id: string) =>
			join(root, ".astra", "jobs", job.state.frame.jobId, "tasks", id, "codex-sources.json");
		await writeFile(ledger(first.id), JSON.stringify(["fixture-retained-source"]));
		await supervisor.tick();
		if (globalLimit) {
			await supervisor.tick();
			expect(job.state.paused).toBe(true);
			expect(job.state.frame.userGate?.kind).toBe("budget");
			expect(job.state.budgetUsage!.turnsUsed).toBeLessThanOrEqual(4);
			return;
		}
		expect(job.state.budgetUsage!.turnsUsed).toBeGreaterThan(used);
		const second = Object.values(job.state.tasks).find((task) => task.supersedesTaskId === first.id)!;
		expect(second.attempt).toBe(2);
		expect(JSON.parse(await readFile(ledger(second.id), "utf8"))).toContain("fixture-retained-source");
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		const firstSession = Object.values(job.state.sessions).find((session) => session.taskId === first.id)!;
		expect(
			calls.some(
				(call) =>
					call.method === "thread/start" &&
					call.params.config["permissions.astra.filesystem"][firstSession.sessionFile!] === "read",
			),
		).toBe(true);
		expect(
			calls.some(
				(call) => call.method === "turn/start" && call.params.input[0].text.includes(firstSession.sessionFile),
			),
		).toBe(true);
		if (persistent) {
			await supervisor.tick();
			expect(job.state.paused).toBe(false);
			await supervisor.tick();
			const workers = Object.values(job.state.tasks).filter((task) => task.role === "worker");
			expect(workers.filter((task) => task.planId === first.planId).map((task) => task.attempt)).toEqual([1, 2, 3]);
			expect(workers.some((task) => task.planId !== first.planId)).toBe(true);
		} else {
			expect(second.status).toBe("succeeded");
			expect(
				Object.values(job.state.reviews).some(
					(review) => job.state.evidence[review.evidenceId]?.taskId === second.id,
				),
			).toBe(true);
		}
	},
);
