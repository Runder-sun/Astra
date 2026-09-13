import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { sourceReceiptFilename } from "../src/literature.ts";
import { searchLiterature } from "../src/literature-search.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});
async function options() {
	const workspaceRoot = await mkdtemp(join(tmpdir(), "astra-fallback-"));
	roots.push(workspaceRoot);
	return { workspaceRoot, jobId: "job_sources", query: "robust estimation", limit: 2 };
}
const crossref = () =>
	Response.json({
		message: {
			"total-results": 2,
			items: [
				{
					DOI: "10.1000/ONE",
					title: ["First paper"],
					author: [{ given: "Ada", family: "Researcher" }],
					published: { "date-parts": [[2025, 1]] },
				},
				{ DOI: "10.1000/two", title: ["Second paper"] },
			],
		},
	});

describe("literature fallback and reuse", () => {
	it("coalesces concurrent identical searches and refuses a cache with a missing or altered receipt", async () => {
		const args = await options();
		const fetcher = vi.fn(async (input: string | URL) =>
			new URL(input).hostname === "api.openalex.org" ? new Response("quota", { status: 429 }) : crossref(),
		);
		const [first, second] = await Promise.all([
			searchLiterature({ ...args, fetcher }),
			searchLiterature({ ...args, fetcher }),
		]);
		expect(first).toEqual(second);
		expect(fetcher).toHaveBeenCalledTimes(2);
		const receiptPath = join(
			args.workspaceRoot,
			".astra/jobs/job_sources/sources",
			sourceReceiptFilename("doi:10.1000/one")!,
		);
		await writeFile(
			receiptPath,
			JSON.stringify({ sourceRef: "doi:10.1000/one", record: { title: "Altered paper" }, sha256: "wrong" }),
		);
		const refreshed = await searchLiterature({ ...args, fetcher });
		expect(refreshed.cacheHit).toBe(false);
		expect(refreshed.results[0].title).toBe("First paper");
		expect(fetcher).toHaveBeenCalledTimes(3);
		await rm(receiptPath);
		expect((await searchLiterature({ ...args, fetcher })).cacheHit).toBe(false);
		expect(fetcher).toHaveBeenCalledTimes(4);
	});
	it("continues after OpenAlex quota exhaustion and reuses the original dated search", async () => {
		const args = await options();
		const fetcher = vi.fn(async (input: string | URL) =>
			new URL(input).hostname === "api.openalex.org"
				? new Response("quota", { status: 429, headers: { "Retry-After": "3600" } })
				: crossref(),
		);
		const first = await searchLiterature({ ...args, fetcher });
		expect(first.results.map((record) => record.sourceRef)).toEqual(["doi:10.1000/one", "doi:10.1000/two"]);
		expect(first.attempts).toContainEqual(expect.objectContaining({ provider: "openalex", status: "failed" }));
		const receipt = JSON.parse(
			await readFile(
				join(args.workspaceRoot, ".astra/jobs/job_sources/sources", sourceReceiptFilename("doi:10.1000/one")!),
				"utf8",
			),
		);
		expect(receipt).toMatchObject({ provider: "crossref", record: { authors: ["Ada Researcher"], year: 2025 } });
		const cached = await searchLiterature({ ...args, fetcher });
		expect(cached.cacheHit).toBe(true);
		expect(cached.retrievedAt).toBe(first.retrievedAt);
		expect(fetcher).toHaveBeenCalledTimes(2);
		await searchLiterature({ ...args, query: "another query", fetcher });
		expect(fetcher).toHaveBeenCalledTimes(3);
	});
	it("deduplicates DOI aliases and fills a partial result with parsed arXiv metadata", async () => {
		const args = await options();
		const fetcher = vi.fn(async (input: string | URL) => {
			const host = new URL(input).hostname;
			if (host === "api.openalex.org")
				return Response.json({
					results: [{ id: "https://openalex.org/W1", title: "First paper", doi: "https://doi.org/10.1000/ONE" }],
				});
			if (host === "api.crossref.org")
				return Response.json({ message: { items: [{ DOI: "10.1000/one", title: ["First paper"] }] } });
			return new Response(
				'<feed xmlns="http://www.w3.org/2005/Atom"><entry><id>http://arxiv.org/abs/2301.12345v2</id><title>Second &amp; useful</title><summary><![CDATA[Evidence < assumptions]]></summary><published>2023-01-20T00:00:00Z</published><author><name>Grace Researcher</name></author></entry></feed>',
			);
		});
		const result = await searchLiterature({ ...args, fetcher });
		expect(result.results).toHaveLength(2);
		expect(result.results[1]).toMatchObject({
			sourceRef: "arxiv:2301.12345",
			title: "Second & useful",
			abstract: "Evidence < assumptions",
			authors: ["Grace Researcher"],
			year: 2023,
		});
	});
	it("returns explicit channel failures without manufacturing papers or caching failure as success", async () => {
		const args = await options();
		const fetcher = vi.fn(async () => new Response("unavailable", { status: 503 }));
		const result = await searchLiterature({ ...args, fetcher });
		expect(result.results).toEqual([]);
		expect(result.attempts).toHaveLength(3);
		expect(result.attempts.every((attempt) => attempt.status === "failed")).toBe(true);
		expect(result.cacheHit).toBe(false);
	});
	it("rejects invalid input before network or cache access", async () => {
		const args = await options();
		const fetcher = vi.fn();
		await expect(searchLiterature({ ...args, query: " ", fetcher })).rejects.toThrow("query");
		await expect(searchLiterature({ ...args, jobId: "../escape", fetcher })).rejects.toThrow();
		expect(fetcher).not.toHaveBeenCalled();
	});
});
