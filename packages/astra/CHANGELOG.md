# Astra changes

## [Unreleased]

### Changed

- Unified public installation instructions around the fixed alpha.1 download; moved technical reference material into developer documentation.
- Moved historical Rust sources out of the current tree; the published alpha.1 tag remains the archive reference. Current migration reporting stays available.
- Pinned direct Pi dependencies to 0.84.1 for future packages.
- Separated Astra packaging and source export from upstream release commands.

### Fixed

- Interrupted review and canonical adoption retain their complete governance consequences and resume deterministically. Retirement and candidate cleanup preserve archives and registered sessions before deleting sources; concurrent budget, lease and task operations validate the current state under the existing journal queue.
- Codex runtime exhaustion now follows budgeted recovery, including main-agent replanning. Planning and plan reviews expose inherited dispatch requirements before approval, and the main agent starts from a focused current-work index instead of the full historical index.
- Codex tool-call exhaustion uses bounded task recovery instead of pausing the whole research job as an infrastructure failure. Worker retries retain read-only execution logs and source ledgers; replanning can inspect recorded failures, and global budgets remain enforced.
- Review confirmation validation runs before closing the Codex session, allowing one correction within the original runtime and tool budget. Rejections identify mismatched fields and retain strict original-assessment checks.
- Repair scheduling continues from the latest failed revision within the oldest unresolved evidence lineage, inheriting recovered inputs and output requirements instead of repeatedly restarting from its first failure.
- Codex workers read a distinct requirement index while original issue bindings stay intact; main-agent review indexes expose findings without requiring repeated reads of expanded historical assessments.

- Repeated failed reviews reuse same-lineage repair requirements instead of recursively cloning issue-ID wrappers. New repair contracts keep explicit original issue bindings, and acceptance still requires independently verified closure of every item.
- Codex reviewers receive a compact criterion file separate from historical alias mappings; planning prompts reference indexed obligations and reviews instead of embedding their expanding history. Generated task counts now match repair, decomposition and search limits.

- Codex worker requests reference a lossless local task contract instead of embedding it in the API prompt, avoiding input-length failures from accumulated repair checks without removing requirements.

- Worker inputs expose candidate deliveries as separately indexed, hashed files, allowing direct content reads without searching oversized task contexts or confusing source task packets with results.

- Codex review presentation groups identical criteria with registered repair-ID prefixes, then restores and validates every original criterion before recording; accumulated repair wrappers no longer require duplicate model assessments.
- Review and repair bundles retain exact host-observed literature search arguments and replies, including browser results, so query counts and source mappings can be checked without relying on worker summaries.
- Codex reviewers receive a separate current-delivery file and explicit target identity, avoiding judgments based on older input artifacts or truncated combined snapshots during repairs.
- Source capture follows bounded publisher redirects and retains original PDFs with extracted text, allowing DOI-linked papers to enter independent review; text pages remain limited to 4 MiB and PDFs to 32 MiB.
- Codex review snapshots expose actual worker tool names behind literature permissions so plans can be checked against available capture tools instead of stale permission labels.
- Codex literature workers can capture bounded original text pages with hashed contents and preserved search provenance for independent review, without granting workspace write access.
- Review receipts tolerate identical repeated criteria and previously verified references while restoring the complete original assessment; altered criteria and unknown receipts still fail.
- Codex review confirmations use short invocation-local identifiers instead of model-copied digests, retaining strict receipt matching and full assessment revalidation.
- Plan-review snapshots record declared inputs' host adoption status separately from immutable delivery text, preventing old pending-review labels from obscuring subsequent adoption.
- Codex submission receipts tolerate rewritten source descriptions while retaining exact source identities and restoring the original validated draft.
- Review bundles and downstream inputs include host-observed failed command receipts so omitted worker failure reports can be checked independently.

- Declined canonical adoption preserves the decision reason and pauses for guided replanning instead of repeatedly reconsidering unchanged accepted evidence.
- Repair workers and plan reviewers receive read-only copies of the original source task packet and recorded reviews for explicit contract comparison.
- Frozen plan-review evidence includes recorded user guidance text, and Codex main-agent prompts repeat the latest guidance so repairs can be checked against the actual updated instructions.
- Backtracking preserves a pause received during an in-flight route decision instead of silently resuming model calls.
- A repair strategy that backtracks to the current stage proceeds to a newly reviewed plan instead of repeatedly requesting the same strategy without executing repairs.
- Historical backtrack resources include original superseded repair evidence archives as well as retired artifacts, retaining original candidate files for provenance checks.
- Codex plan input arrays are bounded by the number of available input references, preventing unbounded repeated-ID output while allowing every available input.
- Backtrack workers and reviewers receive same-stage retired archives as explicit historical repair resources; retired artifacts remain invalid as current scientific inputs and require fresh reviewed delivery.
- Backtrack repair checks retain their original route references across later route decisions; adoption closes only objections explicitly included in the reviewed task contract.
- Repair contracts require reported issues to be resolved rather than requiring historical failure descriptions to remain true; issue closure uses the same explicit criteria while preserving old findings.
- Review guidance distinguishes frozen evidence copies from additional runtime resources and requires checking both before reporting missing files.
- Codex workers can confirm validated submissions with short receipts; the host revalidates the complete original draft instead of requiring models to repeat large escaped JSON payloads.
- Codex reviewers can confirm a fully validated assessment with a short receipt; all criteria, findings and references are restored and checked before recording, while unknown or inconsistent receipts are rejected.
- Retried Codex workers can read their failed predecessor's workspace and failure reason to recover completed outputs without repeating scientific runs.
- Repair plan reviews inherit the failed task's required inputs and resolve replacements consistently with worker dispatch, preventing preparation failures before review starts.
- Codex plans constrain input references to existing artifacts and evidence; rejected plans retain their validation error and pause instead of silently repeating route decisions.
- Deferred evidence decisions retain their reason and pause for guided replanning instead of repeatedly asking about unchanged evidence.
- Guided resume no longer lets evidence from superseded plans block replanning or repeatedly consume acceptance decisions; historical evidence and reviews remain available.
- Tool preflight failures identify invalid fields and include validation parameters such as allowed reference values, and review validation names missing, duplicated, or unexpected frozen criteria so agents can correct their submissions.
- Codex route decisions constrain references to existing research objects; rejected route transitions now retain their error and pause instead of repeatedly consuming model turns.
- Canonical adoption refreshes the next-action hint; resuming saved work no longer asks the main agent to dispatch the same stage again.
- Clarified that plan reviews enforce both acceptance checks and success criteria and can read the mission directly from their snapshot.
- Source export handles removed files and includes current user guides and Astra workflows.
- Runtime boundary checks no longer require historical Rust files.
- Packaged workbench checks install production dependencies before testing the isolated server.

Older development notes remain in [changes.md](changes.md). Published alpha.1 notes and assets are unchanged.
