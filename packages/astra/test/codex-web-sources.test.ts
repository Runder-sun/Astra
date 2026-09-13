import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { recordCodexWebSources } from "../src/codex-web-sources.ts";
import { sourceReceiptFilename } from "../src/literature.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
describe("official Codex search provenance", () => {
	it("accepts returned results but never an unobserved open-page URL or model citation", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-web-sources-"));
		roots.push(root);
		const options = { workspaceRoot: root, jobId: "job_web", query: "robust estimates", limit: 3 };
		expect(
			await recordCodexWebSources(options, {
				id: "open-failed",
				action: { type: "openPage", url: "https://arxiv.org/abs/9999.12345" },
				results: null,
			}),
		).toEqual([]);
		const records = await recordCodexWebSources(options, {
			id: "search",
			type: "webSearch",
			query: "robust estimates",
			results: [
				{
					type: "text_result",
					url: "https://arxiv.org/abs/2301.12345v2",
					title: "A real result",
					snippet: "Search excerpt",
					ref_id: "turn0search0",
				},
				{
					type: "text_result",
					url: "https://arxiv.org/pdf/2301.12345",
					title: "Same paper PDF",
					ref_id: "turn0search1",
				},
				{ type: "text_result", url: "file:///private", title: "Not a web result" },
				{ type: "other", url: "https://example.org/forged", title: "Unknown result type" },
			],
		});
		expect(records).toHaveLength(1);
		expect(records[0]).toMatchObject({
			sourceRef: "arxiv:2301.12345",
			retrievalLevel: "web-search-result",
			snippet: "Search excerpt",
		});
		expect(records[0].abstract).toBeUndefined();
		const receipt = JSON.parse(
			await readFile(
				join(root, ".astra/jobs/job_web/sources", sourceReceiptFilename(records[0].sourceRef)!),
				"utf8",
			),
		);
		expect(receipt).toMatchObject({
			provider: "codex-web",
			observation: { itemId: "search", result: { ref_id: "turn0search0" } },
		});
		await recordCodexWebSources(options, {
			id: "open-page",
			results: [
				{
					type: "text_result",
					url: "https://arxiv.org/abs/2301.12345",
					title: "A real result",
					snippet: "Total lines: 164",
				},
			],
		});
		const retained = JSON.parse(
			await readFile(
				join(root, ".astra/jobs/job_web/sources", sourceReceiptFilename(records[0].sourceRef)!),
				"utf8",
			),
		);
		expect(retained.record.snippet).toBe("Search excerpt");
	});
});
