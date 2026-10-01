import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it.each(["paraphrased-review-receipt", "unknown-review-receipt", "changed-review-receipt"])(
	"corrects only a valid final confirmation within the same review session (%s)",
	async (mode) => {
		const root = await mkdtemp(join(tmpdir(), "astra-review-recovery-"));
		roots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "Recover confirmation",
			workspaceRoot: root,
		});
		const task = await job.dispatchTask({
			stageId: "validation",
			stageExecutionId: "validation",
			role: "worker",
			objective: "Provide evidence",
			inputArtifactRefs: [],
			requiredCanonicalArtifacts: [],
			requiredOutputType: "validation",
			requiredOutputFields: ["content"],
			acceptanceChecks: ["verify source"],
			successCriteria: [],
			failureSignals: [],
			dependencies: [],
			scope: { workspaceRoot: root, allowedPaths: ["."] },
			allowedTools: ["read"],
			writeAuthority: "none",
			budget: { maxTurns: 4, maxToolCalls: 8, maxRuntimeMs: 30000 },
			reviewGateRequired: true,
			resumePolicy: "resume-session",
		});
		await job.setTaskStatus(task.id, "succeeded");
		const evidence = await job.recordEvidence({
			taskId: task.id,
			stageId: task.stageId,
			type: "validation",
			content: {},
			refs: [],
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "review-preflight");
		vi.stubEnv("ASTRA_FAKE_CODEX_REVIEW_RECEIPT", mode);
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({
				executable: process.execPath,
				prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
			}),
		);
		if (mode === "paraphrased-review-receipt") {
			const result = await adapters.review(evidence, job);
			expect(result.verdict).toBe("partial");
			expect(result.findings).toEqual(["Original verified missing evidence finding"]);
			expect(result.criteria?.[0].passed).toBe(false);
			await job.recordReview({ ...result, evidenceId: evidence.id });
			expect(Object.values(job.state.obligations)).toHaveLength(1);
			expect(job.state.evidence[evidence.id].status).toBe("candidate");
		} else {
			await expect(adapters.review(evidence, job)).rejects.toThrow("Unknown or inconsistent review receipt");
			expect(Object.keys(job.state.reviews)).toHaveLength(0);
		}
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		const turns = calls.filter((call) => call.method === "turn/start");
		expect(turns).toHaveLength(2);
		expect(turns[0].params.threadId).toBe(turns[1].params.threadId);
		expect(turns[1].params.input[0].text).toContain("Astra final submission rejected:");
		expect(Object.values(job.state.tasks).filter((entry) => entry.role === "reviewer")).toHaveLength(1);
		expect(Object.values(job.state.sessions)).toHaveLength(1);
	},
);
