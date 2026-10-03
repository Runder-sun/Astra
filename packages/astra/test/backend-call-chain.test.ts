import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { taskDir } from "../src/contracts.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { prepareTaskWorkspace, taskWorkspacePath } from "../src/task-workspace.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function recoveryFixture() {
	const root = await mkdtemp(join(tmpdir(), "astra-recovery-validation-"));
	roots.push(root);
	const store = new MemoryAstraStore();
	const job = await ResearchJob.create(store, { workspaceRoot: root, objective: "validate retry lineage" });
	const prior = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "same bounded task",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["verified"],
		failureSignals: [],
		successCriteria: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 100 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
	});
	await prepareTaskWorkspace(prior, job);
	await job.setTaskStatus(prior.id, "failed");
	const retry = await job.dispatchTask({ ...prior, id: "retry_validation", attempt: 2, supersedesTaskId: prior.id });
	return { root, store, job, prior, retry };
}

it.each([true, false])(
	"C6 Codex recovery prompt and read roots share validated history (foreign=%s)",
	async (foreign) => {
		const { root, job, prior, retry } = await recoveryFixture();
		const outside = join(root, "unrelated-private-log.jsonl");
		await writeFile(outside, "unrelated role");
		if (foreign)
			await job.recordChildSession({
				sessionId: "wrong-role",
				taskId: prior.id,
				role: "main-agent",
				attempt: 99,
				status: "failed",
				sessionFile: outside,
				updatedAt: "2026-10-01T00:00:00Z",
				error: "foreign",
			});
		const valid = join(taskDir(root, prior.jobId, prior.id), "failure-log.json");
		await writeFile(valid, "validated predecessor");
		await job.recordChildSession({
			sessionId: "valid-worker",
			taskId: prior.id,
			role: "worker",
			attempt: prior.attempt,
			status: "failed",
			sessionFile: valid,
			updatedAt: "2026-10-03T00:00:00Z",
			error: "bounded failure",
		});
		const runner = new CodexAppServerRunner();
		const run = vi.spyOn(runner, "run").mockRejectedValue(new Error("offline boundary"));
		await expect(new CodexResearchAdapters(runner).run(retry, job)).rejects.toThrow("offline boundary");
		const options = run.mock.calls[0][0];
		expect(options.readRoots).toContain(valid);
		expect(options.readRoots).not.toContain(outside);
		expect(options.prompt).toContain(valid);
		expect(options.prompt).not.toContain(outside);
		expect(options.prompt).toContain("bounded failure");
		const context = await readFile(
			join(taskWorkspacePath(root, retry.jobId, retry.id), "ASTRA_TASK_CONTEXT.json"),
			"utf8",
		);
		expect(context).toContain(valid);
		expect(context).not.toContain(outside);
	},
);

it.each(["message", "completion", "pre-response"])(
	"C7 actual reviewer registers the corrected turn despite %s ordering",
	async (kind) => {
		const { root, job, retry } = await recoveryFixture();
		await job.setTaskStatus(retry.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: retry.id,
			stageId: retry.stageId,
			type: "validation",
			content: { content: "offline" },
			refs: [],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", `turn-identity-${kind}`);
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const runner = new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		});
		const submitted = await new CodexResearchAdapters(runner).review(evidence, job);
		expect(submitted.verdict).toBe("partial");
		expect(submitted.findings).toEqual(["corrected"]);
		const registered = await job.recordReview({ ...submitted, evidenceId: evidence.id });
		expect(registered.verdict).toBe("partial");
		const session = Object.values(job.state.sessions).find((row) => row.taskId === submitted.reviewerTaskId)!;
		expect(session.status).toBe("completed");
		expect(session.manifestRef).toBeDefined();
		const requests = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		expect(requests.filter((row) => row.method === "turn/start")).toHaveLength(2);
	},
);

it.each([false, true])(
	"C2.3 actual Codex retry accepts default attempt 1 after durable rejection (same key API recovery=%s)",
	async (sameKey) => {
		const { job, retry } = await recoveryFixture();
		await job.setTaskStatus(retry.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: retry.id,
			stageId: retry.stageId,
			type: "validation",
			content: { content: "offline" },
			refs: [],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		const runner = new CodexAppServerRunner({
			executable: process.execPath,
			prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
		});
		const adapter = new CodexResearchAdapters(runner);
		const first = await adapter.review(evidence, job);
		const failed = job.state.tasks[first.reviewerTaskId!];
		const session = Object.values(job.state.sessions).find((row) => row.taskId === failed.id)!;
		const manifestPath = session.manifestRef!;
		// Fault only the assessment, keeping the generated delivery's package identity and files.
		const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
		manifest.criteria = [...manifest.criteria, manifest.criteria[0]];
		const invalidBytes = JSON.stringify(manifest);
		await writeFile(manifestPath, invalidBytes);
		await job.recoverReviewerTaskCompletions(evidence.id);
		expect(job.state.tasks[failed.id].status).toBe("failed");
		expect(job.state.sessions[session.sessionId].status).toBe("failed");
		expect(Object.keys(job.state.reviewDeliveryRejections ?? {})).toEqual([failed.id]);
		if (sameKey) {
			const { id: _id, agentId: _agentId, ...declaration } = failed;
			const next = await job.dispatchTask({ ...declaration, attempt: undefined });
			expect(next.id).not.toBe(failed.id);
			expect(next.replayKey).toBe(failed.replayKey);
			expect(next.attempt).toBe(1);
			await job.recordChildSession({
				sessionId: "recover-default-attempt",
				taskId: next.id,
				role: "reviewer",
				attempt: 1,
				status: "interrupted",
				updatedAt: new Date().toISOString(),
			});
		}
		const second = await adapter.review(evidence, job);
		expect(second.reviewerTaskId).not.toBe(failed.id);
		const next = job.state.tasks[second.reviewerTaskId!];
		expect(next.attempt).toBe(1);
		expect(next.replayKey === failed.replayKey).toBe(sameKey);
		await job.recordReview({ ...second, evidenceId: evidence.id });
		await job.recoverReviewerTaskCompletions(evidence.id);
		expect(job.state.tasks[next.id].status).toBe("succeeded");
		expect(await readFile(manifestPath, "utf8")).toBe(invalidBytes);
	},
);
