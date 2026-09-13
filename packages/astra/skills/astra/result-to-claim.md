---
name: astra-result-to-claim
description: Use during Astra result-to-claim stages to decide exactly which claims experimental results support, contradict, or leave unresolved.
---

# Astra Result To Claim Stage

Map each claim to specific result refs and comparisons. Preserve negative and inconclusive outcomes. Mark unsupported claims and missing evidence explicitly; do not turn correlation, a smoke test, or absence of failure into a stronger claim.

Report `scientificOutcome` as `supported`, `partially-supported`, `refuted`, `inconclusive`, or `insufficient-evidence`, and report `missionCoverage` as `sufficient` or `insufficient`. These fields assess the primary user objective, not whether the workflow ran successfully. Each claim must be an object with a concise `statement` and an `assessment` of `supported`, `partially-supported`, `refuted`, `unsupported`, or `unresolved`.

These two outcome fields are machine-readable string labels, not narrative text or objects. Put observed results and their scope in `claims`, `supportingResults`, and `conclusion`; describe uncovered requirements in `missingEvidence`. Planners must not freeze checks requiring prose inside either label field. Reviewers must assess each label together with its linked explanation and evidence, rather than reject a valid label for being short. When repairing a contradictory request, preserve the labels and make the corresponding explanations explicit in those companion fields.
