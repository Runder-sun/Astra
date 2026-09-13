import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import { scientificAssessment } from "./research.ts";
import type { OutputRef, TaskPacket } from "./types.ts";

const MAX_REFERENCED_FILE_BYTES = 32 * 1024 * 1024;
const SOURCE_REF_PATTERN = /^(?:https:\/\/|openalex:|doi:|arxiv:)/;

export interface WorkerSubmission {
	artifactType: string;
	content: unknown;
	refs: Array<{ kind: string; ref: string; summary: string; sha256?: string }>;
}

export interface WorkerSubmissionOptions {
	executionRoot: string;
	sessionRef: string;
	minSourceRefs?: number;
}

function isInside(root: string, path: string): boolean {
	const value = relative(root, path);
	return value === "" || (!value.startsWith("..") && !isAbsolute(value));
}

function sha256(content: Buffer): string {
	return createHash("sha256").update(content).digest("hex");
}

export async function validateWorkerSubmission(
	packet: TaskPacket,
	submission: WorkerSubmission,
	options: WorkerSubmissionOptions,
): Promise<{ content: unknown; outputRefs: OutputRef[] }> {
	if (submission.artifactType !== packet.requiredOutputType) {
		throw new Error(
			`worker artifact type ${submission.artifactType} does not match required output type ${packet.requiredOutputType}`,
		);
	}
	if (submission.content === null || typeof submission.content !== "object" || Array.isArray(submission.content)) {
		throw new Error("worker content must be a structured object");
	}
	const content = submission.content as Record<string, unknown>;
	const missingFields = packet.requiredOutputFields.filter(
		(field) => !Object.hasOwn(content, field) || content[field] === undefined || content[field] === null,
	);
	if (missingFields.length > 0) {
		throw new Error(`worker content is missing required output fields: ${missingFields.join(", ")}`);
	}
	if (["result-to-claim", "research-review"].includes(packet.requiredOutputType) && !scientificAssessment(content)) {
		throw new Error(
			`${packet.requiredOutputType} requires machine-readable outcome labels: scientificOutcome must be supported, partially-supported, refuted, inconclusive, or insufficient-evidence; missionCoverage must be sufficient or insufficient. Put explanations in companion fields.`,
		);
	}

	const executionRoot = resolve(options.executionRoot);
	const outputRefs: OutputRef[] = [];
	for (const ref of submission.refs) {
		if (!ref.ref.trim() || !ref.summary.trim()) throw new Error("worker output refs require a ref and summary");
		if (ref.kind === "source") {
			if (!SOURCE_REF_PATTERN.test(ref.ref)) throw new Error(`unsupported source ref: ${ref.ref}`);
			outputRefs.push({ kind: "source", ref: ref.ref, summary: ref.summary, sha256: ref.sha256 });
			continue;
		}
		if (ref.kind === "session") {
			outputRefs.push({ kind: "session", ref: ref.ref, summary: ref.summary, sha256: ref.sha256 });
			continue;
		}
		if (ref.kind !== "artifact" && ref.kind !== "log") throw new Error(`unsupported output ref kind: ${ref.kind}`);
		const fileRef =
			ref.kind === "artifact" &&
			packet.inputArtifactRefs.includes(ref.ref) &&
			packet.requiredCanonicalArtifacts.includes(ref.ref)
				? `canonical/${ref.ref}.json`
				: ref.ref;
		const target = resolve(executionRoot, fileRef);
		if (!isInside(executionRoot, target)) throw new Error(`output ref is outside the task workspace: ${ref.ref}`);
		const metadata = await lstat(target);
		if (metadata.isSymbolicLink()) throw new Error(`output ref may not use symbolic links: ${ref.ref}`);
		if (!isInside(await realpath(executionRoot), await realpath(target)))
			throw new Error(`output ref is outside the task workspace through a symbolic link: ${ref.ref}`);
		if (!metadata.isFile()) throw new Error(`output ref is not a file: ${ref.ref}`);
		if (metadata.size > MAX_REFERENCED_FILE_BYTES) throw new Error(`output ref exceeds 32 MiB: ${ref.ref}`);
		const contentBytes = await readFile(target);
		const normalized = relative(executionRoot, target).split("\\").join("/");
		outputRefs.push({ kind: ref.kind, ref: normalized, summary: ref.summary, sha256: sha256(contentBytes) });
	}
	const requiredArtifactField =
		packet.requiredOutputType === "paper-write"
			? "manuscript"
			: packet.requiredOutputType === "paper-compile"
				? "artifact"
				: undefined;
	if (requiredArtifactField) {
		const artifactPath = content[requiredArtifactField];
		if (typeof artifactPath !== "string" || !artifactPath.trim()) {
			throw new Error(`${packet.requiredOutputType} ${requiredArtifactField} must be a relative file path`);
		}
		if (isAbsolute(artifactPath)) {
			throw new Error(`${packet.requiredOutputType} ${requiredArtifactField} must be a relative file path`);
		}
		const artifactTarget = resolve(executionRoot, artifactPath);
		if (!isInside(executionRoot, artifactTarget)) {
			throw new Error(`${packet.requiredOutputType} ${requiredArtifactField} must be inside the task workspace`);
		}
		const normalizedArtifact = relative(executionRoot, artifactTarget).split("\\").join("/");
		if (!outputRefs.some((ref) => ref.kind === "artifact" && ref.ref === normalizedArtifact)) {
			throw new Error(
				`${packet.requiredOutputType} ${requiredArtifactField} path must have a matching artifact ref`,
			);
		}
	}

	const sourceCount = new Set(outputRefs.filter((ref) => ref.kind === "source").map((ref) => ref.ref)).size;
	if (sourceCount < (options.minSourceRefs ?? 0)) {
		throw new Error(`worker output requires at least ${options.minSourceRefs} source refs; received ${sourceCount}`);
	}
	if (!outputRefs.some((ref) => ref.kind === "session")) {
		outputRefs.push({ kind: "session", ref: options.sessionRef, summary: "Pi worker session" });
	}
	return { content: submission.content, outputRefs };
}
