#!/usr/bin/env node

import { type ChildProcess, spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { realpathSync } from "node:fs";
import { mkdir, readdir, readFile, realpath, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { basename, extname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { assertAstraId } from "./contracts.ts";
import type { ResearchControlRequest } from "./research-control.ts";
import { DEFAULT_STAGES } from "./stages.ts";
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
}

export async function startWorkbench(options: { root: string; watch?: string[]; port?: number; runnerPath?: string }) {
	const root = resolve(options.root);
	await mkdir(root, { recursive: true });
	const entries = new Map<string, Entry>();
	const running = new Map<string, RunState>();
	const token = randomUUID();
	const webRoot = fileURLToPath(new URL("../web/", import.meta.url));
	function register(path: string, readonly: boolean) {
		const id = createHash("sha256").update(path).digest("hex").slice(0, 20);
		const entry = { id, root: path, readonly };
		entries.set(id, entry);
		return entry;
	}
	for (const path of options.watch ?? []) register(resolve(path), true);
	for (const directory of await readdir(root, { withFileTypes: true })) {
		if (directory.isDirectory() && directory.name.startsWith("run-")) register(join(root, directory.name), false);
	}
	async function snapshot(entry: Entry): Promise<JobSnapshot | undefined> {
		try {
			const active = JSON.parse(await readFile(join(entry.root, ".astra/active-job.json"), "utf8")) as {
				jobId: string;
			};
			assertAstraId(active.jobId, "job id");
			return JSON.parse(
				await readFile(join(entry.root, ".astra/jobs", active.jobId, "job.json"), "utf8"),
			) as JobSnapshot;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
			throw error;
		}
	}
	function launch(entry: Entry, request: ResearchControlRequest) {
		if (running.get(entry.id)?.child) throw new Error("当前任务已有执行进程，请勿重复启动");
		const env: NodeJS.ProcessEnv = { ...process.env, ASTRA_CODEX_MODEL: "gpt-5.6-luna" };
		delete env.ASTRA_FIXTURE_PROVIDER;
		const child = spawn(
			process.execPath,
			[
				options.runnerPath ??
					fileURLToPath(new URL(`./workbench-runner${extname(fileURLToPath(import.meta.url))}`, import.meta.url)),
			],
			{ cwd: entry.root, env, stdio: ["pipe", "pipe", "pipe"] },
		);
		const state: RunState = { child, output: "" };
		running.set(entry.id, state);
		const collect = (chunk: Buffer) => {
			state.output = (state.output + chunk.toString()).slice(-16000);
		};
		child.stdout?.on("data", collect);
		child.stderr?.on("data", collect);
		child.on("error", (error) => {
			state.error = error.message;
		});
		child.on("close", (code, signal) => {
			state.child = undefined;
			if (code !== 0) state.error ??= `执行进程退出（${signal ?? code}），请查看运行输出与任务状态`;
			void writeFile(join(entry.root, "workbench-output.log"), state.output).catch((error) => {
				state.error = String(error);
			});
			if (state.pauseRequested) launch(entry, { action: "pause", reason: "用户从工作台暂停" });
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
							return {
								...entry,
								frame: state?.frame,
								paused: state?.paused,
								updatedAt: state?.updatedAt,
								running: Boolean(running.get(entry.id)?.child),
								error: running.get(entry.id)?.error,
							};
						} catch (error) {
							return { ...entry, error: String(error) };
						}
					}),
				);
				json(200, { jobs, token, model: "gpt-5.6-luna", stages: DEFAULT_STAGES });
				return;
			}
			const entry = entries.get(url.searchParams.get("id") ?? "");
			if (req.method === "GET" && url.pathname === "/api/job" && entry) {
				json(200, {
					...entry,
					snapshot: await snapshot(entry),
					running: Boolean(running.get(entry.id)?.child),
					output: running.get(entry.id)?.output ?? "",
					error: running.get(entry.id)?.error,
				});
				return;
			}
			if (req.method === "GET" && url.pathname === "/api/file" && entry) {
				const state = await snapshot(entry);
				const artifact = state?.canonical[url.searchParams.get("artifact") ?? ""];
				const ref = url.searchParams.get("ref") ?? "";
				const evidence = artifact && state?.evidence[artifact.evidenceId];
				if (!artifact || !evidence?.refs.includes(ref)) throw new Error("文件不属于该成果的交付记录");
				const jobRoot = await realpath(join(entry.root, ".astra/jobs", state!.frame.jobId));
				const path = await realpath(resolve(entry.root, ref));
				const inside = relative(jobRoot, path);
				if (!inside || inside.startsWith("..") || resolve(jobRoot, inside) !== path)
					throw new Error("文件不在任务目录内");
				const info = await stat(path);
				if (!info.isFile() || info.size > 32 * 1024 * 1024) throw new Error("文件不可下载或超过 32 MiB");
				res.writeHead(200, {
					"Content-Type": extname(path) === ".pdf" ? "application/pdf" : "application/octet-stream",
					"Content-Disposition": `attachment; filename*=UTF-8''${encodeURIComponent(basename(path))}`,
					"X-Content-Type-Options": "nosniff",
				});
				res.end(await readFile(path));
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
				launch(created, {
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
				processState.pauseRequested = true;
				processState.child.kill("SIGINT");
				json(202, { id: entry.id });
				return;
			}
			if (url.pathname === "/api/resume") {
				const state = await snapshot(entry);
				if (!state || state.frame.status === "completed") throw new Error("当前任务不存在或已经完成");
				if (!state.paused) throw new Error("仅可继续已暂停的任务");
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
				launch(entry, {
					action: "resume",
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
