# Kernel Type And Interface Manifest

This document freezes the first-build type and interface line for M0-M2.

`11-implementation-blueprint.md` already defined module responsibilities.

`24-kernel-state-machine-contract.md` already froze the runtime submachines.

`31-kernel-foundation-implementation-plan.md` already froze execution order.

What remained ambiguous was the exact first set of code-level nouns that should
exist before implementation starts.

This document closes that gap.

Language note:

- the active implementation language is Rust
- the canonical nouns and field sets in this document remain authoritative
- Go-shaped examples below are historical structural sketches and should now be
  implemented as Rust `struct`/`enum` types with `serde`-owned serialization
- exact Rust path mapping is frozen in
  `46-rust-kernel-pivot-and-bootstrap.md`

## 1. Purpose

The goal is not to overdesign every future package.

The goal is to make sure M0-M2 workers do not waste time arguing about:

- what the first structs are called
- which package owns each interface
- which objects serialize to schema-backed JSON
- which fields must exist on day one

If a future milestone needs richer fields, it may add them compatibly.

M0-M2 may not rename the canonical objects defined here without updating the
frozen architecture documents.

## 2. First-Build Design Rules

1. canonical persisted objects must have explicit Rust structs or enums
2. every persisted struct must map to one schema owner
3. runtime services talk through interfaces at package boundaries
4. operator views may derive projections, but not alternate truths
5. no UI-only or transport-only shadow structs may become runtime authority

## 3. Canonical Runtime Types

## 3.1 `internal/runtime`

### `InteractiveLaunchResult`

```go
type InteractiveLaunchResult struct {
    SessionID       string                 `json:"session_id,omitempty"`
    ProjectID       string                 `json:"project_id,omitempty"`
    WorkspaceRoot   string                 `json:"workspace_root,omitempty"`
    Preflight       RuntimePreflightReport `json:"preflight"`
    ProjectTrace    ProjectResolutionTrace `json:"project_trace,omitempty"`
    ProfileTrace    ProfileResolutionTrace `json:"profile_trace,omitempty"`
    LaunchDisposition string               `json:"launch_disposition,omitempty"`
    InteractiveEligible bool               `json:"interactive_eligible,omitempty"`
    ReusedSessionID string                 `json:"reused_session_id,omitempty"`
}
```

### `TurnResult`

```go
type TurnResult struct {
    SessionID     string                  `json:"session_id,omitempty"`
    TurnID        string                  `json:"turn_id,omitempty"`
    ProjectID     string                  `json:"project_id,omitempty"`
    Outcome       string                  `json:"outcome,omitempty"`
    ProjectTrace  ProjectResolutionTrace  `json:"project_trace,omitempty"`
    ProfileTrace  ProfileResolutionTrace  `json:"profile_trace,omitempty"`
    ProviderTrace ProviderResolutionTrace `json:"provider_trace"`
}
```

### `ResumeResult`

```go
type ResumeResult struct {
    SessionID   string               `json:"session_id,omitempty"`
    ProjectID   string               `json:"project_id,omitempty"`
    SessionKind string               `json:"session_kind,omitempty"`
    Recap       ResumeRecap          `json:"recap"`
    Lineage     SessionLineageRecord `json:"lineage"`
}
```

### `InterruptResult`

```go
type InterruptResult struct {
    SessionID      string `json:"session_id,omitempty"`
    TurnID         string `json:"turn_id,omitempty"`
    ToolState      string `json:"tool_state,omitempty"`
    TurnState      string `json:"turn_state,omitempty"`
    RetryAvailable bool   `json:"retry_available,omitempty"`
}
```

### `RetryResult`

```go
type RetryResult struct {
    SessionID      string `json:"session_id,omitempty"`
    PreviousTurnID string `json:"previous_turn_id,omitempty"`
    NewTurnID      string `json:"new_turn_id,omitempty"`
    ReplayMode     string `json:"replay_mode,omitempty"`
}
```

### `UndoRefusal`

```go
type UndoRefusal struct {
    SessionID     string `json:"session_id,omitempty"`
    TurnID        string `json:"turn_id,omitempty"`
    ReasonCode    string `json:"reason_code,omitempty"`
    Reason        string `json:"reason,omitempty"`
    CleanupPlanID string `json:"cleanup_plan_id,omitempty"`
}
```

### `FeatureGateResult`

```go
type FeatureGateResult struct {
    FeatureID        string   `json:"feature_id,omitempty"`
    RequestedCommand string   `json:"requested_command,omitempty"`
    GateState        string   `json:"gate_state,omitempty"`
    Milestone        string   `json:"milestone,omitempty"`
    MissingOwners    []string `json:"missing_owners,omitempty"`
    NextSteps        []string `json:"next_steps,omitempty"`
}
```

### `PolicyRefusalResult`

```go
type PolicyRefusalResult struct {
    PolicyKind       string   `json:"policy_kind,omitempty"`
    PolicySource     string   `json:"policy_source,omitempty"`
    RefusalReason    string   `json:"refusal_reason,omitempty"`
    BlockingObjectID string   `json:"blocking_object_id,omitempty"`
    Retryable        bool     `json:"retryable,omitempty"`
    NextSteps        []string `json:"next_steps,omitempty"`
}
```

### `FollowupRequiredResult`

```go
type FollowupRequiredResult struct {
    OutcomeState   string   `json:"outcome_state,omitempty"`
    ValidatedParts []string `json:"validated_parts,omitempty"`
    DeferredParts  []string `json:"deferred_parts,omitempty"`
    FollowupSteps  []string `json:"followup_steps,omitempty"`
}
```

### `CommandError`

```go
type CommandError struct {
    Code      string `json:"code"`
    Message   string `json:"message"`
    Hint      string `json:"hint,omitempty"`
    Retryable bool   `json:"retryable,omitempty"`
}
```

### `CommandSuccess`

```go
type CommandSuccess struct {
    OK        bool   `json:"ok"`
    Command   string `json:"command"`
    ProjectID string `json:"project_id,omitempty"`
    SessionID string `json:"session_id,omitempty"`
    Data      any    `json:"data,omitempty"`
}
```

### `CommandFailure`

```go
type CommandFailure struct {
    OK        bool         `json:"ok"`
    Command   string       `json:"command"`
    ProjectID string       `json:"project_id,omitempty"`
    SessionID string       `json:"session_id,omitempty"`
    Error     CommandError `json:"error"`
    Data      any          `json:"data,omitempty"`
}
```

### `KernelStateBundle`

Authoritative checkpoint object serialized into `.pmcli/project_state.json`.

Minimum shape:

```go
type KernelStateBundle struct {
    ProjectState    ProjectState          `json:"project_state"`
    SessionState    SessionRuntimeState   `json:"session_state"`
    TurnState       TurnRuntimeState      `json:"turn_state"`
    AgentState      AgentRuntimeState     `json:"agent_state"`
    ReviewState     ReviewRuntimeState    `json:"review_state"`
    MemoryState     MemoryRuntimeState    `json:"memory_state"`
    RepoState       RepoRuntimeState      `json:"repo_state"`
    ArtifactState   ArtifactRuntimeState  `json:"artifact_state"`
    ProjectOpsState ProjectOpsRuntimeState `json:"projectops_state"`
    PermissionState PermissionRuntimeState `json:"permission_state"`
    FeatureState    FeatureRuntimeState   `json:"feature_state"`
    RollbackState   RollbackRuntimeState  `json:"rollback_state"`
    BranchState     BranchRuntimeState    `json:"branch_state"`
    RemoteState     RemoteRuntimeState    `json:"remote_state"`
    SeqCursor       uint64                `json:"seq_cursor"`
    CheckpointEpoch uint64                `json:"checkpoint_epoch"`
}
```

Important rule:

- even if several child states are still skeletal in M0-M2, the bundle shape
  must already reserve the canonical slots

### `ProjectState`

Minimal persisted project binding state.

```go
type ProjectState struct {
    ProjectID         string `json:"project_id"`
    WorkspaceRoot     string `json:"workspace_root"`
    WorkspaceHash     string `json:"workspace_hash"`
    ProtocolVersion   string `json:"protocol_version"`
    ActiveSessionID   string `json:"active_session_id,omitempty"`
    CurrentPermissionMode string `json:"current_permission_mode,omitempty"`
}
```

### `SessionRuntimeState`

```go
type SessionRuntimeState struct {
    SessionID      string `json:"session_id,omitempty"`
    Status         string `json:"status"`
    TranscriptPath string `json:"transcript_path,omitempty"`
    SummaryRef     string `json:"summary_ref,omitempty"`
}
```

### `TurnRuntimeState`

```go
type TurnRuntimeState struct {
    TurnID              string `json:"turn_id,omitempty"`
    Status              string `json:"status"`
    ActiveProviderRoute string `json:"active_provider_route,omitempty"`
    PendingPermissionID string `json:"pending_permission_id,omitempty"`
}
```

### Other child runtime states

For M0-M2, these should be minimal aggregate summaries rather than empty
placeholders:

```go
type AgentRuntimeState struct {
    Status           string   `json:"status"`
    ActiveAgentIDs   []string `json:"active_agent_ids,omitempty"`
    PendingCount     int      `json:"pending_count,omitempty"`
}

type ReviewRuntimeState struct {
    Status           string   `json:"status"`
    OpenReviewIDs    []string `json:"open_review_ids,omitempty"`
    PendingCount     int      `json:"pending_count,omitempty"`
}

type BranchRuntimeState struct {
    Status             string   `json:"status"`
    ActiveBatchIDs     []string `json:"active_batch_ids,omitempty"`
    PromotableBranchIDs []string `json:"promotable_branch_ids,omitempty"`
}

type MemoryRuntimeState struct {
    Status                 string `json:"status"`
    WorkingSetCount        int    `json:"working_set_count,omitempty"`
    PendingQueueCount      int    `json:"pending_queue_count,omitempty"`
    TrustedCount           int    `json:"trusted_count,omitempty"`
    ContestedCount         int    `json:"contested_count,omitempty"`
    PendingPromotionCount  int    `json:"pending_promotion_count,omitempty"`
}

type RepoRuntimeState struct {
    Status             string `json:"status"`
    RepoHealth         string `json:"repo_health,omitempty"`
    CleanupProposalRef string `json:"cleanup_proposal_ref,omitempty"`
}

type ArtifactRuntimeState struct {
    Status               string `json:"status"`
    FamilyCount          int    `json:"family_count,omitempty"`
    CanonicalFamilyCount int    `json:"canonical_family_count,omitempty"`
    LatestPromotionRef   string `json:"latest_promotion_ref,omitempty"`
}

type ProjectOpsRuntimeState struct {
    Status                 string `json:"status"`
    LastDigestRef          string `json:"last_digest_ref,omitempty"`
    PendingResearchRuns    int    `json:"pending_research_runs,omitempty"`
    CleanupQueueCount      int    `json:"cleanup_queue_count,omitempty"`
    ActiveTickID           string `json:"active_tick_id,omitempty"`
    PendingDigestCount     int    `json:"pending_digest_count,omitempty"`
    WakeQueueCount         int    `json:"wake_queue_count,omitempty"`
}

type PermissionRuntimeState struct {
    Status                string `json:"status"`
    PendingRequestCount   int    `json:"pending_request_count,omitempty"`
}

type FeatureRuntimeState struct {
    Status             string   `json:"status"`
    DegradedFeatures   []string `json:"degraded_features,omitempty"`
}

type RollbackRuntimeState struct {
    Status             string `json:"status"`
    LastRestoreRef     string `json:"last_restore_ref,omitempty"`
}

type RemoteRuntimeState struct {
    Status               string   `json:"status"`
    ActiveBindingIDs     []string `json:"active_binding_ids,omitempty"`
    MachineLeaseState    string   `json:"machine_lease_state,omitempty"`
    ActiveOwnershipEpoch uint64   `json:"active_ownership_epoch,omitempty"`
}
```

### `SessionInspection`

```go
type SessionInspection struct {
    SessionID     string                  `json:"session_id"`
    Session       SessionIdentity         `json:"session"`
    Recap         ResumeRecap             `json:"recap,omitempty"`
    Lineage       []SessionLineageRecord  `json:"lineage,omitempty"`
    Provider      ProviderResolutionTrace `json:"provider,omitempty"`
    ProviderSource *ProviderSessionSource `json:"provider_source,omitempty"`
    TranscriptSource *TranscriptSourceEnvelope `json:"transcript_source,omitempty"`
    OperatorLogs  []SessionOperatorLogManifest `json:"operator_logs,omitempty"`
    Permission    PermissionDecisionTrace `json:"permission,omitempty"`
    SectionStatus map[string]string       `json:"section_status,omitempty"`
    Sections      map[string]any          `json:"sections,omitempty"`
}
```

### `ProjectInspection`

```go
type ProjectInspection struct {
    ProjectID        string                        `json:"project_id"`
    RegistryEntry    ProjectRegistryEntry          `json:"registry_entry"`
    InitState        string                        `json:"init_state"`
    ActiveSessionID  string                        `json:"active_session_id,omitempty"`
    DegradedFeatures []string                      `json:"degraded_features,omitempty"`
    SectionStatus    ProjectInspectionSectionStatus `json:"section_status"`
    Sections         ProjectInspectionSections     `json:"sections,omitempty"`
}
```

### `ProjectInspectionCore`

```go
type ProjectInspectionCore struct {
    ProjectID        string               `json:"project_id"`
    RegistryEntry    ProjectRegistryEntry `json:"registry_entry"`
    InitState        string               `json:"init_state"`
    ActiveSessionID  string               `json:"active_session_id,omitempty"`
    DegradedFeatures []string             `json:"degraded_features,omitempty"`
}
```

### `ProjectInspectionSections`

```go
type ProjectInspectionSections struct {
    ProjectOps map[string]any `json:"projectops,omitempty"`
    Memory     map[string]any `json:"memory,omitempty"`
    Branches   map[string]any `json:"branches,omitempty"`
    Reviews    map[string]any `json:"reviews,omitempty"`
    ActiveRuns map[string]any `json:"active_runs,omitempty"`
    Cleanup    map[string]any `json:"cleanup,omitempty"`
}
```

### `ProjectInspectionSectionStatus`

```go
type ProjectInspectionSectionStatus struct {
    ProjectOps string `json:"projectops,omitempty"`
    Memory     string `json:"memory,omitempty"`
    Branches   string `json:"branches,omitempty"`
    Reviews    string `json:"reviews,omitempty"`
    ActiveRuns string `json:"active_runs,omitempty"`
    Cleanup    string `json:"cleanup,omitempty"`
}
```

### `CompactResult`

```go
type CompactResult struct {
    SessionID          string `json:"session_id"`
    CompactionRecordID string `json:"compaction_record_id"`
    SummaryRef         string `json:"summary_ref,omitempty"`
    ResumeRecapRef     string `json:"resume_recap_ref,omitempty"`
    CompactedTurnCount int    `json:"compacted_turn_count,omitempty"`
    RawLogRetained     bool   `json:"raw_log_retained,omitempty"`
    LineageOK          bool   `json:"lineage_ok,omitempty"`
    DerivedViewsUpdated []string `json:"derived_views_updated,omitempty"`
    DeferredRepairs    []string `json:"deferred_repairs,omitempty"`
}
```

That keeps the bundle singular from day one without forcing advanced feature
implementation early.

## 3.2 `internal/events`

### `KernelEventEnvelope`

This is the only event object that should reach persistence.

```go
type KernelEventEnvelope struct {
    Seq             uint64                 `json:"seq"`
    EventName       string                 `json:"event_name"`
    Phase           string                 `json:"phase"`
    TerminalOutcome string                 `json:"terminal_outcome,omitempty"`
    ObjectKind      string                 `json:"object_kind"`
    ObjectID        string                 `json:"object_id"`
    ProjectID       string                 `json:"project_id"`
    SessionID       string                 `json:"session_id,omitempty"`
    Timestamp       string                 `json:"timestamp"`
    Payload         map[string]any         `json:"payload,omitempty"`
}
```

Companion enums/constants must be package-owned:

- `EventPhaseStart`
- `EventPhaseUpdate`
- `EventPhaseTerminal`
- `TerminalSucceeded`
- `TerminalFailed`
- `TerminalCancelled`
- `TerminalBlocked`
- `TerminalStaleConflict`

### `EventWriter`

```go
type EventWriter interface {
    Append(ctx context.Context, event KernelEventEnvelope) error
}
```

### `EventFactory`

Optional helper interface for deterministic event creation:

```go
type EventFactory interface {
    NewStart(eventName, objectKind, objectID string, payload map[string]any) KernelEventEnvelope
    NewUpdate(eventName, objectKind, objectID string, payload map[string]any) KernelEventEnvelope
    NewTerminal(eventName, objectKind, objectID, outcome string, payload map[string]any) KernelEventEnvelope
}
```

## 3.2.5 `internal/commands`

### `CommandSpec`

```go
type CommandSpec struct {
    Name             string   `json:"name"`
    SlashAliases     []string `json:"slash_aliases,omitempty"`
    Category         string   `json:"category,omitempty"`
    Summary          string   `json:"summary"`
    SupportsJSONHelp bool     `json:"supports_json_help,omitempty"`
}
```

### `CommandRegistry`

```go
type CommandRegistry interface {
    List() []CommandSpec
    Lookup(name string) (CommandSpec, bool)
}
```

### `HelpRenderer`

```go
type HelpRenderer interface {
    RenderHuman(scope string) (string, error)
    RenderJSON(scope string) (map[string]any, error)
}
```

### `HelpSurfaceReport`

```go
type HelpSurfaceReport struct {
    Scope    string        `json:"scope,omitempty"`
    Commands []CommandSpec `json:"commands,omitempty"`
}
```

### `PaletteEntry`

```go
type PaletteEntry struct {
    Command      string `json:"command"`
    Summary      string `json:"summary,omitempty"`
    Category     string `json:"category,omitempty"`
    ShortcutHint string `json:"shortcut_hint,omitempty"`
}
```

### `PaletteSurfaceReport`

```go
type PaletteSurfaceReport struct {
    Scope   string         `json:"scope,omitempty"`
    Entries []PaletteEntry `json:"entries,omitempty"`
}
```

### `ProfileResolutionTrace`

```go
type ProfileResolutionTrace struct {
    RequestedProfile string   `json:"requested_profile,omitempty"`
    ResolvedProfile  string   `json:"resolved_profile,omitempty"`
    Source           string   `json:"source,omitempty"`
    InheritedScopes  []string `json:"inherited_scopes,omitempty"`
    Warnings         []string `json:"warnings,omitempty"`
}
```

## 3.3 `internal/workspace`

### `WorkspaceFingerprint`

```go
type WorkspaceFingerprint struct {
    RootPath       string `json:"root_path"`
    WorkspaceHash  string `json:"workspace_hash"`
    RealPath       string `json:"real_path"`
}
```

### `WorkspaceBinding`

```go
type WorkspaceBinding struct {
    ProjectID      string               `json:"project_id"`
    Fingerprint    WorkspaceFingerprint `json:"fingerprint"`
    PMCLIRoot      string               `json:"pmcli_root"`
}
```

### `WorkspaceResolver`

```go
type WorkspaceResolver interface {
    Resolve(ctx context.Context, cwd string, explicitPath string) (WorkspaceBinding, error)
}
```

## 3.4 `internal/projects`

### `ProjectRegistryEntry`

```go
type ProjectRegistryEntry struct {
    ProjectID       string `json:"project_id"`
    WorkspaceRoot   string `json:"workspace_root"`
    WorkspaceHash   string `json:"workspace_hash"`
    DataDir         string `json:"data_dir"`
    LastAccessedAt  string `json:"last_accessed_at"`
    InitState       string `json:"init_state"`
    ActiveSessionID string `json:"active_session_id,omitempty"`
}
```

### `CurrentProjectPointer`

```go
type CurrentProjectPointer struct {
    ProjectID      string `json:"project_id"`
    WorkspaceRoot  string `json:"workspace_root"`
    UpdatedAt      string `json:"updated_at"`
}
```

### `ProjectResolutionTrace`

```go
type ProjectResolutionTrace struct {
    RequestedProjectID string               `json:"requested_project_id,omitempty"`
    RequestedCWD       string               `json:"requested_cwd,omitempty"`
    RequestedPath      string               `json:"requested_path,omitempty"`
    ResolutionSource   string               `json:"resolution_source,omitempty"`
    ResolutionStatus   string               `json:"resolution_status,omitempty"`
    ResolvedProjectID  string               `json:"resolved_project_id,omitempty"`
    ResolvedProject    *ProjectRegistryEntry `json:"resolved_project,omitempty"`
    CurrentPointer     *CurrentProjectPointer `json:"current_pointer,omitempty"`
    CandidateProjects  []ProjectRegistryEntry `json:"candidate_projects,omitempty"`
    Warnings           []string             `json:"warnings,omitempty"`
}
```

### `ProjectRegistryList`

```go
type ProjectRegistryList struct {
    Projects         []ProjectRegistryEntry `json:"projects,omitempty"`
    CurrentProjectID string                 `json:"current_project_id,omitempty"`
    TotalCount       int                    `json:"total_count,omitempty"`
}
```

### `ProjectStatus`

```go
type ProjectStatus struct {
    ProjectID        string               `json:"project_id"`
    RegistryEntry    ProjectRegistryEntry `json:"registry_entry"`
    InitState        string               `json:"init_state"`
    ActiveSessionID  string               `json:"active_session_id,omitempty"`
    OpenBranchCount  int                  `json:"open_branch_count,omitempty"`
    RepoHealth       string               `json:"repo_health,omitempty"`
    DegradedFeatures []string             `json:"degraded_features,omitempty"`
}
```

### `ProjectRegistry`

```go
type ProjectRegistry interface {
    Register(ctx context.Context, entry ProjectRegistryEntry) error
    Get(ctx context.Context, projectID string) (ProjectRegistryEntry, error)
    List(ctx context.Context) ([]ProjectRegistryEntry, error)
    Prune(ctx context.Context, apply bool) (PruneResult, error)
    SetCurrent(ctx context.Context, pointer CurrentProjectPointer) error
    GetCurrent(ctx context.Context) (CurrentProjectPointer, error)
}
```

### `ProjectLocator`

```go
type ProjectLocator interface {
    ResolveProject(ctx context.Context, cwd string, explicitProjectID string, explicitPath string) (ProjectRegistryEntry, error)
    ResolveTrace(ctx context.Context, cwd string, explicitProjectID string, explicitPath string) (ProjectResolutionTrace, error)
}
```

## 3.5 `internal/session`

### `SessionIdentity`

```go
type SessionIdentity struct {
    SessionID        string `json:"session_id"`
    ProjectID        string `json:"project_id"`
    WorkspaceRoot    string `json:"workspace_root"`
    Title            string `json:"title,omitempty"`
    Source           string `json:"source,omitempty"`
    SessionKind      string `json:"session_kind"`
    ParentSessionID  string `json:"parent_session_id,omitempty"`
    CompactedFrom    string `json:"compacted_from,omitempty"`
    CreatedAt        string `json:"created_at"`
    Active           bool   `json:"active,omitempty"`
}
```

### `SessionMeta`

```go
type SessionMeta struct {
    SessionID      string `json:"session_id"`
    ProjectID      string `json:"project_id"`
    Title          string `json:"title,omitempty"`
    Status         string `json:"status"`
    CreatedAt      string `json:"created_at"`
    UpdatedAt      string `json:"updated_at"`
}
```

`SessionMeta` is the minimal storage-oriented header.

`SessionIdentity` is the operator-facing canonical session object used by CLI
contracts and resume/inspect/list surfaces.

### `SessionOperatorLogManifest`

```go
type SessionOperatorLogManifest struct {
    SessionID            string `json:"session_id"`
    ProjectID            string `json:"project_id"`
    RequestSeq           int    `json:"request_seq"`
    RequestLogPath       string `json:"request_log_path,omitempty"`
    ResponseStreamPath   string `json:"response_stream_path,omitempty"`
    ResponseLogPath      string `json:"response_log_path,omitempty"`
    ToolResultsLogPath   string `json:"tool_results_log_path,omitempty"`
    TranscriptSource     string `json:"transcript_source,omitempty"`
    ConsistencyState     string `json:"consistency_state,omitempty"`
    RecordedAt           string `json:"recorded_at,omitempty"`
}
```

### `TranscriptLine`

The transcript must be typed, not raw free-form blobs.

```go
type TranscriptLine struct {
    Kind        string         `json:"kind"`
    Timestamp   string         `json:"timestamp"`
    TurnID      string         `json:"turn_id,omitempty"`
    Payload     map[string]any `json:"payload"`
}
```

Allowed initial `Kind` values:

- `control`
- `message`
- `tool_call`
- `tool_result`
- `summary_reference`

### `SessionStore`

```go
type SessionStore interface {
    Create(ctx context.Context, meta SessionMeta) error
    Load(ctx context.Context, sessionID string) (SessionMeta, error)
    ListByProject(ctx context.Context, projectID string) ([]SessionMeta, error)
    Delete(ctx context.Context, sessionID string) error
    AppendTranscript(ctx context.Context, sessionID string, line TranscriptLine) error
}
```

### `SessionSearchHit`

```go
type SessionSearchHit struct {
    SessionID    string `json:"session_id"`
    ProjectID    string `json:"project_id"`
    Title        string `json:"title,omitempty"`
    SourceTag    string `json:"source_tag,omitempty"`
    Snippet      string `json:"snippet,omitempty"`
    Score        int    `json:"score,omitempty"`
}
```

### `SessionSearchResult`

```go
type SessionSearchResult struct {
    SessionID      string `json:"session_id"`
    ProjectID      string `json:"project_id"`
    Title          string `json:"title,omitempty"`
    SourceTag      string `json:"source_tag,omitempty"`
    Snippet        string `json:"snippet,omitempty"`
    Score          int    `json:"score,omitempty"`
    LineageRootID  string `json:"lineage_root_id,omitempty"`
    LineageIndex   int    `json:"lineage_index,omitempty"`
}
```

### `SessionBrowseCandidate`

```go
type SessionBrowseCandidate struct {
    SessionID   string `json:"session_id"`
    ProjectID   string `json:"project_id"`
    Title       string `json:"title,omitempty"`
    Preview     string `json:"preview,omitempty"`
    RelativeAge string `json:"relative_age,omitempty"`
    Active      bool   `json:"active,omitempty"`
}
```

### `SessionBrowseResult`

```go
type SessionBrowseResult struct {
    Candidates []SessionBrowseCandidate `json:"candidates,omitempty"`
    Query      string                   `json:"query,omitempty"`
    TotalCount int                      `json:"total_count,omitempty"`
}
```

### `ResumeAmbiguityResult`

```go
type ResumeAmbiguityResult struct {
    Query      string                   `json:"query,omitempty"`
    Candidates []SessionBrowseCandidate `json:"candidates,omitempty"`
    Reason     string                   `json:"reason,omitempty"`
}
```

### `SessionSearchIndex`

```go
type SessionSearchIndex interface {
    Rebuild(ctx context.Context, projectID string) error
    Search(ctx context.Context, projectID string, query string) ([]SessionSearchResult, error)
}
```

### `DerivedSessionReadModel`

```go
type DerivedSessionReadModel struct {
    ModelID       string `json:"model_id"`
    ProjectID     string `json:"project_id"`
    ModelKind     string `json:"model_kind"`
    StoragePath   string `json:"storage_path"`
    Authoritative bool   `json:"authoritative"`
    Rebuildable   bool   `json:"rebuildable"`
    RefreshedAt   string `json:"refreshed_at,omitempty"`
}
```

### `GatewaySessionIndexEntry`

```go
type GatewaySessionIndexEntry struct {
    SessionID      string `json:"session_id"`
    ProjectID      string `json:"project_id"`
    BindingID      string `json:"binding_id,omitempty"`
    MachineID      string `json:"machine_id,omitempty"`
    RuntimeRef     string `json:"runtime_ref,omitempty"`
    ProjectionRef  string `json:"projection_ref,omitempty"`
    RefreshedAt    string `json:"refreshed_at,omitempty"`
    Authoritative  bool   `json:"authoritative"`
}
```

### `GatewayChannelDirectoryEntry`

```go
type GatewayChannelDirectoryEntry struct {
    ChannelID      string `json:"channel_id"`
    SurfaceKind    string `json:"surface_kind,omitempty"`
    ClientID       string `json:"client_id,omitempty"`
    MachineID      string `json:"machine_id,omitempty"`
    Reachability   string `json:"reachability,omitempty"`
    RefreshedAt    string `json:"refreshed_at,omitempty"`
    Authoritative  bool   `json:"authoritative"`
}
```

### `SessionTitleRecord`

```go
type SessionTitleRecord struct {
    SessionID     string `json:"session_id"`
    Title         string `json:"title,omitempty"`
    TitleSource   string `json:"title_source"`
    LineageRootID string `json:"lineage_root_id,omitempty"`
    LineageIndex  int    `json:"lineage_index,omitempty"`
}
```

### `ResumeRecap`

```go
type ResumeRecap struct {
    SessionID          string   `json:"session_id"`
    MessageCount       int      `json:"message_count,omitempty"`
    ShownExchangeCount int      `json:"shown_exchange_count,omitempty"`
    Truncated          bool     `json:"truncated,omitempty"`
    UserPreview        []string `json:"user_preview,omitempty"`
    AssistantPreview   []string `json:"assistant_preview,omitempty"`
    ToolSummary        []string `json:"tool_summary,omitempty"`
}
```

### `SessionLineageRecord`

```go
type SessionLineageRecord struct {
    ParentSessionID string `json:"parent_session_id"`
    ChildSessionID  string `json:"child_session_id"`
    Trigger         string `json:"trigger"`
    SummaryRef      string `json:"summary_ref,omitempty"`
    CreatedAt       string `json:"created_at"`
}
```

### `SessionExportResult`

```go
type SessionExportResult struct {
    SessionID      string `json:"session_id"`
    Format         string `json:"format"`
    OutputPath     string `json:"output_path"`
    Title          string `json:"title,omitempty"`
    LineageRootID  string `json:"lineage_root_id,omitempty"`
    LineageIndex   int    `json:"lineage_index,omitempty"`
}
```

### `SessionStats`

```go
type SessionStats struct {
    SessionCount    int64 `json:"session_count,omitempty"`
    TokenCount      int64 `json:"token_count,omitempty"`
    CompactionCount int64 `json:"compaction_count,omitempty"`
    ArchivedCount   int64 `json:"archived_count,omitempty"`
}
```

## 3.5.5 `internal/agents`

### `TaskPacket`

```go
type TaskPacket struct {
    RuntimeVersion            string   `json:"runtime_version"`
    MigrationVersion          string   `json:"migration_version"`
    SchemaVersion             string   `json:"schema_version"`
    ConformanceLine           string   `json:"conformance_line"`
    CanonicalPath             string   `json:"canonical_path"`
    RetentionPolicy           string   `json:"retention_policy,omitempty"`
    AtomicWritePolicy         string   `json:"atomic_write_policy,omitempty"`
    TaskPacketID              string   `json:"task_packet_id"`
    OwnerSessionID            string   `json:"owner_session_id"`
    OwnerScopeKind            string   `json:"owner_scope_kind"`
    AgentRole                 string   `json:"agent_role"`
    Objective                 string   `json:"objective"`
    ScopeSummary              string   `json:"scope_summary,omitempty"`
    AllowedPaths              []string `json:"allowed_paths,omitempty"`
    ForbiddenPaths            []string `json:"forbidden_paths,omitempty"`
    InputArtifactRefs         []string `json:"input_artifact_refs,omitempty"`
    ExpectedOutputs           []string `json:"expected_outputs,omitempty"`
    PermissionMode            string   `json:"permission_mode"`
    ToolBudget                int64    `json:"tool_budget,omitempty"`
    TokenBudget               int64    `json:"token_budget,omitempty"`
    ChangeEnvelopeRef         string   `json:"change_envelope_ref,omitempty"`
    RollbackSnapshotRequired  bool     `json:"rollback_snapshot_required,omitempty"`
    ReviewerBlindingRequired  bool     `json:"reviewer_blinding_required,omitempty"`
    SuccessCriteria           []string `json:"success_criteria,omitempty"`
    FailureContract           []string `json:"failure_contract,omitempty"`
}
```

### `AgentRuntimeRecord`

```go
type AgentRuntimeRecord struct {
    AgentID            string `json:"agent_id"`
    RuntimeIdentityRef string `json:"runtime_identity_ref,omitempty"`
    Role               string `json:"role"`
    OwnerSessionID     string `json:"owner_session_id"`
    WorkspaceBinding   string `json:"workspace_binding,omitempty"`
    Status             string `json:"status"`
    BudgetState        string `json:"budget_state,omitempty"`
    Scope              string `json:"scope,omitempty"`
    TaskPacketRef      string `json:"task_packet_ref"`
    LastStateChangeAt  string `json:"last_state_change_at,omitempty"`
    LatestOutputRef    string `json:"latest_output_ref,omitempty"`
}
```

### `AgentRuntimeIdentity`

```go
type AgentRuntimeIdentity struct {
    RuntimeID         string   `json:"runtime_id"`
    AgentID           string   `json:"agent_id"`
    RuntimeKind       string   `json:"runtime_kind"`
    RoleKind          string   `json:"role_kind"`
    RoleProfile       string   `json:"role_profile"`
    RunnerKind        string   `json:"runner_kind"`
    AuthorityScope    string   `json:"authority_scope"`
    LifecycleStatus   string   `json:"lifecycle_status"`
    SessionRef        string   `json:"session_ref,omitempty"`
    TaskPacketRef     string   `json:"task_packet_ref,omitempty"`
    OutputManifestRef string   `json:"output_manifest_ref,omitempty"`
    ProviderID        string   `json:"provider_id,omitempty"`
    Model             string   `json:"model,omitempty"`
    TraceRefs         []string `json:"trace_refs,omitempty"`
    ToolScope         []string `json:"tool_scope,omitempty"`
    EvidenceRefs      []string `json:"evidence_refs,omitempty"`
}
```

## 3.5.6 `internal/branches`

### `BranchBatchRecord`

```go
type BranchBatchRecord struct {
    BatchID           string `json:"batch_id"`
    Objective         string `json:"objective"`
    OwnerSessionID    string `json:"owner_session_id"`
    Status            string `json:"status"`
    MaxParallel       int    `json:"max_parallel,omitempty"`
    MaxTotal          int    `json:"max_total,omitempty"`
    WinnerBranchID    string `json:"winner_branch_id,omitempty"`
    ReviewGate        string `json:"review_gate,omitempty"`
    StaleBaseDetected bool   `json:"stale_base_detected,omitempty"`
}
```

### `BranchRunRecord`

```go
type BranchRunRecord struct {
    BranchID            string  `json:"branch_id"`
    BatchID             string  `json:"batch_id"`
    Hypothesis          string  `json:"hypothesis,omitempty"`
    WorkspaceBinding    string  `json:"workspace_binding,omitempty"`
    Status              string  `json:"status"`
    RetryCount          int     `json:"retry_count,omitempty"`
    EvaluationPacketID  string  `json:"evaluation_packet_id,omitempty"`
    ReviewID            string  `json:"review_id,omitempty"`
    Score               float64 `json:"score,omitempty"`
}
```

## 3.5.7 `internal/reviews`

### `ReviewRuntimeRecord`

```go
type ReviewRuntimeRecord struct {
    ReviewID      string   `json:"review_id"`
    TargetKind    string   `json:"target_kind"`
    TargetRefs    []string `json:"target_refs,omitempty"`
    ReviewerRole  string   `json:"reviewer_role,omitempty"`
    FreshThread   bool     `json:"fresh_thread,omitempty"`
    Blinded       bool     `json:"blinded,omitempty"`
    Status        string   `json:"status"`
    Verdict       string   `json:"verdict,omitempty"`
    CompareAgainst string  `json:"compare_against,omitempty"`
    TraceRef      string   `json:"trace_ref,omitempty"`
}
```

## 3.6 `internal/permissions`

### `PermissionMode`

This should be a typed string or small enum, not freeform user text.

Allowed initial values:

- `read-only`
- `workspace-write`
- `danger-full-access`

### `PermissionRequest`

```go
type PermissionRequest struct {
    RequestID      string `json:"request_id"`
    SessionID      string `json:"session_id"`
    TurnID         string `json:"turn_id"`
    ToolName       string `json:"tool_name"`
    TargetPath     string `json:"target_path,omitempty"`
    Reason         string `json:"reason"`
    RequestedAt    string `json:"requested_at"`
    Status         string `json:"status"`
}
```

### `PermissionDecision`

```go
type PermissionDecision struct {
    RequestID      string `json:"request_id"`
    Decision       string `json:"decision"`
    DecidedBy      string `json:"decided_by"`
    DecidedAt      string `json:"decided_at"`
    Scope          string `json:"scope,omitempty"`
}
```

### `PermissionPolicy`

```go
type PermissionPolicy interface {
    Evaluate(ctx context.Context, req PermissionRequest) (PermissionEvaluation, error)
}
```

### `PermissionEvaluation`

```go
type PermissionEvaluation struct {
    RequiresApproval bool   `json:"requires_approval"`
    Allowed          bool   `json:"allowed"`
    Reason           string `json:"reason,omitempty"`
}
```

### `PermissionDecisionTrace`

```go
type PermissionDecisionTrace struct {
    ToolName            string `json:"tool_name"`
    Action              string `json:"action"`
    RequestedPath       string `json:"requested_path,omitempty"`
    RequiredMode        string `json:"required_mode"`
    CurrentMode         string `json:"current_mode"`
    AllowlistMatch      bool   `json:"allowlist_match,omitempty"`
    WorkspaceBoundaryOK bool   `json:"workspace_boundary_ok,omitempty"`
    BranchBoundaryOK    bool   `json:"branch_boundary_ok,omitempty"`
    Destructive         bool   `json:"destructive,omitempty"`
    Approved            bool   `json:"approved,omitempty"`
    Reason              string `json:"reason,omitempty"`
}
```

## 3.7 `internal/tools`

### `ToolSpec`

```go
type ToolSpec struct {
    Name        string   `json:"name"`
    Kind        string   `json:"kind"`
    Capabilities []string `json:"capabilities,omitempty"`
    ReadOnly    bool     `json:"read_only"`
    Destructive bool     `json:"destructive"`
}
```

Allowed initial `Kind` values:

- `shell`
- `file`
- `web`
- `mcp`

### `ToolCall`

```go
type ToolCall struct {
    ToolName   string         `json:"tool_name"`
    Arguments  map[string]any `json:"arguments,omitempty"`
    SessionID  string         `json:"session_id,omitempty"`
    TurnID     string         `json:"turn_id,omitempty"`
}
```

### `ToolResult`

```go
type ToolResult struct {
    ToolName      string         `json:"tool_name"`
    Status        string         `json:"status"`
    ExitCode      int            `json:"exit_code,omitempty"`
    Output        string         `json:"output,omitempty"`
    Structured    map[string]any `json:"structured,omitempty"`
    ErrorMessage  string         `json:"error_message,omitempty"`
}
```

### `ToolExecutor`

```go
type ToolExecutor interface {
    Execute(ctx context.Context, call ToolCall) (ToolResult, error)
}
```

## 3.8 `internal/providers`

### `ProviderRoute`

```go
type ProviderRoute struct {
    ProviderID       string `json:"provider_id"`
    Model            string `json:"model"`
    ReasoningEffort  string `json:"reasoning_effort,omitempty"`
    SessionMode      string `json:"session_mode,omitempty"`
}
```

### `ProviderAuthStatus`

```go
type ProviderAuthStatus struct {
    ProviderID   string `json:"provider_id"`
    Ready        bool   `json:"ready"`
    Status       string `json:"status"`
    Hint         string `json:"hint,omitempty"`
}
```

### `ProviderStatus`

```go
type ProviderStatus struct {
    ProviderID          string   `json:"provider_id"`
    AuthStatus          string   `json:"auth_status,omitempty"`
    DegradedFeatures    []string `json:"degraded_features,omitempty"`
    SupportedModels     []string `json:"supported_models,omitempty"`
    CatalogSource       string   `json:"catalog_source,omitempty"`
    BaseURLSource       string   `json:"base_url_source,omitempty"`
}
```

### `ProviderStatusList`

```go
type ProviderStatusList struct {
    Providers  []ProviderStatus `json:"providers,omitempty"`
    TotalCount int              `json:"total_count,omitempty"`
}
```

### `ProviderResolution`

```go
type ProviderResolution struct {
    RequestedModel     string `json:"requested_model"`
    ResolvedModel      string `json:"resolved_model"`
    RequestedProvider  string `json:"requested_provider,omitempty"`
    ResolvedProvider   string `json:"resolved_provider"`
    ResolutionReason   string `json:"resolution_reason,omitempty"`
    AuthSource         string `json:"auth_source,omitempty"`
    BaseURLSource      string `json:"base_url_source,omitempty"`
    Degraded           bool   `json:"degraded,omitempty"`
}
```

### `ProviderResolver`

```go
type ProviderResolver interface {
    Resolve(ctx context.Context, input ProviderResolveInput) (ProviderRoute, error)
}
```

### `ProviderResolveInput`

```go
type ProviderResolveInput struct {
    ExplicitProvider string `json:"explicit_provider,omitempty"`
    ExplicitModel    string `json:"explicit_model,omitempty"`
    CommandKind      string `json:"command_kind,omitempty"`
}
```

### `ProviderResolutionTrace`

```go
type ProviderResolutionTrace struct {
    RequestedModel     string   `json:"requested_model"`
    RequestedProvider  string   `json:"requested_provider,omitempty"`
    ModelAliasApplied  string   `json:"model_alias_applied,omitempty"`
    RoutedByPrefix     bool     `json:"routed_by_prefix,omitempty"`
    ResolvedProvider   string   `json:"resolved_provider"`
    ResolvedModel      string   `json:"resolved_model"`
    AuthSource         string   `json:"auth_source,omitempty"`
    AuthShape          string   `json:"auth_shape,omitempty"`
    BaseURL            string   `json:"base_url,omitempty"`
    BaseURLSource      string   `json:"base_url_source,omitempty"`
    Degraded           bool     `json:"degraded,omitempty"`
    Warnings           []string `json:"warnings,omitempty"`
}
```

### `ProviderSessionSource`

```go
type ProviderSessionSource struct {
    SourceID               string `json:"source_id"`
    SessionID              string `json:"session_id"`
    ProviderID             string `json:"provider_id"`
    SourceFamily           string `json:"source_family"`
    SourceLocator          string `json:"source_locator,omitempty"`
    SourceAffinity         string `json:"source_affinity,omitempty"`
    VendorResumeSupported  bool   `json:"vendor_resume_supported,omitempty"`
    HappyAttachSupported   bool   `json:"happy_attach_supported,omitempty"`
    RecordedAt             string `json:"recorded_at,omitempty"`
}
```

## 3.9 `internal/config`

### `EffectiveConfig`

```go
type EffectiveConfig struct {
    DefaultProvider string            `json:"default_provider,omitempty"`
    DefaultModel    string            `json:"default_model,omitempty"`
    PermissionMode  string            `json:"permission_mode,omitempty"`
    Values          map[string]any    `json:"values,omitempty"`
}
```

### `EffectiveConfigReport`

```go
type EffectiveConfigReport struct {
    Scope       string         `json:"scope,omitempty"`
    Effective   EffectiveConfig `json:"effective"`
    ProjectID   string         `json:"project_id,omitempty"`
}
```

### `ConfigSourceMap`

```go
type ConfigSourceMap struct {
    Sources map[string]string `json:"sources"`
}
```

### `ConfigSourceReport`

```go
type ConfigSourceReport struct {
    Sources       map[string]string `json:"sources"`
    OverriddenKey []string          `json:"overridden_key,omitempty"`
}
```

### `ConfigValueResult`

```go
type ConfigValueResult struct {
    Key           string `json:"key"`
    Value         any    `json:"value,omitempty"`
    ValueSource   string `json:"value_source,omitempty"`
    ResolvedScope string `json:"resolved_scope,omitempty"`
}
```

### `ConfigStore`

```go
type ConfigStore interface {
    Load(ctx context.Context) (EffectiveConfig, ConfigSourceMap, error)
    Save(ctx context.Context, cfg EffectiveConfig) error
}
```

### `ProviderCatalogState`

```go
type ProviderCatalogState struct {
    ProviderID      string `json:"provider_id"`
    CatalogSource   string `json:"catalog_source"`
    CatalogVersion  string `json:"catalog_version"`
    Embedded        bool   `json:"embedded"`
    Refreshable     bool   `json:"refreshable"`
    LastRefreshedAt string `json:"last_refreshed_at,omitempty"`
    ModelCount      int    `json:"model_count,omitempty"`
}
```

### `ModelCatalogEntry`

```go
type ModelCatalogEntry struct {
    Alias            string `json:"alias,omitempty"`
    CanonicalModelID string `json:"canonical_model_id"`
    ProviderID       string `json:"provider_id"`
    Degraded         bool   `json:"degraded,omitempty"`
    DefaultSelected  bool   `json:"default_selected,omitempty"`
}
```

### `ModelCatalogList`

```go
type ModelCatalogList struct {
    Models            []ModelCatalogEntry `json:"models,omitempty"`
    CurrentModel      string              `json:"current_model,omitempty"`
    CurrentProvider   string              `json:"current_provider,omitempty"`
    DegradedProviders []string            `json:"degraded_providers,omitempty"`
}
```

### `ModelCurrentResult`

```go
type ModelCurrentResult struct {
    ProviderID    string `json:"provider_id"`
    Model         string `json:"model"`
    ResolvedScope string `json:"resolved_scope,omitempty"`
    ConfigSource  string `json:"config_source,omitempty"`
}
```

## 3.9.5 `internal/plugins`

### `PluginRecord`

```go
type PluginRecord struct {
    PluginID       string   `json:"plugin_id"`
    SourcePath     string   `json:"source_path"`
    Enabled        bool     `json:"enabled"`
    HookIDs        []string `json:"hook_ids,omitempty"`
    HealthStatus   string   `json:"health_status"`
    DisabledReason string   `json:"disabled_reason,omitempty"`
}
```

### `PluginListResult`

```go
type PluginListResult struct {
    Plugins       []PluginRecord `json:"plugins,omitempty"`
    DegradedCount int            `json:"degraded_count,omitempty"`
}
```

### `PluginInspectionResult`

```go
type PluginInspectionResult struct {
    Plugin           PluginRecord `json:"plugin"`
    HookSurface      []HookRecord `json:"hook_surface,omitempty"`
    ToolAdditions    []string     `json:"tool_additions,omitempty"`
    ConfigSource     string       `json:"config_source,omitempty"`
    LastHealthRecord string       `json:"last_health_record,omitempty"`
}
```

### `PluginValidationResult`

```go
type PluginValidationResult struct {
    TargetPluginID   string   `json:"target_plugin_id,omitempty"`
    ValidationStatus string   `json:"validation_status"`
    CheckedHookIDs   []string `json:"checked_hook_ids,omitempty"`
    FailureRefs      []string `json:"failure_refs,omitempty"`
}
```

### `HookRecord`

```go
type HookRecord struct {
    HookID        string `json:"hook_id"`
    OwnerPluginID string `json:"owner_plugin_id,omitempty"`
    Trigger       string `json:"trigger"`
    Enabled       bool   `json:"enabled"`
    FailurePolicy string `json:"failure_policy,omitempty"`
}
```

### `HookListResult`

```go
type HookListResult struct {
    Hooks         []HookRecord `json:"hooks,omitempty"`
    DegradedCount int          `json:"degraded_count,omitempty"`
}
```

### `HookInspectionResult`

```go
type HookInspectionResult struct {
    Hook             HookRecord `json:"hook"`
    SourcePath       string     `json:"source_path,omitempty"`
    TimeoutPolicy    string     `json:"timeout_policy,omitempty"`
    LastHealthRecord string     `json:"last_health_record,omitempty"`
}
```

### `HookTestResult`

```go
type HookTestResult struct {
    TargetHookID      string   `json:"target_hook_id,omitempty"`
    Trigger           string   `json:"trigger,omitempty"`
    TestMode          string   `json:"test_mode,omitempty"`
    ValidationStatus  string   `json:"validation_status"`
    ExecutedHookIDs   []string `json:"executed_hook_ids,omitempty"`
    FailureRefs       []string `json:"failure_refs,omitempty"`
}
```

### `PluginRegistry`

```go
type PluginRegistry interface {
    List(ctx context.Context) (PluginListResult, error)
    Inspect(ctx context.Context, pluginID string) (PluginInspectionResult, error)
    Validate(ctx context.Context, pluginID string) (PluginValidationResult, error)
}
```

### `HookRegistry`

```go
type HookRegistry interface {
    List(ctx context.Context) (HookListResult, error)
    Inspect(ctx context.Context, hookID string) (HookInspectionResult, error)
    Test(ctx context.Context, hookID string) (HookTestResult, error)
}
```

## 3.10 `internal/memory`

### `MemoryRecord`

```go
type MemoryRecord struct {
    RecordID        string   `json:"record_id"`
    ProjectID       string   `json:"project_id"`
    Namespace       string   `json:"namespace,omitempty"`
    Scope           string   `json:"scope,omitempty"`
    Kind            string   `json:"kind"`
    Surface         string   `json:"surface,omitempty"`
    Title           string   `json:"title,omitempty"`
    Summary         string   `json:"summary,omitempty"`
    Body            string   `json:"body,omitempty"`
    Provenance      []string `json:"provenance,omitempty"`
    SourceSessionID string   `json:"source_session_id,omitempty"`
    SourceArtifacts []string `json:"source_artifacts,omitempty"`
    SupportRefs     []string `json:"support_refs,omitempty"`
    Status          string   `json:"status"`
    Confidence      float64  `json:"confidence,omitempty"`
    ValidFrom       string   `json:"valid_from,omitempty"`
    ValidTo         string   `json:"valid_to,omitempty"`
    Supersedes      []string `json:"supersedes,omitempty"`
    SupersededBy    string   `json:"superseded_by,omitempty"`
    UsageCount      int      `json:"usage_count,omitempty"`
}
```

### `MemoryQueryResult`

```go
type MemoryQueryResult struct {
    QueryID        string         `json:"query_id,omitempty"`
    Scope          string         `json:"scope,omitempty"`
    Route          string         `json:"route,omitempty"`
    BudgetUsed     int            `json:"budget_used,omitempty"`
    MatchedRecords []MemoryRecord `json:"matched_records,omitempty"`
    DegradedReasons []string      `json:"degraded_reasons,omitempty"`
}
```

### `MemoryExplainRecord`

```go
type MemoryExplainRecord struct {
    RecordID         string   `json:"record_id"`
    Status           string   `json:"status"`
    RecallReason     string   `json:"recall_reason,omitempty"`
    MatchRoute       string   `json:"match_route,omitempty"`
    SourceRefs       []string `json:"source_refs,omitempty"`
    SupportSpans     []string `json:"support_spans,omitempty"`
    PointerHydration []string `json:"pointer_hydration,omitempty"`
    Confidence       float64  `json:"confidence,omitempty"`
    SupersededBy     string   `json:"superseded_by,omitempty"`
    ContestedBy      string   `json:"contested_by,omitempty"`
}
```

## 3.10.1 `internal/skills`

### `SkillManifest`

```go
type SkillManifest struct {
    SkillID                  string   `json:"skill_id"`
    Version                  string   `json:"version,omitempty"`
    StageID                  string   `json:"stage_id,omitempty"`
    StageClass               string   `json:"stage_class,omitempty"`
    Inputs                   []string `json:"inputs,omitempty"`
    Outputs                  []string `json:"outputs,omitempty"`
    ArtifactFamily           string   `json:"artifact_family,omitempty"`
    RequiredAgents           []string `json:"required_agents,omitempty"`
    ReviewerPolicy           string   `json:"reviewer_policy,omitempty"`
    CitationRules            []string `json:"citation_rules,omitempty"`
    RecoveryRules            []string `json:"recovery_rules,omitempty"`
    ExecutionPreference      string   `json:"execution_preference,omitempty"`
}
```

### `SkillAvailabilityRecord`

```go
type SkillAvailabilityRecord struct {
    SkillID             string   `json:"skill_id"`
    ManifestVersion     string   `json:"manifest_version,omitempty"`
    InstallOrigin       string   `json:"install_origin,omitempty"`
    Enabled             bool     `json:"enabled"`
    DisabledReason      string   `json:"disabled_reason,omitempty"`
    DependencyStatus    []string `json:"dependency_status,omitempty"`
    StageCompatibility  []string `json:"stage_compatibility,omitempty"`
    ValidationStatus    string   `json:"validation_status,omitempty"`
}
```

### `SkillRegistryEntry`

```go
type SkillRegistryEntry struct {
    Manifest     SkillManifest           `json:"manifest"`
    Availability SkillAvailabilityRecord `json:"availability"`
    SourcePath   string                  `json:"source_path,omitempty"`
}
```

### `SkillListResult`

```go
type SkillListResult struct {
    Skills          []SkillRegistryEntry `json:"skills,omitempty"`
    StageFilter     string               `json:"stage_filter,omitempty"`
    SourceFilter    string               `json:"source_filter,omitempty"`
    IncludeDisabled bool                 `json:"include_disabled,omitempty"`
}
```

### `SkillInspectResult`

```go
type SkillInspectResult struct {
    Entry             SkillRegistryEntry `json:"entry"`
    DependencyHealth  []string           `json:"dependency_health,omitempty"`
    StageGraphRefs    []string           `json:"stage_graph_refs,omitempty"`
}
```

### `SkillPathRecord`

```go
type SkillPathRecord struct {
    SearchPath    string   `json:"search_path"`
    SourceKind    string   `json:"source_kind,omitempty"`
    WinningSkills []string `json:"winning_skills,omitempty"`
}
```

### `SkillPathsResult`

```go
type SkillPathsResult struct {
    Paths []SkillPathRecord `json:"paths,omitempty"`
}
```

### `SkillValidationResult`

```go
type SkillValidationResult struct {
    Scope            string   `json:"scope,omitempty"`
    ValidationStatus string   `json:"validation_status"`
    CheckedSkillIDs  []string `json:"checked_skill_ids,omitempty"`
    FailureRefs      []string `json:"failure_refs,omitempty"`
}
```

### `SkillRegistry`

```go
type SkillRegistry interface {
    List(ctx context.Context, stageID string, includeDisabled bool, source string) (SkillListResult, error)
    Inspect(ctx context.Context, skillID string) (SkillInspectResult, error)
    Paths(ctx context.Context) (SkillPathsResult, error)
    Validate(ctx context.Context, skillID string) (SkillValidationResult, error)
}
```

### `MemoryStatusReport`

```go
type MemoryStatusReport struct {
    WorkingSetCount    int      `json:"working_set_count,omitempty"`
    DurableCount       int      `json:"durable_count,omitempty"`
    PromotionQueueSize int      `json:"promotion_queue_size,omitempty"`
    DegradedReasons    []string `json:"degraded_reasons,omitempty"`
}
```

### `ArtifactListResult`

```go
type ArtifactListResult struct {
    Families       []ArtifactFamily `json:"families,omitempty"`
    CanonicalCount int              `json:"canonical_count,omitempty"`
    ArchiveCount   int              `json:"archive_count,omitempty"`
}
```

### `ArtifactInspectionResult`

```go
type ArtifactInspectionResult struct {
    FamilyID          string   `json:"family_id,omitempty"`
    CanonicalPath     string   `json:"canonical_path,omitempty"`
    LineageRefs       []string `json:"lineage_refs,omitempty"`
    PromotionHistory  []string `json:"promotion_history,omitempty"`
    ReviewLinks       []string `json:"review_links,omitempty"`
}
```

### `WorkingMemoryRecord`

```go
type WorkingMemoryRecord struct {
    RecordID       string   `json:"record_id"`
    SessionID      string   `json:"session_id"`
    ProjectID      string   `json:"project_id"`
    Scope          string   `json:"scope,omitempty"`
    Kind           string   `json:"kind"`
    Text           string   `json:"text,omitempty"`
    SupportRefs    []string `json:"support_refs,omitempty"`
    Pinned         bool     `json:"pinned,omitempty"`
    EvictionClass  string   `json:"eviction_class,omitempty"`
    CreatedAt      string   `json:"created_at,omitempty"`
}
```

## 3.10.1 `internal/projectops`

### `ProjectOpsTick`

```go
type ProjectOpsTick struct {
    TickID          string   `json:"tick_id"`
    Trigger         string   `json:"trigger"`
    ProjectID       string   `json:"project_id"`
    SessionID       string   `json:"session_id,omitempty"`
    BranchID        string   `json:"branch_id,omitempty"`
    ReviewID        string   `json:"review_id,omitempty"`
    RunID           string   `json:"run_id,omitempty"`
    StartedAt       string   `json:"started_at"`
    FinishedAt      string   `json:"finished_at,omitempty"`
    State           string   `json:"state,omitempty"`
    DigestCandidateID string `json:"digest_candidate_id,omitempty"`
    ActionsTaken    []string `json:"actions_taken,omitempty"`
    DegradedReasons []string `json:"degraded_reasons,omitempty"`
}
```

### `ProgressDigestCandidate`

```go
type ProgressDigestCandidate struct {
    CandidateID            string   `json:"candidate_id"`
    ProjectID              string   `json:"project_id"`
    TickID                 string   `json:"tick_id,omitempty"`
    Scope                  string   `json:"scope,omitempty"`
    SummaryText            string   `json:"summary_text,omitempty"`
    SupportingEventIDs     []string `json:"supporting_event_ids,omitempty"`
    SupportingArtifactIDs  []string `json:"supporting_artifact_ids,omitempty"`
    SupportingSessionSpans []string `json:"supporting_session_spans,omitempty"`
    PromotionStatus        string   `json:"promotion_status"`
    RejectionReason        string   `json:"rejection_reason,omitempty"`
    Confidence             float64  `json:"confidence,omitempty"`
}
```

### `RepoCleanupProposal`

```go
type RepoCleanupProposal struct {
    ProposalID        string   `json:"proposal_id"`
    ProjectID         string   `json:"project_id"`
    DetectedDuplicates []string `json:"detected_duplicates,omitempty"`
    StalePaths        []string `json:"stale_paths,omitempty"`
    OrphanWorktrees    []string `json:"orphan_worktrees,omitempty"`
    SupersededArtifacts []string `json:"superseded_artifacts,omitempty"`
    SafeActions       []string `json:"safe_actions,omitempty"`
    RiskyActions      []string `json:"risky_actions,omitempty"`
    RequiresHumanGate bool     `json:"requires_human_gate,omitempty"`
}
```

### `PermissionPendingList`

```go
type PermissionPendingList struct {
    Requests []PermissionRequest `json:"requests,omitempty"`
    TotalCount int               `json:"total_count,omitempty"`
}
```

### `PermissionHistoryResult`

```go
type PermissionHistoryResult struct {
    Decisions []PermissionDecisionTrace `json:"decisions,omitempty"`
    TotalCount int                     `json:"total_count,omitempty"`
}
```

### `ConformanceResult`

```go
type ConformanceResult struct {
    Scope            string   `json:"scope,omitempty"`
    PassedFamilies   []string `json:"passed_families,omitempty"`
    FailedFamilies   []string `json:"failed_families,omitempty"`
    FailureRefs      []string `json:"failure_refs,omitempty"`
}
```

### `PruneResult`

```go
type PruneResult struct {
    DryRun            bool     `json:"dry_run,omitempty"`
    CandidateRefs     []string `json:"candidate_refs,omitempty"`
    PrunedRefs        []string `json:"pruned_refs,omitempty"`
    BlockingConflicts []string `json:"blocking_conflicts,omitempty"`
}
```

### `ExperimentSupervisorLease`

```go
type ExperimentSupervisorLease struct {
    LeaseID          string `json:"lease_id"`
    RunID            string `json:"run_id"`
    OwnerAgentID     string `json:"owner_agent_id"`
    State            string `json:"state"`
    ClaimedAt        string `json:"claimed_at,omitempty"`
    HeartbeatAt      string `json:"heartbeat_at,omitempty"`
    StaleAfterSec    int    `json:"stale_after_sec,omitempty"`
    PendingWakeCount int    `json:"pending_wake_count,omitempty"`
    LastSummary      string `json:"last_summary,omitempty"`
    ReclaimedFrom    string `json:"reclaimed_from,omitempty"`
}
```

### `WakeEvent`

```go
type WakeEvent struct {
    WakeID          string `json:"wake_id"`
    RunID           string `json:"run_id"`
    OwnerAgentID    string `json:"owner_agent_id"`
    Kind            string `json:"kind"`
    Urgency         string `json:"urgency"`
    RequiresMainSystem bool `json:"requires_main_system,omitempty"`
    Summary         string `json:"summary,omitempty"`
    Details         string `json:"details,omitempty"`
    LeaseID         string `json:"lease_id,omitempty"`
    State           string `json:"state,omitempty"`
    EscalationReason string `json:"escalation_reason,omitempty"`
}
```

### `ChangeEnvelope`

```go
type ChangeEnvelope struct {
    EnvelopeID      string   `json:"envelope_id"`
    Objective       string   `json:"objective"`
    AllowedPaths    []string `json:"allowed_paths,omitempty"`
    ForbiddenPaths  []string `json:"forbidden_paths,omitempty"`
    EvaluationContract string `json:"evaluation_contract,omitempty"`
    RollbackHint    string   `json:"rollback_hint,omitempty"`
}
```

## 3.10.2 `internal/research`

### `ResearchStageExecution`

```go
type ResearchStageExecution struct {
    RunID               string   `json:"run_id"`
    StageID             string   `json:"stage_id"`
    StageClass          string   `json:"stage_class"`
    SkillID             string   `json:"skill_id,omitempty"`
    State               string   `json:"state,omitempty"`
    InputArtifacts      []string `json:"input_artifacts,omitempty"`
    OutputArtifacts     []string `json:"output_artifacts,omitempty"`
    AssignedAgentRole   string   `json:"assigned_agent_role,omitempty"`
    TaskPacketRef       string   `json:"task_packet_ref"`
    ChangeEnvelopeRef   string   `json:"change_envelope_ref,omitempty"`
    PermissionMode      string   `json:"permission_mode,omitempty"`
    ToolBudget          string   `json:"tool_budget,omitempty"`
    TokenBudget         string   `json:"token_budget,omitempty"`
    RollbackSnapshotRef string   `json:"rollback_snapshot_ref,omitempty"`
    RetryPolicy         string   `json:"retry_policy,omitempty"`
    SuccessCriteria     []string `json:"success_criteria,omitempty"`
    RepairEdges         []string `json:"repair_edges,omitempty"`
    GateRef             string   `json:"gate_ref,omitempty"`
    PivotedFromStageID  string   `json:"pivoted_from_stage_id,omitempty"`
}
```

### `StageExecutionMapEntry`

```go
type StageExecutionMapEntry struct {
    StageID       string   `json:"stage_id"`
    StageClass    string   `json:"stage_class"`
    NextOnSuccess []string `json:"next_on_success,omitempty"`
    NextOnRepair  []string `json:"next_on_repair,omitempty"`
    NextOnPivot   []string `json:"next_on_pivot,omitempty"`
}
```

### `StageExecutionMap`

```go
type StageExecutionMap struct {
    Entries []StageExecutionMapEntry `json:"entries,omitempty"`
}
```

## 3.11 `internal/telemetry`

### `UsageSummary`

```go
type UsageSummary struct {
    Scope             string `json:"scope"`
    InputTokens       int64  `json:"input_tokens,omitempty"`
    OutputTokens      int64  `json:"output_tokens,omitempty"`
    ToolCalls         int64  `json:"tool_calls,omitempty"`
    EstimatedCost     string `json:"estimated_cost,omitempty"`
    AccountingQuality string `json:"accounting_quality,omitempty"`
}
```

### `StatsSummary`

```go
type StatsSummary struct {
    Scope             string `json:"scope"`
    SessionCount      int64  `json:"session_count,omitempty"`
    ActiveSessionCount int64 `json:"active_session_count,omitempty"`
    ProjectCount      int64  `json:"project_count,omitempty"`
}
```

### `TelemetryService`

```go
type Service interface {
    Usage(ctx context.Context, scope string) (UsageSummary, error)
    Cost(ctx context.Context, scope string) (UsageSummary, error)
    Stats(ctx context.Context, scope string) (StatsSummary, error)
}
```

## 3.10.5 `internal/mcp`

### `MCPServerRecord`

```go
type MCPServerRecord struct {
    ServerID        string   `json:"server_id"`
    TransportType   string   `json:"transport_type"`
    ToolCount       int      `json:"tool_count,omitempty"`
    AuthStatus      string   `json:"auth_status,omitempty"`
    DisabledTools   []string `json:"disabled_tools,omitempty"`
    TimeoutPolicy   string   `json:"timeout_policy,omitempty"`
    LastFailure     string   `json:"last_failure,omitempty"`
}
```

### `MCPListResult`

```go
type MCPListResult struct {
    Servers       []MCPServerRecord `json:"servers,omitempty"`
    DegradedCount int               `json:"degraded_count,omitempty"`
}
```

### `MCPInspectionResult`

```go
type MCPInspectionResult struct {
    Server       MCPServerRecord `json:"server"`
    ToolSurface  []string        `json:"tool_surface,omitempty"`
    ConfigSource string          `json:"config_source,omitempty"`
}
```

### `MCPTestResult`

```go
type MCPTestResult struct {
    TargetServerID   string   `json:"target_server_id,omitempty"`
    ValidationStatus string   `json:"validation_status"`
    CheckedTools     []string `json:"checked_tools,omitempty"`
    FailureRefs      []string `json:"failure_refs,omitempty"`
}
```

### `MCPRefreshResult`

```go
type MCPRefreshResult struct {
    RefreshedServerIDs []string `json:"refreshed_server_ids,omitempty"`
    ChangedServerIDs   []string `json:"changed_server_ids,omitempty"`
    DegradedServerIDs  []string `json:"degraded_server_ids,omitempty"`
}
```

### `MCPRegistry`

```go
type MCPRegistry interface {
    List(ctx context.Context) (MCPListResult, error)
    Inspect(ctx context.Context, serverID string) (MCPInspectionResult, error)
    Test(ctx context.Context, serverID string) (MCPTestResult, error)
    Refresh(ctx context.Context) (MCPRefreshResult, error)
}
```

## 3.10.6 `internal/setup`

### `SetupStatusReport`

```go
type SetupStatusReport struct {
    SchemaVersion   string   `json:"schema_version,omitempty"`
    DataDirectories []string `json:"data_directories,omitempty"`
    LastMigration   string   `json:"last_migration,omitempty"`
    InstallState    string   `json:"install_state,omitempty"`
}
```

### `MigrateCheckReport`

```go
type MigrateCheckReport struct {
    PendingMigrations []string `json:"pending_migrations,omitempty"`
    Compatible        bool     `json:"compatible"`
    BlockingReason    string   `json:"blocking_reason,omitempty"`
}
```

### `RepairHintsReport`

```go
type RepairHintsReport struct {
    HintsByComponent map[string][]string `json:"hints_by_component,omitempty"`
}
```

### `InstallRoutesReport`

```go
type InstallRoutesReport struct {
    ProvidersPath   string `json:"providers_path,omitempty"`
    SkillsPath      string `json:"skills_path,omitempty"`
    MCPPath         string `json:"mcp_path,omitempty"`
    PluginsPath     string `json:"plugins_path,omitempty"`
    HooksPath       string `json:"hooks_path,omitempty"`
    RemoteAssetsPath string `json:"remote_assets_path,omitempty"`
    PMCLIRootPath   string `json:"pmcli_root_path,omitempty"`
}
```

### `SetupService`

```go
type SetupService interface {
    Status(ctx context.Context) (SetupStatusReport, error)
    MigrateCheck(ctx context.Context) (MigrateCheckReport, error)
    RepairHints(ctx context.Context) (RepairHintsReport, error)
    InstallRoutes(ctx context.Context) (InstallRoutesReport, error)
}
```

## 3.10.7 `internal/remote`

These are reserved advanced-surface types. They should remain outside M0-M2
execution ownership but are frozen early so later remote implementation does
not invent competing payloads.

### `RemoteMachine`

```go
type RemoteMachine struct {
    MachineID         string `json:"machine_id"`
    PairingState      string `json:"pairing_state"`
    LeaseState        string `json:"lease_state,omitempty"`
    CapabilityVersion string `json:"capability_version,omitempty"`
    MachineMetadataRef string `json:"machine_metadata_ref,omitempty"`
    DaemonStateRef    string `json:"daemon_state_ref,omitempty"`
}
```

### `RemotePairResult`

```go
type RemotePairResult struct {
    Machine          RemoteMachine           `json:"machine"`
    CapabilityMatrix *RemoteCapabilityMatrix `json:"capability_matrix,omitempty"`
    PairTicket       *PairTicket             `json:"pair_ticket,omitempty"`
    RemoteEndpoint   string                  `json:"remote_endpoint,omitempty"`
}
```

### `MachineIdentity`

```go
type MachineIdentity struct {
    MachineID             string `json:"machine_id"`
    MachineLabel          string `json:"machine_label,omitempty"`
    DeviceClass           string `json:"device_class,omitempty"`
    PublicKeyFingerprint  string `json:"public_key_fingerprint,omitempty"`
    HostInstanceID        string `json:"host_instance_id,omitempty"`
    RuntimeVersion        string `json:"runtime_version,omitempty"`
    CreatedAt             string `json:"created_at,omitempty"`
    RotatedAt             string `json:"rotated_at,omitempty"`
    RevokedAt             string `json:"revoked_at,omitempty"`
}
```

### `RemoteClientIdentity`

```go
type RemoteClientIdentity struct {
    ClientID      string `json:"client_id"`
    MachineID     string `json:"machine_id"`
    SurfaceKind   string `json:"surface_kind,omitempty"`
    UserLabel     string `json:"user_label,omitempty"`
    AuthSubject   string `json:"auth_subject,omitempty"`
    RegisteredAt  string `json:"registered_at,omitempty"`
    LastSeenAt    string `json:"last_seen_at,omitempty"`
}
```

### `PairTicket`

```go
type PairTicket struct {
    PairTicketID   string   `json:"pair_ticket_id"`
    MachineID      string   `json:"machine_id"`
    ChallengeNonce string   `json:"challenge_nonce,omitempty"`
    IssuedAt       string   `json:"issued_at,omitempty"`
    ExpiresAt      string   `json:"expires_at,omitempty"`
    Capabilities   []string `json:"capabilities,omitempty"`
    RelayEndpoint  string   `json:"relay_endpoint,omitempty"`
}
```

### `RemoteMachineMetadata`

```go
type RemoteMachineMetadata struct {
    MachineID   string `json:"machine_id"`
    Hostname    string `json:"hostname"`
    Platform    string `json:"platform"`
    CLIVersion  string `json:"cli_version"`
    HomeDir     string `json:"home_dir,omitempty"`
    StateRoot   string `json:"state_root,omitempty"`
    HostVariant string `json:"host_variant,omitempty"`
    UpdatedAt   string `json:"updated_at,omitempty"`
}
```

### `RemoteDaemonState`

```go
type RemoteDaemonState struct {
    MachineID             string   `json:"machine_id"`
    DaemonStatus          string   `json:"daemon_status"`
    DaemonPID             int      `json:"daemon_pid,omitempty"`
    ControlPort           int      `json:"control_port,omitempty"`
    StartedAt             string   `json:"started_at,omitempty"`
    LastHeartbeatAt       string   `json:"last_heartbeat_at,omitempty"`
    StartedWithCLIVersion string   `json:"started_with_cli_version,omitempty"`
    TrackedSessionIDs     []string `json:"tracked_session_ids,omitempty"`
    StateReason           string   `json:"state_reason,omitempty"`
}
```

### `RemoteCapabilityMatrix`

```go
type RemoteCapabilityMatrix struct {
    RuntimeVersion              string   `json:"runtime_version,omitempty"`
    ProviderBackends            []string `json:"provider_backends,omitempty"`
    ResumeSupported             bool     `json:"resume_supported,omitempty"`
    AttachSupported             bool     `json:"attach_supported,omitempty"`
    HandoffSupported            bool     `json:"handoff_supported,omitempty"`
    TerminalSupported           bool     `json:"terminal_supported,omitempty"`
    WorktreeSupported           bool     `json:"worktree_supported,omitempty"`
    ProjectOpsBackgroundSupported bool   `json:"projectops_background_supported,omitempty"`
    QueuedResearchSupported     bool     `json:"queued_research_supported,omitempty"`
    LastCheckedAt               string   `json:"last_checked_at,omitempty"`
}
```

### `LocalControlCapability`

```go
type LocalControlCapability struct {
    Supported      bool   `json:"supported"`
    Topology       string `json:"topology,omitempty"`
    AttachStrategy string `json:"attach_strategy,omitempty"`
}
```

### `ExistingSessionAutomationEligibility`

```go
type ExistingSessionAutomationEligibility struct {
    Eligible   bool   `json:"eligible"`
    AgentID    string `json:"agent_id,omitempty"`
    Strategy   string `json:"strategy,omitempty"`
    ReasonCode string `json:"reason_code,omitempty"`
}
```

### `RemoteCapabilitySet`

```go
type RemoteCapabilitySet struct {
    FollowSession      bool `json:"follow_session,omitempty"`
    AttachProvider     bool `json:"attach_provider,omitempty"`
    AttachTerminal     bool `json:"attach_terminal,omitempty"`
    ApprovePermission  bool `json:"approve_permission,omitempty"`
    RequestHandoff     bool `json:"request_handoff,omitempty"`
    RequestTakeover    bool `json:"request_takeover,omitempty"`
    Notify             bool `json:"notify,omitempty"`
    InspectProjectOps  bool `json:"inspect_projectops,omitempty"`
    InspectBranches    bool `json:"inspect_branches,omitempty"`
    InspectMemory      bool `json:"inspect_memory,omitempty"`
}
```

### `RemoteSessionBinding`

```go
type RemoteSessionBinding struct {
    BindingID              string                   `json:"binding_id"`
    SessionID              string                   `json:"session_id"`
    ProjectID              string                   `json:"project_id"`
    MachineID              string                   `json:"machine_id"`
    TransportMode          string                   `json:"transport_mode"`
    RuntimeDescriptorRef   string                   `json:"runtime_descriptor_ref,omitempty"`
    ProviderSessionSourceRef string                 `json:"provider_session_source_ref,omitempty"`
    ProjectionRef          string                   `json:"projection_ref,omitempty"`
    ControlOwner           *RemoteControlOwner      `json:"control_owner,omitempty"`
    RemoteStatus           string                   `json:"remote_status,omitempty"`
    AttachModes            []string                 `json:"attach_modes,omitempty"`
    RemoteClients          []RemoteClientIdentity   `json:"remote_clients,omitempty"`
    OwnershipEpoch         uint64                   `json:"ownership_epoch,omitempty"`
    AttachedFromTerminal   bool                     `json:"attached_from_terminal,omitempty"`
    AttachedFromRemote     bool                     `json:"attached_from_remote,omitempty"`
}
```

### `SessionRuntimeDescriptor`

```go
type SessionRuntimeDescriptor struct {
    SessionID                 string   `json:"session_id"`
    RuntimeFamily             string   `json:"runtime_family"`
    ProviderSessionSourceRef  string   `json:"provider_session_source_ref,omitempty"`
    SourceKind                string   `json:"source_kind"`
    SourceAffinity            string   `json:"source_affinity,omitempty"`
    TransportMode             string   `json:"transport_mode"`
    TranscriptMode            string   `json:"transcript_mode"`
    LocalControlCapability    *LocalControlCapability `json:"local_control_capability,omitempty"`
    ExistingSessionAutomation *ExistingSessionAutomationEligibility `json:"existing_session_automation,omitempty"`
    AttachModes               []string `json:"attach_modes,omitempty"`
    HandoffModes              []string `json:"handoff_modes,omitempty"`
    TakeoverModes             []string `json:"takeover_modes,omitempty"`
    SupportsInflightSteer     bool     `json:"supports_inflight_steer,omitempty"`
    SupportsLatestTurnRollback bool    `json:"supports_latest_turn_rollback,omitempty"`
    SupportsLocalRemoteSwitch bool     `json:"supports_local_remote_switch,omitempty"`
    SessionEnvelopeProjectionLine string `json:"session_envelope_projection_line,omitempty"`
}
```

### `SessionEnvelopeProjectionV1`

```go
type SessionEnvelopeProjectionV1 struct {
    ProjectionID           string   `json:"projection_id"`
    SchemaVersion          string   `json:"schema_version"`
    SessionID              string   `json:"session_id"`
    ProjectID              string   `json:"project_id"`
    RuntimeDescriptorRef   string   `json:"runtime_descriptor_ref"`
    TranscriptSourceRef    string   `json:"transcript_source_ref,omitempty"`
    Title                  string   `json:"title,omitempty"`
    LatestTurnSummary      string   `json:"latest_turn_summary,omitempty"`
    ActivePhase            string   `json:"active_phase,omitempty"`
    PermissionPendingCount int      `json:"permission_pending_count,omitempty"`
    ControlOwner           *RemoteControlOwner `json:"control_owner,omitempty"`
    FeatureAdvertisements  []RemoteFeatureAdvertisement `json:"feature_advertisements,omitempty"`
    MemoryStatusSummary    string   `json:"memory_status_summary,omitempty"`
    BranchStatusSummary    string   `json:"branch_status_summary,omitempty"`
    ReviewStatusSummary    string   `json:"review_status_summary,omitempty"`
    SeqLowWatermark        uint64   `json:"seq_low_watermark,omitempty"`
    SeqHighWatermark       uint64   `json:"seq_high_watermark,omitempty"`
    UpdatedAt              string   `json:"updated_at,omitempty"`
}
```

### `TranscriptSourceEnvelope`

```go
type TranscriptSourceEnvelope struct {
    EnvelopeID        string `json:"envelope_id"`
    SessionID         string `json:"session_id"`
    TranscriptFamily  string `json:"transcript_family"`
    TranscriptLocator string `json:"transcript_locator"`
    ProjectionLine    string `json:"projection_line,omitempty"`
    LowWatermarkSeq   uint64 `json:"low_watermark_seq,omitempty"`
    HighWatermarkSeq  uint64 `json:"high_watermark_seq,omitempty"`
    ConsistencyState  string `json:"consistency_state,omitempty"`
    UpdatedAt         string `json:"updated_at,omitempty"`
}
```

### `RemoteCursor`

```go
type RemoteCursor struct {
    SessionID      string `json:"session_id"`
    BindingID      string `json:"binding_id"`
    LastEventSeq   uint64 `json:"last_event_seq,omitempty"`
    OwnershipEpoch uint64 `json:"ownership_epoch,omitempty"`
    ReplayToken    string `json:"replay_token,omitempty"`
    SeenAt         string `json:"seen_at,omitempty"`
}
```

### `ControlLease`

```go
type ControlLease struct {
    SessionID        string `json:"session_id"`
    BindingID        string `json:"binding_id"`
    OwnerClientID    string `json:"owner_client_id"`
    OwnerSurfaceKind string `json:"owner_surface_kind"`
    OwnershipEpoch   uint64 `json:"ownership_epoch,omitempty"`
    GrantedAt        string `json:"granted_at,omitempty"`
    ExpiresAt        string `json:"expires_at,omitempty"`
}
```

### `RemoteControlOwner`

```go
type RemoteControlOwner struct {
    SessionID        string `json:"session_id"`
    BindingID        string `json:"binding_id"`
    OwnerClientID    string `json:"owner_client_id"`
    OwnerSurfaceKind string `json:"owner_surface_kind"`
    OwnershipEpoch   uint64 `json:"ownership_epoch,omitempty"`
    ControlMode      string `json:"control_mode,omitempty"`
    ClaimedAt        string `json:"claimed_at,omitempty"`
    ExpiresAt        string `json:"expires_at,omitempty"`
}
```

### `RemoteTakeoverResult`

```go
type RemoteTakeoverResult struct {
    SessionID     string              `json:"session_id,omitempty"`
    BindingID     string              `json:"binding_id,omitempty"`
    PreviousOwner *RemoteControlOwner `json:"previous_owner,omitempty"`
    NewOwner      *RemoteControlOwner `json:"new_owner,omitempty"`
}
```

### `RemoteStatusReport`

```go
type RemoteStatusReport struct {
    MachineID          string         `json:"machine_id,omitempty"`
    PairingState       string         `json:"pairing_state,omitempty"`
    MachineMetadata    *RemoteMachineMetadata `json:"machine_metadata,omitempty"`
    DaemonState        *RemoteDaemonState `json:"daemon_state,omitempty"`
    BindingIDs         []string       `json:"binding_ids,omitempty"`
    ControlOwner       *RemoteControlOwner `json:"control_owner,omitempty"`
    RevocationReason   string         `json:"revocation_reason,omitempty"`
    RevokedAt          string         `json:"revoked_at,omitempty"`
    FeatureAdvertisements []RemoteFeatureAdvertisement `json:"feature_advertisements,omitempty"`
    DegradedFeatures   []string       `json:"degraded_features,omitempty"`
}
```

### `OfflineActionPolicy`

```go
type OfflineActionPolicy struct {
    FollowReplayAllowed      bool `json:"follow_replay_allowed,omitempty"`
    NotifyQueueAllowed       bool `json:"notify_queue_allowed,omitempty"`
    MutatingActionQueueAllowed bool `json:"mutating_action_queue_allowed,omitempty"`
    MaxMutatingQueueDepth    int  `json:"max_mutating_queue_depth,omitempty"`
    PermissionReplyTTLSeconds int `json:"permission_reply_ttl_seconds,omitempty"`
}
```

### `LocalRemoteSwitchPolicy`

```go
type LocalRemoteSwitchPolicy struct {
    SwitchSupported              bool   `json:"switch_supported,omitempty"`
    LocalAttachMode              string `json:"local_attach_mode,omitempty"`
    RemoteFollowAllowedDuringLocal bool `json:"remote_follow_allowed_during_local,omitempty"`
    PendingQueuePolicy           string `json:"pending_queue_policy,omitempty"`
    PermissionModePersistence    string `json:"permission_mode_persistence,omitempty"`
    HandbackTrigger              string `json:"handback_trigger,omitempty"`
}
```

### `RemoteFeatureAdvertisement`

```go
type RemoteFeatureAdvertisement struct {
    FeatureFamily string `json:"feature_family"`
    Enabled       bool   `json:"enabled"`
    PolicyMode    string `json:"policy_mode,omitempty"`
    Reason        string `json:"reason,omitempty"`
}
```

### `RemoteProjectionSnapshot`

```go
type RemoteProjectionSnapshot struct {
    ProjectID          string   `json:"project_id"`
    SessionID          string   `json:"session_id,omitempty"`
    BranchBatchID      string   `json:"branch_batch_id,omitempty"`
    ActiveAgentIDs     []string `json:"active_agent_ids,omitempty"`
    PendingPermissionIDs []string `json:"pending_permission_ids,omitempty"`
    CleanupProposalIDs []string `json:"cleanup_proposal_ids,omitempty"`
    WakeEvents         []string `json:"wake_events,omitempty"`
    MemoryExplainRefs  []string `json:"memory_explain_refs,omitempty"`
    UpdatedAt          string   `json:"updated_at,omitempty"`
}
```

### `AttachEligibility`

```go
type AttachEligibility struct {
    SessionID        string `json:"session_id"`
    Eligible         bool   `json:"eligible"`
    Strategy         string `json:"strategy,omitempty"`
    RefusalReason    string `json:"refusal_reason,omitempty"`
}
```

### `AttachExecutionResult`

```go
type AttachExecutionResult struct {
    SessionID            string `json:"session_id"`
    BindingID            string `json:"binding_id"`
    RuntimeDescriptorRef string `json:"runtime_descriptor_ref,omitempty"`
    ProjectionRef        string `json:"projection_ref,omitempty"`
}
```

### `HandoffEligibility`

```go
type HandoffEligibility struct {
    SessionID        string `json:"session_id"`
    BindingID        string `json:"binding_id,omitempty"`
    Eligible         bool   `json:"eligible"`
    TargetClientID   string `json:"target_client_id,omitempty"`
    RefusalReason    string `json:"refusal_reason,omitempty"`
}
```

### `HandoffExecutionResult`

```go
type HandoffExecutionResult struct {
    SessionID            string `json:"session_id"`
    BindingID            string `json:"binding_id"`
    SourceClientID       string `json:"source_client_id"`
    TargetClientID       string `json:"target_client_id"`
    PreviousOwner        *RemoteControlOwner `json:"previous_owner,omitempty"`
    NewOwner             *RemoteControlOwner `json:"new_owner,omitempty"`
    OwnershipEpoch       uint64 `json:"ownership_epoch,omitempty"`
    RuntimeDescriptorRef string `json:"runtime_descriptor_ref,omitempty"`
    ProjectionRef        string `json:"projection_ref,omitempty"`
}
```

### `RemoteTerminalLease`

```go
type RemoteTerminalLease struct {
    TerminalID      string `json:"terminal_id"`
    MachineID       string `json:"machine_id"`
    ProjectID       string `json:"project_id"`
    CWD             string `json:"cwd"`
    AccessPolicy    string `json:"access_policy"`
    OpenedBy        string `json:"opened_by"`
    LastActivityAt  string `json:"last_activity_at,omitempty"`
}
```

### `NotificationReceipt`

```go
type NotificationReceipt struct {
    NotificationID string `json:"notification_id"`
    ClientID       string `json:"client_id"`
    DeliveryState  string `json:"delivery_state"`
    QueuedAt       string `json:"queued_at,omitempty"`
    DeliveredAt    string `json:"delivered_at,omitempty"`
    AcknowledgedAt string `json:"acknowledged_at,omitempty"`
}
```

### `RemoteNotificationPayload`

```go
type RemoteNotificationPayload struct {
    NotificationID string   `json:"notification_id"`
    Kind           string   `json:"kind"`
    ProjectID      string   `json:"project_id,omitempty"`
    SessionID      string   `json:"session_id,omitempty"`
    Summary        string   `json:"summary,omitempty"`
    DetailRefs     []string `json:"detail_refs,omitempty"`
    Urgency        string   `json:"urgency,omitempty"`
    RequiresAck    bool     `json:"requires_ack,omitempty"`
}
```

### `RemoteNotifyResult`

```go
type RemoteNotifyResult struct {
    Notification *RemoteNotificationPayload `json:"notification,omitempty"`
    Receipts     []NotificationReceipt      `json:"receipts,omitempty"`
}
```

### `RemoteActionActor`

```go
type RemoteActionActor struct {
    ClientID      string `json:"client_id"`
    MachineID     string `json:"machine_id,omitempty"`
    SurfaceKind   string `json:"surface_kind,omitempty"`
    OwnershipEpoch uint64 `json:"ownership_epoch,omitempty"`
}
```

### `RemoteAction`

```go
type RemoteAction struct {
    ActionID                 string                         `json:"action_id"`
    SessionID                string                         `json:"session_id,omitempty"`
    ProjectID                string                         `json:"project_id,omitempty"`
    Actor                    RemoteActionActor              `json:"actor"`
    ActionType               string                         `json:"action_type"`
    SendUserMessage          *SendUserMessageAction         `json:"send_user_message,omitempty"`
    InterruptTurn            *InterruptTurnAction           `json:"interrupt_turn,omitempty"`
    RespondPermissionRequest *RespondPermissionRequestAction `json:"respond_permission_request,omitempty"`
    RequestAttach            *RequestAttachAction           `json:"request_attach,omitempty"`
    RequestHandoff           *RequestHandoffAction          `json:"request_handoff,omitempty"`
    RequestTakeover          *RequestTakeoverAction         `json:"request_takeover,omitempty"`
    ApproveCleanupProposal   *ApproveCleanupProposalAction  `json:"approve_cleanup_proposal,omitempty"`
    InspectMemoryExplain     *InspectMemoryExplainAction    `json:"inspect_memory_explain,omitempty"`
    InspectBranchBatch       *InspectBranchBatchAction      `json:"inspect_branch_batch,omitempty"`
    WakeSupervisedRun        *WakeSupervisedRunAction       `json:"wake_supervised_run,omitempty"`
    InvalidateMemoryRecord   *InvalidateMemoryRecordAction  `json:"invalidate_memory_record,omitempty"`
}
```

Exactly one action payload field must be non-nil, and it must match
`ActionType`.

### `SendUserMessageAction`

```go
type SendUserMessageAction struct {
    MessageText string `json:"message_text"`
}
```

### `InterruptTurnAction`

```go
type InterruptTurnAction struct {
    TurnID string `json:"turn_id,omitempty"`
}
```

### `RespondPermissionRequestAction`

```go
type RespondPermissionRequestAction struct {
    RequestID string `json:"request_id"`
    Decision  string `json:"decision"`
}
```

### `RequestAttachAction`

```go
type RequestAttachAction struct {
    Strategy string `json:"strategy"`
}
```

### `RequestHandoffAction`

```go
type RequestHandoffAction struct {
    BindingID      string `json:"binding_id"`
    TargetClientID string `json:"target_client_id"`
}
```

### `RequestTakeoverAction`

```go
type RequestTakeoverAction struct {
    BindingID string `json:"binding_id"`
    Reason    string `json:"reason,omitempty"`
}
```

### `ApproveCleanupProposalAction`

```go
type ApproveCleanupProposalAction struct {
    ProposalID string `json:"proposal_id"`
    Decision   string `json:"decision"`
}
```

### `InspectMemoryExplainAction`

```go
type InspectMemoryExplainAction struct {
    RecordID string `json:"record_id"`
}
```

### `InspectBranchBatchAction`

```go
type InspectBranchBatchAction struct {
    BatchID string `json:"batch_id"`
}
```

### `WakeSupervisedRunAction`

```go
type WakeSupervisedRunAction struct {
    WakeID string `json:"wake_id"`
}
```

### `InvalidateMemoryRecordAction`

```go
type InvalidateMemoryRecordAction struct {
    RecordID string `json:"record_id"`
    Reason   string `json:"reason,omitempty"`
}
```

## 3.11 `internal/doctor`

### `RuntimeFeatureStatus`

```go
type RuntimeFeatureStatus struct {
    FeatureKind    string `json:"feature_kind"`
    FeatureID      string `json:"feature_id"`
    Status         string `json:"status"`
    Phase          string `json:"phase,omitempty"`
    ErrorCode      string `json:"error_code,omitempty"`
    ErrorMessage   string `json:"error_message,omitempty"`
    ActionableHint string `json:"actionable_hint,omitempty"`
}
```

### `RuntimePreflightReport`

```go
type RuntimePreflightReport struct {
    OverallStatus    string                 `json:"overall_status,omitempty"`
    ProjectID        string                 `json:"project_id,omitempty"`
    WorkspaceRoot    string                 `json:"workspace_root,omitempty"`
    SessionTarget    string                 `json:"session_target,omitempty"`
    ProjectTrace     ProjectResolutionTrace `json:"project_trace,omitempty"`
    ProviderStatus   []DoctorCheck          `json:"provider_status,omitempty"`
    AuthStatus       []DoctorCheck          `json:"auth_status,omitempty"`
    SandboxStatus    string                 `json:"sandbox_status,omitempty"`
    PluginStatus     []DoctorCheck          `json:"plugin_status,omitempty"`
    HookStatus       []DoctorCheck          `json:"hook_status,omitempty"`
    MCPStatus        []DoctorCheck          `json:"mcp_status,omitempty"`
    ConfigStaleness  string                 `json:"config_staleness,omitempty"`
    DegradedFeatures []string               `json:"degraded_features,omitempty"`
    RecoveryHints    []string               `json:"recovery_hints,omitempty"`
    Ready            bool                   `json:"ready"`
}
```

### `DoctorReport`

```go
type DoctorReport struct {
    OverallStatus string        `json:"overall_status"`
    Workspace     DoctorCheck   `json:"workspace"`
    Project       DoctorCheck   `json:"project"`
    Providers     []DoctorCheck `json:"providers,omitempty"`
    Config        DoctorCheck   `json:"config"`
    Binaries      []DoctorCheck `json:"binaries,omitempty"`
    Plugins       []DoctorCheck `json:"plugins,omitempty"`
    Hooks         []DoctorCheck `json:"hooks,omitempty"`
    MCP           []DoctorCheck `json:"mcp,omitempty"`
    Remote        DoctorCheck   `json:"remote"`
    RepairHints   []string      `json:"repair_hints,omitempty"`
}
```

### `SmokeResult`

```go
type SmokeResult struct {
    OverallStatus      string                  `json:"overall_status"`
    Preflight          RuntimePreflightReport  `json:"preflight"`
    Checks             []DoctorCheck           `json:"checks,omitempty"`
    ProviderTrace      ProviderResolutionTrace `json:"provider_trace,omitempty"`
    SessionID          string                  `json:"session_id,omitempty"`
    TurnID             string                  `json:"turn_id,omitempty"`
    EventLogPath       string                  `json:"event_log_path,omitempty"`
    TranscriptPath     string                  `json:"transcript_path,omitempty"`
    SourceMutationFree bool                    `json:"source_mutation_free,omitempty"`
    MutatedPaths       []string                `json:"mutated_paths,omitempty"`
}
```

### `DoctorCheck`

```go
type DoctorCheck struct {
    Name        string `json:"name"`
    Status      string `json:"status"`
    Detail      string `json:"detail,omitempty"`
}
```

### `Service`

```go
type Service interface {
    Preflight(ctx context.Context) (RuntimePreflightReport, error)
    Doctor(ctx context.Context) (DoctorReport, error)
    Smoke(ctx context.Context) (SmokeResult, error)
}
```

## 4. Schema Ownership Map

The following ownership map is mandatory for M0-M2 foundation implementation:

- `schemas/kernel_state_bundle.schema.json` <-> `internal/runtime.KernelStateBundle`
- `schemas/project_state.schema.json` <-> `internal/runtime.ProjectState`
- `schemas/event.schema.json` <-> `internal/events.KernelEventEnvelope`
- `schemas/session.schema.json` <-> `internal/session.SessionMeta` and
  transcript line rules
- global registry file ownership <-> `internal/projects.ProjectRegistryEntry`
  and `internal/projects.CurrentProjectPointer`

The following ownership map is reserved now for later milestones, but does not
become an M0-M2 implementation obligation merely by appearing here:

- `schemas/task_packet.schema.json` <-> `internal/agents.TaskPacket`
- `schemas/agent_runtime_identity.schema.json` <-> `internal/agents.AgentRuntimeIdentity`
- `schemas/agent_runtime_record.schema.json` <-> `internal/agents.AgentRuntimeRecord`
- `schemas/agent_worktree_artifact_candidate_manifest.schema.json` <-> `internal/agents.AgentWorktreeArtifactCandidateManifest`
- `schemas/main_agent_worker_artifact_decision.schema.json` <-> main-agent
  worker artifact accept/reject/defer decision record
- `schemas/review_runtime_record.schema.json` <-> `internal/reviews.ReviewRuntimeRecord`
- `schemas/branch_batch_record.schema.json` <-> `internal/branches.BranchBatchRecord`
- `schemas/branch_run_record.schema.json` <-> `internal/branches.BranchRunRecord`
- `schemas/session_operator_log_manifest.schema.json` <->
  `internal/session.SessionOperatorLogManifest`
- `schemas/derived_session_read_model.schema.json` <->
  `internal/session.DerivedSessionReadModel`
- `schemas/working_memory_record.schema.json` <-> `internal/memory.WorkingMemoryRecord`
- `schemas/memory_record.schema.json` <-> `internal/memory.MemoryRecord`
- `schemas/projectops_tick.schema.json` <-> `internal/projectops.ProjectOpsTick`
- `schemas/progress_digest_candidate.schema.json` <-> `internal/projectops.ProgressDigestCandidate`
- `schemas/experiment_supervisor_lease.schema.json` <-> `internal/projectops.ExperimentSupervisorLease`
- `schemas/wake_event.schema.json` <-> `internal/projectops.WakeEvent`
- `schemas/change_envelope.schema.json` <-> `internal/projectops.ChangeEnvelope`
- `schemas/memory_explain_record.schema.json` <-> `internal/memory.MemoryExplainRecord`
- `schemas/remote_capability_matrix.schema.json` <-> `internal/remote.RemoteCapabilityMatrix`
- `schemas/remote_capability_set.schema.json` <-> `internal/remote.RemoteCapabilitySet`
- `schemas/local_control_capability.schema.json` <-> `internal/remote.LocalControlCapability`
- `schemas/existing_session_automation_eligibility.schema.json` <->
  `internal/remote.ExistingSessionAutomationEligibility`
- `schemas/remote_machine_metadata.schema.json` <-> `internal/remote.RemoteMachineMetadata`
- `schemas/remote_daemon_state.schema.json` <-> `internal/remote.RemoteDaemonState`
- `schemas/provider_session_source.schema.json` <-> `internal/remote.ProviderSessionSource`
- `schemas/remote_binding.schema.json` <-> `internal/remote.RemoteSessionBinding`
- `schemas/remote_control_owner.schema.json` <-> `internal/remote.RemoteControlOwner`
- `schemas/remote_action_actor.schema.json` <-> `internal/remote.RemoteActionActor`
- `schemas/remote_action.schema.json` <-> `internal/remote.RemoteAction`
- `schemas/remote_notification_payload.schema.json` <-> `internal/remote.RemoteNotificationPayload`
- `schemas/transcript_source_envelope.schema.json` <-> `internal/remote.TranscriptSourceEnvelope`
- `schemas/remote_projection_snapshot.schema.json` <-> `internal/remote.RemoteProjectionSnapshot`
- `schemas/remote_terminal_lease.schema.json` <-> `internal/remote.RemoteTerminalLease`
- `schemas/remote_status_report.schema.json` <-> `internal/remote.RemoteStatusReport`
- `schemas/research_stage_execution.schema.json` <-> `internal/research.ResearchStageExecution`
- `schemas/stage_execution_map_entry.schema.json` <-> `internal/research.StageExecutionMapEntry`
- `schemas/stage_execution_map.schema.json` <-> `internal/research.StageExecutionMap`

No second schema owner may appear for these objects.

## 5. First Interface Wiring

The first `App` bootstrap path should wire dependencies in roughly this shape:

```go
type App struct {
    WorkspaceResolver workspace.WorkspaceResolver
    ProjectRegistry   projects.ProjectRegistry
    ProjectLocator    projects.ProjectLocator
    SessionStore      session.SessionStore
    SessionIndex      session.SessionSearchIndex
    EventWriter       events.EventWriter
    PermissionPolicy  permissions.PermissionPolicy
    ToolRegistry      tools.Registry
    ToolExecutor      tools.ToolExecutor
    ProviderResolver  providers.ProviderResolver
    ConfigStore       config.ConfigStore
    PluginRegistry    plugins.PluginRegistry
    HookRegistry      plugins.HookRegistry
    MCPRegistry       mcp.MCPRegistry
    SetupService      setup.SetupService
    TelemetryService  telemetry.Service
    DoctorService     doctor.Service
    Runtime           runtime.Service
}
```

### `CheckpointReducer`

Checkpoint publication must be explicitly owned rather than implicit.

```go
type CheckpointReducer interface {
    Publish(ctx context.Context, plan CheckpointPublishPlan) (KernelStateBundle, error)
}
```

```go
type CheckpointPublishPlan struct {
    BaseSeqCursor       uint64   `json:"base_seq_cursor"`
    BaseCheckpointEpoch uint64   `json:"base_checkpoint_epoch"`
    TouchedFamilies     []string `json:"touched_families,omitempty"`
}
```

The exact struct name may vary, but dependency directions should not:

- `cmd` -> `app`
- `app` -> package interfaces
- `runtime` -> session/events/providers/permissions/tools
- persistence packages do not import `cmd`

## 6. Serialization Rules

For every persisted type above:

- use stable JSON field names
- prefer string IDs over integer counters for object identity
- timestamps must be RFC3339 strings
- enums should serialize as readable strings
- optional fields should omit empty values where appropriate

Do not serialize:

- raw interface values
- transport-only websocket state
- terminal presentation formatting

## 7. What Can Stay Stubbed In M0-M2

To avoid false complexity, the following may stay skeletal:

- advanced child states inside `KernelStateBundle`
- MCP deep capability metadata
- provider catalog richness
- transcript content normalization beyond the initial kinds
- permission scope refinement beyond baseline path/tool scope

But the canonical struct names and package owners must still be created now.

## 8. Exit Condition

This manifest is complete when an implementation worker can create the core Go
types and interfaces without reopening architecture debates about naming,
ownership, or persistence authority.

At that point, M0-M2 becomes an execution problem.
