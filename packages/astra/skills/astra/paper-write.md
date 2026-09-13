---
name: astra-paper-write
description: Use during Astra paper writing to produce an evidence-bound manuscript with verified citations, claim bindings, and explicit limitations.
---

# Astra Paper Write Stage

Write from the canonical paper plan and claim map. Keep citations tied to retrieved stable identifiers, preserve result refs for empirical statements, and state limitations and negative evidence. Never fabricate bibliographic details.

Write the complete manuscript to `paper-manuscript.md`. In the worker submission, `content.manuscript` must be the relative path `paper-manuscript.md`, not the manuscript body. Add an artifact ref for that file. Keep `sections`, `citations`, `claimBindings`, and `limitations` compact and structured.

Do not create `paper-write-output.json`. Do not embed the manuscript text in `astra_submit_worker_output` arguments or copy it into another JSON file. After writing `paper-manuscript.md`, call `astra_submit_worker_output` directly and exactly once.
