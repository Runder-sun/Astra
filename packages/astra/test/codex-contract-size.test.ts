import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it.each(["decompose", "repair", "search"] as const)(
	"bounds generated %s plans by executable task counts",
	async (mode) => {
		const root = await mkdtemp(join(tmpdir(), "astra-contract-size-"));
		roots.push(root);
		const job = await ResearchJob.create(new JsonlAstraStore(root), {
			objective: "bounded contracts",
			workspaceRoot: root,
		});
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "research");
		vi.stubEnv("ASTRA_FAKE_CODEX_LOG", join(root, "requests.jsonl"));
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({
				executable: process.execPath,
				prefixArgs: [fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url))],
			}),
		);
		const pending = adapters.planStage(job, undefined, mode);
		// The fixture deliberately returns one task, which must be rejected for search.
		if (mode === "search") await expect(pending).rejects.toThrow("schema");
		const plan = mode === "search" ? undefined : await pending;
		const calls = (await readFile(join(root, "requests.jsonl"), "utf8"))
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		const schema = calls.find((call) => call.method === "turn/start").params.outputSchema.properties.tasks;
		expect(schema.minItems).toBe(mode === "search" ? 2 : 1);
		expect(schema.maxItems).toBe(mode === "repair" ? 1 : mode === "search" ? 4 : 2);
		if (!plan) return;
		const evidence = await preparePlanEvidence(job, plan);
		const review = await adapters.review(evidence, job);
		const dir = join(root, ".astra", "jobs", job.state.frame.jobId, "tasks", review.reviewerTaskId!);
		const criteria = JSON.parse(await readFile(join(dir, "review-criteria.json"), "utf8"));
		expect(criteria).toEqual(review.criteria?.map((item) => item.criterion));
	},
);
