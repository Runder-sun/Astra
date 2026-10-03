import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const packageRoot = join(root, "packages/astra");
const { version } = JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8"));
for (const file of ["dist/launcher.js", "dist/workbench.js", "dist/workbench-runner.js", "web/index.html"]) {
	if (!existsSync(join(packageRoot, file))) throw new Error(`Missing ${file}; run npm run build:offline first`);
}
const output = join(root, `.artifacts/astra-v${version}/package`);
if (existsSync(output)) throw new Error(`Output already exists: ${output}; use a new release version or a clean worktree`);
mkdirSync(dirname(output), { recursive: true });
const staging = mkdtempSync(join(dirname(output), ".package.stage-"));
let filename;
try {
	const [packed] = JSON.parse(execFileSync(
	process.platform === "win32" ? "npm.cmd" : "npm",
	["pack", "--ignore-scripts", "--json", "--pack-destination", staging],
	{ cwd: packageRoot, encoding: "utf8" },
));
	const checksum = createHash("sha256").update(readFileSync(join(staging, packed.filename))).digest("hex");
	writeFileSync(join(staging, "SHA256SUMS"), `${checksum}  ${packed.filename}\n`);
	writeFileSync(join(staging, "PACKAGE_SOURCE.json"), `${JSON.stringify({
	version,
	baseCommit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root }).toString().trim(),
	includesUncommittedChanges: execFileSync("git", ["status", "--porcelain"], { cwd: root }).toString().trim().length > 0,
	files: packed.files,
}, null, 2)}\n`);
	if (existsSync(output)) throw new Error(`Output already exists: ${output}`);
	renameSync(staging, output);
	filename = packed.filename;
} finally {
	rmSync(staging, { recursive: true, force: true });
}
console.log(`Prepared local package: ${join(output, filename)}\nNothing published or pushed.`);
