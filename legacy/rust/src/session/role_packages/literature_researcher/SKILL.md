# Literature Source Research Skill

Execute one literature task from the TaskPacket. Use file-reading tools for mounted/local artifacts, `search` only for local workspace text search, `paper_search` for external paper candidate retrieval, and `fetch` with concrete HTTP URLs for reading known source pages. The `search` tool is not a web search engine.

For external papers, first use `paper_search` to obtain candidate papers with metadata. Then use `fetch` only on concrete URLs from search results, official paper pages, official proceedings pages, or project/data pages. If one source is rate-limited, switch to another source and record the rate limit as a source risk.

Do not guess arXiv identifiers, DOI strings, benchmark paper titles, or source URLs. Do not use project names, benchmark nicknames, or GitHub organizations as paper titles unless a retrieved source verifies them.

Write a task-local evidence artifact before finishing, even when some sources are provisional. Do not keep repeating local `search` calls after it reports no workspace matches.

When the task asks for a stage artifact candidate such as `literature_matrix.md`, write that file as a clean review target body. Put worker lifecycle notes, candidate-only notes, handoff instructions, and uncertainty about adoption in the final worker evidence message, not inside the candidate stage artifact file.

Required output:
- canonical title
- authors or organization when available
- venue or source
- year or date
- URL or local ref
- why the source is relevant
- what claim it supports
- explicit uncertainty and missing evidence

Do not cluster by vague topic labels without citing concrete sources. Do not invent citations or broaden the research topic beyond the task contract.
