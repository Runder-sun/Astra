pub(crate) fn autonomous_research_review_readiness_blockers(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<String> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let mut blockers = Vec::new();
    let missing_task_types =
        accepted_worker_evidence_missing_required_task_types(&contract, index.as_ref());
    if !missing_task_types.is_empty() {
        blockers.push(format!(
            "missing accepted worker evidence for required task types: {}",
            missing_task_types.join(", ")
        ));
    }
    let quality_failures =
        accepted_worker_evidence_quality_floor_failures(&contract, index.as_ref());
    if !quality_failures.is_empty() {
        blockers.push(format!(
            "accepted worker quality floor failures: {}",
            quality_failures.join("; ")
        ));
    }
    let semantic_failures =
        accepted_worker_evidence_semantic_review_floor_failures(&contract, index.as_ref());
    if !semantic_failures.is_empty() {
        blockers.push(format!(
            "accepted worker semantic review floor failures: {}",
            semantic_failures.join("; ")
        ));
    }
    let active_tool_failures =
        accepted_worker_evidence_active_tool_floor_failures(&contract, index.as_ref());
    if !active_tool_failures.is_empty() {
        blockers.push(format!(
            "accepted worker active tool evidence failures: {}",
            active_tool_failures.join("; ")
        ));
    }
    if let Some(reason) =
        accepted_worker_evidence_missing_adoptable_stage_synthesis_reason(&contract, index.as_ref())
    {
        blockers.push(reason);
    }
    if !autonomous_research_stage_artifact_has_reviewable_content(resolved, job) {
        blockers.push(format!(
            "stage artifact is not reviewable at {}",
            contract.artifact_path
        ));
    } else if !autonomous_research_stage_artifact_has_materialization_receipt(
        resolved,
        job,
        &contract,
    ) {
        blockers.push(format!(
            "stage artifact at {} exists but was not materialized by adopt_stage_artifact",
            contract.artifact_path
        ));
    } else if !accepted_worker_evidence_bound_to_stage_artifact(
        resolved,
        job,
        &contract,
        index.as_ref(),
    ) {
        blockers.push(format!(
            "stage artifact at {} has no active adopt_stage_artifact receipt bound to main-agent-accepted research_synthesizer evidence",
            contract.artifact_path
        ));
    } else {
        let artifact = std::fs::read_to_string(resolved.workspace_root.join(&contract.artifact_path))
            .unwrap_or_default();
        let evidence_binding_failures =
            autonomous_research_stage_artifact_evidence_binding_failures(
                &artifact,
                &contract,
                index.as_ref(),
            );
        if !evidence_binding_failures.is_empty() {
            blockers.push(format!(
                "stage artifact evidence binding failures: {}",
                evidence_binding_failures.join("; ")
            ));
        }
    }
    if !autonomous_research_stage_plan_path(resolved, job, &contract).exists() {
        blockers.push("stage plan docframe is missing".to_string());
    }
    if !autonomous_research_stage_rubric_path(resolved, job, &contract).exists() {
        blockers.push("stage acceptance rubric docframe is missing".to_string());
    }
    blockers.sort();
    blockers.dedup();
    blockers
}

pub(crate) fn autonomous_research_tick_requires_main_agent_blocked_decision(
    job: &AutonomousResearchJobState,
    tick: &AutonomousResearchTickSummary,
) -> bool {
    tick.loop_status == "blocked"
        || tick.next_recommended_action == "resolve_blocked_goal_task"
        || !autonomous_research_open_blocking_obligations(job).is_empty()
}

pub(crate) fn autonomous_research_has_unresolved_worker_review_failures(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> bool {
    !autonomous_research_unresolved_worker_review_failures(resolved, job).is_empty()
}

pub(crate) fn autonomous_research_has_active_stage_running_worker_claims(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> bool {
    !autonomous_research_active_stage_running_worker_task_types(resolved, job).is_empty()
}

pub(crate) fn autonomous_research_active_stage_running_worker_task_types(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<String> {
    let Ok(Some(run)) = crate::orchestration::load_active_run(&resolved.data_dir) else {
        return Vec::new();
    };
    let contract = autonomous_research_stage_contract(resolved, job);
    let current_stage_execution_id = job.stage_execution_id.as_deref();
    let mut task_types = BTreeSet::new();
    for claim in crate::goals::active_goal_task_claims(&run) {
        if crate::goals::goal_task_claim_is_blocked(&run, &claim.entry_id, &claim.agent_id) {
            continue;
        }
        let Ok(reclaim_result) =
            crate::agents::reclaim_stale_running_agent(&resolved.data_dir, &claim.agent_id, 60_000)
        else {
            continue;
        };
        if reclaim_result.reclaimed {
            continue;
        }
        let agent_dir = resolved.data_dir.join("agents").join(&claim.agent_id);
        let runtime_path = agent_dir.join("runtime.json");
        let lifecycle_status = read_json_file_runtime(&runtime_path)
            .ok()
            .and_then(|value| json_string_field_runtime(&value, "lifecycle_status"))
            .unwrap_or_default();
        if lifecycle_status == "failed" || lifecycle_status == "stopped" {
            continue;
        }
        if !autonomous_research_agent_lifecycle_status_is_active(&lifecycle_status) {
            continue;
        }
        let task_packet_path = agent_dir.join("task_packet.json");
        let task_packet = read_json_file_runtime(&task_packet_path).ok();
        let stage_task = task_packet
            .as_ref()
            .and_then(|packet| packet.get("stage_task_contract"));
        let stage_execution_id =
            stage_task.and_then(|value| json_string_field_runtime(value, "stage_execution_id"));
        let stage_id = stage_task.and_then(|value| json_string_field_runtime(value, "stage_id"));
        let matches_current_stage = if let Some(execution_id) = current_stage_execution_id {
            stage_execution_id.as_deref() == Some(execution_id)
                || claim.entry_id.contains(execution_id)
        } else {
            stage_id.as_deref() == Some(contract.stage_id.as_str())
        };
        if !matches_current_stage {
            continue;
        }
        let task_type = stage_task
            .and_then(|value| json_string_field_runtime(value, "task_type"))
            .unwrap_or_else(|| task_type_from_task_id_runtime(&claim.entry_id));
        if !task_type.trim().is_empty() {
            task_types.insert(task_type);
        }
    }
    task_types.into_iter().collect()
}

pub(crate) fn autonomous_research_agent_lifecycle_status_is_active(status: &str) -> bool {
    matches!(status, "running" | "started" | "pending" | "queued")
}

pub(crate) fn autonomous_research_stage_has_dispatchable_worker_debt(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    missing_task_types: &[String],
) -> bool {
    if missing_task_types.is_empty() {
        return false;
    }
    autonomous_research_dispatchable_main_agent_stage_tasks(resolved, job)
        .iter()
        .any(|stage_task| {
            missing_task_types
                .iter()
                .any(|missing| missing == &stage_task.task_type)
        })
}

pub(crate) fn autonomous_research_dispatchable_main_agent_stage_task_ids(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<String> {
    autonomous_research_dispatchable_main_agent_stage_tasks(resolved, job)
        .into_iter()
        .map(|task| task.task_id)
        .collect()
}

pub(crate) fn autonomous_research_dispatchable_main_agent_stage_tasks(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<crate::goals::GoalStageTaskMetadata> {
    let Ok(status) = crate::goals::status(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    ) else {
        return Vec::new();
    };
    let current_stage_execution_id = job.stage_execution_id.as_deref();
    let mut seen = BTreeSet::new();
    let mut tasks = Vec::new();
    for entry in status.task_pool.entries {
        if entry.source.source_kind != "main_agent_published_task"
            || entry.bucket_id != "ready_to_run"
            || entry.action_policy.as_deref() != Some("allowed")
        {
            continue;
        }
        let Some(stage_task) = entry.stage_task else {
            continue;
        };
        if current_stage_execution_id
            .map(|execution_id| stage_task.stage_execution_id != execution_id)
            .unwrap_or(false)
        {
            continue;
        }
        if seen.insert(stage_task.task_id.clone()) {
            tasks.push(stage_task);
        }
    }
    tasks
}

pub(crate) fn autonomous_research_main_agent_reason(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    tick: &AutonomousResearchTickSummary,
) -> String {
    if job.ticks_completed == 1 {
        "first_tick".to_string()
    } else if tick.accepted {
        "agent_output_accepted".to_string()
    } else if tick.dispatch_count > 0 {
        "task_dispatched".to_string()
    } else if job
        .last_review
        .as_ref()
        .map(|review| review.verdict != "pass")
        .unwrap_or(false)
    {
        "failed_review_repair".to_string()
    } else if autonomous_research_has_pending_worker_artifact_decisions(resolved, job) {
        "pending_worker_artifact_decision".to_string()
    } else if autonomous_research_has_unresolved_worker_review_failures(resolved, job) {
        "worker_review_failure_routing".to_string()
    } else if !autonomous_research_open_blocking_obligations(job).is_empty() {
        "open_blocking_obligations".to_string()
    } else if job.phase == "review_readiness_blocked" {
        "review_readiness_blocked".to_string()
    } else if job.provider_rounds.is_empty() {
        "no_provider_round_yet".to_string()
    } else if !autonomous_research_stage_artifact_has_reviewable_content(resolved, job) {
        "missing_reviewable_stage_artifact".to_string()
    } else if autonomous_research_stalled_ticks(job)
        >= AUTONOMOUS_RESEARCH_STALLED_AGENT_TRIGGER_TICKS
    {
        "stalled_goal_loop".to_string()
    } else {
        "goal_loop_progress_only".to_string()
    }
}

pub(crate) fn autonomous_research_open_blocking_obligations(
    job: &AutonomousResearchJobState,
) -> Vec<AutonomousResearchObligation> {
    job.obligations
        .iter()
        .filter(|obligation| obligation.blocking && obligation.status == "open")
        .cloned()
        .collect()
}

pub(crate) fn autonomous_research_review_blocking_obligations(
    job: &AutonomousResearchJobState,
) -> Vec<AutonomousResearchObligation> {
    job.obligations
        .iter()
        .filter(|obligation| {
            obligation.blocking
                && matches!(
                    obligation.status.as_str(),
                    "open"
                        | "acknowledged_by_main_agent"
                        | "strategy_decided_by_main_agent"
                        | "converted_to_board_task"
                )
        })
        .cloned()
        .collect()
}

pub(crate) fn autonomous_research_active_blocking_obligations(
    job: &AutonomousResearchJobState,
) -> Vec<AutonomousResearchObligation> {
    job.obligations
        .iter()
        .filter(|obligation| {
            obligation.blocking && !autonomous_research_obligation_is_closed(&obligation.status)
        })
        .cloned()
        .collect()
}

pub(crate) fn autonomous_research_unresolved_blocking_obligation_count(
    job: &AutonomousResearchJobState,
) -> usize {
    job.obligations
        .iter()
        .filter(|obligation| {
            obligation.blocking
                && !matches!(
                    obligation.status.as_str(),
                    "satisfied"
                        | "superseded_by_rollback"
                        | "rejected_with_rationale"
                        | "human_gate_required"
                )
        })
        .count()
}

pub(crate) fn autonomous_research_obligation_id(
    job: &AutonomousResearchJobState,
    stage_id: &str,
    kind: &str,
    key: &str,
) -> String {
    format!(
        "obl_{}",
        short_sha256_hex(&format!("{}::{stage_id}::{kind}::{key}", job.job_id))
    )
}

pub(crate) fn migrate_autonomous_research_legacy_snapshot_obligations(
    job: &mut AutonomousResearchJobState,
) -> Vec<String> {
    let Some(review) = job
        .last_review
        .as_ref()
        .filter(|review| review.verdict != "pass")
    else {
        return Vec::new();
    };
    let snapshot_only = parse_autonomous_research_blocking_stage_gate_failures(
        &review.response_text,
    )
    .as_slice()
        == ["stage_artifact_adoption_snapshot"];
    if !snapshot_only {
        return Vec::new();
    }

    let now = timestamp_string();
    let migration_ref = format!(
        "resume_state_migration:legacy_snapshot_only:{}",
        review.review_id
    );
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "stage_gate_blocking_failure"
            && obligation.source == "review_gate"
            && obligation.blocking
            && obligation.review_id.as_deref() == Some(review.review_id.as_str())
            && obligation.failure_class.as_deref() == Some("literature_evidence_failure")
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "open".to_string();
        obligation.failure_class = Some("stage_artifact_adoption_snapshot_drift".to_string());
        obligation.satisfied_by = Some(format!("review_pass.{}", obligation.stage_id));
        obligation.missing_task_type = None;
        merge_unique_strings(&mut obligation.handled_by_refs, vec![migration_ref.clone()]);
        merge_unique_strings(
            &mut obligation.evidence_refs,
            vec![format!("review:{}", review.review_id)],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed.sort();
    changed.dedup();
    changed
}

pub(crate) fn autonomous_research_obligation_is_closed(status: &str) -> bool {
    matches!(
        status,
        "satisfied" | "superseded_by_rollback" | "rejected_with_rationale" | "human_gate_required"
    )
}

pub(crate) fn upsert_autonomous_research_obligation(
    job: &mut AutonomousResearchJobState,
    mut obligation: AutonomousResearchObligation,
) -> String {
    let now = timestamp_string();
    obligation.updated_at = now.clone();
    if let Some(existing) = job
        .obligations
        .iter_mut()
        .find(|existing| existing.obligation_id == obligation.obligation_id)
    {
        let existing_closed = autonomous_research_obligation_is_closed(&existing.status);
        let existing_decided = matches!(
            existing.status.as_str(),
            "strategy_decided_by_main_agent" | "converted_to_board_task"
        );
        let same_review =
            existing.review_id.is_some() && existing.review_id == obligation.review_id;
        if (existing_closed || existing_decided) && same_review {
            merge_unique_strings(&mut existing.evidence_refs, obligation.evidence_refs);
            existing.updated_at = now;
            return existing.obligation_id.clone();
        }
        existing.kind = obligation.kind;
        existing.stage_id = obligation.stage_id;
        existing.stage_execution_id = obligation.stage_execution_id;
        existing.source = obligation.source;
        existing.status = "open".to_string();
        existing.blocking = obligation.blocking;
        existing.required_by = obligation.required_by;
        existing.satisfied_by = obligation.satisfied_by;
        existing.review_id = obligation.review_id;
        existing.failure_class = obligation.failure_class;
        existing.missing_task_type = obligation.missing_task_type;
        existing.detail = obligation.detail;
        merge_unique_strings(&mut existing.evidence_refs, obligation.evidence_refs);
        existing.updated_at = now;
        return existing.obligation_id.clone();
    }
    let id = obligation.obligation_id.clone();
    job.obligations.push(obligation);
    id
}

pub(crate) fn autonomous_research_obligation_from_review_failure(
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
    kind: &str,
    key: &str,
    detail: String,
    missing_task_type: Option<String>,
) -> AutonomousResearchObligation {
    let now = timestamp_string();
    let obligation_id = autonomous_research_obligation_id(job, &contract.stage_id, kind, key);
    let failure_class = infer_failure_class(
        &review.response_text.to_ascii_lowercase(),
        &contract.stage_id,
    )
    .to_string();
    AutonomousResearchObligation {
        schema_version: "autonomous_research_obligation.v1".to_string(),
        obligation_id,
        kind: kind.to_string(),
        stage_id: contract.stage_id.clone(),
        stage_execution_id: job.stage_execution_id.clone(),
        source: "review_gate".to_string(),
        status: "open".to_string(),
        blocking: true,
        required_by: format!("stage_contract.{}", contract.stage_id),
        satisfied_by: missing_task_type
            .as_ref()
            .map(|task_type| format!("accepted_worker_evidence.{task_type}"))
            .or_else(|| Some(format!("review_pass.{}", contract.stage_id))),
        review_id: Some(review.review_id.clone()),
        failure_class: Some(failure_class),
        missing_task_type,
        detail,
        handled_by_refs: Vec::new(),
        evidence_refs: vec![
            format!("review:{}", review.review_id),
            contract.artifact_path.clone(),
        ],
        created_at: now.clone(),
        updated_at: now,
    }
}

pub(crate) fn parse_autonomous_research_blocking_stage_gate_failures(review_text: &str) -> Vec<String> {
    for line in review_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("FAIL blocking_stage_gate:") {
            return rest
                .split(',')
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect();
        }
    }
    Vec::new()
}

pub(crate) fn autonomous_research_review_detail_line(review_text: &str, check_name: &str) -> Option<String> {
    let prefix = format!("FAIL {check_name}_detail:");
    review_text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix(&prefix)
            .map(|rest| rest.trim().to_string())
    })
}

pub(crate) fn parse_missing_stage_task_types_from_review(review_text: &str) -> Vec<String> {
    let Some(detail) =
        autonomous_research_review_detail_line(review_text, "required_stage_task_coverage")
    else {
        return Vec::new();
    };
    let categories = detail
        .split("missing accepted stage-local task categories:")
        .nth(1)
        .unwrap_or(detail.as_str());
    categories
        .split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(crate) fn record_autonomous_research_obligations_from_review(
    job: &mut AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
) -> Vec<String> {
    if review.verdict == "pass" {
        return mark_autonomous_research_stage_obligations_satisfied_by_review(
            job, contract, review,
        );
    }

    let mut changed = Vec::new();
    for failure in parse_autonomous_research_blocking_stage_gate_failures(&review.response_text) {
        let detail = autonomous_research_review_detail_line(&review.response_text, &failure)
            .unwrap_or_else(|| format!("Stage gate `{failure}` failed."));
        let obligation = autonomous_research_obligation_from_review_failure(
            job,
            contract,
            review,
            "stage_gate_blocking_failure",
            &failure,
            detail,
            None,
        );
        changed.push(upsert_autonomous_research_obligation(job, obligation));
    }

    for missing_task_type in parse_missing_stage_task_types_from_review(&review.response_text) {
        let detail = format!(
            "Accepted stage-local worker evidence is missing required task type `{missing_task_type}`."
        );
        let key = format!("missing_task::{missing_task_type}");
        let obligation = autonomous_research_obligation_from_review_failure(
            job,
            contract,
            review,
            "missing_stage_evidence",
            &key,
            detail,
            Some(missing_task_type),
        );
        changed.push(upsert_autonomous_research_obligation(job, obligation));
    }

    if changed.is_empty() {
        let detail = compact_single_line(&review.response_text, 600);
        let obligation = autonomous_research_obligation_from_review_failure(
            job,
            contract,
            review,
            "failed_stage_review",
            &review.review_id,
            detail,
            None,
        );
        changed.push(upsert_autonomous_research_obligation(job, obligation));
    }
    changed.sort();
    changed.dedup();
    changed
}

pub(crate) fn mark_autonomous_research_stage_obligations_satisfied_by_review(
    job: &mut AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
) -> Vec<String> {
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.stage_id == contract.stage_id
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(format!("review_pass.{}", review.review_id));
        merge_unique_strings(
            &mut obligation.evidence_refs,
            vec![
                format!("review:{}", review.review_id),
                contract.artifact_path.clone(),
            ],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_obligations_satisfied_by_accepted_evidence(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
) -> Vec<String> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return Vec::new();
    };
    let accepted_task_types = accepted_worker_evidence_current_gate_task_types(Some(&index));
    let missing_required =
        accepted_worker_evidence_missing_required_task_types(&contract, Some(&index));
    let quality_floor_passes =
        accepted_worker_evidence_quality_floor_passes(&contract, Some(&index));
    let semantic_floor_passes =
        accepted_worker_evidence_semantic_review_floor_passes(&contract, Some(&index));
    let active_tool_floor_passes =
        accepted_worker_evidence_active_tool_floor_passes(&contract, Some(&index));
    let required_coverage_obligation_id = autonomous_research_obligation_id(
        job,
        &contract.stage_id,
        "stage_gate_blocking_failure",
        "required_stage_task_coverage",
    );
    let quality_obligation_id = autonomous_research_obligation_id(
        job,
        &contract.stage_id,
        "stage_gate_blocking_failure",
        "accepted_worker_evidence_quality_floor",
    );
    let semantic_obligation_id = autonomous_research_obligation_id(
        job,
        &contract.stage_id,
        "stage_gate_blocking_failure",
        "accepted_worker_semantic_review_floor",
    );
    let active_tool_obligation_id = autonomous_research_obligation_id(
        job,
        &contract.stage_id,
        "stage_gate_blocking_failure",
        "accepted_worker_active_tool_evidence_floor",
    );
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.stage_id == contract.stage_id
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        let satisfied = match obligation.kind.as_str() {
            "missing_stage_evidence" => obligation
                .missing_task_type
                .as_ref()
                .map(|task_type| {
                    accepted_task_types
                        .iter()
                        .any(|accepted| accepted == task_type)
                })
                .unwrap_or(false),
            "stage_gate_blocking_failure" => match obligation.detail.as_str() {
                _ if obligation.obligation_id == required_coverage_obligation_id => {
                    missing_required.is_empty()
                }
                _ if obligation.obligation_id == quality_obligation_id => quality_floor_passes,
                _ if obligation.obligation_id == semantic_obligation_id => semantic_floor_passes,
                _ if obligation.obligation_id == active_tool_obligation_id => {
                    active_tool_floor_passes
                }
                _ => false,
            },
            _ => false,
        };
        if satisfied {
            obligation.status = "satisfied".to_string();
            obligation.satisfied_by = obligation
                .satisfied_by
                .clone()
                .or_else(|| Some("accepted_worker_evidence".to_string()));
            merge_unique_strings(
                &mut obligation.evidence_refs,
                vec![format!(
                    "accepted_worker_evidence_index:{}",
                    index.stage_execution_id
                )],
            );
            obligation.updated_at = now.clone();
            changed.push(obligation.obligation_id.clone());
        }
    }
    changed
}

pub(crate) fn stage_artifact_review_gate_passes(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    gate_name: &str,
) -> bool {
    let path = resolved.workspace_root.join(&contract.artifact_path);
    let artifact = std::fs::read_to_string(path).unwrap_or_default();
    let lower = artifact.to_ascii_lowercase();
    match gate_name {
        "artifact_exists" => !artifact.trim().is_empty(),
        "required_fields" => {
            let required_fields_present = contract
                .required_fields
                .iter()
                .filter(|field| {
                    lower.contains(&field.to_ascii_lowercase())
                        || lower.contains(&field.replace(' ', "_").to_ascii_lowercase())
                })
                .count();
            required_fields_present >= contract.required_fields.len().min(4)
        }
        "accepted_worker_evidence_cited" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            accepted_worker_evidence_bound_to_stage_artifact(resolved, job, contract, index.as_ref())
        }
        "stage_artifact_adoption_snapshot" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            autonomous_research_stage_artifact_adoption_snapshot_failure(
                resolved,
                job,
                contract,
                index.as_ref(),
            )
            .is_none()
        }
        "required_stage_task_coverage" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            accepted_worker_evidence_covers_required_task_types(contract, index.as_ref())
        }
        "accepted_worker_evidence_quality_floor" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            accepted_worker_evidence_quality_floor_passes(contract, index.as_ref())
        }
        "accepted_worker_semantic_review_floor" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            accepted_worker_evidence_semantic_review_floor_passes(contract, index.as_ref())
        }
        "accepted_worker_active_tool_evidence_floor" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            accepted_worker_evidence_active_tool_floor_passes(contract, index.as_ref())
        }
        "stage_artifact_evidence_binding" => {
            let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
            autonomous_research_stage_artifact_evidence_binding_failures(
                &artifact,
                contract,
                index.as_ref(),
            )
            .is_empty()
        }
        "required_upstream_stage_artifacts" => {
            autonomous_research_missing_required_upstream_artifacts(resolved, job, contract)
                .is_empty()
        }
        "stage_artifact_docframe" => {
            if contract.artifact_path.ends_with(".md") {
                autonomous_research_doc_has_valid_doc_frame(resolved, &contract.artifact_path).0
            } else {
                true
            }
        }
        "paper_source_manifest_docframe" => {
            if contract.stage_id == "paper-write" {
                let manifest_ref = relative_workspace_ref(
                    &resolved.workspace_root,
                    &autonomous_research_paper_source_manifest_path(resolved, job),
                );
                autonomous_research_doc_has_valid_doc_frame(resolved, &manifest_ref).0
            } else {
                true
            }
        }
        "tex_not_markdown_only" => {
            artifact.contains("\\documentclass") || artifact.contains("\\section")
        }
        "pdf_bundle" => {
            let pdf_path = Path::new(&contract.artifact_path)
                .parent()
                .map(|parent| resolved.workspace_root.join(parent))
                .unwrap_or_else(|| resolved.workspace_root.clone())
                .join("build")
                .join("main.pdf");
            lower.contains(".pdf")
                && lower.contains("build log")
                && lower.contains("pdf validation result")
                && pdf_path.exists()
                && !lower.contains("fail: pdf is missing")
        }
        _ => false,
    }
}

pub(crate) fn mark_autonomous_research_obligations_satisfied_by_stage_artifact_repair(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    adopted_stage_refs: &[String],
) -> Vec<String> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return Vec::new();
    };
    let stage_artifact_path = resolved.workspace_root.join(&contract.artifact_path);
    let Ok(_stage_artifact) = std::fs::read_to_string(&stage_artifact_path) else {
        return Vec::new();
    };
    if !autonomous_research_stage_artifact_has_reviewable_content(resolved, job)
        || !accepted_worker_evidence_bound_to_stage_artifact(resolved, job, &contract, Some(&index))
        || !accepted_worker_evidence_covers_required_task_types(&contract, Some(&index))
        || !accepted_worker_evidence_quality_floor_passes(&contract, Some(&index))
        || !accepted_worker_evidence_semantic_review_floor_passes(&contract, Some(&index))
        || !accepted_worker_evidence_active_tool_floor_passes(&contract, Some(&index))
        || !autonomous_research_stage_artifact_evidence_binding_failures(
            &_stage_artifact,
            &contract,
            Some(&index),
        )
        .is_empty()
    {
        return Vec::new();
    }

    let now = timestamp_string();
    let mut changed = Vec::new();
    let stage_refs = vec![
        format!("stage_artifact:{}", contract.artifact_path),
        format!(
            "accepted_worker_evidence_index:{}",
            index.stage_execution_id
        ),
    ];
    let main_agent_decision_refs = index
        .entries
        .iter()
        .filter_map(|entry| entry.main_agent_decision_ref.clone())
        .collect::<Vec<_>>();
    let has_main_agent_decision = !main_agent_decision_refs.is_empty();
    let has_adoption = !adopted_stage_refs.is_empty()
        || job
            .artifact_refs
            .iter()
            .any(|reference| reference.contains("stage_artifact_adoptions"));
    if !has_adoption {
        return Vec::new();
    }
    let stage_gate_statuses = [
        "artifact_exists",
        "required_fields",
        "accepted_worker_evidence_cited",
        "stage_artifact_adoption_snapshot",
        "required_stage_task_coverage",
        "accepted_worker_evidence_quality_floor",
        "accepted_worker_semantic_review_floor",
        "accepted_worker_active_tool_evidence_floor",
        "stage_artifact_evidence_binding",
        "required_upstream_stage_artifacts",
        "stage_artifact_docframe",
        "paper_source_manifest_docframe",
        "tex_not_markdown_only",
        "pdf_bundle",
    ]
    .into_iter()
    .map(|gate| {
        (
            gate,
            autonomous_research_obligation_id(
                job,
                &contract.stage_id,
                "stage_gate_blocking_failure",
                gate,
            ),
            stage_artifact_review_gate_passes(resolved, job, &contract, gate),
        )
    })
    .collect::<Vec<_>>();

    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "stage_gate_blocking_failure"
            && obligation.stage_id == contract.stage_id
            && matches!(
                obligation.status.as_str(),
                "open" | "acknowledged_by_main_agent" | "strategy_decided_by_main_agent"
            )
    }) {
        let inferred_gate = stage_gate_statuses
            .iter()
            .find_map(|(gate, obligation_id, passes)| {
                (obligation.obligation_id == *obligation_id).then_some((*gate, *passes))
            })
            .or_else(|| {
                if obligation.detail.contains("required_fields")
                    || obligation.detail.contains("Required fields")
                    || obligation.detail.contains("Stage gate `required_fields`")
                {
                    Some("required_fields")
                } else if obligation.detail.contains("accepted_worker_evidence_cited") {
                    Some("accepted_worker_evidence_cited")
                } else if obligation.detail.contains("stage_artifact_docframe") {
                    Some("stage_artifact_docframe")
                } else if obligation.detail.contains("paper_source_manifest_docframe") {
                    Some("paper_source_manifest_docframe")
                } else if obligation
                    .detail
                    .contains("required_upstream_stage_artifacts")
                {
                    Some("required_upstream_stage_artifacts")
                } else if obligation.detail.contains("tex_not_markdown_only") {
                    Some("tex_not_markdown_only")
                } else if obligation.detail.contains("pdf_bundle") {
                    Some("pdf_bundle")
                } else {
                    None
                }
                .and_then(|gate| {
                    stage_gate_statuses
                        .iter()
                        .find_map(|(known_gate, _, passes)| {
                            (*known_gate == gate).then_some((gate, *passes))
                        })
                })
            });
        let gate_passes = inferred_gate.map(|(_, passes)| passes).unwrap_or(false);
        if !gate_passes {
            continue;
        }
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some("stage_artifact_repair_ready_for_review".to_string());
        merge_unique_strings(&mut obligation.evidence_refs, stage_refs.clone());
        merge_unique_strings(&mut obligation.evidence_refs, adopted_stage_refs.to_vec());
        merge_unique_strings(
            &mut obligation.handled_by_refs,
            main_agent_decision_refs.clone(),
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }

    let failed_review_repairs = job
        .obligations
        .iter()
        .filter(|obligation| {
            obligation.kind == "failed_stage_review"
                && obligation.stage_id == contract.stage_id
                && matches!(
                    obligation.status.as_str(),
                    "open" | "acknowledged_by_main_agent" | "strategy_decided_by_main_agent"
                )
        })
        .filter_map(|obligation| {
            let repair_refs = stage_artifact_repair_adoption_refs_after(
                resolved,
                job,
                &contract,
                &index,
                &obligation.created_at,
            );
            (!repair_refs.is_empty()).then_some((obligation.obligation_id.clone(), repair_refs))
        })
        .collect::<Vec<_>>();

    for (obligation_id, repair_refs) in failed_review_repairs {
        if let Some(obligation) = job
            .obligations
            .iter_mut()
            .find(|obligation| obligation.obligation_id == obligation_id)
        {
            obligation.status = "satisfied".to_string();
            obligation.satisfied_by = Some("stage_artifact_repair_ready_for_review".to_string());
            merge_unique_strings(&mut obligation.evidence_refs, stage_refs.clone());
            merge_unique_strings(&mut obligation.evidence_refs, adopted_stage_refs.to_vec());
            merge_unique_strings(&mut obligation.evidence_refs, repair_refs.clone());
            merge_unique_strings(
                &mut obligation.handled_by_refs,
                main_agent_decision_refs.clone(),
            );
            merge_unique_strings(&mut obligation.handled_by_refs, repair_refs);
            obligation.updated_at = now.clone();
            changed.push(obligation.obligation_id.clone());
        }
    }

    if has_main_agent_decision && !changed.is_empty() {
        let stalled_refs = {
            let mut refs = stage_refs.clone();
            merge_unique_strings(&mut refs, adopted_stage_refs.to_vec());
            merge_unique_strings(&mut refs, main_agent_decision_refs.clone());
            refs
        };
        for obligation in job.obligations.iter_mut().filter(|obligation| {
            obligation.kind == "main_agent_stalled"
                && obligation.source == "supervisor_gate"
                && matches!(
                    obligation.status.as_str(),
                    "open" | "acknowledged_by_main_agent" | "strategy_decided_by_main_agent"
                )
        }) {
            obligation.status = "satisfied".to_string();
            obligation.satisfied_by =
                Some("main_agent_stall_recovered_by_stage_artifact_repair".to_string());
            merge_unique_strings(&mut obligation.evidence_refs, stalled_refs.clone());
            merge_unique_strings(
                &mut obligation.handled_by_refs,
                main_agent_decision_refs.clone(),
            );
            obligation.updated_at = now.clone();
            changed.push(obligation.obligation_id.clone());
        }
    }

    changed.sort();
    changed.dedup();
    changed
}

fn stage_artifact_repair_adoption_refs_after(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    baseline_created_at: &str,
) -> Vec<String> {
    let mut refs = Vec::new();
    let receipts = autonomous_research_stage_artifact_active_adoption_receipts(resolved, job, contract);
    for record in receipts {
        if !index
            .entries
            .iter()
            .any(|entry| accepted_worker_evidence_entry_supports_adoption(entry, &record))
        {
            continue;
        }
        let manifest_path = main_agent_stage_artifact_adoption_manifest_path(resolved, job, &record);
        let manifest = read_json_file_runtime(&manifest_path).unwrap_or(Value::Null);
        let manifest_created_at = json_string_field_runtime(&manifest, "created_at");
        let happened_after_baseline = manifest_created_at
            .as_deref()
            .map(|created_at| timestamp_string_is_after(created_at, baseline_created_at))
            .unwrap_or_else(|| timestamp_string_is_after(&record.created_at, baseline_created_at));
        if !happened_after_baseline {
            continue;
        }
        merge_unique_strings(
            &mut refs,
            vec![
                record.adoption_ref.clone(),
                relative_workspace_ref(&resolved.workspace_root, &manifest_path),
                autonomous_research_stage_adoption_target_ref(contract, &record),
            ],
        );
    }
    refs
}

pub(crate) fn mark_autonomous_research_obligations_acknowledged_by_main_agent(
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
) -> Vec<String> {
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job
        .obligations
        .iter_mut()
        .filter(|obligation| obligation.blocking && obligation.status == "open")
    {
        let explicit_reference = round_content.contains(&obligation.obligation_id)
            || (round_content.to_ascii_lowercase().contains("obligation")
                && round_content.contains(&obligation.kind)
                && round_content.contains(&obligation.stage_id));
        if !explicit_reference {
            continue;
        }
        obligation.status = "acknowledged_by_main_agent".to_string();
        merge_unique_strings(
            &mut obligation.handled_by_refs,
            vec![format!("main_agent_round:{}", round.artifact_path)],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_provider_fault_obligations_satisfied_by_main_agent_round(
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
) -> Vec<String> {
    let lowered = round_content.to_ascii_lowercase();
    let has_provider_fault_decision =
        lowered.contains("provider fault") || lowered.contains("provider_fault");
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "provider_fault"
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        let explicit_reference = round_content.contains(&obligation.obligation_id)
            || round_content.contains(&obligation.kind)
            || obligation
                .failure_class
                .as_ref()
                .is_some_and(|failure_class| round_content.contains(failure_class));
        if !explicit_reference && !has_provider_fault_decision {
            continue;
        }
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(format!(
            "main_agent_provider_round_completed:{}",
            round.artifact_path
        ));
        merge_unique_strings(
            &mut obligation.handled_by_refs,
            vec![format!("main_agent_round:{}", round.artifact_path)],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_provider_fault_obligations_satisfied_by_operator_release(
    job: &mut AutonomousResearchJobState,
    fault: &AutonomousResearchProviderFaultState,
) -> Vec<String> {
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "provider_fault"
            && obligation.source == fault.source
            && obligation.failure_class.as_deref() == Some(fault.category.as_str())
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(format!("provider_fault_released:{}", fault.fault_id));
        merge_unique_strings(
            &mut obligation.evidence_refs,
            vec![format!("provider_fault:{}", fault.fault_id)],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_provider_fault_obligations_satisfied_by_provider_route(
    job: &mut AutonomousResearchJobState,
    fault: &AutonomousResearchProviderFaultState,
    round: &AutonomousResearchAgentRoundSummary,
) -> Vec<String> {
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "provider_fault"
            && obligation.source == fault.source
            && obligation.failure_class.as_deref() == Some(fault.category.as_str())
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(format!(
            "provider_fault_recovered_by_main_agent_round:{}",
            round.artifact_path
        ));
        merge_unique_strings(
            &mut obligation.evidence_refs,
            vec![
                format!("provider_fault:{}", fault.fault_id),
                format!("main_agent_round:{}", round.artifact_path),
            ],
        );
        merge_unique_strings(
            &mut obligation.handled_by_refs,
            vec![format!("main_agent_round:{}", round.artifact_path)],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_provider_fault_obligations_satisfied_by_runtime_recovery(
    job: &mut AutonomousResearchJobState,
    fault: &AutonomousResearchProviderFaultState,
    satisfied_by: String,
    evidence_refs: Vec<String>,
    handled_by_refs: Vec<String>,
) -> Vec<String> {
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "provider_fault"
            && obligation.source == fault.source
            && obligation.failure_class.as_deref() == Some(fault.category.as_str())
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(satisfied_by.clone());
        merge_unique_strings(&mut obligation.evidence_refs, evidence_refs.clone());
        merge_unique_strings(&mut obligation.handled_by_refs, handled_by_refs.clone());
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn clear_active_agent_team_provider_fault_after_worker_acceptance(
    job: &mut AutonomousResearchJobState,
    acceptance: &GoalTaskAcceptanceResult,
) -> Option<(
    AutonomousResearchProviderFaultState,
    Vec<String>,
    Vec<String>,
)> {
    let fault = job.active_provider_fault.clone()?;
    if fault.source != "agent_team_worker"
        || acceptance.status != "accepted"
        || acceptance.provider_failure.is_some()
        || !acceptance
            .stage_task_acceptance
            .as_ref()
            .map(|stage_task| stage_task.verdict == "accepted")
            .unwrap_or(false)
    {
        return None;
    }
    let mut evidence_refs = vec![
        format!("provider_fault:{}", fault.fault_id),
        acceptance.task_packet_ref.clone(),
        acceptance.output_manifest_ref.clone(),
    ];
    merge_unique_strings(&mut evidence_refs, acceptance.trace_refs.clone());
    let handled_by_refs = vec![format!("agent_team_retry:{}", acceptance.agent_id)];
    let obligation_ids =
        mark_autonomous_research_provider_fault_obligations_satisfied_by_runtime_recovery(
            job,
            &fault,
            format!(
                "provider_fault_recovered_by_agent_team_retry:{}",
                acceptance.agent_id
            ),
            evidence_refs.clone(),
            handled_by_refs,
        );
    job.active_provider_fault = None;
    if job.last_error.as_deref() == Some(fault.message.as_str()) {
        job.last_error = None;
    }
    if job
        .stop_reason
        .as_deref()
        .map(|reason| reason.contains("provider"))
        .unwrap_or(false)
    {
        job.stop_reason = None;
    }
    job.updated_at = timestamp_string();
    Some((fault, obligation_ids, evidence_refs))
}

pub(crate) fn clear_active_agent_team_provider_fault_after_existing_worker_evidence(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
) -> Option<(
    AutonomousResearchProviderFaultState,
    Vec<String>,
    Vec<String>,
)> {
    let fault = job.active_provider_fault.clone()?;
    if fault.source != "agent_team_worker" {
        return None;
    }
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job)?;
    let mut evidence_refs = vec![format!("provider_fault:{}", fault.fault_id)];
    let mut handled_by_refs = Vec::new();
    for entry in index.entries.iter() {
        let Some(created_at) = entry.created_at.as_deref() else {
            continue;
        };
        if !timestamp_string_is_after(created_at, &fault.created_at)
            && created_at != fault.created_at
        {
            continue;
        }
        merge_unique_strings(
            &mut evidence_refs,
            vec![
                entry.task_packet_ref.clone(),
                entry.output_manifest_ref.clone(),
            ],
        );
        merge_unique_strings(&mut evidence_refs, entry.evidence_refs.clone());
        merge_unique_strings(
            &mut handled_by_refs,
            vec![format!("accepted_worker_evidence:{}", entry.agent_id)],
        );
    }
    if handled_by_refs.is_empty() {
        return None;
    }
    let obligation_ids =
        mark_autonomous_research_provider_fault_obligations_satisfied_by_runtime_recovery(
            job,
            &fault,
            format!(
                "provider_fault_recovered_by_accepted_worker_evidence:{}",
                index.stage_execution_id
            ),
            evidence_refs.clone(),
            handled_by_refs,
        );
    job.active_provider_fault = None;
    if job.last_error.as_deref() == Some(fault.message.as_str()) {
        job.last_error = None;
    }
    if job
        .stop_reason
        .as_deref()
        .map(|reason| reason.contains("provider"))
        .unwrap_or(false)
    {
        job.stop_reason = None;
    }
    job.updated_at = timestamp_string();
    Some((fault, obligation_ids, evidence_refs))
}

pub(crate) fn mark_autonomous_research_failed_review_obligations_strategy_decided_by_main_agent(
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
) -> Vec<String> {
    if round.execution_mode != "agent_loop" {
        return Vec::new();
    }
    let lowered = round_content.to_ascii_lowercase();
    if !autonomous_research_main_agent_decision_contract_present(&lowered) {
        return Vec::new();
    }
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "failed_stage_review"
            && obligation.source == "review_gate"
            && obligation.blocking
            && matches!(
                obligation.status.as_str(),
                "open" | "acknowledged_by_main_agent"
            )
    }) {
        let explicit_reference = round_content.contains(&obligation.obligation_id)
            || obligation
                .review_id
                .as_ref()
                .is_some_and(|review_id| round_content.contains(review_id))
            || (lowered.contains("failed_stage_review")
                && lowered.contains(&obligation.stage_id.to_ascii_lowercase()));
        if !explicit_reference {
            continue;
        }
        obligation.status = "strategy_decided_by_main_agent".to_string();
        merge_unique_strings(
            &mut obligation.handled_by_refs,
            vec![format!(
                "main_agent_decision_contract:{}",
                round.artifact_path
            )],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_obligations_strategy_decided_from_structured_records(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
) -> Vec<String> {
    let Ok(Some(run)) = crate::orchestration::load_active_run(&resolved.data_dir) else {
        return Vec::new();
    };
    let mut decision_refs = Vec::new();
    for step in &run.steps {
        for artifact in &step.artifacts {
            if let Some(decision_ref) = artifact.strip_prefix("main_agent_obligation_decision::") {
                decision_refs.push(decision_ref.to_string());
            }
        }
    }
    if decision_refs.is_empty() {
        return Vec::new();
    }
    let now = timestamp_string();
    let mut changed = Vec::new();
    for decision_ref in decision_refs {
        let decision_path = resolved
            .data_dir
            .join("main-agent-board")
            .join("obligation-decisions")
            .join(format!("{decision_ref}.json"));
        let Ok(content) = std::fs::read_to_string(&decision_path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(obligation_id) = value.get("obligation_id").and_then(|value| value.as_str())
        else {
            continue;
        };
        let Some(route) = value.get("route").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(evidence_standard) = value
            .get("evidence_standard")
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let cleanup_required = value
            .get("cleanup_required")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let koujing_change = value
            .get("koujing_change")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let task_refs = json_string_array_runtime(&value, "task_refs")
            .into_iter()
            .map(|task_ref| format!("goal_task_pool_entry:{task_ref}"))
            .collect::<Vec<_>>();
        let readiness_refs = json_string_array_runtime(&value, "readiness_refs")
            .into_iter()
            .map(|readiness_ref| format!("readiness_ref:{readiness_ref}"))
            .collect::<Vec<_>>();
        for obligation in job.obligations.iter_mut().filter(|obligation| {
            obligation.obligation_id == obligation_id
                && obligation.blocking
                && (matches!(
                    obligation.status.as_str(),
                    "open" | "acknowledged_by_main_agent"
                ) || (obligation.kind == "provider_fault"
                    && obligation.status == "strategy_decided_by_main_agent"))
        }) {
            obligation.status = if obligation.kind == "provider_fault" {
                "satisfied".to_string()
            } else {
                "strategy_decided_by_main_agent".to_string()
            };
            obligation.satisfied_by = Some(format!("structured_main_agent_decision:{route}"));
            merge_unique_strings(
                &mut obligation.handled_by_refs,
                vec![format!("main_agent_obligation_decision:{decision_ref}")],
            );
            merge_unique_strings(&mut obligation.handled_by_refs, task_refs.clone());
            merge_unique_strings(
                &mut obligation.evidence_refs,
                vec![
                    format!("main_agent_round:{}", round.artifact_path),
                    format!("main_agent_obligation_decision:{decision_ref}"),
                    format!("evidence_standard:{evidence_standard}"),
                ],
            );
            merge_unique_strings(&mut obligation.evidence_refs, readiness_refs.clone());
            if cleanup_required || koujing_change {
                merge_unique_strings(
                    &mut obligation.handled_by_refs,
                    vec!["cleanup_required_by_main_agent_decision".to_string()],
                );
            }
            obligation.updated_at = now.clone();
            changed.push(obligation.obligation_id.clone());
        }
    }
    changed
}

pub(crate) fn json_string_array_runtime(value: &serde_json::Value, key: &str) -> Vec<String> {
    json_value_string_array_runtime(value.get(key))
}

pub(crate) fn json_value_string_array_runtime(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .filter(|item| !item.trim().is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn autonomous_research_main_agent_decision_contract_present(lowered_round_content: &str) -> bool {
    let required = [
        "mainagentobligationdecisioncontract",
        "obligation id",
        "source review id",
        "failure class",
        "root-cause diagnosis",
        "strategy change",
        "chosen route",
        "evidence standard",
        "next review condition",
    ];
    let alternate_header = lowered_round_content
        .contains("main agent obligation decision contract")
        || lowered_round_content.contains("main-agent obligation decision contract");
    (lowered_round_content.contains(required[0]) || alternate_header)
        && required[1..]
            .iter()
            .all(|needle| lowered_round_content.contains(needle))
}

pub(crate) fn mark_autonomous_research_obligations_converted_to_board_tasks_by_main_agent(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
) -> Vec<String> {
    let Ok(status) = crate::goals::status(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    ) else {
        return Vec::new();
    };
    let existing_task_ids = status
        .task_pool
        .entries
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect::<Vec<_>>();
    mark_autonomous_research_obligations_converted_to_board_tasks_from_entry_ids(
        job,
        round,
        round_content,
        &existing_task_ids,
    )
}

#[cfg(test)]
pub(crate) fn mark_autonomous_research_obligations_converted_to_board_tasks_by_main_agent_with_entries(
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
    existing_task_ids: &[String],
) -> Vec<String> {
    mark_autonomous_research_obligations_converted_to_board_tasks_from_entry_ids(
        job,
        round,
        round_content,
        existing_task_ids,
    )
}

pub(crate) fn mark_autonomous_research_obligations_converted_to_board_tasks_from_entry_ids(
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
    existing_task_ids: &[String],
) -> Vec<String> {
    if existing_task_ids.is_empty() {
        return Vec::new();
    }
    let lowered = round_content.to_ascii_lowercase();
    let has_board_conversion_language = lowered.contains("board task")
        || lowered.contains("board-visible")
        || lowered.contains("task_pool")
        || lowered.contains("task pool")
        || lowered.contains("generic goal task pool")
        || lowered.contains("看板");
    if !has_board_conversion_language {
        return Vec::new();
    }
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.blocking
            && matches!(
                obligation.status.as_str(),
                "open" | "acknowledged_by_main_agent"
            )
    }) {
        let explicit_obligation_reference = round_content.contains(&obligation.obligation_id)
            || (lowered.contains("obligation")
                && round_content.contains(&obligation.kind)
                && round_content.contains(&obligation.stage_id));
        if !explicit_obligation_reference {
            continue;
        }
        let linked_task_ids = existing_task_ids
            .iter()
            .filter(|entry_id| round_content.contains(entry_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if linked_task_ids.is_empty() {
            continue;
        }
        obligation.status = "converted_to_board_task".to_string();
        obligation.satisfied_by =
            Some("pending_accepted_worker_evidence_or_review_pass".to_string());
        let refs = linked_task_ids
            .into_iter()
            .map(|entry_id| format!("goal_task_pool_entry:{entry_id}"))
            .chain(std::iter::once(format!(
                "main_agent_round:{}",
                round.artifact_path
            )))
            .collect::<Vec<_>>();
        merge_unique_strings(&mut obligation.handled_by_refs, refs);
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn mark_autonomous_research_failed_review_obligations_repaired_by_main_agent_synthesis(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
) -> Vec<String> {
    if round.execution_mode != "agent_loop" {
        return Vec::new();
    }
    let Some(last_failed_review) = job
        .last_review
        .as_ref()
        .filter(|review| review.verdict != "pass")
        .cloned()
    else {
        return Vec::new();
    };
    let last_failed_review_id = last_failed_review.review_id.clone();
    let contract = autonomous_research_stage_contract(resolved, job);
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return Vec::new();
    };
    if !accepted_worker_evidence_covers_required_task_types(&contract, Some(&index))
        || !accepted_worker_evidence_quality_floor_passes(&contract, Some(&index))
        || !accepted_worker_evidence_semantic_review_floor_passes(&contract, Some(&index))
        || !accepted_worker_evidence_active_tool_floor_passes(&contract, Some(&index))
    {
        return Vec::new();
    }
    let stage_artifact_path = resolved.workspace_root.join(&contract.artifact_path);
    let Ok(stage_artifact) = std::fs::read_to_string(&stage_artifact_path) else {
        return Vec::new();
    };
    if !autonomous_research_stage_artifact_has_reviewable_content(resolved, job)
        || !accepted_worker_evidence_bound_to_stage_artifact(resolved, job, &contract, Some(&index))
    {
        return Vec::new();
    }
    if !failed_review_repair_has_post_review_state_transition(
        resolved,
        job,
        &contract,
        &last_failed_review,
        &index,
        &stage_artifact,
    ) {
        return Vec::new();
    }

    let now = timestamp_string();
    let mut changed = Vec::new();
    let mut repaired_review_ids = Vec::new();
    let repair_refs = vec![
        format!("main_agent_round:{}", round.artifact_path),
        format!("stage_artifact:{}", contract.artifact_path),
        format!(
            "accepted_worker_evidence_index:{}",
            index.stage_execution_id
        ),
    ];
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "failed_stage_review"
            && obligation.stage_id == contract.stage_id
            && obligation.source == "review_gate"
            && obligation.status == "strategy_decided_by_main_agent"
            && obligation.review_id.as_deref() == Some(last_failed_review_id.as_str())
    }) {
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(format!(
            "main_agent_repair_synthesis:{}",
            round.artifact_path
        ));
        merge_unique_strings(&mut obligation.handled_by_refs, repair_refs.clone());
        merge_unique_strings(&mut obligation.evidence_refs, repair_refs.clone());
        obligation.updated_at = now.clone();
        if let Some(review_id) = obligation.review_id.clone() {
            merge_unique_strings(&mut repaired_review_ids, vec![review_id]);
        }
        changed.push(obligation.obligation_id.clone());
    }

    if !repaired_review_ids.is_empty() {
        for obligation in job.obligations.iter_mut().filter(|obligation| {
            obligation.kind == "main_agent_stalled"
                && obligation.source == "supervisor_gate"
                && matches!(
                    obligation.status.as_str(),
                    "open" | "acknowledged_by_main_agent"
                )
                && obligation
                    .review_id
                    .as_ref()
                    .map(|review_id| {
                        repaired_review_ids
                            .iter()
                            .any(|repaired| repaired == review_id)
                    })
                    .unwrap_or(false)
        }) {
            obligation.status = "satisfied".to_string();
            obligation.satisfied_by = Some(format!(
                "main_agent_stall_recovered_by_round:{}",
                round.artifact_path
            ));
            merge_unique_strings(&mut obligation.handled_by_refs, repair_refs.clone());
            merge_unique_strings(&mut obligation.evidence_refs, repair_refs.clone());
            obligation.updated_at = now.clone();
            changed.push(obligation.obligation_id.clone());
        }
    }

    changed.sort();
    changed.dedup();
    changed
}

pub(crate) fn mark_autonomous_research_main_agent_stalled_obligations_recovered_by_main_agent_action(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    round: &AutonomousResearchAgentRoundSummary,
    round_content: &str,
) -> Vec<String> {
    if round.execution_mode != "agent_loop" {
        return Vec::new();
    }
    let contract = autonomous_research_stage_contract(resolved, job);
    let accepted_evidence_ready =
        load_autonomous_research_accepted_worker_evidence_index(resolved, job)
            .map(|index| {
                accepted_worker_evidence_covers_required_task_types(&contract, Some(&index))
                    && accepted_worker_evidence_quality_floor_passes(&contract, Some(&index))
                    && accepted_worker_evidence_semantic_review_floor_passes(
                        &contract,
                        Some(&index),
                    )
                    && accepted_worker_evidence_active_tool_floor_passes(&contract, Some(&index))
            })
            .unwrap_or(false);
    let lowered = round_content.to_ascii_lowercase();
    let explicit_recovery_signal = lowered.contains("strict stage review")
        || lowered.contains("stage review")
        || lowered.contains("review rerun")
        || lowered.contains("review requested")
        || lowered.contains("请求严格")
        || lowered.contains("请求了严格")
        || lowered.contains("已解决")
        || lowered.contains("resolved blocking obligation")
        || lowered.contains("blocking obligation")
        || lowered.contains("strategy change")
        || (lowered.contains("published") && lowered.contains("board task"))
        || (lowered.contains("publish") && lowered.contains("board-visible"));
    if !accepted_evidence_ready && !explicit_recovery_signal {
        return Vec::new();
    }

    let now = timestamp_string();
    let mut changed = Vec::new();
    let repair_refs = vec![
        format!("main_agent_round:{}", round.artifact_path),
        format!("stage_artifact:{}", contract.artifact_path),
    ];
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "main_agent_stalled"
            && obligation.source == "supervisor_gate"
            && matches!(
                obligation.status.as_str(),
                "open" | "acknowledged_by_main_agent" | "strategy_decided_by_main_agent"
            )
    }) {
        let explicit_reference =
            round_content.contains(&obligation.obligation_id) || explicit_recovery_signal;
        let has_main_agent_action = !obligation.handled_by_refs.is_empty()
            || !obligation
                .evidence_refs
                .iter()
                .filter(|reference| reference.starts_with("main_agent_round:"))
                .collect::<Vec<_>>()
                .is_empty()
            || explicit_reference;
        if !has_main_agent_action {
            continue;
        }
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(format!(
            "main_agent_stall_recovered_by_round:{}",
            round.artifact_path
        ));
        merge_unique_strings(&mut obligation.handled_by_refs, repair_refs.clone());
        merge_unique_strings(&mut obligation.evidence_refs, repair_refs.clone());
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed.sort();
    changed.dedup();
    changed
}

pub(crate) fn record_autonomous_research_main_agent_stalled_obligation(
    job: &mut AutonomousResearchJobState,
    reason: &str,
) -> String {
    let contract_stage_id =
        autonomous_research_obligation_id(job, "main-agent", "main_agent_stalled", reason);
    let now = timestamp_string();
    let obligation = AutonomousResearchObligation {
        schema_version: "autonomous_research_obligation.v1".to_string(),
        obligation_id: contract_stage_id,
        kind: "main_agent_stalled".to_string(),
        stage_id: "main-agent".to_string(),
        stage_execution_id: job.stage_execution_id.clone(),
        source: "supervisor_gate".to_string(),
        status: "open".to_string(),
        blocking: true,
        required_by: "autonomous_research_supervisor".to_string(),
        satisfied_by: Some("main_agent_strategy_change_or_human_gate".to_string()),
        review_id: job
            .last_review
            .as_ref()
            .map(|review| review.review_id.clone()),
        failure_class: Some("main_agent_stalled".to_string()),
        missing_task_type: None,
        detail: reason.to_string(),
        handled_by_refs: Vec::new(),
        evidence_refs: vec![format!("auto_research_job:{}", job.job_id)],
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_autonomous_research_obligation(job, obligation)
}

pub(crate) fn record_autonomous_research_project_context_conflict_obligation(
    job: &mut AutonomousResearchJobState,
    context_file: &AutonomousResearchProjectContextFileSummary,
) -> String {
    let now = timestamp_string();
    let detail = format!(
        "Project context file `{}` contains advisory text that conflicts with runtime canonical state: {}. Runtime MissionFrame, stage DAG, obligations, review gates, accepted evidence, and cleanup state remain authoritative.",
        context_file.path,
        context_file.conflict_signals.join(", ")
    );
    let obligation = AutonomousResearchObligation {
        schema_version: "autonomous_research_obligation.v1".to_string(),
        obligation_id: autonomous_research_obligation_id(
            job,
            "project-context",
            "project_context_conflict",
            &format!("{}::{}", context_file.path, context_file.content_hash),
        ),
        kind: "project_context_conflict".to_string(),
        stage_id: "project-context".to_string(),
        stage_execution_id: job.stage_execution_id.clone(),
        source: "project_context_file_advisory_input".to_string(),
        status: "open".to_string(),
        blocking: true,
        required_by: "runtime_canonical_state_precedence".to_string(),
        satisfied_by: Some("main_agent_reconciles_context_with_runtime_state".to_string()),
        review_id: job
            .last_review
            .as_ref()
            .map(|review| review.review_id.clone()),
        failure_class: Some("project_context_conflict".to_string()),
        missing_task_type: None,
        detail,
        handled_by_refs: Vec::new(),
        evidence_refs: vec![
            format!("project_context_file:{}", context_file.path),
            format!("auto_research_job:{}", job.job_id),
        ],
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_autonomous_research_obligation(job, obligation)
}

pub(crate) fn record_autonomous_research_provider_fault_obligation(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    fault: &AutonomousResearchProviderFaultState,
) -> String {
    let contract = autonomous_research_stage_contract(resolved, job);
    let now = timestamp_string();
    let provider = fault
        .provider_id
        .clone()
        .unwrap_or_else(|| "unknown_provider".to_string());
    let model = fault
        .model
        .clone()
        .unwrap_or_else(|| "unknown_model".to_string());
    let detail = format!(
        "Provider fault from `{}` using `{}`/`{}`: category={}, disposition={}, retryable={}, operator_gate_required={}, message={}. Runtime must not synthesize research content from this fault. Runtime owns retry/backoff/failover execution under provider policy; the main agent owns only the research implications: keep tasks pending, split or reprioritize work when capacity changes, request a human gate if provider policy blocks progress, or record an explicit route when the engineering fault changes research feasibility.",
        fault.source,
        provider,
        model,
        fault.category,
        fault.disposition,
        fault.retryable,
        fault.operator_gate_required,
        fault.message
    );
    let obligation = AutonomousResearchObligation {
        schema_version: "autonomous_research_obligation.v1".to_string(),
        obligation_id: autonomous_research_obligation_id(
            job,
            &contract.stage_id,
            "provider_fault",
            &format!(
                "{}::{}::{}::{}",
                fault.source, provider, model, fault.category
            ),
        ),
        kind: "provider_fault".to_string(),
        stage_id: contract.stage_id,
        stage_execution_id: job.stage_execution_id.clone(),
        source: fault.source.clone(),
        status: "open".to_string(),
        blocking: true,
        required_by: "main_agent_provider_fault_decision".to_string(),
        satisfied_by: Some(
            "successful_provider_round_or_main_agent_provider_route_decision".to_string(),
        ),
        review_id: job
            .last_review
            .as_ref()
            .map(|review| review.review_id.clone()),
        failure_class: Some(fault.category.clone()),
        missing_task_type: None,
        detail,
        handled_by_refs: Vec::new(),
        evidence_refs: vec![
            format!("provider_fault:{}", fault.fault_id),
            format!("auto_research_job:{}", job.job_id),
            contract.artifact_path,
        ],
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_autonomous_research_obligation(job, obligation)
}

#[cfg(test)]
pub(crate) fn mark_autonomous_research_stage_obligations_superseded_by_route_change(
    job: &mut AutonomousResearchJobState,
    plan: &AutonomousResearchRepairPlan,
    repair_plan_doc_ref: &str,
) -> Vec<String> {
    if !matches!(
        plan.suggested_operation.as_str(),
        "pivot" | "fork" | "supersede" | "abandon"
    ) && !plan.cleanup_required
    {
        return Vec::new();
    }
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.stage_id == plan.affected_stage_id
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "superseded_by_rollback".to_string();
        obligation.satisfied_by = Some(format!(
            "{}:{}",
            plan.suggested_operation, plan.repair_task_id
        ));
        merge_unique_strings(
            &mut obligation.handled_by_refs,
            vec![repair_plan_doc_ref.to_string()],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

#[cfg(test)]
pub(crate) fn record_autonomous_research_cleanup_required_obligation(
    job: &mut AutonomousResearchJobState,
    plan: &AutonomousResearchRepairPlan,
    repair_plan_doc_ref: &str,
) -> String {
    let now = timestamp_string();
    let key = format!(
        "{}::{}::{}",
        plan.repair_task_id,
        plan.suggested_operation,
        plan.cleanup_reason.clone().unwrap_or_default()
    );
    let obligation = AutonomousResearchObligation {
        schema_version: "autonomous_research_obligation.v1".to_string(),
        obligation_id: autonomous_research_obligation_id(
            job,
            &plan.affected_stage_id,
            "cleanup_required",
            &key,
        ),
        kind: "cleanup_required".to_string(),
        stage_id: plan.affected_stage_id.clone(),
        stage_execution_id: job.stage_execution_id.clone(),
        source: "research_dag_route_change".to_string(),
        status: "open".to_string(),
        blocking: true,
        required_by: "projectops_cleanup".to_string(),
        satisfied_by: Some("projectops_cleanup_summary".to_string()),
        review_id: Some(plan.review_id.clone()),
        failure_class: Some(plan.failure_class.clone()),
        missing_task_type: None,
        detail: plan.cleanup_reason.clone().unwrap_or_else(|| {
            format!(
                "{} requires cleanup after {}",
                plan.failure_class, plan.suggested_operation
            )
        }),
        handled_by_refs: vec![repair_plan_doc_ref.to_string()],
        evidence_refs: vec![
            format!("review:{}", plan.review_id),
            repair_plan_doc_ref.to_string(),
            format!("auto_research_job:{}", job.job_id),
        ],
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_autonomous_research_obligation(job, obligation)
}

pub(crate) fn mark_autonomous_research_cleanup_obligations_satisfied(
    job: &mut AutonomousResearchJobState,
    cleanup_summary_ref: &str,
) -> Vec<String> {
    let now = timestamp_string();
    let mut changed = Vec::new();
    for obligation in job.obligations.iter_mut().filter(|obligation| {
        obligation.kind == "cleanup_required"
            && obligation.blocking
            && !autonomous_research_obligation_is_closed(&obligation.status)
    }) {
        obligation.status = "satisfied".to_string();
        obligation.satisfied_by = Some(cleanup_summary_ref.to_string());
        merge_unique_strings(
            &mut obligation.evidence_refs,
            vec![cleanup_summary_ref.to_string()],
        );
        obligation.updated_at = now.clone();
        changed.push(obligation.obligation_id.clone());
    }
    changed
}

pub(crate) fn retire_canonical_artifacts_for_satisfied_cleanup_obligations(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    satisfied_cleanup_obligation_ids: &[String],
    cleanup_summary_ref: &str,
) -> Result<Vec<String>, String> {
    if satisfied_cleanup_obligation_ids.is_empty() {
        return Ok(Vec::new());
    }
    let ledger =
        canonical_artifacts::load_ledger(&resolved.data_dir).map_err(|err| err.to_string())?;
    let mut retired_refs = Vec::new();
    for obligation_id in satisfied_cleanup_obligation_ids {
        let Some(obligation) = job
            .obligations
            .iter()
            .find(|obligation| obligation.obligation_id == *obligation_id)
        else {
            continue;
        };
        let Some(stage_execution_id) = obligation.stage_execution_id.as_deref() else {
            continue;
        };
        for entry in ledger.entries.iter().filter(|entry| {
            entry.job_id == job.job_id
                && entry.stage_id == obligation.stage_id
                && entry.stage_execution_id == stage_execution_id
                && entry.status != canonical_artifacts::CanonicalArtifactStatus::RetiredOrSuperseded
        }) {
            let retired = canonical_artifacts::record_retired_or_superseded(
                &resolved.data_dir,
                &entry.artifact_id,
                format!("cleanup_obligation:{}", obligation.obligation_id),
                Some(cleanup_summary_ref.to_string()),
            )
            .map_err(|err| err.to_string())?;
            merge_unique_strings(
                &mut retired_refs,
                vec![
                    format!("canonical_artifact_retired:{}", retired.artifact_id),
                    format!("retired_canonical_target:{}", retired.target_artifact_path),
                ],
            );
        }
    }
    Ok(retired_refs)
}

pub(crate) fn autonomous_research_active_job_for_continuity(
    resolved: &ResolvedProject,
) -> Option<AutonomousResearchJobState> {
    let mut jobs = list_autonomous_research_jobs(resolved).ok()?.jobs;
    jobs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    jobs.iter()
        .find(|job| !autonomous_research_job_terminal(&job.status))
        .cloned()
        .or_else(|| jobs.into_iter().next())
}

pub(crate) fn autonomous_research_project_continuity_snapshot(
    resolved: &ResolvedProject,
    active_session_id: Option<String>,
) -> AutonomousResearchProjectContinuitySnapshot {
    let active_job = autonomous_research_active_job_for_continuity(resolved);
    build_autonomous_research_project_continuity_snapshot(
        resolved,
        active_job.as_ref(),
        active_session_id,
    )
}

pub(crate) fn autonomous_research_project_context_files(
    resolved: &ResolvedProject,
) -> Vec<AutonomousResearchProjectContextFileSummary> {
    const MAX_CONTEXT_FILE_BYTES: u64 = 64 * 1024;
    let candidates = [
        ".astra/context.md",
        ".astra/CONTEXT.md",
        ".astra/bootstrap.md",
        "ASTRA.md",
        "AGENTS.md",
        "CLAUDE.md",
        "HERMES.md",
        ".hermes.md",
    ];
    let mut summaries = Vec::new();
    for relative in candidates {
        let path = resolved.workspace_root.join(relative);
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_CONTEXT_FILE_BYTES {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lowered = content.to_ascii_lowercase();
        let mut conflict_signals = Vec::new();
        let signal_needles: [(&str, &[&str]); 3] = [
            (
                "attempts_to_override_runtime_canonical_state",
                &[
                    "override runtime",
                    "source of truth",
                    "canonical state",
                    "ignore missionframe",
                    "ignore mission frame",
                ],
            ),
            (
                "attempts_to_bypass_review_or_obligation_gate",
                &[
                    "skip review",
                    "ignore review",
                    "bypass review",
                    "ignore obligations",
                    "skip obligation",
                ],
            ),
            (
                "attempts_to_bypass_cleanup_after_route_change",
                &[
                    "skip cleanup",
                    "ignore cleanup",
                    "no cleanup required",
                    "bypass cleanup",
                ],
            ),
        ];
        for (signal, needles) in signal_needles {
            if needles.iter().any(|needle| lowered.contains(needle)) {
                conflict_signals.push(signal.to_string());
            }
        }
        summaries.push(AutonomousResearchProjectContextFileSummary {
            schema_version: "project_context_file_summary.v1".to_string(),
            path: relative.to_string(),
            advisory_only: true,
            canonical_authority: false,
            content_hash: short_sha256_hex(&content),
            preview: compact_single_line(&content, 320),
            conflict_signals,
        });
    }
    summaries
}

pub(crate) fn sync_autonomous_research_project_context_conflict_obligations(
    job: &mut AutonomousResearchJobState,
    context_files: &[AutonomousResearchProjectContextFileSummary],
) -> Vec<String> {
    let mut changed = Vec::new();
    for context_file in context_files
        .iter()
        .filter(|context_file| !context_file.conflict_signals.is_empty())
    {
        changed.push(
            record_autonomous_research_project_context_conflict_obligation(job, context_file),
        );
    }
    changed
}

pub(crate) fn build_autonomous_research_project_continuity_snapshot(
    resolved: &ResolvedProject,
    job: Option<&AutonomousResearchJobState>,
    active_session_id: Option<String>,
) -> AutonomousResearchProjectContinuitySnapshot {
    let mission_status = crate::goals::status(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
    )
    .ok();
    let task_pool = mission_status.as_ref().map(|status| {
        let mut selected_entries = status
            .task_pool
            .entries
            .iter()
            .filter(|entry| {
                entry.action_policy.as_deref() == Some("requires_main_agent_task")
                    || entry.source.source_kind.ends_with("_candidate")
            })
            .collect::<Vec<_>>();
        for entry in &status.task_pool.entries {
            if selected_entries.len() >= 30 {
                break;
            }
            if !selected_entries
                .iter()
                .any(|selected| selected.entry_id == entry.entry_id)
            {
                selected_entries.push(entry);
            }
        }
        let entries = selected_entries
            .into_iter()
            .map(|entry| AutonomousResearchTaskPoolEntryContinuity {
                entry_id: entry.entry_id.clone(),
                bucket_id: entry.bucket_id.clone(),
                title: entry.title.clone(),
                summary: entry.summary.clone(),
                status: entry.status.clone(),
                source_kind: entry.source.source_kind.clone(),
                source_id: entry.source.source_id.clone(),
                action_policy: entry.action_policy.clone(),
                action_refs: entry.action_refs.clone(),
                recommended_command: entry.recommended_command.clone(),
                projected_stage_task_type: entry
                    .stage_task
                    .as_ref()
                    .map(|stage_task| stage_task.task_type.clone()),
                projected_stage_task: entry.stage_task.clone(),
            })
            .collect::<Vec<_>>();
        AutonomousResearchTaskPoolContinuity {
            summary: status.task_pool.summary.clone(),
            next_recommended_action: status.task_pool.next_recommended_action.clone(),
            entries,
        }
    });
    let accepted_worker_evidence_index =
        job.and_then(|job| load_autonomous_research_accepted_worker_evidence_index(resolved, job));
    let accepted_worker_evidence_task_types = accepted_worker_evidence_index
        .as_ref()
        .map(|index| {
            index
                .entries
                .iter()
                .map(|entry| entry.task_type.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let active_worker_evidence_count =
        accepted_worker_evidence_current_gate_entry_count(accepted_worker_evidence_index.as_ref());
    let active_worker_evidence_task_ids =
        accepted_worker_evidence_current_gate_task_ids(accepted_worker_evidence_index.as_ref());
    let active_worker_evidence_task_types =
        accepted_worker_evidence_current_gate_task_types(accepted_worker_evidence_index.as_ref());
    let active_evidence_set_ids =
        accepted_worker_evidence_current_gate_set_ids(accepted_worker_evidence_index.as_ref());
    let open_obligations = job
        .map(autonomous_research_active_blocking_obligations)
        .unwrap_or_default();
    let unresolved_worker_review_failures = job
        .map(|job| autonomous_research_unresolved_worker_review_failures(resolved, job))
        .unwrap_or_default();
    let latest_round = job.and_then(|job| job.provider_rounds.last());
    let latest_packet = job.and_then(|job| job.continuity_packets.last());
    let active_stage_id = job.map(|job| autonomous_research_stage_contract(resolved, job).stage_id);
    let project_context_files = autonomous_research_project_context_files(resolved);
    let project_context_warnings = project_context_files
        .iter()
        .flat_map(|context_file| {
            context_file
                .conflict_signals
                .iter()
                .map(|signal| {
                    format!(
                        "{}: advisory project context conflicts with runtime canonical state ({signal})",
                        context_file.path
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let stage_closure_ledger = job.map(|job| {
        build_autonomous_research_stage_closure_ledger(
            resolved,
            job,
            task_pool.as_ref(),
            accepted_worker_evidence_index.as_ref(),
        )
    });
    let next_required_action = autonomous_research_next_required_action_for_snapshot(
        job,
        task_pool.as_ref(),
        &open_obligations,
        &unresolved_worker_review_failures,
        stage_closure_ledger.as_ref(),
    );
    let resume_recommendation = autonomous_research_resume_recommendation_for_snapshot(job);
    AutonomousResearchProjectContinuitySnapshot {
        schema_version: "project_continuity_snapshot.v1".to_string(),
        project_id: resolved.project_id.clone(),
        workspace_root: resolved.workspace_root.display().to_string(),
        active_session_id,
        active_autonomous_job_id: job.map(|job| job.job_id.clone()),
        active_job_status: job.map(|job| job.status.clone()),
        active_job_phase: job.map(|job| job.phase.clone()),
        active_stage_id,
        active_stage_execution_id: job.and_then(|job| job.stage_execution_id.clone()),
        automation_mode: job.map(|job| job.automation_mode),
        mission_frame_ref: mission_status
            .as_ref()
            .map(|status| status.mission_frame_ref.clone()),
        mission_status: mission_status.as_ref().map(|status| status.status.clone()),
        current_implementation_goal: mission_status
            .as_ref()
            .and_then(|status| status.mission_frame.as_ref())
            .map(|frame| frame.current_implementation_goal.clone()),
        task_pool,
        open_blocking_obligation_count: open_obligations.len(),
        open_obligations,
        unresolved_worker_review_failures,
        accepted_worker_evidence_count: accepted_worker_evidence_index
            .as_ref()
            .map(|index| index.entries.len())
            .unwrap_or(0),
        accepted_worker_evidence_task_types,
        active_worker_evidence_count,
        active_worker_evidence_task_ids,
        active_worker_evidence_task_types,
        active_evidence_set_ids,
        stage_closure_ledger,
        latest_review_verdict: job.and_then(|job| {
            job.last_review
                .as_ref()
                .map(|review| review.verdict.clone())
        }),
        latest_review_score: job
            .and_then(|job| job.last_review.as_ref().and_then(|review| review.score)),
        latest_provider_id: latest_round.map(|round| round.provider_id.clone()),
        latest_model: latest_round.map(|round| round.model.clone()),
        latest_continuity_packet_ref: latest_packet.map(|packet| packet.artifact_path.clone()),
        cleanup_plan_ids: job
            .map(|job| job.cleanup_plan_ids.clone())
            .unwrap_or_default(),
        artifact_refs: job.map(|job| job.artifact_refs.clone()).unwrap_or_default(),
        project_context_files,
        project_context_warnings,
        next_required_action,
        resume_recommendation,
        generated_at: timestamp_string(),
    }
}
