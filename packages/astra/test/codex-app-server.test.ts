import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { Type } from "typebox";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CodexAppServerRunner, type CodexRunOptions } from "../src/codex-app-server.ts";
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
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("official Codex app-server transport", () => {
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
		await expect(runner.run(request)).rejects.toBeInstanceOf(NonRetryableResearchError);
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
	});
});
