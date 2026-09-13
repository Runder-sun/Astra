# Remote Command And Fixture Contract

This document is the next hardening layer after:

- `19-remote-host-control-plane.md`
- `20-happy-happier-integration-blueprint.md`
- `21-code-agent-kernel-hardening.md`

Those documents establish:

- why remote control matters
- why `Happy` is the substrate
- why `Happier` informs advanced control contracts
- why the kernel must remain authoritative

This document freezes the operator-facing remote behavior so the remote plane is
testable instead of aspirational.

## 1. Why This Document Exists

The current design already says:

- remote/mobile/web is first-class
- remote control must be projection-driven
- attach/handoff/takeover must be explicit

But until the command contracts and fixtures are frozen, the remote plane is
still too easy to under-specify.

This document defines:

- remote commands
- JSON output expectations
- control actions
- capability/eligibility payloads
- golden fixtures for mobile/web/desktop remote behavior

The transport, pairing, reconnect, revocation, and offline-action rules beneath
these operator commands are frozen in
`docs/deep_study/25-remote-transport-auth-contract.md`.

All remote command JSON payloads must use the same `CommandSuccess` and
`CommandFailure` envelopes frozen in `23-cli-operator-contract.md`. The types
below populate the `data` field rather than replacing the common envelope.

## 2. Remote Product Rule

The remote plane is successful only if it behaves like a serious operator
surface:

- explainable
- typed
- auditable
- capability-gated
- non-destructive by default

That means remote commands cannot be vague wrappers around app behavior.

## 3. Frozen Remote Commands

## `research-cli remote pair`

Purpose:

- register or refresh this machine with the remote host substrate
- publish machine metadata and capability matrix
- ensure the machine is remotely discoverable

Text-mode success should include:

- machine id
- pairing state
- lease state
- remote host endpoint
- capability summary

JSON should return:

```json
{
  "ok": true,
  "command": "remote pair",
  "data": {
    "machine": {
      "machine_id": "rm_123",
      "pairing_state": "paired",
      "lease_state": "active",
      "capability_version": "v1"
    },
    "capability_matrix": {
      "resume_supported": true,
      "attach_supported": true,
      "handoff_supported": false,
      "terminal_supported": true
    },
    "remote_endpoint": "wss://remote.example/ws"
  }
}
```

## `research-cli remote status`

Purpose:

- show pairing state
- show active remote bindings
- show control owner
- show degraded remote features

JSON must expose:

```json
{
  "ok": true,
  "command": "remote status",
  "data": {
    "machine_id": "rm_123",
    "pairing_state": "paired",
    "machine_metadata": {
      "hostname": "devbox",
      "platform": "linux",
      "cli_version": "0.1.0"
    },
    "daemon_state": {
      "daemon_status": "running",
      "last_heartbeat_at": "2026-04-21T10:00:00Z"
    },
    "binding_ids": [],
    "control_owner": null,
    "feature_advertisements": [],
    "degraded_features": [],
    "revocation_reason": null,
    "revoked_at": null
  }
}
```

## `research-cli remote attach`

Purpose:

- attach to an existing session through an explicit attach strategy

This command must never mean "do whatever seems plausible".

It must return one of:

- provider attach available
- terminal-host attach available
- attach not eligible with reason

Frozen grammar:

```text
research-cli remote attach --session <session-id> [--strategy <provider_attach|terminal_host>] [--inspect] [--json]
research-cli remote attach --session <session-id> --strategy <provider_attach|terminal_host> --execute [--json]
```

Rules:

- `--inspect` is default when `--execute` is absent
- inspect mode returns `AttachEligibility` and must not mutate control
  ownership
- execute mode returns `AttachExecutionResult`
- stale epoch, workspace mismatch, or disabled feature returns exit `9`, `7`,
  or `8` with `CommandFailure.data = RemoteActionRejection`
- execute mode must publish the latest `runtime_descriptor_ref`,
  `projection_ref`, and `ownership_epoch`

## `research-cli remote handoff`

Purpose:

- hand control from one currently bound surface to another without rebuilding
  session runtime truth

This command must:

- expose explicit `HandoffEligibility` before execution
- preserve the current session descriptor and transcript cursor
- reject handoff when capability, lease epoch, or target-surface policy blocks it

Frozen grammar:

```text
research-cli remote handoff --session <session-id> --target-client <client-id> [--inspect] [--json]
research-cli remote handoff --session <session-id> --target-client <client-id> --execute [--json]
```

Rules:

- `--inspect` is default when `--execute` is absent
- inspect mode returns `HandoffEligibility`
- execute mode returns `HandoffExecutionResult`
- successful execute mode must preserve runtime descriptor and projection
  continuity while changing only control ownership
- local reclaim after handoff must increment ownership epoch and force remote
  mutating clients back to follow-only until revalidated

## `research-cli remote takeover`

Purpose:

- transfer control ownership of a live session

This command must:

- show the previous owner
- show the requested new owner
- emit `event_name=remote_takeover`
- reject illegal or stale ownership transfers

## `research-cli remote notify`

Purpose:

- emit structured notifications to remote clients

This command should support:

- permission pending
- task finished
- review needed
- cleanup approval needed
- wake event

It must not be string-only by default.

## 4. Frozen JSON Types

## `RemoteMachine`

```text
RemoteMachine {
  machine_id
  pairing_state
  lease_state?
  capability_version?
  machine_metadata_ref?
  daemon_state_ref?
}
```

## `RemotePairResult`

```text
RemotePairResult {
  machine
  capability_matrix?
  pair_ticket?
  remote_endpoint?
}
```

## `RemoteSessionBinding`

```text
RemoteSessionBinding {
  binding_id
  session_id
  project_id
  machine_id
  transport_mode
  runtime_descriptor_ref?
  projection_ref?
  control_owner?
  remote_status
  attach_modes[]
  remote_clients[]
  ownership_epoch
  attached_from_terminal
  attached_from_remote
}
```

## `AttachEligibility`

```text
AttachEligibility {
  session_id?
  eligible
  strategy?
  refusal_reason?
}
```

Allowed strategies:

- `provider_attach`
- `terminal_host`

## `HandoffEligibility`

```text
HandoffEligibility {
  session_id?
  binding_id?
  eligible
  target_client_id?
  refusal_reason?
}
```

## `RemoteControlOwner`

```text
RemoteControlOwner {
  session_id
  binding_id
  owner_client_id
  owner_surface_kind
  ownership_epoch
  control_mode
  claimed_at
  expires_at?
}
```

`RemoteControlOwner` is a projected operator view derived from the canonical
`ControlLease` in `25-remote-transport-auth-contract.md`.

## `RemoteTakeoverResult`

```text
RemoteTakeoverResult {
  session_id?
  binding_id?
  previous_owner?
  new_owner?
}
```

## `RemoteNotifyResult`

```text
RemoteNotifyResult {
  notification?
  receipts[]
}
```

`notification`, when present, serializes the canonical
`RemoteNotificationPayload` shape owned by
`25-remote-transport-auth-contract.md`.

## `RemoteFeatureStatus`

```text
RemoteFeatureStatus {
  feature_kind
  feature_id
  status
  phase
  error_code?
  error_message?
  actionable_hint?
}
```

## 5. Remote Actions Must Be Typed

The host may send only typed actions into the kernel.

## `RemoteAction`

```text
RemoteAction {
  action_id
  session_id?
  project_id?
  actor
  action_type
  send_user_message?
  interrupt_turn?
  respond_permission_request?
  request_takeover?
  request_handoff?
  request_attach?
  approve_cleanup_proposal?
  inspect_memory_explain?
  inspect_branch_batch?
  wake_supervised_run?
  invalidate_memory_record?
}
```

Exactly one typed payload field must be present, and it must match
`action_type`.

Allowed `action_type` values for milestone 1:

- `send_user_message`
- `interrupt_turn`
- `respond_permission_request`
- `request_takeover`
- `request_handoff`
- `request_attach`

Allowed milestone 2 additions:

- `approve_cleanup_proposal`
- `inspect_memory_explain`
- `inspect_branch_batch`
- `wake_supervised_run`
- `invalidate_memory_record`

## 6. Capability Gating Rules

Remote commands must never assume the backend can perform the action.

Required rules:

- attach is gated by `AttachEligibility`
- handoff is gated by `HandoffEligibility`
- terminal attach is gated by terminal capability and path policy
- remote steering is blocked if control ownership is unresolved
- destructive approvals are blocked if the runtime feature is degraded

## 7. Required Golden Fixture Families

These fixtures extend `17-operator-golden-fixtures.md`.

## Pairing fixtures

### `remote_pair_success`

Purpose:

- prove that a machine can pair successfully
- prove that capability publication is included in the result

Expected JSON subset:

```json
{
  "ok": true,
  "command": "remote pair",
  "data": {
    "machine": {
      "machine_id": "rm_fixture"
    },
    "capability_matrix": {
      "terminal_supported": true
    }
  }
}
```

### `remote_pair_degraded_host`

Purpose:

- prove that relay/host issues return structured degraded output

Expected behavior:

- command exits non-zero
- JSON exposes `degraded_features`

## Status fixtures

### `remote_status_unpaired`

Purpose:

- prove that remote status is explicit when no machine is paired

Expected JSON:

```json
{
  "ok": true,
  "command": "remote status",
  "data": {
    "pairing_state": "unpaired",
    "binding_ids": [],
    "feature_advertisements": []
  }
}
```

### `remote_status_active_binding`

Purpose:

- prove that active session bindings, control owner, and degraded features are
  visible

## Attach fixtures

### `remote_attach_provider_attach`

Purpose:

- prove that provider-native attach is surfaced explicitly

Expected JSON subset:

```json
{
  "ok": true,
  "command": "remote attach",
  "data": {
    "eligible": true,
    "strategy": "provider_attach"
  }
}
```

### `remote_attach_terminal_host`

Purpose:

- prove that terminal-host attach is surfaced explicitly

### `remote_attach_rejected_workspace_mismatch`

Purpose:

- prove that attach refuses to cross workspace/project boundaries silently

Expected JSON subset:

```json
{
  "ok": true,
  "command": "remote attach",
  "data": {
    "eligible": false,
    "refusal_reason": "workspace_mismatch"
  }
}
```

## Handoff fixtures

### `remote_handoff_success`

Purpose:

- prove handoff preserves runtime descriptor continuity and control ownership
  changes are explicit

### `remote_handoff_not_eligible`

Purpose:

- prove handoff exposes a typed refusal when the target surface or capability
  matrix disallows it

## Takeover fixtures

### `remote_takeover_success`

Purpose:

- prove ownership transfer is explicit and auditable

Expected JSON subset:

```json
{
  "ok": true,
  "command": "remote takeover",
  "data": {
    "session_id": "sess_123",
    "previous_owner": {
      "owner_surface_kind": "terminal"
    },
    "new_owner": {
      "owner_surface_kind": "mobile"
    }
  }
}
```

### `remote_takeover_conflict`

Purpose:

- prove takeover conflicts do not silently steal control

### `remote_takeover_stale_binding`

Purpose:

- prove stale or dead bindings fail structurally

## Notification fixtures

### `remote_notify_permission_pending`

Purpose:

- prove remote notify supports typed permission notifications

Expected JSON subset:

```json
{
  "ok": true,
  "command": "remote notify",
  "data": {
    "notification": {
      "kind": "permission_pending",
      "requires_ack": true
    }
  }
}
```

### `remote_notify_cleanup_review`

Purpose:

- prove ProjectOps-style notifications can be sent without bypassing kernel
  invariants

## Terminal fixtures

### `remote_terminal_policy_denied`

Purpose:

- prove remote terminal open/restart is blocked for unauthorized cwd/path policy

### `remote_terminal_reused_session`

Purpose:

- prove terminal session reuse is explicit rather than opening duplicate PTYs

## 8. Event Assertions

The canonical event-name registry is frozen in
`24-kernel-state-machine-contract.md`.

Every remote fixture should therefore assert these `event_name` stems where
relevant:

- `remote_pair`
- `remote_status`
- `remote_attach`
- `remote_handoff`
- `remote_takeover`
- `remote_control_owner`
- `remote_notify`
- `remote_terminal`

## 9. Conformance Layout

Recommended layout:

```text
tests/golden/remote/
├── pair/
├── status/
├── attach/
├── takeover/
├── notify/
└── terminal/
```

This should sit beside the existing config/provider/session/permission fixtures,
not outside the parity harness.

## 10. Implementation Consequence

The remote plane is now specific enough that implementation should not improvise
its command semantics.

The implementation order should be:

1. `remote pair`
2. `remote status`
3. `remote attach`
4. `remote handoff`
5. `remote takeover`
6. `remote notify`
7. remote golden fixtures

Only after that should we add richer research-native remote controls.

That is how the remote plane stays clearly above generic remote coding shells
instead of becoming a loosely specified side feature.
