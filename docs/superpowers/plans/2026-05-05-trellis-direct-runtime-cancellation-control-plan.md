# Trellis Direct Runtime Cancellation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:subagent-driven-development to implement this plan task-by-task. Follow TDD: each behavior change starts with a failing test.

**Goal:** make host/mobile interrupt capable of cancelling an active prompt turn directly, while preserving the existing notification-backed interrupt request as an explicit fallback.

**Architecture:** keep `src/runtime/mod.rs` as the prompt execution authority, `src/runtime/cancel.rs` as the cancellation primitive, `src/remote/daemon.rs` as host control authority, `src/host_surface.rs` as the action projection, and `src/assets/mobile/app.js` as the browser shell. Add only the narrow registry/control surface needed to bridge daemon interrupt requests to active runtime cancel tokens.

**Current Baseline:**
- Local TUI has direct cancellation through `RuntimeInterruptHandle`.
- Remote/mobile interrupt only emits `remote_interrupt_requested` and calls `remote::notify`.
- `control_contract: notification_backed_interrupt_request` is now explicit, so the next slice can safely change the contract when direct cancellation exists.

## Task 1: Pin Current Remote Interrupt Limitation

**Files:**
- Modify: `tests/conformance/remote_daemon.rs`
- Modify: `tests/conformance/runtime/event_log.rs` if a new event contract is introduced in the test first.

- [ ] **Step 1: Write a failing direct-cancel regression**

Add a test showing that a remote/mobile interrupt should report direct cancellation delivery when a cancellable active turn exists.

- [ ] **Step 2: Run the focused failing test**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
```

Expected now: FAIL because the daemon only returns `notification_backed_interrupt_request`.

## Task 2: Add Active Turn Cancellation Registry

**Files:**
- Modify: `src/runtime/cancel.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `tests/conformance/runtime/event_log.rs` or `src/runtime/cancel.rs` unit tests.

- [ ] **Step 1: Test registry lifecycle**

Cover registering a project/session/turn handle, cancelling it, and removing it on completion/drop.

- [ ] **Step 2: Implement the smallest registry**

Keep it process-local, scoped, and guard-based. Do not persist handles to disk.

- [ ] **Step 3: Re-run runtime cancellation tests**

Run:

```bash
cargo test --lib runtime::cancel -- --nocapture
```

Expected: PASS.

## Task 3: Route Daemon Interrupt Through Direct Cancellation

**Files:**
- Modify: `src/remote/daemon.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Bind active prompt turns into the registry**

Register cancellable turns around prompt provider execution and unregister on success, failure, cancellation, and permission-block paths.

- [ ] **Step 2: Update daemon interrupt routing**

For `interrupt_turn`, attempt direct cancellation first. If a matching active handle exists, return a direct cancellation contract and event. If not, preserve the existing notification-backed fallback.

- [ ] **Step 3: Re-run daemon control tests**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
```

Expected: PASS.

## Task 4: Update Host/Mobile/TUI Contracts

**Files:**
- Modify: `src/host_surface.rs`
- Modify: `schemas/host_surface_action.schema.json`
- Modify: `schemas/host_surface_projection.schema.json`
- Modify: `src/assets/mobile/app.js` if action copy/result rendering needs the new contract.
- Modify: `src/tui.rs` only if visible local command copy needs adjustment.

- [ ] **Step 1: Extend action contract metadata**

Represent direct cancellation and notification fallback distinctly.

- [ ] **Step 2: Re-run schema and mobile-facing checks**

Run:

```bash
cargo test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture
cargo test --test conformance remote_mobile_web_app_assets_expose_theme_language_and_session_controls -- --nocapture
```

Expected: PASS.

## Task 5: Baseline Verification

- [ ] **Step 1: Run focused matrix**

Run:

```bash
cargo test --test conformance remote_daemon -- --nocapture
cargo test --test conformance runtime_event_log -- --nocapture
cargo test --lib tui::tests:: -- --nocapture
```

- [ ] **Step 2: Run full matrix**

Run:

```bash
cargo test
git diff --check
```

Expected: PASS.

## Non-Goals
- No terminal PTY process kill semantics.
- No distributed cancellation across multiple daemon processes.
- No removal of the notification fallback until direct turn cancellation is proven.
