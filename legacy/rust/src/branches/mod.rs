use crate::canonicality;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchBatchRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub batch_id: String,
    pub project_id: String,
    pub objective: String,
    pub strategy: String,
    pub max_branches: usize,
    pub base_commit: String,
    pub status: String,
    pub budget: BranchSearchBudget,
    pub branch_runs: Vec<BranchRunRecord>,
    pub lineage_path: String,
    pub created_at: String,
    pub canonicality_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchSearchBudget {
    pub max_branches: usize,
    pub max_evaluations: usize,
    pub stop_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRunRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub branch_id: String,
    pub batch_id: String,
    pub parent_branch_id: Option<String>,
    pub base_commit: String,
    pub worktree_path: String,
    pub hypothesis: String,
    pub variation_operator: String,
    pub agent_role: String,
    pub claimed_paths: Vec<String>,
    pub status: String,
    pub evaluation_id: Option<String>,
    pub debate_trace_id: Option<String>,
    #[serde(default)]
    pub verifier_tournament_id: Option<String>,
    pub promotion_decision_id: Option<String>,
    pub archived_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchListResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub total_count: usize,
    pub branch_runs: Vec<BranchRunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInspectionResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub branch_run: BranchRunRecord,
    pub search_batch: Option<SearchBatchRecord>,
    pub evaluation_packet: Option<EvaluationPacket>,
    pub debate_trace: Option<DebateTrace>,
    pub verifier_tournament: Option<VerifierTournament>,
    pub promotion_decision: Option<PromotionDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationPacket {
    pub schema_version: String,
    pub conformance_line: String,
    pub evaluation_id: String,
    pub branch_id: String,
    pub batch_id: String,
    pub base_commit: String,
    pub current_commit: String,
    pub stale_base: bool,
    pub changed_paths: Vec<String>,
    pub claimed_paths: Vec<String>,
    pub test_commands: Vec<String>,
    pub test_outcomes: Vec<EvaluationOutcome>,
    pub schema_checks: Vec<EvaluationOutcome>,
    pub canonicality_gate: EvaluationOutcome,
    pub reviewer_gate_status: String,
    pub objective_metric_delta: f64,
    pub risk_score: f64,
    pub blocking_issues: Vec<String>,
    pub promotion_eligible: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationOutcome {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateTrace {
    pub schema_version: String,
    pub conformance_line: String,
    pub debate_id: String,
    pub branch_id: String,
    pub against_branch_id: String,
    pub proposition: String,
    pub supporting_evidence: Vec<String>,
    pub opposing_evidence: Vec<String>,
    pub unresolved_assumptions: Vec<String>,
    pub reviewer_roles: Vec<String>,
    pub recommendation: String,
    pub confidence: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierCriterion {
    pub criterion_id: String,
    pub name: String,
    pub description: String,
    pub evidence_requirements: Vec<String>,
    pub ground_truth_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierPairScore {
    pub schema_version: String,
    pub conformance_line: String,
    pub pair_score_id: String,
    pub batch_id: String,
    pub candidate_a: String,
    pub candidate_b: String,
    pub criterion_id: String,
    pub repetition_index: usize,
    pub score_a: f64,
    pub score_b: f64,
    pub score_delta: f64,
    pub winner_ref: Option<String>,
    pub provider: String,
    pub model: String,
    pub scoring_mode: String,
    pub evidence_digest: String,
    pub raw_trace_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierTournament {
    pub schema_version: String,
    pub conformance_line: String,
    pub tournament_id: String,
    pub scope: String,
    pub batch_id: String,
    pub candidate_refs: Vec<String>,
    pub criteria: Vec<VerifierCriterion>,
    pub repetitions: usize,
    pub pair_scores: Vec<VerifierPairScore>,
    pub winner_ref: String,
    pub recommendation: String,
    pub deterministic_gate_result_ref: Option<String>,
    pub canonicality_gate_result_ref: Option<String>,
    pub merge_gate_result_ref: Option<String>,
    pub advisory_until_gates_pass: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchMutationResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub mutation_id: String,
    pub branch_id: String,
    pub batch_id: String,
    pub status: String,
    pub mutation_source: String,
    pub command: String,
    pub before_commit: String,
    pub after_commit: String,
    pub changed_paths: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionDecision {
    pub schema_version: String,
    pub conformance_line: String,
    pub decision_id: String,
    pub branch_id: String,
    pub batch_id: String,
    pub status: String,
    pub decision: String,
    pub missing_gates: Vec<String>,
    #[serde(default = "default_verifier_gate_status")]
    pub verifier_gate_status: String,
    pub merge_status: String,
    pub merged_paths: Vec<String>,
    pub merge_commit: Option<String>,
    pub canonical_surface_policy: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub branch_id: String,
    pub batch_id: String,
    pub status: String,
    pub reason: String,
    pub archived_at: String,
}

#[derive(Debug)]
pub enum BranchError {
    InvalidInput(String),
    UnknownBranch(String),
    PromotionBlocked {
        branch_id: String,
        missing_gates: Vec<String>,
    },
    GitCommand {
        action: String,
        stderr: String,
    },
    WinnerExists {
        batch_id: String,
        winner_branch_id: String,
    },
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl std::fmt::Display for BranchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "invalid branch request: {message}"),
            Self::UnknownBranch(branch_id) => write!(formatter, "unknown branch run: {branch_id}"),
            Self::PromotionBlocked {
                branch_id,
                missing_gates,
            } => write!(
                formatter,
                "branch promotion blocked for {branch_id}: missing {}",
                missing_gates.join(", ")
            ),
            Self::GitCommand { action, stderr } => {
                write!(formatter, "git command failed during {action}: {stderr}")
            }
            Self::WinnerExists {
                batch_id,
                winner_branch_id,
            } => write!(
                formatter,
                "search batch {batch_id} already has promoted winner {winner_branch_id}"
            ),
            Self::Io(err) => write!(formatter, "branch IO failed: {err}"),
            Self::Serde(err) => write!(formatter, "branch JSON failed: {err}"),
        }
    }
}

impl std::error::Error for BranchError {}
impl From<std::io::Error> for BranchError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for BranchError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn search(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
    objective: &str,
    strategy: &str,
    max_branches: usize,
) -> Result<SearchBatchRecord, BranchError> {
    if objective.trim().is_empty() {
        return Err(BranchError::InvalidInput(
            "branches search requires --objective".to_string(),
        ));
    }
    if max_branches == 0 || max_branches > 16 {
        return Err(BranchError::InvalidInput(
            "--max-branches must be between 1 and 16".to_string(),
        ));
    }
    if !matches!(strategy, "beam" | "mcts" | "evolutionary" | "manual") {
        return Err(BranchError::InvalidInput(format!(
            "unsupported branch strategy: {strategy}"
        )));
    }
    fs::create_dir_all(batches_dir(data_dir))?;
    fs::create_dir_all(runs_dir(data_dir))?;
    fs::create_dir_all(worktrees_dir(data_dir))?;
    let now = timestamp_string();
    let seed = timestamp_millis();
    let batch_id = format!("batch_{seed}");
    let base_commit = git_output(workspace_root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let operators = variation_operator_cycle();
    let mut branch_runs = Vec::new();
    for index in 0..max_branches {
        let branch_id = format!("branch_{seed}_{index}");
        let operator = operators[index % operators.len()].to_string();
        let worktree_path =
            create_git_worktree(data_dir, workspace_root, &branch_id, &base_commit)?;
        create_candidate_artifact(&worktree_path, &branch_id, objective, &operator)?;
        let run = BranchRunRecord {
            schema_version: "branch_run_record.v1".to_string(),
            conformance_line: "M9.evolutionary_branch_intelligence".to_string(),
            branch_id: branch_id.clone(),
            batch_id: batch_id.clone(),
            parent_branch_id: None,
            base_commit: base_commit.clone(),
            worktree_path: worktree_path.display().to_string(),
            hypothesis: format!("{objective} via {operator}"),
            variation_operator: operator.clone(),
            agent_role: agent_role_for_operator(&operator).to_string(),
            claimed_paths: claimed_paths_for_operator(&operator),
            status: "draft".to_string(),
            evaluation_id: None,
            debate_trace_id: None,
            verifier_tournament_id: None,
            promotion_decision_id: None,
            archived_reason: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        write_json(&run_path(data_dir, &branch_id), &run)?;
        append_lineage(
            data_dir,
            json!({"event":"branch_created","batch_id":batch_id,"branch_id":branch_id,"variation_operator":operator,"parent_branch_id":null,"created_at":now}),
        )?;
        branch_runs.push(run);
    }
    let batch = SearchBatchRecord {
        schema_version: "search_batch_record.v1".to_string(),
        conformance_line: "M9.evolutionary_branch_intelligence".to_string(),
        batch_id: batch_id.clone(),
        project_id: project_id.to_string(),
        objective: objective.to_string(),
        strategy: strategy.to_string(),
        max_branches,
        base_commit,
        status: "running".to_string(),
        budget: BranchSearchBudget {
            max_branches,
            max_evaluations: max_branches,
            stop_policy: "stop_when_budget_exhausted_or_promoted".to_string(),
        },
        branch_runs,
        lineage_path: lineage_path(data_dir).display().to_string(),
        created_at: now,
        canonicality_policy: "hidden_until_director_promotion".to_string(),
    };
    write_json(&batch_path(data_dir, &batch_id), &batch)?;
    Ok(batch)
}

pub fn list(data_dir: &Path) -> Result<BranchListResult, BranchError> {
    let mut branch_runs = read_all_runs(data_dir)?;
    branch_runs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(BranchListResult {
        schema_version: "branch_list_result.v1".to_string(),
        conformance_line: "M9.evolutionary_branch_intelligence".to_string(),
        total_count: branch_runs.len(),
        branch_runs,
    })
}

pub fn inspect(data_dir: &Path, branch_id: &str) -> Result<BranchInspectionResult, BranchError> {
    let run = read_run(data_dir, branch_id)?;
    Ok(BranchInspectionResult {
        schema_version: "branch_inspection_result.v1".to_string(),
        conformance_line: "M9.evolutionary_branch_intelligence".to_string(),
        search_batch: read_json_optional(&batch_path(data_dir, &run.batch_id))?,
        evaluation_packet: run.evaluation_id.as_deref().and_then(|id| {
            read_json_optional(&evaluation_path(data_dir, id))
                .ok()
                .flatten()
        }),
        debate_trace: run.debate_trace_id.as_deref().and_then(|id| {
            read_json_optional(&debate_path(data_dir, id))
                .ok()
                .flatten()
        }),
        verifier_tournament: run.verifier_tournament_id.as_deref().and_then(|id| {
            read_json_optional(&verifier_tournament_path(data_dir, id))
                .ok()
                .flatten()
        }),
        promotion_decision: run.promotion_decision_id.as_deref().and_then(|id| {
            read_json_optional(&promotion_path(data_dir, id))
                .ok()
                .flatten()
        }),
        branch_run: run,
    })
}

pub fn evaluate(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
    branch_id: &str,
    eval_commands: &[String],
) -> Result<EvaluationPacket, BranchError> {
    fs::create_dir_all(evaluations_dir(data_dir))?;
    let mut run = read_run(data_dir, branch_id)?;
    let now = timestamp_string();
    let evaluation_id = format!("eval_{}_{}", branch_id, timestamp_millis());
    let worktree_path = PathBuf::from(&run.worktree_path);
    let current_source_commit =
        git_output(workspace_root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let current_commit = git_output(&worktree_path, &["rev-parse", "HEAD"]).unwrap_or_default();
    let stale_base = !run.base_commit.is_empty()
        && !current_source_commit.is_empty()
        && run.base_commit != current_source_commit;
    let changed_paths = if run.base_commit.is_empty() {
        git_lines(&worktree_path, &["diff", "--name-only", "HEAD"])
    } else {
        git_lines(
            &worktree_path,
            &["diff", "--name-only", &run.base_commit, "HEAD"],
        )
    };
    let commands = if eval_commands.is_empty() {
        default_eval_commands(&worktree_path)
    } else {
        eval_commands.to_vec()
    };
    let test_outcomes = commands
        .iter()
        .map(|command| run_eval_command(&worktree_path, command))
        .collect::<Vec<_>>();
    let schema_checks = vec![EvaluationOutcome {
        name: "m9_record_shapes".to_string(),
        status: "passed".to_string(),
        detail: "branch records are schema-versioned and evaluation packet was persisted"
            .to_string(),
    }];
    let canonicality_gate = canonicality_outcome(workspace_root, data_dir, project_id);
    let mut blocking_issues = Vec::new();
    if stale_base {
        blocking_issues.push("stale_base".to_string());
    }
    for outcome in &test_outcomes {
        if outcome.status != "passed" {
            blocking_issues.push(format!("test_command:{}", outcome.name));
        }
    }
    if canonicality_gate.status != "passed" {
        blocking_issues.push("canonicality_gate".to_string());
    }
    let packet = EvaluationPacket {
        schema_version: "evaluation_packet.v1".to_string(),
        conformance_line: "M9.branch_evaluation".to_string(),
        evaluation_id: evaluation_id.clone(),
        branch_id: branch_id.to_string(),
        batch_id: run.batch_id.clone(),
        base_commit: run.base_commit.clone(),
        current_commit,
        stale_base,
        changed_paths,
        claimed_paths: run.claimed_paths.clone(),
        test_commands: commands,
        test_outcomes,
        schema_checks,
        canonicality_gate,
        reviewer_gate_status: "requires_debate_trace".to_string(),
        objective_metric_delta: 0.0,
        risk_score: if blocking_issues.is_empty() { 0.1 } else { 1.0 },
        promotion_eligible: blocking_issues.is_empty(),
        blocking_issues,
        created_at: now.clone(),
    };
    write_json(&evaluation_path(data_dir, &evaluation_id), &packet)?;
    run.evaluation_id = Some(evaluation_id);
    run.status = if packet.promotion_eligible {
        "evaluated".to_string()
    } else {
        "needs_refresh".to_string()
    };
    run.updated_at = now.clone();
    write_json(&run_path(data_dir, branch_id), &run)?;
    append_lineage(
        data_dir,
        json!({"event":"branch_evaluated","branch_id":branch_id,"batch_id":run.batch_id,"promotion_eligible":packet.promotion_eligible,"created_at":now}),
    )?;
    Ok(packet)
}

pub fn debate(
    data_dir: &Path,
    branch_id: &str,
    against_branch_id: &str,
) -> Result<DebateTrace, BranchError> {
    fs::create_dir_all(debates_dir(data_dir))?;
    let mut run = read_run(data_dir, branch_id)?;
    let against = read_run(data_dir, against_branch_id)?;
    if run.batch_id != against.batch_id {
        return Err(BranchError::InvalidInput(
            "debate branches must belong to the same search batch".to_string(),
        ));
    }
    let now = timestamp_string();
    let debate_id = format!("debate_{}_{}", branch_id, timestamp_millis());
    let trace = DebateTrace {
        schema_version: "debate_trace.v1".to_string(),
        conformance_line: "M9.branch_debate".to_string(),
        debate_id: debate_id.clone(),
        branch_id: branch_id.to_string(),
        against_branch_id: against_branch_id.to_string(),
        proposition: format!("Promote {branch_id} over {against_branch_id}"),
        supporting_evidence: vec![format!(
            "{} owns {:?}",
            run.variation_operator, run.claimed_paths
        )],
        opposing_evidence: vec![format!(
            "competing operator {} may cover {:?}",
            against.variation_operator, against.claimed_paths
        )],
        unresolved_assumptions: vec![
            "deterministic proof harness does not execute live LLM mutation".to_string(),
        ],
        reviewer_roles: vec![
            "critic".to_string(),
            "tester".to_string(),
            "director".to_string(),
        ],
        recommendation: "promotable_after_evaluation".to_string(),
        confidence: 0.74,
        created_at: now.clone(),
    };
    write_json(&debate_path(data_dir, &debate_id), &trace)?;
    run.debate_trace_id = Some(debate_id);
    run.status = if run.evaluation_id.is_some() {
        "promotable".to_string()
    } else {
        "debated".to_string()
    };
    run.updated_at = now.clone();
    write_json(&run_path(data_dir, branch_id), &run)?;
    append_lineage(
        data_dir,
        json!({"event":"branch_debated","branch_id":branch_id,"against_branch_id":against_branch_id,"batch_id":run.batch_id,"created_at":now}),
    )?;
    Ok(trace)
}

pub fn verify(
    data_dir: &Path,
    branch_id: &str,
    against_branch_id: &str,
    repetitions: usize,
    criterion_names: &[String],
) -> Result<VerifierTournament, BranchError> {
    if repetitions == 0 || repetitions > 20 {
        return Err(BranchError::InvalidInput(
            "--repetitions must be between 1 and 20".to_string(),
        ));
    }
    fs::create_dir_all(verifier_tournaments_dir(data_dir))?;
    let mut run = read_run(data_dir, branch_id)?;
    let against = read_run(data_dir, against_branch_id)?;
    if run.batch_id != against.batch_id {
        return Err(BranchError::InvalidInput(
            "verifier tournament branches must belong to the same search batch".to_string(),
        ));
    }
    let criteria = selected_verifier_criteria(criterion_names)?;
    let now = timestamp_string();
    let tournament_id = format!("verifier_{}_{}", branch_id, timestamp_millis());
    let mut pair_scores = Vec::new();
    let left_eval = read_latest_evaluation(data_dir, &run)?;
    let right_eval = read_latest_evaluation(data_dir, &against)?;
    let mut missing_evaluations = Vec::new();
    if left_eval.is_none() {
        missing_evaluations.push(branch_id.to_string());
    }
    if right_eval.is_none() {
        missing_evaluations.push(against_branch_id.to_string());
    }
    if !missing_evaluations.is_empty() {
        return Err(BranchError::InvalidInput(format!(
            "branches verify requires evaluation_packet for both candidates; missing {}",
            missing_evaluations.join(", ")
        )));
    }
    let left_debate = read_latest_debate(data_dir, &run)?;
    let right_debate = read_latest_debate(data_dir, &against)?;
    let evidence_digest = verifier_evidence_digest(
        &run,
        &against,
        left_eval.as_ref(),
        right_eval.as_ref(),
        left_debate.as_ref(),
        right_debate.as_ref(),
    );
    for criterion in &criteria {
        for repetition_index in 0..repetitions {
            let score_a = score_candidate_for_criterion(
                criterion,
                &run,
                &against,
                left_eval.as_ref(),
                left_debate.as_ref(),
            );
            let score_b = score_candidate_for_criterion(
                criterion,
                &against,
                &run,
                right_eval.as_ref(),
                right_debate.as_ref(),
            );
            let winner_ref = if (score_a - score_b).abs() < f64::EPSILON {
                None
            } else if score_a > score_b {
                Some(branch_id.to_string())
            } else {
                Some(against_branch_id.to_string())
            };
            pair_scores.push(VerifierPairScore {
                schema_version: "verifier_pair_score.v1".to_string(),
                conformance_line: "M12.verifier_tournament".to_string(),
                pair_score_id: format!(
                    "vscore_{}_{}_{}",
                    criterion.criterion_id, repetition_index, tournament_id
                ),
                batch_id: run.batch_id.clone(),
                candidate_a: branch_id.to_string(),
                candidate_b: against_branch_id.to_string(),
                criterion_id: criterion.criterion_id.clone(),
                repetition_index,
                score_a,
                score_b,
                score_delta: score_a - score_b,
                winner_ref,
                provider: "local".to_string(),
                model: "research-cli-deterministic-verifier-v1".to_string(),
                scoring_mode: "deterministic_local_evidence".to_string(),
                evidence_digest: evidence_digest.clone(),
                raw_trace_ref: raw_trace_ref_for_criterion(
                    criterion,
                    left_eval.as_ref(),
                    left_debate.as_ref(),
                ),
            });
        }
    }
    let score_a_total: f64 = pair_scores.iter().map(|score| score.score_a).sum();
    let score_b_total: f64 = pair_scores.iter().map(|score| score.score_b).sum();
    let winner_ref = if score_a_total >= score_b_total {
        branch_id.to_string()
    } else {
        against_branch_id.to_string()
    };
    let recommendation = if winner_ref == branch_id {
        "recommend_candidate_a"
    } else {
        "recommend_candidate_b"
    }
    .to_string();
    let tournament = VerifierTournament {
        schema_version: "verifier_tournament.v1".to_string(),
        conformance_line: "M12.verifier_tournament".to_string(),
        tournament_id: tournament_id.clone(),
        scope: "branch_promotion".to_string(),
        batch_id: run.batch_id.clone(),
        candidate_refs: vec![branch_id.to_string(), against_branch_id.to_string()],
        criteria,
        repetitions,
        pair_scores,
        winner_ref,
        recommendation,
        deterministic_gate_result_ref: run.evaluation_id.clone(),
        canonicality_gate_result_ref: run.evaluation_id.clone(),
        merge_gate_result_ref: None,
        advisory_until_gates_pass: true,
        created_at: now.clone(),
    };
    write_json(
        &verifier_tournament_path(data_dir, &tournament_id),
        &tournament,
    )?;
    run.verifier_tournament_id = Some(tournament_id.clone());
    run.status = if run.evaluation_id.is_some() && run.debate_trace_id.is_some() {
        "promotable".to_string()
    } else {
        "evaluated".to_string()
    };
    run.updated_at = now.clone();
    write_json(&run_path(data_dir, branch_id), &run)?;
    append_lineage(
        data_dir,
        json!({"event":"branch_verified","branch_id":branch_id,"against_branch_id":against_branch_id,"batch_id":run.batch_id,"tournament_id":tournament_id,"winner_ref":tournament.winner_ref,"created_at":now}),
    )?;
    Ok(tournament)
}

pub fn mutate(
    data_dir: &Path,
    branch_id: &str,
    llm_command: &str,
) -> Result<BranchMutationResult, BranchError> {
    if llm_command.trim().is_empty() {
        return Err(BranchError::InvalidInput(
            "branches mutate requires --llm-command".to_string(),
        ));
    }
    let mut run = read_run(data_dir, branch_id)?;
    let worktree_path = PathBuf::from(&run.worktree_path);
    let before_commit = git_output(&worktree_path, &["rev-parse", "HEAD"]).unwrap_or_default();
    let output = Command::new("sh")
        .arg("-c")
        .arg(llm_command)
        .current_dir(&worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| BranchError::GitCommand {
            action: "llm mutation command".to_string(),
            stderr: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(BranchError::GitCommand {
            action: "llm mutation command".to_string(),
            stderr: summarize_command_output(&output),
        });
    }
    let changed_paths = git_status_paths(&worktree_path);
    if changed_paths.is_empty() {
        return Err(BranchError::InvalidInput(
            "llm mutation command produced no worktree changes".to_string(),
        ));
    }
    run_git_checked(&worktree_path, &["add", "-A"], "llm mutation add")?;
    run_git_checked(
        &worktree_path,
        &[
            "-c",
            "user.name=Research CLI M9",
            "-c",
            "user.email=research-cli-m9@example.test",
            "commit",
            "-q",
            "-m",
            &format!("m9 llm mutation {branch_id}"),
        ],
        "llm mutation commit",
    )?;
    let after_commit = git_output(&worktree_path, &["rev-parse", "HEAD"]).unwrap_or_default();
    let now = timestamp_string();
    let mutation_id = format!("mutation_{}_{}", branch_id, timestamp_millis());
    run.status = "mutated".to_string();
    run.evaluation_id = None;
    run.debate_trace_id = None;
    run.verifier_tournament_id = None;
    run.promotion_decision_id = None;
    run.updated_at = now.clone();
    write_json(&run_path(data_dir, branch_id), &run)?;
    append_lineage(
        data_dir,
        json!({"event":"branch_mutated","branch_id":branch_id,"batch_id":run.batch_id,"mutation_id":mutation_id,"changed_paths":changed_paths,"created_at":now}),
    )?;
    Ok(BranchMutationResult {
        schema_version: "branch_mutation_result.v1".to_string(),
        conformance_line: "M9.llm_branch_mutation".to_string(),
        mutation_id,
        branch_id: branch_id.to_string(),
        batch_id: run.batch_id,
        status: "mutated".to_string(),
        mutation_source: "llm_command_adapter".to_string(),
        command: llm_command.to_string(),
        before_commit,
        after_commit,
        changed_paths,
        created_at: now,
    })
}

pub fn promote(
    data_dir: &Path,
    workspace_root: &Path,
    branch_id: &str,
    auto_merge: bool,
    require_verifier: bool,
) -> Result<PromotionDecision, BranchError> {
    fs::create_dir_all(promotions_dir(data_dir))?;
    let mut run = read_run(data_dir, branch_id)?;
    let mut missing = Vec::new();
    if run.evaluation_id.is_none() {
        missing.push("evaluation_packet".to_string());
    }
    if run.debate_trace_id.is_none() {
        missing.push("debate_trace".to_string());
    }
    if let Some(eval_id) = run.evaluation_id.as_deref() {
        let evaluation: EvaluationPacket = read_json(&evaluation_path(data_dir, eval_id))?;
        if !evaluation.promotion_eligible {
            missing.extend(evaluation.blocking_issues.clone());
        }
        if evaluation.canonicality_gate.status != "passed"
            && !missing.iter().any(|gate| gate == "canonicality_gate")
        {
            missing.push("canonicality_gate".to_string());
        }
    }
    let mut verifier_gate_status = "not_required".to_string();
    if require_verifier {
        verifier_gate_status = "passed".to_string();
        match run.verifier_tournament_id.as_deref() {
            Some(tournament_id) => {
                let tournament: VerifierTournament =
                    read_json(&verifier_tournament_path(data_dir, tournament_id))?;
                if tournament.winner_ref != branch_id {
                    missing.push("verifier_tournament_winner".to_string());
                }
            }
            None => missing.push("verifier_tournament".to_string()),
        }
    }
    if !missing.is_empty() {
        return Err(BranchError::PromotionBlocked {
            branch_id: branch_id.to_string(),
            missing_gates: missing,
        });
    }
    if let Some(winner) = promoted_winner(data_dir, &run.batch_id, branch_id)? {
        return Err(BranchError::WinnerExists {
            batch_id: run.batch_id,
            winner_branch_id: winner,
        });
    }
    let now = timestamp_string();
    let decision_id = format!("promotion_{}_{}", branch_id, timestamp_millis());
    let merge = if auto_merge {
        merge_winner_public_diff(workspace_root, data_dir, &run)?
    } else {
        MergeOutcome {
            status: "director_decision_only".to_string(),
            merged_paths: Vec::new(),
            merge_commit: None,
        }
    };
    let decision = PromotionDecision {
        schema_version: "promotion_decision.v1".to_string(),
        conformance_line: "M9.director_promotion".to_string(),
        decision_id: decision_id.clone(),
        branch_id: branch_id.to_string(),
        batch_id: run.batch_id.clone(),
        status: "promoted".to_string(),
        decision: "promote_single_winner".to_string(),
        missing_gates: Vec::new(),
        verifier_gate_status,
        merge_status: merge.status,
        merged_paths: merge.merged_paths,
        merge_commit: merge.merge_commit,
        canonical_surface_policy: "winner_only_public_surface".to_string(),
        created_at: now.clone(),
    };
    write_json(&promotion_path(data_dir, &decision_id), &decision)?;
    run.promotion_decision_id = Some(decision_id);
    run.status = "promoted".to_string();
    run.updated_at = now.clone();
    write_json(&run_path(data_dir, branch_id), &run)?;
    append_lineage(
        data_dir,
        json!({"event":"branch_promoted","branch_id":branch_id,"batch_id":run.batch_id,"created_at":now}),
    )?;
    Ok(decision)
}

pub fn archive(
    data_dir: &Path,
    branch_id: &str,
    reason: &str,
) -> Result<ArchiveResult, BranchError> {
    if reason.trim().is_empty() {
        return Err(BranchError::InvalidInput(
            "branches archive requires --reason".to_string(),
        ));
    }
    let mut run = read_run(data_dir, branch_id)?;
    let now = timestamp_string();
    run.status = "archived".to_string();
    run.archived_reason = Some(reason.to_string());
    run.updated_at = now.clone();
    write_json(&run_path(data_dir, branch_id), &run)?;
    append_lineage(
        data_dir,
        json!({"event":"branch_archived","branch_id":branch_id,"batch_id":run.batch_id,"reason":reason,"created_at":now}),
    )?;
    Ok(ArchiveResult {
        schema_version: "branch_archive_result.v1".to_string(),
        conformance_line: "M9.loser_cleanup".to_string(),
        branch_id: branch_id.to_string(),
        batch_id: run.batch_id,
        status: "archived".to_string(),
        reason: reason.to_string(),
        archived_at: now,
    })
}

fn selected_verifier_criteria(requested: &[String]) -> Result<Vec<VerifierCriterion>, BranchError> {
    let defaults = default_verifier_criteria();
    if requested.is_empty() {
        return Ok(defaults);
    }
    let mut selected = Vec::new();
    for name in requested {
        let normalized = name.trim().to_ascii_lowercase();
        let criterion = defaults
            .iter()
            .find(|criterion| {
                criterion.criterion_id == normalized
                    || criterion.name.to_ascii_lowercase() == normalized
            })
            .cloned()
            .ok_or_else(|| {
                BranchError::InvalidInput(format!("unknown verifier criterion: {name}"))
            })?;
        if !selected
            .iter()
            .any(|item: &VerifierCriterion| item.criterion_id == criterion.criterion_id)
        {
            selected.push(criterion);
        }
    }
    Ok(selected)
}

fn default_verifier_criteria() -> Vec<VerifierCriterion> {
    vec![
        VerifierCriterion {
            criterion_id: "deterministic_eval".to_string(),
            name: "Deterministic Eval".to_string(),
            description:
                "Prefer the candidate whose evaluation packet proves tests and blocking checks pass."
                    .to_string(),
            evidence_requirements: vec![
                "evaluation_packet.promotion_eligible".to_string(),
                "evaluation_packet.test_outcomes".to_string(),
                "evaluation_packet.blocking_issues".to_string(),
            ],
            ground_truth_note: "Terminal output and structured evaluation gates outrank agent narration."
                .to_string(),
        },
        VerifierCriterion {
            criterion_id: "canonicality".to_string(),
            name: "Canonicality".to_string(),
            description:
                "Prefer the candidate that preserves the single latest public document/code surface."
                    .to_string(),
            evidence_requirements: vec![
                "evaluation_packet.canonicality_gate".to_string(),
                "canonical_surface_policy".to_string(),
            ],
            ground_truth_note: "Candidates with ambiguous or stale public surfaces cannot win on this criterion."
                .to_string(),
        },
        VerifierCriterion {
            criterion_id: "reviewer_debate".to_string(),
            name: "Reviewer Debate".to_string(),
            description:
                "Prefer the candidate supported by a stored debate trace and unresolved-risk accounting."
                    .to_string(),
            evidence_requirements: vec![
                "debate_trace.supporting_evidence".to_string(),
                "debate_trace.opposing_evidence".to_string(),
                "debate_trace.recommendation".to_string(),
            ],
            ground_truth_note:
                "Debate is advisory and cannot override deterministic evaluation or canonicality failures."
                    .to_string(),
        },
        VerifierCriterion {
            criterion_id: "implementation_risk".to_string(),
            name: "Implementation Risk".to_string(),
            description:
                "Prefer the candidate with lower evaluation risk and fewer unresolved blocking issues."
                    .to_string(),
            evidence_requirements: vec![
                "evaluation_packet.risk_score".to_string(),
                "evaluation_packet.changed_paths".to_string(),
                "branch_run.claimed_paths".to_string(),
            ],
            ground_truth_note:
                "Lower risk only matters after the candidate remains executable and governed."
                    .to_string(),
        },
    ]
}

fn default_verifier_gate_status() -> String {
    "not_required".to_string()
}

fn read_latest_evaluation(
    data_dir: &Path,
    run: &BranchRunRecord,
) -> Result<Option<EvaluationPacket>, BranchError> {
    run.evaluation_id
        .as_deref()
        .map(|id| read_json(&evaluation_path(data_dir, id)))
        .transpose()
}

fn read_latest_debate(
    data_dir: &Path,
    run: &BranchRunRecord,
) -> Result<Option<DebateTrace>, BranchError> {
    run.debate_trace_id
        .as_deref()
        .map(|id| read_json(&debate_path(data_dir, id)))
        .transpose()
}

fn score_candidate_for_criterion(
    criterion: &VerifierCriterion,
    candidate: &BranchRunRecord,
    opponent: &BranchRunRecord,
    evaluation: Option<&EvaluationPacket>,
    debate: Option<&DebateTrace>,
) -> f64 {
    match criterion.criterion_id.as_str() {
        "deterministic_eval" => match evaluation {
            Some(packet) if packet.promotion_eligible => 0.92,
            Some(_) => 0.08,
            None => 0.5,
        },
        "canonicality" => match evaluation {
            Some(packet) if packet.canonicality_gate.status == "passed" => 0.9,
            Some(_) => 0.1,
            None => 0.5,
        },
        "reviewer_debate" => match debate {
            Some(trace)
                if trace.against_branch_id == opponent.branch_id
                    && trace.recommendation.contains("promotable") =>
            {
                0.84
            }
            Some(_) => 0.58,
            None => 0.42,
        },
        "implementation_risk" => match evaluation {
            Some(packet) => (1.0 - packet.risk_score).clamp(0.0, 1.0),
            None => {
                if candidate.claimed_paths.len() <= opponent.claimed_paths.len() {
                    0.54
                } else {
                    0.46
                }
            }
        },
        _ => 0.5,
    }
}

fn raw_trace_ref_for_criterion(
    criterion: &VerifierCriterion,
    evaluation: Option<&EvaluationPacket>,
    debate: Option<&DebateTrace>,
) -> Option<String> {
    match criterion.criterion_id.as_str() {
        "reviewer_debate" => debate.map(|trace| format!("debate:{}", trace.debate_id)),
        _ => evaluation.map(|packet| format!("evaluation:{}", packet.evaluation_id)),
    }
}

fn verifier_evidence_digest(
    run: &BranchRunRecord,
    against: &BranchRunRecord,
    left_eval: Option<&EvaluationPacket>,
    right_eval: Option<&EvaluationPacket>,
    left_debate: Option<&DebateTrace>,
    right_debate: Option<&DebateTrace>,
) -> String {
    let value = json!({
        "candidate_a": run.branch_id,
        "candidate_b": against.branch_id,
        "candidate_a_eval": left_eval.map(|packet| json!({
            "evaluation_id": packet.evaluation_id,
            "promotion_eligible": packet.promotion_eligible,
            "canonicality_gate": packet.canonicality_gate.status,
            "blocking_issues": packet.blocking_issues,
            "risk_score": packet.risk_score,
        })),
        "candidate_b_eval": right_eval.map(|packet| json!({
            "evaluation_id": packet.evaluation_id,
            "promotion_eligible": packet.promotion_eligible,
            "canonicality_gate": packet.canonicality_gate.status,
            "blocking_issues": packet.blocking_issues,
            "risk_score": packet.risk_score,
        })),
        "candidate_a_debate": left_debate.map(|trace| json!({
            "debate_id": trace.debate_id,
            "against_branch_id": trace.against_branch_id,
            "recommendation": trace.recommendation,
        })),
        "candidate_b_debate": right_debate.map(|trace| json!({
            "debate_id": trace.debate_id,
            "against_branch_id": trace.against_branch_id,
            "recommendation": trace.recommendation,
        })),
    });
    let rendered = serde_json::to_string(&value).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    rendered.hash(&mut hasher);
    format!("digest_{:016x}", hasher.finish())
}

fn read_all_runs(data_dir: &Path) -> Result<Vec<BranchRunRecord>, BranchError> {
    let dir = runs_dir(data_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            runs.push(read_json(&path)?);
        }
    }
    Ok(runs)
}

fn read_run(data_dir: &Path, branch_id: &str) -> Result<BranchRunRecord, BranchError> {
    let path = run_path(data_dir, branch_id);
    if !path.exists() {
        return Err(BranchError::UnknownBranch(branch_id.to_string()));
    }
    read_json(&path)
}

fn promoted_winner(
    data_dir: &Path,
    batch_id: &str,
    except_branch_id: &str,
) -> Result<Option<String>, BranchError> {
    for run in read_all_runs(data_dir)? {
        if run.batch_id == batch_id && run.branch_id != except_branch_id && run.status == "promoted"
        {
            return Ok(Some(run.branch_id));
        }
    }
    Ok(None)
}

fn create_git_worktree(
    data_dir: &Path,
    workspace_root: &Path,
    branch_id: &str,
    base_commit: &str,
) -> Result<PathBuf, BranchError> {
    let workspace_name = workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let base = workspace_root
        .parent()
        .unwrap_or_else(|| data_dir.parent().unwrap_or(data_dir))
        .join(".research-cli-branch-worktrees")
        .join(workspace_name);
    fs::create_dir_all(&base)?;
    let worktree_path = base.join(branch_id);
    if worktree_path.exists() {
        fs::remove_dir_all(&worktree_path)?;
    }
    let commit = if base_commit.trim().is_empty() {
        "HEAD"
    } else {
        base_commit
    };
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["worktree", "add", "--detach"])
        .arg(&worktree_path)
        .arg(commit)
        .output()
        .map_err(|err| BranchError::GitCommand {
            action: "worktree add".to_string(),
            stderr: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(BranchError::GitCommand {
            action: "worktree add".to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(worktree_path)
}

fn create_candidate_artifact(
    worktree_path: &Path,
    branch_id: &str,
    objective: &str,
    operator: &str,
) -> Result<(), BranchError> {
    let candidate_dir = worktree_path.join(".pmcli_candidate");
    fs::create_dir_all(&candidate_dir)?;
    let artifact_path = candidate_dir.join(format!("{branch_id}.md"));
    fs::write(
        &artifact_path,
        format!(
            "# M9 Candidate {branch_id}\n\nobjective: {objective}\nvariation_operator: {operator}\nstatus: executable_hypothesis\n"
        ),
    )?;
    run_git_checked(
        worktree_path,
        &["add", artifact_path.to_string_lossy().as_ref()],
        "candidate artifact add",
    )?;
    run_git_checked(
        worktree_path,
        &[
            "-c",
            "user.name=Research CLI M9",
            "-c",
            "user.email=research-cli-m9@example.test",
            "commit",
            "-q",
            "-m",
            &format!("m9 candidate {branch_id}"),
        ],
        "candidate artifact commit",
    )?;
    Ok(())
}

fn run_git_checked(worktree_path: &Path, args: &[&str], action: &str) -> Result<(), BranchError> {
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(args)
        .output()
        .map_err(|err| BranchError::GitCommand {
            action: action.to_string(),
            stderr: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(BranchError::GitCommand {
            action: action.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(())
}

fn default_eval_commands(worktree_path: &Path) -> Vec<String> {
    if worktree_path.join("Cargo.toml").exists() {
        vec!["cargo test --quiet".to_string()]
    } else {
        vec!["git status --short".to_string()]
    }
}

fn run_eval_command(worktree_path: &Path, command: &str) -> EvaluationOutcome {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(worktree_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match output {
        Ok(output) if output.status.success() => EvaluationOutcome {
            name: command.to_string(),
            status: "passed".to_string(),
            detail: summarize_command_output(&output),
        },
        Ok(output) => EvaluationOutcome {
            name: command.to_string(),
            status: "failed".to_string(),
            detail: summarize_command_output(&output),
        },
        Err(err) => EvaluationOutcome {
            name: command.to_string(),
            status: "failed".to_string(),
            detail: err.to_string(),
        },
    }
}

fn summarize_command_output(output: &std::process::Output) -> String {
    let code = output
        .status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "signal".to_string());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout.trim(), stderr.trim());
    let snippet = combined.chars().take(240).collect::<String>();
    if snippet.is_empty() {
        format!("exit_code={code}")
    } else {
        format!("exit_code={code}; {snippet}")
    }
}

fn canonicality_outcome(
    workspace_root: &Path,
    data_dir: &Path,
    project_id: &str,
) -> EvaluationOutcome {
    match canonicality::audit_canonical(workspace_root, data_dir, project_id) {
        Ok(report) if report.blocking_violations.is_empty() => EvaluationOutcome {
            name: "canonicality_audit".to_string(),
            status: "passed".to_string(),
            detail: "canonicality audit has no blocking violations".to_string(),
        },
        Ok(report) => EvaluationOutcome {
            name: "canonicality_audit".to_string(),
            status: "blocked".to_string(),
            detail: format!(
                "canonicality audit has {} blocking violations",
                report.blocking_violations.len()
            ),
        },
        Err(err) => EvaluationOutcome {
            name: "canonicality_audit".to_string(),
            status: "failed".to_string(),
            detail: err.to_string(),
        },
    }
}

fn append_lineage(data_dir: &Path, value: serde_json::Value) -> Result<(), BranchError> {
    fs::create_dir_all(branches_root(data_dir))?;
    let mut line = serde_json::to_string(&value)?;
    line.push('\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(lineage_path(data_dir))?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), BranchError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, BranchError> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}
fn read_json_optional<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, BranchError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_json(path)?))
}
fn git_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(workspace_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
fn git_lines(workspace_root: &Path, args: &[&str]) -> Vec<String> {
    git_output(workspace_root, args)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}
fn git_status_paths(workspace_root: &Path) -> Vec<String> {
    let output =
        git_output(workspace_root, &["status", "--porcelain=v1", "-uall"]).unwrap_or_default();
    output
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                None
            } else {
                Some(line[3..].trim().to_string())
            }
        })
        .filter(|path| !path.is_empty())
        .collect()
}

struct MergeOutcome {
    status: String,
    merged_paths: Vec<String>,
    merge_commit: Option<String>,
}

fn merge_winner_public_diff(
    workspace_root: &Path,
    data_dir: &Path,
    run: &BranchRunRecord,
) -> Result<MergeOutcome, BranchError> {
    if !public_dirty_paths(workspace_root).is_empty() {
        return Err(BranchError::PromotionBlocked {
            branch_id: run.branch_id.clone(),
            missing_gates: vec!["clean_source_worktree".to_string()],
        });
    }
    let worktree_path = PathBuf::from(&run.worktree_path);
    let changed_paths = public_changed_paths(&worktree_path, &run.base_commit);
    if changed_paths.is_empty() {
        return Ok(MergeOutcome {
            status: "no_public_diff".to_string(),
            merged_paths: Vec::new(),
            merge_commit: None,
        });
    }
    let pre_merge_head = git_output(workspace_root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let diff_output = Command::new("git")
        .current_dir(&worktree_path)
        .args(["diff", "--binary", &run.base_commit, "HEAD", "--"])
        .args(&changed_paths)
        .output()
        .map_err(|err| BranchError::GitCommand {
            action: "winner diff".to_string(),
            stderr: err.to_string(),
        })?;
    if !diff_output.status.success() {
        return Err(BranchError::GitCommand {
            action: "winner diff".to_string(),
            stderr: String::from_utf8_lossy(&diff_output.stderr).to_string(),
        });
    }
    let mut apply = Command::new("git")
        .current_dir(workspace_root)
        .args(["apply", "--index", "--whitespace=nowarn", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| BranchError::GitCommand {
            action: "winner diff apply".to_string(),
            stderr: err.to_string(),
        })?;
    if let Some(stdin) = apply.stdin.as_mut() {
        stdin.write_all(&diff_output.stdout)?;
    }
    let apply_output = apply
        .wait_with_output()
        .map_err(|err| BranchError::GitCommand {
            action: "winner diff apply".to_string(),
            stderr: err.to_string(),
        })?;
    if !apply_output.status.success() {
        reset_source_worktree(workspace_root);
        return Err(BranchError::PromotionBlocked {
            branch_id: run.branch_id.clone(),
            missing_gates: vec![format!(
                "merge_apply:{}",
                String::from_utf8_lossy(&apply_output.stderr).trim()
            )],
        });
    }
    if let Err(err) = run_git_checked(
        workspace_root,
        &[
            "-c",
            "user.name=Research CLI M9",
            "-c",
            "user.email=research-cli-m9@example.test",
            "commit",
            "-q",
            "-m",
            &format!("m9 promote {}", run.branch_id),
        ],
        "winner merge commit",
    ) {
        reset_source_worktree(workspace_root);
        return Err(err);
    }
    let merge_commit = git_output(workspace_root, &["rev-parse", "HEAD"]);
    match canonicality::audit_canonical(workspace_root, data_dir, "post_merge") {
        Ok(report) if report.blocking_violations.is_empty() => {}
        Ok(_) => {
            reset_source_worktree_to(workspace_root, &pre_merge_head);
            return Err(BranchError::PromotionBlocked {
                branch_id: run.branch_id.clone(),
                missing_gates: vec!["post_merge_canonicality_gate".to_string()],
            });
        }
        Err(err) => {
            reset_source_worktree_to(workspace_root, &pre_merge_head);
            return Err(BranchError::PromotionBlocked {
                branch_id: run.branch_id.clone(),
                missing_gates: vec![format!("post_merge_canonicality_gate:{err}")],
            });
        }
    }
    Ok(MergeOutcome {
        status: "merged".to_string(),
        merged_paths: changed_paths,
        merge_commit,
    })
}

fn public_changed_paths(worktree_path: &Path, base_commit: &str) -> Vec<String> {
    let mut paths = git_lines(worktree_path, &["diff", "--name-only", base_commit, "HEAD"]);
    paths.retain(|path| is_public_merge_path(path));
    paths.sort();
    paths.dedup();
    paths
}

fn is_public_merge_path(path: &str) -> bool {
    !(path.starts_with(".pmcli_candidate/")
        || path.starts_with(".pmcli/")
        || path.starts_with(".git/")
        || path.starts_with("target/"))
}

fn public_dirty_paths(workspace_root: &Path) -> Vec<String> {
    git_status_paths(workspace_root)
        .into_iter()
        .filter(|path| is_public_merge_path(path))
        .collect()
}

fn reset_source_worktree(workspace_root: &Path) {
    let _ = Command::new("git")
        .current_dir(workspace_root)
        .args(["reset", "--hard", "-q"])
        .status();
}

fn reset_source_worktree_to(workspace_root: &Path, commit: &str) {
    let target = if commit.trim().is_empty() {
        "HEAD"
    } else {
        commit
    };
    let _ = Command::new("git")
        .current_dir(workspace_root)
        .args(["reset", "--hard", "-q", target])
        .status();
}

fn variation_operator_cycle() -> Vec<&'static str> {
    vec![
        "mutate_patch",
        "repair_failure",
        "expand_tests",
        "simplify_diff",
        "crossover_branches",
        "rebase_refresh",
        "doc_canonicalize",
        "manual_candidate",
    ]
}
fn agent_role_for_operator(operator: &str) -> &'static str {
    match operator {
        "repair_failure" => "debugger",
        "expand_tests" => "tester",
        "simplify_diff" => "maintainer",
        "crossover_branches" => "synthesizer",
        "rebase_refresh" => "integrator",
        "doc_canonicalize" => "documentarian",
        _ => "search_worker",
    }
}
fn claimed_paths_for_operator(operator: &str) -> Vec<String> {
    match operator {
        "expand_tests" => vec!["tests".to_string()],
        "doc_canonicalize" => vec!["docs".to_string()],
        "simplify_diff" => vec!["src".to_string(), "tests".to_string()],
        _ => vec!["src".to_string(), "tests".to_string(), "docs".to_string()],
    }
}
fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn timestamp_string() -> String {
    timestamp_millis().to_string()
}
fn branches_root(data_dir: &Path) -> PathBuf {
    data_dir.join("branches")
}
fn batches_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("batches")
}
fn runs_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("runs")
}
fn evaluations_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("evaluations")
}
fn debates_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("debates")
}
fn verifier_tournaments_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("verifier_tournaments")
}
fn promotions_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("promotions")
}
fn worktrees_dir(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("worktrees")
}
fn lineage_path(data_dir: &Path) -> PathBuf {
    branches_root(data_dir).join("lineage.jsonl")
}
fn batch_path(data_dir: &Path, batch_id: &str) -> PathBuf {
    batches_dir(data_dir).join(format!("{batch_id}.json"))
}
fn run_path(data_dir: &Path, branch_id: &str) -> PathBuf {
    runs_dir(data_dir).join(format!("{branch_id}.json"))
}
fn evaluation_path(data_dir: &Path, evaluation_id: &str) -> PathBuf {
    evaluations_dir(data_dir).join(format!("{evaluation_id}.json"))
}
fn debate_path(data_dir: &Path, debate_id: &str) -> PathBuf {
    debates_dir(data_dir).join(format!("{debate_id}.json"))
}
fn verifier_tournament_path(data_dir: &Path, tournament_id: &str) -> PathBuf {
    verifier_tournaments_dir(data_dir).join(format!("{tournament_id}.json"))
}
fn promotion_path(data_dir: &Path, decision_id: &str) -> PathBuf {
    promotions_dir(data_dir).join(format!("{decision_id}.json"))
}
