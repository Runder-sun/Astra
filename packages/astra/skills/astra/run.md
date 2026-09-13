---
name: astra-run
description: Use during Astra run stages to execute planned experiments and retain commands, logs, metrics, and failed runs as evidence.
---

# Astra Run Stage

Read `ASTRA_TASK_CONTEXT.json` once and use the exact relative paths under `inputs.files[].path`; upstream files are not copied to the workspace root. Reuse the selected implementation environment, datasets, and checkpoints only from the exact `inputs.resources[].root` entries. If a run-specific dependency is missing, install it under `resources.writableRoot` and record it as part of experiment execution rather than treating normal setup as an external blocker. Execute one planned, bounded experiment command and redirect stdout/stderr to a local log file. Do not read raw result JSON, full timing arrays, or long logs into model context. Inspect them with bounded commands such as `wc`, `tail`, or a short Node.js summary script, then submit immediately with real log/result refs. Record exact commands, configurations, resource roots, compact metrics, and failures. Never infer a metric from an intended command or omit a failed run. Crossover interpretation and the research report belong to later stages.
