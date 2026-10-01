import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import * as codingAgent from "@earendil-works/pi-coding-agent";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { FIXTURE_PDF_SOURCE } from "../src/fixture-pdf.ts";
import { fetchAstraFixtureOpenAlex } from "../src/fixture-provider.ts";
import { runAstra } from "../src/launcher.ts";
import { writeSourceReceipt } from "../src/literature.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { parseResearchControlArgs, researchBackend, runResearchControl } from "../src/research-control.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import { taskResourcePath } from "../src/task-workspace.ts";
import { incrementalContentHash } from "../src/worker-submission.ts";

vi.mock("@earendil-works/pi-coding-agent", async (importOriginal) => ({
	...(await importOriginal<typeof codingAgent>()),
	main: vi.fn(),
}));

const roots: string[] = [];
const fixture = fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url));
async function workspace(): Promise<string> {
	const root = await mkdtemp(join(tmpdir(), "astra-codex-integration-"));
	roots.push(root);
	return root;
}
afterEach(async () => {
	vi.restoreAllMocks();
	vi.clearAllMocks();
	vi.unstubAllEnvs();
	vi.unstubAllGlobals();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("Codex research backend", () => {
	it("converts nullable planner handoff identities to optional contract fields", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Normalize handoff identities",
			workspaceRoot: root,
		});
		const transfer = {
			sourceTaskId: "legacy",
			sourceContractHash: "a".repeat(64),
			sourceField: "acceptanceChecks",
			sourceIndex: 0,
			exactCriterion: "resolve issue",
			nodeId: null,
			issueId: "issue_current",
			destinationStageId: "validation",
			destinationPhase: "synthesis",
			rationale: "synthesis owns the issue",
		};
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "ok");
		vi.stubEnv(
			"ASTRA_FAKE_CODEX_OUTPUT",
			JSON.stringify({
				tasks: [
					{
						key: "repair",
						objective: "repair local result",
						deliveryKind: "local",
						inputArtifactRefs: [],
						requiredOutputFields: ["result"],
						acceptanceChecks: ["complete result"],
						failureSignals: ["missing result"],
						successCriteria: ["complete result"],
						responsibilityBindings: [],
						responsibilityTransfers: [transfer],
						hypothesis: "bounded repair",
					},
				],
				rationale: "test schema normalization",
			}),
		);
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const plan = await adapters.planStage(job);
		expect(plan.tasks[0].responsibilityTransfers?.[0].nodeId).toBeUndefined();
		expect(plan.tasks[0].responsibilityTransfers?.[0].issueId).toBe("issue_current");
	});
	it("validates a literature increment envelope, receipt, and final evidence candidate", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Incremental literature",
			workspaceRoot: root,
		});
		const refs = ["https://example.org/a", "https://example.org/b", "https://example.org/c"];
		for (const sourceRef of refs) {
			await writeSourceReceipt(
				{ workspaceRoot: root, jobId: job.state.frame.jobId, query: "fixture", limit: 3 },
				{ sourceRef, title: sourceRef, authors: [] },
				"fixture",
				new Date().toISOString(),
			);
		}
		const sourceTask = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			objective: "create base",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature:local",
			requiredOutputFields: ["queryStrategy", "sources"],
			acceptanceChecks: ["record limitations"],
			failureSignals: ["limitations remain undocumented"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		await job.setTaskStatus(sourceTask.id, "succeeded");
		const base = await job.recordEvidence({
			taskId: sourceTask.id,
			stageId: "literature",
			type: "literature:local",
			content: { queryStrategy: { limitations: "search-only" }, sources: refs.map((sourceRef) => ({ sourceRef })) },
			refs,
		});
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			deliveryKind: "local",
			objective: "repair limitations",
			repairOfEvidenceId: base.id,
			inputArtifactRefs: [base.id],
			requiredOutputType: "literature:local",
			requiredOutputFields: ["queryStrategy", "sources"],
			acceptanceChecks: ["record limitations"],
			repairChecks: [{ issueId: "issue_lit", criterion: "record limitations" }],
			requiredCanonicalArtifacts: [],
			failureSignals: ["limitations remain undocumented"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 10000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		const revision = {
			baseEvidenceId: base.id,
			baseHash: incrementalContentHash(base.content),
			operations: [
				{
					op: "set",
					path: ["queryStrategy", "limitations"],
					value: "capture unavailable",
					issueId: "issue_lit",
					sourceRefs: [],
					reason: "Clarify the recorded limitation.",
				},
			],
			affectedCriteria: task.acceptanceChecks,
			rationale: "Preserve the full base and repair only its limitation.",
		};
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "incremental-submission");
		vi.stubEnv(
			"ASTRA_FAKE_CODEX_OUTPUT",
			JSON.stringify({
				artifactType: task.requiredOutputType,
				contentJson: JSON.stringify({ astraIncrementalRevision: revision, content: {} }),
				refs: [],
			}),
		);
		vi.stubEnv("ASTRA_FAKE_CODEX_RECEIPT", "valid");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const result = await adapters.run(task, job);
		expect(result).toMatchObject({
			content: {
				queryStrategy: { limitations: "capture unavailable" },
				sources: refs.map((sourceRef) => ({ sourceRef })),
			},
			incrementalRevision: { baseEvidenceId: base.id, resultHash: expect.any(String) },
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: result.content,
			refs: result.refs,
			currentEvidenceSetId: base.currentEvidenceSetId,
			incrementalRevision: result.incrementalRevision,
		});
		expect(evidence.incrementalRevision?.resultHash).toBe(incrementalContentHash(evidence.content));
	});
	it("keeps oversized worker contracts in a lossless file instead of the API prompt", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Large repair",
			workspaceRoot: root,
		});
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "Read every requirement",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["requirement ".repeat(100000)],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["preserve all checks"],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		await adapters.run(task, job);
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		const prompt = calls.find((call) => call.method === "turn/start").params.input[0].text;
		expect(prompt.length).toBeLessThan(10000);
		expect(prompt).toContain("worker-task.json");
		const contract = JSON.parse(
			await readFile(join(root, ".astra", "jobs", task.jobId, "workspaces", task.id, "worker-task.json"), "utf8"),
		);
		expect(contract).toEqual(task);
		expect(job.state.tasks[task.id].acceptanceChecks).toEqual(task.acceptanceChecks);
	});
	it("restores all frozen repair aliases before recording and closing an obligation", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Verify repeated repair conditions",
			workspaceRoot: root,
		});
		const criterion = "inspect source";
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "Inspect source",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: [criterion],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: [],
		});
		await job.setTaskStatus(task.id, "succeeded");
		const initial = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: "validation",
			content: { content: "initial" },
			refs: [],
		});
		await job.recordReview({
			evidenceId: initial.id,
			verdict: "fail",
			score: 0,
			findings: [],
			verifiedRefs: [initial.id],
			criteria: [{ criterion, passed: false, score: 0, rationale: "Missing source", evidenceRefs: [initial.id] }],
		});
		const obligation = Object.values(job.state.obligations)[0];
		const issueId = obligation.items![0].id;
		const alias = `[${issueId}] ${criterion}`;
		const repair = await job.dispatchTask({
			...task,
			id: "repair_alias",
			replayKey: "repair_alias",
			repairOfEvidenceId: initial.id,
			acceptanceChecks: [criterion, alias],
			repairChecks: [{ issueId, criterion: alias }],
		});
		await job.setTaskStatus(repair.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: repair.id,
			stageId: repair.stageId,
			type: "validation",
			content: { content: "repaired" },
			refs: [],
			currentEvidenceSetId: initial.currentEvidenceSetId,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const review = await adapters.review(evidence, job);
		expect(review.criteria?.map((item) => item.criterion)).toEqual([criterion, alias]);
		const snapshot = JSON.parse(
			await readFile(
				join(root, ".astra", "jobs", task.jobId, "tasks", review.reviewerTaskId!, "review-target-snapshot.json"),
				"utf8",
			),
		);
		expect(snapshot.reviewCriteria).toEqual([{ criterion, frozenCriteria: [criterion, alias] }]);
		await job.recordReview({ ...review, evidenceId: evidence.id });
		await job.decideEvidence(evidence.id, true);
		expect(job.state.obligations[obligation.id].status).toBe("resolved");
	});
	it.each(["validation", "literature"])(
		"exposes actual worker capabilities to plan reviewers for %s",
		async (stageId) => {
			const root = await workspace();
			const job = await ResearchJob.create(new JsonlAstraStore(root), {
				objective: "Audit actual capabilities",
				workspaceRoot: root,
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			const plan = { ...(await adapters.planStage(job)), stageId };
			const evidence = await preparePlanEvidence(job, plan);
			const review = await adapters.review(evidence, job);
			const snapshot = JSON.parse(
				await readFile(
					join(
						root,
						".astra",
						"jobs",
						job.state.frame.jobId,
						"tasks",
						review.reviewerTaskId!,
						"review-target-snapshot.json",
					),
					"utf8",
				),
			);
			expect(snapshot.runtimeCapabilities.backend).toBe("codex");
			expect(snapshot.runtimeCapabilities.workerTools.includes("astra_capture_source")).toBe(
				stageId === "literature",
			);
			expect(snapshot.runtimeCapabilities.workerTools.includes("astra_search_literature")).toBe(
				stageId === "literature",
			);
		},
	);
	it("puts the latest recorded guidance directly in the main-agent prompt", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "guided planning",
			workspaceRoot: root,
		});
		const guidance = "Preserve the original packet and deliver the complete archive in the new resource root";
		await job.resumeWithGuidance(guidance);
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		await adapters.planStage(job);
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		expect(calls.find((call) => call.method === "turn/start").params.input[0].text).toContain(guidance);
	});

	it.each([
		"review-contradiction",
		"review-preflight",
		"review-receipt",
		"repeated-review-receipt",
		"changed-review-receipt",
		"unknown-review-receipt",
	])("keeps report validity separate from a negative scientific assessment (%s)", async (mode) => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Validate a negative review report",
			workspaceRoot: root,
		});
		const task = await job.dispatchTask({
			stageId: "research-review",
			stageExecutionId: "research-review",
			role: "worker",
			objective: "Audit the research and report needed repairs",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "research-review",
			requiredOutputFields: job.definitions["research-review"].requiredOutputFields,
			acceptanceChecks: ["blocking weaknesses become explicit repairs"],
			failureSignals: ["missing repairs"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["blocking weaknesses become explicit repairs"],
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: {
				verdict: "blocked",
				scientificOutcome: "partially-supported",
				missionCoverage: "insufficient",
				strengths: [],
				weaknesses: ["clipped text"],
				claimAudit: [],
				requiredRepairs: ["repair clipped text"],
			},
			refs: [],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", mode.includes("receipt") ? "review-preflight" : mode);
		vi.stubEnv("ASTRA_FAKE_CODEX_REVIEW_RECEIPT", mode.includes("receipt") ? mode : "");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		if (mode === "review-contradiction") {
			await expect(adapters.review(evidence, job)).rejects.toThrow("failed frozen criterion");
		} else if (mode === "unknown-review-receipt" || mode === "changed-review-receipt") {
			await expect(adapters.review(evidence, job)).rejects.toThrow("Unknown or inconsistent review receipt");
		} else {
			const review = await adapters.review(evidence, job);
			expect(review.verdict).toBe("pass");
			expect(review.criteria).toHaveLength(1);
			expect(review.criteria?.[0].criterion).toBe("blocking weaknesses become explicit repairs");
			expect(review.findings).toEqual(["Fixture contract is complete"]);
			const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line));
			const format = calls.find((call) => call.method === "turn/start").params.outputSchema;
			expect(format.required).toEqual(expect.arrayContaining(Object.keys(format.properties)));
			if (mode === "review-receipt") {
				const validated = calls
					.filter((call) => call.result?.success)
					.map((call) => JSON.parse(call.result.contentItems[0].text))
					.find((result) => result.finalOutput?.astraValidatedReview);
				// The model must copy this identifier; a full digest caused a live transcription failure.
				expect(validated.finalOutput.astraValidatedReview.length).toBeLessThanOrEqual(16);
			}
			expect(job.state.evidence[evidence.id].content).toMatchObject({
				verdict: "blocked",
				requiredRepairs: ["repair clipped text"],
			});
			expect(job.completionBlockers()).toContain("no passing whole-research review");
			expect(Object.values(job.state.sessions)).toHaveLength(1);
		}
	});
	it.each([false, true, "receipt", "translated-receipt", "changed-source-receipt", "unknown-receipt"])(
		"repairs missing refs inside one session and rechecks the final submission (invalid final: %s)",
		async (invalidFinal) => {
			const root = await workspace();
			const job = await ResearchJob.create(new JsonlAstraStore(root), {
				objective: "Repair a submission without rerunning research",
				workspaceRoot: root,
			});
			const task = await job.dispatchTask({
				stageId: "literature",
				stageExecutionId: "literature",
				role: "worker",
				objective: "Return a source-grounded review",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "literature",
				requiredOutputFields: ["synthesis"],
				acceptanceChecks: ["retain source refs"],
				failureSignals: ["missing source refs"],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read", "astra_search_papers"],
				writeAuthority: "none",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["retain source refs"],
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "submission-preflight");
			vi.stubEnv("ASTRA_FAKE_CODEX_INVALID_FINAL", invalidFinal === true ? "1" : "0");
			vi.stubEnv("ASTRA_FAKE_CODEX_RECEIPT", typeof invalidFinal === "string" ? invalidFinal : "");
			vi.stubGlobal("fetch", fetchAstraFixtureOpenAlex);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			if (invalidFinal === "unknown-receipt" || invalidFinal === "changed-source-receipt") {
				await expect(adapters.run(task, job)).rejects.toThrow("Unknown or inconsistent submission receipt");
			} else if (invalidFinal === true) {
				await expect(adapters.run(task, job)).rejects.toThrow("missing required output fields: synthesis");
			} else {
				const result = await adapters.run(task, job);
				expect(result.content).toEqual({ synthesis: "fixture synthesis" });
				expect(result.refs).toEqual(expect.arrayContaining(["openalex:W1", "openalex:W2", "openalex:W3"]));
				expect(Object.values(job.state.sessions)).toHaveLength(1);
				expect(Object.values(job.state.sessions)[0].status).toBe("completed");
				expect(Object.values(job.state.evidence)).toHaveLength(0);
			}
		},
	);
	it("constrains source-required submissions before host source validation", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Retain citations",
			workspaceRoot: root,
		});
		const task = await job.dispatchTask({
			stageId: "literature",
			stageExecutionId: "literature",
			role: "worker",
			objective: "Review existing sources",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "literature",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["retain sources"],
			failureSignals: ["missing sources"],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
			successCriteria: ["retain sources"],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		await expect(adapters.run(task, job)).rejects.toThrow("requested research schema");
	});
	it("gives a retry read-only access to its failed predecessor and explains submission recovery", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Recover completed work",
			workspaceRoot: root,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			responsibilityTransfers: [],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "invalid-output");
		await expect(adapters.run(task, job)).rejects.toThrow("JSON research result");
		await job.setTaskStatus(task.id, "failed");
		const previousRoot = join(root, ".astra", "jobs", task.jobId, "workspaces", task.id);
		await writeFile(join(previousRoot, "completed-data.csv"), "value\n1\n");
		const retry = await job.dispatchTask({
			...task,
			id: "retry",
			replayKey: "retry",
			attempt: 2,
			supersedesTaskId: task.id,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "retry-requests.jsonl"));
		await adapters.run(retry, job);
		const calls = (await readFile(join(root, "retry-requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		const start = calls.find((call) => call.method === "thread/start");
		expect(start.params.config["permissions.astra.filesystem"][previousRoot]).toBe("read");
		const turn = calls.find((call) => call.method === "turn/start");
		expect(turn.params.input[0].text).toContain("do not regenerate completed scientific data");
		expect(turn.params.input[0].text).toContain("JSON research result");
		expect(await readFile(join(previousRoot, "completed-data.csv"), "utf8")).toBe("value\n1\n");
	});
	it.runIf(process.platform === "linux" && existsSync("/etc/texmf") && existsSync("/var/lib/texmf"))(
		"mounts system TeX configuration read-only for compilation workers",
		async () => {
			const root = await workspace();
			const job = await ResearchJob.create(new JsonlAstraStore(root), {
				objective: "Compile a paper",
				workspaceRoot: root,
			});
			const task = await job.dispatchTask({
				stageId: "paper-compile",
				stageExecutionId: "compile",
				role: "worker",
				objective: "Compile the manuscript",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "paper-compile",
				requiredOutputFields: ["artifact"],
				acceptanceChecks: ["PDF exists"],
				failureSignals: ["missing PDF"],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read", "bash"],
				writeAuthority: "workspace-write",
				budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
				successCriteria: ["PDF exists"],
			});
			const cwd = join(root, ".astra", "jobs", task.jobId, "workspaces", task.id);
			await mkdir(cwd, { recursive: true });
			await writeFile(join(cwd, "build.mjs"), FIXTURE_PDF_SOURCE);
			const buildLog = execFileSync(process.execPath, ["build.mjs", "paper.pdf"], { cwd, encoding: "utf8" });
			await writeFile(join(cwd, "build.log"), buildLog);
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "ok");
			vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
			vi.stubEnv(
				"ASTRA_FAKE_CODEX_OUTPUT",
				JSON.stringify({
					artifactType: "paper-compile",
					contentJson: JSON.stringify({
						artifact: "paper.pdf",
						source: "build.mjs",
						buildInputs: ["build.mjs"],
						buildLog: "build.log",
						command: "node build.mjs paper.pdf",
					}),
					refs: [
						{ kind: "artifact", ref: "paper.pdf", summary: "Offline permission fixture" },
						{ kind: "artifact", ref: "build.mjs", summary: "Editable fixture source" },
						{ kind: "log", ref: "build.log", summary: "Fixture build output" },
					],
				}),
			);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			await adapters.run(task, job);
			const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line) as { method: string; params: { config?: Record<string, unknown> } });
			const filesystem = calls.find((call) => call.method === "thread/start")?.params.config?.[
				"permissions.astra.filesystem"
			] as Record<string, string>;
			expect(filesystem["/etc/texmf"]).toBe("read");
			expect(filesystem["/var/lib/texmf"]).toBe("read");
			expect(filesystem["/etc"]).toBeUndefined();
			expect(filesystem["/var/lib"]).toBeUndefined();
		},
	);
	it("gives reviewers read-only access to declared runtime results without other task directories", async () => {
		const root = await workspace();
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Inspect actual results",
			workspaceRoot: root,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			responsibilityTransfers: [],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		const output = await adapters.run(task, job);
		await job.setTaskStatus(task.id, "succeeded");
		const resourceRoot = taskResourcePath(root, task.jobId, task.id);
		const unrelatedRoot = taskResourcePath(root, task.jobId, "unrelated");
		await mkdir(resourceRoot, { recursive: true });
		await mkdir(unrelatedRoot, { recursive: true });
		await writeFile(join(resourceRoot, "results.csv"), "replicate,error\n0,0.1\n");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: { ...(output.content as object), runtimeResults: join(resourceRoot, "results.csv") },
			refs: output.refs,
		});
		const review = await adapters.review(evidence, job);
		const snapshot = JSON.parse(
			await readFile(
				join(root, ".astra", "jobs", task.jobId, "tasks", review.reviewerTaskId!, "review-target-snapshot.json"),
				"utf8",
			),
		);
		expect(snapshot.resources).toEqual(
			expect.arrayContaining([expect.objectContaining({ artifactId: evidence.id, root: resourceRoot })]),
		);
		expect(snapshot.targetEvidenceRef).toBe("review-target-evidence.json");
		expect(
			JSON.parse(
				await readFile(
					join(root, ".astra", "jobs", task.jobId, "tasks", review.reviewerTaskId!, snapshot.targetEvidenceRef),
					"utf8",
				),
			),
		).toEqual(evidence);
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map(
				(line) =>
					JSON.parse(line) as {
						method?: string;
						params?: { config?: Record<string, unknown>; developerInstructions?: string };
					},
			);
		const reviewerStart = calls.reverse().find((call) => call.method === "thread/start");
		expect(reviewerStart?.params?.developerInstructions).toContain("Astra Validation Stage");
		expect(reviewerStart?.params?.developerInstructions).toContain("Astra Reviewer Role");
		const filesystem = reviewerStart?.params?.config?.["permissions.astra.filesystem"] as Record<string, string>;
		expect(filesystem[resourceRoot]).toBe("read");
		expect(filesystem[unrelatedRoot]).toBeUndefined();
		expect(filesystem[join(root, ".astra", "jobs", task.jobId)]).toBeUndefined();
		expect(Object.values(filesystem)).not.toContain("write");
		expect(filesystem["/etc/texmf"]).toBeUndefined();
		expect(filesystem["/var/lib/texmf"]).toBeUndefined();
		await rm(resourceRoot, { recursive: true });
		await symlink(unrelatedRoot, resourceRoot);
		await expect(adapters.review(evidence, job)).rejects.toBeInstanceOf(NonRetryableResearchError);
		expect(
			Object.values(job.state.tasks)
				.filter((item) => item.role === "reviewer")
				.at(-1)?.status,
		).toBe("failed");
		const supervisor = new ResearchSupervisor(job, new JsonlAstraStore(root), {
			worker: adapters,
			reviewer: adapters,
			mainAgent: adapters,
			maxParallel: 1,
		});
		const before = Object.keys(job.state.tasks).length;
		await supervisor.tick();
		expect(job.state.paused).toBe(true);
		expect(Object.keys(job.state.tasks)).toHaveLength(before + 1);
		await supervisor.tick();
		expect(Object.keys(job.state.tasks)).toHaveLength(before + 1);
	});
	it("audits every Codex decision log and rejects missing, incomplete or misbound records", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, { objective: "Audit native decisions", workspaceRoot: root });
		const jobRoot = join(root, ".astra", "jobs", job.state.frame.jobId);
		await writeFile(join(jobRoot, "backend.json"), JSON.stringify({ backend: "codex" }));
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		await adapters.planStage(job);
		const firstLog = Object.values(job.state.sessions)[0].sessionFile!;
		const original = await readFile(firstLog, "utf8");
		await job.pause("test restart");
		await job.resume();
		await adapters.planStage(job);
		const audit = () =>
			JSON.parse(
				execFileSync(
					process.execPath,
					[
						fileURLToPath(new URL("../../../scripts/audit-astra-run.mjs", import.meta.url)),
						root,
						job.state.frame.jobId,
					],
					{ encoding: "utf8" },
				),
			);
		const valid = audit();
		expect(valid.runtimeIntegrity.codexSessionsVerified).toBe(true);
		expect(valid.runtimeIntegrity.recoveryCheckpointed).toBe(true);
		expect(valid.sessions.files).toBe(2);
		expect(valid.sessions.models).toEqual(["gpt-5.6-luna"]);
		expect(valid.passed).toBe(false); // A valid session alone does not complete research.
		for (const corrupt of [
			original.replaceAll('"modelProvider":"openai"', '"modelProvider":"untrusted"'),
			original.replaceAll('"permissionProfile":"astra"', '"permissionProfile":"unrestricted"'),
			original
				.split("\n")
				.filter((line) => !line.includes('"method":"turn/completed"'))
				.join("\n"),
		]) {
			await writeFile(firstLog, corrupt);
			expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(false);
		}
		await rm(firstLog);
		expect(audit().runtimeIntegrity.codexSessionsVerified).toBe(false);
	});
	it("indexes large evidence for main-agent decisions without truncating original state", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			objective: "Inspect evidence without repeatedly dumping it",
			workspaceRoot: root,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			responsibilityTransfers: [],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		await job.setTaskStatus(task.id, "succeeded");
		const content = { rawResults: "measurements\n".repeat(20000) };
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: "validation",
			content,
			refs: ["codex-session:fixture"],
		});
		const guidance = await job.recordUserGuidance("Preserve negative results and keep the fixed CPU-only scope");
		await adapters.decideEvidence(evidence, job);
		const contextRoot = join(root, ".astra", "jobs", task.jobId, "main-agent", "codex-context");
		const indexText = await readFile(join(contextRoot, "research-summary.json"), "utf8");
		expect(Buffer.byteLength(indexText)).toBeLessThan(20000);
		expect(JSON.parse(indexText)).toMatchObject({
			activeCapability: { id: "validation" },
			evidence: [{ id: evidence.id, taskId: task.id }],
			userGuidance: expect.arrayContaining([guidance]),
			fullStatePath: "research-context.json",
		});
		const full = JSON.parse(await readFile(join(contextRoot, "research-context.json"), "utf8"));
		expect(full.state.evidence[evidence.id].content).toEqual(content);
		expect(await readFile(join(root, "requests.jsonl"), "utf8")).toContain("Read research-focus.json first");
	});
	it.each(["openalex", "crossref", "codex-web"])(
		"completes all capabilities using %s with search continuation, executable repairs, paper delivery and durable review gates",
		async (provider) => {
			const root = await workspace();
			const store = new JsonlAstraStore(root);
			let job = await ResearchJob.create(store, {
				objective: "Offline full research contract",
				workspaceRoot: root,
				automation: "full",
				requiredArtifactTypes: ["paper-write", "paper-compile"],
				maxTasks: 128,
				maxTurns: 256,
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "full-research");
			vi.stubEnv("ASTRA_FAKE_CODEX_WEB", provider === "codex-web" ? "1" : "0");
			vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
			const fetcher = vi.fn(async (input: string | URL) => {
				if (provider === "codex-web") return new Response("metadata outage", { status: 503 });
				if (provider === "openalex") return fetchAstraFixtureOpenAlex(input);
				if (new URL(input).hostname === "api.openalex.org") return new Response("quota exhausted", { status: 429 });
				if (new URL(input).hostname !== "api.crossref.org") throw new Error("Unexpected literature provider");
				return Response.json({
					message: {
						items: [1, 2, 3].map((index) => ({
							DOI: `10.1000/paper-${index}`,
							title: [`Fixture paper ${index}`],
						})),
					},
				});
			});
			vi.stubGlobal("fetch", fetcher);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			for (let tick = 0; tick < 60 && job.state.frame.status !== "completed"; tick++) {
				const supervisor = new ResearchSupervisor(job, store, {
					worker: adapters,
					reviewer: adapters,
					mainAgent: adapters,
					maxParallel: 1,
				});
				await supervisor.tick();
				expect(
					job.state.paused,
					JSON.stringify({
						frame: job.state.frame,
						sessions: Object.values(job.state.sessions).filter((session) => session.status === "failed"),
					}),
				).toBe(false);
				job = (await ResearchJob.open(store, job.state.frame.jobId))!;
			}
			expect(
				job.state.frame.status,
				JSON.stringify({ frame: job.state.frame, blockers: job.completionBlockers() }),
			).toBe("completed");
			expect(job.completionBlockers()).toEqual([]);
			expect(fetcher).toHaveBeenCalledTimes(provider === "openalex" ? 1 : provider === "crossref" ? 2 : 3);
			expect(Object.keys(job.state.canonicalRoute.stageArtifactIds)).toHaveLength(14);
			expect(
				Object.values(job.state.searchBatches)
					.filter((batch) => batch.stageId === "idea")
					.map((batch) => batch.round)
					.sort(),
			).toEqual([1, 2]);
			expect(Object.keys(job.state.discardedCandidates).length).toBeGreaterThan(0);
			expect(Object.values(job.state.obligations).some((obligation) => obligation.status === "resolved")).toBe(true);
			for (const stage of ["result-to-claim", "research-review"]) {
				const artifact = job.state.canonical[job.state.canonicalRoute.stageArtifactIds[stage]];
				expect(
					Object.values(job.state.reviews).filter(
						(review) => review.evidenceId === artifact.evidenceId && review.verdict === "pass",
					),
				).toHaveLength(2);
			}
			await writeFile(
				join(root, ".astra", "jobs", job.state.frame.jobId, "backend.json"),
				JSON.stringify({ backend: "codex" }),
			);
			const audit = JSON.parse(
				execFileSync(
					process.execPath,
					[
						fileURLToPath(new URL("../../../scripts/audit-astra-run.mjs", import.meta.url)),
						root,
						job.state.frame.jobId,
					],
					{ encoding: "utf8" },
				),
			);
			expect(audit.passed, JSON.stringify({ checks: audit.contractChecks, sessions: audit.sessions })).toBe(true);
		},
		60000,
	);
	it("resumes retrieved sources and a rate-limited reviewer without creating another task", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			objective: "Resume verifiable work",
			workspaceRoot: root,
			automation: "full",
		});
		vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
		vi.stubGlobal("fetch", fetchAstraFixtureOpenAlex);
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const plan = await adapters.planStage(job);
		const task = await job.dispatchTask({
			...plan.tasks[0],
			responsibilityTransfers: [],
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		const sourceTask = { ...task, allowedTools: [...task.allowedTools, "astra_search_papers"] };
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "literature-interrupt");
		await expect(adapters.run(sourceTask, job)).rejects.toThrow("test provider refusal");
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "literature-resume");
		const output = await adapters.run(sourceTask, job);
		expect(output.refs).toContain("openalex:W1");
		const unrelated = await job.dispatchTask({
			...sourceTask,
			id: "unrelated_sources",
			replayKey: "unrelated_sources",
		});
		await expect(adapters.run(unrelated, job)).rejects.toThrow("not retrieved in this task");
		await job.setTaskStatus(task.id, "running");
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: task.requiredOutputType,
			content: output.content,
			refs: output.refs,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "rate-limit");
		await expect(adapters.review(evidence, job)).rejects.toThrow("test provider refusal");
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const reviewed = await adapters.review(evidence, job);
		expect(reviewed.verdict).toBe("pass");
		expect(Object.values(job.state.tasks).filter((task) => task.role === "reviewer")).toHaveLength(1);
	});
	it.each(["valid", "foreign", "paraphrased"])(
		"governs all role sessions and validates %s reviews",
		async (reviewMode) => {
			const root = await workspace();
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				objective: "Bound a scientific question",
				workspaceRoot: root,
				automation: "full",
			});
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
			vi.stubEnv("ASTRA_CODEX_MODEL", "gpt-5.6-luna");
			vi.stubEnv("ASTRA_FAKE_CODEX_FOREIGN_REVIEW_REF", reviewMode === "foreign" ? "1" : "0");
			vi.stubEnv("ASTRA_FAKE_CODEX_PARAPHRASE", reviewMode === "paraphrased" ? "1" : "0");
			vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			const supervisor = new ResearchSupervisor(job, store, {
				worker: adapters,
				reviewer: adapters,
				mainAgent: adapters,
				maxParallel: 1,
			});
			await supervisor.tick();
			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			if (reviewMode !== "valid") {
				expect(reopened?.state.paused).toBe(true);
				expect(Object.keys(job.state.canonical)).toHaveLength(0);
				expect(Object.values(job.state.sessions).find((session) => session.role === "reviewer")).toMatchObject({
					status: "failed",
					error: "Codex final output does not match the requested research schema",
				});
				return;
			}
			expect(reopened?.state.frame.userGate).toMatchObject({
				kind: "research",
				question: "Which benchmark should define the scientific scope?",
			});
			expect(Object.keys(job.state.canonical)).toHaveLength(1);
			const sessions = Object.values(job.state.sessions);
			expect(sessions.map((session) => session.role).sort()).toEqual([
				"main-agent",
				"reviewer",
				"reviewer",
				"worker",
			]);
			expect(sessions.every((session) => session.status === "completed")).toBe(true);
			expect(new Set(sessions.map((session) => session.sessionId)).size).toBe(4);
			const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line) as { method?: string; params?: { model?: string } });
			expect(calls.filter((call) => call.method === "thread/start")).toHaveLength(4);
			expect(calls.filter((call) => call.method === "thread/resume")).toHaveLength(3);
			expect(
				calls
					.filter((call) => call.method === "thread/start" || call.method === "thread/resume")
					.every((call) => call.params?.model === "gpt-5.6-luna"),
			).toBe(true);
			const artifact = Object.values(job.state.canonical)[0];
			expect(job.state.evidence[artifact.evidenceId].refs[0]).toMatch(/^codex-session:/);
			const reviewer = Object.values(job.state.tasks).find((task) => task.role === "reviewer");
			expect(reviewer?.status).toBe("succeeded");
		},
	);

	it("persists the backend and resumes it without changing to Pi", async () => {
		const root = await workspace();
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, { objective: "backend identity", workspaceRoot: root });
		await mkdir(join(root, ".astra"), { recursive: true });
		await writeFile(join(root, ".astra", "active-job.json"), JSON.stringify({ jobId: job.state.frame.jobId }));
		await writeFile(
			join(root, ".astra", "jobs", job.state.frame.jobId, "backend.json"),
			JSON.stringify({ backend: "codex" }),
		);
		expect(await researchBackend({ action: "resume" }, root)).toBe("codex");
		await expect(researchBackend({ action: "resume", backend: "pi" }, root)).rejects.toThrow("Cannot change");
		const status = await runResearchControl({ action: "status" }, root);
		expect(status).toMatchObject({ backend: "codex", costAccounting: "subscription-unavailable" });
		vi.spyOn(process, "cwd").mockReturnValue(root);
		const printed = vi.spyOn(console, "log").mockImplementation(() => {});
		await runAstra(["research", "status"]);
		expect(codingAgent.main).not.toHaveBeenCalled();
		expect(JSON.parse(String(printed.mock.calls[0][0]))).toMatchObject({ backend: "codex" });
	});

	it("preserves Pi's normal launcher modes", async () => {
		await runAstra(["--help"]);
		expect(codingAgent.main).toHaveBeenCalledWith(
			["--help"],
			expect.objectContaining({ extensionFactories: expect.any(Array) }),
		);
	});

	it("parses explicit backend selection and rejects an unsupported dollar limit before creating work", async () => {
		expect(parseResearchControlArgs(["run", "--backend", "codex", "question"])).toEqual({
			action: "run",
			backend: "codex",
			objective: "question",
		});
		expect(() => parseResearchControlArgs(["run", "--backend", "unknown", "question"])).toThrow(
			"unknown Astra backend",
		);
		await expect(
			runResearchControl(
				{ action: "run", backend: "codex", objective: "question", maxCostUsd: 10 },
				await workspace(),
			),
		).rejects.toThrow("USD bill");
	});
});
