import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const target = resolve(process.argv[2] ?? join(root, ".artifacts/astra-v0.1.0-alpha.1/source"));
if (existsSync(target)) throw new Error("Export destination must not exist");
const rootFiles = new Set([
	".gitattributes", ".gitignore", ".npmrc", "AGENTS.md", "CONTRIBUTING.md", "LICENSE", "README.md",
	"RELEASE_NOTES.md", "RELEASE_VALIDATION.md", "SECURITY.md", "biome.json", "package-lock.json", "package.json",
	"pi-test.bat", "pi-test.ps1", "pi-test.sh", "test.sh", "tsconfig.base.json", "tsconfig.json", "vitest.base.ts",
]);
const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: root }).toString().split("\0").filter(Boolean);
const added = execFileSync("git", ["ls-files", "--others", "--exclude-standard", "-z", "packages/astra", "scripts/prepare-astra-source.mjs", "scripts/smoke-astra-package.mjs", "RELEASE_NOTES.md", "RELEASE_VALIDATION.md"], { cwd: root }).toString().split("\0").filter(Boolean);
// Offline compilation needs the validated catalog data normally excluded by Git.
execFileSync(process.execPath, ["packages/ai/scripts/check-model-data.ts"], { cwd: root, stdio: "inherit" });
const modelData = readdirSync(join(root, "packages/ai/src/providers/data"))
	.filter((name) => name.endsWith(".json"))
	.map((name) => `packages/ai/src/providers/data/${name}`);
const paths = [...new Set([...tracked, ...added, ...modelData])].filter((path) => rootFiles.has(path) || /^(?:packages|scripts|docs|legacy)\//.test(path)).sort();
const forbidden = /(?:gh[pousr]_[A-Za-z0-9]{25,}|sk-[A-Za-z0-9_-]{32,}|-----BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY-----|\/mnt\/data\/[A-Za-z][A-Za-z0-9_-]*\/(?:Documents|\.codex|\.cargo))/;
const files = [];
for (const path of paths) {
	const source = join(root, path);
	if (!lstatSync(source).isFile()) throw new Error(`Non-regular file requires review: ${path}`);
	const bytes = readFileSync(source);
	if (forbidden.test(bytes.toString())) throw new Error(`Potential private data requires review: ${path}`);
	files.push({ path, sha256: createHash("sha256").update(bytes).digest("hex"), size: bytes.length });
}
for (const { path } of files) {
	mkdirSync(dirname(join(target, path)), { recursive: true });
	copyFileSync(join(root, path), join(target, path));
}
writeFileSync(join(target, "SOURCE_MANIFEST.json"), `${JSON.stringify({
	version: "0.1.0-alpha.1",
	baseCommit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root }).toString().trim(),
	includesUncommittedChanges: true,
	secretCheck: "Bounded pattern scan; not a proof that all secrets are absent",
	files,
}, null, 2)}\n`);
console.log(`Prepared ${files.length} source files; excluded Git history, local sessions and upstream workflows`);
