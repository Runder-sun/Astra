import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import {
	PiChildSessionRunner,
	PiMainAgentAdapter,
	PiReviewerAdapter,
	PiWorkerAdapter,
} from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function runScript(source: string, timeout = 2000, cap = 2 * 1024 * 1024) {
	const root = await mkdtemp(join(tmpdir(), "astra-pi-boundary-"));
	roots.push(root);
	const launcherPath = join(root, "fixture.mjs");
	await writeFile(launcherPath, source);
	return new PiChildSessionRunner({ launcherPath, maxOutputBytes: cap }).run(
		root,
		"job_fixture",
		"task_fixture",
		1,
		"worker",
		"offline",
		{},
		timeout,
	);
}
it.each([2, 4])("retains streamed cost before oversized diagnostic output (%s MiB)", async (cap) => {
	const result = await runScript(
		'console.log(JSON.stringify({type:"message_end",message:{role:"assistant",usage:{cost:{total:12}}}})); console.log(JSON.stringify({type:"tool_result",text:"x".repeat(3*1024*1024)}));',
		2000,
		cap * 1024 * 1024,
	);
	expect(result).toMatchObject({ exitCode: 0, costUsd: 12 });
	expect(result.stdout.length).toBeLessThanOrEqual(cap * 1024 * 1024);
});
it("counts chunked UTF-8 and final complete JSON only once", async () => {
	const result = await runScript(
		'const line=Buffer.from(JSON.stringify({type:"message_end",message:{role:"assistant",text:"中文",usage:{cost:{total:3}}}})+"\\n"); for (const byte of line) process.stdout.write(Buffer.from([byte])); process.stdout.write(JSON.stringify({type:"message_end",message:{role:"assistant",usage:{cost:{total:4}}}}));',
	);
	expect(result).toMatchObject({ exitCode: 0, costUsd: 7 });
});
it.each(["timeout", "capacity", "configuration"])("waits for ignored SIGTERM child to close (%s)", async (mode) => {
	const message = mode === "capacity" ? "Provider returned HTTP 429" : "No API key found for anthropic";
	const result = await runScript(
		`process.on("SIGTERM",()=>{}); console.log(JSON.stringify({type:"pid",pid:process.pid})); console.log(JSON.stringify({type:"message_end",message:{role:"assistant",usage:{cost:{total:5}}}})); ${mode === "timeout" ? "" : `console.log(JSON.stringify({type:"message_end",message:{role:"assistant",stopReason:"error",errorMessage:${JSON.stringify(message)}}}));`} setInterval(()=>{},1000);`,
		200,
	);
	const pid = (result.jsonEvents[0] as { pid: number }).pid;
	let alive = false;
	try {
		process.kill(pid, 0);
		alive = true;
	} catch {
		/* exited */
	}
	if (alive) process.kill(pid, "SIGKILL");
	expect(alive).toBe(false);
	expect(result).toMatchObject({ exitCode: mode === "timeout" ? 124 : 1, costUsd: 5 });
	if (mode !== "timeout") expect(result).toMatchObject({ providerError: { kind: mode } });
});

it.each(["worker", "reviewer", "planning", "decision"])(
	"records independent streamed facts in the %s adapter",
	async (role) => {
		const root = await mkdtemp(join(tmpdir(), "astra-pi-adapter-cost-"));
		roots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), { workspaceRoot: root, objective: "offline facts" });
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "evidence",
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
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: "validation",
			type: "validation",
			content: { content: "facts" },
			refs: [],
		});
		const runner = new PiChildSessionRunner();
		vi.spyOn(runner, "run").mockResolvedValue({
			exitCode: 1,
			stdout: "",
			stderr: "failure",
			jsonEvents: [],
			costUsd: 12,
			providerError: { kind: "capacity", message: "Provider returned HTTP 429" },
		});
		const main = new PiMainAgentAdapter(runner, root);
		const run =
			role === "worker"
				? () => new PiWorkerAdapter(runner).run(task, job)
				: role === "reviewer"
					? () => new PiReviewerAdapter(runner).review(evidence, job)
					: role === "planning"
						? () => main.planStage(job)
						: () => main.decideEvidence(evidence, job);
		await expect(run()).rejects.toThrow("429");
		expect(job.state.budgetUsage?.costUsdUsed).toBe(12);
	},
);
