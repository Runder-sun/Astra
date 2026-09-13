# Reference Superiority Self-Audit

This document is the internal gate before the next no-context external review.

Its purpose is not to repeat the architecture.

Its purpose is to check whether the current design is now strong enough to
credibly claim:

- better code-agent CLI foundations than the strongest open references
- native project-memory and research extensions without kernel drift
- lower rewrite risk than a direct fork of any one reference project

Important evidence rule:

- this document audits whether the architecture is shaped to exceed the
  references
- it does not by itself prove that superiority has already been demonstrated
- implementation, conformance, parity, and fixture evidence are still required
  before claiming proven superiority

The comparison set is:

- `hermes-agent`
- `claw-code`
- `OpenCode -> Crush`
- `Happy`
- `Happier`
- `Dr. Claw`

## 1. Review Standard

The design only counts as stronger than the references if it is better on both
of these dimensions at the same time:

1. clearer authority and protocol boundaries
2. more executable operator-facing behavior

If a capability exists only as prose aspiration, it does not count.

If a capability exists but introduces a second truth or app-owned runtime, it
also does not count.

Minimum no-context review packet for future external review:

- `14-executable-runtime-protocol.md`
- `15-research-review-and-contracts.md`
- `18-proactive-project-ops.md`
- `23-cli-operator-contract.md`
- `24-kernel-state-machine-contract.md`
- `25-remote-transport-auth-contract.md`
- `26-memory-branch-research-runtime-policy.md`
- `28-schema-registry-and-conformance-plan.md`
- `37-kernel-type-and-interface-manifest.md`
- `40-reference-superiority-self-audit.md`

If a reviewer cannot judge runtime authority, operator behavior, advanced
lifecycle legality, remote semantics, and superiority gating from that packet,
the architecture still is not ready for blind review.

## 1.1 Current Blind-Review Status

The latest no-context blind review result is:

- verdict: `conditional_go`
- current design strength: strong architecture-level authority and contract line
- current proof gap: superiority over `Hermes` / `claw-code` is not yet proven
  without implemented schema, fixture, and conformance evidence

Interpretation rule:

- architecture-level superiority language in this document means
  "positioned to exceed if implemented against the frozen obligations"
- it must not be interpreted as completed implementation proof
- the exact evidence threshold for base-CLI superiority is frozen in
  `44-base-cli-superiority-proof-gate.md`

Scope control rule:

- broad type and schema coverage in the packet does not mean simultaneous
  implementation obligation
- M0-M2 only implements the foundation objects explicitly marked as first-build
  obligations
- later advanced nouns are reserved contract slots and remain feature-gated
  until their milestone, schema, and fixture obligations are satisfied

## 2. Where The Current Design Is Architected To Exceed The References

## 2.0 Reference Decision Matrix

The current design is not trying to "become" any one reference.

For each reference, the intended relationship is:

### `Hermes`

- retained:
  broad operator surface, long-horizon recall, delegation, multi-surface
  delivery
- rejected:
  monolithic runtime ownership
- exceeded:
  kernel authority boundaries, event/protocol discipline, project-local truth

Blind-review qualifier:

- packet-level architecture is stronger
- blind-proven exceedance is still pending implementation and fixture evidence

### `claw-code`

- retained:
  doctor-first bring-up, permission rigor, parity discipline, machine-readable
  CLI behavior
- rejected:
  limiting the product mostly to kernel/runtime strength without native
  project-memory or research-runtime depth
- exceeded:
  project-native memory, ProjectOps, artifact governance, branch/debate/review
  integration under one authority line

Blind-review qualifier:

- packet-level parity is strong
- blind-proven exceedance is still pending implementation and fixture evidence

### `OpenCode -> Crush`

- retained:
  TUI-first ergonomics, session continuity, compaction, service decomposition
- rejected:
  leaving session search/continuity as strong UX but weaker protocol-level
  ownership
- exceeded:
  project registry, rebuildable non-authoritative session index, explicit
  non-graduated command policy, stronger repo cleanup control

Blind-review qualifier:

- packet-level productization is strong
- blind-proven exceedance still depends on the comparative proof matrix in `28`

### `Happy` / `Happier`

- retained:
  machine-scoped daemon concepts, scoped sockets, RPC/control semantics,
  machine/user/session separation
- rejected:
  any drift toward app/remote-owned runtime truth
- exceeded:
  explicit projection-plane-only remote design over a canonical CLI kernel

### `Dr. Claw`

- retained:
  productization lessons, project discovery, research workspace usability
- rejected:
  app/server layer as runtime authority
- exceeded:
  kernel-first truth, protocol ownership, and operator surfaces without losing
  product breadth

## 2.1 Better than `Hermes` on kernel authority

`Hermes` demonstrates breadth:

- memory
- skills
- gateway surfaces
- delegation
- long-horizon recall

But its runtime is too broad to be a safe kernel reference.

The current `research-cli` line is architected more strongly because it now
freezes:

- one top-level `KernelStateBundle` authority
- one canonical `KernelEventEnvelope`
- one reducer-owned checkpoint publication path
- one explicit split between project-local truth and machine-global operator
  state

Authority anchors:

- `10-revised-final-architecture.md`
- `14-executable-runtime-protocol.md`
- `24-kernel-state-machine-contract.md`
- `39-project-registry-and-checkpoint-reconciliation-contract.md`

## 2.2 Better than `claw-code` on native project intelligence

`claw-code` is still the strongest foundation reference for:

- doctor-first behavior
- explicit permissions
- deterministic parity harnessing
- machine-readable operator surfaces

The current design is set up to keep those strengths and extend them with
native project-level systems that are not bolt-ons:

- project registry and current-project pointer
- native artifact-family governance
- proactive ProjectOps state
- project-memory query/explain/invalidation
- branch/debate/review lines that remain inside one runtime authority

Strength anchors:

- `16-kernel-parity-contract.md`
- `18-proactive-project-ops.md`
- `23-cli-operator-contract.md`
- `24-kernel-state-machine-contract.md`
- `26-memory-branch-research-runtime-policy.md`

## 2.3 Better than `OpenCode -> Crush` on project/runtime coherence

`OpenCode` and `Crush` prove:

- TUI-first usability
- session continuity
- compaction necessity
- service decomposition

The current design goes further at the contract level by freezing:

- project-scoped registry ownership
- rebuildable session search index without turning the index into truth
- stronger repo cleanup and supersession semantics
- explicit non-graduated command behavior instead of silent placeholder data

Coherence anchors:

- `21-code-agent-kernel-hardening.md`
- `23-cli-operator-contract.md`
- `31-kernel-foundation-implementation-plan.md`
- `36-repo-bootstrap-and-source-tree-manifest.md`

## 2.4 Better than `Happy` and `Happier` on runtime ownership

`Happy` and `Happier` are the right remote substrate references because they
prove:

- machine-scoped daemon ownership
- user/session/machine scope separation
- explicit RPC/control semantics

The current design is stronger at the architecture level because it refuses to
let the remote substrate become runtime truth.

The implementation route is also now explicit:

- fork/adapt `Happy` for the remote host base
- selectively port or mirror `Happier` advanced contracts where needed
- avoid a greenfield remote transport rewrite
- keep every research-specific behavior in projection, control adaptation, or
  kernel-owned state

Remote/mobile/web are now explicitly:

- a projection plane
- a control plane
- not a second kernel

Remote anchors:

- `19-remote-host-control-plane.md`
- `20-happy-happier-integration-blueprint.md`
- `25-remote-transport-auth-contract.md`

## 2.5 Better than `Dr. Claw` on kernel discipline

`Dr. Claw` is a strong productization reference:

- broad surfaces
- project discovery
- research workspace organization

But it is not a safe authority reference.

The current design clearly absorbs the product lessons without letting the app
layer own truth:

- import/reference external stores if needed
- keep `.pmcli/project_state.json` as the one runtime checkpoint
- keep operator surfaces productized without turning them into authorities

Productization anchor:

- `35-dr-claw-productization-review.md`

## 3. New Strong Points Added During Self-Audit

The current pass added six operator-grade strengths that were previously too
implicit.

## 3.1 Session search is now explicit and rebuildable

To keep the Hermes/OpenCode session-search advantage without introducing a
second persistence truth, the design now freezes:

- `internal/session/index.go`
- `SessionSearchIndex`
- `.pmcli/indexes/session_search.sqlite`

and marks the index as:

- rebuildable
- derived
- non-authoritative

This is stronger than leaving search as a hidden implementation detail.

## 3.2 Plugin and hook surfaces are now first-class

To match top-tier code-agent CLI expectations, the design now freezes:

- `plugins list/inspect/validate`
- `hooks list/inspect/test`
- `PluginListResult`
- `PluginInspectionResult`
- `PluginValidationResult`
- `HookListResult`
- `HookInspectionResult`
- `HookTestResult`
- `internal/plugins/registry.go`
- `internal/plugins/hooks.go`

This closes a real gap relative to `claw-code`.

The same payload closure now exists for registry-style operator surfaces that
are usually left as command-local JSON:

- `SkillListResult`
- `SkillInspectResult`
- `SkillPathsResult`
- `SkillValidationResult`
- `MCPListResult`
- `MCPInspectionResult`
- `MCPTestResult`
- `MCPRefreshResult`

## 3.3 Deterministic mock-provider parity is now implementation-owned

To keep `claw-code`-grade parity discipline, the design now explicitly reserves:

- `tests/mock_provider/`
- `scripts/run_mock_parity_harness.sh`

This matters because provider/auth/session behavior is too important to verify
only against live vendors.

## 3.4 Artifact and ProjectOps aggregates are now reserved in the bundle

This closes the risk that later artifact governance or proactive-project
behavior would become shadow authorities.

The bundle now explicitly reserves:

- `artifact_state`
- `projectops_state`

## 3.5 Remote control and projection types are now field-frozen and
regeneration-gated

The remote design is no longer relying only on architectural prose.

The manifest now explicitly freezes:

- `RemoteCapabilityMatrix`
- `RemoteCapabilitySet`
- `RemoteMachineMetadata`
- `RemoteDaemonState`
- `ProviderSessionSource`
- `SessionRuntimeDescriptor`
- `TranscriptSourceEnvelope`
- `LocalControlCapability`
- `ExistingSessionAutomationEligibility`
- `SessionEnvelopeProjectionV1`
- `RemoteCursor`
- `ControlLease`
- `RemoteControlOwner`
- `RemoteProjectionSnapshot`
- `RemoteTerminalLease`

This matters because remote/mobile/web had become one of the easiest places for
state and ownership drift to hide.

The design is stronger now that remote transport, projection, reconnect, and
control ownership all have explicit type-level anchors rather than only
descriptive text.

The current pass also makes remote projection discipline executable rather than
aspirational by requiring:

- projection mismatch regeneration instead of shadow-state arbitration
- typed revocation and rejection fixtures
- remote harness coverage before remote command graduation
- schema-owned machine identity, client identity, pairing, switch policy, and
  feature advertisement objects
- provider session source, transcript provenance, and local-control strategy to
  remain typed instead of UI heuristics
- cached remote browse indexes to stay explicit read models rather than hidden
  authorities

## 3.6 Core code-agent operator payloads and traces are now frozen

The base CLI no longer relies only on command names and prose contracts.

The manifest and implementation plan now explicitly reserve:

- `InteractiveLaunchResult`
- `TurnResult`
- `ResumeResult`
- `SessionInspection`
- `ProjectInspection`
- `CompactResult`
- `SessionIdentity`
- `SessionLineageRecord`
- `SessionBrowseResult`
- `ResumeAmbiguityResult`
- `SessionSearchResult`
- `SessionExportResult`
- `SessionStats`
- `ProjectRegistryList`
- `ProjectStatus`
- `ProjectResolutionTrace`
- `RuntimePreflightReport`
- `DoctorReport`
- `SmokeResult`
- `CommandSuccess`
- `CommandFailure`
- `FeatureGateResult`
- `PolicyRefusalResult`
- `FollowupRequiredResult`
- `PruneResult`
- `PermissionPendingList`
- `PermissionHistoryResult`
- `ConformanceResult`
- `MemoryStatusReport`
- `ArtifactListResult`
- `ArtifactInspectionResult`
- `HelpSurfaceReport`
- `PaletteSurfaceReport`
- `ProfileResolutionTrace`
- `InterruptResult`
- `RetryResult`
- `UndoRefusal`
- `SessionOperatorLogManifest`
- `DerivedSessionReadModel`
- `startup_profile_doctor_priority`
- `ModelCatalogList`
- `ModelCurrentResult`
- `ProviderStatus`
- `ProviderStatusList`
- `ProviderCatalogState`
- `EffectiveConfigReport`
- `ConfigValueResult`
- `SessionStats`
- `UsageSummary`
- `StatsSummary`
- `SetupStatusReport`
- `MigrateCheckReport`
- `RepairHintsReport`
- `InstallRoutesReport`
- `MCPServerRecord`
- `MCPListResult`
- `MCPInspectionResult`
- `MCPTestResult`
- `MCPRefreshResult`
- `PluginListResult`
- `PluginInspectionResult`
- `PluginValidationResult`
- `HookListResult`
- `HookInspectionResult`
- `HookTestResult`
- `SkillRegistryEntry`
- `SkillListResult`
- `SkillInspectResult`
- `SkillPathsResult`
- `SkillValidationResult`
- `RemotePairResult`
- `RemoteTakeoverResult`
- `RemoteNotifyResult`
- `ProviderResolutionTrace`
- `ProviderSessionSource`
- `TranscriptSourceEnvelope`
- `LocalControlCapability`
- `ExistingSessionAutomationEligibility`
- `PermissionDecisionTrace`
- `RuntimeFeatureStatus`

This matters because these are the exact objects that decide whether the base
product is truly a top-tier code agent CLI or just a promising architecture
with under-specified operator surfaces.

The design is stronger now that the hardest `Hermes` / `claw-code` / `Crush`
comparative areas - session continuity, explainable provider routing,
permission reasoning, and inspectability - have explicit type and ownership
anchors rather than only high-level claims.

## 3.6.3 Session logs and read models are now explicit instead of hidden
implementation detail

The current design now absorbs two practical strengths from the code-level
references without inheriting their drift risks:

- OpenCode-style per-session durable request/response/tool-result log artifacts
- Hermes-style cached browse/routing indexes

But the architecture makes both safer than the references by freezing:

- `SessionOperatorLogManifest` for durable log correlation
- request-sequence continuity as an inspectable operator contract
- `DerivedSessionReadModel` for search/browse caches with
  `authoritative=false`

This matters because a serious code agent CLI needs:

- session-grade debugging without only reading transcript blobs
- search and browse speed without turning indexes into truth
- replay/export surfaces that can explain where evidence came from

## 3.6.5 Session terminal reconciliation and event provenance are now fixture-gated

This pass also hardens the base code-agent CLI line in the exact places where
`claw-code` was still ahead:

- contradictory terminal-state reconciliation
- typed uncertainty after transport death
- event provenance labeling
- owner/scope binding on actionable events

The design is stronger now that these are no longer only roadmap intentions.
They are tied to explicit fixture and CI obligations:

- `session_terminal_reconciliation_under_transport_loss`
- `session_creation_identity_complete`
- `session_event_provenance_and_scope_binding`

That matters because a strong operator CLI cannot merely emit events. It must
emit event truth that remains machine-trustworthy under noisy or contradictory
session churn.

## 3.7 Failure lanes are now typed instead of string-only

The base CLI is now stronger wherever non-zero exits previously risked dropping
structured context.

The operator envelope explicitly allows typed failure `data`, which means:

- ambiguous resume keeps `ResumeAmbiguityResult`
- unresolved project scope keeps `ProjectResolutionTrace`
- blocked `doctor` keeps `DoctorReport`
- blocked `smoke` keeps `SmokeResult`
- `not_graduated` keeps `FeatureGateResult`
- degraded-but-loadable `11` lanes keep `FollowupRequiredResult`

This matters because a serious code agent CLI cannot require humans or wrappers
to scrape stderr just because a command exited non-zero.

## 3.8 Launch, turn, and compaction continuity are now one-hop inspectable

The base CLI is also stronger where other systems often require multiple helper
commands or UI-only state to reconstruct what just happened.

The current design now requires:

- `InteractiveLaunchResult` to expose preflight plus project/profile traces
- `TurnResult` to expose project/profile/provider traces in one payload
- `CompactResult` to expose refreshed derived views plus deferred repairs
- `research-cli --json` / `chat --json` to use launch-inspect-and-exit rather
  than ambiguous mixed REPL stdout

This matters because daily operator loops and remote wrappers need one-hop
truth, not a second round of discovery commands after every launch, turn, or
compaction.

## 3.9 Previously prose-only command families now have canonical payload owners

The design is stronger now that several frozen commands are no longer broader
than the machine contract.

This pass added canonical payload lines for:

- `projects prune`
- `permissions pending`
- `permissions history`
- `research-cli conformance`
- `memory status`
- `artifacts list`
- `artifacts inspect`
- `repo cleanup-plan`
- `repo cleanup-apply`

This matters because a top-tier code agent CLI cannot claim broad operator
surfaces while leaving command results to ad hoc JSON or text scraping.

## 3.10 Advanced memory, ProjectOps, remote, and research runtime are now
fixture-gated instead of prose-only

The most important change in the current pass is that the advanced runtime no
longer stops at state machines and structs.

The design now ties these advanced families to explicit golden fixtures, CI
phases, and release gates:

- memory promotion legality and rollback-triggered invalidation
- ProjectOps digest promotion/rejection traceability
- tagged-union legality for remote actions
- inspect-vs-execute remote attach/handoff grammar
- Happy-style local keypress reclaim with ownership-epoch advancement
- notification payload plus receipt linkage
- wake escalation and supervisor-lease linkage
- research-stage gate -> repair/pivot -> successor replay legality

This matters because the previous external review correctly rejected any design
that only described how superiority would later be validated. The current line
still does not claim implementation proof, but it now makes advanced validation
obligatory rather than optional.

## 4. Why This Is Now More Implementable Than A Fork

Forking any one reference repo would still require:

- retrofitting project-local runtime truth
- retrofitting project-native memory
- retrofitting debate/branch/review governance
- retrofitting remote authority separation
- retrofitting repo-cleanup and artifact-family control

The current design is therefore more implementable for the target product than
a direct fork because the hard boundaries are already aligned with the target.

## 4.2 Superiority Evidence Matrix

The design should not claim superiority unless each major claim can be traced
through all four layers below:

1. authority or contract document
2. implementation blueprint / ownership
3. type or schema freeze
4. implementation plan / PR batch / fixture gate

### Claim: single kernel truth is stronger than the references

- authority:
  `10`, `14`, `24`, `39`
- ownership:
  `11`
- type freeze:
  `37`
- execution + fixtures:
  `31`, `38`, `29`

### Claim: session continuity is stronger than `OpenCode` / `Hermes`

- authority:
  `16`, `21`, `23`
- ownership:
  `11`
- type freeze:
  `37` (`SessionTitleRecord`, `ResumeRecap`, `SessionSearchIndex`)
- execution + fixtures:
  `31`, `38`, `29`

### Claim: plugin/hook surfaces are stronger than the usual MCP-only baseline

- authority:
  `16`, `23`
- ownership:
  `11`
- type freeze:
  `37`
- execution + fixtures:
  `31`, `38`

### Claim: remote/mobile/web is stronger than app-owned remote shells

- authority:
  `19`, `20`, `25`
- ownership:
  `11`
- type freeze:
  `37`
- execution + fixtures:
  `33`, `29`

Remote superiority does not count if any of these regress:

- remote event names drift away from the canonical `24` registry
- remote capability/projection/terminal objects are frozen in prose but not
  registered in `28`
- remote machine metadata, daemon state, or control-owner objects remain
  manifest-only instead of schema-owned
- M0-M2 wording makes advanced remote types look like foundation implementation
  obligations instead of reserved later-milestone owners

### Claim: parity discipline is stronger than prose-only design claims

- authority:
  `16`, `21`, `23`
- ownership:
  `11`
- type freeze:
  `37`
- execution + fixtures:
  `29`, `31`, `38`

### Claim: provider routing and preflight are stronger than ambiguous CLI routing

- authority:
  `21`, `23`
- ownership:
  `11`
- type freeze:
  `37` (`ProviderResolutionTrace`, `ProviderStatus`, `ProviderStatusList`, `ModelCatalogList`, `ModelCurrentResult`, `EffectiveConfigReport`, `ConfigValueResult`, `RuntimePreflightReport`, `DoctorReport`, `SmokeResult`, `RuntimeFeatureStatus`)
- execution + fixtures:
  `28`, `31`, `38`

### Claim: setup, accounting, and environment recovery are stronger than the
usual ad hoc CLI baseline

- authority:
  `21`, `23`
- ownership:
  `11`
- type freeze:
  `37` (`UsageSummary`, `StatsSummary`, `SetupStatusReport`, `MigrateCheckReport`, `RepairHintsReport`, `InstallRoutesReport`, `MCPServerRecord`)
- execution + fixtures:
  `28`, `31`, `38`

### Claim: session identity and lineage are stronger than ID-only session stores

- authority:
  `21`, `23`
- ownership:
  `11`
- type freeze:
  `37` (`SessionIdentity`, `SessionLineageRecord`, `SessionInspection`)
- execution + fixtures:
  `28`, `31`, `38`

### Claim: project scoping is stronger than implicit cwd heuristics

- authority:
  `21`, `23`
- ownership:
  `11`
- type freeze:
  `37` (`ProjectRegistryEntry`, `CurrentProjectPointer`, `ProjectResolutionTrace`)
- execution + fixtures:
  `28`, `31`, `38`

### Claim: project memory and ProjectOps are stronger than script-only repo hygiene

- authority:
  `18`, `23`, `26`
- ownership:
  `11`
- type freeze:
  `37` (`MemoryRecord`, `MemoryQueryResult`, `MemoryExplainRecord`, `ProjectOpsTick`, `ProgressDigestCandidate`, `RepoCleanupProposal`)
- execution + fixtures:
  `28`, `33`, `38`

### Claim: multi-agent runtime is packet-bound rather than prompt-loose

- authority:
  `15`, `23`, `24`, `26`
- ownership:
  `11`
- type freeze:
  `37` (`TaskPacket`, `AgentRuntimeRecord`, `ReviewRuntimeRecord`, `BranchBatchRecord`, `BranchRunRecord`)
- execution + fixtures:
  `28`, `33`, `38`

## 4.5 Concrete Self-Gate Before Claiming Superiority

The design should only be presented as exceeding the references if all of the
following remain true at the same time:

1. every strong operator surface named in `16` also has an explicit owner in
   `31`, `36`, `37`, or `38`
2. every remote/mobile/web surface remains a projection/control surface rather
   than a second persistence authority
3. every rebuildable index/cache is explicitly marked derived and
   non-authoritative
4. every broad product surface added from references also has a refusal mode,
   degraded mode, or feature-graduation rule
5. provider resolution, session lineage, and preflight all have explicit trace
   surfaces
6. project scope resolution has an explicit trace surface and typed unresolved
   failure lane
7. project memory, digest, cleanup, and supervision all have explicit object
   and schema anchors
8. deterministic parity, conformance, and remote harnesses remain first-class
   release gates
9. every remote/session/workbench projection that can be regenerated is marked
   derived, non-authoritative, and fixture-backed under mismatch
10. launch and turn outputs expose routing traces without requiring follow-up
    introspection commands
11. command envelopes and their typed `data` payloads are pair-validated in
    conformance rather than treated as independent promises

## 5. Remaining Internal Bar Before External Review

The next external review should only happen if these statements remain true
across the primary authority docs:

1. no doc reintroduces project-local remote lease authority
2. no doc reintroduces `ProjectState` as top-level truth
3. no doc defines a competing event object beside `KernelEventEnvelope`
4. no M0-M2 implementation plan drops command/help registry ownership
5. no M0-M2 implementation plan drops session identity, session index,
   plugin/hook, provider trace, or mock
   parity ownership

If those stay true, the design is now at the point where a no-context review
should be judging sufficiency, not basic coherence.
