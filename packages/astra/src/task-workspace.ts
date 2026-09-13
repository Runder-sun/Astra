import { createHash } from "node:crypto";
import { access, lstat, mkdir, readFile, realpath, rename, rm, writeFile } from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { assertAstraId, atomicWriteJson, canonicalArtifactPath, taskDir } from "./contracts.ts";
import { sourceReceiptFilename } from "./literature.ts";
import type { ResearchJob } from "./research.ts";
import type { Evidence, TaskPacket } from "./types.ts";

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
}

export async function taskInputResources(task: TaskPacket, job: ResearchJob): Promise<TaskInputResource[]> {
	const snapshot = job.state;
	const resources: TaskInputResource[] = [];
	for (const ref of new Set(task.inputArtifactRefs)) {
		const artifact = snapshot.canonical[ref];
		const evidence = snapshot.evidence[artifact?.evidenceId ?? ref];
		if (!evidence) continue;
		const root = taskResourcePath(task.scope.workspaceRoot, task.jobId, evidence.taskId);
		try {
			const metadata = await lstat(root);
			if (metadata.isSymbolicLink()) {
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
	return resources;
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
	await rm(destination, { force: true });
	await writeFile(destination, content, { mode: 0o444 });
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
		for (const sourceRef of evidence.refs) {
			const receipt = sourceReceiptFilename(sourceRef);
			const allowedRoot = receipt ? join(projectRoot, ".astra", "jobs", task.jobId, "sources") : sourceRoot;
			const source = receipt ? join(allowedRoot, receipt) : evidenceFileSource(projectRoot, sourceRoot, sourceRef);
			if (!source) continue;
			const content = await readEvidenceFile(source, allowedRoot);
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
		await writeFile(
			join(workspace, reviewSummaryPath),
			`${JSON.stringify({ schemaVersion: "astra.research_review_summary.v1", artifacts: reviewSummaries })}\n`,
			{ encoding: "utf8", mode: 0o444 },
		);
	}
	const evidence = relevantInputRefs.flatMap((ref) => {
		const value = snapshot.evidence[ref];
		return value ? [value] : [];
	});
	const openObligations = snapshot.frame.openObligationIds.flatMap((id) => {
		const obligation = snapshot.obligations[id];
		return obligation ? [obligation] : [];
	});
	await atomicWriteJson(join(workspace, "ASTRA_TASK_CONTEXT.json"), {
		schemaVersion: "astra.task_context.v1",
		mission: {
			...snapshot.frame,
			permissions: { ...snapshot.frame.permissions, workspaceRoot: "." },
		},
		stage: job.definitions[task.stageId],
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
		resources: { writableRoot: writableResourceRoot ?? null },
		openObligations,
		createdAt: new Date().toISOString(),
	});
	return workspace;
}

export async function prepareReviewEvidenceBundle(
	task: TaskPacket,
	evidence: Evidence,
	job: ResearchJob,
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
	): Promise<void> => {
		const copyKey = `${source}\0${sourceRef}`;
		if (copiedSources.has(copyKey)) return;
		const content = await readEvidenceFile(source, allowedRoot);
		if (!content) return;
		await writeEvidenceFile(reviewRoot, path, content);
		copiedSources.add(copyKey);
		bundle.push({
			sourceRef,
			path: path.split("\\").join("/"),
			sha256: createHash("sha256").update(content).digest("hex"),
		});
	};
	for (const sourceRef of evidence.refs) {
		const filename = sourceReceiptFilename(sourceRef);
		if (filename) {
			await copyIntoBundle(join(sourcesRoot, filename), sourcesRoot, sourceRef, join("sources", filename));
			continue;
		}
		const source = evidenceFileSource(projectRoot, sourceWorkspace, sourceRef);
		if (source)
			await copyIntoBundle(
				source,
				sourceWorkspace,
				sourceRef,
				join("evidence", evidence.id, relative(sourceWorkspace, source)),
			);
	}

	const sourceTask = job.state.tasks[evidence.taskId];
	if (!sourceTask) throw new Error(`source task not found for review evidence: ${evidence.id}`);
	const canonicalRoot = join(projectRoot, ".astra", "jobs", task.jobId, "canonical");
	for (const inputRef of sourceTask.inputArtifactRefs) {
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
		const upstreamWorkspace = taskWorkspacePath(projectRoot, task.jobId, upstreamEvidence.taskId);
		for (const sourceRef of upstreamEvidence.refs) {
			const filename = sourceReceiptFilename(sourceRef);
			if (filename) {
				await copyIntoBundle(join(sourcesRoot, filename), sourcesRoot, sourceRef, join("sources", filename));
				continue;
			}
			const source = evidenceFileSource(projectRoot, upstreamWorkspace, sourceRef);
			if (!source) continue;
			const materializedRef = join("inputs", inputRef, relative(upstreamWorkspace, source)).split("\\").join("/");
			await copyIntoBundle(source, upstreamWorkspace, materializedRef, materializedRef);
		}
	}
	return bundle;
}
