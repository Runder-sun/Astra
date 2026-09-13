import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { compactLiteratureSearch, searchOpenAlex } from "../src/literature.ts";

const tempRoots: string[] = [];

afterEach(async () => {
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("governed literature retrieval", () => {
	it("stops reading an oversized response instead of buffering its remaining body", async () => {
		let reads = 0;
		let cancelled = false;
		const root = await mkdtemp(join(tmpdir(), "astra-response-bound-"));
		tempRoots.push(root);
		const stream = new ReadableStream<Uint8Array>({
			pull(controller) {
				reads++;
				controller.enqueue(new Uint8Array(3 * 1024 * 1024));
				if (reads === 8) controller.close();
			},
			cancel() {
				cancelled = true;
			},
		});
		await expect(
			searchOpenAlex({
				query: "test bounds",
				limit: 1,
				workspaceRoot: root,
				jobId: "job_bounds",
				fetcher: async () => new Response(stream),
			}),
		).rejects.toThrow("exceeded 4 MiB");
		expect(reads).toBeLessThan(8);
		expect(cancelled).toBe(true);
	});
	it("compacts model-facing results without changing persisted source records", () => {
		const compact = compactLiteratureSearch({
			query: "lookup",
			retrievedAt: "2026-01-01T00:00:00.000Z",
			total: 10,
			results: Array.from({ length: 5 }, (_, index) => ({
				sourceRef: `openalex:W${index}`,
				openAlexId: `W${index}`,
				title: `Paper ${index}`,
				authors: [],
				abstract: "x".repeat(500),
			})),
		});

		expect(compact.results).toHaveLength(3);
		expect(compact.results[0].abstract).toHaveLength(240);
		expect(compact.total).toBe(10);
	});

	it("returns normalized primary-source records and persists provenance cache", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-openalex-"));
		tempRoots.push(root);
		const fetcher = vi.fn(
			async () =>
				new Response(
					JSON.stringify({
						meta: { count: 1 },
						results: [
							{
								id: "https://openalex.org/W123",
								doi: "https://doi.org/10.1000/test",
								title: "A traceable paper",
								publication_year: 2025,
								type: "article",
								cited_by_count: 7,
								authorships: [{ author: { display_name: "Ada Researcher" } }],
								abstract_inverted_index: { A: [0], result: [1] },
								primary_location: { landing_page_url: "https://doi.org/10.1000/test", pdf_url: null },
							},
						],
					}),
					{ status: 200, headers: { "content-type": "application/json" } },
				),
		);

		const result = await searchOpenAlex({
			query: "autonomous scientific research agent",
			limit: 5,
			workspaceRoot: root,
			jobId: "job_literature",
			fetcher,
		});

		expect(fetcher).toHaveBeenCalledOnce();
		expect(result.results[0]).toMatchObject({
			sourceRef: "openalex:W123",
			doi: "10.1000/test",
			title: "A traceable paper",
			abstract: "A result",
		});
		const receipt = JSON.parse(
			await readFile(join(root, ".astra/jobs/job_literature/sources/openalex-W123.json"), "utf8"),
		);
		expect(receipt).toMatchObject({ sourceRef: "openalex:W123", query: "autonomous scientific research agent" });
		expect(receipt.sha256).toMatch(/^[a-f0-9]{64}$/);
	});

	it("rejects empty queries before issuing a network request", async () => {
		const fetcher = vi.fn();
		await expect(
			searchOpenAlex({ query: " ", limit: 5, workspaceRoot: "/tmp", jobId: "job_query", fetcher }),
		).rejects.toThrow("query");
		expect(fetcher).not.toHaveBeenCalled();
	});
});
