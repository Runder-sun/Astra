# Astra CLI Product Command Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the redesigned TUI, remote web, and mobile surfaces execute through the existing research-cli runtime, action projection, session, permission, terminal, streaming, and skills systems instead of becoming a parallel product shell.

**Architecture:** Keep the code CLI conversation loop as the primary surface: plain input and `/prompt` run token-streamed prompt turns; `/` commands route to existing runtime actions; `$` commands route to the existing skills subsystem. `HostSurfaceAction`, `TuiCommandModel`, `/api/tui/actions`, `src/commands/help.rs`, and the current remote daemon endpoints become the shared product command contract consumed by TUI, web, and mobile.

**Tech Stack:** Rust CLI/runtime/TUI/daemon, `src/commands/help.rs` command registry, `src/host_surface.rs` host projection, `src/skills/mod.rs`, static HTML/CSS/JavaScript mobile shell under `src/assets/mobile`, conformance/operator tests under `tests/conformance` and `tests/operator`.

---

## Current CLI Scan

The project already has most of the right integration primitives. The problem is that the new product UI has not consistently routed through them yet.

- `src/commands/help.rs` already defines a CLI command registry with slash aliases, categories, JSON invocations, and palette entries. This should be the base command vocabulary, not a duplicated JavaScript-only palette.
- `src/host_surface.rs` already projects `HostSurfaceAction` entries and a `TuiCommandModel`/`TuiSkillModel`. Existing action ids include `submit_prompt`, `steer_turn`, `interrupt_turn`, `approve_permission`, `deny_permission`, `ack_hitl`, `open_artifact`, `inspect_memory`, `switch_session`, `remote_attach`, `terminal_attach`, `terminal_replay`, and `resize_terminal`.
- `src/remote/daemon.rs` already serves `/api/tui/actions`, `/api/message`, session, permission, attach, terminal, artifact, and host-surface endpoints. Web/mobile should execute against these endpoints via action ids instead of inserting literal slash text into the prompt.
- `src/runtime/mod.rs` already has mature CLI handlers for `prompt`, `skills`, `tui`, `sessions`, `remote`, `resume`, `continue`, terminal attach/replay, and streaming remote prompt turns. TUI command execution should call small reusable functions around these handlers instead of adding separate product logic.
- `src/skills/mod.rs` already discovers, lists, inspects, validates, runs, submits, and publishes skills. `$` should be a first-class typed alias over this subsystem.
- `src/tui.rs` currently treats plain text and `/prompt <text>` as real prompt execution, but many other `/` and `$` commands still return explanatory placeholder strings. This is the main product-level gap.
- `src/assets/mobile/app.js` has `executeProjectedAction`, but slash/skill UI buttons still call `insertSlashCommand`, so `/sessions`, `/terminal`, `$list`, and similar commands can become ordinary agent prompts instead of UI/runtime actions.
- Terminal WebSocket currently allows a token in the query string. That is useful for prototyping but not acceptable as a release product path because URLs are commonly logged.

## Integration Principles

- Do not create a second command system. Extend and normalize `src/commands/help.rs`, `HostSurfaceAction`, `TuiCommandModel`, and `TuiSkillModel`.
- Preserve the native code CLI feeling: first-level UI is a chat/composer/transcript, with `/` and `$` as discoverable command layers. Panels and dashboards are supporting views, not the main interaction model.
- Every visible command must have one of three execution classes:
  - `PromptTurn`: plain text or `/prompt <text>` goes through governed prompt streaming.
  - `ProjectedAction`: `/sessions`, `/terminal`, `/permissions`, `/approve`, `/deny`, `/memory`, and similar commands resolve to action ids or existing daemon/runtime endpoints.
  - `SkillInvocation`: `$list`, `$inspect <skill>`, and `$<skill> ...` resolve to `src/skills/mod.rs`.
- TUI, web, and mobile may render differently, but they must share command metadata, labels, gating, disabled reasons, and execution semantics.
- Product UI should show human workflow language. Contract ids and internal enum strings stay in API payloads and `data-*` attributes.

## File Structure

- Modify: `src/commands/help.rs`
  - Add missing product command aliases and expose a stable command-palette projection suitable for TUI/web/mobile.
- Modify: `src/host_surface.rs`
  - Add typed invocation metadata to projected actions where needed, and ensure `TuiCommandModel`/`TuiSkillModel` can be used as the command source of truth.
- Create: `src/surface_commands.rs`
  - Small shared adapter that parses typed `/` and `$` input into `SurfaceCommandRequest` and `SurfaceCommandKind`.
- Modify: `src/lib.rs`
  - Export the new adapter module.
- Modify: `src/tui.rs`
  - Replace placeholder command responses with real command parsing and execution hooks while keeping the chat-first layout.
- Modify: `src/runtime/mod.rs`
  - Add narrow reusable entry points for executing surface actions and skill invocations without shelling out.
- Modify: `src/remote/daemon.rs`
  - Add a generic action execution endpoint only where it reduces duplication, add skill endpoints or action-backed skill execution, and replace terminal query-token auth with one-time WebSocket tickets.
- Modify: `src/assets/mobile/app.js`
  - Make slash and skill controls execute projected actions or skill endpoints instead of only inserting text.
- Modify: `src/assets/mobile/index.html`
  - Keep command/skill controls but bind them to action ids and skill ids where possible.
- Modify: `src/assets/mobile/styles.css`
  - Polish command palette, skill picker, composer, terminal, and mobile safe-area layout after execution semantics are correct.
- Modify: `tests/operator/cli_surfaces.rs`
  - Add CLI/TUI surface tests for command mapping and non-placeholder behavior.
- Modify: `tests/conformance/remote_daemon.rs`
  - Add remote action, skill, streaming, terminal ticket, and mobile DOM contract tests.
- Modify: `tests/conformance/runtime/bootstrap.rs`
  - Update expected bootstrap/product copy only after visible labels stabilize.

## Task 1: Lock the Shared Command Contract

**Files:**
- Modify: `src/commands/help.rs`
- Modify: `src/host_surface.rs`
- Test: `tests/operator/cli_surfaces.rs`

- [ ] **Step 1: Write failing command-contract tests**

Add tests that assert canonical product commands are present in one shared source and point to real action ids or runtime commands:

```rust
#[test]
fn product_commands_have_cli_backing() {
    let commands = research_cli::commands::command_registry();
    for name in ["prompt", "sessions", "permissions", "skills", "remote", "tui"] {
        assert!(commands.iter().any(|command| command.name == name), "{name}");
    }
}

#[test]
fn tui_product_commands_map_to_projected_actions() {
    let model = research_cli::host_surface::test_tui_command_model();
    let entries: Vec<_> = model.groups.iter().flat_map(|group| &group.commands).collect();
    for (typed, action_id) in [
        ("/prompt <text>", "submit_prompt"),
        ("/sessions", "switch_session"),
        ("/permissions", "inspect_permissions"),
        ("/approve <request-id>", "approve_permission"),
        ("/deny <request-id>", "deny_permission"),
        ("/terminal", "terminal_attach"),
        ("/terminal replay", "terminal_replay"),
        ("/memory", "inspect_memory"),
        ("/research", "open_research"),
    ] {
        assert!(entries.iter().any(|entry| entry.typed == typed && entry.action_id == action_id), "{typed}");
    }
}
```

- [ ] **Step 2: Run tests to verify failure or current gaps**

Run:

```bash
cargo test product_commands_have_cli_backing --test operator
cargo test tui_product_commands_map_to_projected_actions --test operator
```

Expected: FAIL until test helper/export and missing action metadata are completed.

- [ ] **Step 3: Add a stable command projection helper**

Expose a small helper in `src/commands/help.rs` or `src/host_surface.rs` that returns product command entries with:

- typed form: `/sessions`, `/terminal replay`, `$list`
- source: `cli_command`, `host_surface_action`, or `skill`
- execution target: runtime command or action id
- category: conversation, navigation, permissions, work, research, skills
- human label and summary
- gate and disabled reason when action-backed

Do not duplicate this registry in `app.js`.

- [ ] **Step 4: Run targeted tests**

Run:

```bash
cargo test --test operator cli_surface
cargo test --test conformance host_surface_status_and_tui_projections_share_remote_workbench_contract
```

Expected: PASS.

## Task 2: Add a Typed Surface Command Adapter

**Files:**
- Create: `src/surface_commands.rs`
- Modify: `src/lib.rs`
- Test: `tests/operator/cli_surfaces.rs`

- [ ] **Step 1: Write failing parser tests**

Cover prompt, slash, permission, terminal, session, research, and skill syntax:

```rust
#[test]
fn parses_surface_prompt_and_commands() {
    assert_eq!(parse_surface_command("fix the failing test").kind, SurfaceCommandKind::PromptTurn);
    assert_eq!(parse_surface_command("/prompt fix it").kind, SurfaceCommandKind::PromptTurn);
    assert_eq!(parse_surface_command("/sessions").target_id(), Some("switch_session"));
    assert_eq!(parse_surface_command("/terminal replay").target_id(), Some("terminal_replay"));
    assert_eq!(parse_surface_command("$list").kind, SurfaceCommandKind::SkillList);
    assert_eq!(parse_surface_command("$research-lit transformers").kind, SurfaceCommandKind::SkillRun);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test parses_surface_prompt_and_commands --test operator
```

Expected: FAIL because `src/surface_commands.rs` does not exist.

- [ ] **Step 3: Implement the adapter**

Create structs similar to:

```rust
pub struct SurfaceCommandRequest {
    pub raw: String,
    pub kind: SurfaceCommandKind,
    pub action_id: Option<String>,
    pub skill_id: Option<String>,
    pub args: Vec<String>,
    pub prompt: Option<String>,
}

pub enum SurfaceCommandKind {
    PromptTurn,
    ProjectedAction,
    SkillList,
    SkillInspect,
    SkillRun,
    Help,
    Unknown,
}
```

Keep parsing conservative:

- plain text -> `PromptTurn`
- `/prompt <text>` -> `PromptTurn`
- known `/` commands -> `ProjectedAction` or `Help`
- `$list` -> `SkillList`
- `$inspect <skill>` or `$<skill>` without args -> `SkillInspect`
- `$<skill> ...` -> `SkillRun`
- unknown `/` and `$` -> `Unknown` with suggestions, not prompt text

- [ ] **Step 4: Run parser tests**

Run:

```bash
cargo test parses_surface_prompt_and_commands --test operator
cargo test --lib surface_commands
```

Expected: PASS.

## Task 3: Replace TUI Placeholder Commands With Runtime-Backed Execution

**Files:**
- Modify: `src/tui.rs`
- Modify: `src/runtime/mod.rs`
- Test: `tests/operator/cli_surfaces.rs`

- [ ] **Step 1: Write failing TUI behavior tests**

Add tests that prove commands no longer return only explanatory placeholders:

```rust
#[test]
fn tui_sessions_command_renders_real_session_projection() {
    let output = run_tui_surface_command("/sessions");
    assert!(output.contains("Sessions"));
    assert!(!output.contains("shortcuts are optional accelerators"));
}

#[test]
fn tui_skill_list_uses_skill_registry() {
    let output = run_tui_surface_command("$list");
    assert!(output.contains("Skills"));
    assert!(!output.contains("No skills are currently projected") || output.contains("total"));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test tui_sessions_command_renders_real_session_projection --test operator
cargo test tui_skill_list_uses_skill_registry --test operator
```

Expected: FAIL on placeholder text.

- [ ] **Step 3: Route TUI input through `surface_commands`**

Change `prompt_payload` and `route_typed_command` in `src/tui.rs` so all submitted text uses the shared adapter:

- `PromptTurn` continues to call the existing streaming executor.
- `Help` renders the shared command palette/help projection.
- `ProjectedAction` calls a new runtime helper for read-only or mutating action ids.
- `SkillList`, `SkillInspect`, and `SkillRun` call skills helpers.
- `Unknown` renders command suggestions and does not send text to the model.

- [ ] **Step 4: Add runtime helpers without shelling out**

In `src/runtime/mod.rs`, add narrow public helpers such as:

```rust
pub fn execute_surface_action(
    registry: &ProjectRegistry,
    cwd: &Path,
    request: SurfaceCommandRequest,
) -> Result<SurfaceCommandOutput, RuntimeFailure>
```

Start by supporting:

- `/sessions` -> existing session list/resume projection
- `/permissions` -> pending/history projection
- `/approve <request-id>` and `/deny <request-id>` -> existing permission mutation path
- `/terminal` and `/terminal replay` -> existing host_surface terminal attach/replay functions
- `/memory` -> existing memory status/projection
- `/research` -> existing host-surface research summary

- [ ] **Step 5: Preserve token-level prompt streaming**

Verify that plain input and `/prompt <text>` still use `submit_composer_streaming` and do not regress into a blocking placeholder response.

- [ ] **Step 6: Run targeted tests**

Run:

```bash
cargo test --test operator cli_surface
cargo test --test conformance tui
cargo test --test conformance prompt_streams_openai_compatible_chat_completion_deltas_and_persists_final_message
```

Expected: PASS.

## Task 4: Make `$` a First-Class Skills Layer

**Files:**
- Modify: `src/surface_commands.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/tui.rs`
- Test: `tests/operator/cli_surfaces.rs`

- [ ] **Step 1: Write failing skills tests**

Add tests for:

- `$list` returns discovered skills with degraded/enabled state.
- `$<skill>` returns inspect/help output.
- `$<skill> <input>` builds a governed skill run request.
- unknown skill names show nearest suggestions.

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test tui_skill_commands_route_to_existing_skills_subsystem --test operator
```

Expected: FAIL until runtime helpers call `src/skills/mod.rs`.

- [ ] **Step 3: Reuse existing skills APIs**

Do not create a new skill registry. Use:

- `skills::discover`
- `skills::list`
- `skills::inspect`
- existing run/submit helpers used by `handle_skills`

If a helper is currently private inside `handle_skills`, extract only the minimal reusable function and keep CLI output behavior unchanged.

- [ ] **Step 4: Add product-safe rendering**

TUI output should show:

- skill name
- one-line purpose
- source/degraded state
- invocation hint
- required inputs
- latest output location when relevant

Avoid dumping raw JSON unless the user explicitly requests JSON.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test --test operator cli_surface
cargo test --lib skills
```

Expected: PASS.

## Task 5: Make Web/Mobile Commands Execute Actions Instead of Inserting Text

**Files:**
- Modify: `src/assets/mobile/app.js`
- Modify: `src/assets/mobile/index.html`
- Modify: `src/assets/mobile/styles.css`
- Test: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Write failing DOM/JS contract tests**

Assert that command buttons carry execution metadata:

```rust
#[test]
fn mobile_command_controls_bind_to_action_ids() {
    let html = served_mobile_html(&DaemonContext::for_test());
    assert!(html.contains("data-action-id=\"switch_session\""));
    assert!(html.contains("data-action-id=\"terminal_attach\""));
    assert!(html.contains("data-skill-command"));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test mobile_command_controls_bind_to_action_ids --test conformance
```

Expected: FAIL for controls that only have slash insertion metadata.

- [ ] **Step 3: Replace `insertSlashCommand` as the primary path**

In `src/assets/mobile/app.js`:

- Keep text insertion only for explicit “insert into composer” affordances.
- For `/sessions`, `/permissions`, `/terminal`, `/terminal replay`, `/memory`, and `/research`, call `executeProjectedAction` or activate the corresponding local view.
- For `/prompt`, focus composer and show prompt boundary.
- For unknown commands, open the command palette instead of sending them as prompts.

- [ ] **Step 4: Load action metadata from `/api/tui/actions`**

Build the palette from daemon-projected action ids plus command registry metadata. Disabled actions should remain visible with human disabled reasons.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test remote_daemon_serves_installable_mobile_app_shell --test conformance
cargo test mobile_command_controls_bind_to_action_ids --test conformance
```

Expected: PASS.

## Task 6: Add Remote Action and Skill Execution Endpoints Where Needed

**Files:**
- Modify: `src/remote/daemon.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/assets/mobile/app.js`
- Test: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Write failing daemon tests**

Add tests for a generic action route or focused routes, depending on final API shape:

```rust
#[test]
fn remote_daemon_executes_projected_action_ids() {
    let response = post_json("/api/tui/action", json!({
        "action_id": "terminal_replay"
    }));
    assert_success_or_gate(response);
}

#[test]
fn remote_daemon_exposes_skill_list_for_product_surfaces() {
    let response = get_json("/api/skills");
    assert_eq!(response["data"]["schema_version"], "skill_list.v1");
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test remote_daemon_executes_projected_action_ids --test conformance
cargo test remote_daemon_exposes_skill_list_for_product_surfaces --test conformance
```

Expected: FAIL until routes exist.

- [ ] **Step 3: Add minimal route surface**

Prefer one of these two options:

- Option A: `/api/tui/action` accepts `{ action_id, values }` and dispatches through a whitelist to existing functions.
- Option B: Keep existing bespoke endpoints and add only missing `/api/skills`, `/api/skills/:id`, and `/api/skills/:id/run` endpoints.

Choose Option A only for actions that already have `HostSurfaceAction` contracts and clear input schemas. Do not allow arbitrary command vectors from the client.

- [ ] **Step 4: Preserve gates**

Every mutating endpoint must still require the control token/lease and existing permission/session validation. Read-only endpoints may work without control only if current daemon policy allows it.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test --test conformance remote_daemon
```

Expected: PASS.

## Task 7: Replace Terminal Query Token With One-Time WebSocket Tickets

**Files:**
- Modify: `src/remote/daemon.rs`
- Modify: `src/assets/mobile/app.js`
- Test: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Write failing security tests**

Add tests for:

- `/api/terminal/ws?token=<control-token>` is rejected.
- `/api/terminal/ws-ticket` requires normal authenticated control.
- ticket is single-use.
- ticket expires.
- request/error logs do not include the control token.

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test terminal_websocket_uses_one_time_ticket_not_query_token --test conformance
```

Expected: FAIL on current query-token behavior.

- [ ] **Step 3: Implement ticket issuance**

Add `POST /api/terminal/ws-ticket`:

- authenticated with the existing control token header/body pattern
- returns `{ ticket, expires_at }`
- stores ticket in daemon memory with TTL and one-use semantics
- scopes ticket to cwd/session where possible

- [ ] **Step 4: Update WebSocket connection**

In `src/assets/mobile/app.js`, update `connectTerminalBridge`:

- call `/api/terminal/ws-ticket`
- open `ws://.../api/terminal/ws?ticket=<ticket>`
- never place the control token in a URL
- redact ticket in logs

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test terminal_websocket_uses_one_time_ticket_not_query_token --test conformance
cargo test --test conformance remote_daemon
```

Expected: PASS.

## Task 8: Make Remote Streaming Product-Grade

**Files:**
- Modify: `src/remote/daemon.rs`
- Modify: `src/assets/mobile/app.js`
- Test: `tests/conformance/remote_daemon.rs`

- [ ] **Step 1: Write failing streaming stability tests**

Cover:

- `remote_message_delta` events preserve accumulated draft text.
- non-delta refresh events do not wipe the current assistant draft.
- final completion replaces draft exactly once.

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test remote_message_delta_stream_keeps_mobile_draft_stable --test conformance
```

Expected: FAIL if mixed event batches still wipe draft content.

- [ ] **Step 3: Upgrade event consumption**

If feasible, make `/api/events` persistent SSE. If not, keep the existing polling endpoint but parse and merge deltas in `app.js` with explicit draft lifecycle state:

- `idle`
- `streaming`
- `finalizing`
- `complete`
- `failed`

- [ ] **Step 4: Keep token-level provider streaming**

Do not batch provider deltas into large chunks unless the provider only supplies chunk-level data. UI should display token/delta updates as soon as daemon persists them.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test --test conformance prompt_streams_openai_compatible_chat_completion_deltas_and_persists_final_message
cargo test --test conformance remote_daemon
```

Expected: PASS.

## Task 9: Polish TUI Input Without Combo-Key-Only Paths

**Files:**
- Modify: `src/tui.rs`
- Test: `tests/operator/cli_surfaces.rs`

- [ ] **Step 1: Write failing input tests**

Cover:

- UTF-8 input, including Chinese text.
- paste input.
- backspace/delete.
- left/right cursor.
- multiline prompt entry.
- `/` palette selection.
- `$` skill picker selection.
- every action has a typed command path, not only a shortcut.

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test tui_input_supports_utf8_slash_and_skill_paths --test operator
```

Expected: FAIL until input buffering is upgraded.

- [ ] **Step 3: Replace byte-only composer logic**

Use a UTF-8 string buffer and character-index cursor. Preserve existing raw-mode restoration and exit behavior.

- [ ] **Step 4: Rebalance TUI layout**

Make first-level TUI layout:

- transcript/chat stream
- compact research status strip
- composer
- command/skill palette only when invoked

Move internal debug/projection detail behind `/status`, `/debug`, or `/surface`.

- [ ] **Step 5: Run targeted tests**

Run:

```bash
cargo test --test conformance tui
cargo test --test operator cli_surface
```

Expected: PASS.

## Task 10: Final Product Verification

**Files:**
- All touched files.

- [ ] **Step 1: Run formatting and whitespace checks**

Run:

```bash
cargo fmt --check
git diff --check
```

Expected: PASS.

- [ ] **Step 2: Run Rust unit and conformance tests**

Run:

```bash
cargo test --lib
cargo test --test conformance tui
cargo test --test conformance remote_daemon
cargo test --test conformance prompt_streams_openai_compatible_chat_completion_deltas_and_persists_final_message
cargo test --test conformance prompt_executes_openai_compatible_chat_completion_and_persists_assistant_message
cargo test --test conformance goals_mission_frame_persists_and_survives_prompt_resume_continue_and_compact
cargo test --test operator cli_surface
```

Expected: PASS.

- [ ] **Step 3: Run manual smoke flows**

Run:

```bash
cargo run -- tui
cargo run -- remote daemon --port 0
```

Manual checks:

- TUI plain text streams through provider/runtime.
- `/sessions`, `/permissions`, `/terminal`, `/research`, `$list`, and `$<skill>` produce real CLI-backed output.
- Web/mobile slash buttons execute or open real product views.
- Terminal bridge does not expose the control token in URL.
- Mobile composer and bottom navigation do not overlap on a phone viewport.

- [ ] **Step 4: Browser verification**

Use Playwright when the browser profile is available:

- desktop viewport: command palette, transcript stream, terminal pane
- mobile viewport: composer, tabbar safe area, slash palette, skill picker, streaming draft

Expected: screenshots show product-grade surfaces with no internal enum leakage in primary UI.

- [ ] **Step 5: Commit**

Run:

```bash
git add src/commands/help.rs src/host_surface.rs src/surface_commands.rs src/lib.rs src/tui.rs src/runtime/mod.rs src/remote/daemon.rs src/assets/mobile/app.js src/assets/mobile/index.html src/assets/mobile/styles.css tests/operator/cli_surfaces.rs tests/conformance/remote_daemon.rs tests/conformance/runtime/bootstrap.rs docs/superpowers/plans/2026-04-29-astra-cli-product-command-integration.md
git commit -m "feat: integrate product commands into cli surfaces"
```

Expected: commit contains only the command integration, execution, security, streaming, and product polish changes needed for this plan.
