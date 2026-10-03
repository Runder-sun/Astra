import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { reviewPacketPath } from "../src/contracts.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import type { ReviewPacket, TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function setup(kind: "stage" | "local" | "synthesis" = "stage") {
	const root = await mkdtemp(join(tmpdir(), "astra-codex-frozen-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		objective: "offline Codex boundary",
		workspaceRoot: root,
		automation: "full",
	});
	const dispatch = (fields: Partial<TaskPacket> = {}) =>
		job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: `frozen worker ${Object.keys(job.state.tasks).length}`,
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verified"],
			successCriteria: ["complete"],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			...fields,
		});
	const inputArtifactRefs: string[] = [];
	if (kind === "synthesis") {
		const local = await dispatch({ deliveryKind: "local" });
		await job.setTaskStatus(local.id, "succeeded");
		const value = await job.recordEvidence({
			taskId: local.id,
			stageId: local.stageId,
			type: "validation",
			content: { content: "local" },
			refs: [],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: value.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(value.id, true);
		inputArtifactRefs.push(value.id);
	}
	const task = await dispatch({
		deliveryKind: kind,
		inputArtifactRefs,
		repairChecks: [
			{ issueId: "manual", criterion: "repair-only check" },
			{ issueId: "shared", criterion: "verified" },
		],
	});
	const workspace = join(root, ".astra/jobs", task.jobId, "workspaces", task.id);
	await mkdir(workspace, { recursive: true });
	await writeFile(join(workspace, "result.json"), '{"metric":1}\n');
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: task.stageId,
		type: "validation",
		content: { content: "target" },
		refs: ["result.json"],
	});
	vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "review-preflight");
	vi.stubEnv("ASTRA_FAKE_CODEX_REVIEW_RECEIPT", "exact");
	vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
	const runner = new CodexAppServerRunner({
		executable: process.execPath,
		prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
	});
	const adapter = new CodexResearchAdapters(runner);
	return { root, store, job, evidence, runner, adapter };
}
async function corrupt(cwd: string) {
	const packet = JSON.parse(await readFile(join(cwd, "review-packet.json"), "utf8")) as ReviewPacket;
	const ref = packet.resolvedEvidenceRefs.find((ref) => ref.sourceRef === "result.json")!;
	const path = join(cwd, ref.path);
	await chmod(path, 0o600);
	await writeFile(path, '{"metric":999}\n');
}

it.each(["stage", "local", "synthesis"] as const)(
	"Codex expands unique repairChecks and exact deduplication for a valid %s delivery",
	async (kind) => {
		const { job, evidence, adapter } = await setup(kind);
		const review = await adapter.review(evidence, job);
		expect(review.criteria?.map((item) => item.criterion)).toEqual(["verified", "complete", "repair-only check"]);
		await job.recordReview({ ...review, evidenceId: evidence.id });
		await job.recoverPendingOperations();
		expect(Object.values(job.state.reviews).filter((item) => item.evidenceId === evidence.id)).toHaveLength(1);
	},
);

it.each(["core", "restart"])(
	"Codex rejects corrupt uncited actual copies during %s with zero semantic rejections",
	async (mode) => {
		const { root, store, job, evidence, adapter } = await setup();
		const output = await adapter.review(evidence, job);
		await corrupt(dirname(reviewPacketPath(root, job.state.frame.jobId, output.reviewerTaskId!)));
		if (mode === "core")
			await expect(job.recordReview({ ...output, evidenceId: evidence.id })).rejects.toThrow(/integrity|version/i);
		else
			await expect(
				(await ResearchJob.open(store, job.state.frame.jobId))!.recoverPendingOperations(),
			).rejects.toThrow(/integrity|version/i);
		expect(Object.values(job.state.reviews).filter((item) => item.evidenceId === evidence.id)).toHaveLength(0);
		expect(
			(await store.readEvents(job.state.frame.jobId)).filter(
				(saved) => saved.event.type === "review_delivery_rejected",
			),
		).toHaveLength(0);
	},
);

it.each(["tool", "final"])(
	"Codex awaits actual file verification at the %s boundary without a synchronous correction retry",
	async (boundary) => {
		const { root, store, job, evidence, runner, adapter } = await setup();
		const run = runner.run.bind(runner);
		vi.spyOn(runner, "run").mockImplementation(async (options) => {
			if (boundary === "tool") await corrupt(options.cwd);
			const result = await run(options);
			if (boundary === "final") await corrupt(options.cwd);
			return result;
		});
		await expect(adapter.review(evidence, job)).rejects.toThrow(/integrity|version/i);
		expect(
			Object.values(job.state.tasks)
				.filter((task) => task.role === "reviewer")
				.every((task) => task.status === "failed"),
		).toBe(true);
		expect(Object.values(job.state.sessions).every((session) => session.status === "failed")).toBe(true);
		const requests = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line) as { method: string });
		expect(requests.filter((request) => request.method === "turn/start")).toHaveLength(1);
		expect(
			(await store.readEvents(job.state.frame.jobId)).filter(
				(saved) => saved.event.type === "review_delivery_rejected",
			),
		).toHaveLength(0);
	},
);
