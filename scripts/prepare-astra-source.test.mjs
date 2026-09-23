import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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
