import { createHash } from "node:crypto";
import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { writeWorkerOutputManifest } from "../src/contracts.ts";
import { sourceReceiptFilename, writeSourceReceipt } from "../src/literature.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { prepareTaskWorkspace, readVersionedFile } from "../src/task-workspace.ts";
import { incrementalContentHash, validateWorkerSubmission } from "../src/worker-submission.ts";
import { lifecycleScenario } from "./lifecycle-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function scenario() {
	const root = await mkdtemp(join(tmpdir(), "astra-source-version-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "bind exact offline source observations",
		automation: "full",
		definitions: [
			{
				id: "literature",
				label: "Literature",
				suggestedInputArtifactTypes: [],
				outputArtifactType: "literature",
				requiredOutputFields: ["sources", "note"],
				acceptanceChecks: ["record limitations"],
				failureSignals: ["missing source"],
				workerTaskFamily: "literature",
				workerTools: ["read"],
				workspaceWrite: false,
				minSourceRefs: 1,
				gate: "main-agent",
			},
		],
	});
	const task = await job.dispatchTask({
		stageId: "literature",
		stageExecutionId: "literature",
		role: "worker",
		objective: "retain source observation",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "literature",
		requiredOutputFields: ["sources", "note"],
		acceptanceChecks: ["record limitations"],
		successCriteria: ["record limitations"],
		failureSignals: ["missing source"],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	const cwd = await prepareTaskWorkspace(task, job);
	const ref = "doi:10.1000/lifecycle";
	const options = { workspaceRoot: root, jobId: task.jobId, query: "original observation", limit: 1 };
	await writeSourceReceipt(
		options,
		{
			sourceRef: ref,
			title: "Original source",
			authors: ["Author"],
			abstract: "Complete recorded abstract",
			type: "article",
			retrievalLevel: "source-page",
		},
		"offline",
		"2026-10-02T00:00:00Z",
		{ retainedMetadata: { license: "unknown", scope: "metadata, not full text" } },
	);
	const receiptPath = join(root, ".astra", "jobs", task.jobId, "sources", sourceReceiptFilename(ref)!);
	const original = await readFile(receiptPath);
	const submission = {
		artifactType: task.requiredOutputType,
		content: { sources: [{ sourceRef: ref }], note: "metadata only" },
		refs: [{ kind: "source", ref, summary: "validated metadata" }],
	};
	const validationOptions = { executionRoot: cwd, sessionRef: "pi-session:offline", minSourceRefs: 1, job };
	const validate = () => validateWorkerSubmission(job.state.tasks[task.id], submission, validationOptions);
	const manifest = async () => {
		const output = await validate();
		await writeWorkerOutputManifest(
			{
				schemaVersion: "astra.worker_output_manifest.v1",
				manifestId: "source_manifest",
				jobId: task.jobId,
				taskId: task.id,
				agentId: task.agentId,
				status: "completed",
				artifactType: task.requiredOutputType,
				content: output.content,
				outputRefs: output.outputRefs,
				validationStatus: "passed",
				validationErrors: [],
				sessionRef: "pi-session:offline",
				createdAt: new Date().toISOString(),
			},
			root,
		);
		return output;
	};
	return {
		root,
		store,
		job,
		task,
		ref,
		original,
		options,
		receiptPath,
		submission,
		validationOptions,
		validate,
		manifest,
	};
}

it("S01 binds the same complete validated source bytes and refuses a model-provided false digest", async () => {
	const f = await scenario();
	const output = await f.validate();
	const expected = createHash("sha256").update(f.original).digest("hex");
	expect(output.outputRefs.find((ref) => ref.kind === "source")?.sha256).toBe(expected);
	expect(await readFile(join(f.root, ".astra", "jobs", f.task.jobId, "versions", "files", expected))).toEqual(
		f.original,
	);
	await expect(
		validateWorkerSubmission(
			f.job.state.tasks[f.task.id],
			{
				...f.submission,
				refs: [{ ...f.submission.refs[0], sha256: "f".repeat(64) }],
			},
			f.validationOptions,
		),
	).rejects.toThrow(/digest|hash|sha|snapshot/);
});

it.each(["updated", "deleted"])(
	"S02/S04 preserves validated receipt after cache is %s, in registration and JSONL recovery",
	async (change) => {
		for (const recover of [false, true]) {
			const f = await scenario();
			const output = await f.manifest();
			if (change === "deleted") await rm(f.receiptPath);
			else
				await writeSourceReceipt(
					f.options,
					{ sourceRef: f.ref, title: "Later observation", authors: [] },
					"offline",
					"2026-10-02T00:01:00Z",
				);
			await f.job.setTaskStatus(f.task.id, recover ? "running" : "succeeded");
			const job = (await ResearchJob.open(f.store, f.task.jobId))!;
			const evidence = recover
				? await job.recoverWorkerTaskCompletion(f.task.id)
				: await job.recordEvidence({
						taskId: f.task.id,
						stageId: f.task.stageId,
						type: f.task.requiredOutputType,
						content: output.content,
						refs: output.outputRefs.map((ref) => ref.ref),
					});
			expect(evidence).toBeDefined();
			expect(
				await readVersionedFile(
					f.task,
					evidence!,
					f.ref,
					f.receiptPath,
					join(f.root, ".astra", "jobs", f.task.jobId, "sources"),
				),
			).toEqual(f.original);
			expect(evidence!.files!.find((file) => file.sourceRef === f.ref)?.sha256).toBe(
				createHash("sha256").update(f.original).digest("hex"),
			);
			const reopened = (await ResearchJob.open(f.store, f.task.jobId))!;
			await reopened.recoverPendingOperations();
			expect(Object.values(reopened.state.evidence)).toHaveLength(1);
			expect(reopened.state.evidence[evidence!.id].files).toEqual(evidence!.files);
		}
	},
);

it.each([false, true])(
	"S06/S07 incremental source inheritance versus explicit fresh submission (%s)",
	async (explicit) => {
		const f = await scenario();
		await f.job.setTaskStatus(f.task.id, "succeeded");
		const base = await f.job.recordEvidence({
			taskId: f.task.id,
			stageId: f.task.stageId,
			type: f.task.requiredOutputType,
			content: f.submission.content,
			refs: [f.ref],
		});
		const repair = await f.job.dispatchTask({
			...f.task,
			id: "source_repair",
			replayKey: "source_repair",
			objective: "clarify source limitations",
			repairOfEvidenceId: base.id,
			inputArtifactRefs: [base.id],
		});
		const cwd = await prepareTaskWorkspace(repair, f.job);
		if (explicit)
			await writeSourceReceipt(
				f.options,
				{ sourceRef: f.ref, title: "Explicit incremental observation", authors: [] },
				"offline",
				"2026-10-02T00:03:00Z",
			);
		const expectedBytes = explicit ? await readFile(f.receiptPath) : f.original;
		const expectedSha = createHash("sha256").update(expectedBytes).digest("hex");
		await rm(f.receiptPath);
		if (explicit) await writeFile(f.receiptPath, expectedBytes);
		const output = await validateWorkerSubmission(
			repair,
			{
				artifactType: repair.requiredOutputType,
				content: {},
				refs: explicit ? [{ kind: "source", ref: f.ref, summary: "New explicit observation" }] : [],
				incrementalRevision: {
					baseEvidenceId: base.id,
					baseHash: incrementalContentHash(base.content),
					operations: [
						{
							op: "set",
							path: ["note"],
							value: "metadata only; full text unverified",
							sourceRefs: [f.ref],
							reason: "Clarify evidence scope",
						},
					],
					affectedCriteria: ["record limitations"],
					rationale: "Retain original source observation",
				},
			},
			{ executionRoot: cwd, sessionRef: "pi-session:repair", minSourceRefs: 1, job: f.job },
		);
		expect(output.outputRefs.find((ref) => ref.kind === "source")?.sha256).toBe(expectedSha);
		expect(output.content).toMatchObject({ note: "metadata only; full text unverified" });
		expect(
			await readFile(join(f.root, ".astra", "jobs", repair.jobId, "versions", "files", base.files![0].sha256)),
		).toEqual(f.original);
		await writeWorkerOutputManifest(
			{
				schemaVersion: "astra.worker_output_manifest.v1",
				manifestId: "inherited_source_manifest",
				jobId: repair.jobId,
				taskId: repair.id,
				agentId: repair.agentId,
				status: "completed",
				artifactType: repair.requiredOutputType,
				content: output.content,
				outputRefs: output.outputRefs,
				incrementalRevision: output.incrementalRevision,
				validationStatus: "passed",
				validationErrors: [],
				sessionRef: "pi-session:repair",
				createdAt: new Date().toISOString(),
			},
			f.root,
		);
		await f.job.setTaskStatus(repair.id, "running");
		if (explicit) await rm(f.receiptPath);
		const reopened = (await ResearchJob.open(f.store, repair.jobId))!;
		const evidence = await reopened.recoverWorkerTaskCompletion(repair.id);
		expect(evidence!.files!.find((file) => file.sourceRef === f.ref)?.sha256).toBe(expectedSha);
		expect(await readVersionedFile(repair, evidence!, f.ref, "", "")).toEqual(expectedBytes);
		expect(await readVersionedFile(f.task, base, f.ref, "", "")).toEqual(f.original);
	},
);

it.each(["missing", "corrupt", "identity", "unbound"])(
	"S05 fixed source recovery refuses %s original binding without new evidence",
	async (failure) => {
		const f = await scenario();
		const output = await f.manifest();
		const source = output.outputRefs.find((ref) => ref.kind === "source")!;
		const blob = join(f.root, ".astra", "jobs", f.task.jobId, "versions", "files", source.sha256!);
		if (failure === "missing") await rm(blob);
		else if (failure === "corrupt") {
			await chmod(blob, 0o600);
			await writeFile(blob, "corrupted receipt");
		} else if (failure === "identity") {
			const manifestPath = join(f.root, ".astra", "jobs", f.task.jobId, "tasks", f.task.id, "output-manifest.json");
			const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
			manifest.outputRefs.find((ref: { kind: string }) => ref.kind === "source").ref = "doi:10.1000/foreign";
			await writeFile(manifestPath, JSON.stringify(manifest));
		} else {
			const manifestPath = join(f.root, ".astra", "jobs", f.task.jobId, "tasks", f.task.id, "output-manifest.json");
			const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
			delete manifest.outputRefs.find((ref: { kind: string }) => ref.kind === "source").sha256;
			await writeFile(manifestPath, JSON.stringify(manifest));
		}
		await f.job.setTaskStatus(f.task.id, "running");
		const reopened = (await ResearchJob.open(f.store, f.task.jobId))!;
		const seq = reopened.state.eventSeq;
		await expect(reopened.recoverWorkerTaskCompletion(f.task.id)).rejects.toThrow(
			/receipt|binding|snapshot|digest|integrity|source/,
		);
		expect(reopened.state.eventSeq).toBe(seq);
		expect(Object.values(reopened.state.evidence)).toHaveLength(0);
	},
);

it("S07 explicit source resubmission binds fresh bytes and cannot select the old CAS by model digest", async () => {
	const f = await scenario();
	const original = await f.validate();
	const oldSha = original.outputRefs.find((ref) => ref.kind === "source")!.sha256!;
	await writeSourceReceipt(
		f.options,
		{ sourceRef: f.ref, title: "New explicit observation", authors: [] },
		"offline",
		"2026-10-02T00:02:00Z",
	);
	const fresh = await readFile(f.receiptPath);
	await expect(
		validateWorkerSubmission(
			f.job.state.tasks[f.task.id],
			{ ...f.submission, refs: [{ ...f.submission.refs[0], sha256: oldSha }] },
			f.validationOptions,
		),
	).rejects.toThrow(/digest/);
	const updated = await f.validate();
	expect(updated.outputRefs.find((ref) => ref.kind === "source")!.sha256).toBe(
		createHash("sha256").update(fresh).digest("hex"),
	);
	expect(await readFile(join(f.root, ".astra", "jobs", f.task.jobId, "versions", "files", oldSha))).toEqual(
		f.original,
	);
});

it.each(["pi", "codex"] as const)(
	"S02/S03/S04 %s actual adapter retains original receipt on completion and restart",
	async (backend) => {
		for (const recover of [false, true])
			for (const change of ["updated", "deleted"]) {
				const f = await lifecycleScenario(backend);
				roots.push(f.root);
				const ref = "doi:10.1000/adapter-source";
				const options = { workspaceRoot: f.root, jobId: f.job.state.frame.jobId, query: "original", limit: 1 };
				await writeSourceReceipt(
					options,
					{ sourceRef: ref, title: "Original adapter observation", authors: ["Author"] },
					"offline",
					"2026-10-02T00:00:00Z",
				);
				const path = join(f.root, ".astra", "jobs", options.jobId, "sources", sourceReceiptFilename(ref)!);
				const original = await readFile(path);
				const upstream = await f.task("declare available source");
				await f.job.setTaskStatus(upstream.id, "succeeded");
				const base = await f.job.recordEvidence({
					taskId: upstream.id,
					stageId: upstream.stageId,
					type: upstream.requiredOutputType,
					content: { content: "declared upstream source" },
					refs: [ref],
				});
				const task = await f.task("use declared source", [base.id]);
				f.control.sources = [ref];
				await f.job.setTaskStatus(task.id, "running");
				await f.job.consumeTurns(1);
				const output = await f.worker.run(f.job.state.tasks[task.id], f.job);
				if (change === "deleted") await rm(path);
				else
					await writeSourceReceipt(
						options,
						{ sourceRef: ref, title: "Later adapter observation", authors: [] },
						"offline",
						"2026-10-02T00:01:00Z",
					);
				if (recover) f.job = (await ResearchJob.open(f.store, task.jobId))!;
				const evidence = recover
					? await f.job.recoverWorkerTaskCompletion(task.id)
					: await f.job.completeWorkerTask(task.id, output);
				expect(evidence).toBeDefined();
				expect(
					await readVersionedFile(
						task,
						evidence!,
						ref,
						path,
						join(f.root, ".astra", "jobs", task.jobId, "sources"),
					),
				).toEqual(original);
				f.job = (await ResearchJob.open(f.store, task.jobId))!;
				await f.job.recoverPendingOperations();
				expect(f.calls).toEqual([{ role: "worker", taskId: task.id }]);
				expect(f.job.state.tasks[task.id].status).toBe("succeeded");
				expect(f.job.state.budgetUsage!.turnsUsed).toBe(1);
				expect(Object.values(f.job.state.evidence)).toHaveLength(2);
			}
	},
);
