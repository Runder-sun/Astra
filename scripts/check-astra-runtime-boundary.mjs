#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));

function fail(message) {
	console.error(`Astra runtime boundary check failed: ${message}`);
	process.exit(1);
}

// Historical Rust code lives in the published alpha.1 tag, not the current tree.
const forbiddenRootPaths = [
	"legacy/rust",
	"packages/astra/legacy-rust-files.txt",
	"Cargo.toml",
	"Cargo.lock",
	"src",
	"tests",
	"schemas",
	"scripts/package_cli_release.sh",
	"scripts/package_remote_app.sh",
	"scripts/run_mock_parity_harness.sh",
	"scripts/run_parity_demo_suite.sh",
];
const remainingRootPaths = forbiddenRootPaths.filter((path) => existsSync(join(root, path)));
if (remainingRootPaths.length > 0) {
	fail(`Rust product paths remain at the repository root: ${remainingRootPaths.join(", ")}`);
}

const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const defaultProductScripts = Object.entries(packageJson.scripts ?? {});
const legacyProductScripts = defaultProductScripts
	.filter(([, command]) => typeof command === "string" && (command.includes("legacy/rust") || /(^|\s)cargo(\s|$)/.test(command)))
	.map(([name]) => name);
if (legacyProductScripts.length > 0) {
	fail(`default product scripts invoke the legacy Rust runtime: ${legacyProductScripts.join(", ")}`);
}
if (packageJson.scripts?.astra !== "node packages/astra/dist/launcher.js") {
	fail("npm script 'astra' must use the Pi-native Node launcher");
}
if (packageJson.scripts?.["astra:research"] !== "node packages/astra/dist/launcher.js research") {
	fail("npm script 'astra:research' must use the Pi-native Node launcher");
}

const launcher = readFileSync(join(root, "packages", "astra", "src", "launcher.ts"), "utf8");
if (!launcher.includes('import { main } from "@earendil-works/pi-coding-agent";')) {
	fail("Astra launcher must statically import Pi main()");
}
if (launcher.includes('import("@earendil-works/pi-coding-agent")')) {
	fail("Astra launcher must not hide Pi main() behind a dynamic import");
}

console.log("Astra runtime boundary verified (current Node entry points; Rust archive is external).");
