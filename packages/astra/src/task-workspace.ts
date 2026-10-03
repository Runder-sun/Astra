import { createHash } from "node:crypto";
import { access, lstat, mkdir, open, readFile, realpath, rename, rm } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import {
	assertAstraId,
	atomicWriteJson,
	canonicalArtifactPath,
	publishImmutableFile,
	readJson,
	taskDir,
	taskStageContract,
	workerManifestPath,
	writeTaskPacket,
} from "./contracts.ts";
import { sourceTaskContractHash } from "./effective-contract.ts";
import { readSourceReceipt, sourceReceiptFilename } from "./literature.ts";
import { checksum, type ResearchJob } from "./research.ts";
import type {
	Evidence,
	EvidenceFileVersion,
	OutputRef,
	ReviewPacket,
	TaskPacket,
	TaskRecoveryMaterials,
	WorkerOutputManifest,
} from "./types.ts";

/** Verify the declared copies as ordinary files; never regenerate a reviewer's damaged bundle. */
export async function verifyReviewEvidenceBundle(
	task: TaskPacket,
	evidence: Evidence,
	packet: ReviewPacket,
	job: ResearchJob,
): Promise<void> {
	const root = resolve(taskDir(task.scope.workspaceRoot, task.jobId, task.id));
	const rootReal = await realpath(root);
	const expected = await collectReviewEvidenceBundle(task, evidence, job, false);
	if (expected.length !== packet.resolvedEvidenceRefs.length)
		throw new Error("review frozen file declarations differ from their bound sources");
	for (const file of expected) {
		const actual = packet.resolvedEvidenceRefs.find(
			(ref) => ref.sourceRef === file.sourceRef && ref.path === file.path,
		);
		if (!actual || (!file.path.startsWith("input-evidence/") && actual.sha256 !== file.sha256))
			throw new Error(`review frozen file binding is missing or has the wrong version: ${file.sourceRef}`);
	}
	const seen = new Set<string>();
	for (const ref of packet.resolvedEvidenceRefs) {
		if (
			!ref ||
			typeof ref.path !== "string" ||
			typeof ref.sourceRef !== "string" ||
			!/^[a-f0-9]{64}$/.test(ref.sha256)
		)
			throw new Error("invalid review evidence file declaration");
		const path = resolve(root, ref.path);
		if (isAbsolute(ref.path) || !isInside(root, path) || path === root || seen.has(path))
			throw new Error(`review evidence path is outside its bundle or duplicated: ${ref.path}`);
		seen.add(path);
		let parent = root;
		for (const component of relative(root, path).split(/[\\/]/)) {
			parent = join(parent, component);
			if ((await lstat(parent)).isSymbolicLink())
				throw new Error(`review evidence may not use symbolic links: ${ref.path}`);
		}
		if (!(await lstat(path)).isFile() || (await realpath(path)) !== join(rootReal, relative(root, path)))
			throw new Error(`review evidence is not an ordinary bundle file: ${ref.path}`);
		const bytes = await readFile(path);
		if (createHash("sha256").update(bytes).digest("hex") !== ref.sha256)
			throw new Error(`review evidence file integrity failure: ${ref.path}`);
		if (ref.path.startsWith("input-evidence/")) {
			const id = ref.path.slice("input-evidence/".length, -".json".length);
			const input = job.state.evidence[id];
			const saved = JSON.parse(bytes.toString("utf8")) as Evidence;
			const immutable = (value: Evidence) =>
				JSON.parse(
					JSON.stringify({
						...value,
						status: undefined,
						acceptanceAuthority: undefined,
						mainAgentDecisionRef: undefined,
						supersededByTaskId: undefined,
					}),
				) as unknown;
			if (!input || checksum(immutable(saved)) !== checksum(immutable(input)))
				throw new Error("review upstream evidence frozen identity mismatch");
		}
	}
}

const RESEARCH_REVIEW_FIELDS: Record<string, string[]> = {
	validation: ["researchQuestion", "scope", "nonGoals", "acceptanceCriteria", "falsifiableNextStep"],
	literature: ["sources", "closestWork", "gaps", "synthesis"],
	idea: ["selection", "mechanisms", "expectedContributions", "risks"],
	novelty: ["closestPriorWork", "overlap", "differentiators", "verdict"],
	refine: ["problem", "method", "assumptions", "failureModes"],
	"experiment-plan": ["claims", "baselines", "metrics", "stoppingCriteria"],
	"implement-solution": ["implementation", "files", "tests", "commands", "limitations"],
	run: ["commands", "runs", "metrics", "logs", "failures"],
	monitor: ["runStatus", "metrics", "anomalies", "integrityChecks", "decisions"],
	"result-to-claim": [
		"scientificOutcome",
		"missionCoverage",
		"claims",
		"supportingResults",
		"unsupportedClaims",
		"missingEvidence",
		"conclusion",
	],
	"paper-plan": ["narrative", "claimMap", "figurePlan", "citationPlan"],
	"paper-write": ["manuscript", "citations", "claimBindings", "limitations"],
	"paper-compile": ["artifact", "command", "buildLog", "validation", "remainingWarnings"],
};

function compactReviewValue(value: unknown, depth = 0): unknown {
	if (typeof value === "string") return value.length <= 120 ? value : `${value.slice(0, 120)}...`;
	if (value === null || typeof value !== "object") return value;
	if (depth >= 2) {
		return Array.isArray(value)
			? { itemCount: value.length }
			: { fieldNames: Object.keys(value as Record<string, unknown>).slice(0, 8) };
	}
	if (Array.isArray(value)) {
		return {
			itemCount: value.length,
			sample: value.slice(0, 1).map((entry) => compactReviewValue(entry, depth + 1)),
		};
	}
	const entries = Object.entries(value as Record<string, unknown>);
	return {
		...Object.fromEntries(entries.slice(0, 3).map(([key, entry]) => [key, compactReviewValue(entry, depth + 1)])),
		...(entries.length > 3 ? { omittedFieldCount: entries.length - 3 } : {}),
	};
}

export function taskWorkspacePath(workspaceRoot: string, jobId: string, taskId: string): string {
	assertAstraId(jobId, "job id");
	assertAstraId(taskId, "task id");
	return join(resolve(workspaceRoot), ".astra", "jobs", jobId, "workspaces", taskId);
}

export function taskResourcePath(workspaceRoot: string, jobId: string, taskId: string): string {
	assertAstraId(jobId, "job id");
	assertAstraId(taskId, "task id");
	return join(resolve(workspaceRoot), ".astra", "jobs", jobId, "resources", taskId);
}

export interface TaskInputResource {
	artifactId: string;
	artifactType: string;
	taskId: string;
	root: string;
	envVar: string;
	purpose?: "historical-repair";
}

export async function taskInputResources(task: TaskPacket, job: ResearchJob): Promise<TaskInputResource[]> {
	const snapshot = job.state;
	const resources: TaskInputResource[] = [];
	const project = resolve(task.scope.workspaceRoot);
	const projectReal = await realpath(project);
	for (const ref of new Set(task.inputArtifactRefs)) {
		const artifact = snapshot.canonical[ref];
		const evidence = snapshot.evidence[artifact?.evidenceId ?? ref];
		if (!evidence) continue;
		const root = taskResourcePath(task.scope.workspaceRoot, task.jobId, evidence.taskId);
		try {
			const metadata = await lstat(root);
			if (metadata.isSymbolicLink() || (await realpath(root)) !== join(projectReal, relative(project, root))) {
				throw new Error(`upstream task resource may not use a symbolic link: ${evidence.taskId}`);
			}
			if (metadata.isDirectory()) {
				resources.push({
					artifactId: ref,
					artifactType: artifact?.type ?? evidence.type,
					taskId: evidence.taskId,
					root,
					envVar: `ASTRA_INPUT_RESOURCE_ROOT_${resources.length}`,
				});
			}
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
	}
	for (const receipt of job.historicalRepairArchives(task.stageId)) {
		const root = join(
			resolve(task.scope.workspaceRoot),
			".astra",
			"jobs",
			task.jobId,
			"archive",
			"tasks",
			receipt.taskId,
		);
		if (!receipt.archiveRefs?.includes(root)) continue;
		try {
			const metadata = await lstat(root);
			if (metadata.isSymbolicLink() || (await realpath(root)) !== join(projectReal, relative(project, root)))
				throw new Error(`retired task archive may not use a symbolic link: ${receipt.taskId}`);
			if (metadata.isDirectory())
				resources.push({
					artifactId: receipt.artifactId,
					artifactType: receipt.type,
					taskId: receipt.taskId,
					root,
					envVar: `ASTRA_INPUT_RESOURCE_ROOT_${resources.length}`,
					purpose: "historical-repair",
				});
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
	}
	return resources;
}

/** Recovery is tied to one registered predecessor, never to model-provided paths. */
export async function taskRecoveryMaterials(
	task: TaskPacket,
	job: ResearchJob,
): Promise<TaskRecoveryMaterials | undefined> {
	if (!task.supersedesTaskId) return undefined;
	const snapshot = job.state;
	const registered = snapshot.tasks[task.id];
	const previous = snapshot.tasks[task.supersedesTaskId];
	if (
		!registered ||
		!previous ||
		task.jobId !== snapshot.frame.jobId ||
		previous.jobId !== task.jobId ||
		registered.supersedesTaskId !== previous.id ||
		registered.attempt !== task.attempt ||
		registered.agentId !== task.agentId ||
		previous.status !== "failed" ||
		previous.role !== "worker" ||
		task.role !== "worker" ||
		task.attempt !== previous.attempt + 1 ||
		previous.stageRevision !== task.stageRevision ||
		sourceTaskContractHash(task) !== sourceTaskContractHash(previous) ||
		sourceTaskContractHash(task) !== sourceTaskContractHash(registered) ||
		resolve(task.scope.workspaceRoot) !== resolve(snapshot.frame.permissions.workspaceRoot) ||
		resolve(previous.scope.workspaceRoot) !== resolve(task.scope.workspaceRoot)
	)
		throw new Error("retry recovery requires a registered same-job predecessor with the same task contract");
	const project = resolve(task.scope.workspaceRoot);
	const projectReal = await realpath(project);
	const validatedPath = async (path: string, directory: boolean): Promise<string | undefined> => {
		try {
			const metadata = await lstat(path);
			if (metadata.isSymbolicLink() || (await realpath(path)) !== join(projectReal, relative(project, path)))
				throw new Error("retry recovery path may not escape through symbolic links");
			if (directory ? !metadata.isDirectory() : !metadata.isFile())
				throw new Error("retry recovery path has the wrong file type");
			return path;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
			throw error;
		}
	};
	const workspaceRoot = await validatedPath(taskWorkspacePath(project, task.jobId, previous.id), true);
	const session = Object.values(snapshot.sessions)
		.filter(
			(entry) =>
				entry.taskId === previous.id &&
				entry.role === "worker" &&
				entry.attempt === previous.attempt &&
				["failed", "interrupted"].includes(entry.status),
		)
		.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
	let sessionFile: string | undefined;
	if (session?.sessionFile) {
		const path = resolve(session.sessionFile);
		const jobRoot = join(project, ".astra", "jobs", task.jobId);
		const expectedSessionId = `astra-${task.jobId}-${previous.id}-${previous.attempt}`.replace(
			/[^A-Za-z0-9._-]/g,
			"-",
		);
		const piLog =
			isInside(join(jobRoot, "sessions"), path) &&
			session.sessionId === expectedSessionId &&
			basename(path).endsWith(`_${expectedSessionId}.jsonl`);
		const codexLog = path === join(jobRoot, "codex-events", `${previous.id}-${previous.attempt}.jsonl`);
		const failureLog = path === join(taskDir(project, task.jobId, previous.id), "failure-log.json");
		if (!piLog && !codexLog && !failureLog)
			throw new Error("retry recovery log must belong to the same job and predecessor");
		sessionFile = await validatedPath(path, false);
	}
	return {
		previousTaskId: previous.id,
		workspaceRoot,
		sessionFile,
		error: session?.error,
		readRoots: [...(workspaceRoot ? [workspaceRoot] : []), ...(sessionFile ? [sessionFile] : [])],
	};
}

function isInside(root: string, path: string): boolean {
	const value = relative(root, path);
	return (
		value === "" ||
		(value !== ".." && !value.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) && !isAbsolute(value))
	);
}

async function readEvidenceFile(source: string, allowedRoot: string): Promise<Buffer | undefined> {
	if (!isInside(allowedRoot, source)) throw new Error(`evidence file is outside its task workspace: ${source}`);
	try {
		const metadata = await lstat(source);
		if (metadata.isSymbolicLink()) throw new Error(`evidence may not use symbolic links: ${source}`);
		if (!isInside(await realpath(allowedRoot), await realpath(source)))
			throw new Error(`evidence file is outside its task workspace through a symbolic link: ${source}`);
		return metadata.isFile() ? await readFile(source) : undefined;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		return undefined;
	}
}

function evidenceFileSource(projectRoot: string, workspace: string, ref: string): string | undefined {
	if (isAbsolute(ref) || /^[a-z][a-z0-9+.-]*:/i.test(ref)) return undefined;
	return resolve(ref.startsWith(".astra/") ? projectRoot : workspace, ref);
}

export async function freezeEvidenceFiles(
	task: TaskPacket,
	refs: string[],
	boundSources?: OutputRef[],
): Promise<EvidenceFileVersion[]> {
	const projectRoot = resolve(task.scope.workspaceRoot);
	let manifest: WorkerOutputManifest | undefined;
	if (task.version) {
		try {
			manifest = await readJson<WorkerOutputManifest>(workerManifestPath(projectRoot, task.jobId, task.id));
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
		}
	}
	const files: EvidenceFileVersion[] = [];
	for (const sourceRef of new Set(refs)) {
		const receipt = sourceReceiptFilename(sourceRef);
		if (receipt) {
			const binding = boundSources?.find((ref) => ref.kind === "source" && ref.ref === sourceRef);
			if (binding && !binding.sha256) throw new Error(`source snapshot binding is missing: ${sourceRef}`);
			const snapshot = await readSourceReceipt(projectRoot, task.jobId, sourceRef, binding?.sha256);
			if (!snapshot) throw new Error(`source requires an intact retrieval receipt: ${sourceRef}`);
			const sha256 = await publishImmutableFile(
				join(projectRoot, ".astra", "jobs", task.jobId, "versions", "files"),
				snapshot.content,
			);
			files.push({ sourceRef, sha256 });
			continue;
		}
		if (isAbsolute(sourceRef) || /^[a-z][a-z0-9+.-]*:/i.test(sourceRef)) continue;
		const workspace = taskWorkspacePath(projectRoot, task.jobId, task.id);
		const source = evidenceFileSource(projectRoot, workspace, sourceRef);
		if (!source) continue;
		const content = await readEvidenceFile(source, workspace);
		if (!content) {
			if (task.version) throw new Error(`submitted evidence file is missing: ${sourceRef}`);
			continue;
		}
		const sha256 = createHash("sha256").update(content).digest("hex");
		const declared = manifest?.outputRefs.find(
			(ref) => ref.ref === sourceRef || ref.ref === relative(workspace, source).split("\\").join("/"),
		);
		if (declared?.sha256 && declared.sha256 !== sha256)
			throw new Error(`submitted evidence changed after validation: ${sourceRef}`);
		await publishImmutableFile(join(projectRoot, ".astra", "jobs", task.jobId, "versions", "files"), content);
		files.push({ sourceRef, sha256 });
	}
	return files;
}

export async function readVersionedFile(
	task: TaskPacket,
	evidence: Evidence,
	sourceRef: string,
	source: string,
	allowedRoot: string,
): Promise<Buffer | undefined> {
	const file = evidence.files?.find((entry) => entry.sourceRef === sourceRef);
	if (!file) return readEvidenceFile(source, allowedRoot);
	if (!/^[a-f0-9]{64}$/.test(file.sha256)) throw new Error(`invalid evidence file hash: ${sourceRef}`);
	const root = join(resolve(task.scope.workspaceRoot), ".astra", "jobs", task.jobId, "versions", "files");
	const content = await readEvidenceFile(join(root, file.sha256), root);
	if (!content || createHash("sha256").update(content).digest("hex") !== file.sha256)
		throw new Error(`evidence version integrity failure: ${sourceRef}`);
	return content;
}

async function writeEvidenceFile(root: string, path: string, content: Buffer): Promise<void> {
	const destination = resolve(root, path);
	if (!isInside(root, destination)) throw new Error(`evidence destination is outside its bundle: ${path}`);
	await mkdir(root, { recursive: true });
	let parent = root;
	for (const part of relative(root, dirname(destination)).split(/[\\/]/).filter(Boolean)) {
		parent = join(parent, part);
		try {
			await mkdir(parent);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
		}
		if ((await lstat(parent)).isSymbolicLink())
			throw new Error(`evidence destination may not use symbolic links: ${parent}`);
	}
	const temp = `${destination}.tmp-${process.pid}-${Math.random().toString(36).slice(2)}`;
	const handle = await open(temp, "wx", 0o444);
	try {
		await handle.writeFile(content);
		await rename(temp, destination);
	} finally {
		await handle.close();
		await rm(temp, { force: true });
	}
}

async function observedExecutionFiles(task: TaskPacket | undefined, prefix = "") {
	if (!task) return [];
	const root = join(resolve(task.scope.workspaceRoot), ".astra", "jobs", task.jobId, "codex-events");
	const sourceRef = join(root, `${task.id}-${task.attempt}.jsonl`);
	const log = await readEvidenceFile(sourceRef, root);
	if (!log) return [];
	const failures: Record<string, unknown>[] = [];
	const searches: Record<string, unknown>[] = [];
	for (const line of log.toString("utf8").split("\n").filter(Boolean)) {
		const event = JSON.parse(line);
		const item = event.params?.item;
		if (
			event.method === "item/completed" &&
			item?.type === "commandExecution" &&
			((typeof item.exitCode === "number" && item.exitCode !== 0) || item.status === "failed")
		)
			failures.push(item);
		if (
			event.method === "item/completed" &&
			(item?.type === "webSearch" || (item?.type === "dynamicToolCall" && item.tool === "astra_search_literature"))
		) {
			searches.push({ emittedAtMs: event.emittedAtMs, item });
		}
	}
	return [
		{
			name: "failed-commands",
			schemaVersion: "astra.observed_command_failures.v1",
			key: "failures",
			entries: failures,
		},
		{
			name: "literature-searches",
			schemaVersion: "astra.observed_literature_searches.v1",
			key: "searches",
			entries: searches,
		},
	]
		.filter((receipt) => receipt.entries.length)
		.map((receipt) => ({
			sourceRef,
			path: `${prefix}execution/${task.id}/${receipt.name}.json`,
			content: Buffer.from(
				JSON.stringify(
					{
						schemaVersion: receipt.schemaVersion,
						taskId: task.id,
						attempt: task.attempt,
						sourceLogRef: sourceRef,
						sourceLogSha256: createHash("sha256").update(log).digest("hex"),
						[receipt.key]: receipt.entries,
					},
					null,
					2,
				),
			),
		}));
}

async function repairSourceFiles(task: TaskPacket, job: ResearchJob, inputRef: string) {
	if (inputRef !== task.repairOfEvidenceId) return [];
	const evidence = job.state.evidence[inputRef];
	if (!evidence) return [];
	const root = taskDir(task.scope.workspaceRoot, task.jobId, evidence.taskId);
	const sourceRef = join(root, "task-packet.json");
	const content = await readEvidenceFile(sourceRef, root);
	return [
		...(content ? [{ sourceRef, path: `inputs/${inputRef}/repair-source-task.json`, content }] : []),
		{
			sourceRef: `reviews:${inputRef}`,
			path: `inputs/${inputRef}/repair-source-reviews.json`,
			content: Buffer.from(
				JSON.stringify(
					Object.values(job.state.reviews).filter((review) => review.evidenceId === inputRef),
					null,
					2,
				),
			),
		},
	];
}

async function materializeInputFiles(
	inputArtifactRefs: string[],
	task: TaskPacket,
	job: ResearchJob,
	workspace: string,
): Promise<Array<{ artifactId: string; sourceRef: string; path: string; sha256: string }>> {
	const projectRoot = resolve(task.scope.workspaceRoot);
	const inputs: Array<{ artifactId: string; sourceRef: string; path: string; sha256: string }> = [];
	for (const artifactRef of inputArtifactRefs) {
		const artifact = job.state.canonical[artifactRef];
		const evidence = job.state.evidence[artifact?.evidenceId ?? artifactRef];
		if (!evidence) continue;
		const sourceRoot = taskWorkspacePath(projectRoot, task.jobId, evidence.taskId);
		for (const file of [
			...(await repairSourceFiles(task, job, artifactRef)),
			...(await observedExecutionFiles(job.state.tasks[evidence.taskId], `inputs/${artifactRef}/`)),
		]) {
			await writeEvidenceFile(workspace, file.path, file.content);
			inputs.push({
				artifactId: artifactRef,
				sourceRef: file.sourceRef,
				path: file.path,
				sha256: createHash("sha256").update(file.content).digest("hex"),
			});
		}
		for (const sourceRef of evidence.refs) {
			const receipt = sourceReceiptFilename(sourceRef);
			const allowedRoot = receipt ? join(projectRoot, ".astra", "jobs", task.jobId, "sources") : sourceRoot;
			const source = receipt ? join(allowedRoot, receipt) : evidenceFileSource(projectRoot, sourceRoot, sourceRef);
			if (!source) continue;
			const content = await readVersionedFile(task, evidence, sourceRef, source, allowedRoot);
			if (!content) continue;
			const path = join("inputs", artifactRef, receipt ? join("sources", receipt) : relative(sourceRoot, source))
				.split("\\")
				.join("/");
			await writeEvidenceFile(workspace, path, content);
			inputs.push({
				artifactId: artifactRef,
				sourceRef,
				path,
				sha256: createHash("sha256").update(content).digest("hex"),
			});
		}
	}
	return inputs;
}

export async function prepareTaskWorkspace(task: TaskPacket, job: ResearchJob): Promise<string> {
	const recovery = await taskRecoveryMaterials(task, job);
	const version = await job.captureTaskVersion(task.id);
	await writeTaskPacket({ ...task, version });
	const workspace = taskWorkspacePath(task.scope.workspaceRoot, task.jobId, task.id);
	await mkdir(workspace, { recursive: true });
	const writableResourceRoot =
		task.writeAuthority === "workspace-write"
			? taskResourcePath(task.scope.workspaceRoot, task.jobId, task.id)
			: undefined;
	if (writableResourceRoot) {
		await mkdir(dirname(writableResourceRoot), { recursive: true });
		if (task.supersedesTaskId) {
			try {
				await access(writableResourceRoot);
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
				try {
					await rename(
						taskResourcePath(task.scope.workspaceRoot, task.jobId, task.supersedesTaskId),
						writableResourceRoot,
					);
				} catch (moveError) {
					if ((moveError as NodeJS.ErrnoException).code !== "ENOENT") throw moveError;
				}
			}
		}
		await mkdir(writableResourceRoot, { recursive: true });
	}
	const snapshot = job.state;
	const relevantInputRefs = [...new Set(task.inputArtifactRefs)];
	const files = await materializeInputFiles(relevantInputRefs, task, job, workspace);
	const canonicalArtifacts: Array<Record<string, unknown>> = [];
	const resources = await taskInputResources(task, job);
	const reviewSummaries: Array<Record<string, unknown>> = [];
	for (const ref of relevantInputRefs) {
		const artifact = snapshot.canonical[ref];
		if (!artifact) continue;
		const metadata = {
			id: artifact.id,
			type: artifact.type,
			evidenceId: artifact.evidenceId,
			checksum: artifact.checksum,
			status: artifact.status,
			replacementOf: artifact.replacementOf,
			evidenceSnapshotHash: artifact.evidenceSnapshotHash,
			sourceSha256: artifact.sourceSha256,
			targetSha256: artifact.targetSha256,
			adoptedAt: artifact.adoptedAt,
		};
		const contentPath = join("canonical", `${artifact.id}.json`).split("\\").join("/");
		await atomicWriteJson(join(workspace, contentPath), {
			...metadata,
			projection: { kind: "canonical-full", includedFields: ["content"] },
			content: artifact.content,
		});
		if (task.stageId === "research-review") {
			const configuredFields = RESEARCH_REVIEW_FIELDS[artifact.type];
			const includedFields =
				configuredFields &&
				artifact.content !== null &&
				typeof artifact.content === "object" &&
				!Array.isArray(artifact.content)
					? configuredFields.filter((field) => Object.hasOwn(artifact.content as Record<string, unknown>, field))
					: undefined;
			const projectedContent = includedFields
				? Object.fromEntries(
						includedFields.map((field) => [field, (artifact.content as Record<string, unknown>)[field]]),
					)
				: artifact.content;
			canonicalArtifacts.push({ ...metadata, contentPath });
			reviewSummaries.push({
				id: artifact.id,
				type: artifact.type,
				contentPath,
				files: files.filter((file) => file.artifactId === artifact.id).map((file) => file.path),
				resourceRoots: resources
					.filter((resource) => resource.artifactId === artifact.id)
					.map((resource) => resource.root),
				summary: compactReviewValue(projectedContent),
			});
		} else {
			canonicalArtifacts.push({ ...metadata, contentPath });
		}
	}
	const reviewSummaryPath = task.stageId === "research-review" ? "review-summary.json" : undefined;
	if (reviewSummaryPath) {
		await writeEvidenceFile(
			workspace,
			reviewSummaryPath,
			Buffer.from(
				`${JSON.stringify({ schemaVersion: "astra.research_review_summary.v1", artifacts: reviewSummaries })}\n`,
			),
		);
	}
	const evidence: Array<Evidence & { contentPath: string }> = [];
	for (const ref of relevantInputRefs) {
		const value = snapshot.evidence[ref];
		if (!value) continue;
		const contentPath = `input-evidence/${value.id}.json`;
		const content = Buffer.from(JSON.stringify(value, null, 2));
		await writeEvidenceFile(workspace, contentPath, content);
		files.push({
			artifactId: ref,
			sourceRef: contentPath,
			path: contentPath,
			sha256: createHash("sha256").update(content).digest("hex"),
		});
		evidence.push({ ...value, contentPath });
	}
	const openObligations = snapshot.frame.openObligationIds.flatMap((id) => {
		const obligation = snapshot.obligations[id];
		return obligation ? [obligation] : [];
	});
	await atomicWriteJson(join(workspace, "ASTRA_TASK_CONTEXT.json"), {
		schemaVersion: "astra.task_context.v1",
		version,
		mission: {
			...snapshot.frame,
			permissions: { ...snapshot.frame.permissions, workspaceRoot: "." },
		},
		stage: taskStageContract(job.definitions[task.stageId], task),
		task: {
			...task,
			inputArtifactRefs: relevantInputRefs,
			requiredCanonicalArtifacts: task.requiredCanonicalArtifacts.filter((ref) => relevantInputRefs.includes(ref)),
			scope: { ...task.scope, workspaceRoot: "." },
		},
		inputs: {
			canonicalArtifacts,
			...(reviewSummaryPath ? { reviewSummaryPath } : {}),
			evidence,
			files,
			resources,
			omittedRefs: [],
		},
		recovery: recovery ?? null,
		resources: { writableRoot: writableResourceRoot ?? null },
		openObligations,
		createdAt: new Date().toISOString(),
	});
	return workspace;
}

async function collectReviewEvidenceBundle(
	task: TaskPacket,
	evidence: Evidence,
	job: ResearchJob,
	publish: boolean,
): Promise<Array<{ sourceRef: string; path: string; sha256: string }>> {
	const projectRoot = resolve(task.scope.workspaceRoot);
	const sourceWorkspace = taskWorkspacePath(projectRoot, task.jobId, evidence.taskId);
	const sourcesRoot = join(projectRoot, ".astra", "jobs", task.jobId, "sources");
	const reviewRoot = taskDir(projectRoot, task.jobId, task.id);
	const bundle: Array<{ sourceRef: string; path: string; sha256: string }> = [];
	const copiedSources = new Set<string>();
	const copyIntoBundle = async (
		source: string,
		allowedRoot: string,
		sourceRef: string,
		path: string,
		versionedEvidence?: Evidence,
		originalRef = sourceRef,
	): Promise<void> => {
		const content = versionedEvidence
			? await readVersionedFile(task, versionedEvidence, originalRef, source, allowedRoot)
			: await readEvidenceFile(source, allowedRoot);
		if (!content) return;
		const sha256 = createHash("sha256").update(content).digest("hex");
		const copyKey = `${sourceRef}\0${sha256}`;
		if (copiedSources.has(copyKey)) return;
		if (sourceReceiptFilename(originalRef)) path = join("sources", sha256, basename(path));
		if (publish) await writeEvidenceFile(reviewRoot, path, content);
		copiedSources.add(copyKey);
		bundle.push({
			sourceRef,
			path: path.split("\\").join("/"),
			sha256,
		});
	};
	for (const sourceRef of evidence.refs) {
		const filename = sourceReceiptFilename(sourceRef);
		if (filename) {
			await copyIntoBundle(join(sourcesRoot, filename), sourcesRoot, sourceRef, join("sources", filename), evidence);
			continue;
		}
		const source = evidenceFileSource(projectRoot, sourceWorkspace, sourceRef);
		if (source)
			await copyIntoBundle(
				source,
				sourceWorkspace,
				sourceRef,
				join("evidence", evidence.id, relative(sourceWorkspace, source)),
				evidence,
			);
	}

	const sourceTask = job.state.tasks[evidence.taskId];
	if (!sourceTask) throw new Error(`source task not found for review evidence: ${evidence.id}`);
	for (const file of await observedExecutionFiles(sourceTask)) {
		if (publish) await writeEvidenceFile(reviewRoot, file.path, file.content);
		bundle.push({
			sourceRef: file.sourceRef,
			path: file.path,
			sha256: createHash("sha256").update(file.content).digest("hex"),
		});
	}
	const canonicalRoot = join(projectRoot, ".astra", "jobs", task.jobId, "canonical");
	for (const inputRef of sourceTask.inputArtifactRefs) {
		for (const file of await repairSourceFiles(sourceTask, job, inputRef)) {
			if (publish) await writeEvidenceFile(reviewRoot, file.path, file.content);
			bundle.push({
				sourceRef: file.sourceRef,
				path: file.path,
				sha256: createHash("sha256").update(file.content).digest("hex"),
			});
		}
		const artifact = job.state.canonical[inputRef];
		if (artifact)
			await copyIntoBundle(
				canonicalArtifactPath(projectRoot, task.jobId, artifact.id),
				canonicalRoot,
				`canonical/${artifact.id}.json`,
				`canonical/${artifact.id}.json`,
			);
		const upstreamEvidence = job.state.evidence[artifact?.evidenceId ?? inputRef];
		if (!upstreamEvidence) continue;
		for (const file of await observedExecutionFiles(
			job.state.tasks[upstreamEvidence.taskId],
			`inputs/${inputRef}/`,
		)) {
			if (publish) await writeEvidenceFile(reviewRoot, file.path, file.content);
			bundle.push({
				sourceRef: file.sourceRef,
				path: file.path,
				sha256: createHash("sha256").update(file.content).digest("hex"),
			});
		}
		if (!artifact) {
			const path = join("input-evidence", `${inputRef}.json`).split("\\").join("/");
			const content = Buffer.from(JSON.stringify(upstreamEvidence, null, 2));
			if (publish) await writeEvidenceFile(reviewRoot, path, content);
			bundle.push({ sourceRef: path, path, sha256: createHash("sha256").update(content).digest("hex") });
		}
		const upstreamWorkspace = taskWorkspacePath(projectRoot, task.jobId, upstreamEvidence.taskId);
		for (const sourceRef of upstreamEvidence.refs) {
			const filename = sourceReceiptFilename(sourceRef);
			if (filename) {
				await copyIntoBundle(
					join(sourcesRoot, filename),
					sourcesRoot,
					sourceRef,
					join("sources", filename),
					upstreamEvidence,
				);
				continue;
			}
			const source = evidenceFileSource(projectRoot, upstreamWorkspace, sourceRef);
			if (!source) continue;
			const materializedRef = join("inputs", inputRef, relative(upstreamWorkspace, source)).split("\\").join("/");
			await copyIntoBundle(source, upstreamWorkspace, materializedRef, materializedRef, upstreamEvidence, sourceRef);
		}
	}
	return bundle;
}

export async function prepareReviewEvidenceBundle(
	task: TaskPacket,
	evidence: Evidence,
	job: ResearchJob,
): Promise<Array<{ sourceRef: string; path: string; sha256: string }>> {
	return collectReviewEvidenceBundle(task, evidence, job, true);
}
