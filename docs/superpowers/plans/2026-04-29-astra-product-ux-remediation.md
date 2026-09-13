# Astra Product UX Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the current Astra Code TUI, remote web UI, and mobile shell from product-styled projections into usable agent-first command surfaces.

**Architecture:** Introduce one shared surface model for commands, workflow reachability, structured output labels, and product navigation. TUI, web, and mobile should render the same product concepts differently instead of duplicating ad hoc labels and hidden panels. Start with tests that prove workflows are reachable before improving polish.

**Tech Stack:** Rust daemon/TUI, static HTML/CSS/JavaScript under `src/assets/mobile`, conformance tests under `tests/conformance`, operator tests under `tests/operator`.

---

## Review Problems This Plan Fixes

- Mobile hides core workflows: pairing, workspace status, sessions, approvals, tests, message, and event log are not reachable under the current 5-tab activation model.
- TUI is still a static projection frame: most input redraws the frame and only `ctrl-c` / `ctrl-d` exit.
- Internal contract IDs leak into visible UI: `conversation_primary_surface`, `artifact_code_diff_pane`, and `test_result_panel`.
- Slash commands are only insertion chips, not a discoverable command palette.
- Tests assert element presence but not mobile reachability or visible product copy.

## File Structure

- Modify: `src/assets/mobile/index.html`
  - Define mobile page containers, desktop workbench tabs, command palette markup, and visible product labels.
- Modify: `src/assets/mobile/styles.css`
  - Replace single-panel mobile activation with page-level layouts; add command palette, bottom sheets, responsive workflow pages, and visible focus states.
- Modify: `src/assets/mobile/app.js`
  - Add shared command registry, mobile page switching, command palette filtering, and label mapping helpers.
- Modify: `src/tui.rs`
  - Add an interactive composer state, slash palette state, and basic command/session navigation.
- Modify: `tests/conformance/remote_daemon.rs`
  - Add DOM contract tests for mobile workflow reachability and product-visible labels.
- Modify: `tests/operator/cli_surfaces.rs`
  - Add TUI input behavior assertions for slash palette and composer submission.
- Modify: `tests/conformance/runtime/bootstrap.rs`
  - Update expected product output only after visible copy is finalized.
- Create: `docs/superpowers/specs/2026-04-29-astra-product-ux-design.md`
  - UX/UI spec and visual direction for all surfaces.

## Task 1: Mobile Workflow Reachability Contract

**Files:**
- Modify: `tests/conformance/remote_daemon.rs`
- Modify: `src/assets/mobile/index.html`
- Modify: `src/assets/mobile/app.js`
- Modify: `src/assets/mobile/styles.css`

- [ ] **Step 1: Write failing reachability test**

Add a conformance test that parses the served HTML and verifies every critical workflow is assigned to a mobile page:

```rust
#[test]
fn remote_mobile_navigation_reaches_all_operator_workflows() {
    let context = DaemonContext::for_test();
    let html = served_mobile_html(&context);
    for id in [
        "settings",
        "status-panel",
        "sessions-panel",
        "workbench-panel",
        "test-result-panel",
        "message-form",
        "permission-form",
        "log-panel",
    ] {
        assert!(html.contains(&format!("data-mobile-page-section=\"{id}\"")), "{id} should be reachable from a mobile page");
    }
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test remote_mobile_navigation_reaches_all_operator_workflows --test conformance`

Expected: FAIL because current markup hides these panels without page membership.

- [ ] **Step 3: Introduce mobile pages**

In `index.html`, group sections into mobile page containers:

- `chat-page`: conversation, session tabs, composer.
- `servers-page`: server directory, pairing, workspace status.
- `terminal-page`: terminal stream and terminal controls.
- `workbench-page`: approvals, message, workbench stats, sessions, tests, logs.
- `artifacts-page`: artifacts, code diffs, result previews.

Keep desktop visual layout intact by using wrapper classes that become `contents` on wide screens.

- [ ] **Step 4: Replace panel-level mobile activation**

In `app.js`, replace `activateMobileSection(targetId)` with `activateMobilePage(pageId)` driven by `[data-mobile-page]`.

Mobile CSS should hide/show `.mobile-page` instead of every `.panel`.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test remote_daemon_serves_installable_mobile_app_shell --test conformance
cargo test remote_mobile_navigation_reaches_all_operator_workflows --test conformance
```

Expected: PASS.

## Task 2: Product Label Mapping and Contract Hygiene

**Files:**
- Modify: `src/assets/mobile/index.html`
- Modify: `src/assets/mobile/app.js`
- Modify: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Write failing visible-copy test**

Assert served HTML does not expose internal contract IDs as visible text:

```rust
#[test]
fn remote_shell_keeps_contract_ids_out_of_visible_copy() {
    let html = served_mobile_html(&DaemonContext::for_test());
    assert!(!html.contains(">conversation_primary_surface<"));
    assert!(!html.contains(">artifact_code_diff_pane<"));
    assert!(!html.contains(">test_result_panel<"));
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test remote_shell_keeps_contract_ids_out_of_visible_copy --test conformance`

Expected: FAIL on current HTML.

- [ ] **Step 3: Add `surfaceLabel` for all visible state**

Use `surfaceLabel()` in every visible state setter:

- `conversation_primary_surface` -> `Agent stream`
- `artifact_code_diff_pane` -> `Code & diffs`
- `test_result_panel` -> `Tests & results`
- `terminal_compatibility_lane` -> `Terminal stream`
- `permission_required` -> `Approval required`

Keep contract values in `data-surface`, API payloads, and tests that validate contracts.

- [ ] **Step 4: Run targeted tests**

Run:

```bash
cargo test remote_shell_keeps_contract_ids_out_of_visible_copy --test conformance
cargo test remote_daemon_exposes_artifact_code_viewer_projection --test conformance
cargo test remote_daemon_exposes_result_panel_projection_without_new_truth --test conformance
```

Expected: visible-copy test passes while API contract tests remain unchanged.

## Task 3: Shared Slash Command Registry

**Files:**
- Modify: `src/assets/mobile/app.js`
- Modify: `src/assets/mobile/index.html`
- Modify: `src/assets/mobile/styles.css`
- Modify: `src/tui.rs`
- Modify: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Define command registry**

Add a shared JavaScript registry for web/mobile:

```javascript
const commandRegistry = [
  { id: "help", command: "/help", title: "Help", description: "Open command help", scope: "global" },
  { id: "model", command: "/model", title: "Model", description: "Inspect or switch model", scope: "chat" },
  { id: "sessions", command: "/sessions", title: "Sessions", description: "Resume or inspect sessions", scope: "workspace" },
  { id: "terminal", command: "/terminal", title: "Terminal", description: "Attach or control terminal stream", scope: "terminal" },
  { id: "review", command: "/review", title: "Review", description: "Run review workflow", scope: "code" },
  { id: "memory", command: "/memory", title: "Memory", description: "Inspect durable memory context", scope: "context" },
];
```

- [ ] **Step 2: Write command palette tests**

Assert that HTML exposes a command palette container and JS contains registry entries with descriptions.

Run: `cargo test remote_daemon_serves_installable_mobile_app_shell --test conformance`

- [ ] **Step 3: Implement web/mobile command palette**

Behavior:

- Typing `/` in an empty composer opens the palette.
- Filtering narrows commands by command, title, and description.
- Arrow keys move selection.
- Enter inserts selected command or executes immediate actions where safe.
- Escape closes the palette.

- [ ] **Step 4: Mirror command concepts in TUI**

In `src/tui.rs`, introduce `TuiInputState`:

```rust
struct TuiInputState {
    buffer: String,
    cursor: usize,
    palette_open: bool,
    selected_command: usize,
}
```

Start with in-memory behavior; wire actual command execution in a later task.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test remote_daemon_serves_installable_mobile_app_shell --test conformance
cargo test tui::tests::terminal_frame_renders_product_ui_instead_of_projection_dump --lib
```

Expected: PASS.

## Task 4: TUI Interactive Composer

**Files:**
- Modify: `src/tui.rs`
- Modify: `tests/operator/cli_surfaces.rs`
- Modify: `tests/conformance/runtime/bootstrap.rs`

- [ ] **Step 1: Add unit tests for input decoding**

Test printable input, backspace, left/right movement, `/` palette open, escape close, and enter submit.

- [ ] **Step 2: Add render tests for composer state**

Assert the terminal frame includes:

- Current composer buffer.
- Palette rows when `/` is typed.
- Selected command marker.
- Help footer that matches implemented controls.

- [ ] **Step 3: Implement state updates**

Extend the raw-mode loop so non-exit input mutates `TuiInputState` before rendering.

- [ ] **Step 4: Submit command/message events**

For this milestone, submission can append a local transcript line in the TUI frame. Actual daemon integration can remain a later task if no TUI command execution backend exists yet.

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test tui::tests --lib
cargo test host_surface_status_and_tui_projections_share_remote_workbench_contract --test conformance
cargo test --test operator cli_surface
```

Expected: targeted TUI tests pass. If broad operator tests have known provider/help drift, record the unrelated failures.

## Task 5: Desktop Workbench Tab Refinement

**Files:**
- Modify: `src/assets/mobile/index.html`
- Modify: `src/assets/mobile/styles.css`
- Modify: `src/assets/mobile/app.js`

- [ ] **Step 1: Convert right rail into tabbed workbench**

Tabs:

- Approvals
- Artifacts
- Tests
- Logs

Keep critical counts visible in the tab labels.

- [ ] **Step 2: Make approval flow first-class**

Approval cards must show request ID, command/tool, risk label, approve/deny actions, and control-mode requirement.

- [ ] **Step 3: Keep raw terminal stream visible but subordinate**

Terminal output should stream in the terminal pane and be transformed into semantic cards in chat.

- [ ] **Step 4: Verify responsive layout**

Use browser screenshots at:

- `1440x900`
- `1024x768`
- `390x844`

Expected: no overlapping text, all critical workflows reachable.

## Task 6: Visual Regression and Accessibility Gate

**Files:**
- Modify: `tests/conformance/remote_daemon.rs`
- Create or modify: browser-based smoke test script if the repo already has one.

- [ ] **Step 1: Add mobile DOM reachability assertions**

For each bottom tab, assert a visible heading and at least one primary workflow control exists.

- [ ] **Step 2: Add no-internal-visible-copy assertion**

Keep API contract names allowed in JSON but banned inside human-visible labels.

- [ ] **Step 3: Add manual screenshot checklist**

Document screenshots in the PR:

- Desktop web.
- Mobile chat.
- Mobile servers/pairing.
- Mobile workbench/approval.
- TUI slash palette.

- [ ] **Step 4: Run final verification**

Run:

```bash
cargo fmt --check
cargo test remote_daemon:: --test conformance
cargo test tui::tests --lib
```

Expected: PASS for targeted suite. Document known unrelated full-suite failures separately.

## Implementation Order

1. Mobile reachability.
2. Visible product label cleanup.
3. Command registry and command palette.
4. TUI composer.
5. Desktop workbench tab refinement.
6. Regression and accessibility gate.

This order fixes broken workflows before visual polish.
