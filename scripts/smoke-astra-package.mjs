import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { cp, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

// Run against an unpacked release package, without model calls or workspace imports.
const source = resolve(process.argv[2] ?? "packages/astra");
const root = await mkdtemp(join(tmpdir(), "astra-package-smoke-"));
let child;
try {
	await cp(join(source, "dist"), join(root, "dist"), { recursive: true });
	await cp(join(source, "web"), join(root, "web"), { recursive: true });
	const { dependencies } = JSON.parse(await readFile(join(source, "package.json"), "utf8"));
	await writeFile(join(root, "package.json"), JSON.stringify({ type: "module", dependencies }));
	// A real installation includes runtime dependencies; never resolve this checkout's workspaces.
	execFileSync(process.platform === "win32" ? "npm.cmd" : "npm", ["install", "--ignore-scripts", "--omit=dev", "--no-audit", "--no-fund"], {
		cwd: root,
		stdio: "inherit",
	});
	// Replace only the model-facing runner; the compiled server must resolve it itself.
	await writeFile(
		join(root, "dist/workbench-runner.js"),
		`import { writeFile } from "node:fs/promises";
import { ResearchJob } from "./research.js";
import { JsonlAstraStore } from "./store.js";
let body = "";
for await (const chunk of process.stdin) body += chunk;
await writeFile("request.json", body);
const request = JSON.parse(body);
const store = new JsonlAstraStore(process.cwd());
const job = await ResearchJob.create(store, { objective: request.objective, workspaceRoot: process.cwd(), maxTasks: request.maxTasks });
await store.withExecutionLock(job.state.frame.jobId, "package-smoke", async () => {
 await writeFile(".astra/active-job.json", JSON.stringify({ jobId: job.state.frame.jobId }));
 await new Promise((resolve, reject) => process.send({ type: "astra/job-published", jobId: job.state.frame.jobId }, (error) => error ? reject(error) : resolve()));
});
if (process.connected) process.disconnect();
`,
	);
	await symlink(join(root, "dist/workbench.js"), join(root, "astra-workbench"));
	child = spawn(process.execPath, [join(root, "astra-workbench"), "--root", join(root, "runs"), "--port", "0"], {
		stdio: ["ignore", "pipe", "pipe"],
	});
	let output = "";
	child.stdout.on("data", (chunk) => { output += chunk; });
	child.stderr.on("data", (chunk) => { output += chunk; });
	const deadline = Date.now() + 10000;
	let url;
	while (!(url = output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0])) {
		assert(Date.now() < deadline && child.exitCode === null, output || "workbench did not start");
		await new Promise((ok) => setTimeout(ok, 50));
	}
	for (const path of ["/", "/app.js", "/style.css"]) assert.equal((await fetch(url + path)).status, 200);
	const { token } = await (await fetch(`${url}/api/jobs`)).json();
	const response = await fetch(`${url}/api/run`, {
		method: "POST",
		headers: { Origin: url, "X-Astra-Token": token, "Content-Type": "application/json" },
		body: JSON.stringify({ objective: "Offline packaged runner resolution check", maxTasks: 4, requirePaper: false }),
	});
	assert.equal(response.status, 202);
	const { id } = await response.json();
	let job;
	do {
		job = await (await fetch(`${url}/api/job?id=${id}`)).json();
		assert(Date.now() < deadline, "runner did not finish");
		if (job.running) await new Promise((ok) => setTimeout(ok, 50));
	} while (job.running);
	assert.equal(job.error, undefined, job.output);
	assert.equal(JSON.parse(await readFile(join(job.root, "request.json"), "utf8")).backend, "codex");
	console.log("Packaged workbench assets and default runner: passed (no model calls)");
} finally {
	if (child && child.exitCode === null) {
		child.kill("SIGTERM");
		await new Promise((ok) => child.once("close", ok));
	}
	await rm(root, { recursive: true, force: true });
}
