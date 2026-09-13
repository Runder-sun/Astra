# Remote Host And Multi-Device Control Plane

This document hardens a newly promoted architecture decision:

- the final product target is a research-cli-specific remote workbench
- the implementation substrate should be a fork/adaptation of `Happy`
- the product-growth reference should be `Happier`

In short:

- target architecture = `C`
- implementation route = fork/adapt a `Happy`-based remote host
- feature import route = selectively port `Happier` contracts and modules

## 1. Why This Document Exists

The user requirement is explicit:

- the system must support remote control from mobile and web
- the best implementation path is likely to build on `Happy`

That requirement is too important to remain a footnote under
`HostAdapterAPI`.

Remote/mobile/web control is not an optional shell. It is a first-class
product layer for the final system.

## 2. Reference-Derived Conclusion

### `Happy`

`Happy` is the strongest direct substrate for the first remote-host layer.

What it already proves:

- mobile + web client for Claude Code and Codex
- remote takeover and device switching
- end-to-end encrypted relay model
- wrapper CLI approach instead of reimplementing the agent runtime
- session protocol specialized for rendering encrypted coding sessions
- machine-scoped and session-scoped websocket connectivity

This is exactly the class of infrastructure we should avoid rebuilding from
zero.

### `Happy Server`

`Happy Server` proves that the relay plane can stay:

- zero-knowledge
- websocket-driven
- user/session/machine aware
- self-hostable
- simple enough to audit

This fits the desired deployment shape for research-cli remote control.

### `Happier`

`Happier` shows where the remote host naturally wants to evolve:

- broader provider support
- attach / handoff / browse / takeover
- embedded terminal
- project surfaces and worktrees
- pending queues and steering
- multi-server support
- session continuity across terminal, mobile, web, and desktop

We should not blindly adopt all of it, but we should absolutely use it as the
design reference for what a mature remote host becomes.

## 3. Final Decision

The system should not choose between:

- "pure research-cli with no strong remote host"
- "generic remote shell with no research specialization"

It should be built as:

1. `Happy`-derived remote host substrate
2. `research-cli` kernel and project intelligence core
3. `research-cli`-specific remote workbench layered on top

This is the shortest path to the user's requested end state.

Concretely, this means:

- start from the `Happy` codebase for relay, pairing, multi-device connectivity,
  and session transport
- selectively import or mirror `Happier` features for advanced attach, handoff,
  terminal policy, and daemon ownership
- do not begin by designing a brand-new remote substrate inside
  `research-cli`

## 4. Target Architecture `C`

The target is still the fully integrated remote-first architecture:

- research-cli remains the authoritative runtime
- project memory and ProjectOps remain native to research-cli
- multi-agent research orchestration remains native to research-cli
- mobile/web/desktop clients become first-class control surfaces

What changes is the implementation route:

- we do not build the remote substrate from scratch
- we adapt and extend `Happy`
- we selectively import `Happier`-grade contracts where `Happy` alone is not
  enough

## 5. Layering

### Layer 1: `HappyHost`

Responsibilities:

- remote device pairing
- session attach and takeover
- encrypted relay transport
- websocket fanout
- notification delivery
- mobile/web/desktop session rendering

This is the substrate.

It should be implemented by forking or vendoring `Happy` host code and then
modifying it, not by greenfield reimplementation.

### Layer 2: `ResearchRuntime`

Responsibilities:

- code-agent kernel
- provider/model/config/session semantics
- permissions and sandboxing
- structured runtime events
- operator-grade parity behavior

This remains the authority for actual agent work.

### Layer 3: `Project Intelligence`

Responsibilities:

- project memory
- silent digest candidates
- artifact governance
- repo cleanup proposals
- explain/query surfaces

### Layer 4: `Research Orchestration`

Responsibilities:

- multi-agent debate
- branch search
- experiment supervision
- result-to-claim repair routing
- research skill contracts

### Layer 5: `Remote Research Workbench`

This is the research-cli-specific extension over the Happy host.

Responsibilities:

- remote project status
- memory explain panel
- branch / debate inspector
- artifact family browser
- wake queue and supervisor lease view
- repo cleanup proposal review
- remote approval and intervention surface

This is the part that makes the final product truly ours rather than a
rebranded remote shell.

## 6. Architectural Boundary

The key rule is:

- `HappyHost` owns remote transport and remote UX continuity
- `research-cli` owns runtime truth

Remote clients must not become a second hidden runtime.

That means:

- no alternate state machine in the app
- no hidden promotion of memory or artifacts in the relay layer
- no remote-only research semantics that bypass kernel invariants

The remote host renders and controls. The runtime decides.

## 7. Host Control Contract

The remote plane should support at least:

- pair machine
- show connected machines
- browse projects
- browse sessions
- follow a live session
- take over from another device
- attach back from terminal
- approve or deny pending permission requests
- send queued or immediate user messages
- inspect active agents and branches
- inspect project health and wake queue

This should be considered part of the first-class product surface, not an
afterthought.

## 8. Remote Session Model

The session plane needs explicit remote-aware identities.

The field-frozen shapes live in:

- `25-remote-transport-auth-contract.md`
- `37-kernel-type-and-interface-manifest.md`

The objects below should stay aligned with those documents.

### `RemoteMachine`

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

### `RemoteMachineMetadata`

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

### `RemoteDaemonState`

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

### `RemoteSessionBinding`

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

### `RemoteStatusReport`

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

### `RemoteTakeoverEvent`

```text
RemoteTakeoverEvent {
  event_id
  session_id
  from_surface
  to_surface
  reason
  timestamp
}
```

The final product should make these transitions explicit and auditable.

The remote product should also keep machine metadata and daemon state visible as
machine-scoped operator facts rather than ephemeral UI guesses.

## 9. Protocol Strategy

The best near-term move is compatibility-first:

1. map research-cli runtime events into a Happy-compatible session rendering stream
2. preserve research-cli-native structured events under `.pmcli/`
3. add research-specific panels on top of the remote host

This avoids prematurely rewriting the remote wire protocol or prematurely
building a new host stack before we have exhausted the direct value from
`Happy`.

If later needed, we can add a richer side-channel for:

- debate state
- artifact families
- ProjectOps
- branch search telemetry
- research supervisor wake events

But the first move should be adapter-first, not protocol-rewrite-first.

## 10. Security Model

The remote plane should inherit the strongest Happy properties:

- end-to-end encryption by default
- self-hostable relay
- user/session/machine-scoped connectivity
- machine registration and revocation
- push notifications without plaintext transcript exposure

For research-cli specifically, remote control must also respect:

- permission modes
- project boundaries
- branch workspace isolation
- review blinding
- cleanup/destructive human gates

Remote convenience must never weaken runtime safety.

## 11. Why `Happy` Over Building From Scratch

Because building remote control from scratch would require us to independently
rebuild:

- device pairing
- websocket relay
- machine/session routing
- mobile/web clients
- takeover/attach semantics
- push notifications
- self-hosting path
- encrypted transport story

Those are real products, not glue code.

The opportunity cost is too high while the core research-cli runtime is still
being strengthened.

## 12. Why `Happy` Over Directly Forking `Happier`

`Happier` is a valuable reference and may even become a future upstream source
for specific features, but `Happy` is the cleaner foundational answer for now:

- smaller conceptual surface
- simpler direct mapping to Claude/Codex remote hosting
- easier to reason about as a substrate
- less immediate architectural overhang

Use `Happy` as the base host.

Use `Happier` as the roadmap reference for:

- session handoff
- attach
- pending queue
- embedded terminal
- projects/worktrees
- multi-server support
- broader provider abstraction

## 13. Integration Plan

### Phase 1: Happy-Compatible Host Layer

- fork/adapt `Happy` host and wrap research-cli runtime with a compatible remote
  host adapter
- expose session browsing, follow, takeover, and terminal reattach
- keep the remote app mostly generic at first

### Phase 2: Research Workbench Panels

- add agents/branches/reviews/memory/artifacts/project-health panels
- add remote ProjectOps surfaces
- add wake queue and queued-run supervision visibility

### Phase 3: Research-Native Remote Control

- remote branch adjudication
- remote cleanup-plan approval
- remote memory explain and invalidation
- remote research intervention controls

## 14. Changes To The Earlier Architecture

The earlier architecture treated remote execution as a runtime option and
`HostAdapterAPI` as a broad shell boundary.

That is no longer strong enough.

The architecture should now explicitly say:

- remote/mobile/web/desktop control is a first-class host plane
- the host plane should be implemented on top of `Happy`
- the final product should add a research-native remote workbench on top of it

## 15. Success Criteria

This remote strategy is successful only if all are true:

1. a running research-cli session can be followed and controlled from mobile/web
2. the remote layer does not fork runtime truth away from `.pmcli/`
3. project memory, ProjectOps, and research state are visible remotely
4. permission, cleanup, and review gates remain enforceable remotely
5. the system clearly exceeds generic remote coding shells by exposing the
   research-native control surfaces

That is the real meaning of target architecture `C`.

The code-level integration route for this decision is specified in
`docs/deep_study/20-happy-happier-integration-blueprint.md`.
