import {
	type LiteratureRecord,
	readSourceRecord,
	type SearchOpenAlexOptions,
	writeSourceReceipt,
} from "./literature.ts";

/** Only server-returned text results qualify. An attempted URL alone proves no retrieval. */
export async function recordCodexWebSources(
	options: SearchOpenAlexOptions,
	item: Record<string, unknown>,
): Promise<LiteratureRecord[]> {
	const records: LiteratureRecord[] = [];
	const seen = new Set<string>();
	for (const value of Array.isArray(item.results) ? item.results : []) {
		if (!value || typeof value !== "object") continue;
		const result = value as Record<string, unknown>;
		if (
			result.type !== "text_result" ||
			typeof result.url !== "string" ||
			typeof result.title !== "string" ||
			!result.title.trim()
		)
			continue;
		let url: URL;
		try {
			url = new URL(result.url);
		} catch {
			continue;
		}
		if (!["https:", "http:"].includes(url.protocol) || url.username || url.password) continue;
		url.hash = "";
		const arxivId = ["arxiv.org", "www.arxiv.org"].includes(url.hostname)
			? url.pathname.match(
					/^\/(?:abs|pdf)\/((?:\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7}))(?:v\d+)?(?:\.pdf)?$/,
				)?.[1]
			: undefined;
		const doi = ["doi.org", "dx.doi.org"].includes(url.hostname) ? url.pathname.slice(1).toLowerCase() : undefined;
		const sourceRef = arxivId
			? `arxiv:${arxivId}`
			: doi && /^10\.\d{4,9}\/\S+$/.test(doi)
				? `doi:${doi}`
				: url.protocol === "https:"
					? url.href
					: undefined;
		if (!sourceRef || seen.has(sourceRef)) continue;
		seen.add(sourceRef);
		const existing = await readSourceRecord(options.workspaceRoot, options.jobId, sourceRef);
		// Keep the first receipt: open-page events may return only a line count, not page content.
		if (existing) {
			records.push(existing);
			continue;
		}
		const record: LiteratureRecord = {
			sourceRef,
			title: result.title.trim(),
			authors: [],
			landingPageUrl: url.href,
			retrievalLevel: "web-search-result",
			...(typeof result.snippet === "string" ? { snippet: result.snippet } : {}),
		};
		await writeSourceReceipt(
			{ ...options, query: typeof item.query === "string" ? item.query : options.query },
			record,
			"codex-web",
			new Date().toISOString(),
			{ itemId: item.id, action: item.action, result },
		);
		records.push(record);
	}
	return records;
}
