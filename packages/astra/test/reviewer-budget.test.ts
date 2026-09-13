import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { readJson, reviewPacketPath, taskDir, writeReviewerOutputManifest } from "../src/contracts.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { prepareReviewEvidenceBundle, taskWorkspacePath } from "../src/task-workspace.ts";
import type { ReviewPacket, TaskPacket } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	vi.restoreAllMocks();
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("Pi reviewer TaskPacket budget", () => {
	it("carries declared canonical inputs and their file evidence into reviews of derived evidence", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-reviewer-lineage-"));
		tempRoots.push(root);
		const job = await ResearchJob.create(new MemoryAstraStore(), {
			jobId: "job_reviewer_lineage",
			objective: "review a claim against its upstream run",
			workspaceRoot: root,
			automation: "full",
		});
		const runTask = await job.dispatchTask({
			stageId: "run",
			stageExecutionId: "stage_exec_run",
			agentId: "worker_run",
			role: "worker",
			objective: "produce a file-backed run",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "run",
			requiredOutputFields: job.definitions.run.requiredOutputFields,
			acceptanceChecks: job.definitions.run.acceptanceChecks,
			failureSignals: job.definitions.run.failureSignals,
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: job.definitions.run.acceptanceChecks,
		});
		await job.setTaskStatus(runTask.id, "running");
		await job.setTaskStatus(runTask.id, "succeeded");
		const runWorkspace = taskWorkspacePath(root, job.state.frame.jobId, runTask.id);
		await mkdir(runWorkspace, { recursive: true });
		await writeFile(join(runWorkspace, "pilot-summary.json"), '{"traceCompleteness":1}\n', "utf8");
		const runEvidence = await job.recordEvidence({
			taskId: runTask.id,
			stageId: "run",
			type: "run",
			content: {
				commands: ["run pilot"],
				runs: ["pilot"],
				metrics: { traceCompleteness: 1 },
				logs: ["pilot-summary.json"],
				failures: [],
			},
			refs: ["pilot-summary.json", "pi-session:run"],
		});
		await job.recordReview(reviewFixture(job, { evidenceId: runEvidence.id, verdict: "pass", findings: [] }));
		await job.decideEvidence(runEvidence.id, true, "accept-run");
		const runArtifact = await job.adoptEvidence(runEvidence.id);

		const claimTask = await job.dispatchTask({
			stageId: "result-to-claim",
			stageExecutionId: "stage_exec_result-to-claim",
			agentId: "worker_claim",
			role: "worker",
			objective: "derive a bounded claim from the canonical run",
			inputArtifactRefs: [runArtifact.id],
			requiredCanonicalArtifacts: [runArtifact.id],
			requiredOutputType: "result-to-claim",
			requiredOutputFields: job.definitions["result-to-claim"].requiredOutputFields,
			acceptanceChecks: job.definitions["result-to-claim"].acceptanceChecks,
			failureSignals: job.definitions["result-to-claim"].failureSignals,
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: job.definitions["result-to-claim"].acceptanceChecks,
		});
		await job.setTaskStatus(claimTask.id, "running");
		await job.setTaskStatus(claimTask.id, "succeeded");
		const claimEvidence = await job.recordEvidence({
			taskId: claimTask.id,
			stageId: "result-to-claim",
			type: "result-to-claim",
			content: {
				claims: ["trace completeness was one in the bounded pilot"],
				supportingResults: [runArtifact.id],
				unsupportedClaims: [],
				missingEvidence: [],
				conclusion: "bounded result only",
			},
			refs: ["pi-session:claim"],
		});
		const reviewTask = await job.dispatchTask({
			stageId: "result-to-claim",
			stageExecutionId: "stage_exec_result-to-claim",
			agentId: "reviewer_claim",
			role: "reviewer",
			objective: "review the derived claim",
			inputArtifactRefs: [claimEvidence.id],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "review",
			requiredOutputFields: ["verdict", "findings"],
			acceptanceChecks: ["claim is backed by the run"],
			failureSignals: ["run evidence is missing"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: [".astra"] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: false,
			resumePolicy: "resume-session",
			successCriteria: ["review manifest written"],
		});

		const bundle = await prepareReviewEvidenceBundle(reviewTask, claimEvidence, job);
		const canonicalSourceRef = `canonical/${runArtifact.id}.json`;
		const upstreamSourceRef = `inputs/${runArtifact.id}/pilot-summary.json`;

		expect(bundle).toEqual(
			expect.arrayContaining([
				expect.objectContaining({ sourceRef: canonicalSourceRef }),
				expect.objectContaining({ sourceRef: upstreamSourceRef }),
			]),
		);
		const upstream = bundle.find((ref) => ref.sourceRef === upstreamSourceRef);
		expect(upstream).toBeDefined();
		expect(
			await readFile(join(taskDir(root, job.state.frame.jobId, reviewTask.id), upstream?.path ?? "missing"), "utf8"),
		).toContain('"traceCompleteness":1');
	});

	it("allows enough turns and tools to inspect a packet, snapshot, and submit", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-reviewer-budget-"));
		tempRoots.push(root);
		const store = new MemoryAstraStore();
		const job = await ResearchJob.create(store, {
			jobId: "job_reviewer_budget",
			objective: "review a bounded research result",
			workspaceRoot: root,
			automation: "full",
		});
		const workerTask = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "stage_exec_validation",
			agentId: "worker_reviewer_budget",
			role: "worker",
			objective: "produce validation evidence for review",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: job.definitions.validation.requiredOutputFields,
			acceptanceChecks: ["question is bounded"],
			failureSignals: ["question is vague"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 30_000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["question is falsifiable"],
		});
		await job.setTaskStatus(workerTask.id, "running");
		await job.setTaskStatus(workerTask.id, "succeeded");
		const workerWorkspace = taskWorkspacePath(root, job.state.frame.jobId, workerTask.id);
		await mkdir(workerWorkspace, { recursive: true });
		await writeFile(join(workerWorkspace, "benchmark.json"), '{"latency":1}\n', "utf8");
		const evidence = await job.recordEvidence({
			taskId: workerTask.id,
			stageId: "validation",
			type: "validation",
			content: {
				researchQuestion: "When does A outperform B?",
				scope: "Node.js lookup performance on the current CPU",
				nonGoals: ["browser runtimes"],
				acceptanceCriteria: ["measure lookup latency across bounded collection sizes"],
				falsifiableNextStep: "run a warmup-controlled benchmark",
			},
			refs: ["benchmark.json", "pi-session:worker"],
		});
		const runner = new PiChildSessionRunner({ sessionDir: join(root, "sessions") });
		let reviewerTask: TaskPacket | undefined;
		let reviewerPrompt = "";
		let reviewerEnv: Record<string, string | undefined> = {};
		vi.spyOn(runner, "runTask").mockImplementation(async (task, _role, prompt, env) => {
			reviewerTask = task;
			reviewerPrompt = prompt;
			reviewerEnv = env ?? {};
			await writeReviewerOutputManifest(
				{
					schemaVersion: "astra.reviewer_output_manifest.v1",
					manifestId: `review_${task.id}`,
					jobId: task.jobId,
					taskId: task.id,
					evidenceId: evidence.id,
					verdict: "pass",
					findings: ["evidence is bounded"],
					score: 1,
					criteria: [
						{
							criterion: "question is bounded",
							passed: true,
							score: 1,
							evidenceRefs: [`evidence/${evidence.id}/benchmark.json`],
							rationale: "benchmark artifact was inspected",
						},
						{
							criterion: "question is falsifiable",
							passed: true,
							score: 1,
							evidenceRefs: [`evidence/${evidence.id}/benchmark.json`],
							rationale: "benchmark artifact was inspected",
						},
					],
					verifiedRefs: [`evidence/${evidence.id}/benchmark.json`],
					sessionRef: "pi-session:reviewer",
					createdAt: new Date().toISOString(),
				},
				task.scope.workspaceRoot,
			);
			return { exitCode: 0, stdout: "", stderr: "", jsonEvents: [] };
		});

		await new PiReviewerAdapter(runner).review(evidence, job);

		expect(reviewerTask?.budget).toEqual({ maxTurns: 10, maxToolCalls: 32, maxRuntimeMs: 600_000 });
		const packet = await readJson<ReviewPacket>(
			reviewPacketPath(root, job.state.frame.jobId, reviewerTask?.id ?? "missing"),
		);
		expect(packet.stageContract).toEqual({
			stageId: "validation",
			label: "Validate research question",
			outputArtifactType: "validation",
			requiredOutputFields: job.definitions.validation.requiredOutputFields,
			acceptanceChecks: job.definitions.validation.acceptanceChecks,
			failureSignals: job.definitions.validation.failureSignals,
		});
		expect(packet.workerContract).toEqual({
			objective: workerTask.objective,
			requiredOutputFields: workerTask.requiredOutputFields,
			acceptanceChecks: workerTask.acceptanceChecks,
			failureSignals: workerTask.failureSignals,
			successCriteria: workerTask.successCriteria,
		});
		expect(packet.resolvedEvidenceRefs).toEqual([
			expect.objectContaining({ sourceRef: "benchmark.json", path: `evidence/${evidence.id}/benchmark.json` }),
		]);
		expect(
			await readFile(
				join(
					taskDir(root, job.state.frame.jobId, reviewerTask?.id ?? "missing"),
					`evidence/${evidence.id}/benchmark.json`,
				),
				"utf8",
			),
		).toContain("latency");
		expect(reviewerPrompt).toContain("Judge only the current validation stage artifact");
		expect(reviewerPrompt).toContain(
			"future experiment, result, paper, or final mission deliverables are out of scope",
		);
		expect(reviewerPrompt).toContain(
			"Read review-packet.json and review-target-snapshot.json from the current directory",
		);
		expect(reviewerPrompt).toContain("copy each string exactly, including case and punctuation");
		expect(reviewerPrompt).toContain("workerContract.successCriteria string as a separate criterion");
		expect(reviewerEnv.ASTRA_REVIEW_CRITERIA).toBe(
			JSON.stringify(["question is bounded", "question is falsifiable"]),
		);
		expect(reviewerEnv.ASTRA_EXECUTION_ROOT).toBe(
			taskDir(root, job.state.frame.jobId, reviewerTask?.id ?? "missing"),
		);
		expect(job.state.tasks[reviewerTask?.id ?? "missing"]?.status).toBe("running");
	});
});
