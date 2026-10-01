import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { EnvHttpProxyAgent, fetch as proxyFetch } from "undici";
import { assertAstraId, atomicWriteJson, sha256 } from "./contracts.ts";

const OPENALEX_WORKS_URL = "https://api.openalex.org/works";
const MAX_RESPONSE_BYTES = 4 * 1024 * 1024;

interface OpenAlexWork {
	id?: unknown;
	doi?: unknown;
	title?: unknown;
	display_name?: unknown;
	publication_year?: unknown;
	type?: unknown;
	cited_by_count?: unknown;
	authorships?: unknown;
	abstract_inverted_index?: unknown;
	primary_location?: unknown;
}

interface OpenAlexResponse {
	meta?: { count?: unknown };
	results?: OpenAlexWork[];
}

export interface LiteratureRecord {
	sourceRef: string;
	openAlexId?: string;
	doi?: string;
	title: string;
	year?: number;
	type?: string;
	authors: string[];
	abstract?: string;
	landingPageUrl?: string;
	pdfUrl?: string;
	citedByCount?: number;
	retrievalLevel?: "web-search-result" | "source-page";
	snippet?: string;
}

export type LiteratureResponse = Pick<Response, "ok" | "status" | "headers" | "body" | "text">;
export type Fetcher = (
	input: string | URL,
	init?: Pick<RequestInit, "signal" | "headers" | "redirect">,
) => Promise<LiteratureResponse>;

const originalFetch = globalThis.fetch;
let proxyAgent: EnvHttpProxyAgent | undefined;
export const literatureFetch: Fetcher = (input, init) => {
	const usesProxy =
		process.env.HTTPS_PROXY || process.env.https_proxy || process.env.HTTP_PROXY || process.env.http_proxy;
	if (!usesProxy || globalThis.fetch !== originalFetch) return fetch(input, init);
	proxyAgent ??= new EnvHttpProxyAgent();
	// Keep fetch and its dispatcher on the same undici version; do not change global fetch.
	return proxyFetch(input, {
		signal: init?.signal,
		redirect: init?.redirect,
		headers: Object.fromEntries(new Headers(init?.headers).entries()),
		dispatcher: proxyAgent,
	});
};

export function sourceReceiptFilename(sourceRef: string): string | undefined {
	const openAlexId = /^openalex:(W\d+)$/.exec(sourceRef)?.[1];
	if (openAlexId) return `openalex-${openAlexId}.json`;
	return /^(?:doi:10\.\d{4,9}\/\S+|arxiv:(?:\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})|https:\/\/\S+)$/.test(
		sourceRef,
	)
		? `source-${sha256(sourceRef)}.json`
		: undefined;
}

export async function writeSourceReceipt(
	options: SearchOpenAlexOptions,
	record: LiteratureRecord,
	provider: string,
	retrievedAt: string,
	observation?: unknown,
): Promise<void> {
	assertAstraId(options.jobId, "job id");
	const filename = sourceReceiptFilename(record.sourceRef);
	if (!filename) throw new Error(`Invalid literature source reference: ${record.sourceRef}`);
	const receipt = {
		sourceRef: record.sourceRef,
		provider,
		query: options.query,
		retrievedAt,
		record,
		...(observation ? { observation } : {}),
	};
	await atomicWriteJson(join(options.workspaceRoot, ".astra", "jobs", options.jobId, "sources", filename), {
		...receipt,
		sha256: sha256(JSON.stringify(receipt)),
	});
}

export async function readSourceRecord(
	workspaceRoot: string,
	jobId: string,
	sourceRef: string,
): Promise<LiteratureRecord | undefined> {
	assertAstraId(jobId, "job id");
	const filename = sourceReceiptFilename(sourceRef);
	if (!filename) return undefined;
	try {
		const { sha256: digest, ...receipt } = JSON.parse(
			await readFile(join(workspaceRoot, ".astra", "jobs", jobId, "sources", filename), "utf8"),
		) as { sha256?: string; sourceRef?: string; record?: LiteratureRecord };
		if (
			digest !== sha256(JSON.stringify(receipt)) ||
			receipt.sourceRef !== sourceRef ||
			receipt.record?.sourceRef !== sourceRef
		)
			return undefined;
		return receipt.record;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT" || error instanceof SyntaxError) return undefined;
		throw error;
	}
}

export interface LiteratureSearchResult {
	query: string;
	retrievedAt: string;
	total: number;
	results: LiteratureRecord[];
}

export function compactLiteratureSearch(
	search: LiteratureSearchResult,
	maxResults = 3,
	maxAbstractChars = 240,
): LiteratureSearchResult {
	return {
		...search,
		results: search.results.slice(0, maxResults).map((record) => ({
			...record,
			...(record.abstract ? { abstract: record.abstract.slice(0, maxAbstractChars) } : {}),
		})),
	};
}

export interface SearchOpenAlexOptions {
	query: string;
	limit: number;
	workspaceRoot: string;
	jobId: string;
	fetcher?: Fetcher;
}

function abstractFromInvertedIndex(value: unknown): string | undefined {
	if (value === null || typeof value !== "object" || Array.isArray(value)) return undefined;
	const words: Array<{ word: string; position: number }> = [];
	for (const [word, positions] of Object.entries(value as Record<string, unknown>)) {
		if (!Array.isArray(positions)) continue;
		for (const position of positions) if (typeof position === "number") words.push({ word, position });
	}
	if (words.length === 0) return undefined;
	return words
		.sort((left, right) => left.position - right.position)
		.map((entry) => entry.word)
		.join(" ");
}

function text(value: unknown): string | undefined {
	return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function normalizeWork(work: OpenAlexWork): LiteratureRecord | undefined {
	const rawId = text(work.id);
	const title = text(work.title) ?? text(work.display_name);
	if (!rawId || !title) return undefined;
	const openAlexId = rawId.replace(/^https:\/\/openalex\.org\//, "");
	if (!/^W\d+$/.test(openAlexId)) return undefined;
	const authorships = Array.isArray(work.authorships) ? work.authorships : [];
	const authors = authorships.flatMap((entry) => {
		if (entry === null || typeof entry !== "object") return [];
		const author = (entry as Record<string, unknown>).author;
		if (author === null || typeof author !== "object") return [];
		const name = text((author as Record<string, unknown>).display_name);
		return name ? [name] : [];
	});
	const location =
		work.primary_location !== null && typeof work.primary_location === "object"
			? (work.primary_location as Record<string, unknown>)
			: {};
	const doi = text(work.doi)?.replace(/^https:\/\/doi\.org\//, "");
	return {
		sourceRef: `openalex:${openAlexId}`,
		openAlexId,
		...(doi ? { doi } : {}),
		title,
		...(typeof work.publication_year === "number" ? { year: work.publication_year } : {}),
		...(text(work.type) ? { type: text(work.type) } : {}),
		authors,
		...(abstractFromInvertedIndex(work.abstract_inverted_index)
			? { abstract: abstractFromInvertedIndex(work.abstract_inverted_index) }
			: {}),
		...(text(location.landing_page_url) ? { landingPageUrl: text(location.landing_page_url) } : {}),
		...(text(location.pdf_url) ? { pdfUrl: text(location.pdf_url) } : {}),
		...(typeof work.cited_by_count === "number" ? { citedByCount: work.cited_by_count } : {}),
	};
}

async function responseJson(response: LiteratureResponse): Promise<OpenAlexResponse> {
	if (!response.ok) {
		await response.body?.cancel();
		throw new Error(`OpenAlex returned HTTP ${response.status}`);
	}
	return JSON.parse(await readLiteratureResponse(response)) as OpenAlexResponse;
}

export async function readLiteratureResponse(response: LiteratureResponse): Promise<string> {
	return (await readLiteratureBytes(response)).toString("utf8");
}

export async function readLiteratureBytes(
	response: LiteratureResponse,
	maxBytes = MAX_RESPONSE_BYTES,
): Promise<Buffer> {
	if (!response.ok) {
		await response.body?.cancel();
		throw new Error(`HTTP ${response.status}`);
	}
	const reader = response.body?.getReader();
	if (!reader) throw new Error("Empty literature response");
	const chunks: Uint8Array[] = [];
	let size = 0;
	try {
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			size += value.byteLength;
			if (size > maxBytes) throw new Error(`Literature response exceeded ${maxBytes / (1024 * 1024)} MiB`);
			chunks.push(value);
		}
	} finally {
		await reader.cancel();
	}
	return Buffer.concat(chunks);
}

export async function searchOpenAlex(options: SearchOpenAlexOptions): Promise<LiteratureSearchResult> {
	assertAstraId(options.jobId, "job id");
	const query = options.query.trim();
	if (query.length < 3 || query.length > 500) throw new Error("OpenAlex query must contain 3-500 characters");
	if (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 10) {
		throw new Error("OpenAlex limit must be an integer from 1 to 10");
	}
	const url = new URL(OPENALEX_WORKS_URL);
	url.searchParams.set("search", query);
	url.searchParams.set("per-page", String(options.limit));
	url.searchParams.set(
		"select",
		"id,doi,title,display_name,publication_year,type,cited_by_count,authorships,abstract_inverted_index,primary_location",
	);
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), 20_000);
	let payload: OpenAlexResponse;
	try {
		const response = await (options.fetcher ?? literatureFetch)(url, {
			signal: controller.signal,
			headers: { Accept: "application/json", "User-Agent": "Astra research agent" },
		});
		payload = await responseJson(response);
	} finally {
		clearTimeout(timeout);
	}
	const retrievedAt = new Date().toISOString();
	const results = (Array.isArray(payload.results) ? payload.results : []).flatMap((work) => {
		const normalized = normalizeWork(work);
		return normalized ? [normalized] : [];
	});
	for (const record of results) {
		await writeSourceReceipt({ ...options, query }, record, "openalex", retrievedAt);
	}
	return {
		query,
		retrievedAt,
		total: typeof payload.meta?.count === "number" ? payload.meta.count : results.length,
		results,
	};
}
