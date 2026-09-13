# Happy/Happier Code-Level Integration Blueprint

This document is the code-level follow-up to
`19-remote-host-control-plane.md`.

`19` made the product decision:

- target architecture = integrated remote-first research workbench
- implementation substrate = fork/adapt `Happy`
- feature-evolution reference = `Happier`

This document makes that decision executable.

It answers the questions that high-level architecture alone does not answer:

- what parts of `Happy` should be reused directly
- what parts must remain owned by `research-cli`
- what `Happier` patterns should be imported as hard requirements
- how remote/mobile/web support can stay operator-grade without becoming a
  second hidden runtime

## 1. Main Conclusion

The correct implementation route is now:

1. fork/adapt `Happy` as the remote transport and pairing substrate
2. keep `research-cli` as the runtime and persistence authority
3. add a projection/control adapter between them
4. selectively port `Happier` contracts or modules for attach, handoff,
   terminal, daemon ownership, and capability gating

So the final remote stack is not:

- "fork Happy and stuff our runtime into it"
- "bolt a web UI onto research-cli directly"
- "rebuild Happier from scratch"
- "greenfield a new remote substrate under research-cli"

It is:

- forked/adapted `HappyHost` for encrypted device connectivity and
  session/machine routing
- `ResearchRemoteAdapter` for event projection and remote action intake
- `research-cli` kernel for all real state transitions
- `ResearchWorkbench` as a research-native layer over the generic host

The implementation bias should therefore be:

- reuse first from `Happy`
- import second from `Happier`
- invent only the research-cli-specific adapter, projections, and workbench

## 2. What `Happy` Concretely Gives Us

### 2.1 Three-Scope Socket Topology

`Happy` already has the correct remote connectivity split:

- user-scoped sockets for app/web account-level sync
- session-scoped sockets for one live session process
- machine-scoped sockets for one machine daemon

This is much stronger than a generic "one websocket per app" design because it
separates:

- project/session rendering
- device presence
- machine control
- daemon-side RPC ownership

We should keep this topology.

For `research-cli`, the mapping should be:

- user-scoped -> account-level remote client sync and workbench navigation
- session-scoped -> live session transcript projection and runtime control
- machine-scoped -> machine capabilities, terminal RPC, and machine-local
  session operations

### 2.2 Durable Updates, Ephemeral Signals, And RPC Are Different Channels

`Happy` distinguishes:

- durable `update` events
- transient `ephemeral` activity
- point-to-point `rpc-call`

This distinction should be preserved exactly.

For `research-cli`, the mapping should be:

- durable updates -> mirrored session stream, remote metadata, and workbench
  projection snapshots
- ephemeral -> typing/thinking/activity/online status/wake hints
- RPC -> permission responses, takeover, attach, terminal IO, and bounded
  control actions

We should not collapse these into one "remote event bus".

### 2.3 A Flat Session Rendering Protocol Already Exists

`Happy`'s session protocol is already close to what we need for remote
rendering:

- flat event stream
- explicit turn lifecycle
- explicit tool-call start/end
- subagent-aware message projection
- provider-specific runtime details hidden behind adapter mapping

This is extremely valuable because our runtime already wants:

- auditable turns
- explicit tool traces
- explicit branch/debate/subagent rendering
- clean user/mobile/web rendering without provider leakage

So the first move should be:

1. keep `.pmcli/` as canonical runtime state
2. project selected runtime events into Happy-compatible session envelopes
3. avoid rewriting the remote wire format in milestone 1

### 2.4 Remote Control Is Already Modeled As Explicit Session RPC

The important `Happy` lesson is not just "it has a mobile app".

It is that remote control is modeled as explicit RPC handlers for session
behavior:

- abort
- switch
- permission interaction
- notification emission
- session-id tracking across resume/fork/switch

That means remote control is not a UI hack. It is a control contract.

For `research-cli`, remote control should therefore also flow through explicit
kernel actions:

- interrupt current turn
- transfer control owner
- answer permission request
- enqueue user message
- request direct steering
- inspect projected project state

No remote screen should mutate kernel state by writing side metadata directly.

### 2.5 Machine Metadata Is Capability Advertisement, Not Just Decoration

`Happy`'s machine client periodically republishes capability-related metadata:

- CLI availability
- resume support
- daemon state

That is the correct pattern for our design too.

`research-cli` machines should advertise a `RemoteCapabilityMatrix`, not just a
hostname string.

At minimum it should publish:

- installed runtime version
- provider/runtime availability
- resume support
- attach support
- handoff support
- terminal support
- worktree support
- ProjectOps background support
- queued experiment supervision support

This should be refreshed by the machine-scoped daemon, not guessed by the app.

That same daemon should also own a machine-scoped persisted state object for:

- daemon status
- control endpoint reachability
- current CLI version
- tracked session IDs
- shutdown or takeover reason

This follows the strongest `Happy` / `Happier` lesson: machine state must be
owned by the machine-scoped daemon, not reconstructed from UI traffic.

### 2.6 Project Grouping By `machineId + path` Is The Right Seed, But Not The Final Model

`Happy` groups projects by machine ID plus path.

That is the correct substrate for remote browsing, but it is not sufficient for
`research-cli`, because we also need:

- stable project identity across worktrees
- branch family visibility
- artifact-family state
- project memory / ProjectOps projection

So we should adopt:

- `Happy` project grouping as the host-level discovery seed
- `research-cli` `ProjectRegistry` as the actual project authority

The remote host should browse by machine/path, but resolve into
`project_id` as soon as the kernel can identify it.

## 3. What `Happy` We Should Reuse And What We Should Not

### Reuse directly

- encrypted relay model
- user/session/machine-scoped websocket topology
- RPC ownership model
- mobile/web connection flow
- session rendering envelope model
- basic session/machine browsing model
- notification routing substrate

### Reuse with adaptation

- session protocol mapping layer
- project grouping
- machine metadata
- takeover status display
- session browsing UX

### Do not adopt as runtime authority

- generic session metadata as the canonical source of project intelligence
- generic session message store as the only durable transcript
- broad host-side file/bash RPC as the source of code-agent truth
- any app-local state machine that can drift from `.pmcli/`

`research-cli` must not become "Happy with extra metadata stuffed inside".

## 4. What `Happier` Adds That We Must Learn From

### 4.1 Runtime/Backend-Aware Capability Gating

`Happier` is strong where many projects stay vague:

- backend/runtime kind is explicit
- capability support is evaluated per runtime surface
- resume/handoff support is treated as conditional, not assumed

This is exactly what we need because `research-cli` will support:

- multiple providers
- multiple runtime backends
- direct vs queued research execution
- different attach/handoff behavior by backend

So remote control must be capability-driven.

We need a first-class `RemoteCapabilityMatrix` plus evaluation helpers similar
in spirit to `Happier`'s:

- session capability support
- vendor resume eligibility
- vendor handoff eligibility
- attach strategy availability

This should not remain only a boolean matrix.

The stronger `Happier` lesson is that one session should expose one canonical
runtime/source/affinity record that all remote behaviors resolve through.

For `research-cli`, that means:

- runtime identity is carried by one `SessionRuntimeDescriptor`
- source provenance is carried by one `ProviderSessionSource`
- transcript rendering provenance is carried by one `TranscriptSourceEnvelope`
- local-control behavior is carried by one `LocalControlCapability`
- existing-session automation is carried by one
  `ExistingSessionAutomationEligibility`

So resume, browse, attach, handoff, takeover, and local reclaim are all judged
against the same source-of-truth objects instead of per-surface heuristics.

### 4.1.1 Existing Session Automation Must Be Explicit

`Happier` is especially strong at deciding whether an already-running session
should be resumed through the vendor or through the Happy attach path.

We should import that pattern directly.

`research-cli` should therefore freeze:

- `ExistingSessionAutomationEligibility`
- `strategy = vendor_resume | happy_attach`
- refusal reason codes when neither path is legal

That decision must be inspectable before execution.

It must not be hidden in host-side fallback branches.

### 4.1.2 Local Control Must Be Advertised As Capability Metadata

`Happier`'s local-control contract is better than a plain `attach_supported`
flag because it distinguishes:

- whether local control is supported at all
- whether the topology is shared or exclusive
- whether attach is provider-native or host-terminal based

`research-cli` should adopt the same idea through `LocalControlCapability` with:

- `supported`
- `topology`
- `attach_strategy`

This matters because local reclaim, attach execution, remote follow-only mode,
and handback behavior all depend on the same capability record.

### 4.2 Attach And Handoff Must Be Explicit Eligibility + Execution Contracts

`Happier` treats attach and handoff as separate contracts:

- eligibility evaluation
- storage mode awareness
- source-machine constraints
- backend/runtime constraints
- conflict policy
- workspace transfer policy

This should become a hard requirement for `research-cli`.

We should not ship a vague `remote attach` that means different things in
different contexts.

We need:

- `AttachEligibility`
- `HandoffEligibility`
- `WorkspaceTransferPolicy`
- `ControlOwnerTransition`

so that operator behavior is inspectable before it executes.

### 4.3 Embedded Terminal Must Be A Bounded Service

`Happier`'s daemon PTY work is important because it shows the correct shape:

- explicit terminal ensure/read/input/resize/close/restart RPCs
- filesystem-policy-checked cwd resolution
- bounded event buffers
- session limits and idle reaping
- URL extraction and structured terminal stream events

That is much better than "just expose a shell over websocket".

For `research-cli`, remote terminal support should therefore be:

- daemon-managed
- policy-bounded
- project-root aware
- auditable
- optional, not required for every remote feature

The embedded terminal is an operator instrument, not a hidden alternate agent.

### 4.4 Daemon Ownership And Takeover Need A Real Policy

`Happier`'s daemon ownership work addresses a real operational problem:

- which daemon owns the machine
- when an old daemon can be replaced
- when takeover must be explicit
- how version drift affects ownership

We need this too because our target system will have:

- background ProjectOps
- remote follow/takeover
- queued research supervision
- mobile/web control surfaces

Without daemon-ownership policy, remote control becomes flaky and surprising.

### 4.5 Control Client And Timeout Discipline Matter

`Happier`'s control client is valuable because it treats local daemon control
as a real product surface:

- explicit timeouts
- stale-state cleanup
- ping-based reachability
- typed error propagation

We should mirror that discipline for `research-cli`, especially for:

- session spawn
- queued research wake-ups
- remote terminal launch
- remote handoff/attach

## 5. Final Remote Architecture For `research-cli`

The remote architecture should be split into four ownership planes.

### Plane A: `HappyHost`

Owns:

- device pairing
- relay transport
- mobile/web connectivity
- account/session/machine routing
- encrypted message transport
- notification delivery

### Plane B: `ResearchRemoteAdapter`

Owns:

- runtime-event to session-envelope mapping
- remote RPC intake and normalization
- remote control-owner tracking
- remote capability publication
- workbench projection snapshots

This is the key integration plane.

### Plane C: `ResearchKernel`

Owns:

- all real session state
- `.pmcli/` persistence
- permissions
- tools
- memory
- branches
- reviews
- artifacts
- ProjectOps
- research workflows

### Plane D: `ResearchWorkbench`

Owns remote product surfaces that generic hosts do not have:

- memory explain
- branch/debate inspection
- review packet state
- artifact family browser
- cleanup proposal approval
- experiment supervision
- project health
- wake queue

## 6. Required Internal Remote Modules

The current `internal/remotehost` placeholder should be refined into the
following implementation units.

These units are integration-owned modules around the reused host substrate.
They are not a plan to rewrite `Happy` from zero inside our own tree.

### `internal/remotehost/transport`

Owns:

- Happy socket/session/machine integration
- reconnection and mirror cursors
- session-scoped and machine-scoped client wrappers

Primary types:

- `RemoteMirrorCursor`
- `RemoteSocketScope`
- `RemoteTransportState`

### `internal/remotehost/projection`

Owns:

- runtime event -> Happy session envelope projection
- remote transcript mirror checkpoints
- side-channel workbench snapshot assembly
- transcript-source normalization
- read-only cached browse indexes for host routing convenience

Primary types:

- `SessionEnvelopeProjection`
- `RemoteProjectionSnapshot`
- `RemoteProjectionCheckpoint`
- `TranscriptSourceEnvelope`
- `GatewaySessionIndexEntry`
- `GatewayChannelDirectoryEntry`

The serialized wire shape of `SessionEnvelopeProjection` is frozen as
`SessionEnvelopeProjectionV1` in
`docs/deep_study/25-remote-transport-auth-contract.md`.

### `internal/remotehost/actions`

Owns:

- remote action normalization
- permission responses
- takeover requests
- attach requests
- queued message delivery

Primary types:

- `RemoteAction`
- `RemotePendingAction`
- `RemoteActionResult`
- `RemoteControlOwner`

### `internal/remotehost/capabilities`

Owns:

- per-machine capability publication
- per-session attach/handoff/terminal eligibility
- runtime/backend-aware gating

Primary types:

- `RemoteCapabilityMatrix`
- `AttachEligibility`
- `HandoffEligibility`
- `TerminalEligibility`

### `internal/remotehost/terminal`

Owns:

- embedded remote terminal adapter
- PTY session lease and buffer management
- policy-checked working directory resolution

Primary types:

- `RemoteTerminalLease`
- `RemoteTerminalCursor`
- `RemoteTerminalEvent`

### `internal/remotehost/workbench`

Owns:

- projection of project memory / ProjectOps / branch / review state to remote
  panels
- panel refresh policies
- remote explain payloads

Primary types:

- `RemoteProjectKey`
- `RemoteWorkbenchPanel`
- `RemoteExplainPacket`

## 7. Action And Projection Contract

The remote adapter should speak in two directions only.

### 7.1 Kernel -> Host projection

Kernel emits:

- session transcript events
- session status
- permission pending/resolved
- active agents
- branch/debate state
- ProjectOps ticks and proposals
- review state
- artifact-family state
- memory explain payloads

Adapter turns that into:

- Happy-compatible session envelopes for the live transcript
- remote workbench snapshots for research-native panels
- ephemeral activity updates
- notifications

Important projection rule:

- cached remote browse/index structures may exist for routing and UX
- those indexes are convenience read models only
- they are regenerated from kernel-owned state, transcript artifacts, and
  canonical bindings
- they may never become an alternate authority for session ownership,
  transcript truth, or project identity

### 7.2 Host -> Kernel actions

Host sends only typed control actions such as:

- `interrupt_turn`
- `takeover_session`
- `attach_local_terminal`
- `send_user_message`
- `respond_permission_request`
- `approve_cleanup_proposal`
- `inspect_memory_explain`
- `inspect_branch_batch`
- `wake_supervised_run`

The kernel decides whether each action is valid.

The host never writes project memory or artifact state directly.

## 8. Remote Session Model We Should Add

The current `RemoteMachine` / `RemoteSessionBinding` model should be extended
with the following types.

## `RemoteCapabilityMatrix`

```text
RemoteCapabilityMatrix {
  runtime_version
  provider_backends[]
  resume_supported
  attach_supported
  handoff_supported
  terminal_supported
  worktree_supported
  projectops_background_supported
  queued_research_supported
  last_checked_at
}
```

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

This object records the real source and affinity of a live session so attach,
resume, and takeover do not depend on transport-specific guessing.

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

This object makes transcript rendering provenance explicit.

It tells the remote layer whether it is following:

- canonical project transcript
- projected remote envelope stream
- compacted recap line
- imported vendor transcript mirror

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

Read-model rule:

- remote host caches such as `sessions.json`-style indexes and channel
  directories are allowed for browse/routing convenience
- they must be explicitly marked derived, cached, and non-authoritative
- they may lag temporarily, but any mutating operation must re-resolve against
  kernel-owned bindings and runtime descriptors before execution

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

- daemon state is machine-global, not project-local
- daemon state survives normal shutdown as a last-known state record
- tracked session IDs are advisory machine-state facts, not session authority
- control port or local-control reachability must be refreshed by the daemon,
  not guessed by remote UI behavior

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

This is a projected operator view derived from the canonical `ControlLease`
object frozen in `25-remote-transport-auth-contract.md`.

## `RemoteProjectionSnapshot`

```text
RemoteProjectionSnapshot {
  project_id
  session_id?
  branch_batch_id?
  active_agent_ids[]
  pending_permission_ids[]
  cleanup_proposal_ids[]
  wake_events[]
  memory_explain_refs[]
  updated_at
}
```

`RemoteProjectionSnapshot` is an operator projection only.

It is not allowed to become a parallel runtime family schema.

Its fields must therefore be derived from canonical authorities already frozen
elsewhere:

- `active_agent_ids[]` is a projection over canonical agent-family records in
  `AgentRuntimeState`
- `pending_permission_ids[]` is a projection over permission-family pending
  requests
- `cleanup_proposal_ids[]` is a projection over repo/project-ops cleanup state
- `wake_events[]` is a projection over remote/session wake notifications
- `memory_explain_refs[]` is a projection over memory explain/query outputs

If a projected field ever disagrees with project-local runtime truth, the
project-local truth wins and the projection must be regenerated.

## `RemoteTerminalLease`

```text
RemoteTerminalLease {
  terminal_id
  machine_id
  project_id
  cwd
  access_policy
  opened_by
  last_activity_at
}
```

## 9. Command Semantics We Should Freeze

The authoritative remote command registry and event semantics live in:

- `22-remote-command-and-fixture-contract.md`
- `23-cli-operator-contract.md`

This document is explanatory and must stay aligned with those contracts.

### `research-cli remote pair`

- pairs the current machine with the remote host substrate
- publishes machine metadata and capability matrix
- does not start a second runtime

### `research-cli remote status`

- shows paired devices, active sessions, control owner, and remote capability
  view
- must support JSON output

### `research-cli remote attach`

- resolves explicit attach eligibility
- can mean provider-native attach or terminal-host attach
- must explain why attach is unavailable

### `research-cli remote handoff`

- transfers control between already-bound remote surfaces
- preserves runtime descriptor and projection continuity
- must refuse when capability, lease epoch, or target-surface policy blocks it

### `research-cli remote takeover`

- transfers control owner for a live session
- must be auditable
- must not silently drop the terminal-side operator

### `research-cli remote notify`

- sends structured push or surface notifications
- should be routed from kernel events, not ad hoc strings only

## 10. What Makes This Better Than Just Using `Happy` Or `Happier`

If we follow this blueprint, our result is not a clone.

### Better than raw `Happy`

Because we add:

- project-native memory
- ProjectOps
- branch/debate visibility
- artifact-family governance
- research workflow supervision
- result-to-claim repair routing

### Better than raw `Happier`

Because we keep the strong parts of its operator contracts, but the product is
built around:

- native project-level memory
- research-grade multi-agent control
- repo anti-chaos governance
- explicit research workbench surfaces

### Better than both

Because the remote host is no longer just "control a coding session remotely".

It becomes:

- a remote control plane for a project-native code agent
- a remote observability plane for long-running research work
- a remote governance plane for memory, artifacts, cleanup, and review

## 11. Non-Negotiable Guardrails

The following must stay true during implementation:

1. `.pmcli/` remains the source of truth
2. remote session mirrors are projections, not canonical storage
3. ProjectOps, memory, branches, and reviews stay kernel-owned
4. remote terminal is bounded and policy-checked
5. attach/handoff/takeover are explicit eligibility + execution contracts
6. provider session source, transcript source, and local-control capability are
   explicit typed contracts rather than UI heuristics
7. cached host indexes remain read models only and never become runtime truth
6. runtime/backend differences are capability-gated, not hidden
7. mobile/web convenience never weakens permission, cleanup, or review gates

## 12. Implementation Order

### Step 1: Happy Compatibility Layer

- pair machine
- publish capability matrix
- follow live sessions remotely
- support permission response and takeover
- project runtime events into Happy session envelopes

### Step 2: Research Remote Adapter

- add remote action gateway
- add workbench snapshots
- surface memory/artifact/branch/review/ProjectOps panels

### Step 3: Happier-Grade Operator Contracts

- attach eligibility
- handoff eligibility
- terminal PTY service
- daemon ownership and takeover policy
- advanced capability gating

### Step 4: Research-Native Remote Workbench

- debate inspector
- branch adjudication
- cleanup approval
- experiment supervision and wake queue
- remote memory explain and invalidation

That is the concrete route to a mobile/web-capable `research-cli` that is
strictly stronger than the reference systems instead of merely inspired by
them.
