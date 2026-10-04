import { createHash } from "node:crypto";
import { chmod, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { readJson, workerManifestPath, writeWorkerOutputManifest } from "../src/contracts.ts";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { writeSourceReceipt } from "../src/literature.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { prepareReviewEvidenceBundle, prepareTaskWorkspace, readVersionedFile } from "../src/task-workspace.ts";
import type { WorkerOutputManifest } from "../src/types.ts";
import { startWorkbench } from "../src/workbench.ts";
import { incrementalContentHash, validateWorkerSubmission } from "../src/worker-submission.ts";
import { lifecycleDefinition, lifecycleScenario } from "./lifecycle-fixture.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function fixture(mutation?: "changed" | "deleted", source?: "implicit" | "explicit") {
	const scenario = await lifecycleScenario("pi", {
		...lifecycleDefinition,
		id: "literature",
		outputArtifactType: "literature",
	});
	roots.push(scenario.root);
	const original = await scenario.task("original file delivery");
	const workspace = await prepareTaskWorkspace(original, scenario.job);
	await writeFile(join(workspace, "queries.json"), "original frozen bytes");
	await mkdir(join(workspace, "mirror"));
	await writeFile(join(workspace, "mirror", "queries.json"), "original frozen bytes");
	await writeFile(join(workspace, "trace.log"), "original log bytes");
	const ref = relative(scenario.root, join(workspace, "queries.json")).split("\\").join("/");
	const sourceRef = "https://example.invalid/source";
	if (source)
		await writeSourceReceipt(
			{ workspaceRoot: scenario.root, jobId: original.jobId, query: "offline", limit: 1 },
			{ sourceRef, title: "original", authors: [] },
			"offline",
			new Date().toISOString(),
		);
	const base = await scenario.job.completeWorkerTask(original.id, {
		artifactType: "literature",
		content: {
			content: "original",
			queryStrategy: { limitations: "original" },
			sources: source ? [{ sourceRef }] : [],
		},
		refs: [
			ref,
			ref.replace("queries.json", "mirror/queries.json"),
			ref.replace("queries.json", "trace.log"),
			...(source ? [sourceRef] : []),
		],
	});
	await scenario.job.recordReview(
		reviewFixture(scenario.job, { evidenceId: base.id, verdict: "fail", findings: ["clarify content"] }),
	);
	const obligation = Object.values(scenario.job.state.obligations)[0];
	const {
		id: _id,
		jobId: _jobId,
		agentId: _agentId,
		status: _status,
		createdAt: _createdAt,
		version: _version,
		replayKey: _replayKey,
		...contract
	} = original;
	const repair = await scenario.job.dispatchTask({
		...contract,
		objective: "bounded repair",
		inputArtifactRefs: [base.id],
		repairOfEvidenceId: base.id,
		repairChecks: obligation.items!.map((item) => ({ issueId: item.id, criterion: item.criterion })),
	});
	if (mutation === "changed") await writeFile(join(workspace, "queries.json"), "new unrelated live bytes");
	if (mutation === "deleted") await rm(join(workspace, "queries.json"));
	const repairWorkspace = await prepareTaskWorkspace(repair, scenario.job);
	if (source)
		await writeSourceReceipt(
			{ workspaceRoot: scenario.root, jobId: original.jobId, query: "offline refresh", limit: 1 },
			{ sourceRef, title: "refreshed", authors: [] },
			"offline",
			new Date().toISOString(),
		);
	await writeFile(join(repairWorkspace, "queries.json"), "new repair bytes");
	const validated = await validateWorkerSubmission(
		scenario.job.state.tasks[repair.id],
		{
			artifactType: "literature",
			content: {},
			refs: [
				{ kind: "artifact", ref: "queries.json", summary: "new attachment" },
				...(source === "explicit" ? [{ kind: "source" as const, ref: sourceRef, summary: "refresh" }] : []),
			],
			incrementalRevision: {
				baseEvidenceId: base.id,
				baseHash: incrementalContentHash(base.content),
				operations: [
					{
						op: "set",
						path: ["queryStrategy", "limitations"],
						value: "clarified",
						issueId: obligation.items![0].id,
						sourceRefs: [],
						reason: "clarify",
					},
				],
				affectedCriteria: ["verify declared content"],
				rationale: "bounded repair",
			},
		},
		{ executionRoot: repairWorkspace, sessionRef: "pi-session:offline", job: scenario.job },
	);
	await writeWorkerOutputManifest(
		{
			schemaVersion: "astra.worker_output_manifest.v1",
			manifestId: `manifest_${repair.id}`,
			jobId: repair.jobId,
			taskId: repair.id,
			agentId: repair.agentId,
			status: "completed",
			artifactType: "literature",
			content: validated.content,
			outputRefs: validated.outputRefs.map((ref) =>
				ref.kind === "artifact" || ref.kind === "log"
					? { ...ref, ref: `.astra/jobs/${repair.jobId}/workspaces/${repair.id}/${ref.ref}` }
					: ref,
			),
			incrementalRevision: validated.incrementalRevision,
			validationStatus: "passed",
			validationErrors: [],
			sessionRef: "pi-session:offline",
			createdAt: new Date().toISOString(),
		},
		scenario.root,
	);
	const newRef = relative(scenario.root, join(repairWorkspace, "queries.json")).split("\\").join("/");
	return { ...scenario, base, repair, ref, newRef, sourceRef, validated };
}

describe("incremental frozen evidence files", () => {
	it("keeps two independent same-name input files in their own artifact namespaces", async () => {
		const f = await fixture();
		const result = (await f.job.recoverWorkerTaskCompletion(f.repair.id))!;
		const second = await f.task("independent same-name delivery");
		const workspace = await prepareTaskWorkspace(second, f.job);
		await writeFile(join(workspace, "queries.json"), "original frozen bytes");
		const otherRef = relative(f.root, join(workspace, "queries.json")).split("\\").join("/");
		const other = await f.job.completeWorkerTask(second.id, {
			artifactType: "literature",
			content: { content: "independent", queryStrategy: { limitations: "independent" }, sources: [] },
			refs: [otherRef],
		});
		const consumer = await f.task("consume independent same-name inputs", [result.id, other.id]);
		const cwd = await prepareTaskWorkspace(consumer, f.job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8")) as {
			inputs: { files: Array<{ artifactId: string; sourceRef: string; path: string; sha256: string }> };
		};
		const original = context.inputs.files.find((file) => file.artifactId === result.id && file.sourceRef === f.ref)!;
		const independent = context.inputs.files.find(
			(file) => file.artifactId === other.id && file.sourceRef === otherRef,
		)!;
		expect(original.sha256).toBe(independent.sha256);
		expect(original.path).not.toBe(independent.path);
		expect(await readFile(join(cwd, original.path), "utf8")).toBe("original frozen bytes");
		expect(await readFile(join(cwd, independent.path), "utf8")).toBe("original frozen bytes");
	});
	it.each(["new colliding bytes", "original frozen bytes"])(
		"rejects a new path colliding with an inherited materialization identity (%s)",
		async (bytes) => {
			const f = await fixture();
			const workspace = join(f.root, ".astra", "jobs", f.repair.jobId, "workspaces", f.repair.id);
			const local = `files/${createHash("sha256").update(f.ref).digest("hex")}/queries.json`;
			await mkdir(join(workspace, "files", createHash("sha256").update(f.ref).digest("hex")), { recursive: true });
			await writeFile(join(workspace, local), bytes);
			const validated = await validateWorkerSubmission(
				f.job.state.tasks[f.repair.id],
				{
					artifactType: "literature",
					content: {},
					refs: [{ kind: "artifact", ref: local, summary: "new declared file" }],
					incrementalRevision: f.validated.incrementalRevision,
				},
				{ executionRoot: workspace, sessionRef: "pi-session:offline", job: f.job },
			);
			const manifest = await readJson<WorkerOutputManifest>(workerManifestPath(f.root, f.repair.jobId, f.repair.id));
			manifest.outputRefs = validated.outputRefs.map((ref) =>
				ref.kind === "artifact" || ref.kind === "log"
					? { ...ref, ref: `.astra/jobs/${f.repair.jobId}/workspaces/${f.repair.id}/${ref.ref}` }
					: ref,
			);
			await writeWorkerOutputManifest(manifest, f.root);
			const result = (await f.job.recoverWorkerTaskCompletion(f.repair.id))!;
			const next = await f.task("consume colliding identities", [result.id]);
			await expect(prepareReviewEvidenceBundle(next, result, f.job)).rejects.toThrow(
				"conflicting review file bindings",
			);
			await expect(prepareTaskWorkspace(next, f.job)).rejects.toThrow("conflicting input file bindings");
		},
	);
	it("rejects public local refs that try to replace a base file from another workspace", async () => {
		const f = await fixture();
		const workspace = join(f.root, ".astra", "jobs", f.repair.jobId, "workspaces", f.repair.id);
		await expect(
			validateWorkerSubmission(
				f.job.state.tasks[f.repair.id],
				{
					artifactType: "literature",
					content: f.validated.content,
					refs: [{ kind: "artifact", ref: relative(workspace, join(f.root, f.ref)), summary: "replace old file" }],
				},
				{ executionRoot: workspace, sessionRef: "pi-session:offline", job: f.job },
			),
		).rejects.toThrow("outside");
		const path = workerManifestPath(f.root, f.repair.jobId, f.repair.id);
		const manifest = await readJson<WorkerOutputManifest>(path);
		manifest.outputRefs = [
			{ kind: "artifact", ref: f.ref, sha256: "0".repeat(64), summary: "conflicting old binding" },
		];
		await writeWorkerOutputManifest(manifest, f.root);
		await expect(f.job.recoverWorkerTaskCompletion(f.repair.id)).rejects.toThrow("does not match this task");
	});
	it("rejects changed new files and incorrect base identity", async () => {
		const f = await fixture();
		await writeFile(join(f.root, f.newRef), "changed after validation");
		await expect(
			f.job.completeWorkerTask(f.repair.id, {
				artifactType: "literature",
				content: f.validated.content,
				refs: [f.newRef],
				incrementalRevision: f.validated.incrementalRevision,
			}),
		).rejects.toThrow("changed after validation");
		await expect(
			f.job.completeWorkerTask(f.repair.id, {
				artifactType: "literature",
				content: f.validated.content,
				refs: [],
				incrementalRevision: { ...f.validated.incrementalRevision!, baseHash: "0".repeat(64) },
			}),
		).rejects.toThrow("declared base");
		await expect(
			f.job.completeWorkerTask(f.repair.id, {
				artifactType: "literature",
				content: f.validated.content,
				refs: [],
				incrementalRevision: { ...f.validated.incrementalRevision!, baseEvidenceId: "evidence_not_an_input" },
			}),
		).rejects.toThrow("declared base");
	});
	it("three approved repair plans bind frozen ancestor versions through adoption", async () => {
		const f = await lifecycleScenario("pi", {
			...lifecycleDefinition,
			id: "literature",
			outputArtifactType: "literature",
		});
		roots.push(f.root);
		const original = await f.task("original bounded delivery");
		const cwd = await prepareTaskWorkspace(original, f.job);
		await writeFile(join(cwd, "queries.json"), "first frozen file");
		const ref = relative(f.root, join(cwd, "queries.json")).split("\\").join("/");
		let base = await f.job.completeWorkerTask(original.id, {
			artifactType: "literature",
			content: { content: "original", queryStrategy: { limitations: "original" }, sources: [] },
			refs: [ref],
		});
		const frozen = base.files![0];
		for (let round = 1; round <= 3; round++) {
			const failed = await f.job.recordReview(
				reviewFixture(f.job, { evidenceId: base.id, verdict: "fail", findings: [] }),
			);
			const obligation = Object.values(f.job.state.obligations).find((item) => item.sourceReviewId === failed.id)!;
			const plan = await f.job.recordStagePlan({
				schemaVersion: "astra.stage_plan_manifest.v1",
				id: `repair_plan_${round}`,
				jobId: original.jobId,
				stageId: "literature",
				decisionRef: `repair_plan_${round}`,
				obligationId: obligation.id,
				mode: "decompose",
				tasks: [
					{
						key: "repair",
						objective: "bounded approved repair",
						deliveryKind: "stage",
						inputArtifactRefs: [base.id],
						requiredOutputFields: ["content"],
						acceptanceChecks: ["verify declared content"],
						successCriteria: [],
						failureSignals: ["missing content"],
					},
				],
				rationale: "repair exact failed criterion",
				sessionRef: "offline",
				createdAt: new Date().toISOString(),
			});
			const planEvidence = await preparePlanEvidence(f.job, plan);
			await f.job.recordReview(
				reviewFixture(f.job, { evidenceId: planEvidence.id, verdict: "pass", findings: [], blocking: false }),
			);
			const contract = buildEffectiveTaskContract(f.job, plan, plan.tasks[0]);
			expect(contract.inputVersions.every((input) => input.versionHash !== null)).toBe(true);
			const task = await f.job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:repair`,
			});
			const workspace = await prepareTaskWorkspace(task, f.job);
			const check = task.repairChecks![0];
			const validated = await validateWorkerSubmission(
				f.job.state.tasks[task.id],
				{
					artifactType: "literature",
					content: {},
					refs: [],
					incrementalRevision: {
						baseEvidenceId: base.id,
						baseHash: incrementalContentHash(base.content),
						operations: [
							{
								op: "set",
								path: ["queryStrategy", "limitations"],
								value: `round ${round}`,
								issueId: check.issueId,
								sourceRefs: [],
								reason: "clarify",
							},
						],
						affectedCriteria: task.acceptanceChecks,
						rationale: "bounded approved repair",
					},
				},
				{ executionRoot: workspace, sessionRef: "pi-session:offline", job: f.job },
			);
			base = await f.job.completeWorkerTask(task.id, {
				artifactType: "literature",
				content: validated.content,
				refs: [],
				incrementalRevision: validated.incrementalRevision,
			});
			expect(base.files).toContainEqual(frozen);
		}
		const review = await f.reviewer.review(base, f.job);
		await f.job.recordReview({ ...review, evidenceId: base.id });
		await f.job.decideEvidence(base.id, true);
		const adopted = await f.job.adoptEvidence(base.id);
		expect(adopted.adoptionCompletedAt).toBeDefined();
		f.job = (await ResearchJob.open(f.store, original.jobId))!;
		await f.job.recoverPendingOperations();
		expect(f.job.state.canonical[adopted.id].status).toBe("active");
	});
	it.each(["implicit", "explicit"] as const)(
		"keeps %s source snapshot semantics through manifest recovery",
		async (source) => {
			const f = await fixture("deleted", source);
			const result = await f.job.recoverWorkerTaskCompletion(f.repair.id);
			const prior = f.base.files!.find((file) => file.sourceRef === f.sourceRef)!;
			const current = result!.files!.find((file) => file.sourceRef === f.sourceRef)!;
			if (source === "implicit") expect(current).toEqual(prior);
			else expect(current.sha256).not.toBe(prior.sha256);
		},
	);
	it("direct registration inherits the same frozen files", async () => {
		const f = await fixture("deleted");
		await f.job.setTaskStatus(f.repair.id, "succeeded");
		const result = await f.job.recordEvidence({
			taskId: f.repair.id,
			stageId: "literature",
			type: "literature",
			content: f.validated.content,
			refs: [f.ref, f.newRef],
			incrementalRevision: f.validated.incrementalRevision,
			currentEvidenceSetId: f.base.currentEvidenceSetId,
		});
		expect(result.files).toContainEqual(f.base.files![0]);
	});
	it("reviews, adopts, materializes and downloads inherited files with bounded identities", async () => {
		const f = await fixture("deleted");
		const result = (await f.job.recoverWorkerTaskCompletion(f.repair.id))!;
		const reviewed = await f.reviewer.review(result, f.job);
		await f.job.recordReview({ ...reviewed, evidenceId: result.id });
		await f.job.decideEvidence(result.id, true);
		const artifact = await f.job.adoptEvidence(result.id);
		const next = await f.task("consume frozen artifact", [artifact.id]);
		const cwd = await prepareTaskWorkspace(next, f.job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8")) as {
			inputs: { files: Array<{ path: string; sourceRef: string; sha256: string }> };
		};
		const inherited = context.inputs.files.find((file) => file.sourceRef === f.ref)!;
		const mirror = context.inputs.files.find(
			(file) => file.sourceRef === f.ref.replace("queries.json", "mirror/queries.json"),
		)!;
		expect(mirror.sha256).toBe(inherited.sha256);
		expect(mirror.path).not.toBe(inherited.path);
		expect(inherited.path.split("/")).not.toContain("..");
		expect(inherited.path.startsWith(`inputs/${artifact.id}/`)).toBe(true);
		expect(await readFile(join(cwd, inherited.path), "utf8")).toBe("original frozen bytes");
		await writeFile(join(f.root, ".astra", "active-job.json"), JSON.stringify({ jobId: f.repair.jobId }));
		const web = await startWorkbench({ root: f.root, watch: [f.root], port: 0 });
		try {
			const jobs = (await (await fetch(`${web.url}/api/jobs`)).json()) as { jobs: Array<{ id: string }> };
			const query = new URLSearchParams({ id: jobs.jobs[0].id, artifact: artifact.id, ref: f.ref });
			const download = await fetch(`${web.url}/api/file?${query}`);
			expect(download.status).toBe(200);
			expect(await download.text()).toBe("original frozen bytes");
			query.set("artifact", "artifact_from_other_job");
			expect((await fetch(`${web.url}/api/file?${query}`)).status).toBe(400);
			query.set("artifact", artifact.id);
			query.set("ref", f.ref.replace("queries.json", "not-declared.json"));
			expect((await fetch(`${web.url}/api/file?${query}`)).status).toBe(400);
			query.set("ref", f.ref);
			// Deliberately invalid isolated persistence fixture: a declared old ref without its frozen binding.
			const saved = (await f.store.loadSnapshot(f.repair.jobId))!;
			const invalid = structuredClone(saved);
			invalid.evidence[result.id].files = invalid.evidence[result.id].files!.filter(
				(file) => file.sourceRef !== f.ref,
			);
			await f.store.writeSnapshot(invalid);
			const unbound = await fetch(`${web.url}/api/file?${query}`);
			expect(unbound.status).toBe(400);
			expect(await unbound.json()).toEqual({ error: "下载文件不属于来源任务" });
			await f.store.writeSnapshot(saved);
			const blob = join(f.root, ".astra", "jobs", f.repair.jobId, "versions", "files", inherited.sha256);
			await chmod(blob, 0o600);
			await writeFile(blob, "corrupt download bytes");
			expect((await fetch(`${web.url}/api/file?${query}`)).status).toBe(400);
		} finally {
			await new Promise<void>((resolve, reject) => web.server.close((error) => (error ? reject(error) : resolve())));
		}
	});
	for (const mutation of [undefined, "changed", "deleted"] as const) {
		it(`inherits frozen files with old live file ${mutation ?? "intact"}`, async () => {
			const f = await fixture(mutation);
			const result = await f.job.completeWorkerTask(f.repair.id, {
				artifactType: "literature",
				content: f.validated.content,
				refs: [f.newRef],
				incrementalRevision: f.validated.incrementalRevision,
			});
			for (const file of f.base.files!) expect(result.files).toContainEqual(file);
			expect(result.files).toContainEqual({
				sourceRef: f.newRef,
				sha256: createHash("sha256").update("new repair bytes").digest("hex"),
			});
			expect((await readVersionedFile(f.repair, result, f.ref, "", ""))?.toString()).toBe("original frozen bytes");
			const reopened = (await ResearchJob.open(f.store, f.repair.jobId))!;
			expect(await reopened.recoverWorkerTaskCompletion(f.repair.id)).toEqual(result);
		});
	}
	it("recovers a passed manifest without rereading old workspace", async () => {
		const f = await fixture("deleted");
		const reopened = (await ResearchJob.open(f.store, f.repair.jobId))!;
		const result = await reopened.recoverWorkerTaskCompletion(f.repair.id);
		expect(result?.files).toContainEqual(f.base.files![0]);
	});
	it.each(["damaged", "missing"])("rejects %s frozen blobs", async (fault) => {
		const f = await fixture();
		const blob = join(f.root, ".astra", "jobs", f.repair.jobId, "versions", "files", f.base.files![0].sha256);
		if (fault === "damaged") {
			await chmod(blob, 0o600);
			await writeFile(blob, "corrupt");
		} else await rm(blob);
		await expect(
			f.job.completeWorkerTask(f.repair.id, {
				artifactType: "literature",
				content: f.validated.content,
				refs: [f.newRef],
				incrementalRevision: f.validated.incrementalRevision,
			}),
		).rejects.toThrow("integrity");
		if (fault === "damaged") expect(await readFile(blob, "utf8")).toBe("corrupt");
	});
});
