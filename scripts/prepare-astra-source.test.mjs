import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

test("source export omits archived/deleted files and includes current docs and Astra CI", () => {
	const root = mkdtempSync(join(tmpdir(), "astra-export-"));
	try {
		const put = (path, text) => { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), text); };
		put("package.json", '{"type":"module"}');
		put("packages/astra/package.json", '{"version":"0.1.0-alpha.2"}');
		put("packages/ai/scripts/check-model-data.ts", "// Model data validator stub for export selection only.\n");
		put("packages/ai/src/providers/data/example.json", "{}");
		put("legacy/rust/Cargo.toml", "archive");
		put("docs/deleted.md", "removed");
		put("scripts/prepare-astra-source.mjs", "");
		copyFileSync(new URL("./prepare-astra-source.mjs", import.meta.url), join(root, "scripts/prepare-astra-source.mjs"));
		execFileSync("git", ["init", "-q"], { cwd: root });
		execFileSync("git", ["add", "package.json", "packages", "legacy", "docs", "scripts"], { cwd: root });
		execFileSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture"], { cwd: root });
		rmSync(join(root, "docs/deleted.md"));
		put("docs/getting-started.md", "new user guide");
		put("README.en.md", "English entry point");
		put(".github/workflows/astra.yml", "name: Astra");
		put(".env", "PRIVATE_TEST_VALUE=never-export");
		put(".astra/job.json", "private state");
		const target = join(root, "output");
		const result = spawnSync(process.execPath, [join(root, "scripts/prepare-astra-source.mjs"), target], { encoding: "utf8" });
		assert.equal(result.status, 0, result.stderr);
		assert.equal(existsSync(join(target, "legacy")), false);
		assert.equal(existsSync(join(target, "docs/deleted.md")), false);
		assert.equal(existsSync(join(target, ".env")), false);
		assert.equal(existsSync(join(target, ".astra")), false);
		assert.equal(readFileSync(join(target, "docs/getting-started.md"), "utf8"), "new user guide");
		assert.equal(readFileSync(join(target, "README.en.md"), "utf8"), "English entry point");
		assert.equal(existsSync(join(target, ".github/workflows/astra.yml")), true);
		const manifest = JSON.parse(readFileSync(join(target, "SOURCE_MANIFEST.json"), "utf8"));
		assert.equal(manifest.version, "0.1.0-alpha.2");
		assert.equal(manifest.includesUncommittedChanges, true);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

function exportFixture() {
	const root = mkdtempSync(join(tmpdir(), "astra-export-integrity-"));
	const put = (path, bytes) => { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), bytes); };
	put("package.json", '{"type":"module"}');
	put(".gitignore", "output/\n");
	put("packages/astra/package.json", '{"version":"0.1.0-fixture"}');
	put("packages/ai/scripts/check-model-data.ts", "// Catalog validator is outside export consistency tests.\n");
	put("packages/ai/src/providers/data/example.json", "{}");
	put("README.md", "original README\n");
	put("scripts/executable.sh", "#!/bin/sh\nexit 0\n");
	chmodSync(join(root, "scripts/executable.sh"), 0o755);
	put("scripts/prepare-astra-source.mjs", readFileSync(new URL("./prepare-astra-source.mjs", import.meta.url)));
	execFileSync("git", ["init", "-q"], { cwd: root });
	execFileSync("git", ["add", ".gitignore", "package.json", "packages", "scripts", "README.md"], { cwd: root });
	execFileSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture"], { cwd: root });
	const baseCommit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
	const hooks = mkdtempSync(join(tmpdir(), "astra-export-hook-"));
	const target = join(root, "output");
	const run = (hook, destination = target) => {
		const args = [];
		if (hook) {
			const path = join(hooks, "hook.mjs");
			writeFileSync(path, `import fs from "node:fs";\nimport { syncBuiltinESMExports } from "node:module";\nimport { execFileSync } from "node:child_process";\nconst root = ${JSON.stringify(root)};\n${hook}\nsyncBuiltinESMExports();\n`);
			args.push("--import", path);
		}
		return spawnSync(process.execPath, [...args, join(root, "scripts/prepare-astra-source.mjs"), destination], { encoding: "utf8" });
	};
	return { root, target, put, run, baseCommit, close: () => { rmSync(root, { recursive: true, force: true }); rmSync(hooks, { recursive: true, force: true }); } };
}

test("source export writes the scanned bytes once and preserves executable mode", () => {
	const fixture = exportFixture();
	try {
		const result = fixture.run(`const read = fs.readFileSync; let changed = false;
fs.readFileSync = (path, ...args) => { const bytes = read(path, ...args); if (!changed && path === root + "/README.md") { changed = true; fs.writeFileSync(path, "saved after scan\\n"); } return bytes; };`);
		assert.equal(result.status, 0, result.stderr);
		const manifest = JSON.parse(readFileSync(join(fixture.target, "SOURCE_MANIFEST.json")));
		for (const file of manifest.files) {
			const bytes = readFileSync(join(fixture.target, file.path));
			assert.equal(file.sha256, createHash("sha256").update(bytes).digest("hex"), file.path);
			assert.equal(file.size, bytes.length, file.path);
		}
		assert.equal(statSync(join(fixture.target, "scripts/executable.sh")).mode & 0o777, 0o755);
	} finally { fixture.close(); }
});

test("source export failure leaves no target or staging directory and the destination can be retried", () => {
	const fixture = exportFixture();
	try {
		const before = readdirSync(fixture.root).sort();
		const result = fixture.run(`const copy = fs.copyFileSync; const write = fs.writeFileSync;
fs.copyFileSync = (source, target, ...args) => { if (source === root + "/README.md") throw new Error("injected output failure"); return copy(source, target, ...args); };
fs.writeFileSync = (path, ...args) => { if (typeof path === "string" && path.endsWith("/README.md") && path !== root + "/README.md") throw new Error("injected output failure"); return write(path, ...args); };`);
		assert.notEqual(result.status, 0);
		assert.equal(existsSync(fixture.target), false);
		assert.deepEqual(readdirSync(fixture.root).sort(), before);
		const retry = fixture.run();
		assert.equal(retry.status, 0, retry.stderr);
		const manifest = readFileSync(join(fixture.target, "SOURCE_MANIFEST.json"));
		assert.notEqual(fixture.run().status, 0);
		assert.deepEqual(readFileSync(join(fixture.target, "SOURCE_MANIFEST.json")), manifest);
	} finally { fixture.close(); }
});

test("source export refuses a HEAD change after output bytes were written", () => {
	const fixture = exportFixture();
	try {
		const result = fixture.run(`let changed = false; const copy = fs.copyFileSync; const write = fs.writeFileSync;
const commit = () => { if (changed) return; changed = true; write(root + "/README.md", "new committed README\\n"); execFileSync("git", ["add", "README.md"], { cwd: root }); execFileSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "concurrent commit"], { cwd: root }); };
fs.copyFileSync = (source, target, ...args) => { const value = copy(source, target, ...args); if (source === root + "/README.md") commit(); return value; };
fs.writeFileSync = (path, ...args) => { const value = write(path, ...args); if (typeof path === "string" && path.endsWith("/README.md") && path !== root + "/README.md") commit(); return value; };`);
		assert.notEqual(result.status, 0);
		assert.match(result.stderr, /HEAD|commit/i);
		assert.equal(existsSync(fixture.target), false);
	} finally { fixture.close(); }
});

test("clean source provenance requires captured commit bytes and modes even after a transient edit", () => {
	const fixture = exportFixture();
	try {
		const clean = fixture.run();
		assert.equal(clean.status, 0, clean.stderr);
		const cleanManifest = JSON.parse(readFileSync(join(fixture.target, "SOURCE_MANIFEST.json")));
		assert.equal(cleanManifest.baseCommit, fixture.baseCommit);
		assert.equal(cleanManifest.includesUncommittedChanges, false);
		rmSync(fixture.target, { recursive: true });
		const result = fixture.run(`const read = fs.readFileSync; const write = fs.writeFileSync; let changed = false;
fs.readFileSync = (path, ...args) => { if (!changed && path === root + "/README.md") { changed = true; const original = read(path); write(path, "temporary exported content\\n"); const bytes = read(path, ...args); write(path, original); return bytes; } return read(path, ...args); };`);
		assert.equal(result.status, 0, result.stderr);
		const manifest = JSON.parse(readFileSync(join(fixture.target, "SOURCE_MANIFEST.json")));
		assert.equal(manifest.baseCommit, fixture.baseCommit);
		assert.equal(manifest.includesUncommittedChanges, true);
		rmSync(fixture.target, { recursive: true });
		chmodSync(join(fixture.root, "scripts/executable.sh"), 0o644);
		const modeChanged = fixture.run();
		assert.equal(modeChanged.status, 0, modeChanged.stderr);
		assert.equal(JSON.parse(readFileSync(join(fixture.target, "SOURCE_MANIFEST.json"))).includesUncommittedChanges, true);
		assert.equal(statSync(join(fixture.target, "scripts/executable.sh")).mode & 0o777, 0o644);
		rmSync(fixture.target, { recursive: true });
		chmodSync(join(fixture.root, "scripts/executable.sh"), 0o755);
		rmSync(join(fixture.root, "README.md"));
		const missing = fixture.run();
		assert.equal(missing.status, 0, missing.stderr);
		assert.equal(JSON.parse(readFileSync(join(fixture.target, "SOURCE_MANIFEST.json"))).includesUncommittedChanges, true);
	} finally { fixture.close(); }
});

test("source export keeps secret and non-regular-file checks before publication", () => {
	const fixture = exportFixture();
	try {
		fixture.put("docs/private.txt", "-----BEGIN PRIVATE KEY-----\nfixture\n");
		const secret = fixture.run();
		assert.notEqual(secret.status, 0);
		assert.match(secret.stderr, /Potential private data/);
		assert.equal(existsSync(fixture.target), false);
		rmSync(join(fixture.root, "docs/private.txt"));
		rmSync(join(fixture.root, "README.md"));
		mkdirSync(join(fixture.root, "README.md"));
		const nonregular = fixture.run();
		assert.notEqual(nonregular.status, 0);
		assert.match(nonregular.stderr, /Non-regular file/);
		assert.equal(existsSync(fixture.target), false);
	} finally { fixture.close(); }
});

test("source export version comes from exported package bytes and ignores its own output tree", () => {
	const fixture = exportFixture();
	try {
		const result = fixture.run(`const read = fs.readFileSync; let count = 0;
fs.readFileSync = (path, ...args) => { if (path === root + "/packages/astra/package.json" && ++count === 2) fs.writeFileSync(path, '{"version":"0.1.0-saved"}'); return read(path, ...args); };`);
		assert.equal(result.status, 0, result.stderr);
		const first = fixture.target;
		const manifest = JSON.parse(readFileSync(join(first, "SOURCE_MANIFEST.json")));
		assert.equal(manifest.version, JSON.parse(readFileSync(join(first, "packages/astra/package.json"))).version);
		const overlap = join(fixture.root, "docs", "export");
		const repeated = fixture.run(undefined, overlap);
		assert.notEqual(repeated.status, 0);
		assert.match(repeated.stderr, /overlap/i);
		assert.equal(existsSync(overlap), false);
	} finally { fixture.close(); }
});

test("source export rejects physical source overlap through existing parent aliases and missing suffixes", () => {
	const fixture = exportFixture();
	const external = mkdtempSync(join(tmpdir(), "astra-export-parent-"));
	try {
		mkdirSync(join(fixture.root, "docs"));
		symlinkSync(join(fixture.root, "docs"), join(external, "docs-alias"), "dir");
		symlinkSync(fixture.root, join(external, "root-alias"), "dir");
		for (const target of [join(external, "docs-alias", "missing", "export"), join(external, "root-alias", "scripts", "new-export"), join(external, "root-alias", "README.md")]) {
			const result = fixture.run(undefined, target);
			assert.notEqual(result.status, 0, target);
			assert.match(result.stderr, /overlap/i);
			if (!target.endsWith("README.md")) assert.equal(existsSync(target), false);
		}
		const allowed = join(external, "missing", "safe", "export");
		assert.equal(fixture.run(undefined, allowed).status, 0);
		assert.equal(existsSync(join(allowed, "SOURCE_MANIFEST.json")), true);
	} finally { fixture.close(); rmSync(external, { recursive: true, force: true }); }
});
