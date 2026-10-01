import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import { readSourceRecord } from "./literature.ts";
import { validatePaperDelivery } from "./paper-delivery.ts";
import type { ResearchJob } from "./research.ts";
import { scientificAssessment, validateClaimAssessments } from "./research.ts";
import type { IncrementalRevision, OutputRef, TaskPacket } from "./types.ts";

const MAX_REFERENCED_FILE_BYTES = 32 * 1024 * 1024;
const SOURCE_REF_PATTERN = /^(?:https:\/\/|openalex:|doi:|arxiv:)/;

export interface WorkerSubmission {
	artifactType: string;
	content: unknown;
	refs: Array<{ kind: string; ref: string; summary: string; sha256?: string }>;
	incrementalRevision?: Omit<IncrementalRevision, "resultHash">;
}

export interface IncrementalBaseEvidence {
	id: string;
	stageId: string;
	currentEvidenceSetId?: string;
	content: unknown;
	refs?: string[];
}

export function incrementalContentHash(content: unknown): string {
	const stable = (value: unknown): string => {
		if (Array.isArray(value)) return `[${value.map((entry) => stable(entry)).join(",")}]`;
		if (value && typeof value === "object")
			return `{${Object.entries(value as Record<string, unknown>)
				.sort(([left], [right]) => left.localeCompare(right))
				.map(([key, entry]) => `${JSON.stringify(key)}:${stable(entry)}`)
				.join(",")}}`;
		return JSON.stringify(value) ?? "null";
	};
	return createHash("sha256").update(stable(content)).digest("hex");
}

function preservesExistingValue(before: unknown, after: unknown): boolean {
	if (Array.isArray(before))
		return (
			Array.isArray(after) &&
			before.length <= after.length &&
			before.every((value, index) => incrementalContentHash(value) === incrementalContentHash(after[index]))
		);
	if (before && typeof before === "object") {
		if (!after || typeof after !== "object" || Array.isArray(after)) return false;
		if (
			Object.hasOwn(before, "sourceRef") &&
			(before as Record<string, unknown>).sourceRef !== (after as Record<string, unknown>).sourceRef
		)
			return false;
		return Object.entries(before as Record<string, unknown>).every(
			([key, value]) =>
				Object.hasOwn(after, key) && preservesExistingValue(value, (after as Record<string, unknown>)[key]),
		);
	}
	return true;
}

function arraySelectorIndex(array: unknown[], selector: string): number {
	if (!selector.startsWith("@sourceRef:"))
		throw new Error("array paths require an explicit sourceRef stable-key selector");
	const sourceRef = selector.slice("@sourceRef:".length);
	const matches = array.flatMap((value, index) =>
		value &&
		typeof value === "object" &&
		!Array.isArray(value) &&
		(value as Record<string, unknown>).sourceRef === sourceRef
			? [index]
			: [],
	);
	if (matches.length !== 1) throw new Error("array stable-key selector must match exactly one sourceRef");
	return matches[0]!;
}

function childAt(container: Record<string, unknown> | unknown[], part: string): unknown {
	if (Array.isArray(container)) return container[arraySelectorIndex(container, part)];
	if (part.startsWith("@sourceRef:")) throw new Error("stable-key selector can only address an array item");
	if (!Object.hasOwn(container, part)) throw new Error("incremental path does not exist");
	return container[part];
}

function namedSourceArrays(value: unknown, path: string[] = []): Array<{ path: string[]; values: unknown[] }> {
	if (Array.isArray(value))
		return value.flatMap((entry, index) =>
			entry && typeof entry === "object" ? namedSourceArrays(entry, [...path, `@index:${index}`]) : [],
		);
	if (!value || typeof value !== "object") return [];
	return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) => {
		const childPath = [...path, key];
		const current = key === "sources" || key === "ledgerRows";
		return [
			...(current && Array.isArray(child) ? [{ path: childPath, values: child }] : []),
			...namedSourceArrays(child, childPath),
		];
	});
}

export function applyIncrementalRevision(
	packet: TaskPacket,
	base: IncrementalBaseEvidence,
	content: unknown,
	revision: Omit<IncrementalRevision, "resultHash">,
	submissionRefs: Array<{ kind: string; ref: string }> = [],
): { content: Record<string, unknown>; metadata: IncrementalRevision } {
	if (packet.stageId !== "literature" || !packet.requiredOutputType.startsWith("literature"))
		throw new Error("incremental revisions are supported only for literature deliveries");
	if (base.id !== packet.repairOfEvidenceId || !packet.inputArtifactRefs.includes(base.id))
		throw new Error("incremental revision base must be the declared repair input");
	if (revision.baseEvidenceId !== base.id) throw new Error("incremental revision base evidence id does not match");
	if (base.stageId !== packet.stageId) throw new Error("incremental revision base must belong to the same stage");
	if (!revision.rationale.trim() || revision.operations.length === 0)
		throw new Error("incremental revision requires operations and a rationale");
	if (!content || typeof content !== "object" || Array.isArray(content) || Object.keys(content).length !== 0)
		throw new Error("incremental submissions must use operations without replacement content");
	if (incrementalContentHash(base.content) !== revision.baseHash)
		throw new Error("incremental revision base hash does not match the declared evidence");
	if (
		!revision.affectedCriteria.length ||
		revision.affectedCriteria.some(
			(criterion) =>
				![
					...packet.acceptanceChecks,
					...packet.successCriteria,
					...(packet.repairChecks ?? []).map((check) => check.criterion),
				].includes(criterion),
		)
	)
		throw new Error("incremental affected criteria must name declared task criteria");
	if (!Array.isArray(base.content) && base.content && typeof base.content === "object") {
		const candidate = structuredClone(base.content) as Record<string, unknown>;
		const paths: string[][] = [];
		for (const operation of revision.operations) {
			if (
				!Array.isArray(operation.path) ||
				operation.path.length === 0 ||
				operation.path.some(
					(part) => typeof part !== "string" || !part || ["__proto__", "prototype", "constructor"].includes(part),
				)
			)
				throw new Error("incremental operation path is invalid or unsafe");
			if (
				paths.some(
					(path) =>
						path.every((part, index) => operation.path[index] === part) ||
						operation.path.every((part, index) => path[index] === part),
				)
			)
				throw new Error("incremental operations may not overlap");
			paths.push(operation.path);
			if (!Array.isArray(operation.sourceRefs) || !operation.reason?.trim())
				throw new Error("each incremental operation requires sourceRefs and a reason");
			if (!operation.issueId && operation.sourceRefs.length === 0)
				throw new Error("each incremental operation must bind an issue or source reference");
			if (operation.issueId && !(packet.repairChecks ?? []).some((check) => check.issueId === operation.issueId))
				throw new Error("incremental operation issueId is not bound to the repair task");
			if (
				operation.issueId &&
				!(packet.repairChecks ?? [])
					.filter((check) => check.issueId === operation.issueId)
					.some((check) => revision.affectedCriteria.includes(check.criterion))
			)
				throw new Error("incremental affected criteria do not include the bound issue criterion");
			const availableSourceRefs = new Set([
				...(base.refs ?? []).filter((ref) => SOURCE_REF_PATTERN.test(ref)),
				...submissionRefs.filter((ref) => ref.kind === "source").map((ref) => ref.ref),
			]);
			if (operation.sourceRefs.some((ref) => !availableSourceRefs.has(ref)))
				throw new Error("incremental operation names a source not declared by the base or submission");
			let parent: Record<string, unknown> | unknown[] = candidate;
			for (const part of operation.path.slice(0, -1)) {
				const value = childAt(parent, part);
				if (!value || typeof value !== "object" || Array.isArray(value))
					if (Array.isArray(value)) parent = value;
					else
						throw new Error("incremental paths must resolve through declared object fields or stable-key arrays");
				else parent = value as Record<string, unknown>;
			}
			const leaf = operation.path.at(-1)!;
			const leafIsArraySelector = Array.isArray(parent);
			if (!leafIsArraySelector && leaf.startsWith("@sourceRef:"))
				throw new Error("stable-key selector can only address an array item");
			if (operation.path.includes("sourceRef") && operation.op === "set")
				throw new Error("incremental revisions may not change a sourceRef stable identity");
			const isAppend = leaf === "@append" || leaf.startsWith("@append:");
			const appendIdentity = leaf.startsWith("@append:") ? leaf.slice("@append:".length) : undefined;
			const leafKey = leafIsArraySelector && !isAppend ? arraySelectorIndex(parent as unknown[], leaf) : leaf;
			if (operation.op === "delete") {
				if (leafIsArraySelector && isAppend) throw new Error("incremental append cannot delete an array item");
				if (leafIsArraySelector) (parent as unknown[]).splice(leafKey as number, 1);
				else {
					if (!Object.hasOwn(parent, leafKey)) throw new Error("incremental delete path does not exist");
					delete (parent as Record<string, unknown>)[leafKey as string];
				}
			} else if (operation.op === "set") {
				if (!Object.hasOwn(operation, "value")) throw new Error("incremental set operation requires a value");
				if (leafIsArraySelector && isAppend) {
					if (!(operation.path.at(-2) === "sources" || operation.path.at(-2) === "ledgerRows"))
						throw new Error("stable-key array append is limited to sources and ledgerRows");
					if (
						!operation.value ||
						typeof operation.value !== "object" ||
						Array.isArray(operation.value) ||
						typeof (operation.value as Record<string, unknown>).sourceRef !== "string"
					)
						throw new Error("stable-key array append requires an object with sourceRef");
					if (appendIdentity && (operation.value as Record<string, unknown>).sourceRef !== appendIdentity)
						throw new Error("stable-key array append identity does not match its path");
					(parent as unknown[]).push(structuredClone(operation.value));
					continue;
				}
				const exists = leafIsArraySelector && !isAppend ? true : Object.hasOwn(parent, leafKey);
				const currentValue = leafIsArraySelector
					? (parent as unknown[])[leafKey as number]
					: (parent as Record<string, unknown>)[leafKey as string];
				if (exists && !preservesExistingValue(currentValue, operation.value))
					throw new Error("incremental set would implicitly delete or replace existing object or array content");
				if (leafIsArraySelector) (parent as unknown[])[leafKey as number] = structuredClone(operation.value);
				else (parent as Record<string, unknown>)[leafKey as string] = structuredClone(operation.value);
			} else {
				throw new Error("incremental operation must be set or delete");
			}
		}
		const baseArrays = new Map(
			namedSourceArrays(base.content).map(({ path, values }) => [JSON.stringify(path), values]),
		);
		const operationSourceRefs = new Set(revision.operations.flatMap((operation) => operation.sourceRefs));
		for (const { path, values } of namedSourceArrays(candidate)) {
			const key = path.at(-1)!;
			const ids = values.map((value) =>
				value && typeof value === "object" && !Array.isArray(value)
					? (value as Record<string, unknown>).sourceRef
					: undefined,
			);
			if (ids.some((id) => typeof id !== "string" || !id) || new Set(ids).size !== ids.length)
				throw new Error(`${key} stable sourceRef values must be present and unique`);
			const prior = baseArrays.get(JSON.stringify(path));
			const priorIds = Array.isArray(prior)
				? prior.flatMap((value) =>
						value &&
						typeof value === "object" &&
						!Array.isArray(value) &&
						typeof (value as Record<string, unknown>).sourceRef === "string"
							? [(value as Record<string, unknown>).sourceRef as string]
							: [],
					)
				: [];
			const declaredSourceRefs = new Set([
				...(base.refs ?? []).filter((ref) => SOURCE_REF_PATTERN.test(ref)),
				...submissionRefs.filter((ref) => ref.kind === "source").map((ref) => ref.ref),
			]);
			for (const sourceRef of ids as string[]) {
				if (
					!priorIds.includes(sourceRef) &&
					(!declaredSourceRefs.has(sourceRef) || !operationSourceRefs.has(sourceRef))
				)
					throw new Error(`${key} new sourceRef must be declared in operation sourceRefs and submission refs`);
			}
		}
		const resultHash = incrementalContentHash(candidate);
		return {
			content: candidate,
			metadata: { ...structuredClone(revision), resultHash },
		};
	}
	throw new Error("incremental base content must be a structured object");
}

export interface WorkerSubmissionOptions {
	executionRoot: string;
	sessionRef: string;
	minSourceRefs?: number;
	job?: ResearchJob;
}

function declaredIncrementalBase(
	job: ResearchJob,
	packet: TaskPacket,
	revision: WorkerSubmission["incrementalRevision"],
): IncrementalBaseEvidence {
	if (!revision) throw new Error("incremental revision is missing");
	const base = job.state.evidence[revision.baseEvidenceId];
	if (!base) throw new Error("incremental revision base evidence is unavailable");
	if (base.id !== packet.repairOfEvidenceId || !packet.inputArtifactRefs.includes(base.id))
		throw new Error("incremental revision base must be the declared repair input");
	if (base.stageId !== packet.stageId) throw new Error("incremental revision base must belong to the same stage");
	return {
		id: base.id,
		stageId: base.stageId,
		currentEvidenceSetId: base.currentEvidenceSetId,
		content: base.content,
		refs: base.refs,
	};
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
	options: WorkerSubmissionOptions & { incrementalBase?: IncrementalBaseEvidence },
): Promise<{ content: unknown; outputRefs: OutputRef[]; incrementalRevision?: IncrementalRevision }> {
	const incrementalBase = submission.incrementalRevision
		? options.job
			? declaredIncrementalBase(options.job, packet, submission.incrementalRevision)
			: undefined
		: undefined;
	const applied = submission.incrementalRevision
		? applyIncrementalRevision(
				packet,
				incrementalBase ?? { id: "", stageId: "", content: {} },
				submission.content,
				submission.incrementalRevision,
				submission.refs,
			)
		: undefined;
	const submittedContent = applied?.content ?? submission.content;
	if (submission.artifactType !== packet.requiredOutputType) {
		throw new Error(
			`worker artifact type ${submission.artifactType} does not match required output type ${packet.requiredOutputType}`,
		);
	}
	if (submittedContent === null || typeof submittedContent !== "object" || Array.isArray(submittedContent)) {
		throw new Error("worker content must be a structured object");
	}
	const content = submittedContent as Record<string, unknown>;
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
	if (packet.requiredOutputType === "result-to-claim") validateClaimAssessments(content);
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

	if (packet.requiredOutputType === "paper-compile") await validatePaperDelivery(content, outputRefs, executionRoot);
	if (incrementalBase?.refs) {
		for (const ref of incrementalBase.refs.filter((ref) => SOURCE_REF_PATTERN.test(ref))) {
			if (!outputRefs.some((outputRef) => outputRef.kind === "source" && outputRef.ref === ref))
				outputRefs.push({ kind: "source", ref, summary: "Retained from the declared repair base" });
		}
	}
	const sourceCount = new Set(outputRefs.filter((ref) => ref.kind === "source").map((ref) => ref.ref)).size;
	if (sourceCount < (options.minSourceRefs ?? 0)) {
		throw new Error(`worker output requires at least ${options.minSourceRefs} source refs; received ${sourceCount}`);
	}
	for (const ref of outputRefs.filter((ref) => ref.kind === "source")) {
		if (!(await readSourceRecord(packet.scope.workspaceRoot, packet.jobId, ref.ref)))
			throw new Error(`source requires an intact retrieval receipt: ${ref.ref}`);
	}
	if (applied) {
		for (const { path, values } of namedSourceArrays(submittedContent)) {
			const key = path.at(-1)!;
			for (const value of values) {
				const sourceRef = (value as Record<string, unknown>).sourceRef as string;
				if (!(await readSourceRecord(packet.scope.workspaceRoot, packet.jobId, sourceRef)))
					throw new Error(`incremental ${key} source requires an intact retrieval receipt: ${sourceRef}`);
			}
		}
	}
	if (!outputRefs.some((ref) => ref.kind === "session")) {
		outputRefs.push({ kind: "session", ref: options.sessionRef, summary: "Pi worker session" });
	}
	return {
		content: submittedContent,
		outputRefs,
		...(applied ? { incrementalRevision: applied.metadata } : {}),
	};
}
