use crate::review_refs;
use std::fmt;
use std::path::PathBuf;

const ACCEPTED_WORKER_EVIDENCE_INDEX_PREFIX: &str = "accepted_worker_evidence_index:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardTaskRefField {
    InputArtifactRefs,
    DependsOnTaskIds,
    ReviewFindingsRefs,
    BlockerRefs,
    ReviewTargetTaskIds,
    ReviewTargetEvidenceRefs,
    SupersedesTaskIds,
    ReplacementOfTaskIds,
}

impl BoardTaskRefField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InputArtifactRefs => "input_artifact_refs",
            Self::DependsOnTaskIds => "depends_on_task_ids",
            Self::ReviewFindingsRefs => "review_findings_refs",
            Self::BlockerRefs => "blocker_refs",
            Self::ReviewTargetTaskIds => "review_target_task_ids",
            Self::ReviewTargetEvidenceRefs => "review_target_evidence_refs",
            Self::SupersedesTaskIds => "supersedes_task_ids",
            Self::ReplacementOfTaskIds => "replacement_of_task_ids",
        }
    }
}

pub const BOARD_TASK_REF_ARRAY_FIELDS: &[(BoardTaskRefField, &str)] = &[
    (BoardTaskRefField::InputArtifactRefs, "input_artifact_refs"),
    (BoardTaskRefField::DependsOnTaskIds, "depends_on_task_ids"),
    (
        BoardTaskRefField::ReviewFindingsRefs,
        "review_findings_refs",
    ),
    (BoardTaskRefField::BlockerRefs, "blocker_refs"),
    (
        BoardTaskRefField::ReviewTargetTaskIds,
        "review_target_task_ids",
    ),
    (
        BoardTaskRefField::ReviewTargetEvidenceRefs,
        "review_target_evidence_refs",
    ),
    (BoardTaskRefField::SupersedesTaskIds, "supersedes_task_ids"),
    (
        BoardTaskRefField::ReplacementOfTaskIds,
        "replacement_of_task_ids",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardTaskRefKind {
    Empty,
    PromptHash,
    AutoResearchJob,
    AcceptedWorkerEvidence,
    AcceptedWorkerEvidenceTask,
    AcceptedWorkerEvidenceIndex,
    ReviewArtifact,
    LegacyReview,
    BoardTask,
    MainAgentBoardTask,
    MainAgentRound,
    StageClosureLedger,
    StageClosureBlocker,
    ReadinessRef,
    WorkspacePath,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchInputRefPolicy {
    MountRequired,
    MountIfPathExists,
    ContextOnly,
    InvalidForWorkerInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTaskRefError {
    pub field: BoardTaskRefField,
    pub reference: String,
    pub reason: String,
}

impl BoardTaskRefError {
    pub fn message(&self) -> String {
        format!(
            "`{}` contains invalid ref `{}`: {}",
            self.field.as_str(),
            self.reference,
            self.reason
        )
    }
}

impl fmt::Display for BoardTaskRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for BoardTaskRefError {}

pub fn normalize_ref_token(reference: &str) -> String {
    reference.trim().trim_matches('`').trim().to_string()
}

pub fn classify_board_task_ref(reference: &str) -> BoardTaskRefKind {
    let trimmed = normalize_ref_token(reference);
    if trimmed.is_empty() {
        return BoardTaskRefKind::Empty;
    }
    if trimmed.starts_with("prompt_sha256:") {
        return BoardTaskRefKind::PromptHash;
    }
    if trimmed.starts_with("auto_research_job:") {
        return BoardTaskRefKind::AutoResearchJob;
    }
    if trimmed.starts_with("accepted_worker_evidence_task:") {
        return BoardTaskRefKind::AcceptedWorkerEvidenceTask;
    }
    if trimmed.starts_with("accepted_worker_evidence_index:") {
        return BoardTaskRefKind::AcceptedWorkerEvidenceIndex;
    }
    if trimmed.starts_with("accepted_worker_evidence:") {
        return BoardTaskRefKind::AcceptedWorkerEvidence;
    }
    if review_refs::starts_with_review_artifact_prefix(&trimmed) {
        return BoardTaskRefKind::ReviewArtifact;
    }
    if trimmed.starts_with("review:")
        || trimmed.starts_with("review::")
        || trimmed.starts_with("rev_")
    {
        return BoardTaskRefKind::LegacyReview;
    }
    if trimmed.starts_with("board::") {
        return BoardTaskRefKind::BoardTask;
    }
    if trimmed.starts_with("main_agent_board_task::") {
        return BoardTaskRefKind::MainAgentBoardTask;
    }
    if trimmed.starts_with("main_agent_round:") {
        return BoardTaskRefKind::MainAgentRound;
    }
    if trimmed.starts_with("stage_closure_ledger:") {
        return BoardTaskRefKind::StageClosureLedger;
    }
    if trimmed.starts_with("stage_closure_blocker:") || trimmed.starts_with("stage_blocker:") {
        return BoardTaskRefKind::StageClosureBlocker;
    }
    if trimmed.starts_with("readiness_ref:") {
        return BoardTaskRefKind::ReadinessRef;
    }
    if ref_looks_like_workspace_path(&trimmed) {
        return BoardTaskRefKind::WorkspacePath;
    }
    BoardTaskRefKind::Other
}

pub fn canonicalize_ref_for_field(
    field: BoardTaskRefField,
    reference: &str,
) -> Result<Option<String>, BoardTaskRefError> {
    let trimmed = normalize_ref_token(reference);
    if trimmed.is_empty() {
        return Ok(None);
    }
    if classify_board_task_ref(&trimmed) == BoardTaskRefKind::StageClosureLedger {
        return Err(BoardTaskRefError {
            field,
            reference: trimmed,
            reason: "stage_closure_ledger is the main-agent-only StageClosureLedger read-model coordinate, not a board-task ref. Do not copy it into worker task fields; turn the concrete issue into objective/acceptance_checks and cite a concrete blocker in blocker_refs, for example stage_closure_blocker:missing_task_type:<task_type>.".to_string(),
        });
    }
    if trimmed.starts_with(ACCEPTED_WORKER_EVIDENCE_INDEX_PREFIX) {
        let canonical =
            canonicalize_accepted_worker_evidence_index_ref(&trimmed).map_err(|reason| {
                BoardTaskRefError {
                    field,
                    reference: trimmed.clone(),
                    reason,
                }
            })?;
        return Ok(Some(canonical));
    }
    if review_refs::starts_with_review_artifact_prefix(&trimmed) {
        let canonical = review_refs::canonical_review_artifact_ref(&trimmed).ok_or_else(|| {
            BoardTaskRefError {
                field,
                reference: trimmed.clone(),
                reason: "use review_packet:rev_xxx or review_trace:rev_xxx instead of combining a review prefix with an unparseable path".to_string(),
            }
        })?;
        return Ok(Some(canonical));
    }
    Ok(Some(trimmed))
}

fn canonicalize_accepted_worker_evidence_index_ref(reference: &str) -> Result<String, String> {
    let Some(path) = accepted_worker_evidence_index_path_ref(reference) else {
        return Err("accepted_worker_evidence_index refs must point under `research/stages/.../accepted_worker_evidence/index.json`. Do not attach this prefix to stage artifacts such as `literature_matrix.md`; use `accepted_worker_evidence_task:*` for task-level bundles or copy an existing accepted evidence index ref from the active context.".to_string());
    };
    Ok(format!("{ACCEPTED_WORKER_EVIDENCE_INDEX_PREFIX}{path}"))
}

pub fn accepted_worker_evidence_index_path_ref(reference: &str) -> Option<String> {
    let trimmed = normalize_ref_token(reference);
    let payload = trimmed.strip_prefix(ACCEPTED_WORKER_EVIDENCE_INDEX_PREFIX)?;
    let normalized = normalize_accepted_worker_evidence_index_path_payload(payload);
    accepted_worker_evidence_index_path_is_valid(&normalized).then_some(normalized)
}

fn normalize_accepted_worker_evidence_index_path_payload(payload: &str) -> String {
    payload.trim().trim_matches('`').replace('\\', "/")
}

fn accepted_worker_evidence_index_path_is_valid(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    let components = path
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>();
    if components.iter().any(|component| *component == "..") {
        return false;
    }
    if components.len() < 4 {
        return false;
    }
    let Some(last) = components.last() else {
        return false;
    };
    let penultimate = components
        .get(components.len().saturating_sub(2))
        .copied()
        .unwrap_or_default();
    if penultimate != "accepted_worker_evidence" || *last != "index.json" {
        return false;
    }
    components
        .windows(2)
        .any(|window| window == ["research", "stages"])
}

pub fn canonicalize_refs_for_field(
    field: BoardTaskRefField,
    refs: &[String],
) -> Result<Vec<String>, BoardTaskRefError> {
    let mut normalized = Vec::new();
    for reference in refs {
        let Some(canonical) = canonicalize_ref_for_field(field, reference)? else {
            continue;
        };
        if !normalized.iter().any(|existing| existing == &canonical) {
            normalized.push(canonical);
        }
    }
    Ok(normalized)
}

pub fn first_ref_field_violation(
    field: BoardTaskRefField,
    refs: &[String],
) -> Option<BoardTaskRefError> {
    canonicalize_refs_for_field(field, refs).err()
}

pub fn dispatch_input_ref_policy(reference: &str) -> DispatchInputRefPolicy {
    match classify_board_task_ref(reference) {
        BoardTaskRefKind::AcceptedWorkerEvidenceTask
        | BoardTaskRefKind::AcceptedWorkerEvidenceIndex
        | BoardTaskRefKind::ReviewArtifact => DispatchInputRefPolicy::MountRequired,
        BoardTaskRefKind::PromptHash
        | BoardTaskRefKind::AutoResearchJob
        | BoardTaskRefKind::BoardTask
        | BoardTaskRefKind::MainAgentBoardTask
        | BoardTaskRefKind::MainAgentRound
        | BoardTaskRefKind::StageClosureBlocker
        | BoardTaskRefKind::Empty => DispatchInputRefPolicy::ContextOnly,
        BoardTaskRefKind::StageClosureLedger => DispatchInputRefPolicy::InvalidForWorkerInput,
        BoardTaskRefKind::AcceptedWorkerEvidence
        | BoardTaskRefKind::LegacyReview
        | BoardTaskRefKind::ReadinessRef
        | BoardTaskRefKind::WorkspacePath
        | BoardTaskRefKind::Other => DispatchInputRefPolicy::MountIfPathExists,
    }
}

pub fn dispatch_input_ref_is_required(reference: &str) -> bool {
    dispatch_input_ref_policy(reference) == DispatchInputRefPolicy::MountRequired
}

pub fn input_ref_is_context_only(reference: &str) -> bool {
    dispatch_input_ref_policy(reference) == DispatchInputRefPolicy::ContextOnly
}

pub fn review_artifact_workspace_path(reference: &str) -> Option<PathBuf> {
    review_refs::review_artifact_ref_workspace_path(reference)
}

pub fn is_concrete_upstream_dependency_ref(reference: &str) -> bool {
    matches!(
        classify_board_task_ref(reference),
        BoardTaskRefKind::AcceptedWorkerEvidence
            | BoardTaskRefKind::AcceptedWorkerEvidenceTask
            | BoardTaskRefKind::AcceptedWorkerEvidenceIndex
            | BoardTaskRefKind::ReviewArtifact
            | BoardTaskRefKind::LegacyReview
            | BoardTaskRefKind::BoardTask
            | BoardTaskRefKind::MainAgentBoardTask
            | BoardTaskRefKind::WorkspacePath
    )
}

pub fn is_canonical_project_input_ref(reference: &str) -> bool {
    if classify_board_task_ref(reference) != BoardTaskRefKind::WorkspacePath {
        return false;
    }
    let normalized = normalize_ref_token(reference)
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string();
    !(normalized.starts_with(".pmcli/")
        || normalized.starts_with(".trellis/")
        || normalized.starts_with("research/auto/")
        || normalized.contains("/accepted_worker_evidence/"))
}

fn ref_looks_like_workspace_path(reference: &str) -> bool {
    let normalized = reference.replace('\\', "/");
    normalized.contains('/')
        || normalized.ends_with(".py")
        || normalized.ends_with(".json")
        || normalized.ends_with(".jsonl")
        || normalized.ends_with(".md")
        || normalized.ends_with(".toml")
        || normalized.ends_with(".yaml")
        || normalized.ends_with(".yml")
        || normalized.ends_with(".txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_dispatch_input_refs_by_shared_policy() {
        assert_eq!(
            dispatch_input_ref_policy("review_packet:rev_123"),
            DispatchInputRefPolicy::MountRequired
        );
        assert_eq!(
            dispatch_input_ref_policy("accepted_worker_evidence_task:paper_search"),
            DispatchInputRefPolicy::MountRequired
        );
        assert_eq!(
            dispatch_input_ref_policy("auto_research_job:arj_1"),
            DispatchInputRefPolicy::ContextOnly
        );
        assert_eq!(
            dispatch_input_ref_policy("stage_closure_ledger:missing_task_type:paper_search"),
            DispatchInputRefPolicy::InvalidForWorkerInput
        );
        assert!(!dispatch_input_ref_is_required(
            "stage_closure_ledger:missing_task_type:paper_search"
        ));
    }

    #[test]
    fn canonicalizes_review_refs_and_rejects_ledger_refs_for_board_fields() {
        let refs = canonicalize_refs_for_field(
            BoardTaskRefField::InputArtifactRefs,
            &[
                "review_packet:.pmcli/reviews/rev_123/packet.json".to_string(),
                "review_packet:rev_123".to_string(),
            ],
        )
        .expect("review refs should canonicalize");
        assert_eq!(refs, vec!["review_packet:rev_123".to_string()]);

        let err = canonicalize_refs_for_field(
            BoardTaskRefField::ReviewTargetEvidenceRefs,
            &["stage_closure_ledger:review_rerun_blockers".to_string()],
        )
        .expect_err("stage closure ledger refs are not board-task refs");
        assert_eq!(err.field, BoardTaskRefField::ReviewTargetEvidenceRefs);
        assert!(err.message().contains("main-agent-only"));
        assert!(err.message().contains("blocker_refs"));
    }

    #[test]
    fn validates_accepted_worker_evidence_index_ref_shape() {
        let refs = canonicalize_refs_for_field(
            BoardTaskRefField::InputArtifactRefs,
            &["accepted_worker_evidence_index:research/stages/job_1/stage_1/accepted_worker_evidence/index.json".to_string()],
        )
        .expect("accepted evidence index ref should validate");
        assert_eq!(
            refs,
            vec![
                "accepted_worker_evidence_index:research/stages/job_1/stage_1/accepted_worker_evidence/index.json"
                    .to_string()
            ]
        );

        let absolute = accepted_worker_evidence_index_path_ref(
            "accepted_worker_evidence_index:/tmp/workspace/research/stages/job_1/stage_1/accepted_worker_evidence/index.json",
        )
        .expect("absolute context-projected index refs should validate by shape");
        assert_eq!(
            absolute,
            "/tmp/workspace/research/stages/job_1/stage_1/accepted_worker_evidence/index.json"
        );

        let err = canonicalize_refs_for_field(
            BoardTaskRefField::ReviewTargetEvidenceRefs,
            &["accepted_worker_evidence_index:research/stages/job_1/literature/literature_matrix.md".to_string()],
        )
        .expect_err("stage artifacts are not accepted evidence indexes");
        assert_eq!(err.field, BoardTaskRefField::ReviewTargetEvidenceRefs);
        assert!(err
            .message()
            .contains("accepted_worker_evidence/index.json"));
        assert!(err.message().contains("literature_matrix.md"));

        assert!(accepted_worker_evidence_index_path_ref(
            "accepted_worker_evidence_index:research/stages/job_1/stage_1/accepted_worker_evidence/../accepted_worker_evidence/index.json",
        )
        .is_none());
    }

    #[test]
    fn separates_concrete_dependencies_from_context_and_ledger_refs() {
        assert!(is_concrete_upstream_dependency_ref(
            "accepted_worker_evidence_task:paper_search"
        ));
        assert!(is_concrete_upstream_dependency_ref("review_packet:rev_1"));
        assert!(is_concrete_upstream_dependency_ref(
            "research/stages/job/literature.md"
        ));
        assert!(!is_concrete_upstream_dependency_ref(
            "auto_research_job:arj_1"
        ));
        assert!(!is_concrete_upstream_dependency_ref(
            "stage_closure_ledger:adoption_blockers"
        ));

        assert!(is_canonical_project_input_ref("baseline_runner.py"));
        assert!(!is_canonical_project_input_ref(
            "accepted_worker_evidence_task:paper_search"
        ));
        assert!(!is_canonical_project_input_ref(
            ".pmcli/agents/agent_1/provider_worker_evidence.md"
        ));
    }
}
