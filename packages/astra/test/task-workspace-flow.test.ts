import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { afterEach, describe, expect, it } from "vitest";
import { writeWorkerOutputManifest } from "../src/contracts.ts";
import { writeSourceReceipt } from "../src/literature.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import {
	prepareReviewEvidenceBundle,
	prepareTaskWorkspace,
	taskInputResources,
	taskResourcePath,
} from "../src/task-workspace.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function scenario() {
	const root = await mkdtemp(join(tmpdir(), "astra-file-flow-"));
	roots.push(root);
	const job = await ResearchJob.create(new MemoryAstraStore(), {
		workspaceRoot: root,
		objective: "retain executable evidence",
		automation: "full",
	});
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "produce code",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["code runs"],
		failureSignals: ["broken imports"],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read", "write", "bash"],
		writeAuthority: "workspace-write",
		budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["code runs"],
	});
	const sourceRoot = await prepareTaskWorkspace(task, job);
	const files = {
		"src/main.mjs": 'import { value } from "./lib/value.mjs"; console.log(value);',
		"src/lib/value.mjs": "export const value = 42;",
	};
	for (const [path, content] of Object.entries(files)) {
		await mkdir(dirname(join(sourceRoot, path)), { recursive: true });
		await writeFile(join(sourceRoot, path), content);
	}
	await job.setTaskStatus(task.id, "running");
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: "validation",
		content: { content: "executable evidence" },
		refs: [
			`codex-session:source`,
			...Object.keys(files).map((path) => `.astra/jobs/${task.jobId}/workspaces/${task.id}/${path}`),
		],
	});
	return { root, job, task, sourceRoot, evidence };
}

describe("research files across stages and repairs", () => {
	it("exposes candidate content as a directly readable indexed file without adopting it", async () => {
		const { job, task, evidence } = await scenario();
		const consumer = await job.dispatchTask({
			...task,
			id: "read_candidate_content",
			replayKey: "read_candidate_content",
			inputArtifactRefs: [evidence.id],
			writeAuthority: "none",
		});
		const cwd = await prepareTaskWorkspace(consumer, job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
		const input = context.inputs.evidence.find((entry: { id: string }) => entry.id === evidence.id);
		expect(input.contentPath).toBe(`input-evidence/${evidence.id}.json`);
		expect(JSON.parse(await readFile(join(cwd, input.contentPath), "utf8"))).toEqual(evidence);
		expect(context.inputs.files).toContainEqual(
			expect.objectContaining({ artifactId: evidence.id, path: input.contentPath }),
		);
		expect(context.inputs.canonicalArtifacts).toEqual([]);
		expect(context.resources.writableRoot).toBeNull();
	});
	it("preserves exact observed search replies for reviewers and repair inputs", async () => {
		const { root, job, task, evidence } = await scenario();
		const logRoot = join(root, ".astra", "jobs", task.jobId, "codex-events");
		await mkdir(logRoot);
		const search = {
			type: "dynamicToolCall",
			tool: "astra_search_literature",
			arguments: { query: "exact query", limit: 2 },
			success: true,
			contentItems: [
				{
					type: "inputText",
					text: JSON.stringify({
						query: "exact query",
						total: 8,
						results: [{ sourceRef: "openalex:W1" }, { sourceRef: "openalex:W2" }],
					}),
				},
			],
		};
		const web = {
			type: "webSearch",
			action: { type: "search", queries: ["web query"] },
			results: [{ type: "text_result", url: "https://example.org/paper", title: "Paper", ref_id: "turn0search0" }],
		};
		const events = [search, web, { type: "reasoning", text: "private reasoning" }].map((item) => ({
			method: "item/completed",
			params: { item },
			emittedAtMs: 123,
		}));
		await writeFile(
			join(logRoot, `${task.id}-${task.attempt}.jsonl`),
			events.map((event) => JSON.stringify(event)).join("\n"),
		);
		const bundle = await prepareReviewEvidenceBundle({ ...task, id: "review_searches" }, evidence, job);
		const receipt = bundle.find((file) => file.path === `execution/${task.id}/literature-searches.json`);
		expect(receipt).toBeDefined();
		const body = await readFile(
			join(root, ".astra", "jobs", task.jobId, "tasks", "review_searches", receipt!.path),
			"utf8",
		);
		expect(JSON.parse(body)).toMatchObject({
			taskId: task.id,
			searches: [
				{ emittedAtMs: 123, item: search },
				{ emittedAtMs: 123, item: web },
			],
		});
		expect(body).not.toContain("private reasoning");
		const repair = await job.dispatchTask({
			...task,
			id: "consume_searches",
			replayKey: "consume_searches",
			inputArtifactRefs: [evidence.id],
		});
		const cwd = await prepareTaskWorkspace(repair, job);
		expect(
			await readFile(join(cwd, `inputs/${evidence.id}/execution/${task.id}/literature-searches.json`), "utf8"),
		).toBe(body);
		const downstream = await prepareReviewEvidenceBundle(
			{ ...repair, id: "review_consumed_searches" },
			{ ...evidence, taskId: repair.id, refs: [] },
			job,
		);
		expect(downstream).toContainEqual(
			expect.objectContaining({ path: `inputs/${evidence.id}/execution/${task.id}/literature-searches.json` }),
		);
	});
	it("exposes host-observed command failures when worker evidence omits them", async () => {
		const { root, job, task, evidence } = await scenario();
		const logRoot = join(root, ".astra", "jobs", task.jobId, "codex-events");
		await mkdir(logRoot);
		const failed = {
			type: "commandExecution",
			id: "failed_command",
			command: "python3 missing.py",
			exitCode: 2,
			status: "failed",
			aggregatedOutput: "missing.py: No such file",
		};
		const events = [
			{ method: "item/completed", params: { item: { type: "reasoning", text: "private reasoning" } } },
			{ method: "item/completed", params: { item: { ...failed, id: "success", exitCode: 0, status: "completed" } } },
			{ method: "item/completed", params: { item: failed } },
		];
		await writeFile(
			join(logRoot, `${task.id}-${task.attempt}.jsonl`),
			`${events.map((event) => JSON.stringify(event)).join("\n")}\n`,
		);
		const bundle = await prepareReviewEvidenceBundle({ ...task, id: "review_observed_failures" }, evidence, job);
		const receipt = bundle.find((file) => file.path === `execution/${task.id}/failed-commands.json`);
		expect(receipt).toBeDefined();
		const body = await readFile(
			join(root, ".astra", "jobs", task.jobId, "tasks", "review_observed_failures", receipt!.path),
			"utf8",
		);
		expect(JSON.parse(body)).toMatchObject({ taskId: task.id, failures: [failed] });
		expect(body).not.toContain("private reasoning");
		const repair = await job.dispatchTask({
			...task,
			id: "consume_failures",
			replayKey: "consume_failures",
			inputArtifactRefs: [evidence.id],
		});
		const cwd = await prepareTaskWorkspace(repair, job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
		expect(context.inputs.files).toContainEqual(
			expect.objectContaining({ path: `inputs/${evidence.id}/execution/${task.id}/failed-commands.json` }),
		);
		expect(await readFile(join(cwd, `inputs/${evidence.id}/execution/${task.id}/failed-commands.json`), "utf8")).toBe(
			body,
		);
		const downstreamBundle = await prepareReviewEvidenceBundle(
			{ ...repair, id: "review_consumed_failures" },
			{ ...evidence, taskId: repair.id, refs: [] },
			job,
		);
		expect(downstreamBundle).toContainEqual(
			expect.objectContaining({ path: `inputs/${evidence.id}/execution/${task.id}/failed-commands.json` }),
		);
	});
	it.each(["retired", "superseded"])(
		"exposes %s archives for backtrack repair without making them current inputs",
		async (kind) => {
			const { job, task, evidence } = await scenario();
			await writeFile(
				join(taskResourcePath(task.scope.workspaceRoot, task.jobId, task.id), "raw.csv"),
				"value\n42\n",
			);
			let winner = evidence;
			if (kind === "superseded") {
				await job.decideEvidence(evidence.id, false, "reject");
				const replacement = await job.dispatchTask({ ...task, id: "replacement", replayKey: "replacement" });
				await prepareTaskWorkspace(replacement, job);
				await job.setTaskStatus(replacement.id, "succeeded");
				winner = await job.recordEvidence({
					taskId: replacement.id,
					stageId: task.stageId,
					type: evidence.type,
					content: evidence.content,
					refs: [],
					currentEvidenceSetId: evidence.currentEvidenceSetId,
				});
			}
			await job.recordReview(reviewFixture(job, { evidenceId: winner.id, verdict: "pass", findings: [] }));
			await job.decideEvidence(winner.id, true, "accept");
			const artifact = await job.adoptEvidence(winner.id);
			expect(await taskInputResources({ ...task, inputArtifactRefs: [] }, job)).toEqual([]);
			await job.reopenStage(task.stageId, "backtrack_archive", "verify archived results");
			await job.reload();
			await expect(
				job.dispatchTask({
					...task,
					id: "stale_input",
					replayKey: "stale_input",
					inputArtifactRefs: [artifact.id],
				}),
			).rejects.toThrow(/stale/);
			const repair = await job.dispatchTask({
				...task,
				id: "archive_repair",
				replayKey: "archive_repair",
				inputArtifactRefs: [],
			});
			const cwd = await prepareTaskWorkspace(repair, job);
			const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
			expect(context.inputs.canonicalArtifacts).toEqual([]);
			expect(context.inputs.resources).toHaveLength(kind === "superseded" ? 2 : 1);
			const resource = context.inputs.resources.find((entry: { taskId: string }) => entry.taskId === task.id);
			expect(resource).toMatchObject({
				artifactId: kind === "superseded" ? evidence.id : artifact.id,
				purpose: "historical-repair",
				taskId: task.id,
			});
			expect(await readFile(join(resource.root, "resources", "raw.csv"), "utf8")).toBe("value\n42\n");
			expect(await readFile(join(resource.root, "workspace", "src/lib/value.mjs"), "utf8")).toContain("42");
			expect(await taskInputResources({ ...repair, stageId: "literature" }, job)).toEqual([]);
		},
	);

	it("rejects file changes between validated submission and evidence registration", async () => {
		const { root, job, task, evidence, sourceRoot } = await scenario();
		const file = evidence.files![0];
		await writeWorkerOutputManifest(
			{
				schemaVersion: "astra.worker_output_manifest.v1",
				manifestId: "manifest_version",
				jobId: task.jobId,
				taskId: task.id,
				agentId: task.agentId,
				status: "completed",
				artifactType: "validation",
				content: evidence.content,
				outputRefs: [{ kind: "artifact", ref: file.sourceRef, sha256: file.sha256, summary: "validated file" }],
				validationStatus: "passed",
				validationErrors: [],
				sessionRef: "fixture:version",
				createdAt: new Date().toISOString(),
			},
			root,
		);
		await writeFile(join(sourceRoot, "src/main.mjs"), "console.log(999);");
		await expect(
			job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: evidence.type,
				content: evidence.content,
				refs: [file.sourceRef],
			}),
		).rejects.toThrow(/changed after validation/);
	});
	it("rejects a corrupted frozen evidence file", async () => {
		const { root, job, task, evidence } = await scenario();
		const file = evidence.files![0];
		const path = join(root, ".astra", "jobs", task.jobId, "versions", "files", file.sha256);
		await rm(path);
		await writeFile(path, "corrupted bytes");
		await expect(prepareReviewEvidenceBundle({ ...task, id: "review_corrupt" }, evidence, job)).rejects.toThrow(
			/integrity/,
		);
	});
	it("keeps submitted bytes when the worker later changes or deletes its files", async () => {
		const { root, job, task, evidence, sourceRoot } = await scenario();
		await writeFile(join(sourceRoot, "src/lib/value.mjs"), "export const value = 999;");
		await rm(join(sourceRoot, "src/main.mjs"));
		const bundle = await prepareReviewEvidenceBundle({ ...task, id: "review_frozen" }, evidence, job);
		expect(bundle).toHaveLength(2);
		const reviewRoot = join(root, ".astra", "jobs", task.jobId, "tasks", "review_frozen");
		const main = bundle.find((file) => file.path.endsWith("src/main.mjs"))!;
		expect((await promisify(execFile)(process.execPath, [join(reviewRoot, main.path)])).stdout.trim()).toBe("42");
		const consumer = await job.dispatchTask({
			...task,
			id: "consume_frozen",
			replayKey: "consume_frozen",
			inputArtifactRefs: [evidence.id],
		});
		const cwd = await prepareTaskWorkspace(consumer, job);
		expect(
			(
				await promisify(execFile)(process.execPath, [join(cwd, "inputs", evidence.id, "src/main.mjs")])
			).stdout.trim(),
		).toBe("42");
	});
	it("carries non-OpenAlex receipts through downstream workers and independent review", async () => {
		const { root, job, task, evidence } = await scenario();
		const ref = "doi:10.1000/test";
		await writeSourceReceipt(
			{ workspaceRoot: root, jobId: task.jobId, query: "test paper", limit: 1 },
			{ sourceRef: ref, title: "Verified Crossref paper", authors: [] },
			"crossref",
			new Date().toISOString(),
		);
		const sourceEvidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: "validation",
			content: evidence.content,
			refs: [ref],
		});
		const consumer = await job.dispatchTask({
			...task,
			id: "consume_sources",
			replayKey: "consume_sources",
			inputArtifactRefs: [sourceEvidence.id],
		});
		const cwd = await prepareTaskWorkspace(consumer, job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
		expect(context.inputs.files).toContainEqual(expect.objectContaining({ sourceRef: ref }));
		const bundle = await prepareReviewEvidenceBundle(
			{ ...consumer, id: "review_crossref" },
			{ ...evidence, taskId: consumer.id, refs: [] },
			job,
		);
		expect(bundle).toContainEqual(expect.objectContaining({ sourceRef: ref }));
	});
	it.each(["canonical", "repair"])("keeps executable directory structure for %s input", async (kind) => {
		const { root, job, task, evidence } = await scenario();
		let inputRef = evidence.id;
		if (kind === "canonical") {
			await job.recordReview(reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }));
			await job.decideEvidence(evidence.id, true, "accept");
			inputRef = (await job.adoptEvidence(evidence.id)).id;
		}
		const consumer = await job.dispatchTask({
			...task,
			id: `consume_${kind}`,
			replayKey: `consume_${kind}`,
			inputArtifactRefs: [inputRef],
			requiredCanonicalArtifacts: kind === "canonical" ? [inputRef] : [],
		});
		const cwd = await prepareTaskWorkspace(consumer, job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
		expect(context.inputs.files).toHaveLength(kind === "canonical" ? 2 : 3);
		const main = join(cwd, "inputs", inputRef, "src/main.mjs");
		expect((await promisify(execFile)(process.execPath, [main])).stdout.trim()).toBe("42");
		const bundle = await prepareReviewEvidenceBundle(
			{ ...consumer, id: "review" },
			{ ...evidence, taskId: consumer.id, refs: [] },
			job,
		);
		expect(bundle.some((ref) => ref.sourceRef === `inputs/${inputRef}/src/main.mjs`)).toBe(true);
		const reviewRoot = join(root, ".astra", "jobs", task.jobId, "tasks", "review");
		expect(
			(
				await promisify(execFile)(process.execPath, [join(reviewRoot, "inputs", inputRef, "src/main.mjs")])
			).stdout.trim(),
		).toBe("42");
	});

	it("provides resource directories for direct evidence repair inputs", async () => {
		const { job, task, evidence } = await scenario();
		const review = await job.recordReview(
			reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
		);
		const repair = await job.dispatchTask({
			...task,
			id: "repair_resources",
			replayKey: "repair_resources",
			inputArtifactRefs: [evidence.id],
			repairOfEvidenceId: evidence.id,
		});
		const cwd = await prepareTaskWorkspace(repair, job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
		const contractPath = `inputs/${evidence.id}/repair-source-task.json`;
		const reviewsPath = `inputs/${evidence.id}/repair-source-reviews.json`;
		expect(context.inputs.files).toEqual(
			expect.arrayContaining([
				expect.objectContaining({ path: contractPath }),
				expect.objectContaining({ path: reviewsPath }),
			]),
		);
		expect(JSON.parse(await readFile(join(cwd, contractPath), "utf8"))).toMatchObject({
			id: task.id,
			acceptanceChecks: task.acceptanceChecks,
		});
		expect(JSON.parse(await readFile(join(cwd, reviewsPath), "utf8"))).toEqual([review]);
		const bundle = await prepareReviewEvidenceBundle(
			{ ...repair, id: "review_repair_inputs" },
			{ ...evidence, taskId: repair.id, refs: [] },
			job,
		);
		expect(bundle).toEqual(
			expect.arrayContaining([
				expect.objectContaining({ path: contractPath }),
				expect.objectContaining({ path: reviewsPath }),
			]),
		);
		expect(context.inputs.resources).toMatchObject([
			{
				artifactId: evidence.id,
				taskId: task.id,
				root: taskResourcePath(task.scope.workspaceRoot, task.jobId, task.id),
			},
		]);
	});

	it("rejects evidence that escapes through a symlinked parent directory", async () => {
		const { root, job, task, evidence, sourceRoot } = await scenario();
		await mkdir(join(root, "private"));
		await writeFile(join(root, "private", "secret.txt"), "not evidence");
		await symlink(join(root, "private"), join(sourceRoot, "escape"));
		const malicious = { ...evidence, refs: [`.astra/jobs/${task.jobId}/workspaces/${task.id}/escape/secret.txt`] };
		await expect(prepareReviewEvidenceBundle({ ...task, id: "review_escape" }, malicious, job)).rejects.toThrow(
			/outside|symbolic/,
		);
	});

	it("gives independent reviewers the retrieved source receipts", async () => {
		const { root, job, task, evidence } = await scenario();
		const sources = join(root, ".astra", "jobs", task.jobId, "sources");
		await mkdir(sources, { recursive: true });
		await writeFile(
			join(sources, "openalex-W1.json"),
			JSON.stringify({ sourceRef: "openalex:W1", record: { title: "Retrieved paper" } }),
		);
		const bundle = await prepareReviewEvidenceBundle(
			{ ...task, id: "review_sources" },
			{ ...evidence, refs: ["openalex:W1"] },
			job,
		);
		const receipt = bundle.find((ref) => ref.sourceRef === "openalex:W1");
		expect(receipt).toBeDefined();
		expect(
			JSON.parse(
				await readFile(join(root, ".astra", "jobs", task.jobId, "tasks", "review_sources", receipt!.path), "utf8"),
			),
		).toMatchObject({ sourceRef: "openalex:W1" });
	});
});
