import { spawn } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { Type } from "typebox";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
	CodexAppServerRunner,
	type CodexRunOptions,
	CodexRuntimeBudgetError,
	CodexToolBudgetError,
} from "../src/codex-app-server.ts";
import { NonRetryableResearchError, ProviderCapacityError } from "../src/supervisor.ts";

const roots: string[] = [];
const schema = Type.Object({ answer: Type.String() }, { additionalProperties: false });
const fixture = fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url));
const runner = new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] });
async function options(mode = "ok"): Promise<CodexRunOptions<typeof schema>> {
	const cwd = await mkdtemp(join(tmpdir(), "astra-codex-test-"));
	roots.push(cwd);
	return {
		cwd,
		prompt: "test",
		instructions: "test",
		schema,
		logPath: join(cwd, "events.jsonl"),
		maxToolCalls: 2,
		timeoutMs: 5000,
		onThread: async () => {},
		env: { ASTRA_FAKE_CODEX_MODE: mode, ASTRA_FAKE_CODEX_LOG: join(cwd, "requests.jsonl") },
	};
}
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

function barrier() {
	let release = () => {};
	const promise = new Promise<void>((resolve) => {
		release = resolve;
	});
	return { promise, release };
}

it.each(["operator", "runtime", "budget", "disconnect", "failure"])(
	"F03-01/02/06 waits for both started tools on %s termination and keeps the first error",
	async (reason) => {
		const request = await options(`parallel-${reason}`);
		const pidPath = join(request.cwd, "pid");
		const closed = join(request.cwd, "closed");
		request.env = { ...request.env, ASTRA_FAKE_CODEX_PID: pidPath, ASTRA_FAKE_CODEX_CLOSED: closed };
		request.timeoutMs = reason === "runtime" ? 700 : 5000;
		const entered = [barrier(), barrier()];
		const held = [barrier(), barrier()];
		const writes: string[] = [];
		request.tools = [
			{
				name: "audit_tool",
				description: "offline",
				inputSchema: Type.Object({ query: Type.String() }),
				execute: async (input) => {
					const index = Number((input as { query: string }).query);
					entered[index].release();
					await held[index].promise;
					await writeFile(join(request.cwd, `tool-${index}`), "done");
					writes.push(String(index));
					if (reason === "failure" && index === 0) throw new Error("late tool rejection");
					return {};
				},
			},
		];
		let settled = false;
		const running = runner.run(request).then(
			() => {
				settled = true;
				return undefined;
			},
			(error: unknown) => {
				settled = true;
				return error;
			},
		);
		await Promise.all(entered.map((gate) => gate.promise));
		try {
			if (reason === "operator") process.emit("SIGINT");
			else if (reason !== "runtime") process.kill(Number(await readFile(pidPath, "utf8")), "SIGUSR1");
			await vi.waitFor(async () => expect(await readFile(closed, "utf8")).toBe("closed"));
			await new Promise<void>((resolve) => setImmediate(resolve));
			expect(settled).toBe(false);
			held[0].release();
			await vi.waitFor(() => expect(writes).toEqual(["0"]));
			expect(settled).toBe(false);
		} finally {
			for (const gate of held) gate.release();
		}
		const error = await running;
		expect(error).toBeInstanceOf(
			reason === "runtime"
				? CodexRuntimeBudgetError
				: reason === "budget"
					? CodexToolBudgetError
					: NonRetryableResearchError,
		);
		if (reason === "operator") expect((error as Error).message).toContain("interrupted by operator");
		if (reason === "failure") expect((error as Error).message).toContain("controlled connection failure");
		expect(writes).toEqual(["0", "1"]);
		const snapshot = writes.slice();
		await new Promise<void>((resolve) => setImmediate(resolve));
		expect(writes).toEqual(snapshot);
	},
);

it("F03-04 final validation waits for a tool even when turn completion arrives first", async () => {
	const request = await options("early-tool-complete");
	const entered = barrier();
	const held = barrier();
	let toolDone = false;
	const validate = vi.fn(() => expect(toolDone).toBe(true));
	request.tools = [
		{
			name: "audit_tool",
			description: "offline",
			inputSchema: Type.Object({ query: Type.String() }),
			execute: async () => {
				entered.release();
				await held.promise;
				toolDone = true;
				return { receipt: "visible" };
			},
		},
	];
	let settled = false;
	const running = runner.run({ ...request, validateOutput: validate }).finally(() => {
		settled = true;
	});
	void running.catch(() => {});
	await entered.promise;
	try {
		await vi.waitFor(async () =>
			expect(await readFile(request.logPath, "utf8")).toContain('"method":"turn/completed"'),
		);
		await new Promise<void>((resolve) => setImmediate(resolve));
		expect(validate).not.toHaveBeenCalled();
		expect(settled).toBe(false);
	} finally {
		held.release();
	}
	expect((await running).output.answer).toBe("ok");
	expect(validate).toHaveBeenCalledTimes(1);
});

it("F03-03 drains a started web callback and rejects a queued tool after interruption", async () => {
	const request = await options("web-before-tool");
	const entered = barrier();
	const held = barrier();
	const execute = vi.fn(async () => ({}));
	request.webSearch = true;
	request.onWebSearch = async () => {
		entered.release();
		await held.promise;
	};
	request.tools = [
		{ name: "audit_tool", description: "offline", inputSchema: Type.Object({ query: Type.String() }), execute },
	];
	const running = runner.run(request).catch((error: unknown) => error);
	await entered.promise;
	process.emit("SIGINT");
	try {
		expect(execute).not.toHaveBeenCalled();
	} finally {
		held.release();
	}
	expect(await running).toBeInstanceOf(NonRetryableResearchError);
	expect(execute).not.toHaveBeenCalled();
});

it.each(["initialize", "thread/start"])(
	"F03-05 closes pending %s RPC promptly without starting subsequent work",
	async (method) => {
		const request = await options();
		const timers = vi.spyOn(globalThis, "setTimeout");
		const cleared = vi.spyOn(globalThis, "clearTimeout");
		request.env = { ...request.env, ASTRA_FAKE_CODEX_STALL: method };
		const onThread = vi.fn(async () => {});
		request.onThread = onThread;
		request.timeoutMs = 400;
		const before = [process.listenerCount("SIGINT"), process.listenerCount("SIGTERM")];
		await expect(runner.run(request)).rejects.toBeInstanceOf(CodexRuntimeBudgetError);
		const calls = await readFile(join(request.cwd, "requests.jsonl"), "utf8");
		expect(calls).not.toContain('"method":"turn/start"');
		if (method === "initialize") expect(calls).not.toContain('"method":"account/read"');
		expect(onThread).not.toHaveBeenCalled();
		expect([process.listenerCount("SIGINT"), process.listenerCount("SIGTERM")]).toEqual(before);
		const rpcTimers = timers.mock.calls.flatMap((call, index) =>
			call[1] === 30000 ? [timers.mock.results[index].value] : [],
		);
		expect(rpcTimers.length).toBeGreaterThan(0);
		for (const rpcTimer of rpcTimers) expect(cleared.mock.calls.some(([timer]) => timer === rpcTimer)).toBe(true);
	},
	2000,
);

it("F03-05 waits for a started onThread callback after interruption", async () => {
	const request = await options();
	const entered = barrier();
	const held = barrier();
	request.onThread = async () => {
		entered.release();
		await held.promise;
	};
	let settled = false;
	const running = runner
		.run(request)
		.catch((error: unknown) => error)
		.finally(() => {
			settled = true;
		});
	await entered.promise;
	process.emit("SIGINT");
	try {
		await new Promise<void>((resolve) => setImmediate(resolve));
		expect(settled).toBe(false);
	} finally {
		held.release();
	}
	expect(await running).toBeInstanceOf(NonRetryableResearchError);
	expect(await readFile(join(request.cwd, "requests.jsonl"), "utf8")).not.toContain('"method":"turn/start"');
});

it("F03-06 late-close fixture remains alive after stdin EOF until SIGTERM", async () => {
	const request = await options();
	const closed = join(request.cwd, "closed");
	const child = spawn(process.execPath, [fixture], {
		cwd: request.cwd,
		env: { ...request.env, ASTRA_FAKE_CODEX_LATE_CLOSE: "1", ASTRA_FAKE_CODEX_CLOSED: closed },
		stdio: ["pipe", "pipe", "pipe"],
	});
	const exited = new Promise<void>((resolve) => child.once("close", () => resolve()));
	const lines = createInterface({ input: child.stdout });
	const initialized = new Promise<void>((resolve) => lines.once("line", () => resolve()));
	child.stdin.write(`${JSON.stringify({ id: 1, method: "initialize", params: {} })}\n`);
	await initialized;
	child.stdin.end();
	let timer: ReturnType<typeof setTimeout> | undefined;
	try {
		const remainsAlive = await Promise.race([
			exited.then(() => false),
			new Promise<boolean>((resolve) => {
				timer = setTimeout(() => resolve(true), 100);
			}),
		]);
		expect(remainsAlive).toBe(true);
	} finally {
		clearTimeout(timer);
		child.kill("SIGTERM");
		await exited;
		lines.close();
	}
	expect(await readFile(closed, "utf8")).toBe("closed");
});

it("F03-06 awaits ordinary log notifications sent while the server is closing", async () => {
	const request = await options();
	const closed = join(request.cwd, "closed");
	request.env = { ...request.env, ASTRA_FAKE_CODEX_LATE_CLOSE: "1", ASTRA_FAKE_CODEX_CLOSED: closed };
	expect((await runner.run(request)).output.answer).toBe("ok");
	expect(await readFile(closed, "utf8")).toBe("closed");
	const finalLog = await readFile(request.logPath, "utf8");
	expect(finalLog).toContain('"lateClose":true');
	await new Promise<void>((resolve) => setImmediate(resolve));
	expect(await readFile(request.logPath, "utf8")).toBe(finalLog);
});

it.each([false, true])(
	"F03-06 preserves original interruption over source/log rejection and exposes lone rejection (interrupt=%s)",
	async (interrupt) => {
		const request = await options("web-before-tool");
		request.webSearch = true;
		const entered = barrier();
		const held = barrier();
		const execute = vi.fn(async () => ({}));
		request.tools = [
			{ name: "audit_tool", description: "offline", inputSchema: Type.Object({ query: Type.String() }), execute },
		];
		request.onWebSearch = async () => {
			entered.release();
			await held.promise;
			throw new Error("source log failure");
		};
		const running = runner.run(request).catch((error: unknown) => error);
		await entered.promise;
		if (interrupt) process.emit("SIGINT");
		held.release();
		expect(await running).toMatchObject({
			message: interrupt ? "Codex research execution interrupted by operator" : "source log failure",
		});
		expect(execute).not.toHaveBeenCalled();
	},
);

it("F03-07 a budget expiring while a completed turn drains a host tool cannot become success", async () => {
	const request = await options("early-tool-complete");
	const closed = join(request.cwd, "closed");
	request.env = { ...request.env, ASTRA_FAKE_CODEX_CLOSED: closed };
	request.timeoutMs = 600;
	const entered = barrier();
	const held = barrier();
	const validate = vi.fn();
	request.tools = [
		{
			name: "audit_tool",
			description: "offline",
			inputSchema: Type.Object({ query: Type.String() }),
			execute: async () => {
				entered.release();
				await held.promise;
				return {};
			},
		},
	];
	let settled = false;
	const running = runner
		.run({ ...request, validateOutput: validate })
		.catch((error: unknown) => error)
		.finally(() => {
			settled = true;
		});
	await entered.promise;
	try {
		await vi.waitFor(async () => expect(await readFile(closed, "utf8")).toBe("closed"));
		expect(settled).toBe(false);
		expect(validate).not.toHaveBeenCalled();
	} finally {
		held.release();
	}
	expect(await running).toBeInstanceOf(CodexRuntimeBudgetError);
});

describe("official Codex app-server transport", () => {
	it("C7 buffers a legitimate host request before turn/start responds and drains it before validation", async () => {
		const request = await options("pre-response-host");
		const execute = vi.fn(async () => ({ receipt: "current" }));
		request.tools = [
			{ name: "audit_tool", description: "offline", inputSchema: Type.Object({ query: Type.String() }), execute },
		];
		expect(
			(await runner.run({ ...request, validateOutput: () => expect(execute).toHaveBeenCalledOnce() })).output,
		).toEqual({ answer: "ok" });
		expect(execute).toHaveBeenCalledWith({ query: "early" });
	});
	it("C7 retains current flat turn notifications and excludes old flat updates", async () => {
		const request = await options("turn-identity-flat");
		await runner.run(request);
		const updates = (await readFile(request.logPath, "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line))
			.filter((row) => ["turn/diff/updated", "turn/plan/updated"].includes(row.method));
		expect(updates).toHaveLength(2);
		expect(updates.every((row) => row.params.turnId === "turn-1")).toBe(true);
	});
	it.each(["message", "completion", "error", "search", "tool", "started", "pre-response"])(
		"C7 binds correction to its response turn and excludes stale %s notifications",
		async (kind) => {
			const request = await options(`turn-identity-${kind}`);
			const execute = vi.fn(async () => ({}));
			const observe = vi.fn(async () => {});
			request.webSearch = true;
			request.onWebSearch = observe;
			request.maxToolCalls = 0;
			request.tools = [
				{ name: "audit_tool", description: "offline", inputSchema: Type.Object({ query: Type.String() }), execute },
			];
			const validate = vi.fn((output: { answer: string }) => {
				if (output.answer === "draft") throw new Error("correct the draft");
			});
			expect((await runner.run({ ...request, validateOutput: validate })).output).toEqual({ answer: "corrected" });
			expect(validate).toHaveBeenCalledTimes(2);
			expect(execute).not.toHaveBeenCalled();
			expect(observe).not.toHaveBeenCalled();
			if (kind === "started") {
				const events = (await readFile(request.logPath, "utf8"))
					.trim()
					.split("\n")
					.map((line) => JSON.parse(line));
				expect(events.filter((row) => row.method === "turn/started").map((row) => row.params.turn.id)).toEqual([
					"turn-1",
					"turn-2",
				]);
			}
			if (kind === "tool") {
				const calls = (await readFile(join(request.cwd, "requests.jsonl"), "utf8"))
					.trim()
					.split("\n")
					.map((line) => JSON.parse(line));
				expect(calls.find((call) => call.id === "stale-tool")?.error).toMatchObject({ code: -32600 });
			}
		},
	);
	it("bounds final-output correction to one attempt and retains both rejection records", async () => {
		const request = await options();
		const validateOutput = vi.fn(() => {
			throw new Error("confirmation mismatch");
		});
		await expect(runner.run({ ...request, validateOutput })).rejects.toThrow("confirmation mismatch");
		expect(validateOutput).toHaveBeenCalledTimes(2);
		const events = (await readFile(request.logPath, "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		expect(
			events.filter((event) => event.method === "astra/output_rejected").map((event) => event.params.retry),
		).toEqual([true, false]);
	});
	it("shares the original tool budget with the correction turn", async () => {
		const request = await options("web-search");
		await expect(
			runner.run({
				...request,
				maxToolCalls: 1,
				validateOutput: () => {
					throw new Error("confirmation mismatch");
				},
			}),
		).rejects.toThrow("tool-call budget");
	});
	it("does not retry provider failures as final-output corrections", async () => {
		const request = await options("quota");
		const validateOutput = vi.fn();
		await expect(runner.run({ ...request, validateOutput })).rejects.toThrow("allowance exhausted");
		expect(validateOutput).not.toHaveBeenCalled();
		const calls = (await readFile(join(request.cwd, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		expect(calls.filter((call) => call.method === "turn/start")).toHaveLength(1);
	});
	it("reports allowed literal values when a tool reference is invalid", async () => {
		const request = await options("tool");
		const execute = vi.fn(async () => ({}));
		request.tools = [
			{
				name: "audit_tool",
				description: "test",
				inputSchema: Type.Object({ query: Type.Union([Type.Literal("evidence-a"), Type.Literal("evidence-b")]) }),
				execute,
			},
		];
		await runner.run(request);
		expect(execute).not.toHaveBeenCalled();
		const calls = (await readFile(join(request.cwd, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		const feedback = calls.find((call) => call.id === "tool-call")?.result.contentItems[0].text;
		expect(feedback).toContain("/query");
		expect(feedback).toContain("evidence-a");
		expect(feedback).toContain("evidence-b");
	});
	it("awaits host recording of official web observations and excludes foreign threads", async () => {
		const request = await options("web-search");
		const observations: unknown[] = [];
		await runner.run({
			...request,
			webSearch: true,
			onWebSearch: async (item) => {
				await new Promise((resolve) => setTimeout(resolve, 10));
				observations.push(item);
			},
		});
		expect(observations).toHaveLength(1);
		expect(observations[0]).toMatchObject({ id: "web-1", action: { type: "openPage" } });
	});
	it("initializes, requires subscription auth, and resumes the recorded thread", async () => {
		const first = await options();
		const created = await runner.run(first);
		const resumed = await runner.run({ ...first, threadId: created.threadId });
		expect(resumed.output).toEqual({ answer: "ok" });
		expect(resumed.threadId).toBe(created.threadId);
		const log = await readFile(join(first.cwd, "requests.jsonl"), "utf8");
		expect(log).toContain('"method":"thread/resume"');
		expect(log).toContain('"permissions":"astra"');
		expect(log).not.toContain("chatgptAuthTokens");
		expect(log).not.toContain("account/login/start");
		expect(log).not.toContain("apiKey");
	});

	it.each(["logged-out", "api-key"])("rejects %s before requesting model work", async (mode) => {
		const request = await options(mode);
		await expect(runner.run(request)).rejects.toThrow("subscription login");
		expect(await readFile(join(request.cwd, "requests.jsonl"), "utf8")).not.toContain('"method":"turn/start"');
	});

	it("fails closed when Codex does not activate the requested permissions", async () => {
		await expect(runner.run(await options("wrong-permissions"))).rejects.toThrow("permission profile");
	});

	it("rejects an API endpoint override before requesting a subscription turn", async () => {
		const request = await options("api-url");
		await expect(runner.run(request)).rejects.toThrow("openai_base_url");
		expect(await readFile(join(request.cwd, "requests.jsonl"), "utf8")).not.toContain('"method":"thread/start"');
	});

	it("does not forward API credentials or endpoint environment overrides", async () => {
		const request = await options("environment");
		request.env = {
			...request.env,
			OPENAI_API_KEY: "fixture-only",
			CODEX_API_KEY: "fixture-only",
			OPENAI_BASE_URL: "https://example.invalid",
		};
		expect((await runner.run(request)).output.answer).toBe("ok");
	});

	it.runIf(process.platform === "linux")(
		"makes the running executable available inside the restricted filesystem",
		async () => {
			const request = await options();
			await runner.run(request);
			const calls = (await readFile(join(request.cwd, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line) as { method?: string; params?: unknown });
			expect(calls.find((call) => call.method === "thread/start")?.params).toMatchObject({
				config: {
					"permissions.astra.filesystem": { [process.execPath]: "read", [request.cwd]: "read" },
				},
			});
		},
	);

	it("rejects a substituted model before starting a turn", async () => {
		const request = { ...(await options("wrong-model")), model: "gpt-5.6-luna" };
		await expect(runner.run(request)).rejects.toThrow("requested model");
		expect(await readFile(join(request.cwd, "requests.jsonl"), "utf8")).not.toContain('"method":"turn/start"');
	});

	it("disables inherited MCP servers and skills for every role session", async () => {
		const request = await options();
		await runner.run(request);
		const calls = (await readFile(join(request.cwd, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line) as { method?: string; params?: unknown });
		expect(calls.find((call) => call.method === "thread/start")?.params).toMatchObject({
			config: {
				mcp_servers: { ambient: { enabled: false } },
				"skills.config": [{ path: "/ambient/SKILL.md", enabled: false }],
			},
		});
	});

	it.each(["invalid-output", "schema-mismatch"])("rejects %s", async (mode) => {
		const request = await options(mode);
		request.env = { ...request.env, ASTRA_FAKE_CODEX_OUTPUT: '{"unexpected":true}' };
		await expect(runner.run(request)).rejects.toThrow();
	});

	it("pauses on exhausted subscription allowance, but backs off on transient limits", async () => {
		await expect(runner.run(await options("quota"))).rejects.toBeInstanceOf(NonRetryableResearchError);
		await expect(runner.run(await options("rate-limit"))).rejects.toBeInstanceOf(ProviderCapacityError);
	});

	it.each(["retry-error", "fatal-error"])("preserves %s notifications for failure diagnosis", async (mode) => {
		const request = await options(mode);
		if (mode === "fatal-error") await expect(runner.run(request)).rejects.toThrow("fixture stream failure");
		else expect((await runner.run(request)).output.answer).toBe("ok");
		const events = (await readFile(request.logPath, "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line) as { method: string; params: unknown });
		expect(events.filter((event) => event.method === "error")).toEqual([
			expect.objectContaining({
				params: expect.objectContaining({
					willRetry: mode === "retry-error",
					error: { message: "fixture stream failure", codexErrorInfo: "streamDisconnected" },
				}),
			}),
		]);
	});

	it.each(["timeout", "tool-budget", "approval"])("interrupts %s and closes the server", async (mode) => {
		const request = await options(mode);
		if (mode === "timeout") request.timeoutMs = 300;
		await expect(runner.run(request)).rejects.toBeInstanceOf(
			mode === "tool-budget"
				? CodexToolBudgetError
				: mode === "timeout"
					? CodexRuntimeBudgetError
					: NonRetryableResearchError,
		);
	});

	it("does not start model work after interruption during session recording", async () => {
		const request = await options();
		request.onThread = async () => {
			process.emit("SIGINT");
		};
		await expect(runner.run(request)).rejects.toThrow("interrupted by operator");
		expect(await readFile(join(request.cwd, "requests.jsonl"), "utf8")).not.toContain('"method":"turn/start"');
	});

	it.each(["tool", "bad-tool"])("validates %s arguments before invoking host tools", async (mode) => {
		const request = await options(mode);
		const execute = vi.fn(async () => ({ source: "verified" }));
		request.tools = [
			{ name: "audit_tool", description: "test tool", inputSchema: Type.Object({ query: Type.String() }), execute },
		];
		await runner.run(request);
		expect(execute).toHaveBeenCalledTimes(mode === "tool" ? 1 : 0);
		if (mode === "bad-tool") {
			const calls = (await readFile(join(request.cwd, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line));
			expect(calls.find((call) => call.id === "tool-call")?.result).toMatchObject({
				success: false,
				contentItems: [{ type: "inputText", text: expect.stringContaining("/query") }],
			});
		}
	});
});
