import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import test from "node:test";

test("smoke replacement creates a real research identity and flushes IPC (source-level protocol fixture only)", async () => {
	const script = await readFile(new URL("./smoke-astra-package.mjs", import.meta.url), "utf8");
	const expression = script.match(/join\(root, "dist\/workbench-runner\.js"\),\s*([\s\S]*?)\n\t\);/)?.[1]?.replace(/,\s*$/, "");
	assert(expression, "replacement runner source must be present");
	const runner = runInNewContext(expression);
	const root = await mkdtemp(join(tmpdir(), "astra-smoke-protocol-"));
	let child;
	try {
		await mkdir(join(root, "dist"));
		await mkdir(join(root, "run"));
		await writeFile(join(root, "package.json"), '{"type":"module"}');
		// Explicit source-level fixtures: this is not an installed or freshly compiled package test.
		for (const name of ["research", "store"]) await writeFile(join(root, "dist", `${name}.js`), `export * from ${JSON.stringify(new URL(`../packages/astra/src/${name}.ts`, import.meta.url).href)};\n`);
		await writeFile(join(root, "dist/workbench-runner.js"), runner);
		const messages = [];
		child = spawn(process.execPath, [join(root, "dist/workbench-runner.js")], { cwd: join(root, "run"), stdio: ["pipe", "pipe", "pipe", "ipc"] });
		let output = "";
		child.stdout.on("data", (chunk) => { output += chunk; });
		child.stderr.on("data", (chunk) => { output += chunk; });
		child.on("message", (message) => messages.push(message));
		child.stdin.end(JSON.stringify({ action: "run", backend: "codex", objective: "Offline package IPC protocol", maxTasks: 4, requirePaper: false }));
		const code = await new Promise((resolve) => child.on("close", resolve));
		assert.equal(code, 0, output);
		assert.equal(messages.length, 1);
		assert.equal(messages[0].type, "astra/job-published");
		const active = JSON.parse(await readFile(join(root, "run/.astra/active-job.json"), "utf8"));
		assert.equal(active.jobId, messages[0].jobId);
		assert.equal(JSON.parse(await readFile(join(root, "run/.astra/jobs", active.jobId, "job.json"), "utf8")).frame.jobId, active.jobId);
		assert.match(runner, /from ["']\.\/research\.js["']/);
		assert.match(runner, /from ["']\.\/store\.js["']/);
		for (const specifier of runner.matchAll(/from ["']([^"']+)["']/g)) assert.doesNotMatch(specifier[1], /\.ts|workspace|file:\/\/|\/src\//);
	} finally {
		if (child && child.exitCode === null) { child.kill("SIGTERM"); await new Promise((resolve) => child.once("close", resolve)); }
		await rm(root, { recursive: true, force: true });
	}
});
