---
name: astra-research-review
description: Use during Astra final research review to audit the complete problem, novelty, implementation, experiment, claim, citation, and manuscript evidence chain.
---

# Astra Research Review Stage

Review the whole canonical chain as a skeptical external reviewer. Check novelty sources, executable evidence, result-to-claim discipline, citations, and manuscript consistency. Separate blocking repairs from optional improvements and do not pass a summary-only artifact.

You are producing the whole-research assessment now. Its own canonical adoption and independent reviews occur after submission; their current absence is not a defect in the research evidence and must not become a circular required repair. Assess the available scientific and delivery evidence. Do not require upstream result-to-claim artifacts to claim that later workflow stages have already completed.

`inputs.canonicalArtifacts` is a full-chain index. Read `inputs.reviewSummaryPath` first, then inspect the `contentPath` files and actual evidence needed for every acceptance criterion. Batch targeted reads within the task budget; the summary is an index, not a substitute for verification. Use `inputs.files[].path` for cited artifacts and `inputs.resources` for original run outputs. If the budget prevents a required check, report it as unverified rather than assuming it passed.

Each summary entry lists its directly readable `files` separately from optional runtime `resourceRoots`. Manuscripts, PDFs and build logs may be in `files` even when a resource directory is empty. Inspect these listed paths before declaring evidence inaccessible; do not infer absence from the resource directory alone.

Keep submitted `content` under 3,500 characters: at most 3 strengths, at most 5 weaknesses, at most 5 claim-audit entries, and at most 5 required repairs. Submit those four fields as string arrays; each item must be one concise, evidence-linked statement.

Independently report `scientificOutcome` and `missionCoverage` for the primary objective. A workflow can pass with an inconclusive or refuted outcome, but it cannot label insufficient evidence as sufficient coverage. If an in-scope experiment needed for the claimed outcome remains feasible, put it in `requiredRepairs` instead of passing it as a nonblocking caveat.

Use the machine-readable `scientificOutcome` labels `supported`, `partially-supported`, `refuted`, `inconclusive`, or `insufficient-evidence`, and `missionCoverage` labels `sufficient` or `insufficient`. Keep explanations in the review findings and conclusion, not inside these labels. Planners and reviewers must assess the labels together with their supporting explanations and evidence.
