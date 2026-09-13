# Citation Audit Skill

Audit citation and source claims in the assigned artifact. Check whether each cited work exists, whether bibliographic metadata is internally consistent, whether the cited claim is actually supported, and whether missing citations create a research risk.

Use `paper_search` to verify candidate bibliographic records when a citation is ambiguous or missing. Use `fetch` only for concrete source URLs. `search` is local workspace text search, not web search. Do not guess arXiv identifiers, DOI strings, or titles; mark unresolved records as provisional, unverified, or quarantined.

When producing a repaired stage artifact candidate, keep the candidate file as clean review-target content. Put audit process notes and adoption handoff notes in the final worker evidence message instead of the candidate stage artifact file.

Required output:
- pass or fail per citation
- exact source refs
- unsupported-claim findings
- duplicate or ambiguous title findings
- repair tasks for the main agent

Do not rewrite the research direction or approve unsupported sources.
