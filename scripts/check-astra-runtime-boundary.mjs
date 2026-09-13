#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));

function fail(message) {
	console.error(`Astra runtime boundary check failed: ${message}`);
	process.exit(1);
}

function rustSources(dir) {
	return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) return rustSources(path);
		return entry.isFile() && entry.name.endsWith(".rs") ? [relative(root, path)] : [];
	});
}

const manifestPath = join(root, "packages", "astra", "legacy-rust-files.txt");
const legacyRootRelative = join("legacy", "rust");
const legacyRoot = join(root, legacyRootRelative);
const requiredLegacyPaths = [
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
const missingLegacyPaths = requiredLegacyPaths.filter((path) => !existsSync(join(legacyRoot, path)));
if (missingLegacyPaths.length > 0) {
	fail(`legacy Rust archive is incomplete: ${missingLegacyPaths.join(", ")}`);
}

const forbiddenRootPaths = [
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

const expectedRustSources = readFileSync(manifestPath, "utf8")
	.split("\n")
	.filter(Boolean)
	.map((path) => join(legacyRootRelative, path))
	.sort();
const actualRustSources = rustSources(join(legacyRoot, "src")).sort();
const unexpected = actualRustSources.filter((path) => !expectedRustSources.includes(path));
const missing = expectedRustSources.filter((path) => !actualRustSources.includes(path));
if (unexpected.length > 0 || missing.length > 0) {
	fail(
		[
			unexpected.length > 0 ? `unreviewed Rust source files: ${unexpected.join(", ")}` : "",
			missing.length > 0 ? `manifest still lists removed files: ${missing.join(", ")}` : "",
		]
			.filter(Boolean)
			.join("; "),
	);
}

const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const defaultProductScripts = Object.entries(packageJson.scripts ?? {}).filter(
	([name]) => name === "build" || name === "build:offline" || name === "prepublishOnly" || name === "publish" || name === "publish:dry" || name === "release:local" || name.startsWith("release:"),
);
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

console.log(`Astra runtime boundary verified (${actualRustSources.length} archived Rust source files).`);
