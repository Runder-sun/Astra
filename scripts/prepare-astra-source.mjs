import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = realpathSync(resolve(fileURLToPath(new URL("..", import.meta.url))));
function physicalSource(path, directory = false) {
	const parts = path.split("/");
	let current = root;
	for (const [index, part] of parts.entries()) {
		current = join(current, part);
		const metadata = lstatSync(current);
		if (metadata.isSymbolicLink() || (index < parts.length - 1 || directory ? !metadata.isDirectory() : !metadata.isFile()))
			throw new Error(`Non-regular file or directory requires review: ${path}`);
		if (index === parts.length - 1) return metadata;
	}
}
const baseCommit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root }).toString().trim();
const packagePath = join(root, "packages/astra/package.json");
physicalSource("packages/astra/package.json");
const packageBytes = readFileSync(packagePath);
const { version } = JSON.parse(packageBytes.toString("utf8"));
const target = resolve(process.argv[2] ?? join(root, `.artifacts/astra-v${version}/source`));
const stagingParent = dirname(target);
const rootFiles = new Set([
	".gitattributes", ".gitignore", ".npmrc", "AGENTS.md", "CONTRIBUTING.md", "LICENSE", "README.md", "README.en.md",
	"RELEASE_NOTES.md", "RELEASE_VALIDATION.md", "SECURITY.md", "biome.json", "package-lock.json", "package.json",
	"pi-test.bat", "pi-test.ps1", "pi-test.sh", "test.sh", "tsconfig.base.json", "tsconfig.json", "vitest.base.ts",
]);
const selected = (path) => rootFiles.has(path) || /^(?:packages|scripts|docs|\.github)\//.test(path);
function physicalTarget(path) {
	let current = path;
	const suffix = [];
	for (;;) {
		try { return resolve(realpathSync(current), ...suffix); }
		catch (error) {
			if (error.code !== "ENOENT") throw error;
			suffix.unshift(basename(current));
			current = dirname(current);
		}
	}
}
function checkDestination() {
	const physical = physicalTarget(target);
	const local = relative(root, physical).split(sep).join("/");
	if (physical === root || root.startsWith(`${physical}${sep}`) ||
		/^(?:packages|scripts|docs|\.github)(?:\/|$)/.test(local) || rootFiles.has(local))
		throw new Error("Export destination overlaps selected source paths");
	if (existsSync(target)) throw new Error("Export destination must not exist");
	return physical;
}
const destination = checkDestination();
const objectFormat = execFileSync("git", ["rev-parse", "--show-object-format"], { cwd: root }).toString().trim();
const commitFiles = new Map(execFileSync("git", ["ls-tree", "-rz", baseCommit], { cwd: root }).toString()
	.split("\0").filter(Boolean).map((entry) => {
		const tab = entry.indexOf("\t");
		const [mode, type, hash] = entry.slice(0, tab).split(" ");
		return [entry.slice(tab + 1), { mode, type, hash }];
	}));
const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: root }).toString().split("\0").filter(Boolean);
const added = execFileSync("git", ["ls-files", "--others", "--exclude-standard", "-z", "packages/astra", "scripts", "docs", ".github", ...rootFiles], { cwd: root }).toString().split("\0").filter(Boolean);
// Offline compilation needs the validated catalog data normally excluded by Git.
for (const path of ["packages/ai/scripts/check-model-data.ts", "packages/ai/scripts/model-data.ts", "packages/ai/src/models.generated.ts"])
	physicalSource(path);
physicalSource("packages/ai/src/providers", true);
physicalSource("packages/ai/src/providers/data", true);
const modelData = readdirSync(join(root, "packages/ai/src/providers/data"))
	.filter((name) => name.endsWith(".json"))
	.map((name) => `packages/ai/src/providers/data/${name}`);
for (const path of modelData) physicalSource(path);
execFileSync(process.execPath, ["packages/ai/scripts/check-model-data.ts"], { cwd: root, stdio: "inherit" });
const paths = [...new Set([...tracked, ...added, ...modelData])]
	.filter((path) => selected(path) && existsSync(join(root, path)))
	.sort();
const forbidden = /(?:gh[pousr]_[A-Za-z0-9]{25,}|sk-[A-Za-z0-9_-]{32,}|-----BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY-----|\/mnt\/data\/[A-Za-z][A-Za-z0-9_-]*\/(?:Documents|\.codex|\.cargo))/;
const files = [];
const selectedPaths = new Set(paths);
let includesUncommittedChanges = [...commitFiles.keys()].some((path) => selected(path) && !selectedPaths.has(path));
mkdirSync(stagingParent, { recursive: true });
const staging = mkdtempSync(join(stagingParent, `.${basename(target)}.stage-`));
try {
	for (const path of paths) {
		const source = join(root, path);
		const metadata = physicalSource(path);
		const bytes = path === "packages/astra/package.json" ? packageBytes : readFileSync(source);
		if (forbidden.test(bytes.toString())) throw new Error(`Potential private data requires review: ${path}`);
		const mode = metadata.mode & 0o777;
		const committed = commitFiles.get(path);
		const blobHash = createHash(objectFormat).update(`blob ${bytes.length}\0`).update(bytes).digest("hex");
		if (!committed || committed.type !== "blob" || committed.hash !== blobHash ||
			committed.mode !== (mode & 0o111 ? "100755" : "100644")) includesUncommittedChanges = true;
		mkdirSync(dirname(join(staging, path)), { recursive: true });
		writeFileSync(join(staging, path), bytes, { mode });
		chmodSync(join(staging, path), mode);
		files.push({ path, sha256: createHash("sha256").update(bytes).digest("hex"), size: bytes.length });
	}
	writeFileSync(join(staging, "SOURCE_MANIFEST.json"), `${JSON.stringify({
		version,
		baseCommit,
		includesUncommittedChanges,
		secretCheck: "Bounded pattern scan; not a proof that all secrets are absent",
		files,
	}, null, 2)}\n`);
	if (execFileSync("git", ["rev-parse", "HEAD"], { cwd: root }).toString().trim() !== baseCommit)
		throw new Error("Source HEAD changed during export; retry against a stable commit");
	if (checkDestination() !== destination) throw new Error("Export destination changed during preparation");
	renameSync(staging, target);
} finally {
	rmSync(staging, { recursive: true, force: true });
}
console.log(`Prepared ${files.length} source files; excluded Git history, local sessions and legacy Rust`);
