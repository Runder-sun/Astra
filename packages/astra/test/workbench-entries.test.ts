import fsPromises, { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createContext, runInContext } from "node:vm";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { startWorkbench } from "../src/workbench.ts";

const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => {
	vi.restoreAllMocks();
	syncBuiltinESMExports();
	vi.unstubAllEnvs();
	for (const close of cleanup.splice(0).reverse()) await close();
});
class Element {
	textContent = "";
	hidden = false;
	children: Element[] = [];
	value = "";
	dataset = {};
	onclick?: () => void;
	append(...children: Element[]) {
		this.children.push(...children);
	}
	replaceChildren(...children: Element[]) {
		this.children = children;
		this.textContent = "";
	}
	focus() {}
}
async function page(fetcher: (path: string, options?: RequestInit) => Promise<Response>) {
	const elements = new Map<string, Element>();
	const get = (id: string) => {
		let element = elements.get(id);
		if (!element) {
			element = new Element();
			elements.set(id, element);
		}
		return element;
	};
	get("research").hidden = true;
	const context = createContext({
		document: { getElementById: get, createElement: () => new Element(), documentElement: new Element() },
		fetch: fetcher,
		setInterval: () => 1,
		URLSearchParams,
		Date,
		console,
	});
	runInContext(
		await readFile(process.env.ASTRA_UI_SOURCE ?? fileURLToPath(new URL("../web/app.js", import.meta.url)), "utf8"),
		context,
	);
	await expect.poll(() => runInContext("fetching", context)).toBe(false);
	return { get, context };
}
async function project() {
	const root = await mkdtemp(join(tmpdir(), "astra-entry-regression-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	const path = join(root, "run-owned");
	await mkdir(path);
	const job = await ResearchJob.create(new JsonlAstraStore(path), {
		workspaceRoot: path,
		objective: "offline entry regression",
	});
	await writeFile(join(path, ".astra/active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
	return { root, path, job };
}
async function serve(root: string, watch?: string[], runnerPath?: string) {
	const app = await startWorkbench({ root, watch, runnerPath, port: 0 });
	cleanup.push(async () => {
		for (const execution of app.running.values())
			if (execution.child) {
				const child = execution.child;
				child.kill("SIGTERM");
				await new Promise<void>((resolve) => child.once("close", () => resolve()));
			}
		await new Promise<void>((resolve) => app.server.close(() => resolve()));
	});
	return app;
}

it("R4 invalidates old controls immediately and follows the latest selection after an in-flight detail", async () => {
	const { job } = await project();
	const data = (id: string) => ({
		id,
		snapshot: { ...job.state, frame: { ...job.state.frame, jobId: `job_${id}`, objective: id } },
		running: true,
		canPause: true,
		output: id,
		milestones: [],
	});
	let release!: () => void;
	let delayed = false;
	let started!: () => void;
	const detailStarted = new Promise<void>((resolve) => {
		started = resolve;
	});
	const pending = new Promise<void>((resolve) => {
		release = resolve;
	});
	const p = await page(async (path) => {
		if (path === "/api/jobs")
			return Response.json({
				token: "offline",
				stages: [],
				jobs: ["a", "b", "c"].map((id) => ({ id, frame: data(id).snapshot.frame })),
			});
		const id = new URL(path, "http://localhost").searchParams.get("id")!;
		if (id === "a" && delayed) {
			started();
			await pending;
		}
		return Response.json(data(id));
	});
	delayed = true;
	const refreshing = runInContext("refresh()", p.context);
	await detailStarted;
	p.get("jobs").children[1].onclick!();
	p.get("jobs").children[2].onclick!();
	const cleared = runInContext("current", p.context);
	release();
	await refreshing;
	await expect.poll(() => p.get("title").textContent).toBe("c");
	expect(cleared).toBeUndefined();
	expect(runInContext("current.id", p.context)).toBe("c");
	expect(p.get("error").textContent).toBe("");
});

it("N5 updates log text without rebuilding stage details or replacing user input", async () => {
	const { job } = await project();
	let output = "first log";
	const p = await page(async (path) =>
		Response.json(
			path === "/api/jobs"
				? { token: "offline", stages: [], jobs: [{ id: "entry", frame: job.state.frame }] }
				: { id: "entry", snapshot: job.state, running: true, canPause: true, output, milestones: [] },
		),
	);
	const existing = p.get("stage-detail").children[0];
	p.get("guidance").value = "preserved guidance";
	output += "\nsecond log";
	await runInContext("refresh()", p.context);
	expect(p.get("output").textContent).toBe(output);
	expect(p.get("stage-detail").children[0]).toBe(existing);
	expect(p.get("guidance").value).toBe("preserved guidance");
});

it("N6 merges explicit watched paths and physical aliases as read-only before controls", async () => {
	const { root, path, job } = await project();
	await job.pause("offline setup");
	const alias = join(root, "alias");
	await symlink(path, alias, "dir");
	const app = await serve(root, [path, alias]);
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string; readonly: boolean }>;
	};
	expect(listing.jobs).toHaveLength(1);
	expect(listing.jobs[0].readonly).toBe(true);
	const response = await fetch(`${app.url}/api/resume?id=${listing.jobs[0].id}`, {
		method: "POST",
		headers: { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" },
		body: JSON.stringify({ jobId: job.state.frame.jobId }),
	});
	expect(response.status).toBe(400);
});

it("N6 resolves a missing watched path when it appears before a modification request", async () => {
	const { root, path, job } = await project();
	await job.pause("offline setup");
	const missing = join(root, "not-yet-created");
	const app = await serve(root, [missing], fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)));
	const before = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string; root: string }>;
	};
	const owned = before.jobs.find((row) => row.root === path)!;
	await symlink(path, missing, "dir");
	const response = await fetch(`${app.url}/api/resume?id=${owned.id}`, {
		method: "POST",
		headers: { Origin: app.url, "X-Astra-Token": before.token, "Content-Type": "application/json" },
		body: JSON.stringify({ jobId: job.state.frame.jobId }),
	});
	expect(response.status).toBe(400);
	expect(await response.json()).toMatchObject({ error: expect.stringContaining("仅供查看") });
	const after = (await (await fetch(`${app.url}/api/jobs`)).json()) as { jobs: Array<{ readonly: boolean }> };
	expect(after.jobs).toHaveLength(1);
	expect(after.jobs[0].readonly).toBe(true);
});

it("N3 permits recovery of a failed unpaused job with the exact identity", async () => {
	const { root, job } = await project();
	const app = await serve(root, undefined, fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)));
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string }>;
	};
	const headers = { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" };
	const response = await fetch(`${app.url}/api/resume?id=${listing.jobs[0].id}`, {
		method: "POST",
		headers,
		body: JSON.stringify({ jobId: job.state.frame.jobId }),
	});
	expect(response.status).toBe(202);
	expect(
		(await ResearchJob.open(new JsonlAstraStore(job.state.frame.permissions.workspaceRoot), job.state.frame.jobId))!
			.state.paused,
	).toBe(false);
});

it("N3 reports an external execution owner and rejects duplicate recovery without killing it", async () => {
	const { root, path, job } = await project();
	const app = await serve(root, undefined, fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)));
	await new JsonlAstraStore(path).withExecutionLock(job.state.frame.jobId, "external-offline-driver", async () => {
		const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
			token: string;
			jobs: Array<{ id: string; running: boolean; canPause: boolean }>;
		};
		const row = listing.jobs[0];
		expect(row).toMatchObject({ running: true, canPause: false });
		const headers = { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" };
		for (const action of ["resume", "pause"])
			expect(
				(
					await fetch(`${app.url}/api/${action}?id=${row.id}`, {
						method: "POST",
						headers,
						body: JSON.stringify({ jobId: job.state.frame.jobId }),
					})
				).status,
			).toBe(400);
		expect(await new JsonlAstraStore(path).readExecutionOwner(job.state.frame.jobId)).toMatchObject({
			owner: "external-offline-driver",
			pid: process.pid,
		});
		expect(app.running.size).toBe(0);
	});
});

it("N5 restart reads only the active job bounded persistent log and keeps legacy output accessible", async () => {
	const { root, path, job } = await project();
	const first = `${"x".repeat(100000)}end-of-first-job`;
	await writeFile(join(path, ".astra/jobs", job.state.frame.jobId, "workbench-output.log"), first);
	await writeFile(join(path, "workbench-output.log"), "legacy output retained");
	const app = await serve(root);
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { jobs: Array<{ id: string }> };
	const url = `${app.url}/api/job?id=${listing.jobs[0].id}`;
	const initial = (await (await fetch(url)).json()) as { output: string };
	expect(initial.output).toBe(first.slice(-16000));
	const second = await ResearchJob.create(new JsonlAstraStore(path), {
		workspaceRoot: path,
		objective: "another valid research identity",
	});
	await writeFile(join(path, ".astra/active-job.json"), JSON.stringify({ jobId: second.state.frame.jobId }));
	expect(((await (await fetch(url)).json()) as { output: string }).output).toBe("");
	expect(await readFile(join(path, "workbench-output.log"), "utf8")).toBe("legacy output retained");
});

it("N5 actual child logs survive close and restart and report persistent-write failures", async () => {
	const { root, path, job } = await project();
	vi.stubEnv("ASTRA_FAKE_WORKBENCH_MODE", "resume-output-exit");
	const app = await serve(root, undefined, fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)));
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string }>;
	};
	const id = listing.jobs[0].id;
	const headers = { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" };
	const resume = () =>
		fetch(`${app.url}/api/resume?id=${id}`, {
			method: "POST",
			headers,
			body: JSON.stringify({ jobId: job.state.frame.jobId }),
		});
	expect((await resume()).status).toBe(202);
	await expect.poll(() => app.running.get(id)?.published).toBe(true);
	await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
	await app.running.get(id)?.flush;
	const persistent = join(path, ".astra/jobs", job.state.frame.jobId, "workbench-output.log");
	expect(await readFile(persistent, "utf8")).toContain(`fixture-resume-output:${job.state.frame.jobId}`);
	const restarted = await serve(root);
	expect(((await (await fetch(`${restarted.url}/api/job?id=${id}`)).json()) as { output: string }).output).toContain(
		`fixture-resume-output:${job.state.frame.jobId}`,
	);
	// Explicit isolated filesystem failure: a directory prevents an otherwise valid log write.
	await rm(persistent);
	await mkdir(persistent);
	expect((await resume()).status).toBe(202);
	await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
	await app.running.get(id)?.flush;
	expect(((await (await fetch(`${app.url}/api/job?id=${id}`)).json()) as { error: string }).error).toContain(
		"运行输出保存失败",
	);
});

it("N5 waits for a slow old close flush before publishing resumed output", async () => {
	const { root, path, job } = await project();
	vi.stubEnv("ASTRA_FAKE_WORKBENCH_MODE", "resume-output-exit");
	const app = await serve(root, undefined, fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)));
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string }>;
	};
	const id = listing.jobs[0].id;
	const headers = { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" };
	let release!: () => void;
	const delay = new Promise<void>((resolve) => {
		release = resolve;
	});
	const originalWrite = fsPromises.writeFile;
	let delayedClose = false;
	vi.spyOn(fsPromises, "writeFile").mockImplementation(async (...args) => {
		if (args[0] === join(path, "workbench-output.log") && !delayedClose) {
			delayedClose = true;
			await delay;
		}
		return originalWrite(...args);
	});
	syncBuiltinESMExports();
	const resume = () =>
		fetch(`${app.url}/api/resume?id=${id}`, {
			method: "POST",
			headers,
			body: JSON.stringify({ jobId: job.state.frame.jobId }),
		});
	try {
		expect((await resume()).status).toBe(202);
		await expect.poll(() => delayedClose).toBe(true);
		const oldState = app.running.get(id);
		let settled = false;
		const retry = resume().then((response) => {
			settled = true;
			return response;
		});
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(settled).toBe(false);
		expect(app.running.get(id)).toBe(oldState);
		release();
		expect((await retry).status).toBe(202);
		await expect.poll(() => app.running.get(id)?.published).toBe(true);
		await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
		await app.running.get(id)?.flush;
		expect(
			await readFile(join(path, ".astra/jobs", job.state.frame.jobId, "workbench-output.log"), "utf8"),
		).toContain(`fixture-resume-output:${job.state.frame.jobId}`);
	} finally {
		release();
	}
});

it("R4 rejects a detail whose entry or research identity differs from the latest summary", async () => {
	const { job } = await project();
	let calls = 0;
	const p = await page(async (path) => {
		if (path === "/api/jobs")
			return Response.json({ token: "offline", stages: [], jobs: [{ id: "a", frame: job.state.frame }] });
		calls++;
		return Response.json({
			id: calls === 1 ? "wrong-entry" : "a",
			snapshot: calls < 3 ? { ...job.state, frame: { ...job.state.frame, jobId: "job_stale" } } : job.state,
			output: "",
			milestones: [],
		});
	});
	await expect.poll(() => calls).toBe(3);
	expect(runInContext("current.id", p.context)).toBe("a");
	expect(runInContext("current.snapshot.frame.jobId", p.context)).toBe(job.state.frame.jobId);
});

it.each(["resume", "pause", "create"])(
	"R4 a late %s failure cannot replace the newly selected task error",
	async (action) => {
		const { job } = await project();
		let release!: () => void;
		const delayed = new Promise<void>((resolve) => {
			release = resolve;
		});
		let started!: () => void;
		const operationStarted = new Promise<void>((resolve) => {
			started = resolve;
		});
		const p = await page(async (path) => {
			if (path === "/api/jobs")
				return Response.json({
					token: "offline",
					stages: [],
					jobs: ["a", "b"].map((id) => ({ id, frame: job.state.frame })),
				});
			if (path.startsWith(`/api/${action === "create" ? "run" : action}`)) {
				started();
				await delayed;
				return Response.json({ error: "old task operation failed" }, { status: 400 });
			}
			const id = new URL(path, "http://localhost").searchParams.get("id");
			return Response.json({
				id,
				snapshot: job.state,
				running: action === "pause",
				canPause: true,
				output: "",
				milestones: [],
			});
		});
		if (action === "create") p.get("new").onclick!();
		const operation =
			action === "create"
				? runInContext('el("create-form").onsubmit({ preventDefault() {} })', p.context)
				: p.get(action).onclick!();
		await operationStarted;
		p.get("jobs").children[1].onclick!();
		await expect.poll(() => runInContext("current?.id", p.context)).toBe("b");
		runInContext('error("new task message")', p.context);
		release();
		await operation;
		expect(p.get("error").textContent).toBe("new task message");
	},
);

it("N3/N5 restarted workbench cannot control an external live child and its two same-sequence logs refresh the real page", async () => {
	const { root, job } = await project();
	vi.stubEnv("ASTRA_FAKE_WORKBENCH_MODE", "resume-output-stream");
	const first = await serve(
		root,
		undefined,
		fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)),
	);
	const listing = (await (await fetch(`${first.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string }>;
	};
	const id = listing.jobs[0].id;
	expect(
		(
			await fetch(`${first.url}/api/resume?id=${id}`, {
				method: "POST",
				headers: { Origin: first.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" },
				body: JSON.stringify({ jobId: job.state.frame.jobId }),
			})
		).status,
	).toBe(202);
	await expect.poll(() => first.running.get(id)?.output).toContain("fixture-resume-output:");
	const child = first.running.get(id)!.child!;
	const kill = vi.spyOn(child, "kill");
	const restarted = await serve(root);
	const p = await page((path, options) => fetch(restarted.url + path, options));
	const sequence = runInContext("current.snapshot.eventSeq", p.context);
	const details = p.get("stage-detail").children[0];
	p.get("guidance").value = "keep this guidance";
	expect(runInContext("current.running", p.context)).toBe(true);
	expect(p.get("pause").hidden).toBe(true);
	expect(p.get("continue").hidden).toBe(true);
	for (const action of ["resume", "pause"])
		expect(
			(
				await fetch(`${restarted.url}/api/${action}?id=${id}`, {
					method: "POST",
					headers: {
						Origin: restarted.url,
						"X-Astra-Token": runInContext("token", p.context),
						"Content-Type": "application/json",
					},
					body: JSON.stringify({ jobId: job.state.frame.jobId }),
				})
			).status,
		).toBe(400);
	expect(kill).not.toHaveBeenCalled();
	process.kill(child.pid!, 0);
	await expect.poll(() => first.running.get(id)?.output).toContain("fixture-second-output:");
	await first.running.get(id)?.flush;
	await runInContext("refresh()", p.context);
	expect(runInContext("current.snapshot.eventSeq", p.context)).toBe(sequence);
	expect(p.get("output").textContent).toContain("fixture-second-output:");
	expect(p.get("stage-detail").children[0]).toBe(details);
	expect(p.get("guidance").value).toBe("keep this guidance");
});
