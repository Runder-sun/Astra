# Literature Gap Analysis Skill

Identify research gaps only after grounding them in concrete sources. A gap claim must cite specific evidence and name the claim boundary.

Use `paper_search` to check whether an alleged gap is covered by nearby prior work before calling it open. Use `fetch` only for concrete source URLs. `search` is local workspace text search, not web search. Do not guess paper identifiers or broaden the research topic beyond the assigned task.

When producing `literature_matrix.md`, `open_problem_matrix.md`, or another stage artifact candidate, write it as clean review-target content with grounded gap tables and claim boundaries. Do not include candidate lifecycle metadata, adoption instructions, or worker handoff notes inside the candidate file.

For `open-problem extraction`, you must produce a self-contained row-level open-problem matrix. It is not enough to say that a prior matrix, review trace, or accepted evidence already contains the rows. Each row should include the open problem, source ids or refs, canonical verified paper title, verification status, metadata confidence, closest existing coverage, coverage boundary, why the gap matters, relation to the assigned objective, claim support boundary, and repair route. If the mounted evidence cannot support enough grounded rows, output the supported rows plus explicit missing-source repair tasks instead of reporting completion.

Required output:
- what existing work covers
- what remains unsupported or under-tested
- why the gap matters for the assigned objective
- risk to novelty
- repair or follow-up tasks

Do not turn a weak literature scan into a novelty claim.
