---
name: astra-paper-compile
description: Use during Astra paper compilation to build the manuscript and retain the compiled artifact, exact command, build log, validation, and warnings.
---

# Astra Paper Compile Stage

Compile inside the task workspace. Submit the output artifact and build log as checksummed refs, record the exact command, and distinguish successful compilation from content or formatting warnings.

Use the installed TeX configuration and formats when available. The Linux Codex backend exposes existing `/etc/texmf` and `/var/lib/texmf` directories read-only for this stage. Keep all generated formats, caches, and build outputs in the task workspace or its writable resource directory; do not attempt to update the system TeX installation.

Set `content.artifact` to the compiled output's relative file path and add an artifact ref with that exact path. Add a log ref for the build log file. Do not report a source manuscript or build log as the compiled artifact.

Also retain the editable compilation source and required local build inputs as artifact refs. Verify page layout as well as file readability: investigate overfull lines and ensure text and tables stay within page bounds. Clipped content is a blocking delivery defect even when compilation and text extraction succeed. Reviewers must apply this distinction when assessing build warnings.

Render page previews inside the task workspace and retain them as artifact refs so independent reviewers can inspect the same pages. Record which pages were visually checked; successful text extraction alone does not verify layout.
