import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { appendFile, mkdir, readFile, readlink } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { createInterface } from "node:readline";
import type { Static, TSchema } from "typebox";
import { Value } from "typebox/value";
import { classifyProviderErrorMessage } from "./provider-errors.ts";
import { NonRetryableResearchError, ProviderCapacityError } from "./supervisor.ts";

export interface CodexTool {
	name: string;
	description: string;
	inputSchema: TSchema;
	execute(input: unknown): Promise<unknown>;
}

export interface CodexRunOptions<S extends TSchema> {
	cwd: string;
	prompt: string;
	instructions: string;
	schema: S;
	threadId?: string;
	model?: string;
	readRoots?: string[];
	writeRoots?: string[];
	network?: boolean;
	webSearch?: boolean;
	onWebSearch?(item: Record<string, unknown>): Promise<void>;
	env?: Record<string, string>;
	tools?: CodexTool[];
	maxToolCalls: number;
	timeoutMs: number;
	logPath: string;
	onThread(threadId: string): Promise<void>;
}

export interface CodexRunResult<T> {
	output: T;
	model: string;
	threadId: string;
	sessionFile: string;
}

export function codexError(message: string, info?: unknown): Error {
	// Subscription exhaustion needs explicit operator resume, not a five-minute retry loop.
	if (info === "usageLimitExceeded" || /usage limit|quota exceeded|credits? exhausted/i.test(message)) {
		return new NonRetryableResearchError(
			`Codex subscription allowance exhausted; resume after it resets: ${message}`,
		);
	}
	if (
		["rateLimitExceeded", "serverOverloaded"].includes(String(info)) ||
		classifyProviderErrorMessage(message) === "capacity"
	) {
		return new ProviderCapacityError(message);
	}
	return new NonRetryableResearchError(message);
}

function record(value: unknown): Record<string, unknown> {
	return value !== null && typeof value === "object" && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: {};
}

class AppServerConnection {
	private readonly child: ChildProcessWithoutNullStreams;
	private readonly pending = new Map<number, { resolve(value: unknown): void; reject(error: Error): void }>();
	private sequence = 0;
	private closed = false;
	private stderr = "";
	private readonly exited: Promise<void>;
	private failure?: Error;
	onMessage: (message: Record<string, unknown>) => void = () => {};
	onFailure: (error: Error) => void = () => {};

	constructor(executable: string, prefixArgs: string[], cwd: string, env: Record<string, string>, config: string[]) {
		const childEnv = { ...process.env, ...env };
		for (const key of [
			"OPENAI_API_KEY",
			"CODEX_API_KEY",
			"OPENAI_BASE_URL",
			"ANTHROPIC_API_KEY",
			"ANTHROPIC_AUTH_TOKEN",
		])
			delete childEnv[key];
		this.child = spawn(
			executable,
			[...prefixArgs, "app-server", "--listen", "stdio://", ...config.flatMap((value) => ["-c", value])],
			{
				cwd,
				env: childEnv,
				stdio: ["pipe", "pipe", "pipe"],
				shell: false,
			},
		);
		this.exited = new Promise((resolveExit) => this.child.once("close", () => resolveExit()));
		this.child.stderr.on("data", (chunk) => {
			this.stderr = `${this.stderr}${chunk}`.slice(-8192);
		});
		this.child.on("error", (error) =>
			this.fail(new NonRetryableResearchError(`Cannot start official Codex CLI: ${error.message}`)),
		);
		this.child.stdin.on("error", (error) => this.fail(codexError(`Codex connection closed: ${error.message}`)));
		this.child.on("close", () => {
			if (!this.closed) this.fail(codexError(`Codex app-server exited unexpectedly: ${this.stderr}`));
		});
		const lines = createInterface({ input: this.child.stdout });
		lines.on("line", (line) => {
			try {
				const message = record(JSON.parse(line));
				if (typeof message.id === "number" && !message.method) {
					const request = this.pending.get(message.id);
					if (!request) return;
					this.pending.delete(message.id);
					if (message.error)
						request.reject(codexError(String(record(message.error).message ?? "Codex request failed")));
					else request.resolve(message.result);
				} else this.onMessage(message);
			} catch (error) {
				this.fail(
					codexError(`Invalid Codex protocol message: ${error instanceof Error ? error.message : String(error)}`),
				);
			}
		});
	}

	private fail(error: Error): void {
		this.failure ??= error;
		for (const request of this.pending.values()) request.reject(error);
		this.pending.clear();
		this.onFailure(error);
	}

	send(message: unknown): void {
		if (!this.closed) this.child.stdin.write(`${JSON.stringify(message)}\n`);
	}

	request(method: string, params: unknown): Promise<unknown> {
		if (this.failure) return Promise.reject(this.failure);
		const id = ++this.sequence;
		return new Promise((resolveRequest, reject) => {
			const timer = setTimeout(() => {
				this.pending.delete(id);
				reject(codexError(`Codex ${method} timed out`));
			}, 30_000);
			this.pending.set(id, {
				resolve: (value) => {
					clearTimeout(timer);
					resolveRequest(value);
				},
				reject: (error) => {
					clearTimeout(timer);
					reject(error);
				},
			});
			this.send({ id, method, params });
		});
	}

	async initialize(): Promise<void> {
		await this.request("initialize", {
			clientInfo: { name: "astra", title: "Astra research", version: "0.1.0" },
			capabilities: { experimentalApi: true },
		});
		this.send({ method: "initialized", params: {} });
		const response = record(await this.request("account/read", { refreshToken: false }));
		if (record(response.account).type !== "chatgpt") {
			throw new NonRetryableResearchError(
				"Codex backend requires subscription login. Run codex login in your terminal; Astra does not import credentials or fall back to an API key.",
			);
		}
	}

	async runtimeExecutables(): Promise<string[]> {
		if (process.platform !== "linux" || !this.child.pid) return [];
		// Linux's minimal filesystem omits user-installed runtimes. The npm CLI can
		// launch the native server as a child; mount only these executable files.
		const pending = [this.child.pid];
		const paths: string[] = [];
		for (const pid of pending) {
			paths.push(await readlink(`/proc/${pid}/exe`));
			const children = await readFile(`/proc/${pid}/task/${pid}/children`, "utf8");
			pending.push(...children.trim().split(/\s+/).filter(Boolean).map(Number));
		}
		return paths;
	}

	async close(): Promise<void> {
		this.closed = true;
		this.fail(codexError("Codex connection closed"));
		this.child.stdin.end();
		this.child.kill("SIGTERM");
		const timer = setTimeout(() => this.child.kill("SIGKILL"), 2000);
		try {
			await this.exited;
		} finally {
			clearTimeout(timer);
		}
	}
}

/** Uses only the published local app-server protocol. Codex owns auth and inference. */
export class CodexAppServerRunner {
	private readonly executable: string;
	private readonly prefixArgs: string[];

	constructor(options: { executable?: string; prefixArgs?: string[] } = {}) {
		this.executable = options.executable ?? process.env.ASTRA_CODEX_BIN ?? "codex";
		this.prefixArgs = options.prefixArgs ?? [];
	}

	async checkSubscription(cwd: string): Promise<void> {
		const connection = new AppServerConnection(this.executable, this.prefixArgs, cwd, {}, [
			'model_provider="openai"',
		]);
		try {
			await connection.initialize();
		} finally {
			await connection.close();
		}
	}

	async run<S extends TSchema>(options: CodexRunOptions<S>): Promise<CodexRunResult<Static<S>>> {
		const filesystem: Record<string, string> = { ":minimal": "read", [resolve(options.cwd)]: "read" };
		for (const path of options.readRoots ?? []) filesystem[resolve(path)] = "read";
		for (const path of options.writeRoots ?? []) filesystem[resolve(path)] = "write";
		const profile = `{filesystem={${Object.entries(filesystem)
			.map(([key, value]) => `${JSON.stringify(key)}=${JSON.stringify(value)}`)
			.join(",")}},network={enabled=${Boolean(options.network)}}}`;
		const connection = new AppServerConnection(this.executable, this.prefixArgs, options.cwd, options.env ?? {}, [
			'model_provider="openai"',
			'chatgpt_base_url="https://chatgpt.com/backend-api/"',
			`permissions.astra=${profile}`,
			"features.multi_agent=false",
			"features.memories=false",
			"features.hooks=false",
			"features.plugins=false",
			"project_doc_max_bytes=0",
			`web_search="${options.webSearch ? "live" : "disabled"}"`,
		]);
		let threadId = options.threadId ?? "";
		let turnId = "";
		let finalText = "";
		let toolCalls = 0;
		let failure: Error | undefined;
		let log = Promise.resolve();
		let rejectTurn: (error: Error) => void = () => {};
		let resolveTurn: () => void = () => {};
		const completed = new Promise<void>((resolveComplete, reject) => {
			resolveTurn = resolveComplete;
			rejectTurn = (error) => {
				failure ??= error;
				reject(error);
			};
		});
		// Attach immediately: protocol failure may arrive while thread/start is still awaited.
		void completed.catch(() => {});
		connection.onFailure = rejectTurn;
		const interrupt = () => {
			if (threadId && turnId) connection.send({ id: -1, method: "turn/interrupt", params: { threadId, turnId } });
		};
		const abort = () => {
			interrupt();
			rejectTurn(codexError("Codex research execution interrupted by operator"));
		};
		process.once("SIGINT", abort);
		process.once("SIGTERM", abort);
		const timer = setTimeout(() => {
			interrupt();
			rejectTurn(codexError("Codex research task exceeded its runtime budget"));
		}, options.timeoutMs);
		try {
			await mkdir(dirname(options.logPath), { recursive: true });
			connection.onMessage = (message) => {
				const method = String(message.method ?? "");
				const params = record(message.params);
				if (params.threadId && params.threadId !== threadId) return;
				if (
					method.startsWith("item/") ||
					method.startsWith("turn/") ||
					method === "thread/tokenUsage/updated" ||
					method === "error"
				) {
					log = log.then(() => appendFile(options.logPath, `${JSON.stringify(message)}\n`, { mode: 0o600 }));
					void log.catch(rejectTurn);
				}
				if (method === "turn/started") turnId = String(record(params.turn).id ?? "");
				if (method === "item/started") {
					const kind = record(params.item).type;
					if (
						[
							"commandExecution",
							"fileChange",
							"dynamicToolCall",
							"mcpToolCall",
							"webSearch",
							"collabAgentToolCall",
						].includes(String(kind)) &&
						++toolCalls > options.maxToolCalls
					) {
						interrupt();
						rejectTurn(codexError("Codex research task exceeded its tool-call budget"));
					}
				}
				if (method === "item/completed") {
					const item = record(params.item);
					if (item.type === "webSearch" && options.webSearch && options.onWebSearch) {
						const observe = options.onWebSearch;
						log = log.then(() => observe(item));
						void log.catch(rejectTurn);
					}
					if (item.type === "agentMessage" && (item.phase === "final_answer" || item.phase == null))
						finalText = String(item.text ?? "");
				}
				if (method === "turn/completed") {
					const turn = record(params.turn);
					if (turn.status === "completed") resolveTurn();
					else {
						const error = record(turn.error);
						rejectTurn(
							codexError(String(error.message ?? `Codex turn ${String(turn.status)}`), error.codexErrorInfo),
						);
					}
				}
				if (method === "error" && params.willRetry === false) {
					const error = record(params.error);
					rejectTurn(codexError(String(error.message ?? "Codex turn failed"), error.codexErrorInfo));
				}
				if (message.id !== undefined) {
					if (method !== "item/tool/call") {
						connection.send({
							id: message.id,
							error: { code: -32601, message: "Astra requires operator intervention for this request" },
						});
						interrupt();
						rejectTurn(codexError(`Codex requires operator intervention: ${method}`));
						return;
					}
					const tool = options.tools?.find((candidate) => candidate.name === params.tool);
					void (async () => {
						try {
							// A native search completion must be receipted before a subsequent host tool lists sources.
							await log;
							if (!tool || !Value.Check(tool.inputSchema, params.arguments))
								throw new Error("Invalid Astra tool request");
							if (toolCalls > options.maxToolCalls) throw new Error("Tool budget exhausted");
							const value = await tool.execute(params.arguments);
							connection.send({
								id: message.id,
								result: { success: true, contentItems: [{ type: "inputText", text: JSON.stringify(value) }] },
							});
						} catch (error) {
							connection.send({
								id: message.id,
								result: {
									success: false,
									contentItems: [
										{ type: "inputText", text: error instanceof Error ? error.message : String(error) },
									],
								},
							});
						}
					})();
				}
			};
			await connection.initialize();
			if (failure) throw failure;
			const effective = record(
				record(await connection.request("config/read", { cwd: options.cwd, includeLayers: false })).config,
			);
			if (effective.openai_base_url)
				throw codexError("Remove the Codex openai_base_url override to use subscription routing");
			const skillList = record(await connection.request("skills/list", { cwds: [options.cwd] }));
			const skillEntries = Array.isArray(skillList.data) ? skillList.data : [];
			const skills = skillEntries.flatMap((entry) => {
				const found = record(entry).skills;
				return Array.isArray(found) ? found : [];
			});
			for (const path of await connection.runtimeExecutables()) filesystem[path] = "read";
			const config = {
				"permissions.astra.filesystem": filesystem,
				mcp_servers: Object.fromEntries(
					Object.keys(record(effective.mcp_servers)).map((name) => [name, { enabled: false }]),
				),
				"skills.config": skills.map((skill) => ({ path: record(skill).path, enabled: false })),
			};
			const threadParams = {
				config,
				cwd: options.cwd,
				runtimeWorkspaceRoots: [options.cwd],
				modelProvider: "openai",
				...(options.model ? { model: options.model } : {}),
				approvalPolicy: "never",
				permissions: "astra",
				developerInstructions: options.instructions,
			};
			const started = record(
				await connection.request(threadId ? "thread/resume" : "thread/start", {
					...threadParams,
					...(threadId
						? { threadId }
						: {
								allowProviderModelFallback: false,
								dynamicTools: (options.tools ?? []).map(({ name, description, inputSchema }) => ({
									type: "function",
									name,
									description,
									inputSchema,
								})),
							}),
				}),
			);
			threadId = String(record(started.thread).id ?? "");
			if (
				!threadId ||
				record(started.activePermissionProfile).id !== "astra" ||
				started.modelProvider !== "openai"
			) {
				throw codexError("Codex did not activate the required official provider and Astra permission profile");
			}
			if (options.model && started.model !== options.model)
				throw codexError("Codex did not activate the requested model");
			log = log.then(() =>
				appendFile(
					options.logPath,
					`${JSON.stringify({ method: "astra/session", params: { threadId, model: started.model, modelProvider: started.modelProvider, permissionProfile: "astra" } })}\n`,
					{ mode: 0o600 },
				),
			);
			await options.onThread(threadId);
			if (failure) throw failure;
			const turn = record(
				await connection.request("turn/start", {
					threadId,
					input: [{ type: "text", text: options.prompt, text_elements: [] }],
					outputSchema: options.schema,
				}),
			);
			turnId = String(record(turn.turn).id ?? turnId);
			await completed;
			await log;
			let output: unknown;
			try {
				output = JSON.parse(finalText);
			} catch {
				throw codexError("Codex did not return a JSON research result");
			}
			if (!Value.Check(options.schema, output))
				throw codexError("Codex final output does not match the requested research schema");
			return { output: output as Static<S>, model: String(started.model), threadId, sessionFile: options.logPath };
		} finally {
			clearTimeout(timer);
			process.off("SIGINT", abort);
			process.off("SIGTERM", abort);
			await connection.close();
			await log;
		}
	}
}
