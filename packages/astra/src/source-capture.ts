import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { isIP } from "node:net";
import { join } from "node:path";
import { promisify } from "node:util";
import { sha256 } from "./contracts.ts";
import {
	type Fetcher,
	literatureFetch,
	readLiteratureBytes,
	readSourceRecord,
	sourceReceiptFilename,
	writeSourceReceipt,
} from "./literature.ts";

const executeFile = promisify(execFile);

function requirePublicHttps(url: URL): void {
	if (
		url.protocol !== "https:" ||
		url.username ||
		url.password ||
		isIP(url.hostname.replace(/^\[|\]$/g, "")) ||
		!url.hostname.includes(".") ||
		/\.(?:localhost|local|internal)$/.test(url.hostname)
	) {
		throw new Error("Capture requires a public HTTPS source origin");
	}
}

export async function captureSourcePage(options: {
	workspaceRoot: string;
	jobId: string;
	sourceRef: string;
	url: string;
	fetcher?: Fetcher;
}): Promise<{ sourceRef: string; path: string; textPath: string; contentSha256: string; bytes: number }> {
	const record = await readSourceRecord(options.workspaceRoot, options.jobId, options.sourceRef);
	if (!record) throw new Error("Source must already be recorded before capturing a page");
	let url = new URL(options.url);
	const origins = [record.landingPageUrl, record.pdfUrl].flatMap((value) => (value ? [new URL(value).origin] : []));
	if (!origins.includes(url.origin)) throw new Error("Capture requires an HTTPS page on the recorded source origin");
	requirePublicHttps(url);
	url.hash = "";
	const init = {
		signal: AbortSignal.timeout(90_000),
		redirect: "manual" as const,
		headers: {
			Accept: "text/html, text/plain, application/json, application/xhtml+xml, application/pdf",
			"User-Agent": "Astra research agent",
		},
	};
	const redirects: Array<{ from: string; location: string; to: string }> = [];
	let response = await (options.fetcher ?? literatureFetch)(url, init);
	while ([301, 302, 303, 307, 308].includes(response.status)) {
		await response.body?.cancel();
		const location = response.headers.get("location");
		if (!location || redirects.length === 5)
			throw new Error("Source redirect missing a location or exceeding five hops");
		const next = new URL(location, url);
		// Some DOI registries still return HTTP publisher links; never transmit over HTTP.
		if (next.protocol === "http:") next.protocol = "https:";
		next.hash = "";
		requirePublicHttps(next);
		redirects.push({ from: url.href, location, to: next.href });
		url = next;
		response = await (options.fetcher ?? literatureFetch)(url, init);
	}
	const mediaType = response.headers.get("content-type")?.split(";")[0].trim().toLowerCase();
	if (
		!response.ok ||
		!mediaType ||
		!["text/html", "text/plain", "application/json", "application/xhtml+xml", "application/pdf"].includes(mediaType)
	) {
		await response.body?.cancel();
		throw new Error(
			`Source page capture rejected HTTP ${response.status}, type ${mediaType ?? "unknown"}; use an accessible paper or text page`,
		);
	}
	const pdf = mediaType === "application/pdf";
	const bytes = await readLiteratureBytes(response, (pdf ? 32 : 4) * 1024 * 1024);
	let content = bytes.toString("utf8");
	if (pdf) {
		if (!bytes.subarray(0, 5).equals(Buffer.from("%PDF-"))) throw new Error("Invalid PDF header");
		const temporary = await mkdtemp(
			join(options.workspaceRoot, ".astra", "jobs", options.jobId, "sources", ".capture-"),
		);
		try {
			const pdfPath = join(temporary, "source.pdf");
			await writeFile(pdfPath, bytes);
			content = (
				await executeFile("pdftotext", ["-layout", pdfPath, "-"], { timeout: 15_000, maxBuffer: 4 * 1024 * 1024 })
			).stdout;
			if (!content.trim()) throw new Error("PDF has no extractable text; OCR is not available");
		} finally {
			await rm(temporary, { recursive: true, force: true });
		}
	}
	const contentSha256 = createHash("sha256").update(bytes).digest("hex");
	const sourceRef = `${url.href}#astra-page-${contentSha256}`;
	const directory = join(options.workspaceRoot, ".astra", "jobs", options.jobId, "sources");
	const path = join(directory, sourceReceiptFilename(sourceRef)!);
	const textPath = `${path}.txt`;
	if (!(await readSourceRecord(options.workspaceRoot, options.jobId, sourceRef))) {
		const searchReceipt: unknown = JSON.parse(
			await readFile(join(directory, sourceReceiptFilename(options.sourceRef)!), "utf8"),
		);
		await writeSourceReceipt(
			{ ...options, query: `Capture of ${options.sourceRef}`, limit: 1 },
			{ ...record, sourceRef, landingPageUrl: url.href, retrievalLevel: "source-page" },
			"source-page",
			new Date().toISOString(),
			{
				sourceRef: options.sourceRef,
				requestedUrl: options.url,
				url: url.href,
				redirects,
				mediaType,
				contentSha256,
				content,
				...(pdf
					? {
							extraction: "pdftotext -layout",
							extractedTextSha256: sha256(content),
							originalBase64: bytes.toString("base64"),
						}
					: {}),
				searchReceipt,
			},
		);
	}
	await writeFile(textPath, content, "utf8");
	return { sourceRef, path, textPath, contentSha256, bytes: bytes.length };
}
