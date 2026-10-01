import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FIXTURE_PDF_SOURCE } from "../src/fixture-pdf.ts";
import { sourceReceiptFilename, writeSourceReceipt } from "../src/literature.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import type { IncrementalRevision, TaskPacket } from "../src/types.ts";
import {
	applyIncrementalRevision,
	incrementalContentHash,
	validateWorkerSubmission,
} from "../src/worker-submission.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

function packet(root: string): TaskPacket {
	return {
		schemaVersion: "astra.task_packet.v1",
		id: "task_submission",
		jobId: "job_submission",
		agentId: "worker_submission",
		stageId: "implement-solution",
		stageExecutionId: "stage_exec_implementation",
		role: "worker",
		runnerKind: "pi-session",
		objective: "implement and test the method",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "implement-solution",
		requiredOutputFields: ["implementation", "files", "tests", "commands", "limitations"],
		acceptanceChecks: ["implementation is runnable"],
		failureSignals: ["missing test evidence"],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read", "write", "edit"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 60_000 },
		outputManifestRequired: true,
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["implementation is runnable"],
		replayKey: "submission",
		attempt: 1,
		status: "running",
		createdAt: new Date().toISOString(),
	};
}

describe("worker submission validation", () => {
	it("merges a declared literature increment onto its exact same-stage repair base", () => {
		const task = {
			...packet("/workspace"),
			stageId: "literature",
			requiredOutputType: "literature:local",
			repairOfEvidenceId: "evidence_base",
			inputArtifactRefs: ["evidence_base"],
			repairChecks: [{ issueId: "issue_base", criterion: "implementation is runnable" }],
		};
		const base = {
			id: "evidence_base",
			stageId: "literature",
			currentEvidenceSetId: "series_1",
			content: { queryStrategy: { limitations: "search only" }, sources: [] },
		};
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set" as const,
					path: ["queryStrategy", "limitations"],
					value: "search only; page capture unavailable",
					issueId: "issue_base",
					sourceRefs: [],
					reason: "Clarify the retrieval limitation.",
				},
			],
			affectedCriteria: [task.acceptanceChecks[0]!],
			rationale: "Record why the source page could not be captured.",
		};
		expect(applyIncrementalRevision(task, base, {}, revision)).toMatchObject({
			content: { queryStrategy: { limitations: "search only; page capture unavailable" }, sources: [] },
			metadata: { baseEvidenceId: base.id, affectedCriteria: revision.affectedCriteria },
		});
	});

	it.each(["wrong base hash", "undeclared base", "prototype path", "overlapping operations"])(
		"rejects an incremental revision with %s",
		(name) => {
			const task = {
				...packet("/workspace"),
				stageId: "literature",
				requiredOutputType: "literature:local",
				repairOfEvidenceId: "evidence_base",
				inputArtifactRefs: ["evidence_base"],
				repairChecks: [{ issueId: "issue_base", criterion: "implementation is runnable" }],
			};
			const base = {
				id: "evidence_base",
				stageId: "literature",
				currentEvidenceSetId: "series_1",
				content: { queryStrategy: { limitations: "search only" }, sources: [] },
			};
			const revision: Omit<IncrementalRevision, "resultHash"> = {
				baseEvidenceId: base.id,
				baseHash: incrementalContentHash(base.content),
				operations: [
					{
						op: "set",
						path: ["queryStrategy", "limitations"],
						value: "changed",
						issueId: "issue_base",
						sourceRefs: [],
						reason: "Clarify the retrieval limitation.",
					},
				],
				affectedCriteria: [task.acceptanceChecks[0]!],
				rationale: "Correct the limitation.",
			};
			if (name === "wrong base hash") revision.baseHash = "0".repeat(64);
			if (name === "undeclared base") revision.baseEvidenceId = "evidence_other";
			if (name === "prototype path") revision.operations[0]!.path = ["__proto__", "polluted"];
			if (name === "overlapping operations")
				revision.operations.push(
					{
						op: "set",
						path: ["queryStrategy"],
						value: {},
						issueId: "issue_base",
						sourceRefs: [],
						reason: "preserve fields",
					},
					{
						op: "set",
						path: ["queryStrategy", "limitations"],
						value: "changed",
						issueId: "issue_base",
						sourceRefs: [],
						reason: "preserve fields",
					},
				);
			expect(() => applyIncrementalRevision(task, base, {}, revision)).toThrow();
		},
	);

	it("rejects implicit deletions from object or array replacement", () => {
		const task = {
			...packet("/workspace"),
			stageId: "literature",
			requiredOutputType: "literature:local",
			repairOfEvidenceId: "evidence_base",
			inputArtifactRefs: ["evidence_base"],
			repairChecks: [{ issueId: "issue_base", criterion: "implementation is runnable" }],
		};
		const base = {
			id: "evidence_base",
			stageId: "literature",
			currentEvidenceSetId: "series_1",
			content: { queryStrategy: { limitations: "search only", queries: ["q1", "q2"] } },
		};
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set" as const,
					path: ["queryStrategy"],
					value: { limitations: "changed", queries: ["q1"] },
					issueId: "issue_base",
					sourceRefs: [],
					reason: "Do not drop either query.",
				},
			],
			affectedCriteria: [task.acceptanceChecks[0]!],
			rationale: "Do not drop the previous query.",
		};
		expect(() => applyIncrementalRevision(task, base, {}, revision)).toThrow(/delete|remove|preserve/i);
	});

	it("updates array entries only through an exact unique sourceRef selector", () => {
		const task = {
			...packet("/workspace"),
			stageId: "literature",
			requiredOutputType: "literature:local",
			repairOfEvidenceId: "evidence_base",
			inputArtifactRefs: ["evidence_base"],
			repairChecks: [{ issueId: "issue_base", criterion: "implementation is runnable" }],
		};
		const sourceRef = "doi:10.1234/source";
		const base = {
			id: "evidence_base",
			stageId: "literature",
			refs: [sourceRef],
			content: { sources: [{ sourceRef, limitations: "snippet only" }] },
		};
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set" as const,
					path: ["sources", `@sourceRef:${sourceRef}`, "limitations"],
					value: "snippet only; full text unavailable",
					issueId: "issue_base",
					sourceRefs: [sourceRef],
					reason: "Clarify evidence limits.",
				},
			],
			affectedCriteria: [task.acceptanceChecks[0]!],
			rationale: "Retain the existing source row while clarifying its evidence limit.",
		};
		expect(applyIncrementalRevision(task, base, {}, revision).content.sources).toEqual([
			{ sourceRef, limitations: "snippet only; full text unavailable" },
		]);
		const duplicateBase = { ...base, content: { sources: [{ sourceRef }, { sourceRef }] } };
		expect(() =>
			applyIncrementalRevision(
				task,
				duplicateBase,
				{},
				{ ...revision, baseHash: incrementalContentHash(duplicateBase.content) },
			),
		).toThrow(/exactly one/);
		const renamedRevision = {
			...revision,
			operations: [
				{
					...revision.operations[0]!,
					path: ["sources", `@sourceRef:${sourceRef}`],
					value: { sourceRef: "doi:10.1234/renamed", limitations: "changed" },
				},
			],
		};
		expect(() => applyIncrementalRevision(task, base, {}, renamedRevision)).toThrow(/delete or replace/);
	});

	it("appends multiple unique literature rows by explicit stable ID", () => {
		const task = {
			...packet("/workspace"),
			stageId: "literature",
			requiredOutputType: "literature:local",
			repairOfEvidenceId: "evidence_base",
			inputArtifactRefs: ["evidence_base"],
			repairChecks: [{ issueId: "issue_base", criterion: "implementation is runnable" }],
		};
		const base = { id: "evidence_base", stageId: "literature", content: { sources: [] } };
		const sourceRefs = ["doi:10.1234/a", "doi:10.1234/b"];
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: sourceRefs.map((sourceRef) => ({
				op: "set" as const,
				path: ["sources", `@append:${sourceRef}`],
				value: { sourceRef, note: "verified source" },
				issueId: "issue_base",
				sourceRefs: [sourceRef],
				reason: "Add the newly verified source row.",
			})),
			affectedCriteria: [task.acceptanceChecks[0]!],
			rationale: "Append new literature rows while preserving the prior inventory.",
		};
		const refs = sourceRefs.map((ref) => ({ kind: "source", ref }));
		expect(applyIncrementalRevision(task, base, {}, revision, refs).content.sources).toEqual(
			sourceRefs.map((sourceRef) => ({ sourceRef, note: "verified source" })),
		);
		const duplicate = {
			...revision,
			operations: [
				{
					op: "set" as const,
					path: ["sources"],
					value: [{ sourceRef: sourceRefs[0] }, { sourceRef: sourceRefs[0] }],
					issueId: "issue_base",
					sourceRefs,
					reason: "This malformed duplicate must be rejected.",
				},
			],
		};
		expect(() => applyIncrementalRevision(task, base, {}, duplicate, refs)).toThrow(/unique/);
	});

	it("validates an increment against the host base and retains receipted base sources", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-incremental-submission-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			workspaceRoot: root,
			objective: "repair literature evidence",
		});
		const sourceRef = "https://example.org/source";
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: job.state.frame.jobId, query: "source", limit: 1 },
			{ sourceRef, title: "Source", authors: [] },
			"fixture",
			new Date().toISOString(),
		);
		const sourceTask = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			objective: "create a repair base",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature:local",
			requiredOutputFields: ["queryStrategy", "sources"],
			acceptanceChecks: ["record limitations"],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		await job.setTaskStatus(sourceTask.id, "succeeded");
		const base = await job.recordEvidence({
			id: "evidence_base",
			taskId: sourceTask.id,
			stageId: "literature",
			type: "literature:local",
			content: {
				queryStrategy: { limitations: "search-only", repairAppendix: { ledgerRows: [{ sourceRef }] } },
				sources: [{ sourceRef }],
			},
			refs: [sourceRef],
		});
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			deliveryKind: "local",
			objective: "repair literature limitations",
			repairOfEvidenceId: base.id,
			inputArtifactRefs: [base.id],
			requiredOutputType: "literature:local",
			requiredOutputFields: ["queryStrategy", "sources"],
			acceptanceChecks: ["record limitations"],
			repairChecks: [{ issueId: "issue_base", criterion: "record limitations" }],
			requiredCanonicalArtifacts: [],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set" as const,
					path: ["queryStrategy", "limitations"],
					value: "search-only; capture unavailable",
					issueId: "issue_base",
					sourceRefs: [sourceRef],
					reason: "Clarify the retrieval limitation.",
				},
			],
			affectedCriteria: task.acceptanceChecks,
			rationale: "The repair clarifies the capture limitation without rewriting the literature inventory.",
		};
		const addedSourceRef = "https://example.org/new-source";
		const undeclaredSourceRevision = {
			...revision,
			operations: [
				{
					op: "set" as const,
					path: ["sources", `@append:${addedSourceRef}`],
					value: { sourceRef: addedSourceRef },
					issueId: "issue_base",
					sourceRefs: [],
					reason: "Try to add an unreferenced source.",
				},
			],
		};
		await expect(
			validateWorkerSubmission(
				task,
				{
					artifactType: task.requiredOutputType,
					content: {},
					refs: [],
					incrementalRevision: undeclaredSourceRevision,
				},
				{ executionRoot: root, sessionRef: "test:incremental", job },
			),
		).rejects.toThrow(/declared in operation sourceRefs/);
		const addedSourceRevision = {
			...undeclaredSourceRevision,
			operations: [{ ...undeclaredSourceRevision.operations[0]!, sourceRefs: [addedSourceRef] }],
		};
		const addedSourceSubmission = {
			artifactType: task.requiredOutputType,
			content: {},
			refs: [{ kind: "source", ref: addedSourceRef, summary: "new source" }],
			incrementalRevision: addedSourceRevision,
		};
		await expect(
			validateWorkerSubmission(task, addedSourceSubmission, {
				executionRoot: root,
				sessionRef: "test:incremental",
				job,
			}),
		).rejects.toThrow(/receipt/);
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: job.state.frame.jobId, query: "new source", limit: 1 },
			{ sourceRef: addedSourceRef, title: "New source", authors: [] },
			"fixture",
			new Date().toISOString(),
		);
		await expect(
			validateWorkerSubmission(task, addedSourceSubmission, {
				executionRoot: root,
				sessionRef: "test:incremental",
				job,
			}),
		).resolves.toMatchObject({ content: { sources: [{ sourceRef }, { sourceRef: addedSourceRef }] } });
		const nestedSourceRef = "https://example.org/nested-source";
		const nestedRevision = {
			...revision,
			operations: [
				{
					op: "set" as const,
					path: ["queryStrategy", "repairAppendix", "ledgerRows", `@append:${nestedSourceRef}`],
					value: { sourceRef: nestedSourceRef },
					issueId: "issue_base",
					sourceRefs: [nestedSourceRef],
					reason: "Add the source row supporting this repair.",
				},
			],
		};
		const nestedSubmission = {
			artifactType: task.requiredOutputType,
			content: {},
			refs: [{ kind: "source", ref: nestedSourceRef, summary: "nested source" }],
			incrementalRevision: nestedRevision,
		};
		await expect(
			validateWorkerSubmission(task, nestedSubmission, { executionRoot: root, sessionRef: "test:incremental", job }),
		).rejects.toThrow(/receipt/);
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: job.state.frame.jobId, query: "nested source", limit: 1 },
			{ sourceRef: nestedSourceRef, title: "Nested source", authors: [] },
			"fixture",
			new Date().toISOString(),
		);
		await expect(
			validateWorkerSubmission(task, nestedSubmission, { executionRoot: root, sessionRef: "test:incremental", job }),
		).resolves.toMatchObject({
			content: {
				queryStrategy: { repairAppendix: { ledgerRows: [{ sourceRef }, { sourceRef: nestedSourceRef }] } },
			},
		});
		const validated = await validateWorkerSubmission(
			task,
			{ artifactType: task.requiredOutputType, content: {}, refs: [], incrementalRevision: revision },
			{ executionRoot: root, sessionRef: "test:incremental", minSourceRefs: 1, job },
		);
		await job.setTaskStatus(task.id, "succeeded");
		const repaired = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: validated.content,
			refs: [...new Set(validated.outputRefs.map((ref) => ref.ref))],
			currentEvidenceSetId: base.currentEvidenceSetId,
			incrementalRevision: validated.incrementalRevision,
		});
		expect(validated.content).toMatchObject({
			queryStrategy: { limitations: "search-only; capture unavailable" },
			sources: [{ sourceRef }],
		});
		expect(validated.outputRefs).toContainEqual(expect.objectContaining({ kind: "source", ref: sourceRef }));
		expect(validated.incrementalRevision?.baseHash).toBe(revision.baseHash);
		expect(repaired.refs).toContain(sourceRef);
		expect(repaired.incrementalRevision?.resultHash).toBe(incrementalContentHash(repaired.content));
		const reopened = await ResearchJob.open(store, job.state.frame.jobId);
		expect(reopened?.state.evidence[repaired.id]?.incrementalRevision).toEqual(repaired.incrementalRevision);
	});

	it("finds undeclared named source arrays nested in array objects", () => {
		const task = {
			...packet("/workspace"),
			stageId: "literature",
			requiredOutputType: "literature:local",
			repairOfEvidenceId: "evidence_nested_base",
			inputArtifactRefs: ["evidence_nested_base"],
			repairChecks: [{ issueId: "issue_nested", criterion: "record limitations" }],
		};
		const base = {
			id: "evidence_nested_base",
			stageId: "literature",
			content: { metadata: { sections: [] } },
		};
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set" as const,
					path: ["metadata"],
					value: { sections: [[{ ledgerRows: [{ sourceRef: "https://example.org/hidden" }] }]] },
					issueId: "issue_nested",
					sourceRefs: [],
					reason: "Attempt to add a nested unreferenced source.",
				},
			],
			affectedCriteria: ["record limitations"],
			rationale: "Test that nested named source collections remain attributable.",
		};
		expect(() => applyIncrementalRevision(task, base, {}, revision)).toThrow(/declared in operation sourceRefs/);
	});

	it("parses compiled PDFs and requires the log, editable source and complete declared local inputs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-paper-preflight-"));
		tempRoots.push(root);
		await writeFile(join(root, "build.mjs"), FIXTURE_PDF_SOURCE);
		const build = await promisify(execFile)(process.execPath, ["build.mjs", "paper.pdf"], { cwd: root });
		await writeFile(join(root, "build.log"), build.stdout);
		const task = { ...packet(root), requiredOutputType: "paper-compile", requiredOutputFields: ["artifact"] };
		const content = {
			artifact: "paper.pdf",
			command: "node build.mjs paper.pdf",
			buildLog: "build.log",
			source: "build.mjs",
			buildInputs: ["build.mjs"],
		};
		const refs = [
			{ kind: "artifact", ref: "paper.pdf", summary: "compiled" },
			{ kind: "artifact", ref: "build.mjs", summary: "editable source" },
			{ kind: "log", ref: "build.log", summary: "build log" },
		];
		const submit = (value: Record<string, unknown> = content) =>
			validateWorkerSubmission(
				task,
				{ artifactType: "paper-compile", content: value, refs },
				{ executionRoot: root, sessionRef: "test:paper" },
			);
		await expect(submit()).resolves.toBeDefined();
		vi.stubEnv("PATH", root);
		await expect(submit()).rejects.toThrow(/requires pdfinfo/);
		vi.unstubAllEnvs();
		await expect(submit({ ...content, command: "" })).rejects.toThrow(/command/);
		await expect(submit({ ...content, buildLog: [] })).rejects.toThrow(/buildLog/);
		await expect(submit({ ...content, source: "absent.tex" })).rejects.toThrow(/source/);
		await expect(submit({ ...content, buildInputs: ["build.mjs", "missing.bib"] })).rejects.toThrow(/buildInputs/);
		await expect(submit({ ...content, buildInputs: [] })).rejects.toThrow(/buildInputs/);
		await writeFile(join(root, "build.log"), "");
		await expect(submit()).rejects.toThrow(/empty/);
		await writeFile(join(root, "build.log"), build.stdout);
		await writeFile(join(root, "paper.pdf"), "%PDF-1.4\nnot an actual PDF\n%%EOF");
		await expect(submit()).rejects.toThrow(/PDF/);
	});
	it.each([
		"unassessed claim",
		{ statement: "missing judgment" },
		{ statement: "conditional", assessment: "supported-if-replicated" },
	])("rejects a result claim without an explicit valid assessment: %j", async (claim) => {
		const task = {
			...packet("/tmp"),
			requiredOutputType: "result-to-claim",
			requiredOutputFields: ["claims", "scientificOutcome", "missionCoverage"],
		};
		await expect(
			validateWorkerSubmission(
				task,
				{
					artifactType: "result-to-claim",
					content: { claims: [claim], scientificOutcome: "supported", missionCoverage: "sufficient" },
					refs: [],
				},
				{ executionRoot: "/tmp", sessionRef: "test:claim" },
			),
		).rejects.toThrow(/claim.*assessment/);
	});
	it("requires a matching intact retrieval receipt for every cited source", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-source-receipt-"));
		tempRoots.push(root);
		const task = { ...packet(root), requiredOutputFields: ["content"] };
		const sourceRef = "doi:10.1234/example";
		const submission = {
			artifactType: task.requiredOutputType,
			content: { content: "bounded review" },
			refs: [{ kind: "source", ref: sourceRef, summary: "metadata" }],
		};
		const options = { executionRoot: root, sessionRef: "pi-session:test", minSourceRefs: 1 };
		await expect(validateWorkerSubmission(task, submission, options)).rejects.toThrow(/receipt/);
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: task.jobId, query: "example", limit: 1 },
			{ sourceRef, title: "Example", authors: [] },
			"fixture",
			new Date().toISOString(),
		);
		await expect(validateWorkerSubmission(task, submission, options)).resolves.toMatchObject({
			content: submission.content,
		});
		await writeFile(
			join(root, ".astra/jobs", task.jobId, "sources", sourceReceiptFilename(sourceRef)!),
			JSON.stringify({ sourceRef, record: { sourceRef, title: "fabricated" }, sha256: "invalid" }),
		);
		await expect(validateWorkerSubmission(task, submission, options)).rejects.toThrow(/receipt/);
	});
	it.each(["result-to-claim", "research-review"])(
		"rejects unparseable outcome labels before reviewing %s",
		async (artifactType) => {
			const root = await mkdtemp(join(tmpdir(), "astra-submission-outcomes-"));
			tempRoots.push(root);
			const task = {
				...packet(root),
				stageId: artifactType,
				requiredOutputType: artifactType,
				requiredOutputFields: ["scientificOutcome", "missionCoverage", "conclusion"],
			};
			const options = { executionRoot: root, sessionRef: "codex-session:test" };
			for (const labels of [
				{ scientificOutcome: "supported: observations from fixed experiments", missionCoverage: "sufficient" },
				{ scientificOutcome: "supported", missionCoverage: "sufficient: experiments complete" },
				{ scientificOutcome: { status: "supported" }, missionCoverage: "sufficient" },
				{ scientificOutcome: "pending", missionCoverage: "sufficient" },
			]) {
				await expect(
					validateWorkerSubmission(
						task,
						{
							artifactType,
							content: { ...labels, conclusion: "Evidence and scope are explained here" },
							refs: [],
						},
						options,
					),
				).rejects.toThrow("machine-readable outcome labels");
			}
			for (const scientificOutcome of [
				"supported",
				"partially-supported",
				"refuted",
				"inconclusive",
				"insufficient-evidence",
			]) {
				const content = {
					scientificOutcome,
					missionCoverage: "insufficient",
					conclusion: "Preserve the observed outcome and uncovered requirements",
				};
				await expect(
					validateWorkerSubmission(task, { artifactType, content, refs: [] }, options),
				).resolves.toMatchObject({ content });
			}
		},
	);
	it("counts distinct literature sources rather than repeated citations", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-duplicate-"));
		tempRoots.push(root);
		await expect(
			validateWorkerSubmission(
				{ ...packet(root), requiredOutputFields: ["content"] },
				{
					artifactType: "implement-solution",
					content: { content: "same source repeated" },
					refs: Array.from({ length: 3 }, () => ({ kind: "source", ref: "openalex:W1", summary: "one paper" })),
				},
				{ executionRoot: root, sessionRef: "codex-session:test", minSourceRefs: 3 },
			),
		).rejects.toThrow("received 1");
	});

	it.skipIf(process.platform === "win32")("rejects files reached through an escaping parent symlink", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-parent-link-"));
		tempRoots.push(root);
		const workspace = join(root, "workspace");
		await mkdir(workspace);
		await writeFile(join(root, "secret.txt"), "private");
		await symlink(root, join(workspace, "escape"));
		await expect(
			validateWorkerSubmission(
				{ ...packet(workspace), requiredOutputFields: ["content"] },
				{
					artifactType: "implement-solution",
					content: { content: "outside file" },
					refs: [{ kind: "artifact", ref: "escape/secret.txt", summary: "invalid" }],
				},
				{ executionRoot: workspace, sessionRef: "codex-session:test" },
			),
		).rejects.toThrow(/outside|symbolic/);
	});
	it("rejects the wrong output type and missing required fields", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-"));
		tempRoots.push(root);
		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "literature",
					content: {},
					refs: [],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("required output type");
		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "implement-solution",
					content: { implementation: "partial" },
					refs: [],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("missing required output fields");
	});

	it("normalizes local artifact refs and computes their checksum", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-files-"));
		tempRoots.push(root);
		await writeFile(join(root, "implementation.ts"), "export const answer = 42;\n", "utf8");
		const validated = await validateWorkerSubmission(
			packet(root),
			{
				artifactType: "implement-solution",
				content: {
					implementation: "implemented",
					files: ["implementation.ts"],
					tests: ["static check"],
					commands: ["node implementation.ts"],
					limitations: [],
				},
				refs: [{ kind: "artifact", ref: "implementation.ts", summary: "implementation source" }],
			},
			{ executionRoot: root, sessionRef: "pi-session:test" },
		);

		expect(validated.outputRefs[0]).toMatchObject({ kind: "artifact", ref: "implementation.ts" });
		expect(validated.outputRefs[0]?.sha256).toMatch(/^[a-f0-9]{64}$/);
		expect(validated.outputRefs.at(-1)).toMatchObject({ kind: "session", ref: "pi-session:test" });
	});

	it("resolves declared canonical IDs to their materialized snapshots without accepting unknown IDs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-canonical-"));
		tempRoots.push(root);
		await mkdir(join(root, "canonical"));
		await writeFile(join(root, "canonical", "artifact_input.json"), '{"content":{"result":42}}\n');
		await writeFile(join(root, "canonical", "artifact_unrelated.json"), '{"content":{"result":0}}\n');
		const task = {
			...packet(root),
			inputArtifactRefs: ["artifact_input", "artifact_missing"],
			requiredCanonicalArtifacts: ["artifact_input", "artifact_missing", "artifact_unrelated"],
			requiredOutputFields: ["content"],
		};
		const submit = (ref: string) =>
			validateWorkerSubmission(
				task,
				{
					artifactType: task.requiredOutputType,
					content: { content: "Uses the declared upstream result" },
					refs: [{ kind: "artifact", ref, summary: "upstream evidence" }],
				},
				{ executionRoot: root, sessionRef: "codex-session:test" },
			);
		const byId = await submit("artifact_input");
		const byPath = await submit("canonical/artifact_input.json");
		expect(byId.outputRefs).toEqual(byPath.outputRefs);
		expect(byId.outputRefs[0]).toMatchObject({ ref: "canonical/artifact_input.json", sha256: expect.any(String) });
		await expect(submit("artifact_unrelated")).rejects.toThrow("ENOENT");
		await expect(submit("artifact_missing")).rejects.toThrow("ENOENT");
	});

	it("requires paper manuscript paths to have a matching artifact ref", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-manuscript-"));
		tempRoots.push(root);
		await writeFile(join(root, "paper-manuscript.md"), "# Evidence-bound paper\n", "utf8");
		const paperPacket: TaskPacket = {
			...packet(root),
			stageId: "paper-write",
			requiredOutputType: "paper-write",
			requiredOutputFields: ["manuscript", "sections", "citations", "claimBindings", "limitations"],
		};
		const submission = {
			artifactType: "paper-write",
			content: {
				manuscript: "paper-manuscript.md",
				sections: [],
				citations: [],
				claimBindings: [],
				limitations: [],
			},
			refs: [],
		};

		await expect(
			validateWorkerSubmission(paperPacket, submission, {
				executionRoot: root,
				sessionRef: "pi-session:test",
			}),
		).rejects.toThrow("manuscript path must have a matching artifact ref");

		await expect(
			validateWorkerSubmission(
				paperPacket,
				{
					...submission,
					refs: [{ kind: "artifact", ref: "paper-manuscript.md", summary: "complete manuscript" }],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).resolves.toMatchObject({
			outputRefs: [
				{ kind: "artifact", ref: "paper-manuscript.md" },
				{ kind: "session", ref: "pi-session:test" },
			],
		});
	});

	it("requires compiled paper paths to have a matching artifact ref", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-compiled-paper-"));
		tempRoots.push(root);
		await writeFile(join(root, "paper-manuscript.md"), "# Compiled paper\n", "utf8");
		await expect(
			validateWorkerSubmission(
				{
					...packet(root),
					stageId: "paper-compile",
					requiredOutputType: "paper-compile",
					requiredOutputFields: ["artifact", "command", "buildLog", "validation", "remainingWarnings"],
				},
				{
					artifactType: "paper-compile",
					content: {
						artifact: "paper-manuscript.md",
						command: "compile",
						buildLog: [],
						validation: { status: "passed" },
						remainingWarnings: [],
					},
					refs: [],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("paper-compile artifact path must have a matching artifact ref");
	});

	it("adds the authoritative session ref when structured output has no external refs", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-session-only-"));
		tempRoots.push(root);
		const validated = await validateWorkerSubmission(
			packet(root),
			{
				artifactType: "implement-solution",
				content: {
					implementation: "structured result",
					files: [],
					tests: [],
					commands: [],
					limitations: ["no files required by this test"],
				},
				refs: [],
			},
			{ executionRoot: root, sessionRef: "pi-session:test" },
		);

		expect(validated.outputRefs).toEqual([{ kind: "session", ref: "pi-session:test", summary: "Pi worker session" }]);
	});

	it("rejects artifact paths that escape the isolated task workspace", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-escape-"));
		tempRoots.push(root);
		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "implement-solution",
					content: {
						implementation: "invalid",
						files: ["../secret"],
						tests: [],
						commands: [],
						limitations: [],
					},
					refs: [{ kind: "artifact", ref: "../secret", summary: "escaped path" }],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("outside the task workspace");
	});

	it.skipIf(process.platform === "win32")("rejects artifact symlinks that escape the workspace", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-submission-symlink-"));
		tempRoots.push(root);
		const outside = join(root, "..", `${root.split("/").at(-1)}-secret.txt`);
		await writeFile(outside, "secret\n", "utf8");
		tempRoots.push(outside);
		await symlink(outside, join(root, "result.txt"));

		await expect(
			validateWorkerSubmission(
				packet(root),
				{
					artifactType: "implement-solution",
					content: {
						implementation: "invalid",
						files: ["result.txt"],
						tests: [],
						commands: [],
						limitations: [],
					},
					refs: [{ kind: "artifact", ref: "result.txt", summary: "symlinked output" }],
				},
				{ executionRoot: root, sessionRef: "pi-session:test" },
			),
		).rejects.toThrow("symbolic links");
	});
});
