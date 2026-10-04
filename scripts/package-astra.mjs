import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, closeSync, existsSync, fstatSync, lstatSync, mkdirSync, mkdtempSync, openSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const baseCommit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root }).toString().trim();
const initialStatus = execFileSync("git", ["status", "--porcelain"], { cwd: root }).toString().trim();
const objectFormat = execFileSync("git", ["rev-parse", "--show-object-format"], { cwd: root }).toString().trim();
const sourceScopes = ["package.json", ".npmignore", ".gitignore", ".npmrc", "packages/package.json", "packages/.npmignore", "packages/.gitignore", "packages/.npmrc", "packages/astra"];
const commitFiles = new Map(execFileSync("git", ["ls-tree", "-rz", baseCommit, "--", ...sourceScopes], { cwd: root }).toString()
	.split("\0").filter(Boolean).map((entry) => {
		const tab = entry.indexOf("\t");
		const [mode, type, hash] = entry.slice(0, tab).split(" ");
		return [entry.slice(tab + 1), { mode, type, hash }];
	}));
const tracked = new Set(execFileSync("git", ["ls-files", "-z", "--", ...sourceScopes], { cwd: root }).toString().split("\0").filter(Boolean));
const ignored = new Set(execFileSync("git", ["ls-files", "--others", "--ignored", "--exclude-standard", "-z", "--", ...sourceScopes], { cwd: root }).toString().split("\0").filter(Boolean));
function capture(path) {
	let current = root;
	const parts = path.split("/");
	for (const part of parts.slice(0, -1)) {
		current = join(current, part);
		if (!lstatSync(current).isDirectory() || lstatSync(current).isSymbolicLink()) throw new Error(`Non-regular file parent: ${path}`);
	}
	const source = join(root, path);
	if (!lstatSync(source).isFile()) throw new Error(`Non-regular package file: ${path}`);
	const fd = openSync(source, "r");
	try {
		const metadata = fstatSync(fd);
		if (!metadata.isFile()) throw new Error(`Non-regular package file: ${path}`);
		return { bytes: readFileSync(fd), mode: metadata.mode & 0o777 };
	} finally { closeSync(fd); }
}
function writeCaptured(target, captured) {
	mkdirSync(dirname(target), { recursive: true });
	writeFileSync(target, captured.bytes, { mode: captured.mode });
	chmodSync(target, captured.mode);
}
const manifest = capture("packages/astra/package.json");
const packageMetadata = JSON.parse(manifest.bytes.toString("utf8"));
const { version } = packageMetadata;
if (typeof version !== "string" || !/^[0-9A-Za-z][0-9A-Za-z.+-]*$/.test(version)) throw new Error("Invalid package version");
if (packageMetadata.bundleDependencies === true || packageMetadata.bundledDependencies === true || packageMetadata.bundleDependencies?.length || packageMetadata.bundledDependencies?.length)
	throw new Error("Bundled node_modules are outside this package snapshot scope");
const output = join(root, `.artifacts/astra-v${version}/package`);
if (existsSync(output)) throw new Error(`Output already exists: ${output}; use a new release version or a clean worktree`);
mkdirSync(dirname(output), { recursive: true });
const staging = mkdtempSync(join(dirname(output), ".package.stage-"));
let filename;
try {
	const selection = join(staging, "selection");
	const snapshot = join(staging, "source");
	const controls = new Map([["packages/astra/package.json", manifest]]);
	const candidates = new Set();
	const excludedDirectories = new Set(["node_modules", ".git", ".svn", ".hg", "CVS"]);
	function captureControl(path) {
		if (controls.has(path)) return;
		try { lstatSync(join(root, path)); }
		catch (error) {
			if (error.code !== "ENOENT") throw error;
			controls.set(path, null);
			return;
		}
		controls.set(path, capture(path));
	}
	for (const parent of ["", "packages/"])
		for (const name of ["package.json", ".npmignore", ".gitignore", ".npmrc"]) captureControl(`${parent}${name}`);
	function selectionDirectory(path) {
		mkdirSync(join(selection, path), { recursive: true });
		for (const name of [".npmignore", ".gitignore"]) captureControl(`${path}/${name}`);
		if (path === "packages/astra") captureControl(`${path}/.npmrc`);
		for (const entry of readdirSync(join(root, path), { withFileTypes: true })) {
			if (excludedDirectories.has(entry.name)) continue;
			const sourcePath = `${path}/${entry.name}`;
			if (controls.has(sourcePath)) continue;
			if (entry.isDirectory()) selectionDirectory(sourcePath);
			else if (entry.isFile()) {
				writeFileSync(join(selection, sourcePath), "");
				candidates.add(sourcePath);
			}
		}
	}
	selectionDirectory("packages/astra");
	for (const path of commitFiles.keys()) {
		if (path.startsWith("packages/astra/") && /\/(?:\.npmignore|\.gitignore)$/.test(path) &&
			!path.split("/").some((part) => excludedDirectories.has(part)) && !controls.has(path)) controls.set(path, null);
	}
	for (const [path, captured] of controls) {
		if (!captured) continue;
		if (path.startsWith("packages/astra/")) candidates.add(path);
		writeCaptured(join(selection, path), captured);
		writeCaptured(join(snapshot, path), captured);
	}
	const missingCandidatePaths = [...commitFiles].filter(([path, entry]) =>
		path.startsWith("packages/astra/") && entry.type === "blob" && ["100644", "100755"].includes(entry.mode) &&
		!path.split("/").some((part) => excludedDirectories.has(part)) && !candidates.has(path)).map(([path]) => path).sort();
	const [selected] = JSON.parse(execFileSync(npm, ["pack", "--dry-run", "--ignore-scripts", "--json"], { cwd: join(selection, "packages/astra"), encoding: "utf8" }));
	const paths = selected.files.map((file) => file.path).sort();
	for (const path of paths) {
		if (typeof path !== "string" || path.includes("\\") || path.includes("\0") || /^[A-Za-z]:/.test(path) || path.split("/").some((part) => !part || part === "." || part === "..") || path.split("/").includes("node_modules"))
			throw new Error(`Invalid package path: ${path}`);
	}
	if (new Set(paths).size !== paths.length) throw new Error("Duplicate package file selection");
	for (const file of ["package.json", "dist/launcher.js", "dist/workbench.js", "dist/workbench-runner.js", "web/index.html"])
		if (!paths.includes(file)) throw new Error(`Missing ${file}; run npm run build:offline first`);
	const files = [];
	let includesUncommittedChanges = initialStatus.length > 0 || missingCandidatePaths.length > 0;
	function provenance(path, captured, selectionControl = false) {
		const committed = commitFiles.get(path);
		const blobHash = captured ? createHash(objectFormat).update(`blob ${captured.bytes.length}\0`).update(captured.bytes).digest("hex") : null;
		const matchesBaseCommit = committed ? Boolean(captured && committed.type === "blob" && committed.hash === blobHash && committed.mode === (captured.mode & 0o111 ? "100755" : "100644")) : null;
		const source = !captured ? "missing" : committed ? "tracked" : tracked.has(path) ? "uncommitted-tracked" : ignored.has(path) ? "ignored" : "untracked";
		if ((committed && !matchesBaseCommit) || (!committed && captured && (selectionControl || source !== "ignored"))) includesUncommittedChanges = true;
		return { path, present: Boolean(captured), size: captured?.bytes.length ?? null, sha256: captured ? createHash("sha256").update(captured.bytes).digest("hex") : null, sourceMode: captured?.mode ?? null, source, matchesBaseCommit, baseBlob: committed?.hash ?? null, baseMode: committed?.mode ?? null };
	}
	const selectionControls = [...controls].map(([path, captured]) => provenance(path, captured, true));
	for (const path of paths) {
		const sourcePath = `packages/astra/${path}`;
		const captured = controls.has(sourcePath) ? controls.get(sourcePath) : capture(sourcePath);
		if (!captured) throw new Error(`Missing captured package control: ${path}`);
		writeCaptured(join(snapshot, sourcePath), captured);
		files.push({ ...provenance(sourcePath, captured), path });
	}
	const [packed] = JSON.parse(execFileSync(
	npm,
	["pack", "--ignore-scripts", "--json", "--pack-destination", staging],
	{ cwd: join(snapshot, "packages/astra"), encoding: "utf8" },
));
	const packedPaths = packed.files.map((file) => file.path).sort();
	if (packed.version !== version || JSON.stringify(packedPaths) !== JSON.stringify(paths)) throw new Error("Package version or file selection changed during capture; retry with stable package rules");
	if (!/^[^\\/]+\.tgz$/.test(packed.filename)) throw new Error("Invalid package archive filename");
	for (const file of files) file.mode = packed.files.find((entry) => entry.path === file.path).mode;
	const checksum = createHash("sha256").update(readFileSync(join(staging, packed.filename))).digest("hex");
	writeFileSync(join(staging, "SHA256SUMS"), `${checksum}  ${packed.filename}\n`);
	if (execFileSync("git", ["status", "--porcelain"], { cwd: root }).toString().trim()) includesUncommittedChanges = true;
	writeFileSync(join(staging, "PACKAGE_SOURCE.json"), `${JSON.stringify({
	version,
	baseCommit,
	includesUncommittedChanges,
	sourceBoundary: "Captured existing package files; ignored files are pre-existing assets or build outputs, not rebuilt or verified against source by this command",
	files,
	selectionControls,
	missingCandidatePaths,
}, null, 2)}\n`);
	rmSync(selection, { recursive: true });
	rmSync(snapshot, { recursive: true });
	if (execFileSync("git", ["rev-parse", "HEAD"], { cwd: root }).toString().trim() !== baseCommit)
		throw new Error("Package HEAD changed during capture; retry against a stable commit");
	if (existsSync(output)) throw new Error(`Output already exists: ${output}`);
	renameSync(staging, output);
	filename = packed.filename;
} finally {
	rmSync(staging, { recursive: true, force: true });
}
console.log(`Prepared local package: ${join(output, filename)}\nNothing published or pushed.`);
