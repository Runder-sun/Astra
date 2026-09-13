# Rust Kernel Pivot And Bootstrap

This document freezes the active implementation-language decision and the
source-tree mapping for `research-cli`.

It exists because the architecture and blind-review packet were tightened
before the implementation language was finally selected.

The design authority is unchanged:

- one kernel runtime truth
- one protocol surface under `.pmcli/` plus `schemas/`
- one remote control plane adapted from `Happy` and `Happier`
- one project-native memory and ProjectOps line

What changes here is only the implementation substrate for the kernel.

## 1. Active Language Split

The live implementation split is now:

- Rust for the base code-agent CLI kernel, session/runtime/event/persistence
  floor, provider/config/permission/tooling surfaces, and future memory /
  multi-agent / ProjectOps core
- TypeScript/Node for the remote mobile/web control plane adapted from `Happy`
  and selectively upgraded with `Happier` contracts
- Python for research skills, experiment runners, evaluation helpers, and other
  model-heavy sidecars

Decision rule:

- if logic owns canonical kernel state, it belongs in Rust
- if logic adapts the remote transport/workbench plane, it belongs in the
  TypeScript/Node remote stack
- if logic is a research or experiment sidecar, it belongs in Python unless it
  must become kernel authority

## 2. Active Rust Layout

The Rust kernel now starts from this layout:

```text
research_cli/
├── Cargo.toml
├── src/
│   ├── bin/
│   │   └── research-cli.rs
│   ├── lib.rs
│   ├── app/
│   ├── runtime/
│   ├── events/
│   ├── session/
│   ├── workspace/
│   ├── projects/
│   ├── commands/
│   ├── permissions/
│   ├── tools/
│   ├── providers/
│   ├── config/
│   ├── doctor/
│   ├── telemetry/
│   ├── tui/
│   ├── mcp/
│   └── plugins/
├── schemas/
├── tests/
└── docs/
```

Implementation rule:

- the top-level Rust package may later split into multiple crates if compile
  times or dependency boundaries require it
- if that happens, the logical owners in `11`, `31`, `33`, `36`, `37`, and `38`
  must remain unchanged

## 3. Legacy Path Mapping

The earlier Go-style implementation docs remain valuable as logical ownership
documents. Their file paths should now be interpreted through this mapping:

| Legacy path | Active Rust path |
|---|---|
| `cmd/research-cli/main.go` | `src/bin/research-cli.rs` |
| `internal/app/app.go` | `src/app/mod.rs` |
| `internal/runtime/runtime.go` | `src/runtime/mod.rs` |
| `internal/runtime/checkpoint.go` | `src/runtime/checkpoint.rs` |
| `internal/runtime/preflight.go` | `src/runtime/preflight.rs` |
| `internal/runtime/features.go` | `src/runtime/features.rs` |
| `internal/runtime/reducer.go` | `src/runtime/reducer.rs` |
| `internal/events/events.go` | `src/events/mod.rs` |
| `internal/events/writer.go` | `src/events/writer.rs` |
| `internal/session/store.go` | `src/session/store.rs` |
| `internal/session/transcript.go` | `src/session/transcript.rs` |
| `internal/session/index.go` | `src/session/index.rs` |
| `internal/session/identity.go` | `src/session/identity.rs` |
| `internal/session/lineage.go` | `src/session/lineage.rs` |
| `internal/session/compact.go` | `src/session/compact.rs` |
| `internal/workspace/resolve.go` | `src/workspace/resolve.rs` |
| `internal/workspace/hash.go` | `src/workspace/hash.rs` |
| `internal/projects/current.go` | `src/projects/current.rs` |
| `internal/projects/registry.go` | `src/projects/registry.rs` |
| `internal/commands/help.go` | `src/commands/help.rs` |
| `internal/commands/registry.go` | `src/commands/registry.rs` |
| `internal/permissions/policy.go` | `src/permissions/policy.rs` |
| `internal/permissions/requests.go` | `src/permissions/requests.rs` |
| `internal/tools/registry.go` | `src/tools/registry.rs` |
| `internal/tools/shell.go` | `src/tools/shell.rs` |
| `internal/tools/file.go` | `src/tools/file.rs` |
| `internal/tools/web.go` | `src/tools/web.rs` |
| `internal/providers/resolve.go` | `src/providers/resolve.rs` |
| `internal/providers/trace.go` | `src/providers/trace.rs` |
| `internal/providers/auth.go` | `src/providers/auth.rs` |
| `internal/config/config.go` | `src/config/config.rs` |
| `internal/config/sources.go` | `src/config/sources.rs` |
| `internal/doctor/doctor.go` | `src/doctor/doctor.rs` |
| `internal/doctor/smoke.go` | `src/doctor/smoke.rs` |
| `internal/telemetry/usage.go` | `src/telemetry/usage.rs` |
| `internal/telemetry/stats.go` | `src/telemetry/stats.rs` |
| `internal/tui/repl.go` | `src/tui/repl.rs` |
| `internal/tui/palette.go` | `src/tui/palette.rs` |
| `internal/mcp/registry.go` | `src/mcp/registry.rs` |
| `internal/mcp/test.go` | `src/mcp/test.rs` |
| `internal/plugins/registry.go` | `src/plugins/registry.rs` |
| `internal/plugins/hooks.go` | `src/plugins/hooks.rs` |

For advanced systems:

- `internal/remotehost/...` maps to `src/remotehost/...`
- `internal/agents/...` maps to `src/agents/...`
- `internal/memory/...` maps to `src/memory/...`
- `internal/projectops/...` maps to `src/projectops/...`
- `internal/reviews/...` maps to `src/reviews/...`
- `internal/branches/...` maps to `src/branches/...`
- `internal/research/...` maps to `src/research/...`

## 4. Bootstrap Rule

Batch 0 must now be implemented in Rust with the smallest useful kernel floor:

1. `Cargo.toml`
2. `src/lib.rs`
3. `src/bin/research-cli.rs`
4. `src/app/mod.rs`
5. `src/runtime/mod.rs`
6. `tests/conformance/runtime/bootstrap.rs`

The bootstrap must prove:

- the binary entry exists
- the app boot path exists
- the runtime constructor exists
- the no-op run path exits cleanly

## 5. TDD Rule For The Pivot

The Rust pivot does not suspend the TDD discipline.

Batch 0 must be executed in this order:

1. write the first Rust bootstrap test
2. run it and confirm failure due to missing Rust implementation
3. implement the minimal Rust code to pass
4. run the targeted test
5. run the wider Rust test set that exists
6. only then begin Batch 1 bundle/event work

## 6. Historical Review Interpretation

The blind review results in `42` and `43` remain historical records.

Interpret them this way:

- their authority and proof-gate judgments still matter
- their `conditional_go` verdict was about the then-current implementation
  substrate assumption
- the Rust pivot strengthens the kernel-language choice, but it does not erase
  the requirement to produce schema, fixture, and conformance proof
