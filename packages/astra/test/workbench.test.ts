import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createContext, runInContext } from "node:vm";
import { afterEach, expect, it, vi } from "vitest";
import { atomicWriteJson } from "../src/contracts.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { taskWorkspacePath } from "../src/task-workspace.ts";
import { startWorkbench } from "../src/workbench.ts";
import { reviewFixture } from "./review-fixture.ts";

const cleanup: Array<() => Promise<void>> = [];
function forwardedStatus(url: string, headers: Record<string, string>, method = "GET"): Promise<number | undefined> {
	return new Promise((resolve, reject) => {
		const req = request(url, { headers, method }, (response) => {
			response.resume();
			response.on("end", () => resolve(response.statusCode));
		});
		req.on("error", reject);
		req.end(method === "POST" ? "{}" : undefined);
	});
}
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	for (const close of cleanup.splice(0).reverse()) await close();
});

it.each([false, true])(
	"C4 resume carries displayed identity and delayed pause owns the same child (rebound=%s)",
	async (rebound) => {
		const root = await mkdtemp(join(tmpdir(), "astra-workbench-identity-"));
		cleanup.push(() => rm(root, { recursive: true, force: true }));
		const project = join(root, "run-owned");
		await mkdir(project);
		const store = new JsonlAstraStore(project);
		const a = await ResearchJob.create(store, { workspaceRoot: project, objective: "original bound research" });
		const b = await ResearchJob.create(store, { workspaceRoot: project, objective: "displayed research" });
		await a.pause("initial");
		await b.pause("initial");
		await atomicWriteJson(join(project, ".astra/active-job.json"), { jobId: b.state.frame.jobId });
		vi.stubEnv("ASTRA_JOB_ID", a.state.frame.jobId);
		const app = await startWorkbench({
			root,
			port: 0,
			runnerPath: fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)),
		});
		cleanup.push(async () => {
			for (const state of app.running.values()) state.child?.kill("SIGINT");
			await new Promise<void>((resolve) => app.server.close(() => resolve()));
		});
		const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
			token: string;
			jobs: Array<{ id: string }>;
		};
		const id = listing.jobs[0].id;
		const post = (action: string, jobId: string) =>
			fetch(`${app.url}/api/${action}?id=${id}`, {
				method: "POST",
				headers: { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" },
				body: JSON.stringify({ jobId }),
			});
		expect((await post("resume", a.state.frame.jobId)).status).toBe(400);
		expect((await post("resume", b.state.frame.jobId)).status).toBe(202);
		await expect
			.poll(async () => JSON.parse(await readFile(join(project, "fixture-request.json"), "utf8")).request.jobId)
			.toBe(b.state.frame.jobId);
		await expect.poll(async () => (await ResearchJob.open(store, b.state.frame.jobId))!.state.paused).toBe(false);
		if (rebound) await atomicWriteJson(join(project, ".astra/active-job.json"), { jobId: a.state.frame.jobId });
		expect((await post("pause", b.state.frame.jobId)).status).toBe(202);
		await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
		expect((await ResearchJob.open(store, b.state.frame.jobId))!.state.paused).toBe(true);
		expect((await ResearchJob.open(store, a.state.frame.jobId))!.state.paused).toBe(true);
		expect(JSON.parse(await readFile(join(project, "fixture-request.json"), "utf8")).request).toMatchObject({
			action: "pause",
			jobId: b.state.frame.jobId,
		});
	},
);

it.each([true, false])("C4 page resets job selections on rebound (same sequence=%s)", async (sameSequence) => {
	const root = await mkdtemp(join(tmpdir(), "astra-workbench-page-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	const project = join(root, "run-page");
	await mkdir(project);
	const store = new JsonlAstraStore(project);
	const a = await ResearchJob.create(store, { workspaceRoot: project, objective: "first objective" });
	const b = await ResearchJob.create(store, { workspaceRoot: project, objective: "new objective" });
	await a.pause("initial");
	await b.pause("initial");
	if (!sameSequence) await b.recordUserGuidance("different sequence");
	await atomicWriteJson(join(project, ".astra/active-job.json"), { jobId: a.state.frame.jobId });
	const app = await startWorkbench({ root, port: 0 });
	cleanup.push(() => new Promise<void>((resolve) => app.server.close(() => resolve())));
	class Element {
		textContent = "";
		hidden = false;
		children: unknown[] = [];
		value = "";
		dataset = {};
		append(...children: unknown[]) {
			this.children.push(...children);
		}
		replaceChildren(...children: unknown[]) {
			this.children = children;
			this.textContent = "";
		}
		focus() {}
	}
	const elements = new Map<string, Element>();
	const get = (id: string) => {
		if (!elements.has(id)) elements.set(id, new Element());
		return elements.get(id)!;
	};
	get("research").hidden = true;
	const context = createContext({
		document: { getElementById: get, createElement: () => new Element(), documentElement: new Element() },
		fetch: (path: string, options: RequestInit) => fetch(app.url + path, options),
		setInterval: () => 1,
		URLSearchParams,
		Date,
		console,
	});
	runInContext(await readFile(fileURLToPath(new URL("../web/app.js", import.meta.url)), "utf8"), context);
	await expect.poll(() => runInContext("fetching", context)).toBe(false);
	expect(get("title").textContent).toBe("first objective");
	runInContext('stageId = "paper-write"; evidenceId = "old-evidence"', context);
	await atomicWriteJson(join(project, ".astra/active-job.json"), { jobId: b.state.frame.jobId });
	await runInContext("refresh()", context);
	expect(get("title").textContent).toBe("new objective");
	expect(runInContext("stageId", context)).toBe(b.state.frame.activeStageId);
	expect(runInContext("evidenceId", context)).toBe("");
	runInContext(
		"current = { readonly: false, running: true, canPause: true, snapshot: undefined }; renderJob()",
		context,
	);
	expect(get("pause").hidden).toBe(false);
});

it("downloads frozen evidence after original files change or disappear and rejects corrupt snapshots", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-download-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	const job = await ResearchJob.create(new JsonlAstraStore(root), {
		workspaceRoot: root,
		objective: "download version",
	});
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "download version",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["write"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 2, maxToolCalls: 2, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["verified"],
	});
	const workspace = taskWorkspacePath(root, task.jobId, task.id);
	await mkdir(workspace, { recursive: true });
	const ref = `.astra/jobs/${task.jobId}/workspaces/${task.id}/result.txt`;
	await writeFile(join(root, ref), "reviewed bytes");
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: "validation",
		refs: [ref],
		content: { content: "verified" },
	});
	await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
	await job.decideEvidence(evidence.id, true);
	const artifact = await job.adoptEvidence(evidence.id);
	await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: task.jobId }));
	const app = await startWorkbench({ root: join(root, "workbench"), watch: [root], port: 0 });
	cleanup.push(() => new Promise<void>((ok, reject) => app.server.close((error) => (error ? reject(error) : ok()))));
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { jobs: Array<{ id: string }> };
	const url = `${app.url}/api/file?${new URLSearchParams({ id: listing.jobs[0].id, artifact: artifact.id, ref })}`;
	await writeFile(join(root, ref), "unreviewed replacement");
	expect(await (await fetch(url)).text()).toBe("reviewed bytes");
	await rm(join(root, ref));
	expect(await (await fetch(url)).text()).toBe("reviewed bytes");
	const frozen = join(root, ".astra/jobs", task.jobId, "versions/files", evidence.files![0].sha256);
	await rm(frozen);
	await writeFile(frozen, "corrupt");
	expect((await fetch(url)).status).toBe(400);
	await rm(frozen);
	expect((await fetch(url)).status).toBe(400);
});

it("serves real state, isolates new runs, preserves read-only imports and pauses its own child", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-workbench-test-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	const app = await startWorkbench({
		root,
		watch: [join(root, "readonly")],
		port: 0,
		runnerPath: fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)),
	});
	cleanup.push(async () => {
		for (const state of app.running.values()) state.child?.kill("SIGINT");
		await new Promise<void>((ok, reject) => app.server.close((error) => (error ? reject(error) : ok())));
	});
	const initial = (await (await fetch(`${app.url}/api/jobs`)).json()) as {
		token: string;
		jobs: Array<{ id: string }>;
	};
	const headers = { Origin: app.url, "X-Astra-Token": initial.token, "Content-Type": "application/json" };
	expect(await (await fetch(app.url)).text()).toContain("研究工作台");
	for (const host of ["localhost:4319", "127.0.0.1:14319", "[::1]:14319"]) {
		expect(await forwardedStatus(`${app.url}/api/jobs`, { Host: host, Origin: `http://${host}` })).toBe(200);
		expect(
			await forwardedStatus(`${app.url}/api/run`, { ...headers, Host: host, Origin: `http://${host}` }, "POST"),
		).toBe(400);
	}
	expect(await forwardedStatus(`${app.url}/api/jobs`, { Host: "attacker.invalid:4319" })).toBe(403);
	expect(
		await forwardedStatus(`${app.url}/api/jobs`, { Host: "localhost:4319", Origin: "http://localhost:9999" }),
	).toBe(403);
	expect((await fetch(`${app.url}/api/run`, { method: "POST", body: "{}" })).status).toBe(403);
	expect((await fetch(`${app.url}/api/jobs`, { headers: { Origin: "http://other.invalid" } })).status).toBe(403);
	expect(
		(await fetch(`${app.url}/api/resume?id=${initial.jobs[0].id}`, { method: "POST", headers, body: "{}" })).status,
	).toBe(400);
	expect(
		(
			await fetch(`${app.url}/api/run`, {
				method: "POST",
				headers,
				body: JSON.stringify({ objective: "small", maxTasks: 0 }),
			})
		).status,
	).toBe(400);
	const response = await fetch(`${app.url}/api/run`, {
		method: "POST",
		headers,
		body: JSON.stringify({ objective: "A bounded test without paid model calls", maxTasks: 24, requirePaper: false }),
	});
	expect(response.status).toBe(202);
	const { id } = (await response.json()) as { id: string };
	const getJob = async () =>
		(await (await fetch(`${app.url}/api/job?id=${id}`)).json()) as {
			root: string;
			running: boolean;
			snapshot: { paused: boolean; frame: { jobId: string } };
		};
	await expect.poll(async () => (await getJob()).running).toBe(false);
	const created = await getJob();
	expect(created.root).toContain(join(root, "run-"));
	expect(created.snapshot.paused).toBe(true);
	expect(JSON.parse(await readFile(join(created.root, "fixture-request.json"), "utf8"))).toMatchObject({
		model: "gpt-5.6-luna",
		request: { backend: "codex", maxTasks: 24 },
	});
	expect((await fetch(`${app.url}/api/file?id=${id}&artifact=missing&ref=../../etc/passwd`)).status).toBe(400);
	expect(
		(
			await fetch(`${app.url}/api/resume?id=${id}`, {
				method: "POST",
				headers,
				body: JSON.stringify({ jobId: created.snapshot.frame.jobId, guidance: "Keep the scope fixed" }),
			})
		).status,
	).toBe(202);
	await expect.poll(async () => (await getJob()).snapshot.paused).toBe(false);
	expect((await fetch(`${app.url}/api/resume?id=${id}`, { method: "POST", headers, body: "{}" })).status).toBe(400);
	expect(
		(
			await fetch(`${app.url}/api/pause?id=${id}`, {
				method: "POST",
				headers,
				body: JSON.stringify({ jobId: created.snapshot.frame.jobId }),
			})
		).status,
	).toBe(202);
	await expect.poll(async () => (await getJob()).running).toBe(false);
	expect((await getJob()).snapshot.paused).toBe(true);
});

it.each(["delayed-publication", "failed-after-publication", "missing-publication"])(
	"C4 keeps exact published identity across %s and close",
	async (mode) => {
		const root = await mkdtemp(join(tmpdir(), "astra-workbench-publication-"));
		cleanup.push(() => rm(root, { recursive: true, force: true }));
		vi.stubEnv("ASTRA_FAKE_WORKBENCH_MODE", mode);
		const app = await startWorkbench({
			root,
			port: 0,
			runnerPath: fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)),
		});
		cleanup.push(async () => {
			for (const state of app.running.values()) state.child?.kill("SIGINT");
			await new Promise<void>((resolve) => app.server.close(() => resolve()));
		});
		const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { token: string };
		const headers = { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" };
		const response = await fetch(`${app.url}/api/run`, {
			method: "POST",
			headers,
			body: JSON.stringify({
				objective: "publication identity without real models",
				maxTasks: 4,
				requirePaper: false,
			}),
		});
		expect(response.status).toBe(202);
		const { id } = (await response.json()) as { id: string };
		const getJob = async () =>
			(await (await fetch(`${app.url}/api/job?id=${id}`)).json()) as {
				root: string;
				snapshot?: { paused: boolean; frame: { jobId: string } };
			};
		if (mode === "delayed-publication") {
			const data = await getJob();
			await expect
				.poll(async () => readFile(join(data.root, "fixture-request.json"), "utf8"))
				.toContain('"action":"run"');
			expect(app.running.get(id)?.jobId).toBeUndefined();
			const kill = vi.spyOn(app.running.get(id)!.child!, "kill");
			expect((await fetch(`${app.url}/api/pause?id=${id}`, { method: "POST", headers, body: "{}" })).status).toBe(
				202,
			);
			expect(kill).not.toHaveBeenCalled();
		}
		await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
		const final = await getJob();
		if (mode === "missing-publication") expect(app.running.get(id)?.error).toContain("没有发布研究身份");
		else {
			expect(app.running.get(id)?.jobId).toBe(final.snapshot!.frame.jobId);
			if (mode === "delayed-publication") {
				expect(final.snapshot!.paused).toBe(true);
				expect(JSON.parse(await readFile(join(final.root, "fixture-request.json"), "utf8")).request).toMatchObject({
					action: "pause",
					jobId: final.snapshot!.frame.jobId,
				});
			}
		}
	},
);

it("C4 actual control runner flushes the publication before a zero-tick drive failure exits", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-workbench-real-publication-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	vi.stubEnv("ASTRA_MAX_TICKS", "0");
	const app = await startWorkbench({ root, port: 0 });
	cleanup.push(() => new Promise<void>((resolve) => app.server.close(() => resolve())));
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { token: string };
	const response = await fetch(`${app.url}/api/run`, {
		method: "POST",
		headers: { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" },
		body: JSON.stringify({ objective: "zero model publication boundary", maxTasks: 4, requirePaper: false }),
	});
	expect(response.status).toBe(202);
	const { id } = (await response.json()) as { id: string };
	await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
	const current = (await (await fetch(`${app.url}/api/job?id=${id}`)).json()) as {
		snapshot: { frame: { jobId: string } };
		output: string;
	};
	expect(app.running.get(id)?.jobId).toBe(current.snapshot.frame.jobId);
	expect(current.output).toContain("research run exceeded 0 ticks");
});

it("C4 closed child output and error belong only to the published job and remain when selected again", async () => {
	const root = await mkdtemp(join(tmpdir(), "astra-workbench-output-"));
	cleanup.push(() => rm(root, { recursive: true, force: true }));
	vi.stubEnv("ASTRA_FAKE_WORKBENCH_MODE", "failed-after-publication");
	const app = await startWorkbench({
		root,
		port: 0,
		runnerPath: fileURLToPath(new URL("./fixtures/workbench-runner.mjs", import.meta.url)),
	});
	cleanup.push(() => new Promise<void>((resolve) => app.server.close(() => resolve())));
	const listing = (await (await fetch(`${app.url}/api/jobs`)).json()) as { token: string };
	const response = await fetch(`${app.url}/api/run`, {
		method: "POST",
		headers: { Origin: app.url, "X-Astra-Token": listing.token, "Content-Type": "application/json" },
		body: JSON.stringify({
			objective: "keep output bound to its published research",
			maxTasks: 4,
			requirePaper: false,
		}),
	});
	const { id } = (await response.json()) as { id: string };
	await expect.poll(() => Boolean(app.running.get(id)?.child)).toBe(false);
	const get = async () =>
		(await (await fetch(`${app.url}/api/job?id=${id}`)).json()) as {
			root: string;
			snapshot: { frame: { jobId: string } };
			output: string;
			error?: string;
		};
	const a = await get();
	expect(a.output).toContain(`fixture-job-error:${a.snapshot.frame.jobId}`);
	expect(a.error).toBeDefined();
	const b = await ResearchJob.create(new JsonlAstraStore(a.root), {
		workspaceRoot: a.root,
		objective: "different displayed research",
	});
	await atomicWriteJson(join(a.root, ".astra/active-job.json"), { jobId: b.state.frame.jobId });
	const displayed = await get();
	expect(displayed.snapshot.frame.jobId).toBe(b.state.frame.jobId);
	expect(displayed.output).toBe("");
	expect(displayed.error).toBeUndefined();
	const rows = (await (await fetch(`${app.url}/api/jobs`)).json()) as { jobs: Array<{ id: string; error?: string }> };
	expect(rows.jobs.find((row) => row.id === id)?.error).toBeUndefined();
	await atomicWriteJson(join(a.root, ".astra/active-job.json"), { jobId: a.snapshot.frame.jobId });
	expect(await get()).toMatchObject({ output: a.output, error: a.error });
});
