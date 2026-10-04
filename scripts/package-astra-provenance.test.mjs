import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, truncateSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

function fixture() {
	const root = mkdtempSync(join(tmpdir(), "astra-package-provenance-"));
	const hooks = mkdtempSync(join(tmpdir(), "astra-package-provenance-hooks-"));
	const put = (path, bytes) => { mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), bytes); };
	const git = (args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
	put(".gitignore", ".artifacts/\npackages/astra/dist/\n");
	put("packages/astra/package.json", JSON.stringify({ name: "astra-provenance-fixture", version: "1.0.0", type: "module", bin: { astra: "dist/launcher.js" }, files: ["dist", "web", "marker.txt", "source.js"] }));
	put("packages/astra/marker.txt", "A\n");
	for (const path of ["dist/launcher.js", "dist/workbench.js", "dist/workbench-runner.js", "web/index.html"]) put(`packages/astra/${path}`, "#!/usr/bin/env node\n// existing offline fixture\n");
	chmodSync(join(root, "packages/astra/dist/launcher.js"), 0o755);
	put("packages/astra/node_modules/do-not-copy.txt", "not bundled\n");
	put("unrelated.txt", "not part of package\n");
	put("scripts/package.mjs", readFileSync(new URL("./package-astra.mjs", import.meta.url)));
	git(["init", "-q"]);
	git(["config", "user.name", "Fixture"]);
	git(["config", "user.email", "fixture@example.invalid"]);
	git(["config", "core.hooksPath", "/dev/null"]);
	git(["add", ".gitignore", "scripts", "packages/astra/package.json", "packages/astra/marker.txt", "packages/astra/web", "unrelated.txt"]);
	git(["commit", "-qm", "fixture A"]);
	rmSync(join(root, "packages/astra/node_modules"), { recursive: true });
	const baseCommit = git(["rev-parse", "HEAD"]);
	const output = join(root, ".artifacts/astra-v1.0.0/package");
	const run = (hook = "") => {
		const path = join(hooks, "hook.mjs");
		writeFileSync(path, `import child from "node:child_process";\nimport fs from "node:fs";\nimport { syncBuiltinESMExports } from "node:module";\nconst root = ${JSON.stringify(root)};\nconst run = child.execFileSync;\nconst commit = () => { run("git", ["add", "packages/astra"], { cwd: root }); run("git", ["commit", "-qm", "concurrent fixture commit"], { cwd: root }); };\n${hook}\nsyncBuiltinESMExports();\n`);
		return spawnSync(process.execPath, ["--import", path, join(root, "scripts/package.mjs")], { cwd: root, encoding: "utf8", env: { ...process.env, npm_config_offline: "true", npm_config_cache: join(hooks, "npm-cache") } });
	};
	const source = () => JSON.parse(readFileSync(join(output, "PACKAGE_SOURCE.json")));
	const tarball = () => join(output, readdirSync(output).find((path) => path.endsWith(".tgz")));
	const archived = (path) => execFileSync("tar", ["-xOf", tarball(), `package/${path}`]);
	return { root, hooks, put, git, run, baseCommit, output, source, tarball, archived, close: () => { rmSync(root, { recursive: true, force: true }); rmSync(hooks, { recursive: true, force: true }); } };
}

test("stable real npm uses only its selected file snapshot and records hashes, modes and ignored build boundary", () => {
	const f = fixture();
	try {
		const result = f.run(`child.execFileSync = (file, args, options) => {
 if (file !== "npm" && file !== "npm.cmd") return run(file, args, options);
 const value = run(file, args, options);
 if (args.includes("--dry-run")) fs.writeFileSync(root + "/selection.json", JSON.stringify(JSON.parse(value)[0].files.map(entry => entry.path).sort()));
 else {
  if (options.cwd === root + "/packages/astra") throw new Error("live source used for real pack");
  const files = []; const visit = (dir, prefix = "") => { for (const entry of fs.readdirSync(dir, {withFileTypes:true})) { const path = prefix + entry.name; if (entry.isDirectory()) visit(dir + "/" + entry.name, path + "/"); else files.push(path); } }; visit(options.cwd);
  fs.writeFileSync(root + "/snapshot.json", JSON.stringify(files.sort()));
 }
 return value;
};`);
		assert.equal(result.status, 0, result.stderr);
		assert.deepEqual(JSON.parse(readFileSync(join(f.root, "snapshot.json"))), JSON.parse(readFileSync(join(f.root, "selection.json"))));
		const metadata = f.source();
		assert.equal(metadata.baseCommit, f.baseCommit);
		assert.equal(metadata.version, JSON.parse(f.archived("package.json")).version);
		assert.equal(metadata.includesUncommittedChanges, true); // hook observations are real untracked root files
		for (const file of metadata.files) {
			const bytes = f.archived(file.path);
			assert.equal(file.sha256, createHash("sha256").update(bytes).digest("hex"), file.path);
			assert.equal(file.size, bytes.length, file.path);
		}
		const launcher = metadata.files.find((file) => file.path === "dist/launcher.js");
		assert.equal(launcher.source, "ignored");
		assert.equal(launcher.sourceMode, 0o755);
		assert.equal(launcher.matchesBaseCommit, null);
		assert.match(metadata.sourceBoundary, /existing.*not rebuilt/i);
		assert.match(execFileSync("tar", ["-tvzf", f.tarball()], { encoding: "utf8" }), /-rwxr-xr-x.*package\/dist\/launcher\.js/);
		assert.deepEqual(readdirSync(f.output).sort(), ["PACKAGE_SOURCE.json", "SHA256SUMS", "astra-provenance-fixture-1.0.0.tgz"]);
		assert.deepEqual(readdirSync(dirname(f.output)), ["package"]);
	} finally { f.close(); }
});

test("stable HEAD with ignored prebuilt files can produce an honestly clean package", () => {
	const f = fixture();
	try {
		const result = f.run();
		assert.equal(result.status, 0, result.stderr);
		const metadata = f.source();
		assert.equal(metadata.includesUncommittedChanges, false);
		assert.equal(metadata.baseCommit, f.baseCommit);
		assert.deepEqual(metadata.missingCandidatePaths, []);
		assert.equal(metadata.files.find((file) => file.path === "marker.txt").matchesBaseCommit, true);
		assert.equal(metadata.files.find((file) => file.path === "dist/launcher.js").source, "ignored");
	} finally { f.close(); }
});

for (const timing of ["before", "after"]) {
	test(`HEAD commit ${timing} real npm pack is refused without publishing a mixed version`, () => {
		const f = fixture();
		try {
			const result = f.run(`child.execFileSync = (file, args, options) => {
 if ((file === "npm" || file === "npm.cmd") && !args.includes("--dry-run")) {
  const change = () => { const path = root + "/packages/astra/package.json"; const pkg = JSON.parse(fs.readFileSync(path)); pkg.version = "1.0.1"; fs.writeFileSync(path, JSON.stringify(pkg)); commit(); };
  if (${JSON.stringify(timing)} === "before") change();
  const value = run(file, args, options);
  if (${JSON.stringify(timing)} === "after") change();
  return value;
 }
 return run(file, args, options);
};`);
			assert.notEqual(result.status, 0);
			assert.match(result.stderr, /HEAD changed/);
			assert.equal(existsSync(f.output), false);
			assert.notEqual(f.git(["rev-parse", "HEAD"]), f.baseCommit);
			assert.deepEqual(readdirSync(dirname(f.output)), []);
		} finally { f.close(); }
	});
}

test("live A to B to A during real npm pack cannot change the captured clean package", () => {
	const f = fixture();
	try {
		const result = f.run(`child.execFileSync = (file, args, options) => {
 if ((file === "npm" || file === "npm.cmd") && !args.includes("--dry-run")) {
  fs.writeFileSync(root + "/packages/astra/marker.txt", "B\\n");
  const value = run(file, args, options);
  fs.writeFileSync(root + "/packages/astra/marker.txt", "A\\n");
  return value;
 }
 return run(file, args, options);
};`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["status", "--porcelain"]), "");
		assert.equal(f.archived("marker.txt").toString(), "A\n");
		assert.equal(f.source().includesUncommittedChanges, false);
		assert.equal(f.source().files.find((file) => file.path === "marker.txt").matchesBaseCommit, true);
	} finally { f.close(); }
});

test("transient captured tracked bytes remain marked dirty after the live file is restored", () => {
	const f = fixture();
	try {
		const result = f.run(`const read = fs.readFileSync; const open = fs.openSync; const close = fs.closeSync; const fds = new Set();
fs.openSync = (path, ...args) => { const fd = open(path, ...args); if (path === root + "/packages/astra/marker.txt") fds.add(fd); return fd; };
fs.readFileSync = (path, ...args) => { if (path === root + "/packages/astra/marker.txt" || fds.has(path)) { fs.writeFileSync(root + "/packages/astra/marker.txt", "B\\n"); const value = read(path, ...args); fs.writeFileSync(root + "/packages/astra/marker.txt", "A\\n"); return value; } return read(path, ...args); };
fs.closeSync = (fd) => { fds.delete(fd); return close(fd); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["status", "--porcelain"]), "");
		assert.equal(f.archived("marker.txt").toString(), "B\n");
		assert.equal(f.source().includesUncommittedChanges, true);
		assert.equal(f.source().files.find((file) => file.path === "marker.txt").matchesBaseCommit, false);
	} finally { f.close(); }
});

test("dirty tracked bytes, executable mode and selected untracked source keep their provenance", () => {
	const f = fixture();
	try {
		f.put("packages/astra/marker.txt", "draft tracked content\n");
		chmodSync(join(f.root, "packages/astra/marker.txt"), 0o755);
		f.put("packages/astra/source.js", "// uncommitted source\n");
		const result = f.run();
		assert.equal(result.status, 0, result.stderr);
		const metadata = f.source();
		assert.equal(metadata.includesUncommittedChanges, true);
		const marker = metadata.files.find((file) => file.path === "marker.txt");
		assert.equal(marker.source, "tracked");
		assert.equal(marker.sourceMode, 0o755);
		assert.equal(marker.baseMode, "100644");
		assert.equal(marker.matchesBaseCommit, false);
		assert.equal(metadata.files.find((file) => file.path === "source.js").source, "untracked");
		assert.equal(f.archived("marker.txt").toString(), "draft tracked content\n");
	} finally { f.close(); }
});

test("version changed without a commit during selection keeps captured manifest bytes or refuses safely", () => {
	const f = fixture();
	try {
		const original = readFileSync(join(f.root, "packages/astra/package.json"));
		const result = f.run(`let changed = false; child.execFileSync = (file, args, options) => {
 const value = run(file, args, options);
 if (!changed && (file === "npm" || file === "npm.cmd")) { changed = true; const path = root + "/packages/astra/package.json"; const pkg = JSON.parse(fs.readFileSync(path)); pkg.version = "1.0.1"; fs.writeFileSync(path, JSON.stringify(pkg)); }
 return value;
};`);
		if (result.status !== 0) {
			assert.match(result.stderr, /file selection manifest changed/i);
			assert.equal(existsSync(f.output), false);
		} else {
			assert.deepEqual(f.archived("package.json"), original);
			assert.equal(f.source().version, "1.0.0");
			assert.equal(f.archived("marker.txt").toString(), "A\n");
			assert.equal(f.source().includesUncommittedChanges, true);
		}
		assert.equal(existsSync(join(f.root, ".artifacts/astra-v1.0.1/package")), false);
	} finally { f.close(); }
});

test("manifest files rules changing after selection cannot silently drop a captured file", () => {
	const f = fixture();
	try {
		const original = readFileSync(join(f.root, "packages/astra/package.json"));
		const result = f.run(`child.execFileSync = (file, args, options) => {
 const value = run(file, args, options);
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) { const path = root + "/packages/astra/package.json"; const pkg = JSON.parse(fs.readFileSync(path)); pkg.files = ["dist", "web"]; fs.writeFileSync(path, JSON.stringify(pkg)); }
 return value;
};`);
		if (result.status !== 0) {
			assert.match(result.stderr, /file (?:list|selection).*changed/i);
			assert.equal(existsSync(f.output), false);
			if (existsSync(dirname(f.output))) assert.deepEqual(readdirSync(dirname(f.output)), []);
		} else {
			assert.deepEqual(f.archived("package.json"), original);
			assert.equal(f.archived("marker.txt").toString(), "A\n");
			assert.equal(f.source().includesUncommittedChanges, true);
		}
	} finally { f.close(); }
});

test("same-version files rules cannot add an existing file between selection and capture and silently omit it", () => {
	const f = fixture();
	try {
		f.put("packages/astra/extra.txt", "newly selected existing file\n");
		const original = readFileSync(join(f.root, "packages/astra/package.json"));
		const result = f.run(`child.execFileSync = (file, args, options) => {
 const value = run(file, args, options);
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) { const path = root + "/packages/astra/package.json"; const pkg = JSON.parse(fs.readFileSync(path)); pkg.files.push("extra.txt"); fs.writeFileSync(path, JSON.stringify(pkg)); }
 return value;
};`);
		if (result.status !== 0) {
			assert.match(result.stderr, /file selection manifest changed/i);
			assert.equal(existsSync(f.output), false);
		} else {
			assert.deepEqual(f.archived("package.json"), original);
			assert.equal(f.archived("marker.txt").toString(), "A\n");
			assert.equal(f.source().files.some((file) => file.path === "extra.txt"), false);
			assert.equal(f.source().includesUncommittedChanges, true);
		}
	} finally { f.close(); }
});

test("manifest A to B to A during real npm selection cannot silently omit an A-selected file", () => {
	const f = fixture();
	try {
		const original = readFileSync(join(f.root, "packages/astra/package.json"));
		const result = f.run(`child.execFileSync = (file, args, options) => {
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) {
  const path = root + "/packages/astra/package.json"; const original = fs.readFileSync(path); const pkg = JSON.parse(original); pkg.files = ["dist", "web"];
  fs.writeFileSync(path, JSON.stringify(pkg)); try { return run(file, args, options); } finally { fs.writeFileSync(path, original); }
 }
 return run(file, args, options);
};`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["status", "--porcelain"]), "");
		assert.deepEqual(f.archived("package.json"), original);
		assert.equal(f.archived("marker.txt").toString(), "A\n");
		assert.equal(f.source().files.some((file) => file.path === "marker.txt"), true);
		assert.equal(f.source().includesUncommittedChanges, false);
	} finally { f.close(); }
});

test("default npm selection keeps main browser bin README and LICENSE despite ordinary ignore rules", () => {
	const f = fixture();
	try {
		const pkg = JSON.parse(readFileSync(join(f.root, "packages/astra/package.json"))); delete pkg.files; pkg.main = "lib/main.js"; pkg.browser = "lib/browser.js";
		f.put("packages/astra/package.json", JSON.stringify(pkg));
		for (const path of ["lib/main.js", "lib/browser.js", "README.md", "LICENSE", "excluded.txt"]) f.put(`packages/astra/${path}`, `actual ${path}\n`);
		f.put("packages/astra/.npmignore", "lib/\nREADME.md\nLICENSE\nexcluded.txt\n");
		const result = f.run();
		assert.equal(result.status, 0, result.stderr);
		for (const path of ["lib/main.js", "lib/browser.js", "README.md", "LICENSE", "dist/launcher.js"]) assert.ok(f.archived(path).length > 0, path);
		assert.equal(f.source().files.some((file) => file.path === "excluded.txt"), false);
		assert.equal(f.source().selectionControls.find((file) => file.path === "packages/astra/.npmignore").present, true);
	} finally { f.close(); }
});

test("directories bin normalization sees complete candidate paths without expanding files rules itself", () => {
	const f = fixture();
	try {
		const pkg = JSON.parse(readFileSync(join(f.root, "packages/astra/package.json"))); delete pkg.bin; pkg.directories = { bin: "tools" }; pkg.files = ["dist", "web"];
		f.put("packages/astra/package.json", JSON.stringify(pkg));
		f.put("packages/astra/tools/say.js", "#!/usr/bin/env node\nconsole.log('offline');\n");
		const result = f.run();
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.archived("tools/say.js").toString(), "#!/usr/bin/env node\nconsole.log('offline');\n");
		assert.equal(JSON.parse(f.archived("package.json")).directories.bin, "tools");
		assert.ok(f.source().selectionControls.find((file) => file.path === "packages/astra/package.json"));
	} finally { f.close(); }
});

test("gitignore fallback A to B to A during selection uses captured A rules", () => {
	const f = fixture();
	try {
		const pkg = JSON.parse(readFileSync(join(f.root, "packages/astra/package.json"))); delete pkg.files;
		f.put("packages/astra/package.json", JSON.stringify(pkg));
		f.put("packages/astra/.gitignore", "excluded.txt\n");
		f.put("packages/astra/excluded.txt", "ignored by captured rule\n");
		f.git(["add", "packages/astra/package.json", "packages/astra/.gitignore"]); f.git(["commit", "-qm", "fallback rules"]);
		const result = f.run(`child.execFileSync = (file, args, options) => {
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) { const path = root + "/packages/astra/.gitignore"; const original = fs.readFileSync(path); fs.writeFileSync(path, "marker.txt\\n"); try { return run(file, args, options); } finally { fs.writeFileSync(path, original); } }
 return run(file, args, options);
};`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.archived("marker.txt").toString(), "A\n");
		assert.equal(f.source().files.some((file) => file.path === "excluded.txt"), false);
		assert.equal(f.source().selectionControls.find((file) => file.path === "packages/astra/.npmignore").present, false);
		assert.equal(f.source().selectionControls.find((file) => file.path === "packages/astra/.gitignore").matchesBaseCommit, true);
		assert.equal(f.source().includesUncommittedChanges, false);
	} finally { f.close(); }
});

test("nested ignore A to B to A during selection retains the A-selected asset", () => {
	const f = fixture();
	try {
		f.put("packages/astra/web/asset.txt", "nested asset A\n");
		f.put("packages/astra/web/.npmignore", "excluded.txt\n");
		f.git(["add", "packages/astra/web"]); f.git(["commit", "-qm", "nested rules"]);
		const result = f.run(`child.execFileSync = (file, args, options) => {
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) { const path = root + "/packages/astra/web/.npmignore"; const original = fs.readFileSync(path); fs.writeFileSync(path, "asset.txt\\n"); try { return run(file, args, options); } finally { fs.writeFileSync(path, original); } }
 return run(file, args, options);
};`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.archived("web/asset.txt").toString(), "nested asset A\n");
		assert.equal(f.source().includesUncommittedChanges, false);
		assert.equal(f.source().selectionControls.find((file) => file.path === "packages/astra/web/.npmignore").matchesBaseCommit, true);
	} finally { f.close(); }
});

test("captured transient unarchived ignore bytes bind selection and remain dirty after live restoration", () => {
	const f = fixture();
	try {
		f.put("packages/astra/web/asset.txt", "nested asset\n"); f.put("packages/astra/web/excluded.txt", "selected by captured B\n");
		f.put("packages/astra/web/.npmignore", "excluded.txt\n");
		f.git(["add", "packages/astra/web"]); f.git(["commit", "-qm", "captured rule A"]);
		const result = f.run(`const read = fs.readFileSync; const open = fs.openSync; const close = fs.closeSync; const fds = new Set();
fs.openSync = (path, ...args) => { const fd = open(path, ...args); if (path === root + "/packages/astra/web/.npmignore") fds.add(fd); return fd; };
fs.readFileSync = (path, ...args) => { if (path === root + "/packages/astra/web/.npmignore" || fds.has(path)) { const source = root + "/packages/astra/web/.npmignore"; const original = read(source); fs.writeFileSync(source, "asset.txt\\n"); try { return read(path, ...args); } finally { fs.writeFileSync(source, original); } } return read(path, ...args); };
fs.closeSync = (fd) => { fds.delete(fd); return close(fd); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["status", "--porcelain"]), "");
		assert.equal(f.source().includesUncommittedChanges, true);
		assert.equal(f.source().files.some((file) => file.path === "web/asset.txt"), false);
		assert.equal(f.archived("web/excluded.txt").toString(), "selected by captured B\n");
		const control = f.source().selectionControls.find((file) => file.path === "packages/astra/web/.npmignore");
		assert.equal(control.sha256, createHash("sha256").update("asset.txt\n").digest("hex"));
		assert.equal(control.matchesBaseCommit, false);
		assert.equal(f.source().files.some((file) => file.path === "web/.npmignore"), false);
	} finally { f.close(); }
});

test("missing captured npmignore preserves gitignore fallback and records missing committed control as dirty", () => {
	const f = fixture();
	try {
		f.put("packages/astra/web/asset.txt", "selected by fallback\n"); f.put("packages/astra/web/excluded.txt", "excluded by fallback\n");
		f.put("packages/astra/web/.npmignore", "asset.txt\n"); f.put("packages/astra/web/.gitignore", "excluded.txt\n");
		f.git(["add", "packages/astra/web"]); f.git(["commit", "-qm", "fallback control A"]);
		const result = f.run(`const stat = fs.lstatSync; let absent = false;
fs.lstatSync = (path, ...args) => { if (!absent && path === root + "/packages/astra/web/.npmignore") { absent = true; const original = fs.readFileSync(path); fs.unlinkSync(path); try { return stat(path, ...args); } finally { fs.writeFileSync(path, original); } } return stat(path, ...args); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["status", "--porcelain"]), "");
		assert.equal(f.archived("web/asset.txt").toString(), "selected by fallback\n");
		assert.equal(f.source().files.some((file) => file.path === "web/excluded.txt"), false);
		const missing = f.source().selectionControls.find((file) => file.path === "packages/astra/web/.npmignore");
		assert.equal(missing.present, false); assert.equal(missing.sha256, null); assert.equal(missing.matchesBaseCommit, false);
		assert.equal(f.source().includesUncommittedChanges, true);
	} finally { f.close(); }
});

test("selected manifest control uses its original Buffer for archive and both provenance records", () => {
	const f = fixture();
	try {
		const original = readFileSync(join(f.root, "packages/astra/package.json"));
		const result = f.run(`child.execFileSync = (file, args, options) => { const value = run(file, args, options); if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) { const path = root + "/packages/astra/package.json"; const pkg = JSON.parse(fs.readFileSync(path)); pkg.version = "1.0.1"; pkg.files = ["dist", "web"]; fs.writeFileSync(path, JSON.stringify(pkg)); } return value; };`);
		assert.equal(result.status, 0, result.stderr);
		assert.deepEqual(f.archived("package.json"), original);
		assert.equal(f.archived("marker.txt").toString(), "A\n");
		const hash = createHash("sha256").update(original).digest("hex");
		assert.equal(f.source().selectionControls.find((file) => file.path === "packages/astra/package.json").sha256, hash);
		assert.equal(f.source().files.find((file) => file.path === "package.json").sha256, hash);
		assert.equal(f.source().includesUncommittedChanges, true);
	} finally { f.close(); }
});

test("unarchived rule executable mode alone remains dirty when Git status ignores modes", () => {
	const f = fixture();
	try {
		f.put("packages/astra/web/.npmignore", "excluded.txt\n"); f.git(["add", "packages/astra/web/.npmignore"]); f.git(["commit", "-qm", "rule mode A"]);
		f.git(["config", "core.fileMode", "false"]); chmodSync(join(f.root, "packages/astra/web/.npmignore"), 0o755);
		assert.equal(f.git(["status", "--porcelain"]), "");
		const result = f.run(); assert.equal(result.status, 0, result.stderr);
		const control = f.source().selectionControls.find((file) => file.path === "packages/astra/web/.npmignore");
		assert.equal(control.sourceMode, 0o755); assert.equal(control.baseMode, "100644"); assert.equal(control.matchesBaseCommit, false);
		assert.equal(f.source().includesUncommittedChanges, true);
	} finally { f.close(); }
});

test("real workspace selection preserves ancestor ignore prefix and copies only the target package", () => {
	const f = fixture();
	try {
		const pkg = JSON.parse(readFileSync(join(f.root, "packages/astra/package.json"))); delete pkg.files; f.put("packages/astra/package.json", JSON.stringify(pkg));
		f.put("package.json", JSON.stringify({ name: "fixture-workspace-root", private: true, workspaces: ["packages/*"] }));
		f.put(".npmignore", "root-excluded.txt\n"); f.put("packages/.npmignore", "web/ancestor-excluded.txt\n");
		f.put(".npmrc", "fund=false\n"); f.put("packages/.npmrc", "audit=false\n"); f.put("packages/astra/.npmrc", "progress=false\n");
		f.put("packages/astra/root-excluded.txt", "ancestor root excludes\n"); f.put("packages/astra/web/ancestor-excluded.txt", "ancestor packages excludes\n");
		f.put("packages/other/package.json", JSON.stringify({ name: "unrelated-workspace", version: "1.0.0" })); f.put("packages/other/private.txt", "must not copy\n");
		f.git(["add", "package.json", ".npmignore", ".npmrc", "packages"]); f.git(["commit", "-qm", "workspace selection"]);
		const [expected] = JSON.parse(execFileSync(process.platform === "win32" ? "npm.cmd" : "npm", ["pack", "--dry-run", "--ignore-scripts", "--json"], { cwd: join(f.root, "packages/astra"), encoding: "utf8", env: { ...process.env, npm_config_offline: "true" } }));
		const result = f.run(`child.execFileSync = (file, args, options) => {
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) {
  if (options.cwd === root + "/packages/astra") throw new Error("workspace selection used live rules");
  const prefix = options.cwd.slice(0, -"/packages/astra".length);
  if (!fs.existsSync(prefix + "/package.json") || fs.existsSync(prefix + "/packages/other")) throw new Error("wrong workspace skeleton");
  for (const path of [".npmrc", "packages/.npmrc", "packages/astra/.npmrc"]) if (!fs.readFileSync(prefix + "/" + path).equals(fs.readFileSync(root + "/" + path))) throw new Error("missing captured project config");
 }
 return run(file, args, options);
};`);
		assert.equal(result.status, 0, result.stderr);
		assert.deepEqual(f.source().files.map((file) => file.path).sort(), expected.files.map((file) => file.path).sort());
		assert.equal(f.source().files.some((file) => file.path === "root-excluded.txt" || file.path === "web/ancestor-excluded.txt"), false);
		for (const path of ["package.json", ".npmignore", "packages/.npmignore", ".npmrc", "packages/.npmrc", "packages/astra/.npmrc"]) {
			const control = f.source().selectionControls.find((file) => file.path === path);
			assert.equal(control.matchesBaseCommit, true, path);
			assert.equal(control.sha256, createHash("sha256").update(readFileSync(join(f.root, path))).digest("hex"), path);
			assert.equal("bytes" in control || "content" in control, false);
		}
		assert.doesNotMatch(execFileSync("tar", ["-tzf", f.tarball()], { encoding: "utf8" }), /\.npmrc/);
		assert.equal(f.source().includesUncommittedChanges, false);
		assert.deepEqual(readdirSync(dirname(f.output)), ["package"]);
	} finally { f.close(); }
});

test("committed ignore in a directory missing during candidate enumeration is recorded missing and dirty", () => {
	const f = fixture();
	try {
		const pkg = JSON.parse(readFileSync(join(f.root, "packages/astra/package.json"))); pkg.files.push("assets"); f.put("packages/astra/package.json", JSON.stringify(pkg));
		f.put("packages/astra/assets/.npmignore", "excluded.txt\n"); f.put("packages/astra/assets/asset.txt", "existing asset\n");
		f.git(["add", "packages/astra"]); f.git(["commit", "-qm", "directory control A"]);
		const result = f.run(`const read = fs.readdirSync; let removed = false;
fs.readdirSync = (path, ...args) => { if (!removed && path === root + "/packages/astra") { removed = true; fs.renameSync(root + "/packages/astra/assets", root + "/temporary-assets"); try { return read(path, ...args); } finally { fs.renameSync(root + "/temporary-assets", root + "/packages/astra/assets"); } } return read(path, ...args); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["status", "--porcelain"]), "");
		const control = f.source().selectionControls.find((file) => file.path === "packages/astra/assets/.npmignore");
		assert.equal(control.present, false); assert.equal(control.matchesBaseCommit, false); assert.ok(control.baseBlob);
		assert.equal(f.source().includesUncommittedChanges, true);
	} finally { f.close(); }
});

test("tracked candidate A to absent to A remains dirty with the captured missing path after clean HEAD and status", () => {
	const f = fixture();
	try {
		const result = f.run(`const read = fs.readdirSync; let enumerated = false;
const absent = (work) => { fs.renameSync(root + "/packages/astra/marker.txt", root + "/temporary-marker.txt"); try { return work(); } finally { fs.renameSync(root + "/temporary-marker.txt", root + "/packages/astra/marker.txt"); } };
fs.readdirSync = (path, ...args) => { if (!enumerated && path === root + "/packages/astra") { enumerated = true; return absent(() => read(path, ...args)); } return read(path, ...args); };
child.execFileSync = (file, args, options) => { if (!enumerated && (file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) return absent(() => run(file, args, options)); return run(file, args, options); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.git(["rev-parse", "HEAD"]), f.baseCommit);
		assert.equal(f.git(["status", "--porcelain"]), "");
		assert.equal(readFileSync(join(f.root, "packages/astra/marker.txt"), "utf8"), "A\n");
		assert.equal(JSON.parse(f.archived("package.json")).files.includes("marker.txt"), true);
		assert.doesNotMatch(execFileSync("tar", ["-tzf", f.tarball()], { encoding: "utf8" }), /package\/marker\.txt/);
		assert.equal(f.source().includesUncommittedChanges, true);
		assert.deepEqual(f.source().missingCandidatePaths, ["packages/astra/marker.txt"]);
	} finally { f.close(); }
});

test("intentional deletion of an unselected tracked candidate still permits a dirty draft with explicit missing provenance", () => {
	const f = fixture();
	try {
		f.put("packages/astra/unused/source.txt", "unselected committed source\n");
		f.git(["add", "packages/astra/unused/source.txt"]); f.git(["commit", "-qm", "unselected candidate"]);
		rmSync(join(f.root, "packages/astra/unused/source.txt"));
		const result = f.run(`const open = fs.openSync; fs.openSync = (path, ...args) => { if (path === root + "/packages/astra/unused/source.txt") throw new Error("unselected missing payload opened"); return open(path, ...args); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.archived("marker.txt").toString(), "A\n");
		assert.equal(f.source().includesUncommittedChanges, true);
		assert.deepEqual(f.source().missingCandidatePaths, ["packages/astra/unused/source.txt"]);
		assert.equal(f.source().files.some((file) => file.path === "unused/source.txt"), false);
		assert.equal(existsSync(join(f.root, "packages/astra/unused/source.txt")), false);
	} finally { f.close(); }
});

test("selection placeholders do not read or copy unselected large payloads or node_modules", () => {
	const f = fixture();
	try {
		f.put("packages/astra/unused/large.bin", ""); truncateSync(join(f.root, "packages/astra/unused/large.bin"), 16 * 1024 * 1024);
		f.put("packages/astra/node_modules/unselected/payload.bin", "must never read\n");
		const result = f.run(`const open = fs.openSync; const read = fs.readFileSync;
fs.openSync = (path, ...args) => { if (typeof path === "string" && path.startsWith(root + "/packages/astra/") && (path.includes("/unused/") || path.includes("/node_modules/"))) throw new Error("unselected payload opened"); return open(path, ...args); };
fs.readFileSync = (path, ...args) => { if (typeof path === "string" && path.startsWith(root + "/packages/astra/") && (path.includes("/unused/") || path.includes("/node_modules/"))) throw new Error("unselected payload read"); return read(path, ...args); };
child.execFileSync = (file, args, options) => { if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) { if (options.cwd === root + "/packages/astra") throw new Error("selection is live"); if (fs.statSync(options.cwd + "/unused/large.bin").size !== 0 || fs.existsSync(options.cwd + "/node_modules")) throw new Error("unselected payload copied"); } return run(file, args, options); };`);
		assert.equal(result.status, 0, result.stderr);
		assert.equal(f.source().files.some((file) => file.path.startsWith("unused/") || file.path.startsWith("node_modules/")), false);
		assert.deepEqual(readdirSync(dirname(f.output)), ["package"]);
	} finally { f.close(); }
});

test("tracked mode alone is dirty even when Git status ignores executable bit changes", () => {
	const f = fixture();
	try {
		f.git(["config", "core.fileMode", "false"]);
		chmodSync(join(f.root, "packages/astra/marker.txt"), 0o755);
		assert.equal(f.git(["status", "--porcelain"]), "");
		const result = f.run();
		assert.equal(result.status, 0, result.stderr);
		const marker = f.source().files.find((file) => file.path === "marker.txt");
		assert.equal(marker.sourceMode, 0o755);
		assert.equal(marker.baseMode, "100644");
		assert.equal(marker.matchesBaseCommit, false);
		assert.equal(f.source().includesUncommittedChanges, true);
		assert.equal(f.archived("marker.txt").toString(), "A\n");
	} finally { f.close(); }
});

for (const kind of ["file", "directory", "committed-file"]) {
	test(`unselected ${kind} link preserves real npm archive without reading its outside target`, () => {
		const f = fixture();
		try {
			const target = join(f.hooks, "outside-target");
			if (kind === "directory") { mkdirSync(target); writeFileSync(join(target, "outside.txt"), "outside target bytes\n"); }
			else writeFileSync(target, "outside target bytes\n");
			const relative = kind === "committed-file" ? "marker.txt" : "unused/local-link";
			const link = join(f.root, "packages/astra", relative);
			mkdirSync(dirname(link), { recursive: true });
			if (kind === "committed-file") rmSync(link);
			symlinkSync(target, link, kind === "directory" ? "dir" : "file");
			const options = { cwd: join(f.root, "packages/astra"), encoding: "utf8", env: { ...process.env, npm_config_offline: "true", npm_config_cache: join(f.hooks, "npm-cache") } };
			const npm = process.platform === "win32" ? "npm.cmd" : "npm";
			const [selected] = JSON.parse(execFileSync(npm, ["pack", "--dry-run", "--ignore-scripts", "--json"], options));
			const [packed] = JSON.parse(execFileSync(npm, ["pack", "--ignore-scripts", "--json", "--pack-destination", f.hooks], options));
			const expected = selected.files.map((file) => file.path).sort();
			assert.equal(expected.includes(relative), false);
			assert.deepEqual(packed.files.map((file) => file.path).sort(), expected);
			const reference = join(f.hooks, packed.filename);
			const result = f.run(`const blocked = [${JSON.stringify(link)}, ${JSON.stringify(target)}];
for (const name of ["openSync", "readFileSync", "readdirSync"]) { const original = fs[name]; fs[name] = (path, ...args) => { if (typeof path === "string" && blocked.some(value => path === value || path.startsWith(value + "/"))) throw new Error("unselected link or outside target read"); return original(path, ...args); }; }
child.execFileSync = (file, args, options) => { if ((file === "npm" || file === "npm.cmd") && fs.existsSync(options.cwd + "/" + ${JSON.stringify(relative)})) throw new Error("unselected link presented to npm view"); return run(file, args, options); };`);
			assert.equal(result.status, 0, result.stderr);
			assert.deepEqual(f.source().files.map((file) => file.path).sort(), expected);
			assert.deepEqual(execFileSync("tar", ["-tzf", f.tarball()], { encoding: "utf8" }).trim().split("\n").sort(), expected.map((path) => `package/${path}`).sort());
			assert.doesNotMatch(execFileSync("tar", ["-tvzf", f.tarball()], { encoding: "utf8" }), /^l/m);
			for (const path of expected) assert.deepEqual(f.archived(path), execFileSync("tar", ["-xOf", reference, `package/${path}`]), path);
			assert.equal(f.source().includesUncommittedChanges, true);
			assert.deepEqual(f.source().missingCandidatePaths, kind === "committed-file" ? ["packages/astra/marker.txt"] : []);
			assert.equal(f.git(["rev-parse", "HEAD"]), f.baseCommit);
			assert.deepEqual(readdirSync(dirname(f.output)), ["package"]);
		} finally { f.close(); }
	});
}

for (const kind of ["parent-symlink", "file-symlink", "directory", "traversal"]) {
	test(`selected ${kind} file is rejected before publication`, () => {
		const f = fixture();
		try {
			const result = f.run(`child.execFileSync = (file, args, options) => {
 const value = run(file, args, options);
 if ((file === "npm" || file === "npm.cmd") && args.includes("--dry-run")) {
  const kind = ${JSON.stringify(kind)};
  if (kind === "traversal") { const entries = JSON.parse(value); entries[0].files.push({path:"../unrelated.txt", size:20, mode:420}); return JSON.stringify(entries); }
  if (kind === "parent-symlink") { fs.renameSync(root + "/packages/astra/web", root + "/outside-web"); fs.symlinkSync(root + "/outside-web", root + "/packages/astra/web", "dir"); }
  else { fs.unlinkSync(root + "/packages/astra/marker.txt"); if (kind === "file-symlink") fs.symlinkSync(root + "/unrelated.txt", root + "/packages/astra/marker.txt"); else fs.mkdirSync(root + "/packages/astra/marker.txt"); }
 }
 return value;
};`);
			assert.notEqual(result.status, 0);
			assert.match(result.stderr, /Non-regular|Invalid package path/);
			assert.equal(existsSync(f.output), false);
		} finally { f.close(); }
	});
}
