import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

function fixture() {
	const root = mkdtempSync(join(tmpdir(), "astra-pack-transaction-"));
	mkdirSync(join(root, "scripts"));
	mkdirSync(join(root, "packages/astra/dist"), { recursive: true });
	mkdirSync(join(root, "packages/astra/web"));
	copyFileSync(new URL("./package-astra.mjs", import.meta.url), join(root, "scripts/package.mjs"));
	writeFileSync(join(root, "packages/astra/package.json"), JSON.stringify({ name: "astra-offline-package-fixture", version: "0.0.1", type: "module", files: ["dist", "web"] }));
	for (const path of ["dist/launcher.js", "dist/workbench.js", "dist/workbench-runner.js", "web/index.html"]) writeFileSync(join(root, "packages/astra", path), "offline fixture\n");
	const hook = join(root, "fault.mjs");
	writeFileSync(hook, `import child from "node:child_process"; import fs from "node:fs"; import { syncBuiltinESMExports } from "node:module";
const exec = child.execFileSync; const write = fs.writeFileSync; const rename = fs.renameSync;
const fault = process.env.ASTRA_PACKAGE_TEST_FAULT;
child.execFileSync = (file, args, options) => {
 if (file === "git") { if (fault === "git") throw new Error("injected git failure"); return args[0] === "status" ? "" : "a".repeat(40); }
 if (file === "npm" || file === "npm.cmd") { if (fault === "npm") throw new Error("injected npm failure"); }
 return exec(file, args, options);
};
fs.writeFileSync = (path, ...args) => { if (typeof path === "string" && ((fault === "checksum" && path.endsWith("/SHA256SUMS")) || (fault === "metadata" && path.endsWith("/PACKAGE_SOURCE.json")))) throw new Error("injected " + fault + " failure"); return write(path, ...args); };
fs.renameSync = (...args) => { if (fault === "rename") throw new Error("injected rename failure"); return rename(...args); };
syncBuiltinESMExports();`);
	const args = ["--import", hook, join(root, "scripts/package.mjs")];
	const options = (fault = "") => ({ cwd: root, encoding: "utf8", env: { ...process.env, npm_config_offline: "true", ASTRA_PACKAGE_TEST_FAULT: fault } });
	return { root, output: join(root, ".artifacts/astra-v0.0.1/package"), run: (fault) => spawnSync(process.execPath, args, options(fault)), start: () => spawn(process.execPath, args, options()), close: () => rmSync(root, { recursive: true, force: true }) };
}

for (const fault of ["npm", "git", "checksum", "metadata", "rename"]) {
	test(`package ${fault} failure leaves no final output, clears its staging and permits same-version retry`, () => {
		const f = fixture();
		try {
			const failed = f.run(fault);
			assert.notEqual(failed.status, 0);
			assert.match(failed.stderr, /injected/);
			assert.equal(existsSync(f.output), false);
			assert.deepEqual(readdirSync(join(f.root, ".artifacts/astra-v0.0.1")), []);
			const success = f.run();
			assert.equal(success.status, 0, success.stderr);
			const source = readFileSync(join(f.output, "PACKAGE_SOURCE.json"));
			assert.equal(JSON.parse(source).baseCommit, "a".repeat(40));
			assert.match(readFileSync(join(f.output, "SHA256SUMS"), "utf8"), /^[a-f0-9]{64}  astra-offline-package-fixture-0\.0\.1\.tgz\n$/);
			assert.notEqual(f.run().status, 0);
			assert.deepEqual(readFileSync(join(f.output, "PACKAGE_SOURCE.json")), source);
		} finally { f.close(); }
	});
}

test("two ordinary concurrent package calls publish one complete output and clean only their own staging", async () => {
	const f = fixture();
	try {
		const children = [f.start(), f.start()];
		const codes = await Promise.all(children.map((child) => new Promise((resolve) => { child.stdout.resume(); child.stderr.resume(); child.on("close", resolve); })));
		assert.equal(codes.filter((code) => code === 0).length, 1);
		assert.equal(existsSync(join(f.output, "PACKAGE_SOURCE.json")), true);
		assert.deepEqual(readdirSync(join(f.root, ".artifacts/astra-v0.0.1")), ["package"]);
	} finally { f.close(); }
});
