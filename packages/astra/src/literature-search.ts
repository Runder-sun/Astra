import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { XMLParser } from "fast-xml-parser";
import { assertAstraId, atomicWriteJson, sha256 } from "./contracts.ts";
import {
	type Fetcher,
	type LiteratureRecord,
	type LiteratureSearchResult,
	literatureFetch,
	readLiteratureResponse,
	type SearchOpenAlexOptions,
	searchOpenAlex,
	sourceReceiptFilename,
	writeSourceReceipt,
} from "./literature.ts";

type Provider = "openalex" | "crossref" | "arxiv";
interface SearchAttempt {
	provider: Provider;
	status: "ok" | "failed";
	detail?: string;
}
export interface ResilientLiteratureSearch extends LiteratureSearchResult {
	cacheHit: boolean;
	attempts: SearchAttempt[];
}

const CACHE_MAX_AGE_MS = 24 * 60 * 60 * 1000;
const pendingSearches = new Map<string, Promise<ResilientLiteratureSearch>>();
let arxivQueue = Promise.resolve();
let lastArxivRequest = 0;

function object(value: unknown): Record<string, unknown> {
	return value !== null && typeof value === "object" && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: {};
}
function text(value: unknown): string | undefined {
	return typeof value === "string" && value.trim() ? value.trim().replace(/\s+/g, " ") : undefined;
}
function doi(value: unknown): string | undefined {
	const normalized = text(value)
		?.replace(/^https?:\/\/(?:dx\.)?doi\.org\//i, "")
		.toLowerCase();
	return normalized && /^10\.\d{4,9}\/\S+$/.test(normalized) ? normalized : undefined;
}

async function readJson(path: string): Promise<Record<string, unknown> | undefined> {
	try {
		return object(JSON.parse(await readFile(path, "utf8")));
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT" || error instanceof SyntaxError) return undefined;
		throw error;
	}
}

async function searchCrossref(options: SearchOpenAlexOptions, fetcher: Fetcher): Promise<LiteratureRecord[]> {
	const url = new URL("https://api.crossref.org/works");
	url.searchParams.set("query.bibliographic", options.query);
	url.searchParams.set("rows", String(options.limit));
	const payload = object(JSON.parse(await readLiteratureResponse(await fetcher(url))));
	const items = object(payload.message).items;
	if (!Array.isArray(items)) throw new Error("Malformed Crossref response");
	return items.flatMap((value) => {
		const work = object(value);
		const id = doi(work.DOI);
		const title = Array.isArray(work.title) ? text(work.title[0]) : undefined;
		if (!id || !title) return [];
		const dateParts = object(work.published)["date-parts"];
		const year = Array.isArray(dateParts) && Array.isArray(dateParts[0]) ? dateParts[0][0] : undefined;
		const authors = Array.isArray(work.author)
			? work.author.flatMap((value) => {
					const author = object(value);
					const name = [text(author.given), text(author.family)].filter(Boolean).join(" ") || text(author.name);
					return name ? [name] : [];
				})
			: [];
		return [
			{
				sourceRef: `doi:${id}`,
				doi: id,
				title,
				authors,
				landingPageUrl: `https://doi.org/${id}`,
				...(typeof year === "number" ? { year } : {}),
				...(text(work.type) ? { type: text(work.type) } : {}),
				// Crossref abstracts may contain JATS markup; retain it as metadata, not page content.
				...(text(work.abstract) ? { abstract: text(work.abstract) } : {}),
			},
		];
	});
}

async function searchArxiv(options: SearchOpenAlexOptions, fetcher: Fetcher): Promise<LiteratureRecord[]> {
	const previous = arxivQueue;
	let release = () => {};
	arxivQueue = new Promise<void>((resolve) => {
		release = resolve;
	});
	let body: string;
	try {
		await previous;
		await delay(Math.max(0, lastArxivRequest + 3000 - Date.now()));
		lastArxivRequest = Date.now();
		const url = new URL("https://export.arxiv.org/api/query");
		url.searchParams.set(
			"search_query",
			options.query
				.split(/\s+/)
				.map((term) => `all:${JSON.stringify(term)}`)
				.join(" AND "),
		);
		url.searchParams.set("max_results", String(options.limit));
		body = await readLiteratureResponse(await fetcher(url));
	} finally {
		release();
	}
	if (/<!DOCTYPE/i.test(body)) throw new Error("arXiv response contains an unsupported DOCTYPE");
	const parser = new XMLParser({
		ignoreAttributes: true,
		removeNSPrefix: true,
		parseTagValue: false,
		isArray: (name) => name === "entry" || name === "author",
	});
	const payload: unknown = parser.parse(body, true);
	const feed = object(object(payload).feed);
	if (!Object.keys(feed).length) throw new Error("Malformed arXiv feed");
	const entries = Array.isArray(feed.entry) ? feed.entry : [];
	return entries.flatMap((value) => {
		const entry = object(value);
		const id = text(entry.id)?.match(
			/^https?:\/\/arxiv\.org\/abs\/((?:\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7}))(?:v\d+)?$/,
		)?.[1];
		const title = text(entry.title);
		if (!id || !title) return [];
		const year = Number(text(entry.published)?.slice(0, 4));
		const paperDoi = doi(entry.doi);
		return [
			{
				sourceRef: `arxiv:${id}`,
				title,
				authors: (Array.isArray(entry.author) ? entry.author : []).flatMap((value) =>
					text(object(value).name) ? [text(object(value).name)!] : [],
				),
				landingPageUrl: `https://arxiv.org/abs/${id}`,
				pdfUrl: `https://arxiv.org/pdf/${id}`,
				...(Number.isInteger(year) && year > 1900 ? { year } : {}),
				...(paperDoi ? { doi: paperDoi } : {}),
				...(text(entry.summary) ? { abstract: text(entry.summary) } : {}),
			},
		];
	});
}

async function retrieve(options: SearchOpenAlexOptions, cachePath: string): Promise<ResilientLiteratureSearch> {
	const sourcesRoot = join(options.workspaceRoot, ".astra", "jobs", options.jobId, "sources");
	const cached = await readJson(cachePath);
	if (cached) {
		const { sha256: digest, ...search } = cached;
		const age = Date.now() - Date.parse(String(search.retrievedAt));
		if (
			digest === sha256(JSON.stringify(search)) &&
			age >= 0 &&
			age < CACHE_MAX_AGE_MS &&
			Array.isArray(search.results)
		) {
			let valid = true;
			for (const value of search.results) {
				const filename = sourceReceiptFilename(String(object(value).sourceRef));
				const receipt = filename ? await readJson(join(sourcesRoot, filename)) : undefined;
				const { sha256: receiptHash, ...unsigned } = receipt ?? {};
				if (
					!receipt ||
					receiptHash !== sha256(JSON.stringify(unsigned)) ||
					JSON.stringify(unsigned.record) !== JSON.stringify(value)
				)
					valid = false;
			}
			if (valid) return { ...search, cacheHit: true } as unknown as ResilientLiteratureSearch;
		}
	}
	const results: LiteratureRecord[] = [];
	const identities = new Set<string>();
	const attempts: SearchAttempt[] = [];
	const retrievedAt = new Date().toISOString();
	for (const provider of ["openalex", "crossref", "arxiv"] as const) {
		let records: LiteratureRecord[];
		const cooldownPath = join(sourcesRoot, `${provider}-cooldown.json`);
		try {
			const cooldown = await readJson(cooldownPath);
			if (Number(cooldown?.retryAt) > Date.now())
				throw new Error(`Rate limited; retry after ${new Date(Number(cooldown?.retryAt)).toISOString()}`);
			const fetcher: Fetcher = async (url, init) => {
				const response = await (options.fetcher ?? literatureFetch)(url, {
					...init,
					signal: init?.signal ?? AbortSignal.timeout(20_000),
					headers: {
						Accept: provider === "arxiv" ? "application/atom+xml" : "application/json",
						"User-Agent": "Astra research agent",
					},
				});
				if (response.status === 429 || response.status === 503) {
					const retryAfter = response.headers.get("retry-after");
					const retryAt =
						retryAfter && /^\d+$/.test(retryAfter)
							? Date.now() + Number(retryAfter) * 1000
							: Date.parse(retryAfter ?? "");
					await atomicWriteJson(cooldownPath, {
						retryAt: Math.max(Date.now() + 1000, Number.isFinite(retryAt) ? retryAt : Date.now() + 300_000),
					});
				}
				return response;
			};
			records =
				provider === "openalex"
					? (await searchOpenAlex({ ...options, fetcher })).results
					: provider === "crossref"
						? await searchCrossref(options, fetcher)
						: await searchArxiv(options, fetcher);
		} catch (error) {
			attempts.push({ provider, status: "failed", detail: error instanceof Error ? error.message : String(error) });
			continue;
		}
		attempts.push({ provider, status: "ok" });
		for (const record of records) {
			const identity = doi(record.doi) ?? record.sourceRef;
			if (identities.has(identity)) continue;
			identities.add(identity);
			if (provider !== "openalex") await writeSourceReceipt(options, record, provider, retrievedAt);
			results.push(record);
			if (results.length >= options.limit) break;
		}
		if (results.length >= options.limit) break;
	}
	const search: ResilientLiteratureSearch = {
		query: options.query,
		retrievedAt,
		total: results.length,
		results,
		cacheHit: false,
		attempts,
	};
	// Partial searches should get a chance to recover, rather than masking an outage for a day.
	if (results.length >= options.limit)
		await atomicWriteJson(cachePath, { ...search, sha256: sha256(JSON.stringify(search)) });
	return search;
}

/** Shared host retrieval for Pi and Codex. Provider failure is a diagnostic, not a fabricated source. */
export async function searchLiterature(options: SearchOpenAlexOptions): Promise<ResilientLiteratureSearch> {
	assertAstraId(options.jobId, "job id");
	const query = options.query.trim().replace(/\s+/g, " ");
	if (query.length < 3 || query.length > 500) throw new Error("Literature query must contain 3-500 characters");
	if (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 10)
		throw new Error("Literature limit must be an integer from 1 to 10");
	const path = join(
		options.workspaceRoot,
		".astra",
		"jobs",
		options.jobId,
		"sources",
		`search-${sha256(JSON.stringify([query, options.limit]))}.json`,
	);
	const pending = pendingSearches.get(path);
	if (pending) return pending;
	const search = retrieve({ ...options, query }, path);
	pendingSearches.set(path, search);
	try {
		return await search;
	} finally {
		pendingSearches.delete(path);
	}
}
