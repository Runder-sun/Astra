---
name: astra-paper-compile
description: Use during Astra paper compilation to build the manuscript and retain the compiled artifact, exact command, build log, validation, and warnings.
---

# Astra Paper Compile Stage

Compile inside the task workspace. Submit the output artifact and build log as checksummed refs, record the exact command, and distinguish successful compilation from content or formatting warnings.

Use the installed TeX configuration and formats when available. The Linux Codex backend exposes existing `/etc/texmf` and `/var/lib/texmf` directories read-only for this stage. Keep all generated formats, caches, and build outputs in the task workspace or its writable resource directory; do not attempt to update the system TeX installation.

Set `content.artifact` to the compiled output's relative file path and add an artifact ref with that exact path. Add a log ref for the build log file. Do not report a source manuscript or build log as the compiled artifact.

Set `content.source` to the editable compilation entry point and `content.buildInputs` to an array of all required local source, bibliography, image and configuration paths, including that entry point. Every listed input needs an artifact ref; `content.buildLog` must name a nonempty log file with a log ref. Record the nonempty exact build command in `content.command`. Host preflight parses the submitted PDF bytes with Poppler `pdfinfo`; missing parser, malformed PDF or missing delivery files block submission. Parser success does not establish visual layout quality or reproduce the build.

Also retain the editable compilation source and required local build inputs as artifact refs. Verify page layout as well as file readability: investigate overfull lines and ensure text and tables stay within page bounds. Clipped content is a blocking delivery defect even when compilation and text extraction succeed. Reviewers must apply this distinction when assessing build warnings.

Render page previews inside the task workspace and retain them as artifact refs so independent reviewers can inspect the same pages. Record which pages were visually checked; successful text extraction alone does not verify layout.
