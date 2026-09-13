# Trellis Repair Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** close the remaining bridge seams so host interrupt behavior, mobile browser UX, and the last staged TUI surfaces feel like one coherent product instead of separate approximations.

**Architecture:** keep `src/runtime/mod.rs` as execution core, `src/remote/daemon.rs` as the host control authority, `src/tui.rs` as the local conversational surface, and `src/assets/mobile/app.js` as the remote browser shell. Fix the seam order from control semantics to browser UX to visible command audit. Do not widen scope beyond the current contract family.

**Tech Stack:** Rust runtime/daemon/TUI, static mobile JS shell, conformance tests under `tests/conformance`, operator tests under `tests/operator`, browser viewport checks, `cargo test`.

## Current Baseline

- The previous repair slice is committed as `abb5168` and full `cargo test` was green at handoff.
- Runtime transcript/replay, TUI command routing, mobile asset contracts, and the schema drift that blocked status validation are already repaired.
- The remaining work is no longer broad correctness; it is product fidelity at the seam where the host control model, browser shell, and visible commands meet.

## Repair Order

1. Make host interrupt semantics explicit enough that the control path does not feel like a hidden notification hack.
2. Verify mobile behavior in a real narrow viewport and repair layout or interaction defects that only appear in browser rendering.
3. Audit any still-staged TUI commands that are visible in the remote/mobile flow and either promote them or mark them unmistakably as staged.

This order matters because the browser and TUI surfaces should reflect the control contract, not invent their own.

## Task 1: Tighten Host Interrupt Semantics

**Files:**
- Modify: `src/remote/daemon.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/events/mod.rs` if the event model needs a new explicit control marker
- Modify: `tests/conformance/remote_daemon.rs`
- Modify: `tests/conformance/runtime/event_log.rs` if the event contract changes

- [x] **Step 1: Write a failing interrupt regression**

Cover the current ambiguity directly. The test should pin down whether interrupt is only notification-backed or should expose an explicit stop/cancel control event that the host surfaces can rely on.

- [x] **Step 2: Run the test and confirm the current contract gap**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
```

- [x] **Step 3: Implement the smallest contract change**

Either make the interrupt path explicitly direct enough to match user expectations, or keep it notification-backed but make the contract and response text fully explicit everywhere. Do not mix both models silently.

- [x] **Step 4: Re-run the focused control tests**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
cargo test --test conformance runtime_event_log -- --nocapture
```

Expected: PASS.

## Task 2: Verify Mobile Browser UX in a Real Viewport

**Files:**
- Modify: `src/assets/mobile/app.js`
- Modify: `src/assets/mobile/index.html`
- Modify: `src/assets/mobile/styles.css`
- Modify: `tests/conformance/remote_daemon.rs`

- [x] **Step 1: Add narrow-viewport assertions**

Cover the mobile shell at a realistic phone width. The test should exercise the visible terminal/session/control panes rather than only checking that the assets load.

- [x] **Step 2: Run the browser-facing coverage and confirm the current mismatch**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
```

- [x] **Step 3: Fix layout and interaction issues**

Correct overflow, touch target, focus, or control placement issues that only show up on a real viewport. Keep the UI utilitarian and dense.

- [x] **Step 4: Re-run the mobile-facing checks**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
```

Expected: PASS.

## Task 3: Audit Remaining Staged TUI Surfaces

**Files:**
- Modify: `src/tui.rs`
- Modify: `tests/operator/cli_surfaces.rs`

- [x] **Step 1: Enumerate remaining staged commands**

Identify any visible command that still looks executable while only being staged or compatibility-only.

- [x] **Step 2: Decide the right surface**

Each command must either execute a real action, open a supported inspector, or explicitly say it is staged and point to the real CLI route.

- [x] **Step 3: Re-run operator surface tests**

Run:

```bash
cargo test --lib "tui::tests::" -- --nocapture
cargo test --test operator -- --nocapture
```

Expected: PASS.

## Task 4: Baseline Verification

**Files:**
- None; this is verification only

- [x] **Step 1: Run the full matrix**

Run:

```bash
cargo test
```

- [x] **Step 2: Check diff hygiene**

Run:

```bash
git diff --check
```

## Notes

- Keep this round narrower than the previous one.
- Do not expand into new runtime features before the host/mobile seam is clean.
- If the interrupt contract stays notification-backed, the wording and tests must say that plainly.
