# Remote Transport And Auth Contract

This document hardens the remaining remote gaps after:

- `19-remote-host-control-plane.md`
- `20-happy-happier-integration-blueprint.md`
- `22-remote-command-and-fixture-contract.md`

The previous design already fixed the layering question:

- `Happy` is the substrate
- `Happier` is the feature-evolution reference
- `research-cli` remains the runtime authority

The missing operator-grade details were:

- machine identity material
- pairing and revocation
- reconnect and replay safety
- offline action policy
- deterministic mock host harness

This document freezes those details.

## 1. Remote Authority Rule

The remote plane is a control and projection surface.

It is never allowed to become a second runtime.

That means:

- pairing authenticates a device, not a second kernel
- remote actions are requests, not direct state mutations
- only the kernel may assign session ownership, permissions, or canonical
  artifact promotion

## 1.2 Local Phone App Completion Boundary

The first implemented phone app is an installable browser PWA served by the
machine-local daemon:

```text
phone browser PWA
  -> HTTP JSON API
research-cli remote daemon
  -> Rust remote substrate
  -> machine-global remote state + project-local .pmcli/remote + events
```

This app may store browser preferences such as `cwd`, `client_id`, and the last
pair ticket typed by the operator. It may not store session ownership, lease
truth, project state, event state, artifact state, or projection truth. Those
facts remain in the kernel-owned remote substrate and canonical event log.

The local daemon/PWA completion boundary is:

- served by `research-cli remote daemon`
- usable through a private overlay network such as Tailscale; direct LAN access
  is a convenience path only when the phone can actually reach the host
- protected by an application-layer daemon control token through
  `--control-token <token>` or `RESEARCH_CLI_REMOTE_DAEMON_TOKEN`; every
  `/api/*` route except `/api/health` must reject requests when the daemon has
  no configured token, an empty token, a missing `Authorization` header, or a
  wrong token
- routes status, workbench, session listing, pair, attach, handoff, takeover,
  notify, message steering, permission response, revocation, and lease checks
  through the Rust remote substrate
- exposes the shared `HostSurfaceProjection` and `HostSurfaceAction` contract
  through daemon APIs so mobile/web clients do not invent their own view model
- exposes governed terminal attach/replay projection APIs and a PTY/xterm byte
  lane for terminal parity; PTY output must be appended to canonical events and
  checkpointed before any websocket frame is emitted
- writes mutating control results as canonical project events
- exposes canonical project events through `/api/events?cwd=...&after_seq=N`
  as an event-stream projection over `.pmcli/events/events.jsonl`
- treats the mobile UI as a projection/control surface only

### 1.2.1 Tailscale private-overlay operating mode

Tailscale is the graduated M5 transport boundary. Public Happy-style relay and
native app packaging are intentionally out of scope for this product line unless
the project later reopens them as a separate transport initiative. Tailscale
keeps the daemon private while avoiding fragile campus NAT or SSH-only
forwarding paths.

```text
phone browser
  -> Tailscale 100.x address / MagicDNS name
  -> research-cli remote daemon on the workstation
```

Operating rules:

- the workstation and phone must join the same tailnet
- `research-cli remote daemon` must listen on `0.0.0.0:<port>` or be published
  with `tailscale serve`; non-loopback binds are rejected unless a non-empty
  daemon control token is supplied
- the phone must open the workstation's Tailscale address, not the unreachable
  physical `10.x` address
- Tailscale does not create runtime authority; it is only a private transport
  path to the same daemon/API
- a daemon control token is mandatory for daemon API use, including private
  overlays, because Tailscale solves network reachability and device admission,
  not per-daemon control intent
- research-cli does not provide or require a self-hosted public relay; any
  public exposure would be a separate future transport initiative and is not
  part of M14 product completion

Implementation preference:

1. System install, when sudo/systemd is available:
   `tailscale up`, then open `http://<tailscale-ip>:8787/`.
2. Rootless fallback, when sudo is unavailable:
   run `tailscaled --tun=userspace-networking` from a local binary and publish
   the daemon with `tailscale serve` to the tailnet.

The following are non-goals for the M5 completion claim:

- encrypted Happy relay
- native iOS/Android packaging
- background push notification delivery
- full mobile parity with every externally visible terminal CLI command

Native packaging is a product-surface deferral, not a transport limitation.
A later native app may use the same Tailscale private overlay and the same
daemon HTTP API:

```text
iOS / Android native app
  -> phone system Tailscale VPN
  -> research-cli remote daemon HTTP API
  -> Rust remote substrate
```

That later app must remain a thin projection/control client. It may replace the
PWA UI, but it may not introduce a second runtime, second project store, or
phone-owned session truth.

Full mobile CLI parity is a separate future product milestone. It must wait
until the terminal CLI is complete and frozen enough to be the canonical public
surface. When that milestone starts, mobile should expose every command from the
canonical help registry through a generated command palette plus touch-native
forms and panels. It must call back into the same Rust runtime through daemon
APIs rather than duplicating command logic in the app.

### 1.2.2 Daemon API and event projection

The daemon HTTP API is a narrow projection/control API:

```text
GET  /api/health               unauthenticated liveness
GET  /api/status               projected remote status
GET  /api/host-surface         shared local TUI/web/mobile host projection
GET  /api/tui/actions          shared typed action registry for UI clients
GET  /api/workbench            projected workbench surface
GET  /api/sessions             projected session list
GET  /api/artifacts            governed artifact family index
GET  /api/artifact/inspect     governed artifact/code/report preview
GET  /api/results              governed test/result panel projection
GET  /api/events?after_seq=N   canonical event stream projection
POST /api/pair                 pair browser/mobile client
POST /api/attach               request control/follow binding
POST /api/handoff              request owner handoff
POST /api/takeover             request owner takeover
POST /api/notify               append remote notification event
POST /api/message              steer message into a session
POST /api/session/attachments  persist governed conversation image inputs
POST /api/permission           answer a pending permission request
POST /api/revoke               revoke a remote client
POST /api/terminal/attach      attach governed terminal projection lease
GET  /api/terminal/replay      replay governed terminal projection cursor
```

Clients must send:

```text
Authorization: Bearer <token>
```

Browsers cannot attach custom headers to native `EventSource`, so the shipped
PWA polls `/api/events` with `fetch` and the same authorization header. That is
intentional: the stream is still derived from canonical kernel events, and the
phone never receives write authority over the event log.

The following are part of the Tailscale-only M5 contract and must be covered by
the Rust daemon, schemas, and conformance tests:

- reconnect/replay metadata exposure
- revocation and lease-expiry enforcement
- workbench projection
- message steering
- permission response
- daemon control-token rejection and acceptance
- canonical event-stream projection after a cursor

## 1.1 Concrete Happy/Happier Reuse Map

To keep the blind packet self-sufficient, the remote adaptation boundary is
frozen here rather than only in the companion studies.

### `Happy` reuse

`Happy` remains the design reference for:

- separating machine, session, and user scoped update channels
- keeping durable updates, ephemeral presence, and explicit RPC actions separate
- exposing remote actions as typed requests with acknowledgement
- rendering the phone UI from session/control projections rather than letting
  the app become a second runtime

### `Happier` contract import

`Happier` patterns are imported for:

- daemon ownership and takeover discipline
- local-control capability metadata
- existing-session automation strategy resolution
- attach/handoff inspect-vs-execute discipline
- terminal lifecycle and timeout expectations

### `research-cli` adapter ownership

`research-cli` owns the adapter-only layer:

- `ProviderSessionSource`
- `SessionRuntimeDescriptor`
- `TranscriptSourceEnvelope`
- `RemoteSessionBinding`
- `SessionEnvelopeProjectionV1`
- typed action normalization and policy rejection

### Kernel-only ownership

The kernel alone owns:

- canonical session state
- permissions
- project memory and ProjectOps
- artifacts, cleanup, branch/review/research transitions
- event stream and checkpoint truth

Implementation rule:

- if a remote behavior cannot be mapped to one of the four rows above, it may
  not create a new authority surface implicitly

## 2. Canonical Remote Identities

## `MachineIdentity`

```text
MachineIdentity {
  machine_id
  machine_label
  device_class
  public_key_fingerprint
  host_instance_id
  runtime_version
  created_at
  rotated_at?
  revoked_at?
}
```

## `RemoteClientIdentity`

```text
RemoteClientIdentity {
  client_id
  machine_id
  surface_kind
  user_label
  auth_subject
  registered_at
  last_seen_at
}
```

Rules:

- `machine_id` is stable across reconnects until explicit rotate or revoke
- `client_id` identifies one remote browser/mobile surface, not the host machine
- identity rotation must preserve audit linkage to the prior identity

## 3. Pairing Contract

Pairing is a lease-granting flow, not a one-time boolean.

## `PairTicket`

```text
PairTicket {
  pair_ticket_id
  machine_id
  challenge_nonce
  issued_at
  expires_at
  capabilities
  relay_endpoint
}
```

## `RemoteLease`

```text
RemoteLease {
  lease_id
  machine_id
  issued_at
  expires_at
  revocation_epoch
  capability_hash
}
```

### Pair flow

1. `remote pair` asks host for a challenge
2. local runtime signs challenge with machine identity
3. host returns `RemoteLease`
4. kernel stores the lease under machine-global remote state
5. feature status is updated to `ready` or `degraded`

### Pairing rules

- `remote pair --ticket-expires-at <epoch-ms>` models pair-ticket expiry;
  expired pair tickets must fail with exit `12`,
  `reason_code=remote_action_rejected`, and
  `RemoteActionRejection.rejection_code=remote_pair_ticket_expired`
- daemon `/api/pair` may only consume a pre-issued, unconsumed, unexpired pair
  ticket. The browser/mobile caller cannot invent a ticket id and gain a lease.
  The local CLI may still issue-and-consume a ticket in one operator command,
  but daemon pairing is a two-step flow: local ticket issue, then remote pair.
- `remote pair --lease-expires-at <epoch-ms>` models the control lease returned
  by a successful pair operation; an already expired lease may be persisted so
  `remote status` can degrade to `lease_expired`
- replayed challenge nonces must fail with exit `12`,
  `reason_code=remote_action_rejected`, and
  `RemoteActionRejection.rejection_code=remote_pair_ticket_replayed`
- rotated or revoked machine identities invalidate prior leases immediately

Canonical machine-global location:

- `$XDG_STATE_HOME/research-cli/remote/`
- fallback: `~/.local/state/research-cli/remote/`

Machine-global remote objects:

- `machine_identity.json`
- `lease.json`
- `capabilities.json`
- `machine_metadata.json`
- `daemon_state.json`

## 4. Capability Publication Contract

Capabilities are frozen into the lease.

## `RemoteCapabilitySet`

```text
RemoteCapabilitySet {
  follow_session
  attach_provider
  attach_terminal
  approve_permission
  request_handoff
  request_takeover
  notify
  inspect_projectops
  inspect_branches
  inspect_memory
}
```

Rules:

- capabilities are published at pair time and refreshed only through lease
  renewal
- a client may not request an action absent from its capability set
- degraded capabilities must remain visible in `remote status`
- pairing/auth/capability authority is machine-global, not project-local

## 4.6 Machine Metadata And Daemon State Contract

Remote stability depends on machine-global daemon ownership being explicit.

## `RemoteMachineMetadata`

```text
RemoteMachineMetadata {
  machine_id
  hostname
  platform
  cli_version
  home_dir
  state_root
  host_variant?
  updated_at
}
```

## `RemoteDaemonState`

```text
RemoteDaemonState {
  machine_id
  daemon_status
  daemon_pid?
  control_port?
  started_at?
  last_heartbeat_at?
  started_with_cli_version?
  tracked_session_ids[]
  state_reason?
}
```

Rules:

- both objects are machine-global remote facts under XDG remote state
- daemon state must survive graceful shutdown as last-known status rather than
  disappearing entirely
- daemon ownership conflicts must resolve through explicit version and lock
  policy, not silent overwrite
- tracked session IDs are operator diagnostics and cleanup aids, not canonical
  session truth
- `remote status` should expose both capability state and daemon-state health

## 4.5 Runtime Affinity Contract

Remote attach, takeover, handoff, and resume must not reconstruct runtime
semantics from transport-specific branches.

## `SessionRuntimeDescriptor`

```text
SessionRuntimeDescriptor {
  session_id
  runtime_family
  provider_session_source_ref?
  source_kind
  source_affinity?
  transport_mode
  transcript_mode
  local_control_capability?
  existing_session_automation?
  attach_modes[]
  handoff_modes[]
  takeover_modes[]
  supports_inflight_steer
  supports_latest_turn_rollback
  supports_local_remote_switch
  session_envelope_projection_line
}
```

Rules:

- every live session publishes one canonical runtime descriptor
- attach, browse, takeover, handoff, and resume resolve through the same
  descriptor
- provider-backed session source and affinity must be explicit rather than
  inferred from transport branches
- local-control behavior and existing-session automation strategy must be
  resolved through typed metadata rather than app heuristics
- backend-specific transport differences must stay behind the descriptor
  boundary
- missing capabilities must degrade explicitly rather than be inferred from UI
  mode

## `ProviderSessionSource`

```text
ProviderSessionSource {
  source_id
  session_id
  provider_id
  source_family
  source_locator?
  source_affinity
  vendor_resume_supported
  happy_attach_supported
  recorded_at
}
```

Allowed `source_family` values should include:

- `native_provider`
- `happy_remote`
- `hybrid_remote`
- `imported_transcript`

Allowed `source_affinity` values should include:

- `provider_resumable`
- `host_attach_only`
- `projection_only`

## `LocalControlCapability`

```text
LocalControlCapability {
  supported
  topology
  attach_strategy
}
```

Allowed `topology` values:

- `exclusive`
- `shared`

Allowed `attach_strategy` values:

- `provider_attach`
- `terminal_host`
- `unsupported`

## `ExistingSessionAutomationEligibility`

```text
ExistingSessionAutomationEligibility {
  eligible
  agent_id?
  strategy?
  reason_code?
}
```

Allowed `strategy` values:

- `vendor_resume`
- `happy_attach`

## `SessionEnvelopeProjectionV1`

```text
SessionEnvelopeProjectionV1 {
  projection_id
  schema_version
  session_id
  project_id
  runtime_descriptor_ref
  transcript_source_ref?
  title
  latest_turn_summary?
  active_phase
  permission_pending_count
  control_owner?
  feature_advertisements[]
  memory_status_summary
  branch_status_summary
  review_status_summary
  seq_low_watermark
  seq_high_watermark
  updated_at
}
```

Rules:

- this is the only field-frozen session projection shape exported to remote
  clients in first release
- `control_owner`, when present, serializes the canonical
  `RemoteControlOwner` shape rather than a string alias
- `transcript_source_ref`, when present, must resolve to the same
  `TranscriptSourceEnvelope` used by inspect/export surfaces
- projection snapshots may be cached remotely, but runtime truth stays in
  kernel state plus durable logs
- attach, handoff, takeover, and resume must all reference the same runtime
  descriptor and latest projection line
- `SessionEnvelopeProjectionV1` is additive-only within the published line
- projections, cursors, and bindings are project-local remote objects under
  `.pmcli/remote/`

## `RemoteSessionBinding`

```text
RemoteSessionBinding {
  binding_id
  session_id
  project_id
  machine_id
  transport_mode
  runtime_descriptor_ref?
  provider_session_source_ref?
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

Rules:

- `control_owner`, when present, serializes the canonical
  `RemoteControlOwner` projection
- `projection_ref` and `runtime_descriptor_ref` must point at the same runtime
  truth used by attach, handoff, takeover, and resume
- `provider_session_source_ref`, when present, must match the source bound into
  the runtime descriptor for the same session
- `ownership_epoch` changes only on attach execution, handoff, takeover, or
  local reclaim
- `remote_status` is explicit; bindings may not silently drift between
  `follow_only`, `control`, and `stale`

## `TranscriptSourceEnvelope`

```text
TranscriptSourceEnvelope {
  envelope_id
  session_id
  transcript_family
  transcript_locator
  projection_line
  low_watermark_seq?
  high_watermark_seq?
  consistency_state
  updated_at
}
```

Allowed `transcript_family` values should include:

- `canonical_project_transcript`
- `remote_projection_stream`
- `compacted_recap`
- `vendor_mirror`

Rules:

- remote projections may cache transcript material, but transcript provenance
  must remain explicit through one envelope
- attach, browse, export, and reconnect must all point at the same latest
  transcript-source envelope for the chosen session
- consistency drift between transcript envelope and canonical session state must
  degrade the surface and trigger regeneration instead of silent arbitration

## 5. Reconnect And Cursor Contract

Remote follow and control need deterministic replay and reconnect.

## `RemoteCursor`

```text
RemoteCursor {
  session_id
  binding_id
  last_event_seq
  ownership_epoch
  replay_token
  seen_at
}
```

Reconnect rules:

- a reconnect request must present the latest `RemoteCursor`
- the host may replay only events with `seq > last_event_seq`
- stale `ownership_epoch` forces revalidation before control resumes
- missing replay token degrades to follow-only mode until operator approval

## 6. Ownership And Takeover Contract

## `ControlLease`

```text
ControlLease {
  session_id
  binding_id
  owner_client_id
  owner_surface_kind
  ownership_epoch
  granted_at
  expires_at
}
```

Rules:

- exactly one `ControlLease` may be active per foreground session
- follow-only clients do not hold a control lease
- `remote takeover` increments `ownership_epoch`
- any action carrying an old epoch fails with exit `9`

## 6.5 Handoff Contract

Handoff is explicit transfer between already-known surfaces. It is not a
re-pair, a silent reconnect, or a second attach.

## `HandoffExecutionResult`

```text
HandoffExecutionResult {
  session_id
  binding_id
  source_client_id
  target_client_id
  previous_owner
  new_owner
  ownership_epoch
  runtime_descriptor_ref
  projection_ref
}
```

Rules:

- handoff requires a live binding plus current ownership epoch
- handoff may target only surfaces allowed by `handoff_modes[]`
- successful handoff preserves session identity, runtime descriptor, and
  projection continuity
- `previous_owner` and `new_owner` must serialize the canonical
  `RemoteControlOwner` shape rather than a string alias
- handoff failure must preserve the previous control owner rather than degrade
  into ambiguous ownership

## 7. Revocation Contract

Revocation is first-class, not an implied disconnect.

Revocation lanes:

- machine identity revoked
- specific client revoked
- lease revoked by operator
- relay trust revoked

Required behavior:

- revoked identities lose control immediately
- pending offline actions from revoked identities are dropped, not replayed
- revocation emits `remote_lease_revoked` and `remote_binding_revoked`
- `remote status` must expose revocation reason and time

These labels are not remote-local aliases.

They must serialize using the canonical `event_name` registry frozen in
`24-kernel-state-machine-contract.md`.

## 8. Offline Action Policy

Remote/mobile/web must tolerate offline gaps without hidden mutation.

## `OfflineActionPolicy`

```text
OfflineActionPolicy {
  follow_replay_allowed
  notify_queue_allowed
  mutating_action_queue_allowed
  max_mutating_queue_depth
  permission_reply_ttl_seconds
}
```

Frozen policy for first release:

- follow replay: allowed
- typed notifications: queueable
- permission replies: queueable only within TTL and only if request still live
- mutating actions such as `send_user_message`, `request_takeover`, or cleanup
  approval: not queueable across disconnect by default

This avoids silent stale actions.

## 8.5 Local And Remote Switching Policy

Happier demonstrates that attach is not enough; the product needs a clean
local↔remote switching contract.

## `LocalRemoteSwitchPolicy`

```text
LocalRemoteSwitchPolicy {
  switch_supported
  local_attach_mode
  remote_follow_allowed_during_local
  pending_queue_policy
  permission_mode_persistence
  handback_trigger
}
```

Allowed `local_attach_mode` values:

- `shared`
- `exclusive`

Required rules:

- permission mode is stored canonically in session metadata and survives local
  to remote switching
- local reclaim may occur only when `LocalControlCapability.supported=true`
  and the requested attach path matches `attach_strategy`
- mutating queued actions must be drained, rejected, or explicitly rebound
  before exclusive local control is granted
- a local reclaim event must increment control epoch before mutating control
  resumes
- follow-only clients may remain connected during local control unless policy
  disables them
- executing `remote attach` may restart the Happy wrapper into remote mode, but
  may not change session identity, runtime descriptor, or projection lineage
- the canonical first-release `handback_trigger` is local keypress; that local
  reclaim must publish a new `RemoteControlOwner`, increment `ownership_epoch`,
  and degrade remote mutating clients to follow-only until they re-request
  control

Existing-session automation rule:

- inspect surfaces must expose whether the session is eligible for
  `vendor_resume` or requires `happy_attach`
- execute surfaces may not silently switch between those strategies after
  inspection unless the runtime/source affinity has been revalidated and
  reported

## 8.6 Remote Feature Advertisement

The remote host must advertise available feature families so mobile/web clients
adapt without guessing.

## `RemoteFeatureAdvertisement`

```text
RemoteFeatureAdvertisement {
  feature_family
  enabled
  policy_mode
  reason?
}
```

Minimum advertised families:

- `remote_control`
- `session_attach`
- `session_handoff`
- `embedded_terminal`
- `notifications`
- `diagnostics`
- `attachments`

Rules:

- server and machine policy may both narrow the effective feature set
- disabled feature families must still appear in `remote status`
- UI behavior should derive from advertisements, not hard-coded app assumptions

## 8.7 Cached Remote Read-Model Contract

Remote/mobile/web need fast browse surfaces, but cached indexes may not turn
into a second runtime.

Allowed cached read models include:

- gateway session indexes
- channel directories
- browse-ready projection summaries

Required rules:

- every cached read model must declare `authoritative=false`
- every cached read model must be rebuildable from leases, bindings,
  projections, and project-local runtime data
- mutating commands must re-resolve against canonical bindings, descriptors,
  and ownership epochs before acting
- stale read models may degrade browse quality, but may not cause silent
  mutation on the wrong session or machine

## 9. Notification Delivery Contract

## `NotificationReceipt`

```text
NotificationReceipt {
  notification_id
  client_id
  delivery_state
  queued_at
  delivered_at?
  acknowledged_at?
}
```

Allowed `delivery_state` values:

- `queued`
- `delivered`
- `acknowledged`
- `expired`
- `dropped`

`remote notify` returns a canonical notification payload plus delivery
receipts, not a fire-and-forget string.

## `RemoteNotificationPayload`

```text
RemoteNotificationPayload {
  notification_id
  kind
  project_id?
  session_id?
  summary
  detail_refs[]
  urgency?
  requires_ack
}
```

Allowed `kind` values for first release:

- `permission_pending`
- `task_finished`
- `review_needed`
- `cleanup_approval_needed`
- `wake_event`

## `RemoteStatusReport`

```text
RemoteStatusReport {
  machine_id?
  pairing_state?
  machine_metadata?
  daemon_state?
  binding_ids[]
  control_owner?
  revocation_reason?
  revoked_at?
  feature_advertisements[]
  degraded_features[]
}
```

Rules:

- `control_owner`, when present, serializes the canonical
  `RemoteControlOwner` shape
- remote status may not hide degraded families or revoked control state behind
  app-local heuristics
- a local reclaim must be visible in `RemoteStatusReport` immediately after the
  new ownership epoch is published

## 10. Remote Action Validation Pipeline

Every remote action must pass:

1. client identity validation
2. lease validation
3. capability validation
4. ownership or follow-mode validation
5. session/project scope validation
6. permission and feature-health validation
7. kernel admission

Failure at any stage returns a typed error and emits `remote_action_rejected`.

`remote_action_rejected` is a canonical kernel event, not an app-only audit
string.

## `RemoteActionActor`

```text
RemoteActionActor {
  client_id
  machine_id?
  surface_kind?
  ownership_epoch?
}
```

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
  request_attach?
  request_handoff?
  request_takeover?
  approve_cleanup_proposal?
  inspect_memory_explain?
  inspect_branch_batch?
  wake_supervised_run?
  invalidate_memory_record?
}
```

Exactly one typed action payload must be present, and it must match
`action_type`.

## Typed remote action payloads

```text
SendUserMessageAction {
  message_text
}

InterruptTurnAction {
  turn_id?
}

RespondPermissionRequestAction {
  request_id
  decision
}

RequestAttachAction {
  strategy
}

RequestHandoffAction {
  binding_id
  target_client_id
}

RequestTakeoverAction {
  binding_id
  reason?
}

ApproveCleanupProposalAction {
  proposal_id
  decision
}

InspectMemoryExplainAction {
  record_id
}

InspectBranchBatchAction {
  batch_id
}

WakeSupervisedRunAction {
  wake_id
}

InvalidateMemoryRecordAction {
  record_id
  reason?
}
```

## `RemoteActionRejection`

```text
RemoteActionRejection {
  session_id
  binding_id?
  requested_action
  rejection_stage
  rejection_code
  retryable
  ownership_epoch?
  capability_ref?
  next_steps[]
}
```

## 11. Happy Reuse Boundary

The plan should reuse `Happy` for:

- client pairing substrate
- relay/session transport
- mobile/web surface scaffolding
- notification delivery substrate

The plan should keep inside `research-cli`:

- session authority
- control ownership
- permission requests
- event truth
- branch, memory, review, and artifact policies

This is the concrete form of "build on Happy rather than replacing the kernel."

## 12. Deterministic Mock Host Harness

Remote safety claims must be testable without a real mobile device farm.

The repository must therefore ship:

```text
tests/remote_harness/
├── mock_happy_host/
├── mock_clients/
├── recorded_traces/
└── run_remote_conformance.sh
```

The harness must simulate:

- pair success
- pair replay attack
- lease expiry
- reconnect after event gap
- takeover with stale epoch
- revocation during pending notification
- offline permission response after TTL expiry
- local reclaim while remote follow remains attached
- feature family disabled by server policy

## 13. Required Remote Fixtures

These extend `22`.

- `remote_pair_nonce_replay_rejected`
- `remote_reconnect_follow_only_after_missing_replay_token`
- `remote_takeover_epoch_conflict`
- `remote_revocation_drops_mutating_queue`
- `remote_notify_receipt_expired`
- `remote_permission_reply_after_ttl_rejected`
- `remote_local_reclaim_increments_control_epoch`
- `remote_feature_advertisement_disables_handoff`

## 14. Why This Now Reaches Operator Grade

The remote plane is no longer specified only as commands and typed actions.

It now freezes:

- who a machine and client are
- how they pair
- how they reconnect
- how they lose authority
- what can and cannot queue offline
- how runtime affinity and local↔remote switching stay coherent

That is the level needed before claiming serious mobile/web remote control.
