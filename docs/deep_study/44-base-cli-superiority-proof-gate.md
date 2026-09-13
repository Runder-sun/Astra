# Base CLI Superiority Proof Gate

This document defines the exact proof gate for claiming that the base
`research-cli` code-agent CLI exceeds `Hermes`, `claw-code`, and
`OpenCode/Crush`.

It exists because the blind reviews converged on one conclusion:

- the architecture is strong
- the authority line is mostly closed
- but base-CLI superiority is still not implementation-proven

So this document does not add new architecture.

It freezes the evidence threshold.

## 1. Claim Discipline

The project may not claim:

- "base CLI exceeds Hermes"
- "base CLI exceeds claw-code"
- "base CLI is already best-in-class"

unless the workflow classes in Sections 2-4 are all backed by:

1. frozen command and payload contracts
2. implemented schemas
3. implemented golden fixtures
4. passing conformance or replay coverage
5. one reproducible operator demo path

Architecture alone is insufficient.

## 2. Claw-Class Proof Gate

These are the workflow classes that must at least match `claw-code`.

### 2.1 Doctor-first bring-up

Required evidence:

- `research-cli doctor --json`
- blocked and degraded payloads remain typed
- one golden fixture for ready, degraded, and blocked lanes

Pass condition:

- a wrapper can diagnose provider/config/workspace/tooling readiness without
  scraping logs

### 2.2 Prompt plus resume continuity

Required evidence:

- one-shot prompt payloads
- `resume` and `continue` continuity
- interrupted turn recovery
- compaction continuity after replay

Pass condition:

- interrupted or compacted sessions remain resumable with typed continuity

### 2.3 Permission-mode rigor

Required evidence:

- permission mode transitions
- pending/history inspection
- refusal payloads
- remote permission path sharing the same decision trace

Pass condition:

- permission behavior is structurally inspectable, not only interactively
  observable

### 2.4 Machine-readable automation

Required evidence:

- `CommandSuccess` / `CommandFailure`
- typed non-zero JSON
- help/palette/inspect automation lanes

Pass condition:

- wrappers never need to parse prose for normal blocked, ambiguous, or degraded
  lanes

### 2.5 Deterministic parity or replay harness

Required evidence:

- deterministic mock-provider harness
- replay or parity coverage for provider/auth/session flows

Pass condition:

- core operator behavior can be regression-tested without live vendor drift

## 3. Hermes-Class Proof Gate

These are the workflow classes that must at least match `Hermes`.

### 3.1 Provider and config product surface

Required evidence:

- provider inspect/auth-status/test/refresh
- config get/set/effective/sources
- explainable `ProviderResolutionTrace`

Pass condition:

- model/provider/config changes are explainable in one hop

### 3.2 Session browse, search, title, export, and prune

Required evidence:

- session browse/search fixtures
- title and lineage handling
- export including inspection metadata
- prune safety

Pass condition:

- operators can find, disambiguate, export, and clean sessions without hidden
  storage knowledge

### 3.3 Durable operator logs plus non-authoritative read models

Required evidence:

- `sessions logs`
- `SessionOperatorLogManifest`
- request-sequence continuity fixture
- non-authoritative search/index fixture

Pass condition:

- durable logs improve observability without becoming runtime truth

### 3.4 Usage and cost visibility

Required evidence:

- usage and cost payloads
- degraded accounting labeling

Pass condition:

- session and project accounting are inspectable and uncertainty is typed

## 4. Crush-Class Proof Gate

These are the workflow classes that must at least match `OpenCode/Crush`.

### 4.1 Project registry and scope resolution

Required evidence:

- `projects current/list/register/init/status/prune`
- `ProjectResolutionTrace`
- ambiguous/unresolved scope fixtures

Pass condition:

- project identity is operator-visible and machine-explainable

### 4.2 Workspace and session continuity

Required evidence:

- project-local session continuity
- workspace mismatch refusal
- current-project pointer reconciliation

Pass condition:

- session scope cannot silently drift across workspaces or worktrees

### 4.3 Native registries

Required evidence:

- MCP registry inspect/test/refresh
- skills/plugins/hooks registry payloads
- degraded registry behavior

Pass condition:

- productized registries behave as first-class runtime surfaces

### 4.4 Config precedence and effective view

Required evidence:

- precedence fixtures
- effective view and overridden-source reporting

Pass condition:

- no operator has to guess why one config value won

## 5. Research-CLI Exceedance Gate

The base CLI only counts as genuinely beyond the references when the foundation
above is green and the native extensions below are also green.

### 5.1 Project-native memory

Required evidence:

- memory query/explain/status/invalidate
- promotion and invalidation legality

### 5.2 Repo governance

Required evidence:

- artifact family inspection
- cleanup-plan and cleanup-apply traceability

### 5.3 Multi-agent and review surfaces

Required evidence:

- agent inspect/stop/traces
- branch promote/cancel/inspect
- review open/retry/inspect with trace refs

### 5.4 Research runtime

Required evidence:

- stage execution mapping
- repair/pivot legality
- wake escalation and supervision linkage

## 6. Minimum Evidence Ledger

Each workflow class above must be tracked with one status:

- `design_frozen`
- `schema_ready`
- `fixture_ready`
- `implemented_and_passing`

Superiority claim rule:

- no "exceeds Hermes / claw-code / Crush" claim is allowed while any workflow
  class remains below `implemented_and_passing`

## 7. Honest Public Wording

Before all gates are green, the strongest allowed wording is:

- "the architecture is positioned to exceed the references once the frozen
  proof gates are implemented and passing"

The following wording is forbidden before all gates are green:

- "already exceeds Hermes"
- "already exceeds claw-code"
- "already surpasses the reference set"
