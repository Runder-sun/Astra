import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { sha256 } from "../src/contracts.ts";
import { FIXTURE_PDF_SOURCE } from "../src/fixture-pdf.ts";
import { type Fetcher, readSourceRecord, sourceReceiptFilename, writeSourceReceipt } from "../src/literature.ts";
import { captureSourcePage } from "../src/source-capture.ts";

const roots: string[] = [];
afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function setup() {
	const workspaceRoot = await mkdtemp(join(tmpdir(), "astra-source-capture-"));
	roots.push(workspaceRoot);
	const options = { workspaceRoot, jobId: "job_capture", query: "original query", limit: 1 };
	await writeSourceReceipt(
		options,
		{
			sourceRef: "arxiv:2301.12345",
			title: "Paper",
			authors: [],
			landingPageUrl: "https://arxiv.org/abs/2301.12345v2",
			retrievalLevel: "web-search-result",
		},
		"codex-web",
		"2026-01-01T00:00:00Z",
		{ snippet: "Abstract only" },
	);
	return { ...options, sourceRef: "arxiv:2301.12345", url: "https://arxiv.org/html/2301.12345v2" };
}

it("retains fetched page contents and provenance separately from the original search receipt", async () => {
	const options = await setup();
	const content = "<html><body>Methods and limitations</body></html>";
	const fetcher = vi.fn(async () => new Response(content, { headers: { "content-type": "text/html" } }));
	const result = await captureSourcePage({ ...options, fetcher });
	expect(fetcher.mock.calls[0]).toBeDefined();
	const { sha256: digest, ...receipt } = JSON.parse(await readFile(result.path, "utf8"));
	expect(digest).toBe(sha256(JSON.stringify(receipt)));
	expect(receipt.observation).toMatchObject({
		sourceRef: options.sourceRef,
		url: options.url,
		content,
		contentSha256: sha256(content),
	});
	expect(await readSourceRecord(options.workspaceRoot, options.jobId, result.sourceRef)).toMatchObject({
		retrievalLevel: "source-page",
	});
	const original = JSON.parse(
		await readFile(
			join(options.workspaceRoot, ".astra/jobs/job_capture/sources", sourceReceiptFilename(options.sourceRef)!),
			"utf8",
		),
	);
	expect(original.query).toBe("original query");
	expect(original.observation.snippet).toBe("Abstract only");
});

it("rejects unrecorded sources and other origins before fetching", async () => {
	const options = await setup();
	const fetcher = vi.fn();
	await expect(captureSourcePage({ ...options, sourceRef: "arxiv:9999.12345", fetcher })).rejects.toThrow("recorded");
	await expect(captureSourcePage({ ...options, url: "https://localhost/private", fetcher })).rejects.toThrow("origin");
	expect(fetcher).not.toHaveBeenCalled();
});

it("follows bounded publisher redirects and records the final text URL", async () => {
	const options = await setup();
	const fetcher = vi.fn(async (url: string | URL) =>
		String(url).includes("arxiv.org")
			? new Response(null, { status: 302, headers: { location: "http://publisher.example/paper" } })
			: new Response("Original text", { headers: { "content-type": "text/plain" } }),
	);
	const result = await captureSourcePage({ ...options, fetcher });
	expect(String(fetcher.mock.calls[1][0])).toBe("https://publisher.example/paper");
	const receipt = JSON.parse(await readFile(result.path, "utf8"));
	expect(receipt.observation.url).toBe("https://publisher.example/paper");
	expect(receipt.observation.redirects).toHaveLength(1);
	expect(receipt.observation.content).toBe("Original text");
});

it.skipIf(!existsSync("/usr/bin/pdftotext"))(
	"preserves original PDF bytes with independently extracted page text",
	async () => {
		const options = await setup();
		const script = join(options.workspaceRoot, "fixture.mjs");
		const pdfPath = join(options.workspaceRoot, "fixture.pdf");
		await writeFile(script, FIXTURE_PDF_SOURCE);
		execFileSync(process.execPath, [script, pdfPath]);
		const pdf = await readFile(pdfPath);
		const result = await captureSourcePage({
			...options,
			fetcher: async () => new Response(pdf, { headers: { "content-type": "application/pdf" } }),
		});
		const receipt = JSON.parse(await readFile(result.path, "utf8"));
		expect(Buffer.from(receipt.observation.originalBase64, "base64")).toEqual(pdf);
		expect(receipt.observation.content).toContain("Offline Astra fixture - not research evidence");
		expect(receipt.observation.extraction).toBe("pdftotext -layout");
	},
);

it("rejects private redirects, malformed PDFs and oversized pages without creating a capture", async () => {
	const options = await setup();
	for (const response of [
		new Response(null, { status: 302, headers: { location: "http://localhost/private" } }),
		new Response("binary", { headers: { "content-type": "application/pdf" } }),
		new Response("x".repeat(4 * 1024 * 1024 + 1), { headers: { "content-type": "text/plain" } }),
	]) {
		const fetcher = vi.fn(async (_url: string | URL, _init?: Parameters<Fetcher>[1]) => response);
		await expect(
			captureSourcePage({
				...options,
				fetcher,
			}),
		).rejects.toThrow();
		expect(fetcher.mock.calls[0][1]?.redirect).toBe("manual");
	}
});

it("stops redirect loops after five hops", async () => {
	const options = await setup();
	const fetcher = vi.fn(async () => new Response(null, { status: 302, headers: { location: options.url } }));
	await expect(captureSourcePage({ ...options, fetcher })).rejects.toThrow("five hops");
	expect(fetcher).toHaveBeenCalledTimes(6);
});
