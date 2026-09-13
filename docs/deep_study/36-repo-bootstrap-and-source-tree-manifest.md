# Repo Bootstrap And Source-Tree Manifest

This document turns `31-kernel-foundation-implementation-plan.md` and
`32-first-build-task-pack-backlog.md` into a concrete repository bootstrap
manifest.

The purpose is simple:

- remove ambiguity before the first real implementation branch starts
- make the initial source tree and ownership obvious
- prevent workers from inventing alternate layouts during M0-M2

This is the closest thing to a first-build scaffold contract.

## 1. Build Rule

During M0-M2, the repository must optimize for:

- one obvious place per subsystem
- short import paths
- minimal cross-package coupling
- schema and fixture visibility
- clean future extension into remote, agents, memory, and research runtime

No bootstrap task may create:

- a second persistence root
- a second protocol directory
- a second event package
- a browser/server runtime that competes with the CLI kernel

## 2. Required Top-Level Tree

The initial tree should be created exactly along these lines:

```text
research_cli/
├── Cargo.toml
├── src/
│   ├── bin/
│   │   └── research-cli.rs
│   ├── lib.rs
│   ├── app/
│   │   └── mod.rs
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── help.rs
│   │   └── registry.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── config.rs
│   │   └── sources.rs
│   ├── doctor/
│   │   ├── mod.rs
│   │   ├── doctor.rs
│   │   └── smoke.rs
│   ├── events/
│   │   ├── mod.rs
│   │   └── writer.rs
│   ├── mcp/
│   │   ├── mod.rs
│   │   ├── registry.rs
│   │   └── test.rs
│   ├── projects/
│   │   ├── mod.rs
│   │   ├── current.rs
│   │   └── registry.rs
│   ├── permissions/
│   │   ├── mod.rs
│   │   ├── policy.rs
│   │   └── requests.rs
│   ├── plugins/
│   │   ├── mod.rs
│   │   ├── hooks.rs
│   │   └── registry.rs
│   ├── providers/
│   │   ├── mod.rs
│   │   ├── auth.rs
│   │   ├── resolve.rs
│   │   └── trace.rs
│   ├── runtime/
│   │   ├── mod.rs
│   │   ├── checkpoint.rs
│   │   ├── features.rs
│   │   ├── preflight.rs
│   │   └── reducer.rs
│   ├── session/
│   │   ├── mod.rs
│   │   ├── compact.rs
│   │   ├── identity.rs
│   │   ├── index.rs
│   │   ├── lineage.rs
│   │   ├── store.rs
│   │   └── transcript.rs
│   ├── telemetry/
│   │   ├── mod.rs
│   │   ├── stats.rs
│   │   └── usage.rs
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── palette.rs
│   │   └── repl.rs
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── file.rs
│   │   ├── registry.rs
│   │   ├── shell.rs
│   │   └── web.rs
│   └── workspace/
│       ├── mod.rs
│       ├── hash.rs
│       └── resolve.rs
├── schemas/
│   ├── event.schema.json
│   ├── kernel_state_bundle.schema.json
│   ├── project_state.schema.json
│   ├── session.schema.json
│   └── task_packet.schema.json
├── scripts/
│   └── run_mock_parity_harness.sh
├── tests/
│   ├── conformance/
│   │   ├── runtime/
│   │   └── schemas/
│   ├── golden/
│   │   ├── config/
│   │   ├── plugins/
│   │   ├── projects/
│   │   ├── permissions/
│   │   ├── provider/
│   │   └── sessions/
│   ├── mock_provider/
│   └── helpers/
└── docs/
```

This tree is intentionally narrower than the full end-state blueprint.

Language transition note:

- this document is now the active Rust source-tree contract
- older Go-style path references in planning docs are logical-owner aliases
- exact alias-to-Rust mapping is frozen in
  `46-rust-kernel-pivot-and-bootstrap.md`

Remote, agents, memory, artifacts, reviews, branches, ProjectOps, and research
packages should not be created as empty noise directories unless the next
milestone immediately needs them.

The exception is the interactive discoverability layer. M0-M2 must already own
one explicit command/help registry and one TUI discoverability shell so
operator surfaces do not fragment across ad hoc handlers.

## 3. Package Ownership Contract

## `src/bin/research-cli.rs`

Owns:

- CLI argument parsing
- subcommand dispatch
- process exit-code mapping

Must not own:

- persistence logic
- protocol serialization
- provider-specific business rules

## `src/app`

Owns:

- dependency wiring
- config loading
- runtime construction
- mode selection between one-shot and interactive execution

Must not own:

- long-lived runtime state
- schema definitions
- session storage details

## `src/runtime`

Owns:

- turn orchestration
- checkpoint read/write entrypoints
- resume and compact triggers
- runtime preflight
- runtime feature degradation state

Must not own:

- CLI parsing
- direct provider auth storage
- UI-facing shadow state

## `src/events`

Owns:

- `KernelEventEnvelope`
- event writing order
- event serialization helpers

Must not own:

- interpretation-specific analytics
- dashboard projection logic

## `src/session`

Owns:

- session creation/list/load/delete
- session identity and lineage
- transcript JSONL
- compaction summaries

Must not own:

- project root resolution
- provider routing

## `src/workspace`

Owns:

- cwd and explicit-path project resolution
- workspace hash/fingerprint
- `.pmcli/` root creation

Must not own:

- permission approval state
- session transcript content

## `src/permissions`

Owns:

- permission policy evaluation
- request lifecycle state
- approval decision persistence hooks

Must not own:

- actual tool execution
- provider selection

## `src/tools`

Owns:

- tool classification
- built-in tool registry
- normalized execution result shapes

Must not own:

- approval policy
- session identity

## `src/providers`

Owns:

- provider resolution
- auth-status and test checks
- provider capability metadata
- provider resolution tracing

Must not own:

- transcript storage
- command parsing

## `internal/config`

Owns:

- config file shape
- environment and flag precedence
- effective config explanation

Must not own:

- provider runtime behavior
- session storage

## `internal/doctor`

Owns:

- read-only environment inspection
- smoke and readiness reporting

Must not own:

- config writes
- hidden mutation

## `internal/mcp`

Owns:

- server registry read model
- inspect/test/refresh baseline surfaces

Must not own:

- general tool policy
- provider fallback logic

## `internal/telemetry`

Owns:

- usage accumulation
- estimated and exact cost reporting
- operator stats summaries

Must not own:

- provider routing
- session persistence authority

## `internal/projects`

Owns:

- cross-project registry entries
- current-project pointer
- `data_dir` resolution for registered projects
- recency and init-state metadata

Must not own:

- per-project runtime truth inside `.pmcli/`
- session transcript content

## 4. First Commit Sequence

The bootstrap should be landed in a deliberately narrow sequence.

## Commit 1: module skeleton and binary boot

Deliver:

- `Cargo.toml`
- `src/bin/research-cli.rs`
- `src/app/mod.rs`
- minimal `src/runtime/mod.rs`
- `tests/conformance/runtime/bootstrap.rs`

Success signal:

- binary starts
- exits cleanly in no-op mode

## Commit 2: event and bundle primitives

Deliver:

- `src/events/mod.rs`
- `src/events/writer.rs`
- `src/runtime/checkpoint.rs`
- `schemas/event.schema.json`
- `schemas/kernel_state_bundle.schema.json`

Success signal:

- bundle checkpoint round-trip works
- canonical event envelope validates

## Commit 3: workspace and `.pmcli/` bootstrap

Deliver:

- `src/workspace/resolve.rs`
- `src/workspace/hash.rs`
- project root + workspace hash tests

Success signal:

- project resolution is deterministic
- `.pmcli/` root initialization is stable

## Commit 4: session store and transcript

Deliver:

- `src/session/store.rs`
- `src/session/transcript.rs`
- `schemas/session.schema.json`

Success signal:

- sessions persist across restarts
- transcript JSONL is readable and schema-checked

## Commit 5: prompt/chat/resume lane

Deliver:

- shared turn path in `src/runtime/mod.rs`
- CLI wiring in `src/bin/research-cli.rs`

Success signal:

- `prompt`, `chat`, `resume`, `continue` all route through one runtime lane

## Commit 6: compact/doctor/smoke

Deliver:

- `src/session/compact.rs`
- `src/doctor/doctor.rs`
- `src/doctor/smoke.rs`

Success signal:

- operators can inspect and compact without reading raw logs
- readiness failures are machine-explainable

## Commit 7: permissions and tool registry

Deliver:

- `src/permissions/policy.rs`
- `src/permissions/requests.rs`
- `src/tools/*`

Success signal:

- writes are policy-gated
- permission decisions are inspectable

## Commit 8: providers, config, and MCP baseline

Deliver:

- `src/providers/*`
- `src/config/*`
- `src/mcp/*`

Success signal:

- provider/config ambiguity is explainable
- setup and MCP baseline commands stop being placeholders

## 5. Required Non-Code Bootstrap Files

These files should exist before the first milestone is declared stable:

- `README.md`
- `docs/deep_study/README.md`
- `docs/architecture/` for implementation-facing extracted contracts later
- `.gitignore`
- CI workflow file
- conformance and golden test runners

Recommended first supporting files:

- `Makefile` or equivalent task runner
- `tests/helpers/temp_project.go`
- `tests/helpers/assertions.go`
- `tests/helpers/fixtures.go`

## 6. `.pmcli/` Bootstrap Manifest

The first runtime-created project-local tree should look like this:

```text
.pmcli/
├── project_state.json
├── project_meta.json
├── events/
│   └── events.jsonl
├── indexes/
│   └── session_search.sqlite
├── sessions/
│   └── <session_id>/
│       ├── transcript.jsonl
│       ├── summary.md
│       └── meta.json
└── logs/
```

Notes:

- `project_state.json` stores an enveloped `KernelStateBundle` snapshot
- `project_meta.json` stores an enveloped project-metadata snapshot
- event log remains append-only
- `indexes/session_search.sqlite` is a rebuildable derived index, not runtime
  authority
- session transcript JSONL uses typed line records
- session folders remain project-scoped
- `logs/` is non-canonical diagnostics only and must not become runtime authority
- extra directories should only appear when later milestones need them

Do not pre-create:

- `agents/`
- `memory/`
- `branches/`
- `reviews/`
- `remote/`

Those should arrive with their owning milestone and schema contracts.

## 6.5 Global Project Registry Bootstrap

Cross-project operator state must be created outside project-local `.pmcli/`.

Canonical location:

- `$XDG_STATE_HOME/research-cli/registry/`
- fallback: `~/.local/state/research-cli/registry/`

Initial files:

```text
registry/
├── projects.json
└── current_project.json
```

Rules:

- `projects.json` stores `ProjectRegistryEntry` records only
- `current_project.json` stores the last successful current-project pointer only
- both files use `CanonicalObjectEnvelope` registry encoding
- this registry is an operator index and must never outrank project-local
  `project_state.json`

## 7. Test Scaffold Manifest

The initial test tree must prove protocol truth, not just compilation.

Create at least:

- `tests/conformance/runtime/bootstrap_test.go`
- `tests/conformance/runtime/kernel_bundle_test.go`
- `tests/conformance/runtime/workspace_resolution_test.go`
- `tests/conformance/runtime/session_store_test.go`
- `tests/conformance/runtime/turn_lifecycle_test.go`
- `tests/conformance/runtime/permission_machine_test.go`
- `tests/conformance/schemas/event_schema_test.go`
- `tests/golden/sessions/project_scope_test.go`
- `tests/golden/sessions/resume_latest_project_scoped_test.go`
- `tests/golden/sessions/prompt_mode_test.go`
- `tests/golden/sessions/compact_test.go`
- `tests/golden/permissions/permission_mode_test.go`
- `tests/golden/config/config_precedence_test.go`
- `tests/golden/provider/provider_resolution_test.go`
- `tests/golden/provider/doctor_provider_matrix_test.go`

The bootstrap phase is not allowed to defer these into "later QA."

## 8. Workspace Safety Requirements Pulled Forward

Based on the Dr. Claw review, the following safeguards should be present during
the initial workspace implementation, not postponed:

- forbidden system-path denylist
- symlink target escape checks
- home/workspace-root containment checks
- explicit machine-readable rejection reasons
- tests for ambiguous or unsafe project scope

This is one of the few product lessons important enough to pull directly into
M0-M2.

## 9. Exit Condition

This manifest is satisfied when a fresh engineer can clone the repository,
create the listed tree, and start implementing M0-M2 without making any layout
or ownership decisions on their own.

That is the entire point:

- architecture should already be decided
- source-tree shape should already be decided
- persistence root should already be decided
- the implementation phase should now be execution, not reinterpretation
