#!/usr/bin/env node

import { type ChildProcess, spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { realpathSync } from "node:fs";
import { type FileHandle, mkdir, open, readdir, readFile, realpath, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { basename, extname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { assertAstraId } from "./contracts.ts";
import { researchMilestones } from "./progress.ts";
import { ResearchJob } from "./research.ts";
import type { ResearchControlRequest } from "./research-control.ts";
import { DEFAULT_STAGES } from "./stages.ts";
import { JsonlAstraStore } from "./store.ts";
import { readVersionedFile, taskWorkspacePath } from "./task-workspace.ts";
import type { JobSnapshot } from "./types.ts";

interface Entry {
	id: string;
	root: string;
	readonly: boolean;
}
interface RunState {
	child?: ChildProcess;
	output: string;
	error?: string;
	pauseRequested?: boolean;
	jobId?: string;
	published?: boolean;
	flush?: Promise<void>;
}

export async function startWorkbench(options: { root: string; watch?: string[]; port?: number; runnerPath?: string }) {
	await mkdir(resolve(options.root), { recursive: true });
	const root = await realpath(resolve(options.root));
	const entries = new Map<string, Entry>();
	const aliases = new Map<string, string>();
	const running = new Map<string, RunState>();
	const logWrites = new Map<string, Promise<void>>();
	const token = randomUUID();
	const webRoot = fileURLToPath(new URL("../web/", import.meta.url));
	function register(path: string, readonly: boolean) {
		const id = createHash("sha256").update(path).digest("hex").slice(0, 20);
		const entry = entries.get(id) ?? { id, root: path, readonly };
		entry.readonly ||= readonly;
		entries.set(id, entry);
		return entry;
	}
	async function refreshWatchEntries() {
		for (const path of options.watch ?? []) {
			const lexical = resolve(path);
			let physical = lexical;
			try {
				physical = await realpath(lexical);
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			}
			const entry = register(physical, true);
			const oldId = createHash("sha256").update(lexical).digest("hex").slice(0, 20);
			if (oldId !== entry.id) {
				aliases.set(oldId, entry.id);
				entries.delete(oldId);
			}
		}
	}
	await refreshWatchEntries();
	for (const directory of await readdir(root, { withFileTypes: true })) {
		if (directory.isDirectory() && directory.name.startsWith("run-")) register(join(root, directory.name), false);
	}
	async function snapshot(entry: Entry): Promise<JobSnapshot | undefined> {
		try {
			const owned = running.get(entry.id);
			const active =
				owned?.child && owned.jobId
					? { jobId: owned.jobId }
					: (JSON.parse(await readFile(join(entry.root, ".astra/active-job.json"), "utf8")) as {
							jobId: string;
						});
			assertAstraId(active.jobId, "job id");
			return (await ResearchJob.open(new JsonlAstraStore(entry.root), active.jobId))?.state;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
			throw error;
		}
	}
	function displayedRun(entry: Entry, state: JobSnapshot | undefined): RunState | undefined {
		const execution = running.get(entry.id);
		return state && execution?.jobId && execution.jobId !== state.frame.jobId ? undefined : execution;
	}
	function persistLog(entry: Entry, state: RunState): Promise<void> {
		if (!state.published || !state.jobId) return Promise.resolve();
		const path = join(entry.root, ".astra/jobs", state.jobId, "workbench-output.log");
		const output = state.output;
		const write = (logWrites.get(path) ?? Promise.resolve())
			.then(() => writeFile(path, output))
			.catch((error: unknown) => {
				state.error = `运行输出保存失败：${error instanceof Error ? error.message : String(error)}`;
			});
		logWrites.set(path, write);
		return write;
	}
	async function readLog(entry: Entry, jobId: string | undefined): Promise<string> {
		if (!jobId) return "";
		let file: FileHandle;
		try {
			file = await open(join(entry.root, ".astra/jobs", jobId, "workbench-output.log"), "r");
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return "";
			throw error;
		}
		try {
			const size = (await file.stat()).size;
			const bytes = Buffer.alloc(Math.min(size, 64_000));
			await file.read(bytes, 0, bytes.length, Math.max(0, size - bytes.length));
			return bytes.toString().slice(-16000);
		} finally {
			await file.close();
		}
	}
	async function pauseChild(entry: Entry, execution: RunState, deferred = false) {
		if (!execution.published || !execution.jobId) return;
		const owner = await new JsonlAstraStore(entry.root).readExecutionOwner(execution.jobId);
		if (!owner && deferred) return;
		if (!execution.child || owner?.pid !== execution.child.pid || owner?.jobId !== execution.jobId)
			throw new Error("执行归属已变化，不能暂停该进程");
		execution.child.kill("SIGINT");
	}
	async function executionDetails(entry: Entry, state: JobSnapshot | undefined) {
		const execution = displayedRun(entry, state);
		const owner = state ? await new JsonlAstraStore(entry.root).readExecutionOwner(state.frame.jobId) : undefined;
		return {
			running: Boolean(owner || execution?.child),
			canPause:
				!entry.readonly &&
				Boolean(
					execution?.child &&
						(!execution.published || (owner?.pid === execution.child.pid && owner?.jobId === state?.frame.jobId)),
				),
			error: execution?.error,
		};
	}
	async function launch(entry: Entry, request: ResearchControlRequest) {
		await running.get(entry.id)?.flush;
		if (request.jobId && (await new JsonlAstraStore(entry.root).readExecutionOwner(request.jobId)))
			throw new Error("当前研究已有执行进程，请勿重复启动");
		if (running.get(entry.id)?.child) throw new Error("当前任务已有执行进程，请勿重复启动");
		const env: NodeJS.ProcessEnv = { ...process.env, ASTRA_CODEX_MODEL: "gpt-5.6-luna" };
		delete env.ASTRA_FIXTURE_PROVIDER;
		const child = spawn(
			process.execPath,
			[
				options.runnerPath ??
					fileURLToPath(new URL(`./workbench-runner${extname(fileURLToPath(import.meta.url))}`, import.meta.url)),
			],
			{ cwd: entry.root, env, stdio: ["pipe", "pipe", "pipe", "ipc"] },
		);
		const state: RunState = { child, output: "", jobId: request.jobId };
		running.set(entry.id, state);
		const collect = (chunk: Buffer) => {
			state.output = (state.output + chunk.toString()).slice(-16000);
			state.flush = persistLog(entry, state);
		};
		child.stdout?.on("data", collect);
		child.stderr?.on("data", collect);
		child.on("message", (message: unknown) => {
			if (
				!message ||
				typeof message !== "object" ||
				!("type" in message) ||
				message.type !== "astra/job-published" ||
				!("jobId" in message) ||
				typeof message.jobId !== "string"
			)
				return;
			try {
				assertAstraId(message.jobId, "job id");
				if (state.jobId && state.jobId !== message.jobId) throw new Error("执行进程发布的研究身份不一致");
				state.jobId = message.jobId;
				state.published = true;
				state.flush = persistLog(entry, state);
				if (state.pauseRequested)
					void pauseChild(entry, state, true).catch((error: unknown) => {
						state.error = String(error);
					});
			} catch (error) {
				state.error = error instanceof Error ? error.message : String(error);
			}
		});
		child.on("error", (error) => {
			state.error = error.message;
		});
		child.on("close", (code, signal) => {
			state.child = undefined;
			if (!state.published)
				state.error = `${state.error ? `${state.error}\n` : ""}执行进程退出前没有发布研究身份，无法确认控制目标`;
			if (code !== 0) state.error ??= `执行进程退出（${signal ?? code}），请查看运行输出与任务状态`;
			state.flush = persistLog(entry, state)
				.then(() => writeFile(join(entry.root, "workbench-output.log"), state.output))
				.catch((error: unknown) => {
					state.error = String(error);
				});
			if (state.pauseRequested && state.published && state.jobId)
				void state.flush
					.then(() => launch(entry, { action: "pause", jobId: state.jobId, reason: "用户从工作台暂停" }))
					.catch((error: unknown) => {
						state.error = String(error);
					});
		});
		child.stdin?.on("error", (error) => {
			state.error = error.message;
		});
		child.stdin?.end(JSON.stringify(request));
	}
	const server = createServer(async (req, res) => {
		const origin = `http://${req.headers.host ?? ""}`;
		const json = (code: number, value: unknown) => {
			res.writeHead(code, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" });
			res.end(JSON.stringify(value));
		};
		try {
			await refreshWatchEntries();
			if (
				!/^(?:127\.0\.0\.1|localhost|\[::1\])(?::\d{1,5})?$/.test(req.headers.host ?? "") ||
				(req.headers.origin && req.headers.origin !== origin)
			) {
				console.warn("工作台拒绝预览地址", JSON.stringify({ host: req.headers.host, origin: req.headers.origin }));
				json(403, {
					error: "预览地址未获允许，请先在普通浏览器打开 SSH 转发地址",
					receivedHost: req.headers.host,
					receivedOrigin: req.headers.origin ?? null,
				});
				return;
			}
			const url = new URL(req.url ?? "/", origin);
			if (req.method === "GET" && url.pathname === "/favicon.ico") {
				res.writeHead(204);
				res.end();
				return;
			}
			if (req.method === "GET" && ["/", "/app.js", "/style.css"].includes(url.pathname)) {
				const file = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
				const bytes = await readFile(join(webRoot, file));
				res.writeHead(200, {
					"Content-Type": file.endsWith("html")
						? "text/html; charset=utf-8"
						: file.endsWith("css")
							? "text/css"
							: "text/javascript",
					"Content-Security-Policy": "default-src 'self'; object-src 'none'; frame-ancestors 'none'",
					"X-Content-Type-Options": "nosniff",
				});
				res.end(bytes);
				return;
			}
			if (req.method === "GET" && url.pathname === "/api/jobs") {
				const jobs = await Promise.all(
					[...entries.values()].map(async (entry) => {
						try {
							const state = await snapshot(entry);
							const execution = await executionDetails(entry, state);
							return {
								...entry,
								frame: state?.frame,
								paused: state?.paused,
								updatedAt: state?.updatedAt,
								...execution,
							};
						} catch (error) {
							return { ...entry, error: String(error) };
						}
					}),
				);
				json(200, {
					jobs,
					aliases: Object.fromEntries(aliases),
					token,
					model: "gpt-5.6-luna",
					stages: DEFAULT_STAGES,
				});
				return;
			}
			const requestedId = url.searchParams.get("id") ?? "";
			const entry = entries.get(aliases.get(requestedId) ?? requestedId);
			if (req.method === "GET" && url.pathname === "/api/job" && entry) {
				const state = await snapshot(entry);
				const execution = displayedRun(entry, state);
				json(200, {
					...entry,
					snapshot: state,
					milestones: state ? researchMilestones(state) : [],
					...(await executionDetails(entry, state)),
					output: execution?.output ?? (await readLog(entry, state?.frame.jobId)),
				});
				return;
			}
			if (req.method === "GET" && url.pathname === "/api/file" && entry) {
				const state = await snapshot(entry);
				const artifact = state?.canonical[url.searchParams.get("artifact") ?? ""];
				const ref = url.searchParams.get("ref") ?? "";
				if (!ref || /^[a-z][a-z\d+.-]*:/i.test(ref) || /^[\\/]/.test(ref) || ref.split(/[\\/]/).includes(".."))
					throw new Error("invalid local evidence file reference");
				const evidence = artifact && state?.evidence[artifact.evidenceId];
				if (!artifact || !evidence?.refs.includes(ref)) throw new Error("文件不属于该成果的交付记录");
				const task = state?.tasks[evidence.taskId];
				if (!task) throw new Error("交付任务记录缺失");
				const workspace = taskWorkspacePath(entry.root, task.jobId, task.id);
				const path = resolve(ref.startsWith(".astra/") ? entry.root : workspace, ref);
				if (task.jobId !== state!.frame.jobId) throw new Error("下载来源任务不属于当前作业");
				const local = relative(workspace, path);
				if (local === ".." || local.startsWith("../") || local.startsWith("..\\") || isAbsolute(local))
					throw new Error("下载文件不属于来源任务");
				const bytes = await readVersionedFile(
					{ ...task, scope: { ...task.scope, workspaceRoot: entry.root } },
					evidence,
					ref,
					path,
					workspace,
				);
				if (!bytes || bytes.length > 32 * 1024 * 1024) throw new Error("文件不可下载或超过 32 MiB");
				res.writeHead(200, {
					"Content-Type": extname(path) === ".pdf" ? "application/pdf" : "application/octet-stream",
					"Content-Disposition": `attachment; filename*=UTF-8''${encodeURIComponent(basename(path))}`,
					"X-Content-Type-Options": "nosniff",
				});
				res.end(bytes);
				return;
			}
			if (req.method !== "POST") {
				json(404, { error: "入口不存在" });
				return;
			}
			if (req.headers["x-astra-token"] !== token || req.headers.origin !== origin) {
				json(403, { error: "请刷新工作台后再操作" });
				return;
			}
			let body = "";
			for await (const chunk of req) {
				body += chunk.toString();
				if (Buffer.byteLength(body) > 20000) throw new Error("输入过长");
			}
			const input = JSON.parse(body) as Record<string, unknown>;
			if (url.pathname === "/api/run") {
				if (
					typeof input.objective !== "string" ||
					input.objective.trim().length < 10 ||
					input.objective.length > 10000
				)
					throw new Error("请用 10 至 10000 字描述研究目标和范围");
				const maxTasks = input.maxTasks;
				if (typeof maxTasks !== "number" || !Number.isInteger(maxTasks) || maxTasks < 4 || maxTasks > 144)
					throw new Error("任务预算应为 4 至 144 的整数");
				if (typeof input.requirePaper !== "boolean") throw new Error("请选择是否交付论文");
				const directory = join(root, `run-${new Date().toISOString().slice(0, 10)}-${randomUUID().slice(0, 8)}`);
				await mkdir(directory);
				const created = register(directory, false);
				await launch(created, {
					action: "run",
					backend: "codex",
					objective: input.objective.trim(),
					automation: "autonomous",
					maxTasks,
					maxTurns: maxTasks * 4,
					requirePaper: input.requirePaper,
				});
				json(202, { id: created.id });
				return;
			}
			if (!entry || entry.readonly) throw new Error("该任务仅供查看，不能从此入口修改");
			if (url.pathname === "/api/pause") {
				const processState = running.get(entry.id);
				if (!processState?.child) throw new Error("工作台没有正在执行的进程");
				if (processState.jobId ? input.jobId !== processState.jobId : input.jobId !== undefined)
					throw new Error("页面研究目标已变化，请刷新后操作");
				if (processState.published) await pauseChild(entry, processState);
				processState.pauseRequested = true;
				json(202, { id: entry.id });
				return;
			}
			if (url.pathname === "/api/resume") {
				const state = await snapshot(entry);
				if (!state || state.frame.status === "completed") throw new Error("当前任务不存在或已经完成");
				if (input.jobId !== state.frame.jobId) throw new Error("页面研究目标已变化，请刷新后操作");
				if (await new JsonlAstraStore(entry.root).readExecutionOwner(state.frame.jobId))
					throw new Error("当前研究已有执行进程，请勿重复启动");
				if (input.guidance !== undefined && (typeof input.guidance !== "string" || input.guidance.length > 10000))
					throw new Error("补充说明格式不正确");
				if (
					input.maxTasks !== undefined &&
					(typeof input.maxTasks !== "number" ||
						!Number.isInteger(input.maxTasks) ||
						input.maxTasks < Object.keys(state.tasks).length ||
						input.maxTasks > 144)
				)
					throw new Error("预算不能小于已创建的任务数或超过 144");
				await launch(entry, {
					action: "resume",
					jobId: state.frame.jobId,
					backend: "codex",
					...(typeof input.maxTasks === "number"
						? { maxTasks: input.maxTasks, maxTurns: Math.max(state.frame.budget.maxTurns, input.maxTasks * 4) }
						: {}),
					...(typeof input.guidance === "string" && input.guidance.trim()
						? { guidance: input.guidance.trim() }
						: {}),
				});
				json(202, { id: entry.id });
				return;
			}
			json(404, { error: "入口不存在" });
		} catch (error) {
			json(400, { error: error instanceof Error ? error.message : String(error) });
		}
	});
	await new Promise<void>((ok, reject) => {
		server.once("error", reject);
		server.listen(options.port ?? 4318, "127.0.0.1", ok);
	});
	return { server, url: `http://127.0.0.1:${(server.address() as { port: number }).port}`, running };
}

if (process.argv[1] && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href) {
	const args = process.argv.slice(2);
	const watch: string[] = [];
	let root = join(process.cwd(), ".astra-workbench");
	let port = 4318;
	for (let index = 0; index < args.length; index++) {
		const flag = args[index];
		const value = args[++index];
		if (!value) throw new Error(`缺少参数 ${flag}`);
		if (flag === "--root") root = value;
		else if (flag === "--watch") watch.push(value);
		else if (flag === "--port" && /^\d+$/.test(value)) port = Number(value);
		else throw new Error(`未知参数 ${flag}`);
	}
	const workbench = await startWorkbench({ root, watch, port });
	console.log(`Astra 工作台：${workbench.url}`);
}
