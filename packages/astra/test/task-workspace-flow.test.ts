import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { afterEach, describe, expect, it } from "vitest";
import { writeSourceReceipt } from "../src/literature.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { prepareReviewEvidenceBundle, prepareTaskWorkspace, taskResourcePath } from "../src/task-workspace.ts";
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
		expect(context.inputs.files).toHaveLength(2);
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
		const repair = await job.dispatchTask({
			...task,
			id: "repair_resources",
			replayKey: "repair_resources",
			inputArtifactRefs: [evidence.id],
		});
		const cwd = await prepareTaskWorkspace(repair, job);
		const context = JSON.parse(await readFile(join(cwd, "ASTRA_TASK_CONTEXT.json"), "utf8"));
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
