# Blind Review Packet And Rubric

This document defines how to request a truly no-context external review of the
current `research-cli` architecture.

It exists to prevent two failure modes:

- context-soaked review that inherits our internal conclusions
- vague review that scores concepts without checking executable authority

## 1. Blind Review Objective

The reviewer should decide, from the packet alone:

1. whether the base code agent CLI is specified strongly enough to match or
   exceed `Hermes`, `claw-code`, and `OpenCode/Crush`
2. whether remote/mobile/web remains an adapted `Happy`/`Happier` control plane
   instead of a second runtime
3. whether native project memory, ProjectOps, repo governance, multi-agent
   debate/search, and research-runtime loops remain kernel-native
4. whether the design is implementation-ready or still missing authority,
   payload, replay, or fixture obligations

## 2. Required Blind Review Packet

The required packet is:

1. `docs/deep_study/14-executable-runtime-protocol.md`
2. `docs/deep_study/15-research-review-and-contracts.md`
3. `docs/deep_study/18-proactive-project-ops.md`
4. `docs/deep_study/23-cli-operator-contract.md`
5. `docs/deep_study/24-kernel-state-machine-contract.md`
6. `docs/deep_study/25-remote-transport-auth-contract.md`
7. `docs/deep_study/26-memory-branch-research-runtime-policy.md`
8. `docs/deep_study/28-schema-registry-and-conformance-plan.md`
9. `docs/deep_study/37-kernel-type-and-interface-manifest.md`
10. `docs/deep_study/40-reference-superiority-self-audit.md`

Optional background, only if the reviewer explicitly asks:

- `docs/deep_study/17-operator-golden-fixtures.md`
- `docs/deep_study/20-happy-happier-integration-blueprint.md`
- `docs/deep_study/21-code-agent-kernel-hardening.md`
- `docs/deep_study/29-fixture-ci-rollout-plan.md`

## 3. Context Hygiene Rules

The review request must not include:

- our historical review summaries
- our own claims that the design already surpasses references
- prior reviewer findings unless the reviewer independently asks for them
- implementation roadmap details outside the packet unless they are inside the
  packet docs

The review request may include only:

- the packet paths
- the product goal in one short paragraph
- the review questions in Section 4
- the scoring rubric in Section 5

## 4. Mandatory Blind Review Questions

The review must answer all of these questions explicitly.

### 4.1 Base CLI Quality

- Does the base code-agent CLI contract clearly reach or exceed the operator
  quality of `claw-code` and `Hermes`?
- Are launch, resume, compact, inspect, logs, search, provider/auth, and
  degraded-state lanes concrete enough to implement without inventing new
  authority?
- Are durable session logs, search indexes, and other read models safely kept
  non-authoritative?
- Does the packet clearly distinguish between:
  contract-level parity,
  architecture-level exceedance intent,
  and implementation-proven superiority?

### 4.2 Remote / Happy-Happier Adaptation

- Does the remote design clearly adapt `Happy` / `Happier` instead of creating
  a second runtime?
- Are provider-session source, transcript-source provenance, local-control
  capability, and existing-session automation strategy explicit enough?
- Can attach, handoff, takeover, and local reclaim be implemented without
  hidden heuristics?

### 4.3 Native Personalized Requirements

- Does the architecture really make project memory, ProjectOps, repo cleanup,
  multi-agent debate/search, and research runtime kernel-native?
- Are these features integrated into the same authority line as the base CLI,
  or do any of them still risk becoming sidecar systems?

### 4.4 Implementation Readiness

- If a strong implementation team followed this design, could they implement
  it without needing new architectural invention?
- Which remaining gaps are critical blockers versus normal implementation work?

## 5. Reviewer Scoring Rubric

The reviewer should score each area `0-5`.

### 5.1 Authority Clarity

- `0`: multiple hidden truths, unclear ownership
- `3`: main authority line exists, but several side systems still drift
- `5`: one kernel authority line, side surfaces clearly projection or read model

### 5.2 Base CLI Competitiveness

- `0`: still below top open-source code-agent CLIs
- `3`: roughly at parity, but not clearly stronger
- `5`: visibly stronger in operator contracts, replay, failure typing, and
  inspectability

Scoring rule:

- the reviewer may not award `5` if superiority depends only on future fixture
  or conformance completion

### 5.3 Remote Operability

- `0`: remote design is mostly conceptual
- `3`: attach/takeover exists, but source and control details are incomplete
- `5`: Happy/Happier adaptation is concrete, typed, and non-duplicative

### 5.4 Personalized Feature Integration

- `0`: memory/research/project ops are bolt-ons
- `3`: integrated directionally, but not fully closed under one runtime
- `5`: all personalized features are obviously kernel-native and state-safe

### 5.5 Verification Readiness

- `0`: no serious fixture/conformance path
- `3`: partial fixtures exist but key contracts are not test-backed
- `5`: critical contracts are clearly expected to be fixture/conformance gated

## 6. Reviewer Output Format

The reviewer should return:

1. verdict:
   `go`, `conditional_go`, or `no_go`
2. score table for Section 5
3. critical blockers ordered by severity
4. likely implementation traps
5. explicit statement:
   whether the base CLI reaches/exceeds `Hermes` and `claw-code`
6. explicit statement:
   whether the design truly keeps remote as `Happy`/`Happier` adaptation rather
   than a second runtime

## 7. Acceptance Rule Before Claiming Superiority

We may only claim the design is ready to exceed the references at the
architecture level if the blind reviewer concludes:

- `go` or strong `conditional_go`
- no critical authority ambiguity
- no critical base-CLI contract gap relative to `Hermes` / `claw-code`
- no critical remote dual-runtime risk
- no critical kernel-native integration gap for memory / ProjectOps /
  multi-agent / research runtime
