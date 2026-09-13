pub(crate) struct AutonomousResearchAppliedRouteChange {
    previous_review: Option<AutonomousResearchReviewState>,
    previous_review_passed: bool,
}

pub(crate) fn apply_autonomous_research_main_agent_route_change_if_ready(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    tick_index: usize,
    output_json: bool,
) -> Result<Option<AutonomousResearchAppliedRouteChange>, CommandFailureOutcome> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let Some(request) = latest_main_agent_route_change_request_for_stage(resolved, job, &contract)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?
    else {
        return Ok(None);
    };
    let already_applied_ref = format!("applied:{}", request.record_ref);
    if job
        .artifact_refs
        .iter()
        .any(|artifact| artifact == &already_applied_ref)
    {
        return Ok(None);
    }

    if request.operation != "advance" {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "reason": "runtime currently executes only validated advance requests; rollback, pivot, fork, supersede, abandon, and cleanup-bearing routes remain main-agent decisions that must go through the cleanup/DAG path"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    let Some(review) = job.last_review.clone() else {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "reason": "advance requires a passing review in canonical job state"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    };
    if review.verdict != "pass" {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "review_id": review.review_id,
                "review_verdict": review.verdict,
                "reason": "advance requires a passing review"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    let Some(closure_decision) =
        load_main_agent_stage_closure_decision_record(resolved, job, &contract.stage_id)
    else {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "stage_id": contract.stage_id,
                "stage_execution_id": job.stage_execution_id,
                "reason": "advance requires main-agent recorded stage closure decision"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    };
    if closure_decision.decision != "close_and_advance" {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "closure_decision_ref": closure_decision.decision_ref,
                "operation": request.operation,
                "stage_closure_decision": closure_decision.decision,
                "reason": "advance requires a main-agent stage closure decision of close_and_advance"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    if closure_decision.target_stage_id != request.target_stage_id {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "closure_decision_ref": closure_decision.decision_ref,
                "operation": request.operation,
                "target_stage_id": request.target_stage_id,
                "closure_target_stage_id": closure_decision.target_stage_id,
                "reason": "advance target must match the main-agent stage closure decision"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    if !main_agent_stage_closure_review_ref_matches(&closure_decision.review_ref, &review) {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "closure_decision_ref": closure_decision.decision_ref,
                "operation": request.operation,
                "review_id": review.review_id,
                "closure_review_ref": closure_decision.review_ref,
                "reason": "stage closure decision must cite the current passing review"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    if !main_agent_stage_closure_artifact_ref_matches(
        &closure_decision.stage_artifact_ref,
        &contract,
        &review,
    ) {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "closure_decision_ref": closure_decision.decision_ref,
                "operation": request.operation,
                "stage_artifact_ref": closure_decision.stage_artifact_ref,
                "expected_stage_artifact_path": contract.artifact_path,
                "reason": "stage closure decision must cite the reviewed active stage artifact"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    if closure_decision.accepted_evidence_refs.is_empty()
        || closure_decision.readiness_refs.is_empty()
        || closure_decision
            .why_no_more_stage_work_is_needed
            .trim()
            .is_empty()
    {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "closure_decision_ref": closure_decision.decision_ref,
                "operation": request.operation,
                "reason": "stage closure decision must include accepted evidence refs, readiness refs, and why no more stage work is needed"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    if closure_decision.cleanup_required != request.cleanup_required {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "closure_decision_ref": closure_decision.decision_ref,
                "operation": request.operation,
                "request_cleanup_required": request.cleanup_required,
                "closure_cleanup_required": closure_decision.cleanup_required,
                "reason": "route cleanup requirement must match the main-agent stage closure decision"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    let expected_next_stage_id = autonomous_research_default_next_stage_id(&contract.stage_id);
    if expected_next_stage_id.as_deref() != Some(request.target_stage_id.as_str()) {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "target_stage_id": request.target_stage_id,
                "expected_target_stage_id": expected_next_stage_id,
                "reason": "advance must follow StageExecutionMap.default_next_stage_id"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    let unresolved = autonomous_research_active_blocking_obligations(job);
    if !unresolved.is_empty() {
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "reason": "advance requires all blocking obligations to be closed",
                "open_blocking_obligations": unresolved
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }
    if request.cleanup_required {
        record_autonomous_research_route_change_cleanup_obligation(job, &contract, &request);
        job.status = "running".to_string();
        job.phase = "route_change_waiting_for_cleanup".to_string();
        job.stop_reason = Some("main_agent_route_change_requires_cleanup".to_string());
        job.updated_at = timestamp_string();
        write_autonomous_research_job_state(resolved, job).map_err(|err| {
            internal_failure("research jobs tick", err).with_output_json(output_json)
        })?;
        append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_route_change_deferred",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "route_change_ref": request.record_ref,
                "operation": request.operation,
                "reason": "cleanup_required=true; cleanup must close before the route can mutate the active stage"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        return Ok(None);
    }

    let thread_id = job
        .thread_id
        .clone()
        .or_else(|| {
            job.stage_execution_id.as_deref().and_then(|execution_id| {
                research::inspect_stage(&resolved.data_dir, execution_id)
                    .ok()
                    .map(|inspection| inspection.thread.thread_id)
            })
        })
        .ok_or_else(|| {
            internal_failure(
                "research jobs tick",
                "main-agent route change cannot be applied without an active research thread"
                    .to_string(),
            )
            .with_output_json(output_json)
        })?;
    let mut evidence_refs = vec![
        format!("main_agent_route_change_request:{}", request.record_ref),
        format!(
            "main_agent_stage_closure_decision:{}",
            closure_decision.decision_ref
        ),
        format!("review:{}", review.review_id),
        contract.artifact_path.clone(),
    ];
    merge_unique_strings(&mut evidence_refs, request.readiness_refs.clone());
    merge_unique_strings(&mut evidence_refs, closure_decision.readiness_refs.clone());
    merge_unique_strings(
        &mut evidence_refs,
        closure_decision.accepted_evidence_refs.clone(),
    );
    let decision = research::decide(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
        research::ResearchDecisionRequest {
            thread_id,
            operation: "advance".to_string(),
            decision: "approve".to_string(),
            reason: request.rationale.clone(),
            evidence_refs: evidence_refs.clone(),
        },
    )
    .map_err(|err| research_failure("research jobs tick", err, resolved, output_json))?;
    let status = research::status(&resolved.data_dir, &resolved.project_id)
        .map_err(|err| research_failure("research jobs tick", err, resolved, output_json))?;
    let Some(new_stage) = status.active_stage_execution else {
        return Err(internal_failure(
            "research jobs tick",
            "research decision succeeded but no active stage execution was recorded".to_string(),
        )
        .with_output_json(output_json));
    };
    if new_stage.stage_id != request.target_stage_id {
        return Err(internal_failure(
            "research jobs tick",
            format!(
                "research decision advanced to `{}` but main-agent route requested `{}`",
                new_stage.stage_id, request.target_stage_id
            ),
        )
        .with_output_json(output_json));
    }

    let previous_review = job.last_review.clone();
    let previous_stage_execution_id = job.stage_execution_id.clone();
    let previous_stage_id = contract.stage_id.clone();
    job.stage_execution_id = Some(new_stage.execution_id.clone());
    job.thread_id = Some(new_stage_decision_thread_id(resolved, &new_stage, job));
    job.last_review = None;
    job.last_error = None;
    job.stop_reason = None;
    job.status = "running".to_string();
    job.phase = "stage_advanced_by_main_agent_route_change".to_string();
    job.updated_at = timestamp_string();
    merge_unique_strings(
        &mut job.artifact_refs,
        vec![
            format!("main_agent_route_change_request:{}", request.record_ref),
            already_applied_ref,
            format!("research_decision:{}", decision.decision_id),
            format!("research_stage:{}", new_stage.execution_id),
        ],
    );
    write_autonomous_research_job_state(resolved, job)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "main_agent_route_change_applied",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "tick_index": tick_index,
            "route_change_ref": request.record_ref,
            "closure_decision_ref": closure_decision.decision_ref,
            "operation": request.operation,
            "from_stage_id": previous_stage_id,
            "from_stage_execution_id": previous_stage_execution_id,
            "to_stage_id": new_stage.stage_id,
            "to_stage_execution_id": new_stage.execution_id,
            "research_decision_id": decision.decision_id,
            "review_id": review.review_id,
            "closure_created_at": closure_decision.created_at.clone(),
            "closure_rationale": closure_decision.closure_rationale.clone(),
            "closure_remaining_risks": closure_decision.remaining_risks.clone(),
            "closure_cleanup_rationale": closure_decision.cleanup_rationale.clone(),
            "runtime_boundary": "runtime executed a structured main-agent route request after validating main-agent stage closure decision, review pass, default DAG edge, obligations, and cleanup state"
        }),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    publish_project_checkpoint(
        resolved,
        &[
            "mission_frame",
            "research",
            "orchestration",
            "agents",
            "review",
            "projectops",
            "artifacts",
        ],
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    Ok(Some(AutonomousResearchAppliedRouteChange {
        previous_review,
        previous_review_passed: true,
    }))
}

pub(crate) fn new_stage_decision_thread_id(
    resolved: &ResolvedProject,
    new_stage: &research::ResearchStageExecution,
    job: &AutonomousResearchJobState,
) -> String {
    research::inspect_stage(&resolved.data_dir, &new_stage.execution_id)
        .ok()
        .map(|inspection| inspection.thread.thread_id)
        .or_else(|| job.thread_id.clone())
        .unwrap_or_else(|| "thread_unknown".to_string())
}

pub(crate) fn latest_main_agent_route_change_request_for_stage(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<Option<MainAgentRouteChangeRequestRecord>, String> {
    let records = load_main_agent_route_change_request_records(resolved)?;
    Ok(records.into_iter().rev().find(|record| {
        record
            .job_id
            .as_deref()
            .is_none_or(|job_id| job_id == job.job_id)
            && record
                .stage_id
                .as_deref()
                .is_none_or(|stage_id| stage_id == contract.stage_id)
            && record
                .stage_execution_id
                .as_deref()
                .is_none_or(|stage_execution_id| {
                    Some(stage_execution_id) == job.stage_execution_id.as_deref()
                })
    }))
}

pub(crate) fn load_main_agent_route_change_request_records(
    resolved: &ResolvedProject,
) -> Result<Vec<MainAgentRouteChangeRequestRecord>, String> {
    let dir = resolved
        .data_dir
        .join("main-agent-board")
        .join("route_change_requests");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(value) = read_json_file_runtime(&path) else {
            continue;
        };
        let Some(record) = main_agent_route_change_request_record_from_value(&value) else {
            continue;
        };
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.record_ref.cmp(&right.record_ref))
    });
    Ok(records)
}

pub(crate) fn main_agent_route_change_request_record_from_value(
    value: &Value,
) -> Option<MainAgentRouteChangeRequestRecord> {
    if value.get("tool_name").and_then(|value| value.as_str()) != Some("request_route_change") {
        return None;
    }
    if value.get("published_by").and_then(|value| value.as_str()) != Some("main_agent") {
        return None;
    }
    let arguments = value.get("arguments")?;
    Some(MainAgentRouteChangeRequestRecord {
        record_ref: json_string_field_runtime(value, "record_ref")?,
        operation: json_string_field_runtime(arguments, "operation")?,
        target_stage_id: json_string_field_runtime(arguments, "target_stage_id")?,
        rationale: json_string_field_runtime(arguments, "rationale")?,
        cleanup_required: arguments
            .get("cleanup_required")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        readiness_refs: json_string_array_runtime(arguments, "readiness_refs"),
        job_id: json_string_field_runtime(value, "job_id"),
        stage_id: json_string_field_runtime(value, "stage_id"),
        stage_execution_id: json_string_field_runtime(value, "stage_execution_id"),
        created_at: json_string_field_runtime(value, "created_at").unwrap_or_default(),
    })
}

pub(crate) fn main_agent_stage_closure_review_ref_matches(
    review_ref: &str,
    review: &AutonomousResearchReviewState,
) -> bool {
    let review_ref = relative_display_ref(review_ref);
    let review_id_ref = format!("review:{}", review.review_id);
    let review_summary = review
        .review_summary_path
        .as_deref()
        .map(relative_display_ref);
    review_ref == review.review_id
        || review_ref == review_id_ref
        || review_summary
            .as_deref()
            .is_some_and(|summary| review_ref == summary)
        || review_ref.contains(&review.review_id)
}

pub(crate) fn main_agent_stage_closure_artifact_ref_matches(
    stage_artifact_ref: &str,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
) -> bool {
    let stage_artifact_ref = relative_display_ref(stage_artifact_ref);
    stage_artifact_ref == relative_display_ref(&contract.artifact_path)
        || stage_artifact_ref == relative_display_ref(&review.target_path)
}

pub(crate) fn autonomous_research_default_next_stage_id(stage_id: &str) -> Option<String> {
    research::stage_execution_map()
        .entries
        .into_iter()
        .find(|entry| entry.stage_id == stage_id)
        .and_then(|entry| entry.default_next_stage_id)
}

pub(crate) fn record_autonomous_research_route_change_cleanup_obligation(
    job: &mut AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    request: &MainAgentRouteChangeRequestRecord,
) -> String {
    let now = timestamp_string();
    let obligation = AutonomousResearchObligation {
        schema_version: "autonomous_research_obligation.v1".to_string(),
        obligation_id: autonomous_research_obligation_id(
            job,
            &contract.stage_id,
            "cleanup_required",
            &request.record_ref,
        ),
        kind: "cleanup_required".to_string(),
        stage_id: contract.stage_id.clone(),
        stage_execution_id: job.stage_execution_id.clone(),
        source: "main_agent_route_change".to_string(),
        status: "open".to_string(),
        blocking: true,
        required_by: "projectops_cleanup".to_string(),
        satisfied_by: Some("projectops_cleanup_summary".to_string()),
        review_id: job
            .last_review
            .as_ref()
            .map(|review| review.review_id.clone()),
        failure_class: Some("route_change_cleanup_required".to_string()),
        missing_task_type: None,
        detail: format!(
            "Main agent route change `{}` requested {} -> {} with cleanup_required=true. Rationale: {}",
            request.record_ref, contract.stage_id, request.target_stage_id, request.rationale
        ),
        handled_by_refs: vec![format!(
            "main_agent_route_change_request:{}",
            request.record_ref
        )],
        evidence_refs: request.readiness_refs.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    upsert_autonomous_research_obligation(job, obligation)
}

