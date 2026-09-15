import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it } from "vitest";
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
	for (const close of cleanup.splice(0).reverse()) await close();
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
			snapshot: { paused: boolean };
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
				body: JSON.stringify({ guidance: "Keep the scope fixed" }),
			})
		).status,
	).toBe(202);
	await expect.poll(async () => (await getJob()).snapshot.paused).toBe(false);
	expect((await fetch(`${app.url}/api/resume?id=${id}`, { method: "POST", headers, body: "{}" })).status).toBe(400);
	expect((await fetch(`${app.url}/api/pause?id=${id}`, { method: "POST", headers, body: "{}" })).status).toBe(202);
	await expect.poll(async () => (await getJob()).running).toBe(false);
	expect((await getJob()).snapshot.paused).toBe(true);
});
