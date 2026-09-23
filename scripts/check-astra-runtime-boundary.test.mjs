import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

test("current runtime works without the Rust archive and rejects restored product paths and scripts", () => {
	const root = mkdtempSync(join(tmpdir(), "astra-boundary-"));
	try {
		mkdirSync(join(root, "scripts"));
		mkdirSync(join(root, "packages/astra/src"), { recursive: true });
		copyFileSync(new URL("./check-astra-runtime-boundary.mjs", import.meta.url), join(root, "scripts/check.mjs"));
		writeFileSync(join(root, "packages/astra/src/launcher.ts"), 'import { main } from "@earendil-works/pi-coding-agent";\n');
		const scripts = { astra: "node packages/astra/dist/launcher.js", "astra:research": "node packages/astra/dist/launcher.js research", build: "echo build" };
		const save = () => writeFileSync(join(root, "package.json"), JSON.stringify({ scripts }));
		const run = () => spawnSync(process.execPath, [join(root, "scripts/check.mjs")], { encoding: "utf8" });
		save();
		assert.equal(run().status, 0, "a current checkout must not require historical Rust sources");
		mkdirSync(join(root, "legacy/rust"), { recursive: true });
		assert.notEqual(run().status, 0, "archived runtime must not return to the main tree");
		rmSync(join(root, "legacy"), { recursive: true });
		writeFileSync(join(root, "Cargo.toml"), "[package]\n");
		assert.notEqual(run().status, 0);
		rmSync(join(root, "Cargo.toml"));
		scripts.build = "cargo build";
		save();
		assert.notEqual(run().status, 0);
		scripts.build = "echo build";
		scripts["astra:package"] = "bash legacy/rust/scripts/package_cli_release.sh";
		save();
		assert.notEqual(run().status, 0);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
