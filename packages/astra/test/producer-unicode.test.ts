import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { Type } from "typebox";
import { afterEach, expect, it, vi } from "vitest";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { PiChildSessionRunner, PiWorkerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { ResearchSupervisor } from "../src/supervisor.ts";

const roots: string[] = [];
const fixture = fileURLToPath(new URL("./fixtures/producer-unicode.mjs", import.meta.url));
afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function root() {
	const directory = await mkdtemp(join(tmpdir(), "astra-producer-unicode-"));
	roots.push(directory);
	return directory;
}
const modes = ["split", "ascii", "eof", "cut", "pair", "default"];
it.each(["stdout", "stderr"].flatMap((channel) => modes.map((mode) => ({ channel, mode }))))(
	"Pi public runner $channel preserves $mode text and exit behavior",
	async ({ channel, mode }) => {
		const cwd = await root();
		const small = mode === "cut" || mode === "pair";
		const limit = mode === "default" ? 2 * 1024 * 1024 : 10;
		const runner = new PiChildSessionRunner({ launcherPath: fixture, ...(small ? { maxOutputBytes: limit } : {}) });
		const result = await runner.run(
			cwd,
			"job_probe",
			"task_probe",
			1,
			"worker",
			"offline",
			{
				ASTRA_PRODUCER_MODE: mode,
				ASTRA_PRODUCER_CHANNEL: channel,
				ASTRA_PRODUCER_LIMIT: String(limit),
			},
			10000,
		);
		expect(result.exitCode).toBe(1);
		expect(result.providerError).toBeUndefined();
		expect(result.costUsd).toBe(0);
		const output = channel === "stdout" ? result.stdout : result.stderr;
		if (mode === "default") {
			expect(output.length).toBe(limit - 1);
			expect(output.charCodeAt(0)).toBe(120);
		} else
			expect(output).toBe(
				mode === "split"
					? "中文😀🚀"
					: mode === "ascii"
						? "ASCII diagnostic"
						: mode === "eof"
							? "�"
							: mode === "cut"
								? "x".repeat(9)
								: `😀${"x".repeat(8)}`,
			);
	},
);

it.each(["whole", "split", "ascii", "eof", "cut", "pair"])(
	"Codex public runner retains $0 stderr diagnostic",
	async (mode) => {
		const cwd = await root();
		const runner = new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] });
		const result = await runner
			.run({
				cwd,
				prompt: "offline",
				instructions: "offline",
				schema: Type.Object({}),
				maxToolCalls: 1,
				timeoutMs: 10000,
				logPath: join(cwd, "events.jsonl"),
				env: { ASTRA_PRODUCER_MODE: mode, ASTRA_PRODUCER_LIMIT: "8192" },
				onThread: async () => {
					throw new Error("must not reach inference");
				},
			})
			.catch((error: unknown) => error);
		expect(result).toBeInstanceOf(Error);
		const message = (result as Error).message;
		const prefix = "Codex app-server exited unexpectedly: ";
		expect(message.startsWith(prefix)).toBe(true);
		const output = message.slice(prefix.length);
		if (mode === "cut") {
			expect(output.length).toBe(8191);
			expect(output.charCodeAt(0)).toBe(120);
		} else
			expect(output).toBe(
				mode === "whole" || mode === "split"
					? "中文😀🚀"
					: mode === "ascii"
						? "ASCII diagnostic"
						: mode === "eof"
							? "�"
							: `😀${"x".repeat(8190)}`,
			);
	},
);

it("actual Pi worker failure persists three safe 4096 tails and reopens the failed identity", async () => {
	const cwd = await root();
	const store = new JsonlAstraStore(cwd);
	const job = await ResearchJob.create(store, {
		workspaceRoot: cwd,
		objective: "offline retained error",
		automation: "full",
		maxTurns: 1,
	});
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "offline",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: cwd, allowedPaths: ["."] },
		allowedTools: [],
		writeAuthority: "none",
		budget: { maxTurns: 1, maxToolCalls: 1, maxRuntimeMs: 10000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["verified"],
	});
	vi.stubEnv("ASTRA_PRODUCER_MODE", "failure");
	const unexpected = async () => {
		throw new Error("must not reach model");
	};
	await new ResearchSupervisor(job, store, {
		worker: new PiWorkerAdapter(new PiChildSessionRunner({ launcherPath: fixture })),
		reviewer: { review: unexpected },
		mainAgent: {
			planStage: unexpected,
			decideEvidence: unexpected,
			decideAdoption: unexpected,
			decideSearch: unexpected,
			decideRoute: unexpected,
		},
	}).tick();
	const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
	expect(reopened.state.tasks[task.id].status).toBe("failed");
	const session = Object.values(reopened.state.sessions)[0];
	expect(session).toMatchObject({ role: "worker", taskId: task.id, attempt: 1, status: "failed" });
	expect(session.error).toBe(`😀${"x".repeat(4095)}`);
	const log = JSON.parse(await readFile(session.sessionFile!, "utf8")) as {
		taskId: string;
		attempt: number;
		exitCode: number;
		error: string;
		stdoutTail: string;
		stderrTail: string;
	};
	expect(log).toMatchObject({ taskId: task.id, attempt: 1, exitCode: 1 });
	for (const field of ["error", "stdoutTail", "stderrTail"] as const) expect(log[field]).toBe("x".repeat(4095));
	expect(reopened.status().budget.turnsUsed).toBe(1);
});
