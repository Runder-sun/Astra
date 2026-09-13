pub(crate) fn build_autonomous_research_stage_closure_ledger(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    task_pool: Option<&AutonomousResearchTaskPoolContinuity>,
    accepted_worker_evidence_index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> AutonomousResearchStageClosureLedger {
    let contract = autonomous_research_stage_contract(resolved, job);
    let stage_execution_id = job
        .stage_execution_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let required_task_types = contract.worker_task_types.clone();
    let accepted_task_types =
        accepted_worker_evidence_current_gate_task_types(accepted_worker_evidence_index);
    let active_evidence_count =
        accepted_worker_evidence_current_gate_entry_count(accepted_worker_evidence_index);
    let active_evidence_task_ids =
        accepted_worker_evidence_current_gate_task_ids(accepted_worker_evidence_index);
    let active_evidence_set_ids =
        accepted_worker_evidence_current_gate_set_ids(accepted_worker_evidence_index);
    let non_current_evidence_count =
        accepted_worker_evidence_non_current_entry_count(accepted_worker_evidence_index);
    let adoption_ready_candidates = autonomous_research_adoption_ready_candidates(
        resolved,
        &contract,
        job,
        accepted_worker_evidence_index,
    );
    let latest_failed_review_repair = build_autonomous_research_failed_review_repair_ledger(
        resolved,
        job,
        &contract,
        accepted_worker_evidence_index,
    );
    let canonical_artifacts = autonomous_research_stage_closure_canonical_artifacts(
        resolved,
        &contract,
        &stage_execution_id,
    );
    let canonical_artifact_count = canonical_artifacts.len();
    let canonical_adoption_requested_count = canonical_artifacts
        .iter()
        .filter(|artifact| artifact.status == "adoption_requested")
        .count();
    let canonical_materialized_count = canonical_artifacts
        .iter()
        .filter(|artifact| artifact.status == "materialized")
        .count();
    let canonical_baseline_visible_count = canonical_artifacts
        .iter()
        .filter(|artifact| artifact.status == "baseline_visible")
        .count();
    let canonical_integration_verified_count = canonical_artifacts
        .iter()
        .filter(|artifact| artifact.status == "integration_verified")
        .count();
    let canonical_active_stage_evidence_count = canonical_artifacts
        .iter()
        .filter(|artifact| artifact.status == "active_stage_evidence")
        .count();
    let canonical_artifact_blockers =
        autonomous_research_stage_closure_canonical_artifact_blockers(&canonical_artifacts);
    let missing_task_types = accepted_worker_evidence_missing_required_task_types(
        &contract,
        accepted_worker_evidence_index,
    );
    let published_tasks =
        autonomous_research_stage_closure_published_tasks(resolved, job, task_pool, &contract);
    let task_type_statuses = required_task_types
        .iter()
        .map(|task_type| {
            let tasks = published_tasks
                .iter()
                .filter(|task| task.task_type == *task_type)
                .collect::<Vec<_>>();
            let accepted = accepted_task_types
                .iter()
                .any(|accepted| accepted == task_type);
            let missing = missing_task_types
                .iter()
                .any(|missing| missing == task_type);
            AutonomousResearchStageTaskTypeStatus {
                task_type: task_type.clone(),
                status: autonomous_research_stage_task_type_status(accepted, missing, &tasks),
                accepted,
                missing,
                published_count: tasks.len(),
                running_count: tasks
                    .iter()
                    .filter(|task| task.bucket_id == "running" || task.status == "running")
                    .count(),
                ready_count: tasks
                    .iter()
                    .filter(|task| task.bucket_id == "ready_to_run" || task.status == "ready")
                    .count(),
                blocked_count: tasks
                    .iter()
                    .filter(|task| {
                        task.bucket_id == "blocked"
                            || task.status == "blocked"
                            || task.bucket_id == "contract_invalid"
                            || task.status == "contract_invalid"
                    })
                    .count(),
                merged_count: tasks.iter().filter(|task| task.merged).count(),
            }
        })
        .collect::<Vec<_>>();
    let rubric_path = relative_workspace_ref(
        &resolved.workspace_root,
        &autonomous_research_stage_rubric_path(resolved, job, &contract),
    );
    let rubric_exists = resolved.workspace_root.join(&rubric_path).exists();
    let mut adoption_blockers = Vec::new();
    if contract.evidence_plan_status != "adopted" {
        adoption_blockers.push(
            "main-agent-adopted stage evidence plan is missing; default task catalogs are advisory and cannot define evidence sufficiency".to_string(),
        );
    }
    if !missing_task_types.is_empty() {
        adoption_blockers.push(format!(
            "missing accepted worker evidence for required task types: {}",
            missing_task_types.join(", ")
        ));
    }
    let ambiguity_failures =
        accepted_worker_evidence_ambiguity_failures(accepted_worker_evidence_index);
    if !ambiguity_failures.is_empty() {
        adoption_blockers.push(format!(
            "ambiguous accepted worker evidence selection: {}",
            ambiguity_failures.join("; ")
        ));
    }
    let quality_failures =
        accepted_worker_evidence_quality_floor_failures(&contract, accepted_worker_evidence_index);
    if !quality_failures.is_empty() {
        adoption_blockers.push(format!(
            "accepted worker quality floor failures: {}",
            quality_failures.join("; ")
        ));
    }
    let semantic_failures = accepted_worker_evidence_semantic_review_floor_failures(
        &contract,
        accepted_worker_evidence_index,
    );
    if !semantic_failures.is_empty() {
        adoption_blockers.push(format!(
            "accepted worker semantic review floor failures: {}",
            semantic_failures.join("; ")
        ));
    }
    let active_tool_failures = accepted_worker_evidence_active_tool_floor_failures(
        &contract,
        accepted_worker_evidence_index,
    );
    if !active_tool_failures.is_empty() {
        adoption_blockers.push(format!(
            "accepted worker active tool evidence failures: {}",
            active_tool_failures.join("; ")
        ));
    }
    if contract.evidence_plan_status == "adopted" {
        if let Some(reason) = accepted_worker_evidence_missing_adoptable_stage_synthesis_reason(
            &contract,
            accepted_worker_evidence_index,
        ) {
            adoption_blockers.push(reason);
        }
    }
    if !canonical_artifact_blockers.is_empty() {
        adoption_blockers.push(format!(
            "canonical artifact blockers: {}",
            canonical_artifact_blockers.join("; ")
        ));
    }
    let mut review_rerun_blockers = adoption_blockers.clone();
    let artifact_path = resolved.workspace_root.join(&contract.artifact_path);
    if !artifact_path.exists()
        || !std::fs::read_to_string(&artifact_path)
            .map(|content| {
                autonomous_research_stage_artifact_candidate_is_reviewable(&content, &contract)
            })
            .unwrap_or(false)
    {
        review_rerun_blockers.push(format!(
            "stage artifact is not reviewable at {}",
            contract.artifact_path
        ));
    } else if !autonomous_research_stage_artifact_has_materialization_receipt(
        resolved, job, &contract,
    ) {
        review_rerun_blockers.push(format!(
            "stage artifact at {} exists but was not materialized by adopt_stage_artifact",
            contract.artifact_path
        ));
    } else if !accepted_worker_evidence_bound_to_stage_artifact(
        resolved,
        job,
        &contract,
        accepted_worker_evidence_index,
    ) {
        review_rerun_blockers.push(format!(
            "stage artifact at {} has no active adopt_stage_artifact receipt bound to main-agent-accepted research_synthesizer evidence",
            contract.artifact_path
        ));
    } else {
        let artifact = std::fs::read_to_string(&artifact_path).unwrap_or_default();
        let evidence_binding_failures =
            autonomous_research_stage_artifact_evidence_binding_failures(
                &artifact,
                &contract,
                accepted_worker_evidence_index,
            );
        if !evidence_binding_failures.is_empty() {
            review_rerun_blockers.push(format!(
                "stage artifact evidence binding failures: {}",
                evidence_binding_failures.join("; ")
            ));
        }
    }
    if !rubric_exists {
        review_rerun_blockers.push(format!(
            "stage acceptance rubric is missing at {rubric_path}"
        ));
    }
    if latest_failed_review_repair
        .as_ref()
        .map(|repair| repair.review_rerun_blocked)
        .unwrap_or(false)
    {
        if let Some(repair) = latest_failed_review_repair.as_ref() {
            if autonomous_research_failed_review_repair_requires_evidence_gap_worker_repair(repair)
            {
                review_rerun_blockers.push(format!(
                    "latest failed review `{}` requires post-review accepted evidence-gap worker evidence cited by the stage artifact; artifact_repair/adoption-only rewrites do not count",
                    repair.review_id
                ));
            } else {
                review_rerun_blockers.push(format!(
                    "latest failed review `{}` has no post-review accepted repair/adoption evidence; main agent must convert required repairs into board-visible worker work before request_review_rerun",
                    repair.review_id
                ));
            }
        }
    }
    let mut next_stage_action_constraints = vec![
        "runtime ledger is factual only and must not choose research strategy".to_string(),
        "main agent must publish, update, merge, accept, reject, adopt, review, route, or cleanup through explicit tools".to_string(),
    ];
    if contract.evidence_plan_status != "adopted" {
        next_stage_action_constraints.push(
            "before evidence coverage can be judged, the main agent must record_stage_evidence_plan using standard-setting evidence or an explicitly raised stage plan".to_string(),
        );
        next_stage_action_constraints.push(
            "advisory default task catalogs may guide planning, but they are not required_task_types until adopted by the main agent".to_string(),
        );
    }
    if !missing_task_types.is_empty() {
        next_stage_action_constraints.push(
            "do not call adopt_stage_artifact or request_review_rerun until missing task types have accepted worker evidence".to_string(),
        );
        next_stage_action_constraints.push(
            "proposal synthesis cannot substitute for a missing required task type such as method proposal".to_string(),
        );
    }
    if contract.evidence_plan_status == "adopted"
        && !accepted_worker_evidence_has_adoptable_stage_synthesis(
            &contract,
            accepted_worker_evidence_index,
        )
    {
        next_stage_action_constraints.push(
            "after local task evidence is accepted, the main agent must publish or update a `stage artifact synthesis` task for worker_role=`research_synthesizer`, accept that synthesis evidence, and only then call `adopt_stage_artifact` for the canonical stage artifact".to_string(),
        );
    }
    if !quality_failures.is_empty()
        || !semantic_failures.is_empty()
        || !active_tool_failures.is_empty()
        || !canonical_artifact_blockers.is_empty()
    {
        next_stage_action_constraints.push(
            "do not call adopt_stage_artifact or request_review_rerun while accepted worker evidence fails quality, semantic-review, active-tool floors, or canonical artifact blockers".to_string(),
        );
        next_stage_action_constraints.push(
            "main agent must publish, update, merge, adopt, record integration, replace, route, or cleanup according to the exact failing evidence or canonical artifact state".to_string(),
        );
    }
    if !canonical_artifact_blockers.is_empty() {
        next_stage_action_constraints.push(
            "canonical artifact blockers must be resolved through main-agent adoption, integration-check, replacement, route, or cleanup decisions before stage closure".to_string(),
        );
    }
    if !adoption_ready_candidates.is_empty() {
        next_stage_action_constraints.push(
            "adoption-ready candidate artifact(s) exist; the main agent must accept/reject/defer them and, if accepted, call adopt_stage_artifact before publishing another same-artifact repair task".to_string(),
        );
        next_stage_action_constraints.push(
            "old worker-review failures remain useful context, but a newer adoption-ready candidate is the primary closure route until the main agent decides it".to_string(),
        );
    }
    if latest_failed_review_repair
        .as_ref()
        .map(|repair| repair.review_rerun_blocked)
        .unwrap_or(false)
    {
        if latest_failed_review_repair
            .as_ref()
            .map(autonomous_research_failed_review_repair_requires_evidence_gap_worker_repair)
            .unwrap_or(false)
        {
            next_stage_action_constraints.push(
                "latest failed review is evidence-gap blocking; main agent must publish/update/merge targeted evidence-search, verification, comparison, or audit tasks before synthesis/adoption/review rerun".to_string(),
            );
        } else {
            next_stage_action_constraints.push(
                "latest failed review has concrete required repairs and no post-review repair evidence; main agent must publish/update/merge precise board-visible repair work before any review rerun".to_string(),
            );
        }
    }
    AutonomousResearchStageClosureLedger {
        schema_version: "stage_closure_ledger.v1".to_string(),
        stage_id: contract.stage_id,
        stage_execution_id,
        artifact_type: contract.artifact_type,
        artifact_path: contract.artifact_path,
        rubric_status: if rubric_exists {
            "present".to_string()
        } else {
            "missing".to_string()
        },
        rubric_path: Some(rubric_path),
        standard_source: "standard-setting agent drafts the high stage rubric; main agent adopts, raises, or rejects it; reviewer applies the adopted rubric strictly; runtime only records and projects these facts".to_string(),
        evidence_plan_status: contract.evidence_plan_status,
        evidence_plan_ref: contract.evidence_plan_ref,
        advisory_task_types: contract.advisory_worker_task_types,
        required_task_types,
        accepted_task_types,
        missing_task_types,
        active_evidence_count,
        active_evidence_task_ids,
        active_evidence_set_ids,
        non_current_evidence_count,
        adoption_ready_candidate_count: adoption_ready_candidates.len(),
        adoption_ready_candidates,
        latest_failed_review_repair,
        canonical_artifact_count,
        canonical_adoption_requested_count,
        canonical_materialized_count,
        canonical_baseline_visible_count,
        canonical_integration_verified_count,
        canonical_active_stage_evidence_count,
        canonical_artifact_blockers,
        canonical_artifacts,
        task_type_statuses,
        published_tasks,
        latest_review_verdict: job.last_review.as_ref().map(|review| review.verdict.clone()),
        latest_review_score: job.last_review.as_ref().and_then(|review| review.score),
        adoption_blockers,
        review_rerun_blockers,
        next_stage_action_constraints,
    }
}

pub(crate) fn autonomous_research_stage_closure_canonical_artifacts(
    resolved: &ResolvedProject,
    contract: &AutonomousResearchStageContract,
    stage_execution_id: &str,
) -> Vec<AutonomousResearchCanonicalArtifactClosureSummary> {
    let Ok(ledger) = canonical_artifacts::load_ledger(&resolved.data_dir) else {
        return Vec::new();
    };
    ledger
        .entries
        .into_iter()
        .filter(|entry| {
            entry.stage_id == contract.stage_id && entry.stage_execution_id == stage_execution_id
        })
        .map(|entry| {
            let integration_evidence_refs = entry
                .integration_checks
                .iter()
                .flat_map(|check| check.evidence_refs.clone())
                .collect::<Vec<_>>();
            AutonomousResearchCanonicalArtifactClosureSummary {
                artifact_id: entry.artifact_id,
                target_artifact_path: entry.target_artifact_path,
                status: entry.status.as_str().to_string(),
                artifact_kind: entry.artifact_kind,
                task_type: entry.task_type,
                source_agent_id: entry.source_agent_id,
                source_task_id: entry.source_task_id,
                source_artifact_path: entry.source_artifact_path,
                decision_ref: entry.decision_ref,
                baseline_ref: entry.baseline_ref,
                integration_check_count: entry.integration_checks.len(),
                integration_evidence_refs,
                materialization_blocker: entry.materialization_blocker,
                baseline_blocker: entry.baseline_blocker,
            }
        })
        .collect()
}

pub(crate) fn autonomous_research_stage_closure_canonical_artifact_blockers(
    artifacts: &[AutonomousResearchCanonicalArtifactClosureSummary],
) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| {
            let status_is_blocking = matches!(
                artifact.status.as_str(),
                "materialization_blocked" | "baseline_promotion_blocked" | "integration_failed"
            );
            if !status_is_blocking {
                return None;
            }
            let detail = artifact
                .materialization_blocker
                .as_deref()
                .or(artifact.baseline_blocker.as_deref())
                .unwrap_or("no detailed blocker recorded");
            Some(format!(
                "{} is {}: {}",
                artifact.target_artifact_path, artifact.status, detail
            ))
        })
        .collect()
}

pub(crate) fn build_autonomous_research_failed_review_repair_ledger(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    accepted_worker_evidence_index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Option<AutonomousResearchFailedReviewRepairLedger> {
    let review = job
        .last_review
        .as_ref()
        .filter(|review| review.verdict != "pass")?;
    let transition_refs = autonomous_research_post_failed_review_state_transition_refs(
        resolved,
        job,
        contract,
        review,
        accepted_worker_evidence_index,
    );
    let review_rerun_blocked = accepted_worker_evidence_index
        .and_then(|index| {
            std::fs::read_to_string(resolved.workspace_root.join(&contract.artifact_path))
                .ok()
                .map(|stage_artifact| {
                    !failed_review_repair_has_post_review_state_transition(
                        resolved,
                        job,
                        contract,
                        review,
                        index,
                        &stage_artifact,
                    )
                })
        })
        .unwrap_or(true);
    Some(AutonomousResearchFailedReviewRepairLedger {
        review_id: review.review_id.clone(),
        score: review.score,
        failure_class: extract_review_failure_class(&review.response_text)
            .map(|value| sanitize_review_label(&value)),
        suggested_operation: extract_review_labeled_value(
            &review.response_text,
            "suggested operation",
        )
        .map(|value| normalize_review_operation(&value)),
        cleanup_requirement: extract_review_labeled_value(
            &review.response_text,
            "cleanup requirement",
        )
        .map(|value| sanitize_review_label(&value)),
        review_summary_path: review.review_summary_path.clone(),
        required_repairs_excerpt: extract_required_repairs_excerpt_for_prompt(
            &review.response_text,
        ),
        review_rerun_blocked,
        post_review_state_transition_refs: transition_refs,
    })
}

pub(crate) fn autonomous_research_post_failed_review_state_transition_refs(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
    accepted_worker_evidence_index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let mut refs = Vec::new();
    let requires_evidence_gap_worker_repair =
        autonomous_research_review_requires_evidence_gap_worker_repair(review);
    if let Some(index) = accepted_worker_evidence_index {
        for record in
            main_agent_stage_artifact_adoption_records_after_review(resolved, job, contract, review)
        {
            if index.entries.iter().any(|entry| {
                accepted_worker_evidence_entry_supports_adoption(entry, &record)
                    && (!requires_evidence_gap_worker_repair
                        || accepted_worker_evidence_entry_can_repair_evidence_gap(entry))
            }) {
                merge_unique_strings(
                    &mut refs,
                    vec![
                        record.adoption_ref.clone(),
                        format!("stage_artifact:{}", record.target_artifact_path),
                    ],
                );
            }
        }
        for entry in index.entries.iter().filter(|entry| {
            entry
                .created_at
                .as_deref()
                .map(|created_at| timestamp_string_is_after(created_at, &review.created_at))
                .unwrap_or(false)
                && accepted_worker_review_result_passes_after_failure(entry)
                && (!requires_evidence_gap_worker_repair
                    || accepted_worker_evidence_entry_can_repair_evidence_gap(entry))
        }) {
            merge_unique_strings(
                &mut refs,
                vec![format!(
                    "accepted_worker_evidence:{}::{}",
                    entry.agent_id, entry.task_id
                )],
            );
        }
    }
    refs
}

pub(crate) fn autonomous_research_review_requires_evidence_gap_worker_repair(
    review: &AutonomousResearchReviewState,
) -> bool {
    let failure_class = extract_review_failure_class(&review.response_text)
        .map(|value| sanitize_review_label(&value));
    autonomous_research_failure_text_requires_evidence_gap_worker_repair(
        failure_class.as_deref(),
        &review.response_text,
    )
}

pub(crate) fn autonomous_research_failed_review_repair_requires_evidence_gap_worker_repair(
    repair: &AutonomousResearchFailedReviewRepairLedger,
) -> bool {
    autonomous_research_failure_text_requires_evidence_gap_worker_repair(
        repair.failure_class.as_deref(),
        &repair.required_repairs_excerpt,
    )
}

pub(crate) fn autonomous_research_failure_text_requires_evidence_gap_worker_repair(
    failure_class: Option<&str>,
    text: &str,
) -> bool {
    let joined = format!("{} {}", failure_class.unwrap_or_default(), text).to_ascii_lowercase();
    if joined.contains("stage_artifact_adoption_snapshot")
        && ![
            "evidence_gap",
            "evidence gap",
            "evidence-gap",
            "unexamined closest prior",
            "missing source",
            "missing-source",
            "source gap",
            "citation gap",
            "unsupported claim",
        ]
        .iter()
        .any(|needle| text.to_ascii_lowercase().contains(needle))
    {
        return false;
    }
    [
        "evidence_gap",
        "evidence gap",
        "evidence-gap",
        "unexamined closest prior",
        "unexamined_closest_prior",
        "closest-prior",
        "closest prior",
        "strongest-prior",
        "prior family",
        "prior families",
        "source grounding",
        "source-grounding",
        "insufficient_source_grounding",
        "literature_evidence_failure",
        "missing source",
        "missing-source",
        "source gap",
        "citation gap",
        "unsupported claim",
        "targeted evidence",
        "not mounted here",
    ]
    .iter()
    .any(|needle| joined.contains(needle))
}

pub(crate) fn accepted_worker_evidence_entry_can_repair_evidence_gap(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    if entry.worker_role == "research_synthesizer"
        && entry.task_type.to_ascii_lowercase().contains("synthesis")
    {
        return true;
    }
    let profile = format!(
        "{} {} {}",
        entry.task_type, entry.worker_role, entry.required_output_artifact_type
    )
    .to_ascii_lowercase();
    let tokens = profile
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let evidence_keywords = [
        "search",
        "verification",
        "citation",
        "survey",
        "comparison",
        "extraction",
        "analysis",
        "audit",
        "review",
        "cluster",
        "clustering",
        "prior",
        "source",
        "evidence",
        "baseline",
        "metric",
        "ablation",
        "experiment",
        "run",
        "log",
        "result",
        "claim",
    ];
    evidence_keywords
        .iter()
        .any(|needle| tokens.iter().any(|token| token == needle))
}

pub(crate) fn extract_required_repairs_excerpt_for_prompt(response_text: &str) -> String {
    let lowered = response_text.to_ascii_lowercase();
    let start = lowered
        .find("required repairs")
        .or_else(|| lowered.find("repair instructions"))
        .or_else(|| lowered.find("concrete repair"))
        .unwrap_or(0);
    truncate_for_prompt(response_text[start..].trim(), 2_400)
}

pub(crate) fn autonomous_research_unresolved_worker_review_failures(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<AutonomousResearchWorkerReviewFailureContext> {
    let Ok(Some(run)) = crate::orchestration::load_active_run(&resolved.data_dir) else {
        return Vec::new();
    };
    let Some(step) = run
        .steps
        .iter()
        .find(|step| step.step_id == "goal_acceptance")
    else {
        return Vec::new();
    };
    let mut failures = Vec::new();
    for artifact in &step.artifacts {
        let Some((agent_id, review_id, verdict)) =
            parse_goal_repair_review_failed_artifact_ref(artifact)
        else {
            continue;
        };
        if goal_repair_review_failure_has_main_agent_action(resolved, &run, &review_id) {
            continue;
        }
        let Ok(review) = crate::reviews::inspect(&resolved.data_dir, &review_id) else {
            continue;
        };
        let task_packet_path = resolved
            .data_dir
            .join("agents")
            .join(&agent_id)
            .join("task_packet.json");
        let task_packet = read_json_file_runtime(&task_packet_path).ok();
        let stage_task = task_packet
            .as_ref()
            .and_then(|packet| packet.get("stage_task_contract"));
        let task_id = stage_task.and_then(|value| json_string_field_runtime(value, "task_id"));
        let task_type = stage_task.and_then(|value| json_string_field_runtime(value, "task_type"));
        if autonomous_research_worker_review_failure_is_isolated_candidate_failure(
            resolved,
            job,
            &agent_id,
            task_id.as_deref(),
            task_type.as_deref(),
            &review,
        ) {
            continue;
        }
        if autonomous_research_worker_review_failure_is_superseded_by_current_evidence(
            resolved,
            job,
            &agent_id,
            task_id.as_deref(),
            task_type.as_deref(),
            &review,
        ) {
            continue;
        }
        if adoption_ready_candidate_supersedes_review_failure(resolved, job, &review) {
            continue;
        }
        let main_agent_allowed_tools =
            autonomous_research_worker_review_failure_allowed_tools(task_type.as_deref());
        failures.push(AutonomousResearchWorkerReviewFailureContext {
            main_agent_decision_id: format!("worker_review_failure_route::{review_id}"),
            main_agent_required_action: autonomous_research_worker_review_failure_required_action(
                task_type.as_deref(),
            ),
            main_agent_allowed_tools,
            repair_instruction_excerpt: extract_required_repairs_excerpt_for_prompt(
                &review.latest_trace.response_text,
            ),
            runtime_boundary: "runtime only projects this failed-review fact; main agent owns repair semantics and must publish/update/merge board work, record a decision, route, cleanup, or gate explicitly".to_string(),
            agent_id,
            review_id,
            verdict,
            task_id,
            task_type,
            worker_role: stage_task
                .and_then(|value| json_string_field_runtime(value, "worker_role")),
            failure_class: extract_review_failure_class(&review.latest_trace.response_text)
                .map(|value| sanitize_review_label(&value)),
            suggested_operation: extract_review_labeled_value(
                &review.latest_trace.response_text,
                "suggested operation",
            )
            .map(|value| normalize_review_operation(&value)),
            cleanup_requirement: extract_review_labeled_value(
                &review.latest_trace.response_text,
                "cleanup requirement",
            )
            .map(|value| sanitize_review_label(&value)),
            review_packet_ref: relative_workspace_ref(
                &resolved.workspace_root,
                Path::new(&review.packet_path),
            ),
            review_trace_ref: relative_workspace_ref(
                &resolved.workspace_root,
                Path::new(&review.latest_trace.trace_path),
            ),
            response_excerpt: compact_single_line(&review.latest_trace.response_text, 1_200),
            target_refs: review
                .packet
                .target_paths
                .iter()
                .take(8)
                .map(|target| relative_workspace_ref(&resolved.workspace_root, Path::new(target)))
                .collect(),
        });
    }
    failures.sort_by(|left, right| right.review_id.cmp(&left.review_id));
    failures.truncate(6);
    failures
}

pub(crate) fn autonomous_research_worker_review_failure_required_action(
    task_type: Option<&str>,
) -> String {
    let task = task_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("the failed worker task");
    if autonomous_research_task_type_is_standard_setting(task) {
        return "main agent must inspect the standard-setting candidate; if usable, call `record_worker_artifact_decision` and then `record_stage_evidence_plan` by copying or raising the candidate `evidence_requirements`. Do not call `adopt_stage_artifact` for `stage_acceptance_rubric.md` because stage standards are governance/evidence-plan state, not the final stage artifact".to_string();
    }
    format!(
        "main agent must inspect the strict review and decide the repair route for `{task}`; if repair is chosen, convert the review findings into precise board-visible agent-team work before any review rerun or stage closure"
    )
}

pub(crate) fn autonomous_research_task_type_is_standard_setting(task_type: &str) -> bool {
    task_type.trim() == "acceptance standard setting"
}

pub(crate) fn autonomous_research_worker_review_failure_allowed_tools(
    task_type: Option<&str>,
) -> Vec<String> {
    let tools = if task_type
        .map(autonomous_research_task_type_is_standard_setting)
        .unwrap_or(false)
    {
        [
            "record_worker_artifact_decision",
            "record_stage_evidence_plan",
            "publish_board_tasks",
            "update_board_task",
            "merge_board_tasks",
            "record_obligation_decision",
        ]
        .as_slice()
    } else {
        [
            "publish_board_tasks",
            "update_board_task",
            "merge_board_tasks",
            "record_obligation_decision",
            "request_route_change",
            "request_cleanup_plan",
            "record_worker_artifact_decision",
            "adopt_stage_artifact",
            "request_review_rerun",
        ]
        .as_slice()
    };
    tools.iter().map(ToString::to_string).collect()
}

pub(crate) fn autonomous_research_worker_review_failure_is_isolated_candidate_failure(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    failed_agent_id: &str,
    task_id: Option<&str>,
    task_type: Option<&str>,
    review: &crate::reviews::ReviewInspectionResult,
) -> bool {
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return false;
    };
    let matching_accepted_entries = accepted_worker_evidence_active_entries(&index)
        .into_iter()
        .filter(|entry| {
            accepted_worker_evidence_entry_is_main_agent_accepted(entry)
                && (task_id
                    .map(|task_id| entry.task_id == task_id)
                    .unwrap_or(false)
                    || task_type
                        .map(|task_type| entry.task_type == task_type)
                        .unwrap_or(false))
        })
        .collect::<Vec<_>>();
    if matching_accepted_entries
        .iter()
        .any(|entry| entry.agent_id == failed_agent_id)
    {
        return false;
    }
    !matching_accepted_entries.is_empty()
        && !matching_accepted_entries
            .iter()
            .any(|entry| worker_review_targets_accepted_worker_evidence(review, entry))
}

pub(crate) fn worker_review_targets_accepted_worker_evidence(
    review: &crate::reviews::ReviewInspectionResult,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    let mut target_refs = review.packet.target_paths.clone();
    merge_unique_strings(&mut target_refs, review.latest_trace.file_list.clone());
    let entry_refs = accepted_worker_evidence_entry_refs(entry);
    if target_refs.iter().any(|target| {
        entry_refs
            .iter()
            .any(|entry_ref| accepted_worker_evidence_ref_matches(entry_ref, target))
    }) {
        return true;
    }
    let response_text = &review.latest_trace.response_text;
    response_text.contains(&entry.agent_id)
        || response_text.contains(&entry.task_id)
        || entry
            .main_agent_decision_ref
            .as_ref()
            .is_some_and(|decision_ref| response_text.contains(decision_ref))
}

pub(crate) fn autonomous_research_worker_review_failure_is_superseded_by_current_evidence(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    failed_agent_id: &str,
    task_id: Option<&str>,
    task_type: Option<&str>,
    review: &crate::reviews::ReviewInspectionResult,
) -> bool {
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return false;
    };
    let matching_accepted_entries = accepted_worker_evidence_current_gate_entries(&index)
        .into_iter()
        .filter(|entry| {
            task_id
                .map(|task_id| entry.task_id == task_id)
                .unwrap_or(false)
                || task_type
                    .map(|task_type| entry.task_type == task_type)
                    .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    if matching_accepted_entries
        .iter()
        .any(|entry| worker_review_targets_accepted_worker_evidence(review, entry))
    {
        return false;
    }
    matching_accepted_entries.into_iter().any(|entry| {
        let Some(created_at) = entry.created_at.as_deref() else {
            return false;
        };
        entry.agent_id != failed_agent_id
            && (timestamp_string_is_after(created_at, &review.latest_trace.timestamp)
                || created_at == review.latest_trace.timestamp)
            && accepted_worker_review_result_passes_after_failure(entry)
            && !worker_review_targets_accepted_worker_evidence(review, entry)
    })
}

pub(crate) fn accepted_worker_review_result_passes_after_failure(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    entry
        .semantic_review
        .as_ref()
        .map(|review| review.verdict == "pass" && review.score.unwrap_or(0) > 0)
        .unwrap_or(false)
        || entry
            .main_agent_acceptance
            .as_ref()
            .map(|review| review.verdict == "pass" && review.score.unwrap_or(0) > 0)
            .unwrap_or(false)
}

pub(crate) fn parse_goal_repair_review_failed_artifact_ref(
    value: &str,
) -> Option<(String, String, String)> {
    let rest = value.strip_prefix("goal_repair_review_failed::agent:")?;
    let (agent_id, rest) = rest.split_once("::review:")?;
    let (review_id, verdict) = rest.split_once("::verdict:")?;
    if agent_id.trim().is_empty() || review_id.trim().is_empty() || verdict.trim().is_empty() {
        return None;
    }
    Some((
        agent_id.to_string(),
        review_id.to_string(),
        verdict.to_string(),
    ))
}

pub(crate) fn goal_repair_review_failure_has_main_agent_action(
    resolved: &ResolvedProject,
    run: &crate::orchestration::OrchestrationRun,
    review_id: &str,
) -> bool {
    goal_repair_review_failure_has_main_agent_action_in_run(run, review_id)
        || goal_repair_review_failure_has_main_agent_action_in_board(resolved, review_id)
}

fn goal_repair_review_failure_has_main_agent_action_in_run(
    run: &crate::orchestration::OrchestrationRun,
    review_id: &str,
) -> bool {
    run.steps.iter().any(|step| {
        step.artifacts.iter().any(|artifact| {
            artifact.contains(review_id)
                && (artifact.contains("main_agent_board_task")
                    || artifact.contains("main_agent_worker_artifact_decision"))
        })
    })
}

fn goal_repair_review_failure_has_main_agent_action_in_board(
    resolved: &ResolvedProject,
    review_id: &str,
) -> bool {
    let board_dir = resolved.data_dir.join("main-agent-board");
    for subdir in ["tasks", "task-updates", "obligation-decisions"] {
        let dir = board_dir.join(subdir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(value) = read_json_file_runtime(&path) else {
                continue;
            };
            if main_agent_board_record_routes_review_failure(&value, review_id) {
                return true;
            }
        }
    }
    false
}

fn main_agent_board_record_routes_review_failure(value: &Value, review_id: &str) -> bool {
    match value
        .get("schema_version")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
    {
        "main_agent_board_task.v1" => [
            "review_findings_refs",
            "blocker_refs",
            "review_target_task_ids",
            "review_target_evidence_refs",
            "supersedes_task_ids",
            "replacement_of_task_ids",
        ]
        .iter()
        .any(|field| {
            value
                .get(field)
                .is_some_and(|refs| json_value_contains_string(refs, review_id))
        }),
        "main_agent_control_record.v1" => {
            value.get("tool_name").and_then(|value| value.as_str()) == Some("update_board_task")
                && json_value_contains_string(value, review_id)
        }
        "main_agent_obligation_decision.v1" => {
            let obligation_matches = json_string_field_runtime(value, "obligation_id")
                .map(|obligation_id| obligation_id.contains(review_id))
                .unwrap_or(false);
            let readiness_matches = value
                .get("readiness_refs")
                .map(|refs| json_value_contains_string(refs, review_id))
                .unwrap_or(false);
            let task_refs_present = value
                .get("task_refs")
                .and_then(|refs| refs.as_array())
                .map(|refs| !refs.is_empty())
                .unwrap_or(false);
            let route = json_string_field_runtime(value, "route").unwrap_or_default();
            let review_matches = obligation_matches || readiness_matches;
            if !review_matches {
                return false;
            }
            if matches!(
                route.as_str(),
                "repair" | "retry" | "update_task" | "publish_task" | "merge_task"
            ) {
                return false;
            }
            matches!(
                route.as_str(),
                "reject"
                    | "defer"
                    | "human_gate"
                    | "cleanup"
                    | "route_change"
                    | "pivot"
                    | "rollback"
                    | "abandon"
            ) || task_refs_present
        }
        _ => false,
    }
}

fn json_value_contains_string(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.contains(needle),
        Value::Array(items) => items
            .iter()
            .any(|item| json_value_contains_string(item, needle)),
        Value::Object(map) => map
            .values()
            .any(|item| json_value_contains_string(item, needle)),
        _ => false,
    }
}

pub(crate) fn autonomous_research_stage_task_type_status(
    accepted: bool,
    missing: bool,
    tasks: &[&AutonomousResearchStageClosureTaskStatus],
) -> String {
    if accepted {
        return "accepted".to_string();
    }
    if tasks
        .iter()
        .any(|task| task.bucket_id == "running" || task.status == "running")
    {
        return "running".to_string();
    }
    if tasks
        .iter()
        .any(|task| task.bucket_id == "ready_to_run" || task.status == "ready")
    {
        return "ready_to_run".to_string();
    }
    if tasks
        .iter()
        .any(|task| task.bucket_id == "blocked" || task.status == "blocked")
    {
        return "blocked".to_string();
    }
    if tasks
        .iter()
        .any(|task| task.bucket_id == "contract_invalid" || task.status == "contract_invalid")
    {
        return "blocked".to_string();
    }
    if !tasks.is_empty() {
        return "published".to_string();
    }
    if missing {
        "missing_unpublished".to_string()
    } else {
        "not_required".to_string()
    }
}

pub(crate) fn autonomous_research_stage_closure_published_tasks(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    task_pool: Option<&AutonomousResearchTaskPoolContinuity>,
    contract: &AutonomousResearchStageContract,
) -> Vec<AutonomousResearchStageClosureTaskStatus> {
    let mut tasks = Vec::new();
    if let Some(task_pool) = task_pool {
        for entry in &task_pool.entries {
            let Some(stage_task) = entry.projected_stage_task.as_ref() else {
                continue;
            };
            if stage_task.stage_id != contract.stage_id {
                continue;
            }
            if job
                .stage_execution_id
                .as_ref()
                .map(|execution_id| stage_task.stage_execution_id != *execution_id)
                .unwrap_or(false)
            {
                continue;
            }
            tasks.push(AutonomousResearchStageClosureTaskStatus {
                task_id: stage_task.task_id.clone(),
                task_type: stage_task.task_type.clone(),
                worker_role: stage_task.worker_role.clone(),
                bucket_id: entry.bucket_id.clone(),
                status: entry.status.clone(),
                action_policy: entry
                    .action_policy
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
                source_kind: entry.source_kind.clone(),
                source_id: entry.source_id.clone(),
                stage_execution_id: stage_task.stage_execution_id.clone(),
                merged: false,
                merged_into: None,
                claimed_by_agent_id: stage_task.claimed_by_agent_id.clone(),
                contract_violation: None,
                title: entry.title.clone(),
            });
        }
    }
    tasks.extend(autonomous_research_stage_closure_board_file_tasks(
        resolved, job, contract,
    ));
    let mut deduped = Vec::new();
    for task in tasks {
        if deduped
            .iter()
            .any(|existing: &AutonomousResearchStageClosureTaskStatus| {
                existing.task_id == task.task_id
                    && existing.bucket_id == task.bucket_id
                    && existing.status == task.status
                    && existing.merged == task.merged
            })
        {
            continue;
        }
        deduped.push(task);
    }
    deduped
}

pub(crate) fn autonomous_research_stage_closure_board_file_tasks(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Vec<AutonomousResearchStageClosureTaskStatus> {
    let task_dir = resolved.data_dir.join("main-agent-board").join("tasks");
    let Ok(entries) = std::fs::read_dir(&task_dir) else {
        return Vec::new();
    };
    let mut tasks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        if value.get("schema_version").and_then(|value| value.as_str())
            != Some("main_agent_board_task.v1")
        {
            continue;
        }
        let stage_id = value
            .get("stage_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if !stage_id.is_empty() && stage_id != contract.stage_id {
            continue;
        }
        let stage_execution_id = value
            .get("stage_execution_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if let Some(active_execution_id) = job.stage_execution_id.as_ref() {
            if !stage_execution_id.is_empty() && stage_execution_id != active_execution_id {
                continue;
            }
        }
        let task_id = value
            .get("task_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if task_id.is_empty() {
            continue;
        }
        let status = value
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("published");
        let merged_into = value
            .get("merged_into")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        let merged = status == "merged";
        let contract_violation = if merged {
            None
        } else {
            autonomous_research_board_task_contract_violation_for_stage(&value, contract)
        };
        let effective_status = contract_violation
            .as_ref()
            .map(|_| "contract_invalid")
            .unwrap_or(status);
        tasks.push(AutonomousResearchStageClosureTaskStatus {
            task_id: task_id.to_string(),
            task_type: value
                .get("task_type")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string(),
            worker_role: value
                .get("worker_role")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string(),
            bucket_id: if contract_violation.is_some() {
                "contract_invalid".to_string()
            } else if merged {
                "merged".to_string()
            } else {
                "board_file".to_string()
            },
            status: effective_status.to_string(),
            action_policy: if contract_violation.is_some() {
                "blocked_invalid_contract".to_string()
            } else {
                "fact_only".to_string()
            },
            source_kind: "main_agent_board_task_file".to_string(),
            source_id: format!("main_agent_board_task::{task_id}"),
            stage_execution_id: stage_execution_id.to_string(),
            merged,
            merged_into,
            claimed_by_agent_id: None,
            contract_violation,
            title: format!(
                "{}: {}",
                if stage_id.is_empty() {
                    contract.stage_id.as_str()
                } else {
                    stage_id
                },
                value
                    .get("task_type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
            ),
        });
    }
    tasks
}

pub(crate) fn autonomous_research_board_task_contract_violation_for_stage(
    value: &Value,
    contract: &AutonomousResearchStageContract,
) -> Option<String> {
    if let Some(violation) = autonomous_research_board_task_ref_contract_violation_for_stage(value)
    {
        return Some(violation);
    }
    let task_id = value
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let supersedes_task_ids = json_value_string_array_runtime(value.get("supersedes_task_ids"));
    let replacement_of_task_ids =
        json_value_string_array_runtime(value.get("replacement_of_task_ids"));
    if let Some(violation) = crate::board_task_contract::self_reference_contract_violation(
        task_id,
        &supersedes_task_ids,
        &replacement_of_task_ids,
    ) {
        return Some(violation);
    }
    if let Some(violation) =
        autonomous_research_repair_board_task_contract_violation_for_stage(value)
    {
        return Some(violation);
    }
    if !autonomous_research_board_task_type_is_artifact_repair(
        value
            .get("task_type")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
    ) {
        return None;
    }
    let required = value
        .get("required_output_artifact_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if required.trim() == contract.artifact_type {
        return None;
    }
    let task_id = value
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    Some(format!(
        "artifact_repair task `{task_id}` must set required_output_artifact_type to `{}` for active stage `{}`, not `{required}`",
        contract.artifact_type, contract.stage_id
    ))
}

fn autonomous_research_board_task_ref_contract_violation_for_stage(
    value: &Value,
) -> Option<String> {
    let task_id = value
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    for (field, field_name) in crate::board_task_refs::BOARD_TASK_REF_ARRAY_FIELDS {
        let refs = json_value_string_array_runtime(value.get(field_name));
        if let Some(err) = crate::board_task_refs::first_ref_field_violation(*field, &refs) {
            return Some(format!("task `{task_id}` {}", err.message()));
        }
    }
    None
}

fn autonomous_research_repair_board_task_contract_violation_for_stage(
    value: &Value,
) -> Option<String> {
    let task_id = value
        .get("task_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let task_type = value
        .get("task_type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let objective = value
        .get("objective")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let review_findings_refs = json_value_string_array_runtime(value.get("review_findings_refs"));
    let blocker_refs = json_value_string_array_runtime(value.get("blocker_refs"));
    let replacement_of_task_ids =
        json_value_string_array_runtime(value.get("replacement_of_task_ids"));
    let acceptance_checks = json_value_string_array_runtime(value.get("acceptance_checks"));
    let context = crate::board_task_contract::BoardTaskRepairContext {
        task_id,
        task_type,
        objective,
        review_findings_refs: &review_findings_refs,
        blocker_refs: &blocker_refs,
        replacement_of_task_ids: &replacement_of_task_ids,
    };
    crate::board_task_contract::repair_task_contract_violation(&context, &acceptance_checks)
}

fn autonomous_research_board_task_type_is_artifact_repair(task_type: &str) -> bool {
    crate::board_task_contract::normalize_task_key(task_type).contains("artifact repair")
}

pub(crate) fn autonomous_research_next_required_action_for_snapshot(
    job: Option<&AutonomousResearchJobState>,
    task_pool: Option<&AutonomousResearchTaskPoolContinuity>,
    open_obligations: &[AutonomousResearchObligation],
    unresolved_worker_review_failures: &[AutonomousResearchWorkerReviewFailureContext],
    stage_closure_ledger: Option<&AutonomousResearchStageClosureLedger>,
) -> String {
    if let Some(ledger) = stage_closure_ledger {
        if ledger.adoption_ready_candidate_count > 0 {
            return format!(
                "Main agent must decide {} adoption-ready candidate artifact(s) for `{}` before publishing more same-artifact repair work.",
                ledger.adoption_ready_candidate_count, ledger.artifact_path
            );
        }
        if ledger
            .latest_failed_review_repair
            .as_ref()
            .map(|repair| repair.review_rerun_blocked)
            .unwrap_or(false)
        {
            if let Some(repair) = ledger.latest_failed_review_repair.as_ref() {
                return format!(
                    "Main agent must convert latest failed review `{}` required repairs into board-visible worker repair before requesting another review rerun.",
                    repair.review_id
                );
            }
        }
    }
    if let Some(failure) = unresolved_worker_review_failures.first() {
        return format!(
            "Main agent must decide worker-review repair route `{}` for review `{}` before further generic dispatch or review rerun.",
            failure.main_agent_decision_id, failure.review_id
        );
    }
    if !open_obligations.is_empty() {
        return "Main agent must explicitly handle open blocking obligations before repeated review or stage advance.".to_string();
    }
    if let Some(job) = job {
        if autonomous_research_job_terminal(&job.status) {
            return format!("Job is terminal with status `{}`.", job.status);
        }
        if let Some(review) = job.last_review.as_ref() {
            if review.verdict != "pass" {
                return "Repair the failed stage review through board-visible work, accepted evidence, or route-changing cleanup.".to_string();
            }
        }
    }
    task_pool
        .map(|pool| pool.next_recommended_action.clone())
        .filter(|action| !action.trim().is_empty())
        .unwrap_or_else(|| {
            "Attach project context; no autonomous research job is active.".to_string()
        })
}

pub(crate) fn autonomous_research_resume_recommendation_for_snapshot(
    job: Option<&AutonomousResearchJobState>,
) -> String {
    let Some(job) = job else {
        return "No autonomous research job was found; attach the active session if one exists."
            .to_string();
    };
    if autonomous_research_job_terminal(&job.status) {
        return format!(
            "Inspect terminal job `{}` before starting a new research run.",
            job.job_id
        );
    }
    match job.automation_mode {
        GoalAutomationMode::HumanInTheLoop => {
            "Attach context and show resume options; do not automatically consume research budget."
                .to_string()
        }
        GoalAutomationMode::HighAutonomy => {
            "Attach context automatically; continue only through policy-safe gates and budget checks."
                .to_string()
        }
        GoalAutomationMode::FullAuto => {
            "Attach context and allow background resume when budget, provider, and obligation gates pass."
                .to_string()
        }
    }
}

pub(crate) fn write_autonomous_research_continuity_packet(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    reason: &str,
    current_provider: Option<&crate::providers::ProviderResolutionTrace>,
) -> Result<AutonomousResearchContinuityPacketSummary, String> {
    let context_obligation_ids = sync_autonomous_research_project_context_conflict_obligations(
        job,
        &autonomous_research_project_context_files(resolved),
    );
    if !context_obligation_ids.is_empty() {
        merge_unique_strings(
            &mut job.warnings,
            vec![format!(
                "Project context file conflict obligation(s) recorded: {}",
                context_obligation_ids.join(", ")
            )],
        );
    }
    let previous_round = job.provider_rounds.last();
    let current_provider_id = current_provider.map(|trace| trace.resolved_provider.clone());
    let current_model = current_provider.map(|trace| trace.resolved_model.clone());
    let previous_provider_id = previous_round.map(|round| round.provider_id.clone());
    let previous_model = previous_round.map(|round| round.model.clone());
    let model_handoff = previous_round.is_some()
        && current_provider_id.is_some()
        && (previous_provider_id != current_provider_id || previous_model != current_model);
    let snapshot = build_autonomous_research_project_continuity_snapshot(
        resolved,
        Some(job),
        job.session_id.clone(),
    );
    let created_at = timestamp_string();
    let packet_id = format!(
        "continuity_{}_{}",
        created_at,
        short_sha256_hex(&format!(
            "{}::{reason}::{:?}::{:?}",
            job.job_id, current_provider_id, current_model
        ))
    );
    let packet = AutonomousResearchContinuityPacket {
        schema_version: "main_agent_continuity_packet.v1".to_string(),
        packet_id: packet_id.clone(),
        reason: reason.to_string(),
        job_id: job.job_id.clone(),
        project_id: resolved.project_id.clone(),
        created_at: created_at.clone(),
        snapshot: snapshot.clone(),
        previous_provider_id: previous_provider_id.clone(),
        previous_model: previous_model.clone(),
        current_provider_id: current_provider_id.clone(),
        current_model: current_model.clone(),
        model_handoff,
        previous_main_agent_decision: job
            .provider_rounds
            .last()
            .map(|round| format!("latest main-agent artifact: {}", round.artifact_path)),
        next_required_action: snapshot.next_required_action.clone(),
    };
    let packet_path = resolved
        .workspace_root
        .join("research")
        .join("auto")
        .join(&job.job_id)
        .join("continuity")
        .join(format!("{packet_id}.json"));
    if let Some(parent) = packet_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(
        &packet_path,
        serde_json::to_vec_pretty(&packet).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let artifact_path = relative_workspace_ref(&resolved.workspace_root, &packet_path);
    merge_unique_strings(&mut job.artifact_refs, vec![artifact_path.clone()]);
    let summary = AutonomousResearchContinuityPacketSummary {
        packet_id,
        reason: reason.to_string(),
        artifact_path,
        active_stage_id: snapshot.active_stage_id.clone(),
        open_blocking_obligation_count: snapshot.open_blocking_obligation_count,
        previous_provider_id,
        previous_model,
        current_provider_id,
        current_model,
        model_handoff,
        created_at,
    };
    job.continuity_packets.push(summary.clone());
    Ok(summary)
}

pub(crate) fn render_autonomous_research_continuity_for_prompt(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> String {
    let snapshot = build_autonomous_research_project_continuity_snapshot(
        resolved,
        Some(job),
        job.session_id.clone(),
    );
    let obligations = if snapshot.open_obligations.is_empty() {
        "- none".to_string()
    } else {
        snapshot
            .open_obligations
            .iter()
            .map(|obligation| {
                format!(
                    "- id=`{}` kind=`{}` stage=`{}` status=`{}` required_by=`{}` satisfied_by=`{}` detail={}",
                    obligation.obligation_id,
                    obligation.kind,
                    obligation.stage_id,
                    obligation.status,
                    obligation.required_by,
                    obligation
                        .satisfied_by
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                    obligation.detail
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let worker_review_failures = if snapshot.unresolved_worker_review_failures.is_empty() {
        "- none".to_string()
    } else {
        snapshot
            .unresolved_worker_review_failures
            .iter()
            .map(render_autonomous_research_worker_review_failure_for_prompt)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let task_pool = snapshot
        .task_pool
        .as_ref()
        .map(|pool| {
            format!(
                "total={}, ready={}, running={}, blocked={}, needs_review={}, next={}",
                pool.summary.total,
                pool.summary.ready_to_run,
                pool.summary.running,
                pool.summary.blocked,
                pool.summary.needs_review,
                pool.next_recommended_action
            )
        })
        .unwrap_or_else(|| "not available".to_string());
    let task_pool_entries = snapshot
        .task_pool
        .as_ref()
        .map(render_autonomous_research_task_pool_entries_for_prompt)
        .unwrap_or_else(|| "- not available".to_string());
    let stage_closure_ledger = snapshot
        .stage_closure_ledger
        .as_ref()
        .map(render_autonomous_research_stage_closure_ledger_for_prompt)
        .unwrap_or_else(|| "- not available".to_string());
    let stage_closure_protocol = snapshot
        .stage_closure_ledger
        .as_ref()
        .map(render_autonomous_research_stage_closure_protocol_for_prompt)
        .unwrap_or_else(|| "- not available".to_string());
    let latest_packet = job
        .continuity_packets
        .last()
        .map(|packet| {
            format!(
                "{} (reason={}, handoff={})",
                packet.artifact_path, packet.reason, packet.model_handoff
            )
        })
        .unwrap_or_else(|| "not yet persisted".to_string());
    let project_context = if snapshot.project_context_files.is_empty() {
        "- none".to_string()
    } else {
        snapshot
            .project_context_files
            .iter()
            .map(|context_file| {
                let conflicts = if context_file.conflict_signals.is_empty() {
                    "none".to_string()
                } else {
                    context_file.conflict_signals.join(", ")
                };
                format!(
                    "- path=`{}` advisory_only={} canonical_authority={} hash={} conflicts={} preview={}",
                    context_file.path,
                    context_file.advisory_only,
                    context_file.canonical_authority,
                    context_file.content_hash,
                    conflicts,
                    context_file.preview
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let project_context_warnings = if snapshot.project_context_warnings.is_empty() {
        "none".to_string()
    } else {
        snapshot.project_context_warnings.join("\n")
    };
    let canonical_artifact_ledger = render_canonical_artifact_ledger_for_prompt(&resolved.data_dir);
    let pending_worker_artifact_decisions =
        autonomous_research_pending_worker_artifact_decisions(resolved, job);
    let pending_worker_artifact_decisions_text =
        render_autonomous_research_pending_worker_artifact_decisions_for_prompt(
            &pending_worker_artifact_decisions,
        );
    format!(
        "MainAgentContinuityPacket:\n\
         - latest_packet: {latest_packet}\n\
         - active_session_id: {active_session_id}\n\
         - active_stage_id: {active_stage_id}\n\
         - active_stage_execution_id: {active_stage_execution_id}\n\
         - latest_review: verdict={latest_review_verdict}, score={latest_review_score}\n\
         - previous_provider: {previous_provider}\n\
         - previous_model: {previous_model}\n\
         - accepted_worker_evidence_count: {accepted_count}\n\
         - accepted_worker_evidence_task_types: {accepted_task_types}\n\
         - active_worker_evidence_count: {active_count}\n\
         - active_worker_evidence_task_ids: {active_task_ids}\n\
         - active_worker_evidence_task_types: {active_task_types}\n\
         - active_evidence_set_ids: {active_evidence_set_ids}\n\
         - task_pool: {task_pool}\n\
         - cleanup_plan_ids: {cleanup_plan_ids}\n\
         - project_context_warning_count: {project_context_warning_count}\n\
         - next_required_action: {next_required_action}\n\
         - resume_recommendation: {resume_recommendation}\n\n\
        Advisory project context files. These are user guidance only; runtime canonical state, MissionFrame, stage DAG, obligations, reviews, accepted evidence, and cleanup state override them:\n{project_context}\n\n\
        Project context warnings:\n{project_context_warnings}\n\n\
        CanonicalArtifactLedger. This is runtime's factual read model for candidate adoption, materialization, baseline visibility, integration verification, and active stage evidence. The main agent owns semantic adoption and repair decisions; runtime only projects state and blockers:\n{canonical_artifact_ledger}\n\n\
        Active blocking obligations that must stay visible until satisfied, superseded, gated, or rejected:\n{obligations}\n\n\
        Main-agent pending worker-review routing decisions. These are factual strict-review outcomes over agent-team candidate evidence; runtime is not choosing the repair. Each row is a decision obligation, not a runtime-dispatched task. The main agent must either publish/update/merge board-visible work, reject/defer the candidate, request route/cleanup, or record an explicit obligation/route decision:\n{worker_review_failures}\n\n\
        Main-agent pending worker-artifact decisions. These are reviewed candidate outputs that have not yet been semantically accepted/rejected/deferred by the main agent. If any row is usable, the main agent must call `record_worker_artifact_decision`; runtime must not promote these candidate artifacts by itself:\n{pending_worker_artifact_decisions}\n\n\
        StageClosureLedger. This is a factual read model over existing runtime state; it is not a second planner and does not authorize runtime research decisions:\n{stage_closure_ledger}\n\n\
        MainAgentStageClosureProtocol. This is the action order the main agent must follow when the ledger shows a closure problem. It is derived from facts above and does not authorize runtime strategy changes:\n{stage_closure_protocol}\n\n\
        StageArtifactHandoffRule. Local worker evidence is not a canonical stage artifact. The main agent must first accept local evidence tasks, then publish or update a `stage artifact synthesis` task with worker_role=`research_synthesizer` and explicit input_artifact_refs to accepted evidence. Only a main-agent-accepted research_synthesizer candidate can be adopted as `{stage_artifact_path}`. Runtime may validate, materialize, and block unsafe adoption; it must not synthesize the research artifact itself.\n\n\
        FieldRoutingTable. Use these existing fields consistently; do not invent a second state path. `input_artifact_refs` is for worker-readable supporting context such as failed review packets, accepted worker evidence, and current candidate refs. Rejected or superseded worker artifacts are never supporting context: put them only in `review_target_evidence_refs` as non-authoritative repair targets, and require the worker to quarantine or supersede them explicitly. `review_target_evidence_refs` is for the review packet or worker evidence that this task is meant to address. For review artifacts, use canonical refs like `review_packet:rev_xxx` or `review_trace:rev_xxx`; do not combine a review prefix with a full file path. `review_findings_refs` is for concrete failed-review finding ids when the task repairs a finding. `blocker_refs` is for the blocking obligation, failed review, or blocked claim being removed. `depends_on_task_ids` is for upstream board tasks whose accepted output this task consumes. `depends_on_task_ids` orders dataflow but does not mount the accepted evidence bundle; synthesis, artifact repair, or repair-task work that consumes accepted worker evidence must also cite `accepted_worker_evidence_task:*` or `accepted_worker_evidence_index:*` in `input_artifact_refs` or `review_target_evidence_refs`. `required_canonical_artifacts` is only for external canonical artifacts that already exist before the task starts; never put `{stage_artifact_path}` there when the task is producing or repairing that same artifact. `stage_closure_ledger:*` refs are main-agent-only ledger coordinates; never copy them into board-task ref arrays. Convert the concrete ledger issue into objective/acceptance_checks and, when a blocker id is needed, use `blocker_refs` with a concrete `stage_closure_blocker:*` ref. `replacement_of_task_ids` is for board-task replacement. `replacement_of_artifact_ids` belongs to `adopt_stage_artifact`: always include it; use [] for first adoption at a target path, or copy active artifact ids from CanonicalArtifactLedger when replacing an existing canonical artifact.\n\n\
        Main-agent task publication queue. These entries are facts from the existing task pool, not dispatch authorization. Entries with action_policy=`requires_main_agent_task` or source_kind ending in `_candidate` must be converted by the main agent into `publish_board_tasks`, `update_board_task`, or `merge_board_tasks` before agent-team dispatch:\n{task_pool_entries}\n\n\
        Main-agent board publication contract: every `publish_board_tasks` task must include non-empty `task_type`, `worker_role`, `objective`, `required_output_artifact_type`, `required_output_fields`, `acceptance_checks`, `failure_signals`, and `evidence_standard`. `artifact_repair` is a task type, not an output artifact type; for active stage repair tasks, set `required_output_artifact_type` to the active StageClosureLedger `artifact_type`, not to `artifact_repair`. If a failed-review repair is meant to satisfy a required worker evidence slot, keep `task_type` equal to that canonical slot such as `literature synthesis`; put the repair cause in `review_findings_refs`, `blocker_refs`, `replacement_of_task_ids`, `review_target_task_ids`, and `acceptance_checks` instead of inventing a `... repair` task type. If a task verifies, audits, clusters, compares, extracts from, synthesizes, or otherwise consumes another worker's evidence, it must declare explicit upstream dataflow through `depends_on_task_ids` and/or `input_artifact_refs`; implicit dependency batches are rejected. After an evidence-gap failed review has accepted post-review worker evidence, any synthesis, artifact repair, or repair-task work that claims to use accepted evidence must mount it with `accepted_worker_evidence_task:*` or `accepted_worker_evidence_index:*`; task ids alone only order execution and do not give the worker the evidence bundle. If a task repairs a failed review, replacement task, or blocker, it must copy the failed review/finding into `review_findings_refs`, bind the concrete blocker in `blocker_refs` or the replaced work in `replacement_of_task_ids`, and put the surgical repair criteria in `acceptance_checks` (for example: rewrite contradicted claim, downgrade unsupported conclusion, add auditable citation mapping, separate source classes, or add row-level evidence). Do not publish broad \"repair\" or \"more evidence\" tasks that do not say which review finding they close. Copy `auto_research_job`, task ids, stage ids, and evidence refs exactly from the active context instead of retyping them from memory; stale context refs are rejected. When converting a candidate, copy the candidate's stage_task fields directly and set `evidence_standard` to the concrete accepted-evidence rule that would let the strict reviewer close the current stage gate. Hollow board tasks are rejected by the tool and do not count as progress. Stage standards are drafted by `acceptance standard setting` workers, adopted or raised by the main agent, and applied by reviewers; runtime only records and projects them.",
        latest_packet = latest_packet,
        active_session_id = snapshot
            .active_session_id
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        active_stage_id = snapshot
            .active_stage_id
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        active_stage_execution_id = snapshot
            .active_stage_execution_id
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        latest_review_verdict = snapshot
            .latest_review_verdict
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        latest_review_score = snapshot
            .latest_review_score
            .map(|score| score.to_string())
            .unwrap_or_else(|| "none".to_string()),
        previous_provider = snapshot
            .latest_provider_id
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        previous_model = snapshot
            .latest_model
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        accepted_count = snapshot.accepted_worker_evidence_count,
        accepted_task_types = if snapshot.accepted_worker_evidence_task_types.is_empty() {
            "none".to_string()
        } else {
            snapshot.accepted_worker_evidence_task_types.join(", ")
        },
        active_count = snapshot.active_worker_evidence_count,
        active_task_ids = if snapshot.active_worker_evidence_task_ids.is_empty() {
            "none".to_string()
        } else {
            snapshot.active_worker_evidence_task_ids.join(", ")
        },
        active_task_types = if snapshot.active_worker_evidence_task_types.is_empty() {
            "none".to_string()
        } else {
            snapshot.active_worker_evidence_task_types.join(", ")
        },
        active_evidence_set_ids = if snapshot.active_evidence_set_ids.is_empty() {
            "none".to_string()
        } else {
            snapshot.active_evidence_set_ids.join(", ")
        },
        task_pool = task_pool,
        task_pool_entries = task_pool_entries,
        cleanup_plan_ids = if snapshot.cleanup_plan_ids.is_empty() {
            "none".to_string()
        } else {
            snapshot.cleanup_plan_ids.join(", ")
        },
        project_context_warning_count = snapshot.project_context_warnings.len(),
        next_required_action = snapshot.next_required_action,
        resume_recommendation = snapshot.resume_recommendation,
        project_context = project_context,
        project_context_warnings = project_context_warnings,
        canonical_artifact_ledger = canonical_artifact_ledger,
        obligations = obligations,
        worker_review_failures = worker_review_failures,
        pending_worker_artifact_decisions = pending_worker_artifact_decisions_text,
        stage_closure_ledger = stage_closure_ledger,
        stage_closure_protocol = stage_closure_protocol,
        stage_artifact_path = snapshot
            .stage_closure_ledger
            .as_ref()
            .map(|ledger| ledger.artifact_path.clone())
            .unwrap_or_else(|| "active stage artifact".to_string())
    )
}

pub(crate) fn render_autonomous_research_worker_review_failure_for_prompt(
    failure: &AutonomousResearchWorkerReviewFailureContext,
) -> String {
    format!(
        "- decision_id=`{}` required_action={} allowed_tools={} runtime_boundary={}\n  review_id=`{}` agent_id=`{}` verdict=`{}` task_id=`{}` task_type=`{}` worker_role=`{}` failure_class=`{}` suggested_operation=`{}` cleanup_requirement=`{}` packet=`{}` trace=`{}` targets={}\n  repair_instruction_excerpt={}\n  feedback={}",
        failure.main_agent_decision_id,
        compact_single_line(&failure.main_agent_required_action, 500),
        render_prompt_list(&failure.main_agent_allowed_tools),
        compact_single_line(&failure.runtime_boundary, 420),
        failure.review_id,
        failure.agent_id,
        failure.verdict,
        failure
            .task_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        failure
            .task_type
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        failure
            .worker_role
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        failure
            .failure_class
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        failure
            .suggested_operation
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        failure
            .cleanup_requirement
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        failure.review_packet_ref,
        failure.review_trace_ref,
        render_prompt_list(&failure.target_refs),
        compact_single_line(&failure.repair_instruction_excerpt, 1_200),
        compact_single_line(&failure.response_excerpt, 1_200)
    )
}

pub(crate) fn render_canonical_artifact_ledger_for_prompt(data_dir: &Path) -> String {
    let Ok(ledger) = canonical_artifacts::load_ledger(data_dir) else {
        return "- unavailable: canonical artifact ledger could not be loaded".to_string();
    };
    if ledger.entries.is_empty() {
        return "- none".to_string();
    }
    ledger
        .entries
        .iter()
        .map(|entry| {
            let source_path = entry
                .source_artifact_path
                .clone()
                .unwrap_or_else(|| "none".to_string());
            let source_task = entry
                .source_task_id
                .clone()
                .unwrap_or_else(|| "none".to_string());
            let baseline_ref = entry
                .baseline_ref
                .clone()
                .unwrap_or_else(|| "none".to_string());
            let blockers = [
                entry.materialization_blocker.as_deref(),
                entry.baseline_blocker.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            let blocker_text = if blockers.is_empty() {
                "none".to_string()
            } else {
                blockers.join("; ")
            };
            let integration = if entry.integration_checks.is_empty() {
                "none".to_string()
            } else {
                entry
                    .integration_checks
                    .iter()
                    .map(|check| {
                        let evidence_refs = if check.evidence_refs.is_empty() {
                            "none".to_string()
                        } else {
                            check.evidence_refs.join(",")
                        };
                        format!(
                            "{}:{}:{}:evidence_refs={}",
                            check.check_id,
                            check.status,
                            compact_single_line(&check.detail, 160),
                            evidence_refs
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            format!(
                "- artifact_id=`{}` target=`{}` status=`{}` stage=`{}` source_agent=`{}` source_task=`{}` source_path=`{}` decision_ref=`{}` baseline_ref=`{}` blockers=`{}` integration_checks=`{}`",
                entry.artifact_id,
                entry.target_artifact_path,
                entry.status.as_str(),
                entry.stage_id,
                entry.source_agent_id,
                source_task,
                source_path,
                entry.decision_ref,
                baseline_ref,
                compact_single_line(&blocker_text, 240),
                integration
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_autonomous_research_stage_closure_ledger_for_prompt(
    ledger: &AutonomousResearchStageClosureLedger,
) -> String {
    let task_statuses = if ledger.task_type_statuses.is_empty() {
        "- none".to_string()
    } else {
        ledger
            .task_type_statuses
            .iter()
            .map(|status| {
                format!(
                    "- task_type=`{}` status=`{}` accepted={} missing={} published={} ready={} running={} blocked={} merged={}",
                    status.task_type,
                    status.status,
                    status.accepted,
                    status.missing,
                    status.published_count,
                    status.ready_count,
                    status.running_count,
                    status.blocked_count,
                    status.merged_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let published_tasks = if ledger.published_tasks.is_empty() {
        "- none".to_string()
    } else {
        ledger
            .published_tasks
            .iter()
            .take(24)
            .map(|task| {
                format!(
                    "- task_id=`{}` type=`{}` worker_role=`{}` bucket=`{}` status=`{}` policy=`{}` source=`{}::{}` execution=`{}` merged={} merged_into=`{}` claimed_by=`{}` contract_violation=`{}` title={}",
                    task.task_id,
                    task.task_type,
                    task.worker_role,
                    task.bucket_id,
                    task.status,
                    task.action_policy,
                    task.source_kind,
                    task.source_id,
                    task.stage_execution_id,
                    task.merged,
                    task.merged_into
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                    task.claimed_by_agent_id
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                    task.contract_violation
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                    compact_single_line(&task.title, 180)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let adoption_ready_candidates = if ledger.adoption_ready_candidates.is_empty() {
        "- none".to_string()
    } else {
        ledger
            .adoption_ready_candidates
            .iter()
            .map(render_autonomous_research_adoption_ready_candidate_summary)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let latest_failed_review_repair = ledger
        .latest_failed_review_repair
        .as_ref()
        .map(render_autonomous_research_failed_review_repair_for_prompt)
        .unwrap_or_else(|| "- none".to_string());
    let canonical_artifacts = if ledger.canonical_artifacts.is_empty() {
        "- none".to_string()
    } else {
        ledger
            .canonical_artifacts
            .iter()
            .map(|artifact| {
                let source_task = artifact
                    .source_task_id
                    .clone()
                    .unwrap_or_else(|| "none".to_string());
                let source_path = artifact
                    .source_artifact_path
                    .clone()
                    .unwrap_or_else(|| "none".to_string());
                let baseline_ref = artifact
                    .baseline_ref
                    .clone()
                    .unwrap_or_else(|| "none".to_string());
                let integration_refs = if artifact.integration_evidence_refs.is_empty() {
                    "none".to_string()
                } else {
                    artifact.integration_evidence_refs.join(", ")
                };
                let blockers = [
                    artifact.materialization_blocker.as_deref(),
                    artifact.baseline_blocker.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                let blocker_text = if blockers.is_empty() {
                    "none".to_string()
                } else {
                    blockers.join("; ")
                };
                format!(
                    "- artifact_id=`{}` target=`{}` status=`{}` kind=`{}` task_type=`{}` source_agent=`{}` source_task=`{}` source_path=`{}` decision_ref=`{}` baseline_ref=`{}` integration_checks={} integration_evidence_refs=`{}` blockers=`{}`",
                    artifact.artifact_id,
                    artifact.target_artifact_path,
                    artifact.status,
                    artifact.artifact_kind,
                    artifact.task_type,
                    artifact.source_agent_id,
                    source_task,
                    source_path,
                    artifact.decision_ref,
                    baseline_ref,
                    artifact.integration_check_count,
                    integration_refs,
                    compact_single_line(&blocker_text, 240)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "- schema: `{}`\n\
         - stage: `{}` execution=`{}` artifact_type=`{}` artifact_path=`{}`\n\
	         - rubric_status: `{}` rubric_path=`{}`\n\
	         - standard_source: {}\n\
	         - evidence_plan_status: `{}` evidence_plan_ref=`{}`\n\
	         - advisory_task_types_not_gate: {}\n\
	         - required_task_types: {}\n\
	         - accepted_task_types: {}\n\
         - missing_task_types: {}\n\
         - active_evidence_count: {}\n\
         - active_evidence_task_ids: {}\n\
         - active_evidence_set_ids: {}\n\
         - non_current_evidence_count: {}\n\
         - adoption_ready_candidate_count: {}\n\
         - canonical_artifact_count: {}\n\
         - canonical_adoption_requested_count: {}\n\
         - canonical_materialized_count: {}\n\
         - canonical_baseline_visible_count: {}\n\
         - canonical_integration_verified_count: {}\n\
         - canonical_active_stage_evidence_count: {}\n\
         - canonical_artifact_blockers: {}\n\
         - latest_review: verdict=`{}` score=`{}`\n\
         - adoption_blockers: {}\n\
         - review_rerun_blockers: {}\n\
         - next_stage_action_constraints: {}\n\
         - latest_failed_review_repair:\n{}\n\
         - canonical_artifact_statuses:\n{}\n\
         - adoption_ready_candidates:\n{}\n\
         - task_type_statuses:\n{}\n\
         - published_task_status_table:\n{}",
        ledger.schema_version,
        ledger.stage_id,
        ledger.stage_execution_id,
        ledger.artifact_type,
        ledger.artifact_path,
        ledger.rubric_status,
        ledger
            .rubric_path
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        ledger.standard_source,
        ledger.evidence_plan_status,
        ledger
            .evidence_plan_ref
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        render_prompt_list(&ledger.advisory_task_types),
        render_prompt_list(&ledger.required_task_types),
        render_prompt_list(&ledger.accepted_task_types),
        render_prompt_list(&ledger.missing_task_types),
        ledger.active_evidence_count,
        render_prompt_list(&ledger.active_evidence_task_ids),
        render_prompt_list(&ledger.active_evidence_set_ids),
        ledger.non_current_evidence_count,
        ledger.adoption_ready_candidate_count,
        ledger.canonical_artifact_count,
        ledger.canonical_adoption_requested_count,
        ledger.canonical_materialized_count,
        ledger.canonical_baseline_visible_count,
        ledger.canonical_integration_verified_count,
        ledger.canonical_active_stage_evidence_count,
        render_prompt_list(&ledger.canonical_artifact_blockers),
        ledger
            .latest_review_verdict
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        ledger
            .latest_review_score
            .map(|score| score.to_string())
            .unwrap_or_else(|| "none".to_string()),
        render_prompt_list(&ledger.adoption_blockers),
        render_prompt_list(&ledger.review_rerun_blockers),
        render_prompt_list(&ledger.next_stage_action_constraints),
        latest_failed_review_repair,
        canonical_artifacts,
        adoption_ready_candidates,
        task_statuses,
        published_tasks
    )
}

pub(crate) fn render_autonomous_research_stage_closure_protocol_for_prompt(
    ledger: &AutonomousResearchStageClosureLedger,
) -> String {
    let missing_task_types = if ledger.missing_task_types.is_empty() {
        "none".to_string()
    } else {
        render_prompt_list(&ledger.missing_task_types)
    };
    let active_task_types = ledger
        .task_type_statuses
        .iter()
        .filter(|status| {
            status.status == "ready_to_run"
                || status.status == "running"
                || status.status == "blocked"
                || status.status == "published"
        })
        .map(|status| status.task_type.clone())
        .collect::<Vec<_>>();
    let mut lines = vec![
        "1. Read the ledger first. It is factual only and does not create research strategy.".to_string(),
        "2. Use the ledger to decide the next structured tool family, not to write prose-only closure.".to_string(),
    ];
    if !ledger.adoption_ready_candidates.is_empty() {
        let candidate_refs = ledger
            .adoption_ready_candidates
            .iter()
            .map(|candidate| {
                format!(
                    "{} from {}",
                    candidate.task_id,
                    relative_display_ref(&candidate.recommended_source_ref)
                )
            })
            .collect::<Vec<_>>();
        lines.push(format!(
            "3. Adoption-ready candidate artifacts are waiting for main-agent decision: {}. Before publishing another same-artifact repair task, inspect these refs, call `record_worker_artifact_decision`, and then either `adopt_stage_artifact` for the selected candidate or explicitly reject/defer it with rationale.",
            render_prompt_list(&candidate_refs)
        ));
    }
    if ledger
        .latest_failed_review_repair
        .as_ref()
        .map(|repair| repair.review_rerun_blocked)
        .unwrap_or(false)
    {
        if let Some(repair) = ledger.latest_failed_review_repair.as_ref() {
            if autonomous_research_failed_review_repair_requires_evidence_gap_worker_repair(repair)
            {
                lines.push(format!(
                    "3. Latest failed review `{}` is an evidence-gap blocker. Convert the gap into targeted agent-team evidence work with `publish_board_tasks`, `update_board_task`, or `merge_board_tasks`; artifact_repair/prose rewrite alone cannot satisfy this blocker, and `request_review_rerun` remains blocked until post-review accepted evidence is cited by the stage artifact.",
                    repair.review_id
                ));
            } else {
                lines.push(format!(
                    "3. Latest failed review `{}` still has no post-review accepted repair/adoption evidence. Convert its required repairs into precise `publish_board_tasks`, `update_board_task`, or `merge_board_tasks` work before any `request_review_rerun`; do not answer by accepting old supporting evidence only.",
                    repair.review_id
                ));
            }
        }
    }
    if ledger.rubric_status != "present" {
        lines.push(
            "3. If the stage acceptance rubric is missing, create or update standard-setting work. The main agent may accept, raise, reject, or defer the candidate standard, then use `record_stage_evidence_plan` for adopted requirements. Do not use `adopt_stage_artifact` to target `stage_acceptance_rubric.md`; stage standards are governance/evidence-plan state, not the final stage artifact.".to_string(),
        );
    }
    if ledger.evidence_plan_status != "adopted" {
        lines.push(
            "3. Stage evidence plan is missing. Use `record_stage_evidence_plan` to adopt or raise a concrete evidence plan from standard-setting evidence before treating any task type as required. If a standard-setting candidate is present, inspect it and copy/raise its `CandidateStageEvidencePlan.evidence_requirements` into the JSON `evidence_requirements` argument. If no standard-setting evidence exists, publish precise standard-setting board work first; runtime advisory defaults are not gates.".to_string(),
        );
    }
    if !ledger.missing_task_types.is_empty() {
        lines.push(format!(
            "4. Missing required task types: {}. Publish, update, or merge board-visible work for those exact task types. Missing required evidence blocks `adopt_stage_artifact` and `request_review_rerun` until accepted evidence covers them.",
            missing_task_types
        ));
        lines.push(
            "4a. If the accepted-evidence index already shows a candidate for a missing task type with passing independent semantic review but `main_agent_decision_ref=none`, do not publish another duplicate task first; inspect that candidate and call `record_worker_artifact_decision` to accept, reject, or defer it. Only accepted local evidence can unblock synthesis.".to_string(),
        );
    }
    if ledger.evidence_plan_status == "adopted" && active_task_types.is_empty() {
        lines.push(
            "5. No matching ready/running/blocked/published board work is visible for the missing task types. Publish precise board-visible work only for the missing requirements; do not create broad duplicate tasks.".to_string(),
        );
    } else if ledger.evidence_plan_status == "adopted" {
        lines.push(format!(
            "5. Existing board work is already visible for: {}. If the task is ready, running, blocked, or published, update or merge instead of duplicating it.",
            render_prompt_list(&active_task_types)
        ));
    }
    if ledger.evidence_plan_status == "adopted" {
        lines.push(
            "6. When accepted research_synthesizer evidence contains a reviewable final stage artifact candidate, inspect candidate refs and call `record_worker_artifact_decision` before `adopt_stage_artifact`. If the artifact is not reviewable yet, keep the stage in repair or publish the synthesis work needed to make it reviewable.".to_string(),
        );
    } else {
        lines.push(
            "6. While the stage evidence plan is missing, do not call `adopt_stage_artifact`; first route standard-setting evidence through `record_worker_artifact_decision` and `record_stage_evidence_plan` so required task types are defined by the main agent.".to_string(),
        );
    }
    if !ledger.adoption_blockers.is_empty() {
        lines.push(format!(
            "7. `adopt_stage_artifact` is blocked by: {}. Convert these exact evidence failures into board-visible repair, replacement, or independent-review tasks through `publish_board_tasks`, `update_board_task`, or `merge_board_tasks`; do not retry adoption until the blockers are gone.",
            render_prompt_list(&ledger.adoption_blockers)
        ));
    } else if !ledger.review_rerun_blockers.is_empty() {
        lines.push(format!(
            "7. `request_review_rerun` is blocked by: {}. If the problem is the stage standard, use `record_stage_evidence_plan`; if the problem is the final stage artifact, accept a research_synthesizer candidate and use `adopt_stage_artifact`; otherwise create/update standard-setting or synthesis work first. Do not request review rerun while blockers remain.",
            render_prompt_list(&ledger.review_rerun_blockers)
        ));
    } else if ledger.rubric_status == "present"
        && ledger.missing_task_types.is_empty()
        && ledger.adoption_blockers.is_empty()
        && ledger.review_rerun_blockers.is_empty()
    {
        lines.push(
            "7. When the rubric is present, required evidence is accepted, and readiness refs exist, request `request_review_rerun` with explicit readiness refs. Do not do this earlier.".to_string(),
        );
    }
    if let Some(verdict) = ledger.latest_review_verdict.as_deref() {
        if verdict != "pass" {
            lines.push(
                "8. If the latest review failed, call `record_obligation_decision` and convert the failure into board work, route change, cleanup, or a human gate. Acknowledgement is not closure.".to_string(),
            );
        }
    }
    lines.push(
        "8. A passing review is not stage closure by itself. Before `request_route_change(operation=\"advance\")`, the main agent must call `record_stage_closure_decision` with decision=`close_and_advance`, the target next stage, accepted evidence refs, the passing review ref, the reviewed stage artifact ref, remaining risks, cleanup judgment, and why no more work is needed in this stage.".to_string(),
    );
    lines.push(
        "Tool families: evidence-plan authority = `record_stage_evidence_plan`; stage-closure authority = `record_stage_closure_decision`; board publication = `publish_board_tasks`, `update_board_task`, `merge_board_tasks`; worker evidence decisions = `record_worker_artifact_decision`; adoption/integration/review = `adopt_stage_artifact`, `record_canonical_artifact_integration_check`, `request_review_rerun`; route/cleanup = `record_obligation_decision`, `request_route_change`, `request_cleanup_plan`.".to_string(),
    );
    lines.push(
        "Authority boundary: runtime projects facts only; the main agent owns the actual decision."
            .to_string(),
    );
    lines.join("\n")
}

pub(crate) fn render_autonomous_research_task_pool_entries_for_prompt(
    pool: &AutonomousResearchTaskPoolContinuity,
) -> String {
    let entries = pool
        .entries
        .iter()
        .filter(|entry| {
            entry.action_policy.as_deref() == Some("requires_main_agent_task")
                || entry.source_kind.ends_with("_candidate")
                || entry.source_kind == "goal_run_automation_policy"
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return "- none".to_string();
    }
    entries
        .into_iter()
        .map(render_autonomous_research_task_pool_entry_for_prompt)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn render_autonomous_research_task_pool_entry_for_prompt(
    entry: &AutonomousResearchTaskPoolEntryContinuity,
) -> String {
    let action_policy = entry
        .action_policy
        .clone()
        .unwrap_or_else(|| "none".to_string());
    let command = if entry.recommended_command.is_empty() {
        "none".to_string()
    } else {
        entry.recommended_command.join(" ")
    };
    let action_intents = if entry.action_refs.is_empty() {
        "none".to_string()
    } else {
        entry
            .action_refs
            .iter()
            .map(|action| {
                format!(
                    "{}:{}",
                    action.action_id,
                    compact_single_line(&action.intent, 120)
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let mut lines = vec![format!(
        "- entry_id=`{}` bucket=`{}` status=`{}` source=`{}::{}` action_policy=`{}` title={} summary={} action_intents={} recommended_command={}",
        entry.entry_id,
        entry.bucket_id,
        entry.status,
        entry.source_kind,
        entry.source_id,
        action_policy,
        compact_single_line(&entry.title, 160),
        compact_single_line(&entry.summary, 220),
        action_intents,
        command
    )];
    if let Some(stage_task) = entry.projected_stage_task.as_ref() {
        lines.push(format!(
            "  projected_stage_task: task_id=`{}` stage=`{}` execution=`{}` type=`{}` worker_role=`{}` priority={} status=`{}` objective={}",
            stage_task.task_id,
            stage_task.stage_id,
            stage_task.stage_execution_id,
            stage_task.task_type,
            stage_task.worker_role,
            stage_task.priority,
            stage_task.status,
            compact_single_line(&stage_task.objective, 260)
        ));
        lines.push(format!(
            "  projected_output: artifact_type=`{}` fields={}",
            stage_task.required_output_artifact_type,
            render_prompt_list(&stage_task.required_output_fields)
        ));
        lines.push(format!(
            "  acceptance_checks: {}",
            render_prompt_list(&stage_task.acceptance_checks)
        ));
        if !stage_task.failure_signals.is_empty() {
            lines.push(format!(
                "  failure_signals: {}",
                render_prompt_list(&stage_task.failure_signals)
            ));
        }
        if !stage_task.input_artifact_refs.is_empty() {
            lines.push(format!(
                "  input_artifact_refs: {}",
                render_prompt_list(&stage_task.input_artifact_refs)
            ));
        }
        if !stage_task.depends_on_task_ids.is_empty() {
            lines.push(format!(
                "  depends_on_task_ids: {}",
                render_prompt_list(&stage_task.depends_on_task_ids)
            ));
        }
    } else if let Some(stage_task_type) = entry.projected_stage_task_type.as_ref() {
        lines.push(format!("  projected_stage_task_type: {stage_task_type}"));
    }
    lines.join("\n")
}

pub(crate) fn render_prompt_list(values: &[String]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values
        .iter()
        .map(|value| compact_single_line(value, 160))
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(crate) fn render_autonomous_research_worker_task_catalog(
    contract: &AutonomousResearchStageContract,
) -> String {
    if !contract.worker_task_requirements.is_empty() {
        let requirements = contract
            .worker_task_requirements
            .iter()
            .map(|requirement| {
                format!(
                    "- source=`adopted_stage_evidence_plan` task_type=`{}` worker_role=`{}` required_output=`{}` objective={} required_fields={} acceptance_checks={} failure_signals={} evidence_standard={}",
                    requirement.task_type,
                    requirement.worker_role,
                    requirement.required_output_artifact_type,
                    compact_single_line(&requirement.objective, 260),
                    render_prompt_list(&requirement.required_output_fields),
                    render_prompt_list(&requirement.acceptance_checks),
                    render_prompt_list(&requirement.failure_signals),
                    compact_single_line(&requirement.evidence_standard, 260)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return format!(
            "Adopted stage evidence plan status: `{}` ref=`{}`. Required evidence tasks below are main-agent-adopted and runtime may mechanically gate against them:\n{}",
            contract.evidence_plan_status,
            contract.evidence_plan_ref.as_deref().unwrap_or("none"),
            requirements
        );
    }
    if contract.advisory_worker_task_types.is_empty() {
        return "- no adopted evidence plan and no advisory defaults".to_string();
    }
    contract
        .advisory_worker_task_types
        .iter()
        .map(|task_type| {
            format!(
                "- source=`advisory_default_not_gate` task_type=`{}` suggested_worker_role=`{}` suggested_output=`{}` suggested_objective={} suggested_fields={} suggested_checks={} suggested_failure_signals={} suggested_evidence_standard={}",
                task_type,
                autonomous_research_stage_worker_role(&contract.stage_id, task_type),
                autonomous_research_stage_task_required_output_artifact_type(
                    &contract.stage_id,
                    task_type,
                    &contract.artifact_type
                ),
                compact_single_line(
                    &autonomous_research_stage_task_objective(&contract.stage_id, task_type),
                    260
                ),
                render_prompt_list(&autonomous_research_stage_task_required_fields(
                    &contract.stage_id,
                    task_type,
                    &contract.required_fields
                )),
                render_prompt_list(&autonomous_research_stage_task_acceptance_checks(
                    &contract.stage_id,
                    task_type,
                    &contract.pass_criteria
                )),
                render_prompt_list(&autonomous_research_stage_task_failure_signals(
                    &contract.stage_id,
                    task_type,
                    &contract.failure_signals
                )),
                compact_single_line(
                    &autonomous_research_stage_task_evidence_standard(&contract.stage_id, task_type),
                    260
                )
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn autonomous_research_active_stage(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Option<research::ResearchStageExecution> {
    job.stage_execution_id
        .as_deref()
        .and_then(|execution_id| research::inspect_stage(&resolved.data_dir, execution_id).ok())
        .map(|inspection| inspection.stage_execution)
        .or_else(|| {
            research::status(&resolved.data_dir, &resolved.project_id)
                .ok()
                .and_then(|status| status.active_stage_execution)
        })
}

pub(crate) fn autonomous_research_stage_contract(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> AutonomousResearchStageContract {
    let active_stage = autonomous_research_active_stage(resolved, job);
    let stage_id = active_stage
        .as_ref()
        .map(|stage| stage.stage_id.clone())
        .unwrap_or_else(|| "literature".to_string());
    let stage_class = active_stage
        .as_ref()
        .map(|stage| stage.stage_class.clone())
        .unwrap_or_else(|| autonomous_research_stage_class(&stage_id).to_string());
    let artifact_type = autonomous_research_stage_artifact_type(&stage_id).to_string();
    let advisory_worker_task_types = autonomous_research_stage_worker_task_types(&stage_id)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let evidence_plan = load_main_agent_stage_evidence_plan_record(resolved, job, &stage_id);
    let worker_task_requirements = evidence_plan
        .as_ref()
        .map(|plan| plan.evidence_requirements.clone())
        .unwrap_or_default();
    let worker_task_types = worker_task_requirements
        .iter()
        .map(|requirement| requirement.task_type.clone())
        .collect::<Vec<_>>();
    let evidence_plan_status = if evidence_plan.is_some() {
        "adopted"
    } else {
        "missing"
    }
    .to_string();
    let evidence_plan_ref = evidence_plan.as_ref().map(|plan| plan.plan_ref.clone());
    AutonomousResearchStageContract {
        artifact_path: autonomous_research_stage_artifact_path(job, &stage_id, &artifact_type),
        review_strength: autonomous_research_stage_review_strength(&stage_id).to_string(),
        objective: autonomous_research_stage_objective(&stage_id).to_string(),
        required_fields: autonomous_research_stage_required_fields(&stage_id)
            .into_iter()
            .map(str::to_string)
            .collect(),
        pass_criteria: autonomous_research_stage_pass_criteria(&stage_id)
            .into_iter()
            .map(str::to_string)
            .collect(),
        failure_signals: autonomous_research_stage_failure_signals(&stage_id)
            .into_iter()
            .map(str::to_string)
            .collect(),
        worker_task_types,
        worker_task_requirements,
        advisory_worker_task_types,
        evidence_plan_status,
        evidence_plan_ref,
        paper_allowed: autonomous_research_stage_allows_paper(&stage_id),
        final_completion_stage: stage_id == "research-review" || stage_id == "validation",
        stage_id,
        stage_class,
        artifact_type,
    }
}

pub(crate) fn main_agent_stage_evidence_plan_latest_path(
    resolved: &ResolvedProject,
    stage_execution_id: &str,
) -> PathBuf {
    resolved
        .data_dir
        .join("main-agent-board")
        .join("stage-evidence-plans")
        .join(format!(
            "latest_{}.json",
            sanitize_file_component_runtime(stage_execution_id)
        ))
}

pub(crate) fn load_main_agent_stage_evidence_plan_record(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    stage_id: &str,
) -> Option<MainAgentStageEvidencePlanRecord> {
    let stage_execution_id = job.stage_execution_id.as_ref()?;
    let path = main_agent_stage_evidence_plan_latest_path(resolved, stage_execution_id);
    let content = std::fs::read_to_string(path).ok()?;
    let plan = serde_json::from_str::<MainAgentStageEvidencePlanRecord>(&content).ok()?;
    if plan.stage_id != stage_id || plan.stage_execution_id != *stage_execution_id {
        return None;
    }
    if plan.evidence_requirements.is_empty() {
        return None;
    }
    Some(plan)
}

pub(crate) fn main_agent_stage_closure_decision_latest_path(
    resolved: &ResolvedProject,
    stage_execution_id: &str,
) -> PathBuf {
    resolved
        .data_dir
        .join("main-agent-board")
        .join("stage-closure-decisions")
        .join(format!(
            "latest_{}.json",
            sanitize_file_component_runtime(stage_execution_id)
        ))
}

pub(crate) fn load_main_agent_stage_closure_decision_record(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    stage_id: &str,
) -> Option<MainAgentStageClosureDecisionRecord> {
    let stage_execution_id = job.stage_execution_id.as_ref()?;
    let path = main_agent_stage_closure_decision_latest_path(resolved, stage_execution_id);
    let value = read_json_file_runtime(&path).ok()?;
    let record = main_agent_stage_closure_decision_record_from_value(&value)?;
    if record.stage_id != stage_id || record.stage_execution_id != *stage_execution_id {
        return None;
    }
    if record
        .job_id
        .as_deref()
        .is_some_and(|job_id| job_id != job.job_id)
    {
        return None;
    }
    Some(record)
}

pub(crate) fn main_agent_stage_closure_decision_record_from_value(
    value: &Value,
) -> Option<MainAgentStageClosureDecisionRecord> {
    if value.get("published_by").and_then(|value| value.as_str()) != Some("main_agent") {
        return None;
    }
    Some(MainAgentStageClosureDecisionRecord {
        decision_ref: json_string_field_runtime(value, "decision_ref")?,
        decision: json_string_field_runtime(value, "decision")?,
        target_stage_id: json_string_field_runtime(value, "target_stage_id")?,
        closure_rationale: json_string_field_runtime(value, "closure_rationale")?,
        why_no_more_stage_work_is_needed: json_string_field_runtime(
            value,
            "why_no_more_stage_work_is_needed",
        )?,
        accepted_evidence_refs: json_string_array_runtime(value, "accepted_evidence_refs"),
        review_ref: json_string_field_runtime(value, "review_ref")?,
        stage_artifact_ref: json_string_field_runtime(value, "stage_artifact_ref")?,
        remaining_risks: json_string_array_runtime(value, "remaining_risks"),
        cleanup_required: value
            .get("cleanup_required")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        cleanup_rationale: json_string_field_runtime(value, "cleanup_rationale")?,
        readiness_refs: json_string_array_runtime(value, "readiness_refs"),
        job_id: json_string_field_runtime(value, "job_id"),
        stage_id: json_string_field_runtime(value, "stage_id")?,
        stage_execution_id: json_string_field_runtime(value, "stage_execution_id")?,
        created_at: json_string_field_runtime(value, "created_at").unwrap_or_default(),
    })
}

pub(crate) fn autonomous_research_stage_artifact_path(
    job: &AutonomousResearchJobState,
    stage_id: &str,
    artifact_type: &str,
) -> String {
    match stage_id {
        "paper-write" => format!("papers/{}/main.tex", job.job_id),
        "paper-compile" => format!("papers/{}/compiled_pdf_bundle.md", job.job_id),
        "research-review" => format!(
            "research/stages/{}/{stage_id}/hard_review_packet.md",
            job.job_id
        ),
        _ => format!(
            "research/stages/{}/{stage_id}/{artifact_type}.md",
            job.job_id
        ),
    }
}

pub(crate) fn autonomous_research_stage_artifact_type(stage_id: &str) -> &'static str {
    match stage_id {
        "validation" => "system_validation_report",
        "literature" => "literature_matrix",
        "novelty" => "novelty_report",
        "refine" => "research_proposal",
        "experiment-plan" => "experiment_plan",
        "implement-solution" => "implementation_manifest",
        "run" => "run_manifest",
        "monitor" => "experiment_report",
        "result-to-claim" => "claim_table",
        "paper-plan" => "paper_outline",
        "paper-write" => "paper_tex_bundle",
        "paper-compile" => "compiled_pdf_bundle",
        "research-review" => "hard_review_packet",
        "rebuttal" => "review_response_plan",
        "meta-optimize" => "strategy_update",
        _ => "stage_artifact",
    }
}

pub(crate) fn autonomous_research_stage_class(stage_id: &str) -> &'static str {
    match stage_id {
        "validation" => "validation",
        "literature" => "survey",
        "idea" => "idea_form",
        "novelty" | "refine" => "idea_refine",
        "experiment-plan" => "experiment_design",
        "implement-solution" => "implement",
        "run" | "monitor" => "experiment_run",
        "result-to-claim" => "result_to_claim",
        "paper-plan" | "paper-write" => "document",
        "paper-compile" | "research-review" | "rebuttal" => "publish",
        "meta-optimize" => "repair",
        _ => "unknown",
    }
}

pub(crate) fn autonomous_research_stage_review_strength(stage_id: &str) -> &'static str {
    match stage_id {
        "validation" => "validation_review",
        "novelty" | "result-to-claim" => "adversarial_stage_review",
        "implement-solution" | "run" | "monitor" | "paper-compile" => "reproducibility_review",
        "research-review" => "final_hard_review",
        _ => "standard_stage_review",
    }
}

pub(crate) fn autonomous_research_stage_allows_paper(stage_id: &str) -> bool {
    matches!(
        stage_id,
        "paper-plan" | "paper-write" | "paper-compile" | "research-review" | "rebuttal"
    )
}

pub(crate) fn autonomous_research_stage_required_fields(stage_id: &str) -> Vec<&'static str> {
    match stage_id {
        "validation" => vec![
            "validation objective",
            "check results",
            "runtime evidence refs",
            "status conclusion",
            "limitations",
            "remaining risks",
        ],
        "literature" => vec![
            "research question",
            "citation ledger",
            "source entries",
            "canonical verified title",
            "source verification status",
            "metadata confidence",
            "method family",
            "problem setting",
            "key evidence",
            "limitation",
            "relation to current objective",
            "closest-family coverage note",
            "claim support boundary",
            "missing-source risks",
        ],
        "novelty" => vec![
            "closest prior table",
            "overlap analysis",
            "novelty risks",
            "what appears new",
            "what is explicitly not new",
            "rejected novelty angles",
            "reviewer attack points",
        ],
        "refine" => vec![
            "problem statement",
            "motivation from literature",
            "hypothesis or expected contribution",
            "method sketch",
            "required implementation components",
            "validation path",
            "risk list",
            "rollback triggers",
        ],
        "experiment-plan" => vec![
            "experiment questions",
            "dataset/task/environment",
            "baselines",
            "metrics",
            "ablations",
            "run matrix",
            "commands or command templates",
            "expected output paths",
            "compute budget",
            "failure diagnosis plan",
        ],
        "implement-solution" => vec![
            "implemented method components",
            "baseline components",
            "code entry points",
            "config files",
            "tests and smoke commands",
            "known limitations",
            "mapping from code to experiment plan",
            "reproducibility instructions",
        ],
        "run" => vec![
            "run id",
            "command",
            "code version",
            "config snapshot",
            "environment snapshot",
            "seed/randomness",
            "stdout/stderr refs",
            "raw output refs",
            "status or typed failure",
        ],
        "monitor" => vec![
            "metric table",
            "raw result refs",
            "parsing script or command",
            "anomaly list",
            "missing run list",
            "baseline comparison",
            "statistical or repeated-run notes",
            "recommended reruns or repairs",
        ],
        "result-to-claim" => vec![
            "claim",
            "evidence refs",
            "support level",
            "scope and assumptions",
            "limitations",
            "required extra evidence if partial",
            "deletion/narrowing decision",
        ],
        "paper-plan" => vec![
            "section list",
            "claim-to-section map",
            "figure/table plan",
            "citation plan",
            "evidence refs per section",
            "writing risks",
        ],
        "paper-write" => vec![
            "TeX source path",
            "bib/citation refs",
            "figure/table refs",
            "reproducibility appendix",
            "limitation section",
            "claim table ref",
            "source bundle manifest",
        ],
        "paper-compile" => vec![
            "PDF path",
            "source bundle ref",
            "build command",
            "build log",
            "unresolved references or warnings",
            "PDF validation result",
        ],
        "research-review" => vec![
            "PDF and TeX refs",
            "claim table ref",
            "literature matrix ref",
            "experiment report ref",
            "implementation manifest ref",
            "reproducibility audit",
            "novelty audit",
            "verdict, score, findings, repair routes",
        ],
        "rebuttal" => vec![
            "review finding",
            "failure class",
            "affected stage",
            "selected operation",
            "repair task ids",
            "required artifacts",
            "acceptance checks",
            "rejected reviewer demands and rationale",
        ],
        "meta-optimize" => vec![
            "repeated failure pattern",
            "root cause",
            "stage contract adjustment if needed",
            "task decomposition change",
            "reviewer change",
            "next stage target",
            "constraints for next loop",
        ],
        _ => vec![
            "stage objective",
            "evidence refs",
            "limitations",
            "next action",
        ],
    }
}

pub(crate) fn autonomous_research_stage_worker_task_types(stage_id: &str) -> Vec<&'static str> {
    match stage_id {
        "validation" => vec!["system validation", "stage artifact synthesis"],
        "literature" => vec![
            "paper search",
            "source verification",
            "citation verification",
            "paper clustering",
            "closest-family survey",
            "method comparison",
            "open-problem extraction",
            "stage artifact synthesis",
        ],
        "novelty" => vec![
            "closest-prior search",
            "overlap analysis",
            "novelty-risk review",
        ],
        "refine" => vec![
            "method proposal",
            "feasibility critique",
            "risk analysis",
            "proposal synthesis",
        ],
        "experiment-plan" => vec![
            "baseline selection",
            "metric design",
            "ablation design",
            "run matrix",
            "budget plan",
        ],
        "implement-solution" => vec![
            "method implementation",
            "baseline implementation",
            "fixture/test creation",
            "code review",
        ],
        "run" => vec![
            "experiment launch",
            "environment capture",
            "config snapshot",
            "log capture",
        ],
        "monitor" => vec![
            "metric extraction",
            "anomaly detection",
            "missing-run analysis",
        ],
        "result-to-claim" => vec![
            "evidence-to-claim mapping",
            "unsupported-claim removal",
            "limitation extraction",
        ],
        "paper-plan" => vec!["outline", "claim-to-section map", "figure/table plan"],
        "paper-write" => vec![
            "TeX section drafting",
            "citation integration",
            "reproducibility appendix",
        ],
        "paper-compile" => vec!["TeX compile", "PDF validation", "build log inspection"],
        "research-review" => vec![
            "adversarial review",
            "reproducibility review",
            "claim-evidence audit",
        ],
        _ => vec!["stage artifact synthesis"],
    }
}

pub(crate) fn autonomous_research_stage_worker_role(
    stage_id: &str,
    task_type: &str,
) -> &'static str {
    if autonomous_research_stage_task_is_synthesis(task_type) {
        return "research_synthesizer";
    }
    match stage_id {
        "literature" => match task_type {
            "source verification" | "citation verification" => "citation_auditor",
            "paper clustering" | "closest-family survey" | "method comparison" => {
                "literature_comparison_researcher"
            }
            "open-problem extraction" => "literature_gap_analyst",
            _ => "literature_researcher",
        },
        "novelty" => "novelty_reviewer",
        "refine" => {
            if task_type.contains("critique") || task_type.contains("risk") {
                "research_critic"
            } else {
                "research_planner"
            }
        }
        "experiment-plan" => "experiment_designer",
        "implement-solution" => {
            if task_type.contains("review") {
                "code_reviewer"
            } else {
                "implementation_worker"
            }
        }
        "run" => "experiment_operator",
        "monitor" => "result_analyst",
        "result-to-claim" => "claim_auditor",
        "paper-plan" => "paper_planner",
        "paper-write" => "latex_writer",
        "paper-compile" => "paper_build_operator",
        "research-review" => "hard_reviewer",
        "rebuttal" => "repair_router",
        "meta-optimize" => "process_diagnostician",
        _ => "goal_worker",
    }
}

pub(crate) fn autonomous_research_stage_task_is_synthesis(task_type: &str) -> bool {
    crate::accepted_worker_evidence::task_type_is_stage_synthesis(task_type)
}

pub(crate) fn autonomous_research_stage_task_required_output_artifact_type(
    stage_id: &str,
    task_type: &str,
    default_artifact_type: &str,
) -> &'static str {
    if stage_id == "literature" {
        match task_type {
            "source verification" | "citation verification" => "citation_ledger",
            "paper clustering" => "literature_cluster_matrix",
            "closest-family survey" => "closest_family_survey",
            "method comparison" => "method_comparison_matrix",
            "open-problem extraction" => "open_problem_matrix",
            _ if autonomous_research_stage_task_is_synthesis(task_type) => "literature_matrix",
            _ => "literature_matrix",
        }
    } else {
        match default_artifact_type {
            "literature_matrix" => "literature_matrix",
            "novelty_report" => "novelty_report",
            "research_proposal" => "research_proposal",
            "experiment_plan" => "experiment_plan",
            "implementation_manifest" => "implementation_manifest",
            "run_manifest" => "run_manifest",
            "experiment_report" => "experiment_report",
            "claim_table" => "claim_table",
            "paper_outline" => "paper_outline",
            "paper_tex_bundle" => "paper_tex_bundle",
            "compiled_pdf_bundle" => "compiled_pdf_bundle",
            "hard_review_packet" => "hard_review_packet",
            "review_response_plan" => "review_response_plan",
            "strategy_update" => "strategy_update",
            _ => "stage_artifact",
        }
    }
}

pub(crate) fn autonomous_research_stage_task_objective(stage_id: &str, task_type: &str) -> String {
    if stage_id == "literature" {
        return match task_type {
            "paper search" => "Retrieve and read concrete prior-work sources for the active research objective; produce row-level source evidence rather than narrative-only related work.".to_string(),
            "source verification" => "Normalize and verify every source row against concrete metadata refs, separating verified, provisional, unverified, irrelevant, and quarantined sources.".to_string(),
            "citation verification" => "Audit citation hygiene and claim support boundaries so title/ref mismatches, project-name pollution, malformed ids, and unverified sources cannot support claims.".to_string(),
            "paper clustering" => "Cluster only row-level source entries into adjacent method families with representative papers, family-level gaps, and blocked claims.".to_string(),
            "closest-family survey" => "Rank the closest adjacent method families and identify which verified sources most constrain novelty before novelty review.".to_string(),
            "method comparison" => "Compare the candidate research direction against adjacent source-grounded methods by setting, mechanism, evidence, limitations, and allowed claim boundary.".to_string(),
            "open-problem extraction" => "Extract source-grounded open problems and route each to novelty, method design, experiment planning, or source/citation repair.".to_string(),
            _ if autonomous_research_stage_task_is_synthesis(task_type) => "Synthesize accepted literature-stage evidence, including paper search, verification, clustering, method comparison, and open-problem evidence, into one clean literature_matrix.md candidate for main-agent adoption.".to_string(),
            _ => format!("Complete `{task_type}` for literature with source-grounded evidence."),
        };
    }
    if autonomous_research_stage_task_is_synthesis(task_type) {
        return format!(
            "Synthesize accepted `{stage_id}` stage-local worker evidence into one clean `{}` candidate for main-agent adoption.",
            autonomous_research_stage_artifact_type(stage_id)
        );
    }
    format!(
        "Complete `{task_type}` for `{stage_id}` and produce auditable evidence for the active stage artifact."
    )
}

pub(crate) fn autonomous_research_stage_task_required_fields(
    stage_id: &str,
    task_type: &str,
    default_required_fields: &[String],
) -> Vec<String> {
    if stage_id == "literature" {
        let fields = match task_type {
            "source verification" | "citation verification" => vec![
                "research question",
                "citation ledger",
                "source_id",
                "paper title",
                "canonical verified title",
                "url or local ref",
                "doi/arxiv/openreview/semantic_scholar_id",
                "retrieval source",
                "source verification status",
                "metadata confidence",
                "claim support boundary",
                "rejected or quarantined sources",
                "repair tasks",
            ],
            "paper clustering" => vec![
                "research question",
                "citation ledger",
                "source entries",
                "cluster family",
                "representative papers",
                "method family",
                "family-level gap",
                "blocked claim",
                "claim support boundary",
                "repair tasks",
            ],
            _ if autonomous_research_stage_task_is_synthesis(task_type) => vec![
                "research question",
                "citation ledger",
                "source entries",
                "evidence binding ledger",
                "accepted input evidence refs",
                "evidence-to-section mapping",
                "method family",
                "method comparison",
                "problem setting",
                "key evidence",
                "limitation",
                "relation to current objective",
                "closest-family coverage note",
                "claim support boundary",
                "missing-source risks",
                "repair tasks",
            ],
            _ => vec![
                "research question",
                "citation ledger",
                "source entries",
                "canonical verified title",
                "source verification status",
                "metadata confidence",
                "method family",
                "problem setting",
                "key evidence",
                "limitation",
                "relation to current objective",
                "closest-family coverage note",
                "claim support boundary",
                "missing-source risks",
            ],
        };
        return fields.into_iter().map(str::to_string).collect();
    }
    if autonomous_research_stage_task_is_synthesis(task_type) {
        let mut fields = default_required_fields.to_vec();
        merge_unique_strings(&mut fields, vec!["evidence binding ledger".to_string()]);
        return fields;
    }
    default_required_fields.to_vec()
}

pub(crate) fn autonomous_research_stage_task_acceptance_checks(
    stage_id: &str,
    task_type: &str,
    default_pass_criteria: &[String],
) -> Vec<String> {
    if stage_id == "literature" {
        let mut checks = vec![
            "row-level source entries are present for every positive evidence claim".to_string(),
            "citation ledger rows include source ids, canonical titles, refs, verification status, metadata confidence, and claim support boundaries".to_string(),
            "verified, provisional, unverified, irrelevant, and quarantined sources are separated".to_string(),
        ];
        match task_type {
            "source verification" => {
                checks.push("every accepted/provisional source has concrete metadata verification or an explicit quarantine reason".to_string());
            }
            "citation verification" => {
                checks.push("title/ref mismatches, project-name pollution, duplicate ids, and unsupported claim boundaries are repaired or quarantined".to_string());
            }
            "paper clustering" => {
                checks.push("clusters cite row-level representative sources instead of aggregate counts only".to_string());
            }
            "closest-family survey" | "method comparison" => {
                checks.push("closest-family comparison constrains novelty and blocked claims for the active objective".to_string());
            }
            "open-problem extraction" => {
                checks.push("each open problem is tied to a verified/provisional source row and routed to a downstream stage or repair task".to_string());
            }
            _ if autonomous_research_stage_task_is_synthesis(task_type) => {
                checks.push("candidate stage artifact integrates accepted paper search, verification, clustering, method comparison, and open-problem evidence instead of leaving them as disconnected fragments".to_string());
                checks.push("candidate body is a clean review target and cites exact accepted evidence refs".to_string());
                checks.push("candidate body includes an Evidence Binding Ledger with unit_id, unit_kind, statement_or_target, support_status, accepted_evidence_refs, main_agent_decision_refs, canonical_artifact_refs, support_scope, and limitations_or_missing_risks".to_string());
            }
            _ => {}
        }
        return checks;
    }
    if autonomous_research_stage_task_is_synthesis(task_type) {
        let mut checks = default_pass_criteria.to_vec();
        checks.push("candidate stage artifact integrates accepted worker evidence and cites exact accepted evidence refs".to_string());
        checks.push(
            "candidate body is a clean review target, not a repair memo or lifecycle handoff note"
                .to_string(),
        );
        checks.push("candidate body includes an Evidence Binding Ledger with unit-level accepted evidence or canonical artifact refs".to_string());
        return checks;
    }
    default_pass_criteria.to_vec()
}

pub(crate) fn autonomous_research_stage_task_failure_signals(
    stage_id: &str,
    task_type: &str,
    default_failure_signals: &[String],
) -> Vec<String> {
    if stage_id == "literature" {
        let mut signals = vec![
            "missing citation ledger or verification status".to_string(),
            "project names, benchmark nicknames, method names, or GitHub organizations used as paper titles".to_string(),
            "title/ref mismatch not quarantined".to_string(),
            "unverified sources used to support novelty, method, experiment, or paper claims".to_string(),
        ];
        if task_type == "paper clustering" {
            signals.push("aggregate-only clustering without row-level source entries".to_string());
        }
        if autonomous_research_stage_task_is_synthesis(task_type) {
            signals.push("stage artifact candidate omits accepted method comparison or other accepted local evidence".to_string());
            signals.push("candidate body contains adoption handoff notes instead of reviewable stage content".to_string());
        }
        signals.push("generic summary without relation to objective".to_string());
        return signals;
    }
    if autonomous_research_stage_task_is_synthesis(task_type) {
        let mut signals = default_failure_signals.to_vec();
        signals.push("synthesis omits accepted worker evidence refs".to_string());
        signals
            .push("candidate body is a repair memo rather than a clean stage artifact".to_string());
        return signals;
    }
    default_failure_signals.to_vec()
}

pub(crate) fn autonomous_research_stage_task_evidence_standard(
    stage_id: &str,
    task_type: &str,
) -> String {
    if stage_id == "literature" {
        return match task_type {
            "source verification" => "A strict reviewer can trace each positive source row to a canonical title/id/ref and see whether it is verified, provisional, unverified, irrelevant, or quarantined; only verified rows may support stage-local claims.".to_string(),
            "citation verification" => "A strict reviewer can see that every title/ref pair and claim support boundary has been audited, suspicious rows are quarantined, and repair tasks exist for unresolved metadata.".to_string(),
            "paper clustering" => "A strict reviewer can inspect row-level representative papers for each cluster; aggregate counts alone do not satisfy the task.".to_string(),
            "closest-family survey" | "method comparison" => "A strict reviewer can see which verified sources define the closest families, what claims they block, and which gaps remain for novelty review.".to_string(),
            "open-problem extraction" => "A strict reviewer can trace every open problem to source rows and a downstream research or repair route.".to_string(),
            _ if autonomous_research_stage_task_is_synthesis(task_type) => "A strict reviewer can trace every section of the integrated literature_matrix.md candidate back to accepted worker evidence, including method comparison and evidence gaps; disconnected fragments or provenance-only appendices do not satisfy this task.".to_string(),
            _ => "A strict reviewer can inspect concrete retrieved/read sources, citation ledger rows, relevance judgments, limitations, and missing-source risks.".to_string(),
        };
    }
    if autonomous_research_stage_task_is_synthesis(task_type) {
        return "A strict reviewer can trace the stage artifact candidate back to accepted worker evidence and verify that it is a clean, standalone review target.".to_string();
    }
    "A strict reviewer can trace the worker output to concrete refs and use it in the active stage artifact without relying on prose-only assertions.".to_string()
}

#[cfg(test)]
pub(crate) fn autonomous_research_advisory_stage_evidence_requirements(
    stage_id: &str,
    artifact_type: &str,
    task_types: &[String],
) -> Vec<AutonomousResearchStageEvidenceRequirement> {
    let required_fields = autonomous_research_stage_required_fields(stage_id)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let pass_criteria = autonomous_research_stage_pass_criteria(stage_id)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let failure_signals = autonomous_research_stage_failure_signals(stage_id)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    task_types
        .iter()
        .map(|task_type| AutonomousResearchStageEvidenceRequirement {
            task_type: task_type.clone(),
            worker_role: autonomous_research_stage_worker_role(stage_id, task_type).to_string(),
            objective: autonomous_research_stage_task_objective(stage_id, task_type),
            required_output_artifact_type:
                autonomous_research_stage_task_required_output_artifact_type(
                    stage_id,
                    task_type,
                    artifact_type,
                )
                .to_string(),
            required_output_fields: autonomous_research_stage_task_required_fields(
                stage_id,
                task_type,
                &required_fields,
            ),
            acceptance_checks: autonomous_research_stage_task_acceptance_checks(
                stage_id,
                task_type,
                &pass_criteria,
            ),
            failure_signals: autonomous_research_stage_task_failure_signals(
                stage_id,
                task_type,
                &failure_signals,
            ),
            evidence_standard: autonomous_research_stage_task_evidence_standard(
                stage_id, task_type,
            ),
        })
        .collect()
}

pub(crate) fn autonomous_research_stage_pass_criteria(stage_id: &str) -> Vec<&'static str> {
    match stage_id {
        "validation" => vec![
            "each requested validation check has a runtime-visible result",
            "the status conclusion is bounded by cited evidence",
            "remaining risks and unverified checks are explicit",
        ],
        "literature" => vec![
            "sources include citation-ledger rows with canonical titles, refs, verification status, and claim support boundaries",
            "closest method families are compared against the current objective",
            "limitations and gaps are specific enough to drive novelty review",
            "unverified, provisional, irrelevant, and quarantined sources are separated from verified sources",
            "no formal paper claim is required at this stage",
        ],
        "novelty" => vec![
            "closest priors and overlaps are explicitly analyzed",
            "novelty risks and rejected angles are recorded",
            "reviewer attack points are concrete",
        ],
        "refine" => vec![
            "method proposal is technically concrete and motivated by literature",
            "implementation components and validation path are feasible",
            "rollback triggers are explicit",
        ],
        "experiment-plan" => vec![
            "baselines, metrics, ablations, commands, and budgets are auditable",
            "failure diagnosis is planned before runs start",
        ],
        "implement-solution" => vec![
            "code entry points, tests, configs, and reproducibility commands are recorded",
            "implementation maps back to experiment plan",
        ],
        "run" => vec![
            "run commands, code version, environment, logs, and raw outputs are recorded",
            "failures are typed and rerunnable",
        ],
        "monitor" => vec![
            "metrics are parsed from raw refs and anomalies are explained",
            "missing runs and rerun recommendations are explicit",
        ],
        "result-to-claim" => vec![
            "each claim maps to accepted evidence with support level",
            "unsupported claims are rejected or narrowed",
            "limitations and assumptions are explicit",
        ],
        "paper-plan" => vec![
            "outline maps accepted claims to sections, citations, figures, and risks",
        ],
        "paper-write" => vec![
            "TeX source and source bundle manifest exist",
            "claims cite accepted evidence and limitations are present",
        ],
        "paper-compile" => vec![
            "PDF, source bundle, build command, and build log exist",
            "unresolved references and warnings are audited",
        ],
        "research-review" => vec![
            "hard review inspects PDF, TeX, claim table, experiments, literature, and reproducibility bundle",
            "final verdict is pass with no active stage-local repair tasks",
            "cleanup plan exists for final canonicality",
        ],
        _ => vec!["required fields are present", "evidence refs are auditable"],
    }
}

pub(crate) fn autonomous_research_stage_failure_signals(stage_id: &str) -> Vec<&'static str> {
    match stage_id {
        "validation" => vec![
            "validation conclusion without runtime evidence",
            "requested check omitted from the report",
            "unverified behavior reported as passing",
        ],
        "literature" => vec![
            "missing closest-family coverage",
            "unverifiable source refs",
            "missing citation ledger or verification status",
            "project names, benchmark nicknames, or method names used as paper titles",
            "aggregate-only clustering without row-level source entries",
            "generic summary without relation to objective",
        ],
        "novelty" => vec!["unexamined closest prior", "unclear novelty boundary"],
        "refine" => vec!["vague method", "infeasible validation path"],
        "experiment-plan" => vec!["missing baseline", "missing metric", "unbounded compute"],
        "implement-solution" => vec!["missing code entry point", "failing tests"],
        "run" => vec![
            "missing logs",
            "unrecorded config",
            "typed failure without rerun plan",
        ],
        "monitor" => vec!["cherry-picked metrics", "unexplained anomaly"],
        "result-to-claim" => vec!["unsupported claim", "claim stronger than evidence"],
        "paper-plan" => vec!["sections not mapped to accepted claims"],
        "paper-write" => vec!["Markdown-only paper", "claims missing evidence refs"],
        "paper-compile" => vec!["missing PDF", "missing build log", "unresolved references"],
        "research-review" => vec!["hard review fail", "missing TeX/PDF", "missing claim table"],
        _ => vec!["missing required field", "missing evidence"],
    }
}

pub(crate) fn autonomous_research_stage_objective(stage_id: &str) -> &'static str {
    match stage_id {
        "validation" => "Validate the requested system behaviors and synthesize a canonical runtime-evidence report.",
        "literature" => "Build a grounded literature matrix before proposing claims or implementation.",
        "novelty" => "Stress-test the novelty boundary against closest prior work.",
        "refine" => "Synthesize a feasible, literature-grounded research proposal.",
        "experiment-plan" => "Design experiments, baselines, metrics, ablations, commands, and budgets.",
        "implement-solution" => "Implement the proposed method and baselines with tests and reproducibility notes.",
        "run" => "Run or record experiments with commands, configs, environment, logs, and raw outputs.",
        "monitor" => "Parse results, diagnose anomalies, and decide reruns or repairs.",
        "result-to-claim" => "Map accepted evidence to supported, partial, unsupported, or rejected claims.",
        "paper-plan" => "Plan the TeX paper from accepted claims and evidence.",
        "paper-write" => "Write canonical TeX and source bundle from the accepted paper plan.",
        "paper-compile" => "Compile PDF from canonical TeX and record build evidence.",
        "research-review" => "Run final hard review over PDF, TeX, claims, experiments, literature, and reproducibility.",
        "rebuttal" => "Map review findings to repair decisions and response plan.",
        "meta-optimize" => "Analyze repeated failure patterns and adjust the strategy.",
        _ => "Synthesize and review the active research stage artifact.",
    }
}

pub(crate) fn render_autonomous_research_stage_evidence_binding_contract(
    contract: &AutonomousResearchStageContract,
) -> String {
    let unit_kinds = crate::evidence_binding::stage_evidence_binding_unit_kinds(&contract.stage_id)
        .into_iter()
        .map(|kind| format!("- {kind}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Every canonical `{}` artifact must include a section named `Evidence Binding Ledger`. \
         This is a stage-generic contract: it does not force every stage into the same research table, \
         but it does require every future-dependent unit in the artifact body to be bound to accepted evidence, \
         canonical artifacts, or explicit missing-evidence risk.\n\n\
         Unit kinds for stage `{}`:\n{}\n\n\
         The ledger must use mechanically readable columns or keys with these names: \
         `unit_id`, `unit_kind`, `statement_or_target`, `support_status`, \
         `accepted_evidence_refs`, `main_agent_decision_refs`, `canonical_artifact_refs`, \
         `support_scope`, and `limitations_or_missing_risks`.\n\n\
         `support_status` must be one of `supported`, `partial`, `provisional`, `missing`, or `rejected`. \
         Use `accepted_worker_evidence_task:*` and exact `main_agent_worker_artifact_decision::*` refs for accepted worker evidence. \
         Use canonical project artifact refs only for baseline-visible project artifacts. \
         `.pmcli/input-bundles` paths are readable provenance context only and must never be listed as canonical support.",
        contract.artifact_type, contract.stage_id, unit_kinds
    )
}

pub(crate) fn autonomous_research_stage_contract_event(
    contract: &AutonomousResearchStageContract,
) -> Value {
    json!({
        "stage_id": contract.stage_id,
        "stage_class": contract.stage_class,
        "artifact_type": contract.artifact_type,
        "artifact_path": contract.artifact_path,
        "review_strength": contract.review_strength,
        "evidence_plan_status": contract.evidence_plan_status,
        "evidence_plan_ref": contract.evidence_plan_ref,
        "advisory_worker_task_types": contract.advisory_worker_task_types,
        "required_worker_task_types": contract.worker_task_types,
        "paper_allowed": contract.paper_allowed,
        "final_completion_stage": contract.final_completion_stage
    })
}

pub(crate) fn autonomous_research_can_auto_repair_stage(job: &AutonomousResearchJobState) -> bool {
    crate::orchestration::GoalRunAutomationPolicyProjection::from_mode(job.automation_mode)
        .action_policy("repair_failed_task")
        == Some("allowed")
}

pub(crate) fn autonomous_research_final_completion_ready(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> bool {
    if !contract.final_completion_stage {
        return false;
    }
    if crate::orchestration::GoalRunAutomationPolicyProjection::from_mode(job.automation_mode)
        .action_policy("finish_goal")
        != Some("allowed")
    {
        return false;
    }
    if contract.stage_id == "validation" {
        return resolved
            .workspace_root
            .join(&contract.artifact_path)
            .exists()
            && autonomous_research_missing_required_upstream_artifacts(resolved, job, contract)
                .is_empty()
            && job
                .last_review
                .as_ref()
                .map(|review| review.verdict.as_str())
                == Some("pass");
    }
    let hard_packet = resolved.workspace_root.join(&contract.artifact_path);
    let paper_tex = resolved
        .workspace_root
        .join(autonomous_research_stage_artifact_path(
            job,
            "paper-write",
            "paper_tex_bundle",
        ));
    let compiled_pdf_bundle =
        resolved
            .workspace_root
            .join(autonomous_research_stage_artifact_path(
                job,
                "paper-compile",
                "compiled_pdf_bundle",
            ));
    let compiled_pdf = compiled_pdf_bundle
        .parent()
        .unwrap_or(resolved.workspace_root.as_path())
        .join("build")
        .join("main.pdf");
    let cleanup_summary = autonomous_research_cleanup_summary_path(resolved, job);
    hard_packet.exists()
        && paper_tex.exists()
        && compiled_pdf_bundle.exists()
        && compiled_pdf.exists()
        && !job.cleanup_plan_ids.is_empty()
        && cleanup_summary.exists()
        && autonomous_research_missing_required_upstream_artifacts(resolved, job, contract)
            .is_empty()
        && job
            .last_review
            .as_ref()
            .map(|review| review.verdict.as_str())
            == Some("pass")
}

pub(crate) fn autonomous_research_stalled_ticks(job: &AutonomousResearchJobState) -> usize {
    job.tick_summaries
        .iter()
        .rev()
        .take_while(|tick| !tick.accepted && tick.dispatch_count == 0)
        .count()
}

pub(crate) fn autonomous_research_stage_artifact_has_reviewable_content(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> bool {
    let contract = autonomous_research_stage_contract(resolved, job);
    let path = resolved.workspace_root.join(&contract.artifact_path);
    std::fs::read_to_string(path)
        .map(|content| {
            autonomous_research_stage_artifact_candidate_is_reviewable(&content, &contract)
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub(in crate::runtime) struct AutonomousResearchRequiredUpstreamArtifact {
    stage_id: &'static str,
    artifact_type: &'static str,
    label: &'static str,
}

pub(in crate::runtime) fn autonomous_research_required_upstream_artifacts(
    stage_id: &str,
) -> Vec<AutonomousResearchRequiredUpstreamArtifact> {
    let stage = |stage_id, artifact_type, label| AutonomousResearchRequiredUpstreamArtifact {
        stage_id,
        artifact_type,
        label,
    };
    match stage_id {
        "result-to-claim" => vec![
            stage(
                "literature",
                "literature_matrix",
                "accepted literature matrix",
            ),
            stage(
                "experiment-plan",
                "experiment_plan",
                "accepted experiment plan",
            ),
            stage(
                "implement-solution",
                "implementation_manifest",
                "accepted implementation manifest",
            ),
            stage("monitor", "experiment_report", "accepted experiment report"),
        ],
        "paper-plan" => vec![
            stage(
                "literature",
                "literature_matrix",
                "accepted literature matrix",
            ),
            stage("novelty", "novelty_report", "accepted novelty report"),
            stage("monitor", "experiment_report", "accepted experiment report"),
            stage("result-to-claim", "claim_table", "accepted claim table"),
        ],
        "paper-write" => vec![
            stage(
                "literature",
                "literature_matrix",
                "accepted literature matrix",
            ),
            stage(
                "implement-solution",
                "implementation_manifest",
                "accepted implementation manifest",
            ),
            stage("monitor", "experiment_report", "accepted experiment report"),
            stage("result-to-claim", "claim_table", "accepted claim table"),
            stage("paper-plan", "paper_outline", "accepted paper outline"),
        ],
        "paper-compile" => vec![
            stage("result-to-claim", "claim_table", "accepted claim table"),
            stage("paper-plan", "paper_outline", "accepted paper outline"),
            stage("paper-write", "paper_tex_bundle", "current TeX source"),
        ],
        "research-review" => vec![
            stage(
                "literature",
                "literature_matrix",
                "accepted literature matrix",
            ),
            stage("novelty", "novelty_report", "accepted novelty report"),
            stage(
                "implement-solution",
                "implementation_manifest",
                "accepted implementation manifest",
            ),
            stage("monitor", "experiment_report", "accepted experiment report"),
            stage("result-to-claim", "claim_table", "accepted claim table"),
            stage("paper-write", "paper_tex_bundle", "current TeX source"),
            stage(
                "paper-compile",
                "compiled_pdf_bundle",
                "compiled PDF bundle",
            ),
        ],
        _ => Vec::new(),
    }
}

pub(crate) fn autonomous_research_missing_required_upstream_artifacts(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Vec<String> {
    autonomous_research_required_upstream_artifacts(&contract.stage_id)
        .into_iter()
        .filter_map(|required| {
            let relative_path = autonomous_research_stage_artifact_path(
                job,
                required.stage_id,
                required.artifact_type,
            );
            let absolute_path = resolved.workspace_root.join(&relative_path);
            match std::fs::read_to_string(&absolute_path) {
                Ok(content) if !content.trim().is_empty() => None,
                Ok(_) => Some(format!(
                    "{} missing content at {}",
                    required.label, relative_path
                )),
                Err(_) => Some(format!("{} missing at {}", required.label, relative_path)),
            }
        })
        .collect()
}

pub(crate) fn autonomous_research_accepted_worker_evidence_dir(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    stage_execution_id: &str,
) -> PathBuf {
    resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(&job.job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence")
}

pub(crate) fn autonomous_research_accepted_worker_evidence_index_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    stage_execution_id: &str,
) -> PathBuf {
    autonomous_research_accepted_worker_evidence_dir(resolved, job, stage_execution_id)
        .join("index.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutonomousResearchAcceptedWorkerArtifactProjection {
    schema_version: String,
    agent_id: String,
    projection_manifest_ref: String,
    workspace_refs: Vec<String>,
    runtime_source_refs: Vec<String>,
    entries: Vec<AutonomousResearchAcceptedWorkerArtifactProjectionEntry>,
    generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutonomousResearchAcceptedWorkerArtifactProjectionEntry {
    artifact_kind: String,
    workspace_ref: String,
    runtime_source_ref: String,
    size_bytes: u64,
    sha256: String,
}

fn project_accepted_worker_artifacts_with_refs(
    resolved: &ResolvedProject,
    job_id: &str,
    stage_execution_id: &str,
    agent_id: &str,
    extra_refs: &[String],
) -> Result<AutonomousResearchAcceptedWorkerArtifactProjection, String> {
    let projection_root = resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(job_id)
        .join(stage_execution_id)
        .join("accepted_worker_evidence")
        .join("materialized")
        .join(sanitize_runtime_path_component(agent_id));
    let projection_manifest_path = projection_root.join("projection.json");
    if projection_manifest_path.is_file() {
        return read_json_file_runtime(&projection_manifest_path).and_then(|value| {
            serde_json::from_value(value).map_err(|err| {
                format!(
                    "failed to parse accepted worker artifact projection {}: {err}",
                    projection_manifest_path.display()
                )
            })
        });
    }

    let projection_parent = projection_root
        .parent()
        .ok_or_else(|| "accepted worker projection root has no parent".to_string())?;
    std::fs::create_dir_all(projection_parent).map_err(|err| {
        format!(
            "failed to create accepted worker projection parent {}: {err}",
            projection_parent.display()
        )
    })?;
    if projection_root.exists() {
        std::fs::remove_dir_all(&projection_root).map_err(|err| {
            format!(
                "failed to clear incomplete accepted worker projection {}: {err}",
                projection_root.display()
            )
        })?;
    }
    let staging_root = projection_parent.join(format!(
        ".{}.staging-{}",
        sanitize_runtime_path_component(agent_id),
        timestamp_string()
    ));
    if staging_root.exists() {
        std::fs::remove_dir_all(&staging_root).map_err(|err| err.to_string())?;
    }
    std::fs::create_dir_all(&staging_root).map_err(|err| {
        format!(
            "failed to create accepted worker projection staging dir {}: {err}",
            staging_root.display()
        )
    })?;

    let agent_dir = resolved.data_dir.join("agents").join(agent_id);
    let mut sources = vec![
        (
            "output_manifest",
            agent_dir.join("output_manifest.json"),
            PathBuf::from("output_manifest.json"),
        ),
        (
            "task_packet",
            agent_dir.join("task_packet.json"),
            PathBuf::from("task_packet.json"),
        ),
        (
            "worker_evidence",
            agent_dir.join("provider_worker_evidence.md"),
            PathBuf::from("provider_worker_evidence.md"),
        ),
        (
            "candidate_manifest",
            agent_dir.join("worktree_artifact_candidates.json"),
            PathBuf::from("worktree_artifact_candidates.json"),
        ),
        (
            "tool_receipts",
            agent_dir.join("tool_receipts.json"),
            PathBuf::from("tool_receipts.json"),
        ),
        (
            "trace",
            agent_dir.join("traces").join("trace.json"),
            PathBuf::from("trace.json"),
        ),
        (
            "stdout",
            agent_dir.join("stdout.txt"),
            PathBuf::from("stdout.txt"),
        ),
        (
            "stderr",
            agent_dir.join("stderr.txt"),
            PathBuf::from("stderr.txt"),
        ),
        (
            "workspace_binding",
            agent_dir.join("workspace_binding.json"),
            PathBuf::from("workspace_binding.json"),
        ),
        (
            "runtime_identity",
            agent_dir.join("runtime_identity.json"),
            PathBuf::from("runtime_identity.json"),
        ),
    ];
    let candidate_manifest_path = agent_dir.join("worktree_artifact_candidates.json");
    if candidate_manifest_path.is_file() {
        let manifest = std::fs::read(&candidate_manifest_path).map_err(|err| {
            format!(
                "failed to read {}: {err}",
                candidate_manifest_path.display()
            )
        })?;
        let manifest = serde_json::from_slice::<
            crate::agents::AgentWorktreeArtifactCandidateManifest,
        >(&manifest)
        .map_err(|err| {
            format!(
                "failed to parse worker candidate manifest {}: {err}",
                candidate_manifest_path.display()
            )
        })?;
        for entry in manifest.candidate_entries {
            if !stage_artifact_adoption_candidate_path_is_safe(&entry.relative_path) {
                continue;
            }
            let source_path = entry
                .candidate_archive_ref
                .as_deref()
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .unwrap_or_else(|| Path::new(&manifest.worktree_path).join(&entry.relative_path));
            if !source_path.is_file() {
                continue;
            }
            let bytes = std::fs::read(&source_path)
                .map_err(|err| format!("failed to read {}: {err}", source_path.display()))?;
            let actual_sha256 = sha256_hex(&bytes);
            if !entry.sha256.trim().is_empty() && entry.sha256 != actual_sha256 {
                return Err(format!(
                    "worker candidate hash mismatch for {}: expected {}, observed {}",
                    entry.relative_path, entry.sha256, actual_sha256
                ));
            }
            sources.push((
                "candidate",
                source_path,
                PathBuf::from("candidates").join(&entry.relative_path),
            ));
        }
    }
    for reference in extra_refs {
        let source_path = resolve_adoption_reference_path(resolved, reference);
        if !source_path.is_file() || source_path.starts_with(&resolved.workspace_root) {
            continue;
        }
        if sources
            .iter()
            .any(|(_, existing_source, _)| existing_source == &source_path)
        {
            continue;
        }
        let destination = if source_path.starts_with(resolved.data_dir.join("reviews")) {
            let relative = source_path
                .strip_prefix(resolved.data_dir.join("reviews"))
                .map_err(|err| err.to_string())?;
            PathBuf::from("reviews").join(relative)
        } else {
            let file_name = source_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("artifact");
            PathBuf::from("runtime_evidence").join(format!(
                "{}-{file_name}",
                &sha256_hex(source_path.display().to_string().as_bytes())[..12]
            ))
        };
        sources.push(("runtime_evidence", source_path, destination));
    }

    let mut entries = Vec::new();
    let mut workspace_refs = Vec::new();
    let mut runtime_source_refs = Vec::new();
    let mut seen_destinations = BTreeSet::new();
    for (artifact_kind, source_path, relative_destination) in sources {
        if !source_path.is_file() || !seen_destinations.insert(relative_destination.clone()) {
            continue;
        }
        let bytes = std::fs::read(&source_path)
            .map_err(|err| format!("failed to read {}: {err}", source_path.display()))?;
        let staging_path = staging_root.join(&relative_destination);
        if let Some(parent) = staging_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create projection dir {}: {err}",
                    parent.display()
                )
            })?;
        }
        std::fs::write(&staging_path, &bytes)
            .map_err(|err| format!("failed to write {}: {err}", staging_path.display()))?;
        let workspace_path = projection_root.join(&relative_destination);
        let workspace_ref = relative_workspace_ref(&resolved.workspace_root, &workspace_path);
        let runtime_source_ref = source_path.display().to_string();
        merge_unique_strings(&mut workspace_refs, vec![workspace_ref.clone()]);
        merge_unique_strings(&mut runtime_source_refs, vec![runtime_source_ref.clone()]);
        entries.push(AutonomousResearchAcceptedWorkerArtifactProjectionEntry {
            artifact_kind: artifact_kind.to_string(),
            workspace_ref,
            runtime_source_ref,
            size_bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }
    let projection_manifest_ref =
        relative_workspace_ref(&resolved.workspace_root, &projection_manifest_path);
    merge_unique_strings(&mut workspace_refs, vec![projection_manifest_ref.clone()]);
    let projection = AutonomousResearchAcceptedWorkerArtifactProjection {
        schema_version: "autonomous_research.accepted_worker_artifact_projection.v1".to_string(),
        agent_id: agent_id.to_string(),
        projection_manifest_ref,
        workspace_refs,
        runtime_source_refs,
        entries,
        generated_at: timestamp_string(),
    };
    std::fs::write(
        staging_root.join("projection.json"),
        serde_json::to_vec_pretty(&projection).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    std::fs::rename(&staging_root, &projection_root).map_err(|err| {
        format!(
            "failed to publish accepted worker projection {}: {err}",
            projection_root.display()
        )
    })?;
    Ok(projection)
}

fn apply_accepted_worker_artifact_projection(
    resolved: &ResolvedProject,
    job_id: &str,
    stage_execution_id: &str,
    entry: &mut AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Result<(), String> {
    let mut source_refs = entry.evidence_refs.clone();
    merge_unique_strings(
        &mut source_refs,
        vec![
            entry.output_manifest_ref.clone(),
            entry.task_packet_ref.clone(),
        ],
    );
    let projection = project_accepted_worker_artifacts_with_refs(
        resolved,
        job_id,
        stage_execution_id,
        &entry.agent_id,
        &source_refs,
    )?;
    if let Some(output_manifest_ref) = projection
        .entries
        .iter()
        .find(|projected| projected.artifact_kind == "output_manifest")
        .map(|projected| projected.workspace_ref.clone())
    {
        entry.output_manifest_ref = output_manifest_ref;
    }
    if let Some(task_packet_ref) = projection
        .entries
        .iter()
        .find(|projected| projected.artifact_kind == "task_packet")
        .map(|projected| projected.workspace_ref.clone())
    {
        entry.task_packet_ref = task_packet_ref;
    }
    let mut workspace_refs = source_refs
        .iter()
        .filter_map(|reference| workspace_visible_accepted_worker_ref(resolved, reference))
        .collect::<Vec<_>>();
    merge_unique_strings(&mut workspace_refs, projection.workspace_refs.clone());
    entry.evidence_refs = workspace_refs;
    Ok(())
}

fn workspace_visible_accepted_worker_ref(
    resolved: &ResolvedProject,
    reference: &str,
) -> Option<String> {
    let path = Path::new(reference);
    if path.is_absolute() {
        path.starts_with(&resolved.workspace_root)
            .then(|| relative_workspace_ref(&resolved.workspace_root, path))
    } else {
        Some(reference.to_string())
    }
}

pub(crate) fn load_autonomous_research_accepted_worker_evidence_index_at_path(
    path: &Path,
) -> Option<AutonomousResearchAcceptedWorkerEvidenceIndex> {
    std::fs::read_to_string(path).ok().and_then(|content| {
        serde_json::from_str::<AutonomousResearchAcceptedWorkerEvidenceIndex>(&content).ok()
    })
}

pub(crate) fn write_autonomous_research_accepted_worker_evidence_index(
    path: &Path,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Result<(), String> {
    let mut index = index.clone();
    compact_autonomous_research_accepted_worker_evidence_entries(&mut index.entries);
    write_text_atomic_runtime(
        path,
        &serde_json::to_string_pretty(&index).map_err(|err| err.to_string())?,
    )
}

pub(crate) fn record_autonomous_research_accepted_worker_evidence(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    acceptance: &GoalTaskAcceptanceResult,
) -> Result<Option<String>, String> {
    if acceptance.status != "accepted" {
        return Ok(None);
    }
    let Some(stage_task_acceptance) = acceptance.stage_task_acceptance.as_ref() else {
        return Ok(None);
    };
    if stage_task_acceptance.verdict != "accepted" {
        return Ok(None);
    }
    let stage_execution_id = stage_task_acceptance.stage_execution_id.clone();
    let path =
        autonomous_research_accepted_worker_evidence_index_path(resolved, job, &stage_execution_id);
    let mut index = if path.exists() {
        load_autonomous_research_accepted_worker_evidence_index_at_path(&path).unwrap_or_else(
            || {
                empty_autonomous_research_accepted_worker_evidence_index(
                    resolved,
                    job,
                    stage_task_acceptance,
                )
            },
        )
    } else {
        empty_autonomous_research_accepted_worker_evidence_index(
            resolved,
            job,
            stage_task_acceptance,
        )
    };
    let mut evidence_refs = stage_task_acceptance.matched_evidence_refs.clone();
    merge_unique_strings(
        &mut evidence_refs,
        vec![
            acceptance.task_packet_ref.clone(),
            acceptance.output_manifest_ref.clone(),
        ],
    );
    merge_unique_strings(&mut evidence_refs, acceptance.trace_refs.clone());
    merge_unique_strings(
        &mut evidence_refs,
        acceptance.worker_artifact_candidate_refs.clone(),
    );
    let mut entry = AutonomousResearchAcceptedWorkerEvidenceEntry {
        agent_id: acceptance.agent_id.clone(),
        task_id: stage_task_acceptance.task_id.clone(),
        task_type: stage_task_acceptance.task_type.clone(),
        worker_role: stage_task_acceptance.worker_role.clone(),
        required_output_artifact_type: stage_task_acceptance.required_output_artifact_type.clone(),
        output_manifest_ref: acceptance.output_manifest_ref.clone(),
        task_packet_ref: acceptance.task_packet_ref.clone(),
        evidence_refs,
        matched_required_fields: stage_task_acceptance.matched_required_fields.clone(),
        matched_acceptance_checks: stage_task_acceptance.matched_acceptance_checks.clone(),
        matched_quality_signals: stage_task_acceptance.matched_quality_signals.clone(),
        quality_profile: stage_task_acceptance.quality_profile.clone(),
        semantic_review: stage_task_acceptance.semantic_review.clone(),
        main_agent_acceptance: None,
        acceptance_authority: Some("goal_acceptance".to_string()),
        main_agent_decision_ref: None,
        review_required: None,
        active_status: Some("candidate".to_string()),
        current_evidence_set_id: None,
        superseded_by_task_id: None,
        replacement_of_task_ids: Vec::new(),
        decision_reason: None,
        created_at: Some(timestamp_string()),
    };
    apply_accepted_worker_artifact_projection(
        resolved,
        &job.job_id,
        &stage_execution_id,
        &mut entry,
    )?;
    upsert_autonomous_research_accepted_worker_evidence_entry(&mut index.entries, entry);
    bind_independent_semantic_reviews_to_target_worker_evidence(resolved, &mut index);
    index.generated_at = timestamp_string();
    write_autonomous_research_accepted_worker_evidence_index(&path, &index)?;
    Ok(Some(relative_workspace_ref(
        &resolved.workspace_root,
        &path,
    )))
}

pub(crate) fn sync_main_agent_worker_artifact_decisions_into_accepted_worker_evidence(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<Vec<String>, String> {
    let records = load_main_agent_worker_artifact_decision_records(resolved)?;
    let mut changed_refs = Vec::new();
    let active_stage_execution_id = job.stage_execution_id.as_deref();
    for record in records {
        if !matches!(record.decision.as_str(), "accept" | "reject" | "defer") {
            continue;
        }
        if record
            .project_id
            .as_ref()
            .is_some_and(|project_id| project_id != &resolved.project_id)
        {
            continue;
        }
        if record
            .job_id
            .as_ref()
            .is_some_and(|job_id| job_id != &job.job_id)
        {
            continue;
        }
        if record
            .stage_id
            .as_ref()
            .is_some_and(|stage_id| stage_id != &contract.stage_id)
        {
            continue;
        }
        if active_stage_execution_id.is_some_and(|stage_execution_id| {
            record
                .stage_execution_id
                .as_deref()
                .is_some_and(|record_stage_execution_id| {
                    record_stage_execution_id != stage_execution_id
                })
        }) {
            continue;
        }
        let Some(entry) = accepted_worker_evidence_entry_from_main_agent_decision(
            resolved, job, contract, &record,
        )?
        else {
            continue;
        };
        let stage_execution_id = entry.stage_execution_id_for_index(&record, job);
        let path = autonomous_research_accepted_worker_evidence_index_path(
            resolved,
            job,
            &stage_execution_id,
        );
        let mut index = if path.exists() {
            load_autonomous_research_accepted_worker_evidence_index_at_path(&path).unwrap_or_else(
                || {
                    empty_accepted_worker_evidence_index_for_stage(
                        resolved,
                        job,
                        contract,
                        &stage_execution_id,
                    )
                },
            )
        } else {
            empty_accepted_worker_evidence_index_for_stage(
                resolved,
                job,
                contract,
                &stage_execution_id,
            )
        };
        select_main_agent_accepted_evidence_revision(&mut index.entries, &entry);
        if upsert_autonomous_research_accepted_worker_evidence_entry(&mut index.entries, entry) {
            bind_independent_semantic_reviews_to_target_worker_evidence(resolved, &mut index);
            index.generated_at = timestamp_string();
            write_autonomous_research_accepted_worker_evidence_index(&path, &index)?;
            merge_unique_strings(
                &mut changed_refs,
                vec![
                    relative_workspace_ref(&resolved.workspace_root, &path),
                    record.decision_ref.clone(),
                ],
            );
        }
    }
    if let Some(index_ref) =
        bind_existing_independent_semantic_reviews_in_accepted_worker_evidence_index(
            resolved, job, contract,
        )?
    {
        merge_unique_strings(&mut changed_refs, vec![index_ref]);
    }
    merge_unique_strings(
        &mut changed_refs,
        sync_goal_accepted_stage_task_outputs_into_accepted_worker_evidence(
            resolved, job, contract,
        )?,
    );
    Ok(changed_refs)
}

pub(crate) fn sync_goal_accepted_stage_task_outputs_into_accepted_worker_evidence(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<Vec<String>, String> {
    let Some(run) =
        crate::orchestration::load_active_run(&resolved.data_dir).map_err(|err| err.to_string())?
    else {
        return Ok(Vec::new());
    };
    let active_stage_execution_id = job.stage_execution_id.as_deref();
    let mut changed_refs = Vec::new();
    for (task_id, agent_id) in goal_accepted_stage_task_claims(&run) {
        let Some(entry) = accepted_worker_evidence_entry_from_goal_accepted_claim(
            resolved, job, contract, &task_id, &agent_id,
        )?
        else {
            continue;
        };
        if active_stage_execution_id.is_some_and(|stage_execution_id| {
            !entry.task_id.contains(stage_execution_id)
                && entry
                    .current_evidence_set_id
                    .as_deref()
                    .is_some_and(|set_id| !set_id.starts_with(&format!("{stage_execution_id}::")))
        }) {
            continue;
        }
        let stage_execution_id = job
            .stage_execution_id
            .clone()
            .unwrap_or_else(|| "stage_unknown".to_string());
        let path = autonomous_research_accepted_worker_evidence_index_path(
            resolved,
            job,
            &stage_execution_id,
        );
        let mut index = if path.exists() {
            load_autonomous_research_accepted_worker_evidence_index_at_path(&path).unwrap_or_else(
                || {
                    empty_accepted_worker_evidence_index_for_stage(
                        resolved,
                        job,
                        contract,
                        &stage_execution_id,
                    )
                },
            )
        } else {
            empty_accepted_worker_evidence_index_for_stage(
                resolved,
                job,
                contract,
                &stage_execution_id,
            )
        };
        if upsert_autonomous_research_accepted_worker_evidence_entry(&mut index.entries, entry) {
            bind_independent_semantic_reviews_to_target_worker_evidence(resolved, &mut index);
            index.generated_at = timestamp_string();
            write_autonomous_research_accepted_worker_evidence_index(&path, &index)?;
            merge_unique_strings(
                &mut changed_refs,
                vec![relative_workspace_ref(&resolved.workspace_root, &path)],
            );
        }
    }
    Ok(changed_refs)
}

pub(crate) fn goal_accepted_stage_task_claims(
    run: &crate::orchestration::OrchestrationRun,
) -> Vec<(String, String)> {
    let mut claims = BTreeSet::new();
    for artifact in run.steps.iter().flat_map(|step| step.artifacts.iter()) {
        if let Some((task_id, agent_id)) = parse_goal_accepted_stage_task_claim_ref(artifact) {
            claims.insert((task_id, agent_id));
        }
    }
    claims.into_iter().collect()
}

pub(crate) fn parse_goal_accepted_stage_task_claim_ref(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix("goal_task_claim_closed::")?;
    let (task_id, rest) = rest.rsplit_once("::agent:")?;
    let (agent_id, status) = rest.split_once("::")?;
    if status != "accepted" || task_id.trim().is_empty() || agent_id.trim().is_empty() {
        return None;
    }
    Some((task_id.to_string(), agent_id.to_string()))
}

pub(crate) fn accepted_worker_evidence_entry_from_goal_accepted_claim(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    task_id: &str,
    agent_id: &str,
) -> Result<Option<AutonomousResearchAcceptedWorkerEvidenceEntry>, String> {
    let agent_dir = resolved.data_dir.join("agents").join(agent_id);
    let task_packet_path = agent_dir.join("task_packet.json");
    let output_manifest_path = agent_dir.join("output_manifest.json");
    if !task_packet_path.exists() || !output_manifest_path.exists() {
        return Ok(None);
    }
    let task_packet = read_json_file_runtime(&task_packet_path)?;
    let output_manifest = read_json_file_runtime(&output_manifest_path)?;
    if output_manifest
        .get("status")
        .and_then(|value| value.as_str())
        != Some("complete")
        || output_manifest
            .get("validation_status")
            .and_then(|value| value.as_str())
            != Some("valid")
    {
        return Ok(None);
    }
    let stage_task = task_packet
        .get("stage_task_contract")
        .cloned()
        .unwrap_or(Value::Null);
    if json_string_field_runtime(&stage_task, "task_id").as_deref() != Some(task_id) {
        return Ok(None);
    }
    let stage_id = json_string_field_runtime(&stage_task, "stage_id")
        .unwrap_or_else(|| contract.stage_id.clone());
    if stage_id != contract.stage_id {
        return Ok(None);
    }
    let stage_execution_id = json_string_field_runtime(&stage_task, "stage_execution_id")
        .or_else(|| job.stage_execution_id.clone());
    if job.stage_execution_id.as_deref().is_some()
        && stage_execution_id.as_deref() != job.stage_execution_id.as_deref()
    {
        return Ok(None);
    }
    let task_type = json_string_field_runtime(&stage_task, "task_type")
        .unwrap_or_else(|| task_type_from_task_id_runtime(task_id));
    let worker_role = json_string_field_runtime(&stage_task, "worker_role").unwrap_or_else(|| {
        autonomous_research_stage_worker_role(&contract.stage_id, &task_type).to_string()
    });
    let required_output_artifact_type =
        json_string_field_runtime(&stage_task, "required_output_artifact_type").unwrap_or_else(
            || {
                autonomous_research_stage_task_required_output_artifact_type(
                    &contract.stage_id,
                    &task_type,
                    &contract.artifact_type,
                )
                .to_string()
            },
        );
    let mut evidence_refs = output_manifest_refs_runtime(&output_manifest);
    merge_unique_strings(
        &mut evidence_refs,
        vec![
            task_packet_path.display().to_string(),
            output_manifest_path.display().to_string(),
        ],
    );
    let semantic_review = goal_accepted_claim_semantic_review_for_agent(resolved, agent_id);
    if let Some(review) = semantic_review.as_ref() {
        merge_unique_strings(
            &mut evidence_refs,
            vec![
                review.review_packet_ref.clone(),
                review.review_trace_ref.clone(),
            ],
        );
    }
    let evidence_text = collect_goal_accepted_claim_evidence_text(
        resolved,
        &task_packet,
        &output_manifest,
        &evidence_refs,
    );
    let searchable = normalize_match_text_runtime(&evidence_text);
    let required_fields = json_value_string_array_runtime(stage_task.get("required_output_fields"));
    let acceptance_checks = json_value_string_array_runtime(stage_task.get("acceptance_checks"));
    let matched_required_fields = matched_labels_runtime(&searchable, &required_fields);
    let matched_acceptance_checks =
        matched_acceptance_checks_runtime(&searchable, &acceptance_checks);
    let matched_quality_signals =
        matched_stage_quality_signals_runtime(&searchable, &contract.stage_id, &task_type);
    let quality_profile = main_agent_decision_quality_profile(
        &required_output_artifact_type,
        &matched_required_fields,
        &required_fields,
        &matched_acceptance_checks,
        &acceptance_checks,
        &matched_quality_signals,
        &evidence_refs,
        "goal accepted claim replayed from the active orchestration run",
    );
    let decision_reason = if semantic_review.is_some() {
        "runtime replayed a previously accepted goal claim into the factual accepted-evidence index"
    } else {
        "runtime replayed a previously accepted goal claim but found no discoverable independent semantic review; stage gates must keep this evidence review-blocked"
    };
    let mut entry = AutonomousResearchAcceptedWorkerEvidenceEntry {
        agent_id: agent_id.to_string(),
        task_id: task_id.to_string(),
        task_type,
        worker_role,
        required_output_artifact_type,
        output_manifest_ref: output_manifest_path.display().to_string(),
        task_packet_ref: task_packet_path.display().to_string(),
        evidence_refs,
        matched_required_fields,
        matched_acceptance_checks,
        matched_quality_signals,
        quality_profile,
        semantic_review,
        main_agent_acceptance: None,
        acceptance_authority: Some("goal_acceptance_replay".to_string()),
        main_agent_decision_ref: None,
        review_required: Some(true),
        active_status: Some("candidate".to_string()),
        current_evidence_set_id: None,
        superseded_by_task_id: None,
        replacement_of_task_ids: json_value_string_array_runtime(
            stage_task.get("replacement_of_task_ids"),
        ),
        decision_reason: Some(decision_reason.to_string()),
        created_at: accepted_worker_evidence_source_created_at(
            &task_packet,
            Some(&output_manifest),
        ),
    };
    apply_accepted_worker_artifact_projection(
        resolved,
        &job.job_id,
        stage_execution_id.as_deref().unwrap_or(&contract.stage_id),
        &mut entry,
    )?;
    Ok(Some(entry))
}

pub(crate) fn accepted_worker_evidence_source_created_at(
    task_packet: &Value,
    output_manifest: Option<&Value>,
) -> Option<String> {
    json_string_field_runtime(task_packet, "created_at")
        .or_else(|| {
            output_manifest.and_then(|manifest| json_string_field_runtime(manifest, "generated_at"))
        })
        .or_else(|| {
            output_manifest.and_then(|manifest| json_string_field_runtime(manifest, "created_at"))
        })
        .or_else(|| Some(timestamp_string()))
}

pub(crate) fn collect_goal_accepted_claim_evidence_text(
    resolved: &ResolvedProject,
    task_packet: &Value,
    output_manifest: &Value,
    evidence_refs: &[String],
) -> String {
    let mut parts = vec![task_packet.to_string(), output_manifest.to_string()];
    for reference in evidence_refs {
        let path = resolve_adoption_reference_path(resolved, reference);
        if path.exists() && path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                parts.push(content);
            }
        }
        if path.file_name().and_then(|value| value.to_str())
            == Some("worktree_artifact_candidates.json")
        {
            if let Ok(candidate_refs) = worktree_manifest_candidate_file_refs(&path) {
                for candidate_ref in candidate_refs {
                    if let Ok(content) = std::fs::read_to_string(candidate_ref) {
                        parts.push(content);
                    }
                }
            }
        }
    }
    parts.join("\n\n")
}

pub(crate) fn worktree_manifest_candidate_file_refs(path: &Path) -> Result<Vec<PathBuf>, String> {
    let content = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(|err| err.to_string())?;
    let Some(worktree_path) = value.get("worktree_path").and_then(|value| value.as_str()) else {
        return Ok(Vec::new());
    };
    let mut candidates = json_string_array_runtime(&value, "changed_paths");
    merge_unique_strings(
        &mut candidates,
        json_string_array_runtime(&value, "untracked_paths"),
    );
    Ok(candidates
        .into_iter()
        .filter(|candidate| {
            !Path::new(candidate).is_absolute() && !candidate.split('/').any(|part| part == "..")
        })
        .map(|candidate| Path::new(worktree_path).join(candidate))
        .filter(|candidate| candidate.exists() && candidate.is_file())
        .collect())
}

pub(crate) fn goal_accepted_claim_semantic_review_for_agent(
    resolved: &ResolvedProject,
    agent_id: &str,
) -> Option<GoalStageTaskSemanticReviewResult> {
    let reviews_dir = resolved.data_dir.join("reviews");
    let entries = std::fs::read_dir(reviews_dir).ok()?;
    let mut reviews = Vec::new();
    for entry in entries.flatten() {
        let review_dir = entry.path();
        let packet_path = review_dir.join("packet.json");
        let trace_path = review_dir.join("trace.latest.json");
        if !packet_path.exists() || !trace_path.exists() {
            continue;
        }
        let Ok(packet_text) = std::fs::read_to_string(&packet_path) else {
            continue;
        };
        let Ok(packet) = serde_json::from_str::<Value>(&packet_text) else {
            continue;
        };
        if !review_packet_explicitly_targets_agent(&packet, agent_id) {
            continue;
        }
        let Ok(trace) = read_json_file_runtime(&trace_path) else {
            continue;
        };
        let response_text = trace
            .get("response_text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let Some(verdict) = trace
            .get("verdict")
            .and_then(|value| value.as_str())
            .and_then(parse_semantic_review_report_verdict)
            .or_else(|| {
                extract_review_labeled_value(response_text, "verdict")
                    .and_then(|value| parse_semantic_review_report_verdict(&value))
            })
        else {
            continue;
        };
        if verdict != "pass" {
            continue;
        }
        let Some(score) = trace
            .get("score")
            .and_then(|value| value.as_u64())
            .or_else(|| {
                extract_review_labeled_value(response_text, "score")
                    .and_then(|value| parse_semantic_review_report_score(&value))
            })
        else {
            continue;
        };
        let reviewer_role = packet
            .get("reviewer_role")
            .and_then(|value| value.as_str())
            .unwrap_or("research_quality_reviewer")
            .to_string();
        let timestamp = trace
            .get("timestamp")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        reviews.push((
            timestamp,
            GoalStageTaskSemanticReviewResult {
                schema_version: "goal_stage_task_semantic_review_result.v1".to_string(),
                verdict,
                score: Some(score),
                reviewer_role,
                review_model: trace
                    .get("model")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                execution_mode: "independent_semantic_review".to_string(),
                review_packet_ref: packet_path.display().to_string(),
                review_trace_ref: trace_path.display().to_string(),
                findings: accepted_worker_semantic_review_report_findings(response_text),
                failure_class: extract_review_failure_class(response_text)
                    .filter(|value| value != "none"),
                suggested_operation: extract_review_labeled_value(
                    response_text,
                    "suggested operation",
                ),
                rollback_target: extract_review_labeled_value(response_text, "rollback target")
                    .filter(|value| value != "none"),
                cleanup_required: extract_review_labeled_value(
                    response_text,
                    "cleanup requirement",
                )
                .map(|value| value.contains("required") && !value.contains("not required"))
                .unwrap_or(false),
                provider_id: None,
                model: trace
                    .get("model")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string),
            },
        ));
    }
    reviews.sort_by(|left, right| left.0.cmp(&right.0));
    reviews.pop().map(|(_, review)| review)
}

pub(crate) fn review_packet_explicitly_targets_agent(packet: &Value, agent_id: &str) -> bool {
    if agent_id.trim().is_empty() {
        return false;
    }
    json_value_string_array_runtime(packet.get("target_paths"))
        .iter()
        .any(|reference| review_packet_target_path_matches_agent(reference, agent_id))
        || json_value_string_array_runtime(packet.get("blind_context"))
            .iter()
            .any(|context| context.trim() == format!("agent_id: {agent_id}"))
        || json_string_field_runtime(packet, "objective")
            .as_deref()
            .is_some_and(|objective| review_packet_objective_targets_agent(objective, agent_id))
}

pub(crate) fn review_packet_target_path_matches_agent(reference: &str, agent_id: &str) -> bool {
    let needle = format!("/{agent_id}/");
    reference.contains(&format!("/agents{needle}")) || reference.contains(&needle)
}

pub(crate) fn review_packet_objective_targets_agent(objective: &str, agent_id: &str) -> bool {
    let normalized = objective
        .replace('`', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    normalized.contains(&format!("from {agent_id}"))
        || normalized.contains(&format!("agent_id: {agent_id}"))
}

impl AutonomousResearchAcceptedWorkerEvidenceEntry {
    fn stage_execution_id_for_index(
        &self,
        record: &MainAgentWorkerArtifactDecisionRecord,
        job: &AutonomousResearchJobState,
    ) -> String {
        record
            .stage_execution_id
            .clone()
            .or_else(|| job.stage_execution_id.clone())
            .unwrap_or_else(|| "stage_unknown".to_string())
    }
}

pub(crate) fn empty_accepted_worker_evidence_index_for_stage(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    stage_execution_id: &str,
) -> AutonomousResearchAcceptedWorkerEvidenceIndex {
    AutonomousResearchAcceptedWorkerEvidenceIndex {
        schema_version: "autonomous_research.accepted_worker_evidence_index.v1".to_string(),
        job_id: job.job_id.clone(),
        project_id: resolved.project_id.clone(),
        stage_execution_id: stage_execution_id.to_string(),
        stage_id: contract.stage_id.clone(),
        generated_at: timestamp_string(),
        entries: Vec::new(),
    }
}

pub(crate) fn bind_existing_independent_semantic_reviews_in_accepted_worker_evidence_index(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<Option<String>, String> {
    let Some(stage_execution_id) = job.stage_execution_id.as_deref() else {
        return Ok(None);
    };
    let path =
        autonomous_research_accepted_worker_evidence_index_path(resolved, job, stage_execution_id);
    if !path.exists() {
        return Ok(None);
    }
    let Some(mut index) = load_autonomous_research_accepted_worker_evidence_index_at_path(&path)
    else {
        return Ok(None);
    };
    if index.stage_id != contract.stage_id {
        return Ok(None);
    }
    if !bind_independent_semantic_reviews_to_target_worker_evidence(resolved, &mut index) {
        return Ok(None);
    }
    index.generated_at = timestamp_string();
    write_autonomous_research_accepted_worker_evidence_index(&path, &index)?;
    Ok(Some(relative_workspace_ref(
        &resolved.workspace_root,
        &path,
    )))
}

pub(crate) fn load_main_agent_worker_artifact_decision_records(
    resolved: &ResolvedProject,
) -> Result<Vec<MainAgentWorkerArtifactDecisionRecord>, String> {
    let dir = resolved
        .data_dir
        .join("main-agent-board")
        .join("worker-artifact-decisions");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<MainAgentWorkerArtifactDecisionRecord>(&content)
        else {
            continue;
        };
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.decision_ref.cmp(&right.decision_ref))
    });
    Ok(records)
}

pub(crate) fn accepted_worker_evidence_entry_from_main_agent_decision(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentWorkerArtifactDecisionRecord,
) -> Result<Option<AutonomousResearchAcceptedWorkerEvidenceEntry>, String> {
    let Some(task_packet_path) =
        find_main_agent_decision_agent_file(resolved, record, "task_packet.json")
    else {
        return Ok(None);
    };
    let task_packet = read_json_file_runtime(&task_packet_path)?;
    let stage_task = task_packet
        .get("stage_task_contract")
        .cloned()
        .unwrap_or(Value::Null);
    let stage_id = json_string_field_runtime(&stage_task, "stage_id")
        .or_else(|| record.stage_id.clone())
        .unwrap_or_else(|| contract.stage_id.clone());
    if stage_id != contract.stage_id {
        return Ok(None);
    }
    let stage_execution_id = json_string_field_runtime(&stage_task, "stage_execution_id")
        .or_else(|| record.stage_execution_id.clone())
        .or_else(|| job.stage_execution_id.clone());
    if job.stage_execution_id.as_deref().is_some()
        && stage_execution_id.as_deref() != job.stage_execution_id.as_deref()
    {
        return Ok(None);
    }
    let task_id =
        json_string_field_runtime(&stage_task, "task_id").unwrap_or_else(|| record.task_id.clone());
    if task_id != record.task_id {
        return Ok(None);
    }
    let task_type = json_string_field_runtime(&stage_task, "task_type")
        .unwrap_or_else(|| task_type_from_task_id_runtime(&record.task_id));
    let worker_role = json_string_field_runtime(&stage_task, "worker_role").unwrap_or_else(|| {
        autonomous_research_stage_worker_role(&contract.stage_id, &task_type).to_string()
    });
    let required_output_artifact_type =
        json_string_field_runtime(&stage_task, "required_output_artifact_type").unwrap_or_else(
            || {
                autonomous_research_stage_task_required_output_artifact_type(
                    &contract.stage_id,
                    &task_type,
                    &contract.artifact_type,
                )
                .to_string()
            },
        );
    let output_manifest_path =
        find_main_agent_decision_agent_file(resolved, record, "output_manifest.json");
    let output_manifest_ref = output_manifest_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| {
            resolved
                .data_dir
                .join("agents")
                .join(&record.agent_id)
                .join("output_manifest.json")
                .display()
                .to_string()
        });
    let output_manifest = output_manifest_path
        .as_ref()
        .and_then(|path| read_json_file_runtime(path).ok());
    let output_refs = output_manifest
        .as_ref()
        .map(output_manifest_refs_runtime)
        .unwrap_or_default();
    let mut evidence_refs =
        normalize_review_artifact_refs_runtime(resolved, record.candidate_refs.clone());
    merge_unique_strings(
        &mut evidence_refs,
        normalize_review_artifact_refs_runtime(resolved, record.readiness_refs.clone()),
    );
    merge_unique_strings(
        &mut evidence_refs,
        normalize_review_artifact_refs_runtime(resolved, record.canonical_target_refs.clone()),
    );
    merge_unique_strings(&mut evidence_refs, output_refs);
    merge_unique_strings(
        &mut evidence_refs,
        vec![
            task_packet_path.display().to_string(),
            output_manifest_ref.clone(),
            record.decision_ref.clone(),
        ],
    );
    if let Some(adoption_scope) = record.adoption_scope.as_ref() {
        merge_unique_strings(
            &mut evidence_refs,
            vec![format!("adoption_scope:{adoption_scope}")],
        );
    }
    let evidence_text = collect_main_agent_decision_evidence_text(
        resolved,
        record,
        &task_packet,
        output_manifest.as_ref(),
    );
    let searchable = normalize_match_text_runtime(&evidence_text);
    let required_fields = json_value_string_array_runtime(stage_task.get("required_output_fields"));
    let acceptance_checks = json_value_string_array_runtime(stage_task.get("acceptance_checks"));
    let matched_required_fields = matched_labels_runtime(&searchable, &required_fields);
    let matched_acceptance_checks =
        matched_acceptance_checks_runtime(&searchable, &acceptance_checks);
    let matched_quality_signals =
        matched_stage_quality_signals_runtime(&searchable, &contract.stage_id, &task_type);
    let quality_profile = main_agent_decision_quality_profile(
        &required_output_artifact_type,
        &matched_required_fields,
        &required_fields,
        &matched_acceptance_checks,
        &acceptance_checks,
        &matched_quality_signals,
        &evidence_refs,
        &record.rationale,
    );
    let stage_execution_id = json_string_field_runtime(&stage_task, "stage_execution_id")
        .unwrap_or_else(|| contract.stage_id.clone());
    let current_evidence_set_id = format!("{stage_execution_id}::{task_id}");
    let mut entry = AutonomousResearchAcceptedWorkerEvidenceEntry {
        agent_id: record.agent_id.clone(),
        task_id,
        task_type: task_type.clone(),
        worker_role,
        required_output_artifact_type,
        output_manifest_ref,
        task_packet_ref: task_packet_path.display().to_string(),
        evidence_refs,
        matched_required_fields,
        matched_acceptance_checks,
        matched_quality_signals,
        quality_profile,
        semantic_review: None,
        main_agent_acceptance: Some(main_agent_decision_acceptance_projection_result(
            record, &task_type,
        )),
        acceptance_authority: Some("main_agent_worker_artifact_decision".to_string()),
        main_agent_decision_ref: Some(record.decision_ref.clone()),
        review_required: Some(record.review_required),
        active_status: Some(main_agent_worker_artifact_decision_active_status(record).to_string()),
        current_evidence_set_id: Some(current_evidence_set_id),
        superseded_by_task_id: None,
        replacement_of_task_ids: json_value_string_array_runtime(
            stage_task.get("replacement_of_task_ids"),
        ),
        decision_reason: Some(record.rationale.clone()),
        created_at: Some(record.created_at.clone()),
    };
    apply_accepted_worker_artifact_projection(
        resolved,
        &job.job_id,
        &stage_execution_id,
        &mut entry,
    )?;
    Ok(Some(entry))
}

pub(crate) fn find_main_agent_decision_agent_file(
    resolved: &ResolvedProject,
    record: &MainAgentWorkerArtifactDecisionRecord,
    file_name: &str,
) -> Option<PathBuf> {
    let preferred = resolved
        .data_dir
        .join("agents")
        .join(&record.agent_id)
        .join(file_name);
    if preferred.exists() {
        return Some(preferred);
    }
    record
        .candidate_refs
        .iter()
        .map(|reference| resolve_adoption_reference_path(resolved, reference))
        .find(|path| {
            path.file_name().and_then(|value| value.to_str()) == Some(file_name) && path.exists()
        })
}

pub(crate) fn read_json_file_runtime(path: &Path) -> Result<Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

pub(crate) fn json_string_field_runtime(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
}

pub(crate) fn output_manifest_refs_runtime(value: &Value) -> Vec<String> {
    let mut refs = Vec::new();
    if let Some(items) = value.get("output_refs").and_then(|value| value.as_array()) {
        for item in items {
            if let Some(reference) = item.get("ref").and_then(|value| value.as_str()) {
                merge_unique_strings(&mut refs, vec![reference.to_string()]);
            }
        }
    }
    refs
}

pub(crate) fn collect_main_agent_decision_evidence_text(
    resolved: &ResolvedProject,
    record: &MainAgentWorkerArtifactDecisionRecord,
    task_packet: &Value,
    output_manifest: Option<&Value>,
) -> String {
    let mut parts = vec![record.rationale.clone(), task_packet.to_string()];
    if let Some(output_manifest) = output_manifest {
        parts.push(output_manifest.to_string());
        for reference in output_manifest_refs_runtime(output_manifest) {
            let path = resolve_adoption_reference_path(resolved, &reference);
            if path.exists() && path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    parts.push(content);
                }
            }
        }
    }
    let decision_refs = record
        .candidate_refs
        .iter()
        .chain(record.readiness_refs.iter())
        .cloned()
        .collect::<Vec<_>>();
    for reference in normalize_review_artifact_refs_runtime(resolved, decision_refs) {
        let path = resolve_adoption_reference_path(resolved, &reference);
        if path.exists() && path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                parts.push(content);
            }
        }
        if path.file_name().and_then(|value| value.to_str())
            == Some("worktree_artifact_candidates.json")
        {
            if let Ok(candidate_refs) = output_manifest_worktree_candidate_refs(&path) {
                for manifest_ref in candidate_refs {
                    let manifest_path = PathBuf::from(manifest_ref);
                    if let Ok(content) = std::fs::read_to_string(manifest_path) {
                        parts.push(content);
                    }
                }
            }
        }
    }
    parts.join("\n\n")
}

pub(crate) fn normalize_match_text_runtime(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn label_matches_runtime(searchable: &str, label: &str) -> bool {
    searchable.contains(&normalize_match_text_runtime(label))
}

pub(crate) fn matched_labels_runtime(searchable: &str, labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .filter(|label| label_matches_runtime(searchable, label))
        .cloned()
        .collect()
}

pub(crate) fn meaningful_match_tokens_runtime(value: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "have", "in", "is",
        "it", "of", "on", "or", "that", "the", "to", "with",
    ];
    normalize_match_text_runtime(value)
        .split_whitespace()
        .filter(|token| token.len() >= 4 && !STOPWORDS.contains(token))
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn acceptance_check_matches_runtime(searchable: &str, check: &str) -> bool {
    if label_matches_runtime(searchable, check) {
        return true;
    }
    let tokens = meaningful_match_tokens_runtime(check);
    if tokens.is_empty() {
        return true;
    }
    let matched = tokens
        .iter()
        .filter(|token| searchable.contains(token.as_str()))
        .count();
    matched >= tokens.len().min(2)
}

pub(crate) fn matched_acceptance_checks_runtime(
    searchable: &str,
    checks: &[String],
) -> Vec<String> {
    checks
        .iter()
        .filter(|check| acceptance_check_matches_runtime(searchable, check))
        .cloned()
        .collect()
}

pub(crate) fn stage_quality_signals_for_task_runtime(
    stage_id: &str,
    task_type: &str,
) -> Vec<String> {
    if task_type == "acceptance standard setting" {
        return vec![
            "expert acceptance target",
            "stage-specific pass criteria",
            "review-to-task routing",
            "CandidateStageEvidencePlan",
            "evidence_requirements",
            "record_stage_evidence_plan",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
    }
    match stage_id {
        "literature" => vec![
            "source entries|source_entries",
            "closest-family coverage note",
            "missing-source risks|missing source risk|missing-source risk",
            "citation ledger|source retrieval manifest|literature retrieval manifest",
            "provider_tool_completed|tool_actions|worker tool audit",
        ],
        "novelty" => vec![
            "closest prior|closest-prior",
            "overlap analysis",
            "novelty risks|novelty-risk",
        ],
        "refine" => vec![
            "method sketch|method proposal",
            "feasibility",
            "risk analysis",
        ],
        "experiment-plan" => vec!["baselines", "metrics", "ablations", "run matrix"],
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub(crate) fn quality_signal_matches_runtime(searchable: &str, signal: &str) -> bool {
    signal
        .split('|')
        .any(|option| label_matches_runtime(searchable, option.trim()))
        || acceptance_check_matches_runtime(searchable, signal)
}

pub(crate) fn matched_stage_quality_signals_runtime(
    searchable: &str,
    stage_id: &str,
    task_type: &str,
) -> Vec<String> {
    stage_quality_signals_for_task_runtime(stage_id, task_type)
        .into_iter()
        .filter(|signal| quality_signal_matches_runtime(searchable, signal))
        .collect()
}

pub(crate) fn main_agent_decision_quality_profile(
    required_output_artifact_type: &str,
    matched_required_fields: &[String],
    required_fields: &[String],
    matched_acceptance_checks: &[String],
    acceptance_checks: &[String],
    matched_quality_signals: &[String],
    evidence_refs: &[String],
    rationale: &str,
) -> GoalStageTaskQualityProfile {
    let mut strengths = Vec::new();
    let mut risks = Vec::new();
    if !required_output_artifact_type.trim().is_empty() {
        strengths.push(format!(
            "required output artifact type `{required_output_artifact_type}` is present in task contract"
        ));
    }
    if required_fields.is_empty() || matched_required_fields.len() >= required_fields.len().min(3) {
        strengths.push("main-agent accepted artifact with required-field evidence".to_string());
    } else {
        risks.push(format!(
            "main-agent accepted artifact but some required fields were not detected in projection: {}",
            required_fields
                .iter()
                .filter(|field| !matched_required_fields.iter().any(|matched| matched == *field))
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if acceptance_checks.is_empty()
        || matched_acceptance_checks.len() >= acceptance_checks.len().min(2)
    {
        strengths.push("main-agent accepted artifact with acceptance-check evidence".to_string());
    } else {
        risks.push("not all acceptance checks were detected in projected evidence".to_string());
    }
    if !matched_quality_signals.is_empty() {
        strengths.push("projected evidence includes stage-specific quality signals".to_string());
    } else {
        risks.push(
            "stage-specific quality signals were not detected in projected evidence".to_string(),
        );
    }
    if !evidence_refs.is_empty() {
        strengths.push(format!(
            "{} auditable evidence refs recorded",
            evidence_refs.len()
        ));
    } else {
        risks.push("missing auditable evidence refs".to_string());
    }
    if rationale.trim().is_empty() {
        risks.push("main-agent acceptance rationale is empty".to_string());
    }
    let level = if risks
        .iter()
        .any(|risk| risk.contains("missing auditable") || risk.contains("rationale is empty"))
    {
        "insufficient"
    } else {
        "partial"
    };
    GoalStageTaskQualityProfile {
        schema_version: "goal_stage_task_quality_profile.v1".to_string(),
        score: if level == "partial" { 85 } else { 45 },
        level: level.to_string(),
        strengths,
        risks,
    }
}

pub(crate) fn main_agent_decision_acceptance_projection_result(
    record: &MainAgentWorkerArtifactDecisionRecord,
    task_type: &str,
) -> GoalStageTaskSemanticReviewResult {
    let (verdict, score, suggested_operation, action_phrase, failure_class) =
        match record.decision.as_str() {
            "reject" => (
                "fail",
                Some(0),
                "reject",
                "rejected",
                Some("main_agent_rejected_worker_artifact".to_string()),
            ),
            "defer" => (
                "blocked",
                Some(0),
                "defer",
                "deferred",
                Some("main_agent_deferred_worker_artifact".to_string()),
            ),
            _ => ("pass", Some(80), "accept", "accepted", None),
        };
    GoalStageTaskSemanticReviewResult {
        schema_version: "goal_stage_task_semantic_review_result.v1".to_string(),
        verdict: verdict.to_string(),
        score,
        reviewer_role: "main_agent_worker_artifact_acceptance".to_string(),
        review_model: "astra-main-agent".to_string(),
        execution_mode: "main_agent_authority_projection".to_string(),
        review_packet_ref: record.decision_ref.clone(),
        review_trace_ref: record.decision_ref.clone(),
        findings: vec![
            format!(
                "Main agent explicitly {action_phrase} `{task_type}` worker artifact evidence through record_worker_artifact_decision."
            ),
            compact_single_line(&record.rationale, 500),
        ],
        failure_class,
        suggested_operation: Some(suggested_operation.to_string()),
        rollback_target: None,
        cleanup_required: record.cleanup_required,
        provider_id: None,
        model: None,
    }
}

pub(crate) fn main_agent_worker_artifact_decision_active_status(
    record: &MainAgentWorkerArtifactDecisionRecord,
) -> &'static str {
    match record.decision.as_str() {
        "accept" if record.review_required => "accepted_pending_review",
        "accept" => "active",
        "reject" => "rejected",
        "defer" => "deferred",
        _ => "candidate",
    }
}

pub(crate) fn task_type_from_task_id_runtime(task_id: &str) -> String {
    task_id
        .rsplit("::")
        .next()
        .unwrap_or(task_id)
        .replace('_', " ")
}

pub(crate) fn empty_autonomous_research_accepted_worker_evidence_index(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    stage_task_acceptance: &crate::goals::GoalStageTaskAcceptanceCheckResult,
) -> AutonomousResearchAcceptedWorkerEvidenceIndex {
    AutonomousResearchAcceptedWorkerEvidenceIndex {
        schema_version: "autonomous_research.accepted_worker_evidence_index.v1".to_string(),
        job_id: job.job_id.clone(),
        project_id: resolved.project_id.clone(),
        stage_execution_id: stage_task_acceptance.stage_execution_id.clone(),
        stage_id: stage_task_acceptance.stage_id.clone(),
        generated_at: timestamp_string(),
        entries: Vec::new(),
    }
}

pub(crate) fn load_autonomous_research_accepted_worker_evidence_index(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Option<AutonomousResearchAcceptedWorkerEvidenceIndex> {
    let active_stage_execution_id = job.stage_execution_id.as_deref()?;
    let (active_stage_id, stage_execution_ids) =
        autonomous_research_accepted_worker_evidence_stage_execution_ids(resolved, job);
    let mut merged: Option<AutonomousResearchAcceptedWorkerEvidenceIndex> = None;
    for stage_execution_id in stage_execution_ids {
        let path = autonomous_research_accepted_worker_evidence_index_path(
            resolved,
            job,
            &stage_execution_id,
        );
        let Some(mut index) = std::fs::read_to_string(path).ok().and_then(|content| {
            serde_json::from_str::<AutonomousResearchAcceptedWorkerEvidenceIndex>(&content).ok()
        }) else {
            continue;
        };
        bind_independent_semantic_reviews_to_target_worker_evidence(resolved, &mut index);
        if active_stage_id
            .as_ref()
            .is_some_and(|stage_id| index.stage_id != *stage_id)
        {
            continue;
        }
        let target = merged.get_or_insert_with(|| AutonomousResearchAcceptedWorkerEvidenceIndex {
            schema_version: "autonomous_research.accepted_worker_evidence_index.v1".to_string(),
            job_id: job.job_id.clone(),
            project_id: resolved.project_id.clone(),
            stage_execution_id: active_stage_execution_id.to_string(),
            stage_id: active_stage_id
                .clone()
                .unwrap_or_else(|| index.stage_id.clone()),
            generated_at: index.generated_at.clone(),
            entries: Vec::new(),
        });
        for entry in index.entries {
            upsert_autonomous_research_accepted_worker_evidence_entry(&mut target.entries, entry);
        }
        target.generated_at = timestamp_string();
    }
    merged
}

pub(crate) fn autonomous_research_accepted_worker_evidence_stage_execution_ids(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> (Option<String>, Vec<String>) {
    let mut stage_execution_ids = Vec::new();
    let mut active_stage_id = None;
    let active_stage_execution_id = job.stage_execution_id.clone();

    if let Some(active_stage_execution_id) = active_stage_execution_id.as_ref() {
        if let Ok(report) = crate::research::status(&resolved.data_dir, &resolved.project_id) {
            if let Some(stage) = report.active_stage_execution {
                if stage.execution_id == *active_stage_execution_id {
                    active_stage_id = Some(stage.stage_id.clone());
                    if let Some(parent_execution_id) = stage.parent_execution_id {
                        merge_unique_strings(&mut stage_execution_ids, vec![parent_execution_id]);
                    }
                    merge_unique_strings(&mut stage_execution_ids, vec![stage.root_execution_id]);
                    merge_unique_strings(&mut stage_execution_ids, stage.supersedes);
                }
            }
        }
        for obligation in &job.obligations {
            if active_stage_id
                .as_ref()
                .map(|stage_id| obligation.stage_id == *stage_id)
                .unwrap_or(true)
            {
                if let Some(stage_execution_id) = obligation.stage_execution_id.as_ref() {
                    merge_unique_strings(
                        &mut stage_execution_ids,
                        vec![stage_execution_id.clone()],
                    );
                }
                active_stage_id.get_or_insert_with(|| obligation.stage_id.clone());
            }
        }
        stage_execution_ids
            .retain(|stage_execution_id| stage_execution_id != active_stage_execution_id);
        stage_execution_ids.push(active_stage_execution_id.clone());
    }

    (active_stage_id, stage_execution_ids)
}

pub(crate) fn upsert_autonomous_research_accepted_worker_evidence_entry(
    entries: &mut Vec<AutonomousResearchAcceptedWorkerEvidenceEntry>,
    entry: AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    let mut entry = entry;
    normalize_autonomous_research_accepted_worker_evidence_entry(&mut entry);
    let replacement_task_ids = entry.replacement_of_task_ids.clone();
    let replacement_task_id = entry.task_id.clone();
    if let Some(existing) = entries
        .iter_mut()
        .find(|existing| accepted_worker_evidence_entries_match(existing, &entry))
    {
        let same_task_identity =
            existing.agent_id == entry.agent_id || existing.task_id == entry.task_id;
        let mut merged = entry;
        let preserve_rejection =
            accepted_worker_evidence_rejection_should_survive_upsert(existing, &merged);
        if same_task_identity {
            merge_autonomous_research_accepted_worker_evidence_entry(existing, &mut merged);
        }
        if preserve_rejection {
            merged.active_status = Some("rejected".to_string());
            merged.decision_reason = existing.decision_reason.clone();
            merged.created_at = existing.created_at.clone().or(merged.created_at);
        }
        *existing = merged;
        mark_replaced_accepted_worker_evidence_entries(
            entries,
            &replacement_task_ids,
            &replacement_task_id,
        );
        true
    } else {
        entries.push(entry);
        mark_replaced_accepted_worker_evidence_entries(
            entries,
            &replacement_task_ids,
            &replacement_task_id,
        );
        true
    }
}

pub(crate) fn compact_autonomous_research_accepted_worker_evidence_entries(
    entries: &mut Vec<AutonomousResearchAcceptedWorkerEvidenceEntry>,
) -> bool {
    let original_len = entries.len();
    let mut compacted = Vec::new();
    let mut changed = false;
    for mut entry in std::mem::take(entries) {
        changed |= normalize_autonomous_research_accepted_worker_evidence_entry(&mut entry);
        upsert_autonomous_research_accepted_worker_evidence_entry(&mut compacted, entry);
    }
    if compacted.len() != original_len {
        changed = true;
    }
    *entries = compacted;
    changed
}

pub(crate) fn normalize_autonomous_research_accepted_worker_evidence_entry(
    entry: &mut AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    let original_len = entry.replacement_of_task_ids.len();
    entry
        .replacement_of_task_ids
        .retain(|task_id| task_id != &entry.task_id);
    entry.replacement_of_task_ids.len() != original_len
}

pub(crate) fn mark_replaced_accepted_worker_evidence_entries(
    entries: &mut [AutonomousResearchAcceptedWorkerEvidenceEntry],
    replacement_of_task_ids: &[String],
    superseded_by_task_id: &str,
) {
    if replacement_of_task_ids.is_empty() {
        return;
    }
    for entry in entries.iter_mut() {
        if entry.task_id == superseded_by_task_id {
            continue;
        }
        if replacement_of_task_ids
            .iter()
            .any(|task_id| task_id == &entry.task_id)
        {
            entry.active_status = Some("superseded".to_string());
            entry.superseded_by_task_id = Some(superseded_by_task_id.to_string());
        }
    }
}

pub(crate) fn select_main_agent_accepted_evidence_revision(
    entries: &mut [AutonomousResearchAcceptedWorkerEvidenceEntry],
    selected: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) {
    if !accepted_worker_evidence_entry_is_main_agent_accepted(selected) {
        return;
    }
    let selected_set_id = selected
        .current_evidence_set_id
        .as_deref()
        .unwrap_or(&selected.task_id);
    let Some(selected_decision_ref) = selected.main_agent_decision_ref.as_deref() else {
        return;
    };
    for entry in entries.iter_mut() {
        let entry_set_id = entry
            .current_evidence_set_id
            .as_deref()
            .unwrap_or(&entry.task_id);
        if entry_set_id != selected_set_id
            || entry.main_agent_decision_ref.as_deref() == Some(selected_decision_ref)
            || !accepted_worker_evidence_entry_is_main_agent_accepted(entry)
        {
            continue;
        }
        entry.active_status = Some("superseded".to_string());
        entry.decision_reason = Some(format!(
            "superseded_by_main_agent_evidence_selection:{selected_decision_ref}"
        ));
    }
}

pub(crate) fn accepted_worker_evidence_entries_match(
    existing: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    incoming: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    let same_candidate = existing.agent_id == incoming.agent_id
        && existing.task_id == incoming.task_id
        && existing.output_manifest_ref == incoming.output_manifest_ref
        && existing.task_packet_ref == incoming.task_packet_ref;
    if same_candidate {
        return true;
    }
    if existing.main_agent_decision_ref.is_some()
        && incoming.main_agent_decision_ref.is_some()
        && existing.main_agent_decision_ref != incoming.main_agent_decision_ref
    {
        return false;
    }
    if incoming
        .replacement_of_task_ids
        .iter()
        .any(|task_id| task_id != &incoming.task_id && task_id == &existing.task_id)
        || existing
            .replacement_of_task_ids
            .iter()
            .any(|task_id| task_id != &existing.task_id && task_id == &incoming.task_id)
    {
        return false;
    }
    existing.agent_id == incoming.agent_id && existing.task_id == incoming.task_id
}

pub(crate) fn accepted_worker_evidence_rejection_should_survive_upsert(
    existing: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    incoming: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    if !accepted_worker_evidence_entry_is_rejected(existing) {
        return false;
    }
    if existing.agent_id != incoming.agent_id || existing.task_id != incoming.task_id {
        return false;
    }
    if existing.current_evidence_set_id.is_some()
        && existing.current_evidence_set_id == incoming.current_evidence_set_id
    {
        return true;
    }
    let existing_refs = accepted_worker_evidence_entry_refs(existing)
        .into_iter()
        .map(|reference| relative_display_ref(&reference))
        .collect::<BTreeSet<_>>();
    accepted_worker_evidence_entry_refs(incoming)
        .into_iter()
        .map(|reference| relative_display_ref(&reference))
        .any(|reference| existing_refs.contains(&reference))
}

pub(crate) fn merge_autonomous_research_accepted_worker_evidence_entry(
    existing: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    incoming: &mut AutonomousResearchAcceptedWorkerEvidenceEntry,
) {
    let existing_has_main_agent_decision =
        accepted_worker_evidence_entry_has_main_agent_decision(existing);
    let incoming_has_main_agent_decision =
        accepted_worker_evidence_entry_has_main_agent_decision(incoming);
    let same_main_agent_decision = incoming_has_main_agent_decision
        && existing_has_main_agent_decision
        && existing.main_agent_decision_ref == incoming.main_agent_decision_ref;
    if incoming_has_main_agent_decision
        && (!existing_has_main_agent_decision || same_main_agent_decision)
    {
        incoming.quality_profile = existing.quality_profile.clone();
        incoming.matched_required_fields = existing.matched_required_fields.clone();
        incoming.matched_acceptance_checks = existing.matched_acceptance_checks.clone();
        incoming.matched_quality_signals = existing.matched_quality_signals.clone();
    }
    incoming.created_at = earliest_optional_timestamp_string(
        existing.created_at.as_deref(),
        incoming.created_at.as_deref(),
    );
    if incoming.semantic_review.is_none() {
        incoming.semantic_review = existing.semantic_review.clone();
    }
    if incoming.main_agent_acceptance.is_none() {
        incoming.main_agent_acceptance = existing.main_agent_acceptance.clone();
    }
    if incoming.review_required.is_none() {
        incoming.review_required = existing.review_required;
    }
    if incoming.main_agent_decision_ref.is_none() {
        incoming.main_agent_decision_ref = existing.main_agent_decision_ref.clone();
    }
    if incoming.active_status.is_none() {
        incoming.active_status = existing.active_status.clone();
    }
    if incoming.acceptance_authority.is_none() {
        incoming.acceptance_authority = existing.acceptance_authority.clone();
    }
    if existing_has_main_agent_decision && !incoming_has_main_agent_decision {
        incoming.acceptance_authority = existing.acceptance_authority.clone();
        incoming.main_agent_decision_ref = existing.main_agent_decision_ref.clone();
        incoming.active_status = existing.active_status.clone();
        incoming.review_required = existing.review_required;
        incoming.decision_reason = existing.decision_reason.clone();
    }
    if incoming.current_evidence_set_id.is_none() {
        incoming.current_evidence_set_id = existing.current_evidence_set_id.clone();
    }
    if incoming.superseded_by_task_id.is_none() {
        incoming.superseded_by_task_id = existing.superseded_by_task_id.clone();
    }
    if incoming.decision_reason.is_none() {
        incoming.decision_reason = existing.decision_reason.clone();
    }
    merge_unique_strings(
        &mut incoming.replacement_of_task_ids,
        existing.replacement_of_task_ids.clone(),
    );
    let existing_evidence_refs =
        if accepted_worker_evidence_entry_has_workspace_projection(incoming) {
            existing
                .evidence_refs
                .iter()
                .filter(|reference| !Path::new(reference).is_absolute())
                .cloned()
                .collect()
        } else {
            existing.evidence_refs.clone()
        };
    merge_unique_strings(&mut incoming.evidence_refs, existing_evidence_refs);
    merge_unique_strings(
        &mut incoming.matched_required_fields,
        existing.matched_required_fields.clone(),
    );
    merge_unique_strings(
        &mut incoming.matched_acceptance_checks,
        existing.matched_acceptance_checks.clone(),
    );
    merge_unique_strings(
        &mut incoming.matched_quality_signals,
        existing.matched_quality_signals.clone(),
    );
}

fn accepted_worker_evidence_entry_has_workspace_projection(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    entry.evidence_refs.iter().any(|reference| {
        reference.contains("/accepted_worker_evidence/materialized/")
            && reference.ends_with("/projection.json")
    })
}

pub(crate) fn accepted_worker_evidence_entry_has_main_agent_decision(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    entry
        .acceptance_authority
        .as_deref()
        .is_some_and(|authority| authority == "main_agent_worker_artifact_decision")
        && entry
            .main_agent_decision_ref
            .as_deref()
            .is_some_and(|decision_ref| !decision_ref.trim().is_empty())
}

pub(crate) fn earliest_optional_timestamp_string(
    left: Option<&str>,
    right: Option<&str>,
) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => {
            if timestamp_string_is_after(left, right) {
                Some(right.to_string())
            } else {
                Some(left.to_string())
            }
        }
        (Some(value), None) | (None, Some(value)) => Some(value.to_string()),
        (None, None) => None,
    }
}

#[derive(Debug, Clone)]
pub(in crate::runtime) struct AcceptedWorkerSemanticReviewBinding {
    source_task_id: String,
    target_task_ids: Vec<String>,
    review: GoalStageTaskSemanticReviewResult,
    evidence_refs: Vec<String>,
}

pub(crate) fn bind_independent_semantic_reviews_to_target_worker_evidence(
    resolved: &ResolvedProject,
    index: &mut AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> bool {
    let mut changed = scrub_misattributed_independent_semantic_reviews(resolved, index);
    let bindings = index
        .entries
        .iter()
        .filter_map(|entry| {
            accepted_worker_semantic_review_binding_from_entry(resolved, &index.stage_id, entry)
        })
        .collect::<Vec<_>>();
    for binding in bindings {
        for entry in &mut index.entries {
            if entry.task_id == binding.source_task_id {
                continue;
            }
            if !binding
                .target_task_ids
                .iter()
                .any(|target_task_id| target_task_id == &entry.task_id)
            {
                continue;
            }
            if !accepted_worker_semantic_review_should_replace(
                entry.semantic_review.as_ref(),
                &binding.review,
            ) {
                continue;
            }
            entry.semantic_review = Some(binding.review.clone());
            merge_unique_strings(
                &mut entry.evidence_refs,
                vec![format!(
                    "semantic_review_source_task:{}",
                    binding.source_task_id
                )],
            );
            merge_unique_strings(&mut entry.evidence_refs, binding.evidence_refs.clone());
            changed = true;
        }
    }
    for entry in &mut index.entries {
        if entry.active_status.as_deref() == Some("accepted_pending_review")
            && accepted_worker_semantic_review_passes_for_stage(&index.stage_id, entry)
        {
            entry.active_status = Some("active".to_string());
            changed = true;
        }
    }
    changed
}

fn accepted_worker_semantic_review_passes_for_stage(
    stage_id: &str,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    if entry.review_required == Some(false) {
        return true;
    }
    let Some(review) = entry.semantic_review.as_ref() else {
        return false;
    };
    review.verdict == "pass"
        && review.execution_mode != "main_agent_authority_projection"
        && review.reviewer_role != "main_agent_worker_artifact_acceptance"
        && review
            .score
            .map(|score| {
                score >= accepted_worker_semantic_review_required_score_for_stage(stage_id)
            })
            .unwrap_or(false)
}

pub(crate) fn scrub_misattributed_independent_semantic_reviews(
    resolved: &ResolvedProject,
    index: &mut AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> bool {
    let mut changed = false;
    for entry in &mut index.entries {
        let Some(review) = entry.semantic_review.as_ref() else {
            continue;
        };
        if review.execution_mode != "independent_semantic_review" {
            continue;
        }
        if semantic_review_packet_explicitly_targets_entry_agent(resolved, entry, review) {
            continue;
        }
        let review_refs = [
            review.review_packet_ref.clone(),
            review.review_trace_ref.clone(),
        ];
        entry.semantic_review = None;
        retain_evidence_refs_excluding(&mut entry.evidence_refs, &review_refs);
        changed = true;
    }
    changed
}

pub(crate) fn semantic_review_packet_explicitly_targets_entry_agent(
    resolved: &ResolvedProject,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    review: &GoalStageTaskSemanticReviewResult,
) -> bool {
    let packet_path = resolve_adoption_reference_path(resolved, &review.review_packet_ref);
    let Ok(packet) = read_json_file_runtime(&packet_path) else {
        return true;
    };
    review_packet_explicitly_targets_agent(&packet, &entry.agent_id)
}

pub(crate) fn retain_evidence_refs_excluding(
    references: &mut Vec<String>,
    excluded_refs: &[String],
) {
    let excluded = excluded_refs
        .iter()
        .filter(|reference| !reference.trim().is_empty())
        .map(|reference| relative_display_ref(reference))
        .collect::<BTreeSet<_>>();
    if excluded.is_empty() {
        return;
    }
    references.retain(|reference| !excluded.contains(&relative_display_ref(reference)));
}

pub(in crate::runtime) fn accepted_worker_semantic_review_binding_from_entry(
    resolved: &ResolvedProject,
    stage_id: &str,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Option<AcceptedWorkerSemanticReviewBinding> {
    if !accepted_worker_evidence_entry_is_semantic_review_report(entry) {
        return None;
    }
    let source_review = entry.semantic_review.as_ref()?;
    if !independent_semantic_review_report_quality_passes(stage_id, source_review) {
        return None;
    }
    let target_task_ids =
        accepted_worker_semantic_review_target_task_ids_from_packet(resolved, entry);
    if target_task_ids.is_empty() {
        return None;
    }
    let review_text = accepted_worker_semantic_review_report_text(resolved, entry);
    let target_review =
        accepted_worker_semantic_review_result_from_report(entry, source_review, &review_text)?;
    Some(AcceptedWorkerSemanticReviewBinding {
        source_task_id: entry.task_id.clone(),
        target_task_ids,
        review: target_review,
        evidence_refs: accepted_worker_semantic_review_binding_refs(entry),
    })
}

pub(crate) fn accepted_worker_evidence_entry_is_semantic_review_report(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    normalize_match_text_runtime(&entry.required_output_artifact_type)
        .contains("semantic review report")
}

pub(crate) fn independent_semantic_review_report_quality_passes(
    stage_id: &str,
    review: &GoalStageTaskSemanticReviewResult,
) -> bool {
    review.verdict == "pass"
        && review.execution_mode != "main_agent_authority_projection"
        && review.reviewer_role != "main_agent_worker_artifact_acceptance"
        && review
            .score
            .map(|score| {
                score >= accepted_worker_semantic_review_required_score_for_stage(stage_id)
            })
            .unwrap_or(false)
}

pub(crate) fn accepted_worker_semantic_review_target_task_ids_from_packet(
    resolved: &ResolvedProject,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut target_task_ids = Vec::new();
    for packet_ref in accepted_worker_semantic_review_task_packet_refs(entry) {
        let path = resolve_adoption_reference_path(resolved, &packet_ref);
        let Ok(value) = read_json_file_runtime(&path) else {
            continue;
        };
        let Some(stage_task) = value.get("stage_task_contract") else {
            continue;
        };
        for target_id in json_value_string_array_runtime(stage_task.get("review_target_task_ids")) {
            if let Some(task_id) = normalize_accepted_worker_review_target_task_id(&target_id) {
                merge_unique_strings(&mut target_task_ids, vec![task_id]);
            }
        }
        for reference in
            json_value_string_array_runtime(stage_task.get("review_target_evidence_refs"))
        {
            if let Some(task_id) =
                accepted_worker_review_target_task_id_from_evidence_ref(&reference)
            {
                merge_unique_strings(&mut target_task_ids, vec![task_id]);
            }
        }
    }
    target_task_ids
}

pub(crate) fn accepted_worker_semantic_review_task_packet_refs(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut refs = Vec::new();
    if !entry.task_packet_ref.trim().is_empty() {
        refs.push(entry.task_packet_ref.clone());
    }
    for reference in &entry.evidence_refs {
        if reference.ends_with("task_packet.json") {
            merge_unique_strings(&mut refs, vec![reference.clone()]);
        }
    }
    refs
}

pub(crate) fn accepted_worker_review_target_task_id_from_evidence_ref(
    reference: &str,
) -> Option<String> {
    let trimmed = reference.trim().trim_matches('`');
    trimmed
        .strip_prefix("accepted_worker_evidence_task:")
        .and_then(normalize_accepted_worker_review_target_task_id)
}

pub(crate) fn normalize_accepted_worker_review_target_task_id(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_matches('`')
        .trim_start_matches("main_agent_board_task::")
        .trim_start_matches("accepted_worker_evidence_task:")
        .trim();
    if trimmed.is_empty() || trimmed.contains('/') {
        return None;
    }
    Some(trimmed.to_string())
}

pub(crate) fn accepted_worker_semantic_review_report_text(
    resolved: &ResolvedProject,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> String {
    let mut parts = Vec::new();
    for reference in accepted_worker_semantic_review_report_refs(entry) {
        let path = resolve_adoption_reference_path(resolved, &reference);
        if path.exists() && path.is_file() {
            if let Ok(content) = std::fs::read_to_string(path) {
                parts.push(content);
            }
        }
    }
    parts.join("\n\n")
}

pub(crate) fn accepted_worker_semantic_review_report_refs(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut refs = Vec::new();
    for reference in entry
        .evidence_refs
        .iter()
        .chain(std::iter::once(&entry.output_manifest_ref))
    {
        let lower = reference.to_ascii_lowercase();
        if lower.ends_with("provider_worker_evidence.md")
            || lower.ends_with("semantic_review_report.md")
        {
            merge_unique_strings(&mut refs, vec![reference.clone()]);
        }
    }
    refs
}

pub(crate) fn accepted_worker_semantic_review_result_from_report(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    source_review: &GoalStageTaskSemanticReviewResult,
    review_text: &str,
) -> Option<GoalStageTaskSemanticReviewResult> {
    if review_text.trim().is_empty() {
        return None;
    }
    let verdict_text = extract_review_labeled_value(review_text, "semantic_floor_verdict")
        .or_else(|| extract_review_labeled_value(review_text, "verdict"))?;
    let verdict = parse_semantic_review_report_verdict(&verdict_text)?;
    let score_text = extract_review_labeled_value(review_text, "score")?;
    let score = parse_semantic_review_report_score(&score_text)?;
    let report_ref = accepted_worker_semantic_review_report_refs(entry)
        .into_iter()
        .next()
        .unwrap_or_else(|| entry.output_manifest_ref.clone());
    Some(GoalStageTaskSemanticReviewResult {
        schema_version: "goal_stage_task_semantic_review_result.v1".to_string(),
        verdict,
        score: Some(score),
        reviewer_role: entry.worker_role.clone(),
        review_model: source_review.review_model.clone(),
        execution_mode: "accepted_independent_semantic_review_task".to_string(),
        review_packet_ref: report_ref,
        review_trace_ref: source_review.review_trace_ref.clone(),
        findings: accepted_worker_semantic_review_report_findings(review_text),
        failure_class: extract_review_failure_class(review_text).filter(|value| value != "none"),
        suggested_operation: extract_review_labeled_value(review_text, "unblock_decision")
            .or_else(|| extract_review_labeled_value(review_text, "suggested operation")),
        rollback_target: extract_review_labeled_value(review_text, "rollback target")
            .filter(|value| value != "none"),
        cleanup_required: source_review.cleanup_required
            || extract_review_labeled_value(review_text, "cleanup requirement")
                .map(|value| value.contains("required") && !value.contains("not required"))
                .unwrap_or(false),
        provider_id: source_review.provider_id.clone(),
        model: source_review.model.clone(),
    })
}

pub(crate) fn parse_semantic_review_report_verdict(value: &str) -> Option<String> {
    let lowered = value
        .trim()
        .trim_matches('`')
        .trim_matches('*')
        .to_ascii_lowercase();
    let normalized = lowered.replace(['-', ' '], "_");
    if normalized.contains("unreviewable")
        || normalized.contains("missing_evidence")
        || normalized.contains("missing_input")
        || normalized.contains("cannot_review")
    {
        Some("unreviewable_missing_evidence".to_string())
    } else if normalized.contains("replacement_needed")
        || normalized.contains("replace_needed")
        || normalized.contains("needs_replacement")
        || normalized.contains("must_replace")
    {
        Some("replacement_needed".to_string())
    } else if normalized.contains("repair_needed")
        || normalized.contains("needs_repair")
        || normalized.contains("revision_needed")
        || normalized.contains("revise")
    {
        Some("repair_needed".to_string())
    } else if lowered.contains("pass") && !lowered.contains("fail") {
        Some("pass".to_string())
    } else if lowered.contains("fail") || lowered.contains("block") {
        Some("fail".to_string())
    } else {
        None
    }
}

pub(crate) fn parse_semantic_review_report_score(value: &str) -> Option<u64> {
    let mut digits = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse::<u64>().ok()
}

pub(crate) fn accepted_worker_semantic_review_report_findings(review_text: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for line in review_text.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = item.trim();
            if !item.is_empty() {
                findings.push(item.to_string());
            }
        } else if let Some((_, item)) = trimmed.split_once(". ") {
            if trimmed
                .chars()
                .next()
                .map(|ch| ch.is_ascii_digit())
                .unwrap_or(false)
                && !item.trim().is_empty()
            {
                findings.push(item.trim().to_string());
            }
        }
        if findings.len() >= 12 {
            break;
        }
    }
    if findings.is_empty() {
        findings.push("independent semantic review report was accepted and bound by explicit review_target fields".to_string());
    }
    findings
}

pub(crate) fn accepted_worker_semantic_review_binding_refs(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut refs = Vec::new();
    merge_unique_strings(
        &mut refs,
        vec![
            entry.task_packet_ref.clone(),
            entry.output_manifest_ref.clone(),
        ],
    );
    merge_unique_strings(
        &mut refs,
        accepted_worker_semantic_review_report_refs(entry),
    );
    if let Some(review) = entry.semantic_review.as_ref() {
        merge_unique_strings(
            &mut refs,
            vec![
                review.review_packet_ref.clone(),
                review.review_trace_ref.clone(),
            ],
        );
    }
    refs
}

pub(crate) fn accepted_worker_semantic_review_should_replace(
    existing: Option<&GoalStageTaskSemanticReviewResult>,
    incoming: &GoalStageTaskSemanticReviewResult,
) -> bool {
    let Some(existing) = existing else {
        return true;
    };
    let existing_pass = existing.verdict == "pass";
    let incoming_pass = incoming.verdict == "pass";
    if existing_pass && !incoming_pass {
        return false;
    }
    if incoming_pass && !existing_pass {
        return true;
    }
    incoming.score.unwrap_or(0) >= existing.score.unwrap_or(0)
}

pub(crate) fn render_autonomous_research_accepted_worker_evidence_index(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> String {
    let Some(index) = index else {
        return "No accepted stage-local worker evidence has been recorded for the active stage yet."
            .to_string();
    };
    if index.entries.is_empty() {
        return "Accepted worker evidence index exists but has no entries.".to_string();
    }
    let current_gate_entries = accepted_worker_evidence_current_gate_entries(index);
    if current_gate_entries.is_empty() {
        let pending_count = index
            .entries
            .iter()
            .filter(|entry| !accepted_worker_evidence_entry_is_retired(entry))
            .filter(|entry| !accepted_worker_evidence_entry_has_main_agent_decision(entry))
            .count();
        return format!(
            "No main-agent-accepted stage-local worker evidence is available for the active stage yet. {pending_count} reviewed candidate entries may exist in the pending worker-artifact decision queue, but they do not satisfy stage coverage until the main agent calls `record_worker_artifact_decision`."
        );
    }
    current_gate_entries
        .into_iter()
        .map(|entry| {
            format!(
                "- task_id: `{}`\n  agent_id: `{}`\n  task_type: `{}`\n  worker_role: `{}`\n  acceptance_authority: `{}`\n  active_status: `{}`\n  current_evidence_set_id: `{}`\n  replacement_of_task_ids: {}\n  superseded_by_task_id: `{}`\n  main_agent_decision_ref: `{}`\n  required_output_artifact_type: `{}`\n  output_manifest_ref: `{}`\n  evidence_refs: {}\n  matched_required_fields: {}\n  matched_acceptance_checks: {}\n  matched_quality_signals: {}\n  quality_profile: level=`{}` score=`{}` strengths={} risks={}\n  main_agent_acceptance: {}\n  semantic_review: {}",
                entry.task_id,
                entry.agent_id,
                entry.task_type,
                entry.worker_role,
                entry.acceptance_authority.as_deref().unwrap_or("unknown"),
                entry.active_status.as_deref().unwrap_or("candidate"),
                entry.current_evidence_set_id.as_deref().unwrap_or("none"),
                comma_or_none_runtime(&entry.replacement_of_task_ids),
                entry.superseded_by_task_id.as_deref().unwrap_or("none"),
                entry.main_agent_decision_ref.as_deref().unwrap_or("none"),
                entry.required_output_artifact_type,
                relative_display_ref(&entry.output_manifest_ref),
                comma_or_none_runtime(&entry.evidence_refs),
                comma_or_none_runtime(&entry.matched_required_fields),
                comma_or_none_runtime(&entry.matched_acceptance_checks),
                comma_or_none_runtime(&entry.matched_quality_signals),
                entry.quality_profile.level,
                entry.quality_profile.score,
                comma_or_none_runtime(&entry.quality_profile.strengths),
                comma_or_none_runtime(&entry.quality_profile.risks),
                render_accepted_worker_semantic_review(entry.main_agent_acceptance.as_ref()),
                render_accepted_worker_semantic_review(entry.semantic_review.as_ref())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn autonomous_research_review_gate_accepted_worker_evidence_context(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let Some(index) = index else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let index_ref = relative_workspace_ref(
        &resolved.workspace_root,
        &autonomous_research_accepted_worker_evidence_index_path(
            resolved,
            job,
            &index.stage_execution_id,
        ),
    );
    let mut target_paths = Vec::new();
    if resolved.workspace_root.join(&index_ref).exists() {
        target_paths.push(index_ref.clone());
    }
    let current_entries = accepted_worker_evidence_current_gate_entries(index);
    if current_entries.is_empty() {
        return (target_paths, Vec::new(), Vec::new());
    }

    let mut task_types = current_entries
        .iter()
        .map(|entry| entry.task_type.clone())
        .collect::<Vec<_>>();
    task_types.sort();
    task_types.dedup();
    let mut task_ids = current_entries
        .iter()
        .map(|entry| entry.task_id.clone())
        .collect::<Vec<_>>();
    task_ids.sort();
    task_ids.dedup();
    let mut authority_rows = current_entries
        .iter()
        .map(|entry| {
            let semantic_review = entry
                .semantic_review
                .as_ref()
                .map(|review| {
                    format!(
                        "{}/{}",
                        review.verdict,
                        review
                            .score
                            .map(|score| score.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    )
                })
                .unwrap_or_else(|| "missing".to_string());
            let main_agent_acceptance = entry
                .main_agent_acceptance
                .as_ref()
                .map(|review| {
                    format!(
                        "{}/{}",
                        review.verdict,
                        review
                            .score
                            .map(|score| score.to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    )
                })
                .unwrap_or_else(|| "missing".to_string());
            format!(
                "task_type=`{}` authority=`{}` semantic_review=`{}` main_agent_acceptance=`{}` active_status=`{}` task_id=`{}` main_agent_decision_ref=`{}`",
                entry.task_type,
                entry.acceptance_authority.as_deref().unwrap_or("unknown"),
                semantic_review,
                main_agent_acceptance,
                entry.active_status.as_deref().unwrap_or("candidate"),
                entry.task_id,
                entry.main_agent_decision_ref.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>();
    authority_rows.sort();

    let blind_context = vec![
        format!("accepted_worker_evidence_index:{index_ref}"),
        format!(
            "accepted_worker_evidence_stage_execution_id:{}",
            index.stage_execution_id
        ),
        format!("accepted_worker_evidence_stage_id:{}", index.stage_id),
        format!(
            "accepted_worker_evidence_active_task_types:{}",
            render_prompt_list(&task_types)
        ),
        format!(
            "accepted_worker_evidence_active_task_ids:{}",
            render_prompt_list(&task_ids)
        ),
        format!(
            "accepted_worker_evidence_acceptance_authorities:{}",
            render_prompt_list(&authority_rows)
        ),
    ];
    let evidence_required = vec![format!(
        "accepted worker evidence index at {index_ref} establishes current stage-local worker evidence coverage, semantic-review status, and acceptance authority"
    )];
    (target_paths, blind_context, evidence_required)
}

pub(crate) fn autonomous_research_adoption_ready_candidates(
    resolved: &ResolvedProject,
    contract: &AutonomousResearchStageContract,
    job: &AutonomousResearchJobState,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<AutonomousResearchAdoptionReadyCandidate> {
    let Some(index) = index else {
        return Vec::new();
    };
    let latest_review_created_at = job
        .last_review
        .as_ref()
        .filter(|review| review.verdict != "pass")
        .map(|review| review.created_at.as_str());
    let already_selected_records = load_main_agent_stage_artifact_adoption_records(resolved)
        .unwrap_or_default()
        .into_iter()
        .filter(|record| {
            record.stage_id == contract.stage_id
                && record.target_artifact_path == contract.artifact_path
                && Some(record.stage_execution_id.as_str()) == job.stage_execution_id.as_deref()
        })
        .collect::<Vec<_>>();
    let mut candidates = index
        .entries
        .iter()
        .filter(|entry| !accepted_worker_evidence_entry_is_retired(entry))
        .filter(|entry| {
            !already_selected_records.iter().any(|record| {
                main_agent_stage_artifact_adoption_record_targets_entry(entry, record)
            })
        })
        .filter(|entry| {
            latest_review_created_at
                .map(|review_created_at| {
                    entry
                        .created_at
                        .as_deref()
                        .map(|entry_created_at| {
                            timestamp_string_is_after(entry_created_at, review_created_at)
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        })
        .filter(|entry| entry.required_output_artifact_type == contract.artifact_type)
        .filter(|entry| {
            accepted_worker_evidence_entry_is_adoptable_stage_synthesis(contract, entry)
        })
        .filter(|entry| matches!(entry.quality_profile.level.as_str(), "strong" | "partial"))
        .filter(|entry| accepted_worker_semantic_review_passes(contract, entry))
        .filter(|entry| {
            !accepted_worker_active_tool_evidence_required(entry)
                || accepted_worker_active_tool_evidence_passes(entry)
        })
        .filter_map(|entry| {
            let candidate_refs = adoption_candidate_refs_for_prompt(contract, entry);
            if candidate_refs.is_empty() {
                return None;
            }
            let recommended_source_ref =
                recommended_adoption_source_ref_for_prompt(&candidate_refs);
            let recommended_source_artifact_path =
                recommended_adoption_source_artifact_path_for_prompt(
                    resolved,
                    contract,
                    entry,
                    &recommended_source_ref,
                );
            let post_latest_failed_review = latest_review_created_at
                .zip(entry.created_at.as_deref())
                .map(|(review_created_at, entry_created_at)| {
                    timestamp_string_is_after(entry_created_at, review_created_at)
                })
                .unwrap_or(false);
            Some(AutonomousResearchAdoptionReadyCandidate {
                agent_id: entry.agent_id.clone(),
                task_id: entry.task_id.clone(),
                task_type: entry.task_type.clone(),
                worker_role: entry.worker_role.clone(),
                required_output_artifact_type: entry.required_output_artifact_type.clone(),
                quality_level: entry.quality_profile.level.clone(),
                quality_score: entry.quality_profile.score,
                semantic_review_verdict: entry
                    .semantic_review
                    .as_ref()
                    .map(|review| review.verdict.clone()),
                semantic_review_score: entry
                    .semantic_review
                    .as_ref()
                    .and_then(|review| review.score),
                semantic_review_ref: entry
                    .semantic_review
                    .as_ref()
                    .map(|review| review.review_packet_ref.clone()),
                main_agent_decision_ref: entry.main_agent_decision_ref.clone(),
                post_latest_failed_review,
                candidate_refs,
                recommended_source_ref,
                recommended_source_artifact_path,
                target_artifact_path: contract.artifact_path.clone(),
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        let left_ref = left
            .semantic_review_ref
            .clone()
            .unwrap_or_else(|| left.task_id.clone());
        let right_ref = right
            .semantic_review_ref
            .clone()
            .unwrap_or_else(|| right.task_id.clone());
        right_ref.cmp(&left_ref)
    });
    candidates.truncate(8);
    candidates
}

pub(crate) fn render_autonomous_research_adoption_ready_candidates_for_prompt(
    resolved: &ResolvedProject,
    contract: &AutonomousResearchStageContract,
    job: &AutonomousResearchJobState,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> String {
    let candidates = autonomous_research_adoption_ready_candidates(resolved, contract, job, index);
    if candidates.is_empty() {
        return "- none".to_string();
    }
    let Some(index) = index else {
        return "- none".to_string();
    };
    let entries = index
        .entries
        .iter()
        .filter(|entry| !accepted_worker_evidence_entry_is_retired(entry))
        .collect::<Vec<_>>();
    let lines = candidates
        .iter()
        .filter_map(|candidate| {
            let entry = entries
                .iter()
                .find(|entry| entry.agent_id == candidate.agent_id && entry.task_id == candidate.task_id)?;
            let semantic_review = render_accepted_worker_semantic_review(entry.semantic_review.as_ref());
            let main_agent_decision = candidate
                .main_agent_decision_ref
                .as_deref()
                .unwrap_or("missing");
            Some(format!(
                "- agent_id: `{}`\n  task_id: `{}`\n  task_type: `{}`\n  worker_role: `{}`\n  semantic_review: {}\n  quality_profile: level=`{}` score=`{}`\n  post_latest_failed_review: `{}`\n  main_agent_decision_ref: `{}`\n  candidate_refs: {}\n  recommended_adopt_stage_artifact_args: stage_id=`{}` stage_execution_id=`{}` source_agent_id=`{}` source_task_id=`{}` source_ref=`{}` source_artifact_path=`{}` target_artifact_path=`{}` request_review_rerun=`true`\n  adoption_validation_note: raw candidate body must already be a clean canonical stage artifact body before provenance is appended; runtime rejects candidate wrapper, retention/preflight/adoption handoff wording, diagnostics, and unsafe provenance, while stage-required research sections remain main-agent/reviewer semantic judgment. Use the source_agent_id, source_ref, and source_artifact_path from the same listed candidate; do not mix a source_ref from another agent.\n  required_main_agent_action: inspect the candidate, then call `record_worker_artifact_decision` and `adopt_stage_artifact`, or explicitly reject/defer it before publishing another repair for the same blocker.",
                candidate.agent_id,
                candidate.task_id,
                candidate.task_type,
                candidate.worker_role,
                semantic_review,
                candidate.quality_level,
                candidate.quality_score,
                candidate.post_latest_failed_review,
                main_agent_decision,
                comma_or_none_runtime(&candidate.candidate_refs),
                contract.stage_id,
                job.stage_execution_id.as_deref().unwrap_or("unknown"),
                candidate.agent_id,
                candidate.task_id,
                relative_display_ref(&candidate.recommended_source_ref),
                candidate
                    .recommended_source_artifact_path
                    .as_deref()
                    .unwrap_or("none"),
                candidate.target_artifact_path
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "- none".to_string();
    }
    format!(
        "This is a factual queue only; runtime is not choosing adoption. Main agent owns the accept/reject/defer/adopt decision.\n{}",
        lines.join("\n")
    )
}

pub(crate) fn render_autonomous_research_adoption_ready_candidate_summary(
    candidate: &AutonomousResearchAdoptionReadyCandidate,
) -> String {
    format!(
        "- agent_id=`{}` task_id=`{}` task_type=`{}` worker_role=`{}` quality=`{}` score={} semantic=`{}`/{} decision_ref=`{}` post_latest_failed_review={} source_ref=`{}` source_artifact_path=`{}` target=`{}` refs={}",
        candidate.agent_id,
        candidate.task_id,
        candidate.task_type,
        candidate.worker_role,
        candidate.quality_level,
        candidate.quality_score,
        candidate
            .semantic_review_verdict
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        candidate
            .semantic_review_score
            .map(|score| score.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        candidate
            .main_agent_decision_ref
            .clone()
            .unwrap_or_else(|| "missing".to_string()),
        candidate.post_latest_failed_review,
        relative_display_ref(&candidate.recommended_source_ref),
        candidate
            .recommended_source_artifact_path
            .as_deref()
            .unwrap_or("none"),
        candidate.target_artifact_path,
        comma_or_none_runtime(&candidate.candidate_refs)
    )
}

pub(crate) fn autonomous_research_pending_worker_artifact_decisions(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Vec<AutonomousResearchPendingWorkerArtifactDecision> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return Vec::new();
    };
    let mut candidates = index
        .entries
        .iter()
        .filter(|entry| !accepted_worker_evidence_entry_is_retired(entry))
        .filter(|entry| !accepted_worker_evidence_entry_has_main_agent_decision(entry))
        .filter(|entry| matches!(entry.quality_profile.level.as_str(), "strong" | "partial"))
        .filter(|entry| accepted_worker_semantic_review_passes(&contract, entry))
        .filter(|entry| {
            !accepted_worker_active_tool_evidence_required(entry)
                || accepted_worker_active_tool_evidence_passes(entry)
        })
        .filter_map(|entry| {
            let candidate_refs = pending_worker_artifact_decision_refs(&contract, entry);
            if candidate_refs.is_empty() {
                return None;
            }
            Some(AutonomousResearchPendingWorkerArtifactDecision {
                agent_id: entry.agent_id.clone(),
                task_id: entry.task_id.clone(),
                task_type: entry.task_type.clone(),
                worker_role: entry.worker_role.clone(),
                required_output_artifact_type: entry.required_output_artifact_type.clone(),
                quality_level: entry.quality_profile.level.clone(),
                quality_score: entry.quality_profile.score,
                semantic_review_verdict: entry
                    .semantic_review
                    .as_ref()
                    .map(|review| review.verdict.clone()),
                semantic_review_score: entry
                    .semantic_review
                    .as_ref()
                    .and_then(|review| review.score),
                semantic_review_ref: entry
                    .semantic_review
                    .as_ref()
                    .map(|review| review.review_packet_ref.clone()),
                candidate_refs,
                required_main_agent_action: pending_worker_artifact_required_action(
                    &contract, entry, &index,
                ),
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.task_type
            .cmp(&right.task_type)
            .then_with(|| left.task_id.cmp(&right.task_id))
            .then_with(|| left.agent_id.cmp(&right.agent_id))
    });
    candidates.truncate(12);
    candidates
}

pub(crate) fn autonomous_research_has_pending_worker_artifact_decisions(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> bool {
    !autonomous_research_pending_worker_artifact_decisions(resolved, job).is_empty()
}

pub(crate) fn pending_worker_artifact_decision_refs(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut refs = if accepted_worker_evidence_entry_is_stage_synthesis(contract, entry) {
        adoption_candidate_refs_for_prompt(contract, entry)
    } else {
        accepted_worker_evidence_entry_refs(entry)
            .into_iter()
            .filter(|reference| {
                let file_name = Path::new(reference)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                matches!(
                    file_name,
                    "provider_worker_evidence.md"
                        | "worktree_artifact_candidates.json"
                        | "stdout.txt"
                        | "output_manifest.json"
                ) || reference.ends_with(".md")
                    || reference.ends_with(".json")
            })
            .collect::<Vec<_>>()
    };
    refs.sort();
    refs.dedup();
    refs
}

pub(crate) fn pending_worker_artifact_required_action(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> String {
    if accepted_worker_evidence_entry_is_stage_synthesis(contract, entry) {
        return "inspect the synthesis candidate; call record_worker_artifact_decision with accept/reject/defer, and if accepted call adopt_stage_artifact for the canonical stage artifact".to_string();
    }
    if autonomous_research_task_type_is_standard_setting(&entry.task_type)
        || entry.worker_role.contains("standard_setter")
        || entry.required_output_artifact_type == "stage_acceptance_rubric"
    {
        return "inspect this standard-setting candidate; call record_worker_artifact_decision with accept/reject/defer. If accepted or raised, call record_stage_evidence_plan with concrete JSON evidence_requirements copied or tightened from CandidateStageEvidencePlan. Do not call adopt_stage_artifact for stage_acceptance_rubric.md".to_string();
    }
    let mut accepted_if_main_agent_accepts =
        accepted_worker_evidence_current_gate_task_types(Some(index));
    merge_unique_strings(
        &mut accepted_if_main_agent_accepts,
        vec![entry.task_type.clone()],
    );
    let missing_after_accept = contract
        .worker_task_types
        .iter()
        .filter(|task_type| {
            !accepted_if_main_agent_accepts
                .iter()
                .any(|accepted| accepted == *task_type)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing_after_accept.is_empty() {
        return format!(
            "inspect this local evidence; call record_worker_artifact_decision with accept/reject/defer. If accepted, continue or repair missing task types before synthesis: {}",
            render_prompt_list(&missing_after_accept)
        );
    }
    "inspect this local evidence; call record_worker_artifact_decision with accept/reject/defer. If accepted, publish or update a stage artifact synthesis task for worker_role=`research_synthesizer` that consumes the active accepted evidence refs; do not adopt local evidence directly".to_string()
}

pub(crate) fn render_autonomous_research_pending_worker_artifact_decisions_for_prompt(
    decisions: &[AutonomousResearchPendingWorkerArtifactDecision],
) -> String {
    if decisions.is_empty() {
        return "- none".to_string();
    }
    let lines = decisions
        .iter()
        .map(|candidate| {
            format!(
                "- agent_id=`{}` task_id=`{}` task_type=`{}` worker_role=`{}` required_output=`{}` quality=`{}` score={} semantic=`{}`/{} semantic_ref=`{}` candidate_refs={} required_action={}",
                candidate.agent_id,
                candidate.task_id,
                candidate.task_type,
                candidate.worker_role,
                candidate.required_output_artifact_type,
                candidate.quality_level,
                candidate.quality_score,
                candidate
                    .semantic_review_verdict
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                candidate
                    .semantic_review_score
                    .map(|score| score.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                candidate
                    .semantic_review_ref
                    .as_deref()
                    .map(relative_display_ref)
                    .unwrap_or_else(|| "none".to_string()),
                comma_or_none_runtime(&candidate.candidate_refs),
                candidate.required_main_agent_action
            )
        })
        .collect::<Vec<_>>();
    format!(
        "These are reviewed worker candidates that are not active evidence until the main agent makes an explicit semantic decision. Runtime is only projecting the queue.\n{}",
        lines.join("\n")
    )
}

pub(crate) fn render_autonomous_research_failed_review_repair_for_prompt(
    repair: &AutonomousResearchFailedReviewRepairLedger,
) -> String {
    format!(
        "- review_id=`{}` score=`{}` failure_class=`{}` suggested_operation=`{}` cleanup_requirement=`{}` review_summary=`{}` evidence_gap_worker_repair_required={} review_rerun_blocked={}\n  post_review_state_transition_refs: {}\n  required_repairs_excerpt: {}",
        repair.review_id,
        repair
            .score
            .map(|score| score.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        repair
            .failure_class
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        repair
            .suggested_operation
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        repair
            .cleanup_requirement
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        repair
            .review_summary_path
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        autonomous_research_failed_review_repair_requires_evidence_gap_worker_repair(repair),
        repair.review_rerun_blocked,
        render_prompt_list(&repair.post_review_state_transition_refs),
        compact_single_line(&repair.required_repairs_excerpt, 2_400)
    )
}

pub(crate) fn adoption_ready_candidate_supersedes_review_failure(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    review: &crate::reviews::ReviewInspectionResult,
) -> bool {
    let contract = autonomous_research_stage_contract(resolved, job);
    let Some(index) = load_autonomous_research_accepted_worker_evidence_index(resolved, job) else {
        return false;
    };
    if accepted_worker_evidence_current_gate_entries(&index)
        .into_iter()
        .any(|entry| worker_review_targets_accepted_worker_evidence(review, entry))
    {
        return false;
    }
    autonomous_research_adoption_ready_candidates(resolved, &contract, job, Some(&index))
        .into_iter()
        .any(|candidate| {
            candidate.post_latest_failed_review
                || index.entries.iter().any(|entry| {
                    entry.agent_id == candidate.agent_id
                        && entry.task_id == candidate.task_id
                        && entry
                            .created_at
                            .as_deref()
                            .map(|created_at| {
                                timestamp_string_is_after(
                                    created_at,
                                    &review.latest_trace.timestamp,
                                ) || created_at == review.latest_trace.timestamp
                            })
                            .unwrap_or(false)
                })
        })
}

pub(crate) fn adoption_candidate_refs_for_prompt(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let artifact_type = contract.artifact_type.to_ascii_lowercase();
    let target_file_name = Path::new(&contract.artifact_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let target_stem = Path::new(&contract.artifact_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut refs = Vec::new();
    for reference in accepted_worker_evidence_entry_refs(entry) {
        let lower = reference.to_ascii_lowercase();
        let file_name = Path::new(&reference)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if file_name == "worktree_artifact_candidates.json" {
            merge_unique_strings(&mut refs, vec![reference.clone()]);
            continue;
        }
        let looks_like_candidate_artifact = file_name.contains(&artifact_type)
            || (!target_stem.is_empty() && file_name.contains(&target_stem))
            || (!target_file_name.is_empty() && file_name == target_file_name)
            || (file_name.contains("candidate") && lower.ends_with(".md"));
        let runtime_wrapper = file_name == "provider_worker_evidence.md"
            || file_name == "stdout.txt"
            || file_name == "stderr.txt"
            || file_name == "task_packet.json"
            || file_name == "output_manifest.json"
            || file_name == "trace.json";
        if looks_like_candidate_artifact && !runtime_wrapper {
            merge_unique_strings(&mut refs, vec![reference]);
        }
    }
    refs
}

pub(crate) fn recommended_adoption_source_ref_for_prompt(candidate_refs: &[String]) -> String {
    candidate_refs
        .iter()
        .find(|reference| {
            Path::new(reference)
                .file_name()
                .and_then(|value| value.to_str())
                .map(|file_name| file_name != "worktree_artifact_candidates.json")
                .unwrap_or(false)
        })
        .or_else(|| candidate_refs.first())
        .cloned()
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn recommended_adoption_source_artifact_path_for_prompt(
    resolved: &ResolvedProject,
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    recommended_source_ref: &str,
) -> Option<String> {
    if adoption_reference_can_name_candidate_file(recommended_source_ref) {
        return Path::new(recommended_source_ref)
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToString::to_string);
    }
    let path = resolve_adoption_reference_path(resolved, recommended_source_ref);
    if path.file_name().and_then(|value| value.to_str()) == Some("output_manifest.json") {
        for manifest_ref in output_manifest_worktree_candidate_refs(&path).unwrap_or_default() {
            if let Some(candidate) = recommended_candidate_path_from_worktree_manifest(
                contract,
                entry,
                &PathBuf::from(manifest_ref),
            ) {
                return Some(candidate);
            }
        }
    }
    if path.file_name().and_then(|value| value.to_str())
        == Some("worktree_artifact_candidates.json")
    {
        return recommended_candidate_path_from_worktree_manifest(contract, entry, &path);
    }
    None
}

pub(crate) fn recommended_candidate_path_from_worktree_manifest(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    manifest_path: &Path,
) -> Option<String> {
    let mut candidates = available_adoption_candidate_paths_from_manifest(entry, manifest_path);
    candidates.sort_by_key(|path| {
        let file_name = Path::new(path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let target_name = Path::new(&contract.artifact_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let target_stem = Path::new(&contract.artifact_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let artifact_type = contract.artifact_type.to_ascii_lowercase();
        (
            file_name != target_name,
            !file_name.contains(&artifact_type),
            target_stem.is_empty() || !file_name.contains(&target_stem),
            !file_name.contains("candidate"),
            path.clone(),
        )
    });
    candidates.into_iter().next()
}

pub(crate) fn render_accepted_worker_semantic_review(
    review: Option<&GoalStageTaskSemanticReviewResult>,
) -> String {
    let Some(review) = review else {
        return "missing".to_string();
    };
    format!(
        "verdict=`{}` score=`{}` reviewer_role=`{}` execution_mode=`{}` review_packet_ref=`{}` findings={} failure_class=`{}` suggested_operation=`{}`",
        review.verdict,
        review
            .score
            .map(|score| score.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        review.reviewer_role,
        review.execution_mode,
        relative_display_ref(&review.review_packet_ref),
        comma_or_none_runtime(&review.findings),
        review.failure_class.as_deref().unwrap_or("none"),
        review.suggested_operation.as_deref().unwrap_or("none")
    )
}

pub(crate) fn accepted_worker_evidence_required_for_stage(
    contract: &AutonomousResearchStageContract,
    _index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    !contract.worker_task_types.is_empty()
}

pub(crate) fn accepted_worker_evidence_entry_is_retired(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    let status = entry
        .active_status
        .as_deref()
        .unwrap_or("candidate")
        .to_ascii_lowercase();
    entry.superseded_by_task_id.is_some()
        || matches!(
            status.as_str(),
            "superseded" | "rejected" | "deferred" | "obsolete" | "inactive"
        )
}

pub(crate) fn accepted_worker_evidence_entry_is_rejected(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    entry
        .active_status
        .as_deref()
        .unwrap_or("candidate")
        .eq_ignore_ascii_case("rejected")
        || entry
            .decision_reason
            .as_deref()
            .map(|reason| reason.starts_with("stage_artifact_adoption_rejected:"))
            .unwrap_or(false)
}

pub(crate) fn accepted_worker_evidence_entry_is_active(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    !accepted_worker_evidence_entry_is_retired(entry)
        && entry
            .active_status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("active"))
}

pub(crate) fn accepted_worker_evidence_current_gate_entries(
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Vec<&AutonomousResearchAcceptedWorkerEvidenceEntry> {
    let mut active_by_evidence_set =
        BTreeMap::<&str, Vec<&AutonomousResearchAcceptedWorkerEvidenceEntry>>::new();
    for entry in index
        .entries
        .iter()
        .filter(|entry| accepted_worker_evidence_entry_counts_for_stage_gate(entry))
    {
        let evidence_set_id = entry
            .current_evidence_set_id
            .as_deref()
            .unwrap_or(&entry.task_id);
        active_by_evidence_set
            .entry(evidence_set_id)
            .or_default()
            .push(entry);
    }
    active_by_evidence_set
        .into_values()
        .filter_map(|entries| (entries.len() == 1).then(|| entries[0]))
        .collect()
}

pub(crate) fn accepted_worker_evidence_ambiguity_failures(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    let mut active_by_evidence_set = BTreeMap::<&str, Vec<&str>>::new();
    for entry in accepted_worker_evidence_active_entries(index) {
        let evidence_set_id = entry
            .current_evidence_set_id
            .as_deref()
            .unwrap_or(&entry.task_id);
        active_by_evidence_set
            .entry(evidence_set_id)
            .or_default()
            .push(
                entry
                    .main_agent_decision_ref
                    .as_deref()
                    .unwrap_or("missing_main_agent_decision_ref"),
            );
    }
    active_by_evidence_set
        .into_iter()
        .filter_map(|(evidence_set_id, revision_refs)| {
            (revision_refs.len() > 1).then(|| {
                format!(
                    "evidence set `{evidence_set_id}` has multiple active main-agent-accepted revisions: {}. Record an explicit worker artifact acceptance to select one revision before synthesis, adoption, or review.",
                    revision_refs.join(", ")
                )
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn accepted_worker_evidence_snapshot(
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Result<(Vec<String>, String), String> {
    accepted_worker_evidence_snapshot_from_entries(
        index,
        accepted_worker_evidence_active_entries(index),
    )
}

pub(crate) fn accepted_worker_evidence_snapshot_for_contract(
    contract: &AutonomousResearchStageContract,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Result<(Vec<String>, String), String> {
    accepted_worker_evidence_snapshot_from_entries(
        index,
        accepted_worker_evidence_active_entries(index)
            .into_iter()
            .filter(|entry| accepted_worker_evidence_entry_matches_contract(contract, entry))
            .collect(),
    )
}

fn accepted_worker_evidence_snapshot_from_entries(
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    entries: Vec<&AutonomousResearchAcceptedWorkerEvidenceEntry>,
) -> Result<(Vec<String>, String), String> {
    let mut active_by_evidence_set =
        BTreeMap::<&str, Vec<&AutonomousResearchAcceptedWorkerEvidenceEntry>>::new();
    for entry in entries {
        let evidence_set_id = entry
            .current_evidence_set_id
            .as_deref()
            .unwrap_or(&entry.task_id);
        active_by_evidence_set
            .entry(evidence_set_id)
            .or_default()
            .push(entry);
    }
    let ambiguous_sets = active_by_evidence_set
        .iter()
        .filter_map(|(evidence_set_id, entries)| (entries.len() > 1).then_some(*evidence_set_id))
        .collect::<Vec<_>>();
    if !ambiguous_sets.is_empty() {
        return Err(format!(
            "accepted evidence snapshot is ambiguous for evidence sets: {}",
            ambiguous_sets.join(", ")
        ));
    }

    let mut revisions = active_by_evidence_set
        .into_values()
        .filter_map(|entries| entries.into_iter().next())
        .into_iter()
        .map(|entry| {
            let mut evidence_refs = entry.evidence_refs.clone();
            evidence_refs.sort();
            serde_json::json!({
                "evidence_set_id": entry.current_evidence_set_id.as_deref().unwrap_or(&entry.task_id),
                "task_id": entry.task_id,
                "main_agent_decision_ref": entry.main_agent_decision_ref,
                "output_manifest_ref": entry.output_manifest_ref,
                "evidence_refs": evidence_refs,
            })
        })
        .collect::<Vec<_>>();
    revisions.sort_by(|left, right| left.to_string().cmp(&right.to_string()));
    let revision_refs = revisions
        .iter()
        .filter_map(|revision| {
            revision
                .get("main_agent_decision_ref")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    let snapshot = serde_json::json!({
        "stage_execution_id": index.stage_execution_id,
        "stage_id": index.stage_id,
        "revisions": revisions,
    });
    let bytes = serde_json::to_vec(&snapshot).map_err(|err| err.to_string())?;
    Ok((revision_refs, sha256_hex(&bytes)))
}

fn accepted_worker_evidence_active_entries(
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Vec<&AutonomousResearchAcceptedWorkerEvidenceEntry> {
    index
        .entries
        .iter()
        .filter(|entry| accepted_worker_evidence_entry_counts_for_stage_gate(entry))
        .collect()
}

pub(crate) fn accepted_worker_evidence_entries_for_contract<'a>(
    contract: &AutonomousResearchStageContract,
    index: &'a AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Vec<&'a AutonomousResearchAcceptedWorkerEvidenceEntry> {
    accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .filter(|entry| accepted_worker_evidence_entry_matches_contract(contract, entry))
        .collect()
}

fn accepted_worker_evidence_entry_matches_contract(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    contract
        .worker_task_types
        .iter()
        .any(|task_type| task_type == &entry.task_type)
        || accepted_worker_evidence_entry_is_stage_synthesis(contract, entry)
        || (entry.task_type == "artifact_repair"
            && entry.required_output_artifact_type == contract.artifact_type)
}

pub(crate) fn accepted_worker_evidence_pending_review_entries_for_contract<'a>(
    contract: &AutonomousResearchStageContract,
    index: &'a AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Vec<&'a AutonomousResearchAcceptedWorkerEvidenceEntry> {
    index
        .entries
        .iter()
        .filter(|entry| {
            entry.active_status.as_deref() == Some("accepted_pending_review")
                && accepted_worker_evidence_entry_has_main_agent_decision(entry)
                && (contract
                    .worker_task_types
                    .iter()
                    .any(|task_type| task_type == &entry.task_type)
                    || accepted_worker_evidence_entry_is_stage_synthesis(contract, entry)
                    || (entry.task_type == "artifact_repair"
                        && entry.required_output_artifact_type == contract.artifact_type))
        })
        .collect()
}

pub(crate) fn accepted_worker_evidence_gate_entries_for_contract<'a>(
    contract: &AutonomousResearchStageContract,
    index: &'a AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Vec<&'a AutonomousResearchAcceptedWorkerEvidenceEntry> {
    let mut entries = accepted_worker_evidence_entries_for_contract(contract, index);
    for entry in accepted_worker_evidence_pending_review_entries_for_contract(contract, index) {
        if !entries
            .iter()
            .any(|existing| existing.agent_id == entry.agent_id)
        {
            entries.push(entry);
        }
    }
    entries
}

pub(crate) fn accepted_worker_evidence_current_gate_entry_count(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> usize {
    index
        .map(|index| accepted_worker_evidence_current_gate_entries(index).len())
        .unwrap_or(0)
}

pub(crate) fn accepted_worker_evidence_non_current_entry_count(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> usize {
    let Some(index) = index else {
        return 0;
    };
    let current_task_ids = accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .map(|entry| entry.task_id.clone())
        .collect::<BTreeSet<_>>();
    index
        .entries
        .iter()
        .filter(|entry| !current_task_ids.contains(&entry.task_id))
        .count()
}

pub(crate) fn accepted_worker_evidence_current_gate_task_ids(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    let mut task_ids = accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .map(|entry| entry.task_id.clone())
        .collect::<Vec<_>>();
    task_ids.sort();
    task_ids.dedup();
    task_ids
}

pub(crate) fn accepted_worker_evidence_current_gate_task_types(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    let mut task_types = accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .map(|entry| entry.task_type.clone())
        .collect::<Vec<_>>();
    task_types.sort();
    task_types.dedup();
    task_types
}

pub(crate) fn accepted_worker_evidence_current_gate_set_ids(
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    let mut set_ids = accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .filter_map(|entry| entry.current_evidence_set_id.clone())
        .collect::<Vec<_>>();
    set_ids.sort();
    set_ids.dedup();
    set_ids
}

pub(crate) fn accepted_worker_evidence_entry_refs(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut refs = entry.evidence_refs.clone();
    merge_unique_strings(
        &mut refs,
        vec![
            entry.output_manifest_ref.clone(),
            entry.task_packet_ref.clone(),
        ],
    );
    refs
}

pub(crate) fn accepted_worker_evidence_ref_matches(reference: &str, candidate: &str) -> bool {
    let left = relative_display_ref(reference);
    let right = relative_display_ref(candidate);
    reference == candidate
        || left == right
        || reference.ends_with(&right)
        || candidate.ends_with(&left)
}

pub(crate) fn accepted_worker_evidence_entry_supports_adoption(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> bool {
    if accepted_worker_evidence_entry_is_retired(entry) {
        return false;
    }
    if entry.agent_id != record.source_agent_id {
        return false;
    }
    if record
        .source_task_id
        .as_ref()
        .is_some_and(|task_id| task_id != &entry.task_id)
    {
        return false;
    }
    let refs = accepted_worker_evidence_entry_refs(entry);
    refs.iter()
        .any(|candidate| accepted_worker_evidence_ref_matches(candidate, &record.source_ref))
        || record.evidence_refs.iter().any(|requested_ref| {
            refs.iter()
                .any(|candidate| accepted_worker_evidence_ref_matches(candidate, requested_ref))
        })
}

pub(crate) fn accepted_worker_evidence_entry_is_main_agent_accepted(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    accepted_worker_evidence_entry_has_main_agent_decision(entry)
        && accepted_worker_evidence_entry_is_active(entry)
}

pub(crate) fn accepted_worker_evidence_entry_counts_for_stage_gate(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    accepted_worker_evidence_entry_is_main_agent_accepted(entry)
}

pub(crate) fn accepted_worker_evidence_entry_is_stage_synthesis(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    crate::accepted_worker_evidence::stage_synthesis_matches_artifact(
        &entry.worker_role,
        &entry.task_type,
        &entry.required_output_artifact_type,
        &contract.artifact_type,
    )
}

pub(crate) fn accepted_worker_evidence_entry_is_adoptable_stage_synthesis(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    accepted_worker_evidence_entry_is_main_agent_accepted(entry)
        && accepted_worker_evidence_entry_is_stage_synthesis(contract, entry)
}

pub(crate) fn accepted_worker_evidence_has_adoptable_stage_synthesis(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    index
        .map(|index| {
            accepted_worker_evidence_current_gate_entries(index)
                .into_iter()
                .any(|entry| {
                    accepted_worker_evidence_entry_is_adoptable_stage_synthesis(contract, entry)
                })
        })
        .unwrap_or(false)
}

pub(crate) fn accepted_worker_evidence_missing_adoptable_stage_synthesis_reason(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Option<String> {
    if accepted_worker_evidence_has_adoptable_stage_synthesis(contract, index) {
        return None;
    }
    Some(format!(
        "missing main-agent-accepted research_synthesizer stage artifact synthesis evidence for `{}`; local evidence fragments such as method comparison must be integrated into a clean `{}` candidate before adoption or review",
        contract.stage_id, contract.artifact_type
    ))
}

pub(crate) fn main_agent_stage_artifact_adoption_record_targets_entry(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> bool {
    if entry.agent_id != record.source_agent_id {
        return false;
    }
    if let Some(source_task_id) = record.source_task_id.as_ref() {
        return &entry.task_id == source_task_id
            || accepted_worker_evidence_entry_supports_adoption(entry, record);
    }
    accepted_worker_evidence_entry_supports_adoption(entry, record)
}

pub(crate) fn main_agent_stage_artifact_adoption_records_after_review(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
) -> Vec<MainAgentStageArtifactAdoptionRecord> {
    load_main_agent_stage_artifact_adoption_records(resolved)
        .unwrap_or_default()
        .into_iter()
        .filter(|record| {
            record.stage_id == contract.stage_id
                && record.target_artifact_path == contract.artifact_path
                && Some(record.stage_execution_id.as_str()) == job.stage_execution_id.as_deref()
                && timestamp_string_is_after(&record.created_at, &review.created_at)
        })
        .filter(|record| {
            main_agent_stage_artifact_adoption_manifest_path(resolved, job, record).exists()
        })
        .collect()
}

pub(crate) fn failed_review_repair_has_post_review_state_transition(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    stage_artifact: &str,
) -> bool {
    let requires_evidence_gap_worker_repair =
        autonomous_research_review_requires_evidence_gap_worker_repair(review);
    let post_review_adoptions =
        main_agent_stage_artifact_adoption_records_after_review(resolved, job, contract, review);
    if post_review_adoptions.iter().any(|record| {
        index.entries.iter().any(|entry| {
            accepted_worker_evidence_entry_supports_adoption(entry, record)
                && (!requires_evidence_gap_worker_repair
                    || accepted_worker_evidence_entry_can_repair_evidence_gap(entry))
        })
    }) {
        return true;
    }

    let post_review_entries = index
        .entries
        .iter()
        .filter(|entry| {
            entry
                .created_at
                .as_deref()
                .map(|created_at| timestamp_string_is_after(created_at, &review.created_at))
                .unwrap_or(false)
                && (!requires_evidence_gap_worker_repair
                    || accepted_worker_evidence_entry_can_repair_evidence_gap(entry))
        })
        .cloned()
        .collect::<Vec<_>>();
    if post_review_entries.is_empty() {
        return false;
    }
    let post_review_index = AutonomousResearchAcceptedWorkerEvidenceIndex {
        schema_version: index.schema_version.clone(),
        job_id: index.job_id.clone(),
        project_id: index.project_id.clone(),
        stage_execution_id: index.stage_execution_id.clone(),
        stage_id: index.stage_id.clone(),
        generated_at: index.generated_at.clone(),
        entries: post_review_entries,
    };
    stage_artifact_cites_accepted_worker_evidence(stage_artifact, Some(&post_review_index))
}

pub(crate) fn accepted_worker_evidence_missing_required_task_types(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let mut accepted_task_types = Vec::new();
    if let Some(index) = index {
        for entry in accepted_worker_evidence_gate_entries_for_contract(contract, index) {
            merge_unique_strings(&mut accepted_task_types, vec![entry.task_type.clone()]);
        }
    }
    contract
        .worker_task_types
        .iter()
        .filter(|task_type| {
            !accepted_task_types
                .iter()
                .any(|accepted| accepted == *task_type)
        })
        .cloned()
        .collect()
}

pub(crate) fn accepted_worker_evidence_covers_required_task_types(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    accepted_worker_evidence_missing_required_task_types(contract, index).is_empty()
}

pub(crate) fn accepted_worker_evidence_quality_floor_passes(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    if !accepted_worker_evidence_required_for_stage(contract, index) {
        return true;
    }
    let Some(index) = index else {
        return false;
    };
    let current_entries = accepted_worker_evidence_gate_entries_for_contract(contract, index);
    current_entries
        .iter()
        .all(|entry| matches!(entry.quality_profile.level.as_str(), "strong" | "partial"))
}

pub(crate) fn accepted_worker_evidence_quality_floor_failures(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    accepted_worker_evidence_gate_entries_for_contract(contract, index)
        .into_iter()
        .filter(|entry| !matches!(entry.quality_profile.level.as_str(), "strong" | "partial"))
        .map(|entry| {
            format!(
                "{} / {} / level={} / score={} / risks={}",
                entry.task_id,
                entry.task_type,
                entry.quality_profile.level,
                entry.quality_profile.score,
                comma_or_none_runtime(&entry.quality_profile.risks)
            )
        })
        .collect()
}

pub(crate) fn accepted_worker_evidence_semantic_review_floor_passes(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    if !accepted_worker_evidence_required_for_stage(contract, index) {
        return true;
    }
    let Some(index) = index else {
        return false;
    };
    let current_entries = accepted_worker_evidence_gate_entries_for_contract(contract, index);
    current_entries
        .iter()
        .all(|entry| accepted_worker_semantic_review_passes(contract, entry))
}

pub(crate) fn accepted_worker_semantic_review_passes(
    contract: &AutonomousResearchStageContract,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    accepted_worker_semantic_review_passes_for_stage(&contract.stage_id, entry)
}

pub(crate) fn accepted_worker_semantic_review_required_score(
    contract: &AutonomousResearchStageContract,
) -> u64 {
    accepted_worker_semantic_review_required_score_for_stage(&contract.stage_id)
}

pub(crate) fn accepted_worker_semantic_review_required_score_for_stage(stage_id: &str) -> u64 {
    if stage_id == "research-review" {
        85
    } else {
        75
    }
}

pub(crate) fn accepted_worker_evidence_semantic_review_floor_failures(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    let required_score = accepted_worker_semantic_review_required_score(contract);
    accepted_worker_evidence_gate_entries_for_contract(contract, index)
        .into_iter()
        .filter(|entry| !accepted_worker_semantic_review_passes(contract, entry))
        .map(|entry| {
            if let Some(review) = entry.semantic_review.as_ref() {
                format!(
                    "{} / {} / semantic verdict={} / score={} / required_score={} / findings={}",
                    entry.task_id,
                    entry.task_type,
                    review.verdict,
                    review
                        .score
                        .map(|score| score.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    required_score,
                    comma_or_none_runtime(&review.findings)
                )
            } else if let Some(acceptance) = entry.main_agent_acceptance.as_ref() {
                format!(
                    "{} / {} / independent semantic review missing / review_required={} / main_agent_acceptance verdict={} score={} execution_mode={} / required_score={}",
                    entry.task_id,
                    entry.task_type,
                    entry
                        .review_required
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unspecified".to_string()),
                    acceptance.verdict,
                    acceptance
                        .score
                        .map(|score| score.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    acceptance.execution_mode,
                    required_score
                )
            } else {
                format!(
                    "{} / {} / semantic review missing / required_score={}",
                    entry.task_id, entry.task_type, required_score
                )
            }
        })
        .collect()
}

pub(crate) fn accepted_worker_evidence_active_tool_floor_passes(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    if !accepted_worker_evidence_required_for_stage(contract, index) {
        return true;
    }
    let Some(index) = index else {
        return false;
    };
    let current_entries = accepted_worker_evidence_gate_entries_for_contract(contract, index);
    current_entries
        .iter()
        .filter(|entry| accepted_worker_active_tool_evidence_required(entry))
        .all(|entry| accepted_worker_active_tool_evidence_passes(entry))
}

pub(crate) fn accepted_worker_active_tool_evidence_required(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    !matches!(
        entry.task_type.as_str(),
        "acceptance standard setting" | "review finding triage"
    ) && !entry.worker_role.contains("standard_setter")
}

pub(crate) fn accepted_worker_active_tool_evidence_passes(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> bool {
    entry.matched_quality_signals.iter().any(|signal| {
        signal.contains("action_schema_version")
            || signal.contains("provider_tool_completed")
            || signal.contains("tool_actions")
            || signal.contains("worker tool audit")
    }) || entry.evidence_refs.iter().any(|evidence_ref| {
        let lowered = evidence_ref.to_ascii_lowercase();
        lowered.contains(".pmcli/goal-worker-output/actions")
            || lowered.contains("provider_tool_audit")
            || lowered.contains("_source_retrieval.tsv")
            || lowered.contains("_experiment_plan.tsv")
            || lowered.contains("_tests.log")
            || lowered.contains("_run.log")
            || lowered.contains("_metrics.tsv")
            || lowered.contains("_paper_bundle.tsv")
            || lowered.contains("_latex.log")
            || lowered.contains("_final_review_audit.tsv")
            || lowered.contains("_strategy_update.md")
    })
}

pub(crate) fn accepted_worker_evidence_active_tool_floor_failures(
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    if !accepted_worker_evidence_required_for_stage(contract, index) {
        return Vec::new();
    }
    let Some(index) = index else {
        return vec!["accepted worker evidence index missing".to_string()];
    };
    accepted_worker_evidence_gate_entries_for_contract(contract, index)
        .into_iter()
        .filter(|entry| accepted_worker_active_tool_evidence_required(entry))
        .filter(|entry| !accepted_worker_active_tool_evidence_passes(entry))
        .map(|entry| {
            format!(
                "{} / {} / matched_quality_signals={} / evidence_refs={}",
                entry.task_id,
                entry.task_type,
                comma_or_none_runtime(&entry.matched_quality_signals),
                comma_or_none_runtime(&entry.evidence_refs)
            )
        })
        .collect()
}

pub(crate) fn stage_artifact_cites_accepted_worker_evidence(
    artifact: &str,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    let Some(index) = index else {
        return true;
    };
    let current_entries = accepted_worker_evidence_current_gate_entries(index);
    if current_entries.is_empty() {
        return true;
    }
    let artifact_lower = artifact.to_ascii_lowercase();
    current_entries.iter().any(|entry| {
        artifact_lower.contains(&entry.agent_id.to_ascii_lowercase())
            || artifact_lower.contains(&entry.task_id.to_ascii_lowercase())
            || entry.evidence_refs.iter().any(|evidence_ref| {
                artifact_lower.contains(&relative_display_ref(evidence_ref).to_ascii_lowercase())
            })
            || artifact_lower
                .contains(&relative_display_ref(&entry.output_manifest_ref).to_ascii_lowercase())
    })
}

pub(crate) fn accepted_worker_evidence_bound_to_stage_artifact(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    if !accepted_worker_evidence_required_for_stage(contract, index) {
        return true;
    }
    autonomous_research_stage_artifact_has_active_adoption_receipt(resolved, job, contract, index)
}

pub(crate) fn autonomous_research_stage_artifact_has_active_adoption_receipt(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> bool {
    autonomous_research_stage_artifact_active_adoption_receipts(resolved, job, contract)
        .into_iter()
        .any(|record| {
            index
                .map(|index| {
                    index.entries.iter().any(|entry| {
                        accepted_worker_evidence_entry_supports_adoption(entry, &record)
                            && accepted_worker_evidence_entry_is_adoptable_stage_synthesis(
                                contract, entry,
                            )
                    })
                })
                .unwrap_or(false)
        })
}

pub(crate) fn autonomous_research_stage_artifact_has_materialization_receipt(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> bool {
    !autonomous_research_stage_artifact_active_adoption_receipts(resolved, job, contract).is_empty()
}

pub(crate) fn autonomous_research_stage_artifact_active_adoption_receipts(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Vec<MainAgentStageArtifactAdoptionRecord> {
    let stage_artifact_path = resolved.workspace_root.join(&contract.artifact_path);
    let Ok(stage_artifact) = std::fs::read_to_string(&stage_artifact_path) else {
        return Vec::new();
    };
    let stage_artifact_sha256 = sha256_hex(stage_artifact.as_bytes());
    let review_body_sha256 =
        sha256_hex(autonomous_research_stage_artifact_review_body(&stage_artifact).as_bytes());
    load_main_agent_stage_artifact_adoption_records(resolved)
        .unwrap_or_default()
        .into_iter()
        .filter(|record| {
            record.stage_id == contract.stage_id
                && Some(record.stage_execution_id.as_str()) == job.stage_execution_id.as_deref()
                && record.target_artifact_path == contract.artifact_path
                && main_agent_stage_artifact_adoption_manifest_path(resolved, job, record).exists()
        })
        .filter(|record| {
            let manifest_path =
                main_agent_stage_artifact_adoption_manifest_path(resolved, job, record);
            let manifest = read_json_file_runtime(&manifest_path).unwrap_or(Value::Null);
            json_string_field_runtime(&manifest, "content_sha256")
                .map(|value| value == stage_artifact_sha256)
                .unwrap_or(false)
                || json_string_field_runtime(&manifest, "review_body_sha256")
                    .map(|value| value == review_body_sha256)
                    .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn append_accepted_worker_evidence_section(
    content: &str,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> String {
    let Some(index) = index else {
        return content.to_string();
    };
    if index.entries.is_empty() {
        return content.to_string();
    }
    let lower = content.to_ascii_lowercase();
    let has_accepted_worker_evidence_section = [
        "\n## accepted worker evidence",
        "\n# accepted worker evidence",
        "## accepted worker evidence",
        "# accepted worker evidence",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if stage_artifact_cites_accepted_worker_evidence(content, Some(index))
        && has_accepted_worker_evidence_section
    {
        return content.to_string();
    }
    format!(
        "{}\n\n## Accepted Worker Evidence\n\n{}\n",
        content.trim_end(),
        render_autonomous_research_accepted_worker_evidence_index(Some(index))
    )
}

pub(crate) fn persist_autonomous_research_stage_artifact_candidate(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    stage_artifact_path: &Path,
    _final_content: &str,
    _index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Result<(), String> {
    let current = std::fs::read_to_string(stage_artifact_path).unwrap_or_default();
    if autonomous_research_stage_artifact_candidate_is_reviewable(&current, contract)
        && autonomous_research_stage_artifact_has_materialization_receipt(resolved, job, contract)
    {
        return Ok(());
    }

    if !current.trim().is_empty() {
        preserve_unreviewable_stage_artifact_diagnostic(
            resolved,
            job,
            contract,
            stage_artifact_path,
            &current,
        )?;
    }

    if stage_artifact_path.exists() {
        std::fs::remove_file(stage_artifact_path).map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub(crate) fn preserve_unreviewable_stage_artifact_diagnostic(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    stage_artifact_path: &Path,
    current: &str,
) -> Result<(), String> {
    let review_body = autonomous_research_stage_artifact_review_body(current);
    let lifecycle_violations =
        autonomous_research_stage_artifact_candidate_lifecycle_violations(current, contract);
    let is_reviewable =
        autonomous_research_stage_artifact_candidate_is_reviewable(current, contract);
    let has_materialization_receipt =
        autonomous_research_stage_artifact_has_materialization_receipt(resolved, job, contract);
    let active_receipt_count =
        autonomous_research_stage_artifact_active_adoption_receipts(resolved, job, contract).len();
    let rejection_reason = match (is_reviewable, has_materialization_receipt) {
        (false, false) => "stage_artifact_not_reviewable_and_missing_adopt_stage_artifact_receipt",
        (false, true) => "stage_artifact_not_reviewable",
        (true, false) => "stage_artifact_missing_adopt_stage_artifact_receipt",
        (true, true) => "stage_artifact_removed_after_unexpected_boundary_failure",
    };
    let lifecycle_violations = if lifecycle_violations.is_empty() {
        "none".to_string()
    } else {
        lifecycle_violations.join("; ")
    };
    let archive_dir = resolved
        .workspace_root
        .join("research")
        .join("auto")
        .join(&job.job_id)
        .join("stage-artifact-rejections");
    std::fs::create_dir_all(&archive_dir).map_err(|err| err.to_string())?;
    let archive_path = archive_dir.join(format!(
        "{}-{}.md",
        sanitize_runtime_path_component(&contract.artifact_type),
        timestamp_string()
    ));
    let archive_body = format!(
        "# Rejected Stage Artifact Candidate\n\n\
         - job_id: `{}`\n\
         - stage_id: `{}`\n\
         - stage_artifact_path: `{}`\n\
         - rejection_reason: `{}`\n\
         - is_reviewable: `{}`\n\
         - has_materialization_receipt: `{}`\n\
         - active_adoption_receipt_count: `{}`\n\
         - review_body_word_count: `{}`\n\
         - lifecycle_violations: `{}`\n\
         - boundary: `only_explicit_adopt_stage_artifact_materializes_the_canonical_stage_artifact`\n\
         - removed_from_review_target: `{}`\n\n\
         ## Preserved Content\n\n{}\n",
        job.job_id,
        contract.stage_id,
        contract.artifact_path,
        rejection_reason,
        is_reviewable,
        has_materialization_receipt,
        active_receipt_count,
        review_body.split_whitespace().count(),
        lifecycle_violations,
        stage_artifact_path.display(),
        current.trim()
    );
    write_text_atomic_runtime(&archive_path, &archive_body)
}

pub(crate) fn process_main_agent_stage_artifact_adoptions(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Result<Vec<String>, String> {
    let records = load_main_agent_stage_artifact_adoption_records(resolved)?;
    if records.is_empty() {
        return Ok(Vec::new());
    }
    let mut adopted_refs = Vec::new();
    for record in records {
        if main_agent_stage_artifact_adoption_record_targets_stage_governance(
            resolved, job, contract, &record,
        ) {
            append_autonomous_research_job_event(
                resolved,
                job,
                "stage_governance_artifact_adoption_skipped",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "adoption_ref": record.adoption_ref,
                    "stage_id": record.stage_id,
                    "stage_execution_id": record.stage_execution_id,
                    "source_agent_id": record.source_agent_id,
                    "source_ref": record.source_ref,
                    "target_artifact_path": record.target_artifact_path,
                    "reason": "stage governance documents are not final stage artifacts; main agent must use record_stage_evidence_plan for adopted requirements, while runtime maintains review support docframes mechanically"
                }),
            )?;
            continue;
        }
        if record.stage_id != contract.stage_id
            || Some(record.stage_execution_id.as_str()) != job.stage_execution_id.as_deref()
            || autonomous_research_stage_adoption_target_kind(resolved, job, contract, &record)
                .is_none()
        {
            continue;
        }
        if main_agent_stage_artifact_adoption_manifest_path(resolved, job, &record).exists() {
            continue;
        }
        if main_agent_stage_artifact_adoption_has_non_retryable_rejection(resolved, job, &record) {
            continue;
        }
        match apply_main_agent_stage_artifact_adoption(resolved, job, contract, index, &record) {
            Ok(adoption_manifest_ref) => {
                append_autonomous_research_job_event(
                    resolved,
                    job,
                    "stage_artifact_adopted",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "adoption_ref": record.adoption_ref,
                        "adoption_manifest_ref": adoption_manifest_ref,
                        "stage_id": record.stage_id,
                        "stage_execution_id": record.stage_execution_id,
                        "source_agent_id": record.source_agent_id,
                        "source_ref": record.source_ref,
                        "target_artifact_path": record.target_artifact_path,
                        "request_review_rerun": record.request_review_rerun
                    }),
                )?;
                merge_unique_strings(
                    &mut adopted_refs,
                    vec![
                        adoption_manifest_ref,
                        autonomous_research_stage_adoption_target_ref(contract, &record),
                    ],
                );
            }
            Err(reason) => {
                let rejection_manifest_ref =
                    write_main_agent_stage_artifact_adoption_rejection_manifest(
                        resolved, job, contract, &record, &reason,
                    )
                    .ok();
                let retired_evidence_index_ref =
                    retire_accepted_worker_evidence_for_rejected_stage_adoption(
                        resolved, job, contract, &record, &reason,
                    )
                    .ok()
                    .flatten();
                append_autonomous_research_job_event(
                    resolved,
                    job,
                    "stage_artifact_adoption_rejected",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "adoption_ref": record.adoption_ref,
                        "stage_id": record.stage_id,
                        "stage_execution_id": record.stage_execution_id,
                        "source_agent_id": record.source_agent_id,
                        "source_ref": record.source_ref,
                        "target_artifact_path": record.target_artifact_path,
                        "reason": reason,
                        "rejection_manifest_ref": rejection_manifest_ref,
                        "retired_evidence_index_ref": retired_evidence_index_ref
                    }),
                )?;
            }
        }
    }
    Ok(adopted_refs)
}

pub(crate) fn autonomous_research_stage_adoption_target_kind(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> Option<MainAgentStageArtifactAdoptionTargetKind> {
    if record.target_artifact_path == contract.artifact_path {
        return Some(MainAgentStageArtifactAdoptionTargetKind::StageArtifact);
    }
    if main_agent_stage_artifact_adoption_record_targets_stage_governance(
        resolved, job, contract, record,
    ) {
        return None;
    }
    if canonical_project_file_adoption_target_is_allowed(&record.target_artifact_path) {
        return Some(MainAgentStageArtifactAdoptionTargetKind::ProjectFile);
    }
    None
}

pub(crate) fn autonomous_research_stage_adoption_target_ref(
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> String {
    if record.target_artifact_path == contract.artifact_path {
        format!("stage_artifact:{}", contract.artifact_path)
    } else if canonical_project_file_adoption_target_is_allowed(&record.target_artifact_path) {
        format!("canonical_project_file:{}", record.target_artifact_path)
    } else {
        format!(
            "unsupported_stage_artifact_adoption_target:{}",
            record.target_artifact_path
        )
    }
}

pub(crate) fn main_agent_stage_artifact_adoption_record_targets_stage_governance(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> bool {
    let target = record.target_artifact_path.replace('\\', "/");
    let stage_plan_ref = relative_workspace_ref(
        &resolved.workspace_root,
        &autonomous_research_stage_plan_path(resolved, job, contract),
    );
    let rubric_ref = relative_workspace_ref(
        &resolved.workspace_root,
        &autonomous_research_stage_rubric_path(resolved, job, contract),
    );
    if target == stage_plan_ref || target == rubric_ref {
        return true;
    }
    target.starts_with("research/stages/")
        && matches!(
            Path::new(&target)
                .file_name()
                .and_then(|value| value.to_str()),
            Some("stage_plan.md") | Some("stage_acceptance_rubric.md")
        )
}

pub(crate) fn canonical_project_file_adoption_target_is_allowed(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if !stage_artifact_adoption_candidate_path_is_safe(&normalized) {
        return false;
    }
    if normalized == ".pmcli"
        || normalized.starts_with(".pmcli/")
        || normalized.starts_with(".git/")
        || normalized == ".git"
    {
        return false;
    }
    let path = Path::new(&normalized);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension.is_empty() {
        return normalized.starts_with("experiments/")
            || normalized.starts_with("src/")
            || normalized.starts_with("paper/")
            || normalized.starts_with("papers/")
            || normalized.starts_with("research/");
    }
    matches!(
        extension.as_str(),
        "py" | "json"
            | "jsonl"
            | "md"
            | "tex"
            | "bib"
            | "toml"
            | "yaml"
            | "yml"
            | "csv"
            | "tsv"
            | "txt"
            | "rs"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
    )
}

pub(crate) fn record_canonical_artifact_adoption_requested(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Result<String, String> {
    canonical_artifacts::upsert_adoption_requested(
        &resolved.data_dir,
        CanonicalArtifactSeed {
            job_id: job.job_id.clone(),
            stage_id: contract.stage_id.clone(),
            stage_execution_id: record.stage_execution_id.clone(),
            source_agent_id: record.source_agent_id.clone(),
            source_task_id: record.source_task_id.clone(),
            source_ref: record.source_ref.clone(),
            source_artifact_path: record.source_artifact_path.clone(),
            target_artifact_path: record.target_artifact_path.clone(),
            artifact_kind: canonical_artifact_kind_for_adoption_target(
                contract,
                &record.target_artifact_path,
            ),
            task_type: entry.task_type.clone(),
            decision_ref: record.adoption_ref.clone(),
            source_sha256: None,
        },
        "main agent requested canonical artifact adoption; runtime has not proven materialization or baseline visibility yet",
    )
    .map(|entry| entry.artifact_id)
    .map_err(|err| err.to_string())
}

pub(crate) fn canonical_artifact_kind_for_adoption_target(
    contract: &AutonomousResearchStageContract,
    target_artifact_path: &str,
) -> String {
    if target_artifact_path == contract.artifact_path {
        return contract.artifact_type.clone();
    }
    match Path::new(target_artifact_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => "python_source",
        "rs" | "ts" | "tsx" | "js" | "jsx" => "source_file",
        "json" => "json",
        "jsonl" => "jsonl",
        "md" => "markdown",
        "tex" => "latex_source",
        "bib" => "bibtex",
        "csv" => "csv",
        "tsv" => "tsv",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "txt" => "text",
        _ => "project_file",
    }
    .to_string()
}

pub(crate) fn validate_accepted_worker_evidence_for_adoption_target(
    contract: &AutonomousResearchStageContract,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    target_kind: MainAgentStageArtifactAdoptionTargetKind,
) -> Result<(), String> {
    match target_kind {
        MainAgentStageArtifactAdoptionTargetKind::StageArtifact => {
            if !accepted_worker_evidence_covers_required_task_types(contract, Some(index)) {
                return Err(format!(
                    "accepted worker evidence is missing required task types: {}",
                    accepted_worker_evidence_missing_required_task_types(contract, Some(index))
                        .join(", ")
                ));
            }
            if !accepted_worker_evidence_quality_floor_passes(contract, Some(index)) {
                return Err(format!(
                    "accepted worker evidence quality floor failed: {}",
                    accepted_worker_evidence_quality_floor_failures(contract, Some(index))
                        .join("; ")
                ));
            }
            if !accepted_worker_evidence_semantic_review_floor_passes(contract, Some(index)) {
                return Err(format!(
                    "accepted worker semantic review floor failed: {}",
                    accepted_worker_evidence_semantic_review_floor_failures(contract, Some(index))
                        .join("; ")
                ));
            }
            if !accepted_worker_evidence_active_tool_floor_passes(contract, Some(index)) {
                return Err(format!(
                    "accepted worker active-tool evidence floor failed: {}",
                    accepted_worker_evidence_active_tool_floor_failures(contract, Some(index))
                        .join("; ")
                ));
            }
        }
        MainAgentStageArtifactAdoptionTargetKind::ProjectFile => {
            if accepted_worker_evidence_entry_is_retired(entry) {
                return Err("accepted worker evidence entry is retired".to_string());
            }
            if !matches!(entry.quality_profile.level.as_str(), "strong" | "partial") {
                return Err(format!(
                    "accepted worker evidence quality floor failed for source task `{}`: level={} score={}",
                    entry.task_id, entry.quality_profile.level, entry.quality_profile.score
                ));
            }
            if !accepted_worker_semantic_review_passes(contract, entry) {
                return Err(format!(
                    "accepted worker semantic review floor failed for source task `{}`",
                    entry.task_id
                ));
            }
            if accepted_worker_active_tool_evidence_required(entry)
                && !accepted_worker_active_tool_evidence_passes(entry)
            {
                return Err(format!(
                    "accepted worker active-tool evidence floor failed for source task `{}`",
                    entry.task_id
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn record_canonical_artifact_materialized(
    resolved: &ResolvedProject,
    artifact_id: &str,
    contents: &str,
) -> Result<(), String> {
    record_canonical_artifact_materialized_bytes(resolved, artifact_id, contents.as_bytes())
}

pub(crate) fn record_canonical_artifact_materialized_bytes(
    resolved: &ResolvedProject,
    artifact_id: &str,
    contents: &[u8],
) -> Result<(), String> {
    canonical_artifacts::record_materialized(&resolved.data_dir, artifact_id, sha256_hex(contents))
        .map_err(|err| err.to_string())?;
    match canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
        &resolved.data_dir,
        &resolved.workspace_root,
        artifact_id,
    ) {
        Ok(_) => Ok(()),
        Err(err) => {
            let reason = err.to_string();
            let _ = canonical_artifacts::record_baseline_promotion_blocked(
                &resolved.data_dir,
                artifact_id,
                &reason,
            );
            Err(reason)
        }
    }
}

pub(crate) fn record_canonical_artifact_materialized_directory(
    resolved: &ResolvedProject,
    artifact_id: &str,
    directory_sha256: &str,
) -> Result<(), String> {
    canonical_artifacts::record_materialized(
        &resolved.data_dir,
        artifact_id,
        directory_sha256.to_string(),
    )
    .map_err(|err| err.to_string())?;
    match canonical_artifacts::promote_materialized_artifact_to_overlay_baseline(
        &resolved.data_dir,
        &resolved.workspace_root,
        artifact_id,
    ) {
        Ok(_) => Ok(()),
        Err(err) => {
            let reason = err.to_string();
            let _ = canonical_artifacts::record_baseline_promotion_blocked(
                &resolved.data_dir,
                artifact_id,
                &reason,
            );
            Err(reason)
        }
    }
}

pub(crate) fn active_canonical_artifact_ids_for_target(
    resolved: &ResolvedProject,
    target_artifact_path: &str,
) -> Result<Vec<String>, String> {
    let ledger = canonical_artifacts::load_ledger(&resolved.data_dir)
        .map_err(|err| format!("canonical artifact ledger could not be loaded: {err}"))?;
    Ok(ledger
        .entries
        .iter()
        .filter(|entry| {
            entry.target_artifact_path == target_artifact_path
                && matches!(
                    entry.status,
                    canonical_artifacts::CanonicalArtifactStatus::BaselineVisible
                        | canonical_artifacts::CanonicalArtifactStatus::IntegrationFailed
                        | canonical_artifacts::CanonicalArtifactStatus::IntegrationVerified
                        | canonical_artifacts::CanonicalArtifactStatus::ActiveStageEvidence
                )
        })
        .map(|entry| entry.artifact_id.clone())
        .collect())
}

pub(crate) fn validate_main_agent_replacement_intent_for_target(
    resolved: &ResolvedProject,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> Result<Vec<String>, String> {
    let active_artifact_ids =
        active_canonical_artifact_ids_for_target(resolved, &record.target_artifact_path)?;
    if active_artifact_ids.is_empty() {
        return Ok(Vec::new());
    }
    if record.replacement_of_artifact_ids.is_empty() {
        return Err(format!(
            "target `{}` already has active canonical artifact(s) {}; main agent must explicitly set replacement_of_artifact_ids before runtime can materialize a replacement",
            record.target_artifact_path,
            active_artifact_ids.join(", ")
        ));
    }
    let missing = active_artifact_ids
        .iter()
        .filter(|artifact_id| {
            !record
                .replacement_of_artifact_ids
                .iter()
                .any(|replacement_id| replacement_id == *artifact_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "replacement_of_artifact_ids for target `{}` does not include active canonical artifact(s) {}",
            record.target_artifact_path,
            missing.join(", ")
        ));
    }
    Ok(active_artifact_ids)
}

pub(crate) fn retire_replaced_canonical_artifacts_after_materialization(
    resolved: &ResolvedProject,
    record: &MainAgentStageArtifactAdoptionRecord,
    replaced_artifact_ids: &[String],
    replacement_artifact_id: &str,
) -> Result<Vec<String>, String> {
    let mut retired_refs = Vec::new();
    for artifact_id in replaced_artifact_ids {
        let retired = canonical_artifacts::record_retired_or_superseded(
            &resolved.data_dir,
            artifact_id,
            format!(
                "{} replaces with {}",
                record.adoption_ref, replacement_artifact_id
            ),
            if record.cleanup_required {
                Some(format!("{}::cleanup_required", record.adoption_ref))
            } else {
                None
            },
        )
        .map_err(|err| format!("canonical artifact replacement retirement failed: {err}"))?;
        merge_unique_strings(
            &mut retired_refs,
            vec![
                format!("canonical_artifact_retired:{}", retired.artifact_id),
                format!("retired_canonical_target:{}", retired.target_artifact_path),
            ],
        );
    }
    Ok(retired_refs)
}

pub(crate) fn load_main_agent_stage_artifact_adoption_records(
    resolved: &ResolvedProject,
) -> Result<Vec<MainAgentStageArtifactAdoptionRecord>, String> {
    let dir = resolved
        .data_dir
        .join("main-agent-board")
        .join("stage-artifact-adoptions");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|err| err.to_string())? {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<MainAgentStageArtifactAdoptionRecord>(&content)
        else {
            continue;
        };
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.adoption_ref.cmp(&right.adoption_ref))
    });
    Ok(records)
}

pub(crate) fn validate_main_agent_stage_artifact_adoption_evidence_snapshot(
    contract: &AutonomousResearchStageContract,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> Result<(Vec<String>, String), String> {
    let (accepted_evidence_revision_refs, evidence_snapshot_hash) =
        accepted_worker_evidence_snapshot_for_contract(contract, index)?;
    if let Some(recorded_snapshot_hash) = record.evidence_snapshot_hash.as_deref() {
        if recorded_snapshot_hash != evidence_snapshot_hash {
            return Err(format!(
                "adoption evidence snapshot changed: recorded `{recorded_snapshot_hash}`, current `{evidence_snapshot_hash}`. Record a new main-agent adoption decision for the current accepted evidence snapshot."
            ));
        }
    }
    if !record.accepted_evidence_revision_refs.is_empty() {
        let mut recorded_revision_refs = record.accepted_evidence_revision_refs.clone();
        recorded_revision_refs.sort();
        let mut current_revision_refs = accepted_evidence_revision_refs.clone();
        current_revision_refs.sort();
        if recorded_revision_refs != current_revision_refs {
            return Err(
                "adoption accepted evidence revisions no longer match the current evidence snapshot"
                    .to_string(),
            );
        }
    }
    Ok((accepted_evidence_revision_refs, evidence_snapshot_hash))
}

pub(crate) fn autonomous_research_stage_artifact_adoption_snapshot_failure(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Option<String> {
    let adoption = match load_main_agent_stage_artifact_adoption_records(resolved) {
        Ok(records) => records.into_iter().rev().find(|record| {
            record.stage_id == contract.stage_id
                && Some(record.stage_execution_id.as_str()) == job.stage_execution_id.as_deref()
                && record.target_artifact_path == contract.artifact_path
        }),
        Err(reason) => {
            return Some(format!(
                "stage artifact adoption records could not be loaded: {reason}"
            ));
        }
    };
    let Some(adoption) = adoption else {
        return None;
    };
    let Some(index) = index else {
        return Some("accepted worker evidence index is missing".to_string());
    };
    validate_main_agent_stage_artifact_adoption_evidence_snapshot(contract, index, &adoption).err()
}

pub(crate) fn apply_main_agent_stage_artifact_adoption(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> Result<String, String> {
    if record.stage_id != contract.stage_id {
        return Err(format!(
            "adoption stage `{}` does not match active stage `{}`",
            record.stage_id, contract.stage_id
        ));
    }
    if Some(record.stage_execution_id.as_str()) != job.stage_execution_id.as_deref() {
        return Err(format!(
            "adoption stage execution `{}` does not match active stage execution `{:?}`",
            record.stage_execution_id, job.stage_execution_id
        ));
    }
    let Some(target_kind) =
        autonomous_research_stage_adoption_target_kind(resolved, job, contract, record)
    else {
        return Err(format!(
            "target artifact `{}` is not an adoptable active-stage artifact for `{}`",
            record.target_artifact_path, contract.stage_id
        ));
    };
    if record.rationale.trim().is_empty() {
        return Err("main-agent adoption rationale is empty".to_string());
    }
    let Some(index) = index else {
        return Err("accepted worker evidence index is missing".to_string());
    };
    let (accepted_evidence_revision_refs, _) =
        validate_main_agent_stage_artifact_adoption_evidence_snapshot(contract, index, record)?;
    let Some(entry) = index
        .entries
        .iter()
        .find(|entry| accepted_worker_evidence_entry_supports_adoption(entry, record))
    else {
        return Err(format!(
            "source ref `{}` was not found in accepted worker evidence for agent `{}`",
            record.source_ref, record.source_agent_id
        ));
    };
    if accepted_worker_evidence_entry_has_main_agent_decision(entry)
        && entry
            .main_agent_decision_ref
            .as_ref()
            .is_none_or(|decision_ref| !accepted_evidence_revision_refs.contains(decision_ref))
    {
        return Err(format!(
            "source task `{}` is accepted but is not the explicitly selected current evidence revision for its evidence set",
            entry.task_id
        ));
    }
    validate_adoption_record_source_ref_agent_binding(resolved, record, entry)?;
    if target_kind == MainAgentStageArtifactAdoptionTargetKind::StageArtifact
        && !accepted_worker_evidence_entry_is_adoptable_stage_synthesis(contract, entry)
    {
        return Err(format!(
            "stage artifact adoption for `{}` must use a main-agent-accepted research_synthesizer stage artifact synthesis candidate; source task `{}` has task_type `{}`, worker_role `{}`, required_output `{}`, active_status `{}`, main_agent_decision_ref `{}`",
            contract.artifact_path,
            entry.task_id,
            entry.task_type,
            entry.worker_role,
            entry.required_output_artifact_type,
            entry.active_status.as_deref().unwrap_or("candidate"),
            entry.main_agent_decision_ref.as_deref().unwrap_or("none")
        ));
    }
    if target_kind == MainAgentStageArtifactAdoptionTargetKind::StageArtifact {
        validate_stage_synthesis_evidence_freshness(resolved, index, entry)?;
    }
    validate_accepted_worker_evidence_for_adoption_target(contract, index, entry, target_kind)?;
    if target_kind == MainAgentStageArtifactAdoptionTargetKind::StageArtifact {
        if let Some(manifest_ref) = try_rebind_visible_stage_artifact_snapshot(
            resolved, job, contract, index, record, entry,
        )? {
            return Ok(manifest_ref);
        }
    }
    let canonical_artifact_id =
        record_canonical_artifact_adoption_requested(resolved, job, contract, record, entry)?;
    let replaced_artifact_ids =
        match validate_main_agent_replacement_intent_for_target(resolved, record) {
            Ok(ids) => ids,
            Err(reason) => {
                let _ = canonical_artifacts::record_materialization_blocked(
                    &resolved.data_dir,
                    &canonical_artifact_id,
                    &reason,
                );
                return Err(reason);
            }
        };
    let candidates = match collect_main_agent_stage_artifact_adoption_source_candidates(
        resolved, contract, record, entry,
    ) {
        Ok(candidates) => candidates,
        Err(reason) => {
            let _ = canonical_artifacts::record_materialization_blocked(
                &resolved.data_dir,
                &canonical_artifact_id,
                &reason,
            );
            return Err(reason);
        }
    };
    if candidates.is_empty() {
        let reason = no_readable_source_candidate_reason(resolved, contract, record, entry);
        let _ = canonical_artifacts::record_materialization_blocked(
            &resolved.data_dir,
            &canonical_artifact_id,
            &reason,
        );
        return Err(reason);
    }
    let mut rejection_reasons = Vec::new();
    for candidate in candidates {
        match target_kind {
            MainAgentStageArtifactAdoptionTargetKind::StageArtifact => {
                let source_ref = candidate.source_ref;
                let candidate_content = candidate.content;
                let lifecycle_violations =
                    autonomous_research_stage_artifact_candidate_lifecycle_violations(
                        &candidate_content,
                        contract,
                    );
                if !lifecycle_violations.is_empty() {
                    rejection_reasons.push(format!(
                        "{source_ref}: candidate is not a clean canonical stage artifact body: {}",
                        lifecycle_violations.join("; ")
                    ));
                    continue;
                }
                if !autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                    &candidate_content,
                    contract,
                    candidate.relative_path.as_deref(),
                ) {
                    rejection_reasons.push(format!("{source_ref}: candidate is not reviewable"));
                    continue;
                }
                let evidence_binding_failures =
                    autonomous_research_stage_artifact_evidence_binding_failures(
                        &candidate_content,
                        contract,
                        Some(index),
                    );
                if !evidence_binding_failures.is_empty() {
                    rejection_reasons.push(format!(
                        "{source_ref}: candidate evidence binding contract failed: {}",
                        evidence_binding_failures.join("; ")
                    ));
                    continue;
                }
                let stage_content = wrap_autonomous_research_stage_artifact_body_with_docframe(
                    resolved,
                    job,
                    contract,
                    &candidate_content,
                );
                let target = resolved.workspace_root.join(&contract.artifact_path);
                write_autonomous_research_report(&target, &stage_content)
                    .map_err(|err| err.to_string())?;
                record_canonical_artifact_materialized(
                    resolved,
                    &canonical_artifact_id,
                    &stage_content,
                )?;
                retire_replaced_canonical_artifacts_after_materialization(
                    resolved,
                    record,
                    &replaced_artifact_ids,
                    &canonical_artifact_id,
                )?;
                let manifest_ref = write_main_agent_stage_artifact_adoption_manifest(
                    resolved,
                    job,
                    contract,
                    record,
                    entry,
                    &source_ref,
                    &stage_content,
                )?;
                return Ok(manifest_ref);
            }
            MainAgentStageArtifactAdoptionTargetKind::ProjectFile => {
                if !canonical_project_file_adoption_target_is_allowed(&record.target_artifact_path)
                {
                    rejection_reasons.push(format!(
                        "{}: target project file path is not allowed",
                        candidate.source_ref
                    ));
                    continue;
                }
                if candidate.kind == MainAgentStageArtifactAdoptionSourceCandidateKind::Directory {
                    let Some(directory_manifest) = candidate.directory_manifest.as_ref() else {
                        let reason = format!(
                            "{}: directory candidate is missing directory manifest",
                            candidate.source_ref
                        );
                        let _ = canonical_artifacts::record_materialization_blocked(
                            &resolved.data_dir,
                            &canonical_artifact_id,
                            &reason,
                        );
                        return Err(reason);
                    };
                    let Some(source_worktree_path) = candidate.source_worktree_path.as_ref() else {
                        let reason = format!(
                            "{}: directory candidate is missing source worktree path",
                            candidate.source_ref
                        );
                        let _ = canonical_artifacts::record_materialization_blocked(
                            &resolved.data_dir,
                            &canonical_artifact_id,
                            &reason,
                        );
                        return Err(reason);
                    };
                    let materialized_sha256 = match materialize_worker_directory_candidate_runtime(
                        &resolved.workspace_root,
                        source_worktree_path,
                        directory_manifest,
                        &record.target_artifact_path,
                        !replaced_artifact_ids.is_empty(),
                    ) {
                        Ok(sha256) => sha256,
                        Err(reason) => {
                            let _ = canonical_artifacts::record_materialization_blocked(
                                &resolved.data_dir,
                                &canonical_artifact_id,
                                &reason,
                            );
                            return Err(reason);
                        }
                    };
                    if materialized_sha256 != candidate.sha256 {
                        let reason = format!(
                            "{}: materialized directory manifest checksum does not match source checksum",
                            candidate.source_ref
                        );
                        let _ = canonical_artifacts::record_materialization_blocked(
                            &resolved.data_dir,
                            &canonical_artifact_id,
                            &reason,
                        );
                        return Err(reason);
                    }
                    record_canonical_artifact_materialized_directory(
                        resolved,
                        &canonical_artifact_id,
                        &materialized_sha256,
                    )?;
                    retire_replaced_canonical_artifacts_after_materialization(
                        resolved,
                        record,
                        &replaced_artifact_ids,
                        &canonical_artifact_id,
                    )?;
                    let manifest_ref = write_main_agent_stage_artifact_adoption_manifest(
                        resolved,
                        job,
                        contract,
                        record,
                        entry,
                        &candidate.source_ref,
                        &candidate.content,
                    )?;
                    return Ok(manifest_ref);
                }
                let target = resolved.workspace_root.join(&record.target_artifact_path);
                if let Some(parent) = target.parent() {
                    if let Err(err) = std::fs::create_dir_all(parent) {
                        let reason = format!(
                            "failed to create canonical project file parent directory `{}`: {err}",
                            parent.display()
                        );
                        let _ = canonical_artifacts::record_materialization_blocked(
                            &resolved.data_dir,
                            &canonical_artifact_id,
                            &reason,
                        );
                        return Err(reason);
                    }
                }
                if let Err(err) = std::fs::write(&target, &candidate.bytes) {
                    let reason = format!(
                        "failed to write canonical project file `{}`: {err}",
                        target.display()
                    );
                    let _ = canonical_artifacts::record_materialization_blocked(
                        &resolved.data_dir,
                        &canonical_artifact_id,
                        &reason,
                    );
                    return Err(reason);
                }
                let written_bytes = match std::fs::read(&target) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        let reason = format!(
                            "failed to read materialized canonical project file `{}`: {err}",
                            target.display()
                        );
                        let _ = canonical_artifacts::record_materialization_blocked(
                            &resolved.data_dir,
                            &canonical_artifact_id,
                            &reason,
                        );
                        return Err(reason);
                    }
                };
                if sha256_hex(&written_bytes) != candidate.sha256 {
                    let reason = format!(
                        "{}: copied bytes do not match source checksum",
                        candidate.source_ref
                    );
                    let _ = canonical_artifacts::record_materialization_blocked(
                        &resolved.data_dir,
                        &canonical_artifact_id,
                        &reason,
                    );
                    return Err(reason);
                }
                record_canonical_artifact_materialized_bytes(
                    resolved,
                    &canonical_artifact_id,
                    &written_bytes,
                )?;
                retire_replaced_canonical_artifacts_after_materialization(
                    resolved,
                    record,
                    &replaced_artifact_ids,
                    &canonical_artifact_id,
                )?;
                let manifest_ref = write_main_agent_stage_artifact_adoption_manifest(
                    resolved,
                    job,
                    contract,
                    record,
                    entry,
                    &candidate.source_ref,
                    &String::from_utf8_lossy(&written_bytes),
                )?;
                return Ok(manifest_ref);
            }
        }
    }
    let reason = format!(
        "no source candidate passed stage artifact validation: {}",
        rejection_reasons.join("; ")
    );
    let _ = canonical_artifacts::record_materialization_blocked(
        &resolved.data_dir,
        &canonical_artifact_id,
        &reason,
    );
    Err(reason)
}

pub(crate) fn try_rebind_visible_stage_artifact_snapshot(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Result<Option<String>, String> {
    let ledger = canonical_artifacts::load_ledger(&resolved.data_dir)
        .map_err(|err| format!("canonical artifact ledger could not be loaded: {err}"))?;
    let Some(existing) = ledger.entries.iter().find(|artifact| {
        record
            .replacement_of_artifact_ids
            .iter()
            .any(|artifact_id| artifact_id == &artifact.artifact_id)
            && artifact.target_artifact_path == record.target_artifact_path
            && artifact.source_agent_id == record.source_agent_id
            && artifact.source_task_id == record.source_task_id
            && artifact.source_ref == record.source_ref
            && artifact.status.satisfies_file_dependency()
    }) else {
        return Ok(None);
    };
    let target_path = resolved.workspace_root.join(&record.target_artifact_path);
    let target_content = std::fs::read_to_string(&target_path).map_err(|err| {
        format!(
            "snapshot rebind target `{}` could not be read: {err}",
            target_path.display()
        )
    })?;
    let target_sha256 = sha256_hex(target_content.as_bytes());
    if existing.target_sha256.as_deref() != Some(target_sha256.as_str()) {
        return Err(format!(
            "snapshot rebind target `{}` no longer matches canonical artifact `{}`",
            record.target_artifact_path, existing.artifact_id
        ));
    }
    let target_review_sha256 =
        sha256_hex(autonomous_research_stage_artifact_review_body(&target_content).as_bytes());
    let candidates = collect_main_agent_stage_artifact_adoption_source_candidates(
        resolved, contract, record, entry,
    )?;
    for candidate in candidates {
        if candidate.kind != MainAgentStageArtifactAdoptionSourceCandidateKind::File
            || !autonomous_research_stage_artifact_candidate_lifecycle_violations(
                &candidate.content,
                contract,
            )
            .is_empty()
            || !autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                &candidate.content,
                contract,
                candidate.relative_path.as_deref(),
            )
            || !autonomous_research_stage_artifact_evidence_binding_failures(
                &candidate.content,
                contract,
                Some(index),
            )
            .is_empty()
        {
            continue;
        }
        let candidate_review_sha256 = sha256_hex(
            autonomous_research_stage_artifact_review_body(&candidate.content).as_bytes(),
        );
        if candidate_review_sha256 != target_review_sha256 {
            continue;
        }
        canonical_artifacts::record_adoption_snapshot_rebound(
            &resolved.data_dir,
            &existing.artifact_id,
            &record.adoption_ref,
        )
        .map_err(|err| err.to_string())?;
        return write_main_agent_stage_artifact_adoption_manifest(
            resolved,
            job,
            contract,
            record,
            entry,
            &candidate.source_ref,
            &target_content,
        )
        .map(Some);
    }
    Err(format!(
        "snapshot rebind source no longer matches visible canonical artifact `{}`; accept a distinct synthesis revision before replacement",
        existing.artifact_id
    ))
}

pub(crate) fn preflight_main_agent_stage_artifact_adoption(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> Result<MainAgentStageArtifactAdoptionPreflight, String> {
    if record.stage_id != contract.stage_id {
        return Err(format!(
            "adoption stage `{}` does not match active stage `{}`",
            record.stage_id, contract.stage_id
        ));
    }
    if Some(record.stage_execution_id.as_str()) != job.stage_execution_id.as_deref() {
        return Err(format!(
            "adoption stage execution `{}` does not match active stage execution `{:?}`",
            record.stage_execution_id, job.stage_execution_id
        ));
    }
    let Some(target_kind) =
        autonomous_research_stage_adoption_target_kind(resolved, job, contract, record)
    else {
        return Err(format!(
            "target artifact `{}` is not an adoptable active-stage artifact for `{}`",
            record.target_artifact_path, contract.stage_id
        ));
    };
    if record.rationale.trim().is_empty() {
        return Err("main-agent adoption rationale is empty".to_string());
    }
    let Some(index) = index else {
        return Err("accepted worker evidence index is missing".to_string());
    };
    let (accepted_evidence_revision_refs, evidence_snapshot_hash) =
        accepted_worker_evidence_snapshot_for_contract(contract, index)?;
    if let Some(recorded_snapshot_hash) = record.evidence_snapshot_hash.as_deref() {
        if recorded_snapshot_hash != evidence_snapshot_hash {
            return Err(format!(
                "adoption evidence snapshot changed: recorded `{recorded_snapshot_hash}`, current `{evidence_snapshot_hash}`. Record a new main-agent adoption decision for the current accepted evidence snapshot."
            ));
        }
    }
    if !record.accepted_evidence_revision_refs.is_empty() {
        let mut recorded_revision_refs = record.accepted_evidence_revision_refs.clone();
        recorded_revision_refs.sort();
        let mut current_revision_refs = accepted_evidence_revision_refs.clone();
        current_revision_refs.sort();
        if recorded_revision_refs != current_revision_refs {
            return Err(
                "adoption accepted evidence revisions no longer match the current evidence snapshot"
                    .to_string(),
            );
        }
    }
    let Some(entry) = index
        .entries
        .iter()
        .find(|entry| accepted_worker_evidence_entry_supports_adoption(entry, record))
    else {
        return Err(format!(
            "source ref `{}` was not found in accepted worker evidence for agent `{}`",
            record.source_ref, record.source_agent_id
        ));
    };
    if accepted_worker_evidence_entry_has_main_agent_decision(entry)
        && entry
            .main_agent_decision_ref
            .as_ref()
            .is_none_or(|decision_ref| !accepted_evidence_revision_refs.contains(decision_ref))
    {
        return Err(format!(
            "source task `{}` is accepted but is not the explicitly selected current evidence revision for its evidence set",
            entry.task_id
        ));
    }
    validate_adoption_record_source_ref_agent_binding(resolved, record, entry)?;
    if target_kind == MainAgentStageArtifactAdoptionTargetKind::StageArtifact
        && !accepted_worker_evidence_entry_is_adoptable_stage_synthesis(contract, entry)
    {
        return Err(format!(
            "stage artifact adoption for `{}` must use a main-agent-accepted research_synthesizer stage artifact synthesis candidate; source task `{}` has task_type `{}`, worker_role `{}`, required_output `{}`, active_status `{}`, main_agent_decision_ref `{}`",
            contract.artifact_path,
            entry.task_id,
            entry.task_type,
            entry.worker_role,
            entry.required_output_artifact_type,
            entry.active_status.as_deref().unwrap_or("candidate"),
            entry.main_agent_decision_ref.as_deref().unwrap_or("none")
        ));
    }
    if target_kind == MainAgentStageArtifactAdoptionTargetKind::StageArtifact {
        validate_stage_synthesis_evidence_freshness(resolved, index, entry)?;
    }
    validate_accepted_worker_evidence_for_adoption_target(contract, index, entry, target_kind)?;
    let candidates = collect_main_agent_stage_artifact_adoption_source_candidates(
        resolved, contract, record, entry,
    )?;
    if candidates.is_empty() {
        return Err(no_readable_source_candidate_reason(
            resolved, contract, record, entry,
        ));
    }
    let mut resolved_source_refs = Vec::new();
    let mut candidate_relative_paths = Vec::new();
    for candidate in &candidates {
        merge_unique_strings(
            &mut resolved_source_refs,
            vec![candidate.source_ref.clone()],
        );
        if let Some(relative_path) = candidate.relative_path.as_ref() {
            merge_unique_strings(&mut candidate_relative_paths, vec![relative_path.clone()]);
        }
    }
    if target_kind == MainAgentStageArtifactAdoptionTargetKind::StageArtifact {
        let mut candidate_validation_failures = Vec::new();
        let has_valid_candidate = candidates.iter().any(|candidate| {
            if candidate.kind != MainAgentStageArtifactAdoptionSourceCandidateKind::File {
                return false;
            }
            if !autonomous_research_stage_artifact_candidate_lifecycle_violations(
                &candidate.content,
                contract,
            )
            .is_empty()
            {
                return false;
            }
            if !autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                &candidate.content,
                contract,
                candidate.relative_path.as_deref(),
            ) {
                return false;
            }
            let evidence_binding_failures =
                autonomous_research_stage_artifact_evidence_binding_failures(
                    &candidate.content,
                    contract,
                    Some(index),
                );
            if !evidence_binding_failures.is_empty() {
                candidate_validation_failures.push(format!(
                    "{}: {}",
                    candidate.source_ref,
                    evidence_binding_failures.join("; ")
                ));
                return false;
            }
            true
        });
        if !has_valid_candidate && !candidate_validation_failures.is_empty() {
            return Err(format!(
                "no source candidate passed stage artifact evidence binding validation: {}",
                candidate_validation_failures.join("; ")
            ));
        }
    }
    Ok(MainAgentStageArtifactAdoptionPreflight {
        source_agent_id: record.source_agent_id.clone(),
        source_task_id: record.source_task_id.clone(),
        source_ref: record.source_ref.clone(),
        target_artifact_path: record.target_artifact_path.clone(),
        target_kind: match target_kind {
            MainAgentStageArtifactAdoptionTargetKind::StageArtifact => "stage_artifact",
            MainAgentStageArtifactAdoptionTargetKind::ProjectFile => "project_file",
        }
        .to_string(),
        resolved_source_refs,
        candidate_relative_paths,
        evidence_snapshot_hash,
        accepted_evidence_revision_refs,
    })
}

fn validate_stage_synthesis_evidence_freshness(
    resolved: &ResolvedProject,
    index: &AutonomousResearchAcceptedWorkerEvidenceIndex,
    synthesis_entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Result<(), String> {
    let task_packet_path =
        resolve_adoption_reference_path(resolved, &synthesis_entry.task_packet_ref);
    let Ok(task_packet) = read_json_file_runtime(&task_packet_path) else {
        return Ok(());
    };
    let Some(dispatched_at) = task_packet
        .get("created_at")
        .and_then(|value| value.as_str())
        .filter(|value| value.parse::<u128>().is_ok())
    else {
        return Ok(());
    };
    let Some(newer_entry) = accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .filter(|entry| {
            entry
                .main_agent_decision_ref
                .as_deref()
                .is_some_and(|decision_ref| !decision_ref.trim().is_empty())
        })
        .filter(|entry| {
            !crate::accepted_worker_evidence::task_type_is_stage_synthesis(&entry.task_type)
        })
        .filter(|entry| {
            entry
                .created_at
                .as_deref()
                .filter(|created_at| created_at.parse::<u128>().is_ok())
                .is_some_and(|created_at| timestamp_string_is_after(created_at, dispatched_at))
        })
        .min_by(|left, right| left.created_at.cmp(&right.created_at))
    else {
        return Ok(());
    };
    Err(format!(
        "stage synthesis candidate is stale: active evidence task `{}` was accepted after synthesis worker dispatch. Update the canonical synthesis board task and dispatch a fresh worker before adoption.",
        newer_entry.task_id
    ))
}

pub(crate) fn preflight_main_agent_stage_artifact_adoption_from_args(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    stage_id: &str,
    stage_execution_id: &str,
    source_agent_id: &str,
    source_task_id: Option<&str>,
    source_ref: &str,
    source_artifact_path: Option<&str>,
    target_artifact_path: &str,
    rationale: &str,
    evidence_refs: &[String],
    replacement_of_artifact_ids: &[String],
) -> Result<MainAgentStageArtifactAdoptionPreflight, String> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let (accepted_evidence_revision_refs, evidence_snapshot_hash) = index
        .as_ref()
        .ok_or_else(|| "accepted worker evidence index is missing".to_string())
        .and_then(|index| accepted_worker_evidence_snapshot_for_contract(&contract, index))?;
    let record = MainAgentStageArtifactAdoptionRecord {
        adoption_ref: "main_agent_stage_artifact_adoption::preflight".to_string(),
        stage_id: stage_id.to_string(),
        stage_execution_id: stage_execution_id.to_string(),
        source_agent_id: source_agent_id.to_string(),
        source_task_id: source_task_id.map(ToString::to_string),
        source_ref: source_ref.to_string(),
        source_artifact_path: source_artifact_path.map(ToString::to_string),
        target_artifact_path: target_artifact_path.to_string(),
        rationale: rationale.to_string(),
        evidence_refs: evidence_refs.to_vec(),
        evidence_snapshot_hash: Some(evidence_snapshot_hash),
        accepted_evidence_revision_refs,
        cleanup_required: false,
        request_review_rerun: false,
        replacement_of_artifact_ids: replacement_of_artifact_ids.to_vec(),
        created_at: timestamp_string(),
    };
    preflight_main_agent_stage_artifact_adoption(resolved, job, &contract, index.as_ref(), &record)
}

pub(crate) fn validate_adoption_record_source_ref_agent_binding(
    resolved: &ResolvedProject,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Result<(), String> {
    if let Some(agent_id) = agent_id_from_adoption_reference(resolved, &record.source_ref) {
        if agent_id != record.source_agent_id || agent_id != entry.agent_id {
            return Err(format!(
                "source_ref/source_agent_id mismatch: source_agent_id `{}` task `{}` cannot adopt source_ref `{}` because that ref belongs to agent `{}`. Use a source_ref under `.pmcli/agents/{}/` from the accepted worker evidence, usually `.pmcli/agents/{}/worktree_artifact_candidates.json`, and use a source_artifact_path from that manifest.",
                record.source_agent_id,
                record
                    .source_task_id
                    .as_deref()
                    .unwrap_or(entry.task_id.as_str()),
                record.source_ref,
                agent_id,
                record.source_agent_id,
                record.source_agent_id
            ));
        }
    }
    if let Some(source_artifact_path) = record.source_artifact_path.as_ref() {
        if !stage_artifact_adoption_candidate_path_is_safe(source_artifact_path) {
            return Err(format!(
                "source_artifact_path `{source_artifact_path}` is not a safe worker candidate relative path"
            ));
        }
    }
    Ok(())
}

pub(crate) fn agent_id_from_adoption_reference(
    resolved: &ResolvedProject,
    reference: &str,
) -> Option<String> {
    let path = resolve_adoption_reference_path(resolved, reference);
    agent_id_from_agent_artifact_path(&path.display().to_string())
        .or_else(|| agent_id_from_agent_artifact_path(reference))
}

pub(crate) fn no_readable_source_candidate_reason(
    resolved: &ResolvedProject,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> String {
    let available = available_adoption_candidate_paths_for_entry(resolved, entry);
    let requested = record.source_artifact_path.as_deref().unwrap_or("none");
    if available.is_empty() {
        return format!(
            "no readable source candidate artifact was found for adoption for agent `{}` task `{}`; no safe worker candidate paths are listed in that worker's manifests. Use a source_ref from `.pmcli/agents/{}/worktree_artifact_candidates.json` after the worker produces a safe candidate for `{}`.",
            record.source_agent_id,
            record
                .source_task_id
                .as_deref()
                .unwrap_or(entry.task_id.as_str()),
            record.source_agent_id,
            contract.artifact_path
        );
    }
    format!(
        "no readable source candidate artifact was found for adoption for agent `{}` task `{}` with source_artifact_path `{}`; available safe candidate paths for this worker are: {}. Retry adopt_stage_artifact using one of these source_artifact_path values and a source_ref from the same worker.",
        record.source_agent_id,
        record
            .source_task_id
            .as_deref()
            .unwrap_or(entry.task_id.as_str()),
        requested,
        comma_or_none_runtime(&available)
    )
}

pub(crate) fn available_adoption_candidate_paths_for_entry(
    resolved: &ResolvedProject,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Vec<String> {
    let mut paths = Vec::new();
    for reference in accepted_worker_evidence_entry_refs(entry) {
        let path = resolve_adoption_reference_path(resolved, &reference);
        if path.file_name().and_then(|value| value.to_str()) == Some("output_manifest.json") {
            for manifest_ref in output_manifest_worktree_candidate_refs(&path).unwrap_or_default() {
                merge_unique_strings(
                    &mut paths,
                    available_adoption_candidate_paths_from_manifest(
                        entry,
                        &PathBuf::from(manifest_ref),
                    ),
                );
            }
            continue;
        }
        if path.file_name().and_then(|value| value.to_str())
            == Some("worktree_artifact_candidates.json")
        {
            merge_unique_strings(
                &mut paths,
                available_adoption_candidate_paths_from_manifest(entry, &path),
            );
        }
    }
    paths.sort();
    paths
}

pub(crate) fn available_adoption_candidate_paths_from_manifest(
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    manifest_path: &Path,
) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(manifest_path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    if value.get("agent_id").and_then(|value| value.as_str()) != Some(entry.agent_id.as_str()) {
        return Vec::new();
    }
    if value
        .get("authority_scope")
        .and_then(|value| value.as_str())
        != Some("worker_evidence_only")
    {
        return Vec::new();
    }
    let mut allowed_paths = json_string_array_runtime(&value, "changed_paths");
    merge_unique_strings(
        &mut allowed_paths,
        json_string_array_runtime(&value, "untracked_paths"),
    );
    let candidate_entries = value
        .get("candidate_entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    allowed_paths
        .into_iter()
        .filter(|path| stage_artifact_adoption_candidate_path_is_safe(path))
        .filter(|path| {
            candidate_entries
                .iter()
                .find(|entry| {
                    entry.get("relative_path").and_then(|value| value.as_str())
                        == Some(path.as_str())
                })
                .map(|entry| {
                    entry
                        .get("safe_status")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        == "safe"
                })
                .unwrap_or(true)
        })
        .collect()
}

pub(crate) fn collect_main_agent_stage_artifact_adoption_source_candidates(
    resolved: &ResolvedProject,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
) -> Result<Vec<MainAgentStageArtifactAdoptionSourceCandidate>, String> {
    let mut refs = accepted_worker_evidence_entry_refs(entry);
    merge_unique_strings(&mut refs, vec![record.source_ref.clone()]);
    merge_unique_strings(&mut refs, record.evidence_refs.clone());
    let mut manifest_refs = Vec::new();
    let mut direct_refs = Vec::new();
    for reference in &refs {
        let path = resolve_adoption_reference_path(resolved, reference);
        if path.file_name().and_then(|value| value.to_str())
            == Some("worktree_artifact_candidates.json")
            && path.exists()
        {
            merge_unique_strings(&mut manifest_refs, vec![path.display().to_string()]);
            continue;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some("output_manifest.json")
            && path.exists()
        {
            merge_unique_strings(
                &mut manifest_refs,
                output_manifest_worktree_candidate_refs(&path)?,
            );
            continue;
        }
        if record.source_artifact_path.is_none()
            && path.exists()
            && path.is_file()
            && path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            merge_unique_strings(&mut direct_refs, vec![path.display().to_string()]);
        }
    }
    let mut candidates = Vec::new();
    for manifest_ref in manifest_refs {
        let manifest_path = PathBuf::from(&manifest_ref);
        candidates.extend(candidate_contents_from_worktree_manifest(
            contract,
            record,
            entry,
            &manifest_path,
        )?);
    }
    for direct_ref in direct_refs {
        let direct = PathBuf::from(direct_ref);
        if let Ok(bytes) = std::fs::read(&direct) {
            let content = String::from_utf8(bytes.clone()).map_err(|_| {
                format!(
                    "direct candidate {} is not UTF-8 text and cannot be adopted in the first implementation scope",
                    direct.display()
                )
            })?;
            candidates.push(MainAgentStageArtifactAdoptionSourceCandidate {
                source_ref: relative_display_ref(&direct.display().to_string()),
                relative_path: None,
                sha256: sha256_hex(&bytes),
                bytes,
                content,
                kind: MainAgentStageArtifactAdoptionSourceCandidateKind::File,
                directory_manifest: None,
                source_worktree_path: None,
            });
        }
    }
    Ok(candidates)
}

pub(crate) fn output_manifest_worktree_candidate_refs(path: &Path) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(|err| err.to_string())?;
    let mut refs = Vec::new();
    if let Some(items) = value.get("output_refs").and_then(|value| value.as_array()) {
        for item in items {
            let kind = item
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let reference = item
                .get("ref")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if kind.contains("worktree_artifact_candidates") && !reference.trim().is_empty() {
                merge_unique_strings(&mut refs, vec![reference.to_string()]);
            }
        }
    }
    Ok(refs)
}

pub(crate) fn candidate_contents_from_worktree_manifest(
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    manifest_path: &Path,
) -> Result<Vec<MainAgentStageArtifactAdoptionSourceCandidate>, String> {
    let content = std::fs::read_to_string(manifest_path).map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&content).map_err(|err| err.to_string())?;
    if value.get("agent_id").and_then(|value| value.as_str()) != Some(entry.agent_id.as_str()) {
        return Ok(Vec::new());
    }
    if value
        .get("authority_scope")
        .and_then(|value| value.as_str())
        != Some("worker_evidence_only")
    {
        return Ok(Vec::new());
    }
    let Some(worktree_path) = value.get("worktree_path").and_then(|value| value.as_str()) else {
        return Ok(Vec::new());
    };
    let mut allowed_paths = json_string_array_runtime(&value, "changed_paths");
    merge_unique_strings(
        &mut allowed_paths,
        json_string_array_runtime(&value, "untracked_paths"),
    );
    let candidate_entries = value
        .get("candidate_entries")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let requested_paths = requested_main_agent_stage_artifact_adoption_candidate_paths(
        contract,
        record,
        &allowed_paths,
    );
    let mut candidates = Vec::new();
    for requested in requested_paths {
        if !stage_artifact_adoption_candidate_path_is_safe(&requested) {
            continue;
        }
        if !allowed_paths.iter().any(|allowed| allowed == &requested) {
            continue;
        }
        let matching_entry = candidate_entries.iter().find(|entry| {
            entry.get("relative_path").and_then(|value| value.as_str()) == Some(requested.as_str())
        });
        if let Some(candidate_entry) = matching_entry {
            let safe_status = candidate_entry
                .get("safe_status")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if safe_status != "safe" {
                return Err(format!(
                    "{}::{} is not a safe canonical artifact candidate",
                    relative_display_ref(&manifest_path.display().to_string()),
                    requested
                ));
            }
            if candidate_entry
                .get("is_directory")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                let directory_manifest_ref = candidate_entry
                    .get("directory_manifest_ref")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        format!(
                            "{}::{} is a directory candidate without directory_manifest_ref",
                            relative_display_ref(&manifest_path.display().to_string()),
                            requested
                        )
                    })?;
                let directory_manifest_path = PathBuf::from(directory_manifest_ref);
                let directory_manifest = read_worker_directory_candidate_manifest_runtime(
                    &directory_manifest_path,
                    &requested,
                )?;
                validate_worker_directory_candidate_manifest_runtime(
                    Path::new(worktree_path),
                    &directory_manifest,
                )?;
                let directory_sha256 =
                    sha256_worker_directory_candidate_manifest_runtime(&directory_manifest);
                candidates.push(MainAgentStageArtifactAdoptionSourceCandidate {
                    source_ref: format!(
                        "{}::{}",
                        relative_display_ref(&manifest_path.display().to_string()),
                        requested
                    ),
                    relative_path: Some(requested.clone()),
                    sha256: directory_sha256,
                    bytes: Vec::new(),
                    content: serde_json::to_string_pretty(&serde_json::json!({
                        "directory_manifest_ref": directory_manifest_ref,
                        "root_relative_path": directory_manifest.root_relative_path,
                        "file_count": directory_manifest.entries.len(),
                    }))
                    .map_err(|err| err.to_string())?,
                    kind: MainAgentStageArtifactAdoptionSourceCandidateKind::Directory,
                    directory_manifest: Some(directory_manifest),
                    source_worktree_path: Some(PathBuf::from(worktree_path)),
                });
                continue;
            }
        }
        let expected_sha256 = matching_entry
            .and_then(|entry| entry.get("sha256"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        let source_path = matching_entry
            .and_then(|entry| entry.get("candidate_archive_ref"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(worktree_path).join(&requested));
        if !source_path.exists() || !source_path.is_file() {
            return Err(format!(
                "candidate source for {}::{} is missing or not a file: {}",
                relative_display_ref(&manifest_path.display().to_string()),
                requested,
                source_path.display()
            ));
        }
        let candidate_bytes = std::fs::read(&source_path)
            .map_err(|err| format!("failed to read candidate {}: {err}", source_path.display()))?;
        let actual_sha256 = sha256_hex(&candidate_bytes);
        if let Some(expected_sha256) = expected_sha256.as_ref() {
            if &actual_sha256 != expected_sha256 {
                return Err(format!(
                    "candidate checksum mismatch for {}::{}: expected {}, got {}",
                    relative_display_ref(&manifest_path.display().to_string()),
                    requested,
                    expected_sha256,
                    actual_sha256
                ));
            }
        }
        let candidate_content = String::from_utf8(candidate_bytes.clone()).map_err(|_| {
            format!(
                "candidate {} is not UTF-8 text and cannot be adopted in the first implementation scope",
                source_path.display()
            )
        })?;
        candidates.push(MainAgentStageArtifactAdoptionSourceCandidate {
            source_ref: format!(
                "{}::{}",
                relative_display_ref(&manifest_path.display().to_string()),
                requested
            ),
            relative_path: Some(requested),
            sha256: actual_sha256,
            bytes: candidate_bytes,
            content: candidate_content,
            kind: MainAgentStageArtifactAdoptionSourceCandidateKind::File,
            directory_manifest: None,
            source_worktree_path: Some(PathBuf::from(worktree_path)),
        });
    }
    Ok(candidates)
}

pub(crate) fn requested_main_agent_stage_artifact_adoption_candidate_paths(
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    allowed_paths: &[String],
) -> Vec<String> {
    let mut requested = Vec::new();
    if let Some(path) = record.source_artifact_path.as_ref() {
        merge_unique_strings(&mut requested, vec![path.clone()]);
    }
    if adoption_reference_can_name_candidate_file(&record.source_ref) {
        merge_unique_strings(&mut requested, vec![record.source_ref.clone()]);
    }
    for reference in &record.evidence_refs {
        if adoption_reference_can_name_candidate_file(reference) {
            merge_unique_strings(&mut requested, vec![reference.clone()]);
        }
    }
    if requested.is_empty() {
        let target_name = Path::new(&contract.artifact_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        for candidate in allowed_paths {
            if Path::new(candidate)
                .file_name()
                .and_then(|value| value.to_str())
                == Some(target_name)
            {
                merge_unique_strings(&mut requested, vec![candidate.clone()]);
            }
        }
    }
    expand_stage_artifact_adoption_requested_paths(requested, allowed_paths)
}

pub(crate) fn adoption_reference_can_name_candidate_file(reference: &str) -> bool {
    let path = Path::new(reference);
    if path.file_name().and_then(|value| value.to_str())
        == Some("worktree_artifact_candidates.json")
    {
        return false;
    }
    if path.file_name().and_then(|value| value.to_str()) == Some("output_manifest.json") {
        return false;
    }
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension != "json")
}

pub(crate) fn expand_stage_artifact_adoption_requested_paths(
    requested_paths: Vec<String>,
    allowed_paths: &[String],
) -> Vec<String> {
    let safe_allowed_paths = allowed_paths
        .iter()
        .filter(|path| stage_artifact_adoption_candidate_path_is_safe(path))
        .cloned()
        .collect::<Vec<_>>();
    let mut expanded = Vec::new();
    for requested in requested_paths {
        if !stage_artifact_adoption_candidate_path_is_safe(&requested) {
            continue;
        }
        if safe_allowed_paths
            .iter()
            .any(|allowed| allowed == &requested)
        {
            merge_unique_strings(&mut expanded, vec![requested.clone()]);
            continue;
        }
        let suffix_matches = safe_allowed_paths
            .iter()
            .filter(|allowed| path_has_relative_suffix(allowed, &requested))
            .cloned()
            .collect::<Vec<_>>();
        if suffix_matches.len() == 1 {
            merge_unique_strings(&mut expanded, suffix_matches);
            continue;
        }
        let Some(requested_name) = Path::new(&requested)
            .file_name()
            .and_then(|value| value.to_str())
        else {
            continue;
        };
        let name_matches = safe_allowed_paths
            .iter()
            .filter(|allowed| {
                Path::new(allowed)
                    .file_name()
                    .and_then(|value| value.to_str())
                    == Some(requested_name)
            })
            .cloned()
            .collect::<Vec<_>>();
        if name_matches.len() == 1 {
            merge_unique_strings(&mut expanded, name_matches);
        }
    }
    expanded
}

pub(crate) fn path_has_relative_suffix(path: &str, suffix: &str) -> bool {
    path == suffix
        || path
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

pub(crate) fn stage_artifact_adoption_candidate_path_is_safe(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
}

pub(crate) fn read_worker_directory_candidate_manifest_runtime(
    manifest_path: &Path,
    requested_root: &str,
) -> Result<WorkerDirectoryCandidateManifestRuntime, String> {
    let content = std::fs::read_to_string(manifest_path).map_err(|err| {
        format!(
            "failed to read directory candidate manifest `{}`: {err}",
            manifest_path.display()
        )
    })?;
    let manifest: WorkerDirectoryCandidateManifestRuntime = serde_json::from_str(&content)
        .map_err(|err| {
            format!(
                "failed to parse directory candidate manifest `{}`: {err}",
                manifest_path.display()
            )
        })?;
    if manifest.root_relative_path != requested_root {
        return Err(format!(
            "directory candidate manifest root `{}` does not match requested `{}`",
            manifest.root_relative_path, requested_root
        ));
    }
    Ok(manifest)
}

pub(crate) fn validate_worker_directory_candidate_manifest_runtime(
    worktree_path: &Path,
    manifest: &WorkerDirectoryCandidateManifestRuntime,
) -> Result<(), String> {
    if !stage_artifact_adoption_candidate_path_is_safe(&manifest.root_relative_path) {
        return Err(format!(
            "directory candidate root `{}` is not a safe relative path",
            manifest.root_relative_path
        ));
    }
    if manifest.entries.is_empty() {
        return Err(format!(
            "directory candidate `{}` has an empty manifest",
            manifest.root_relative_path
        ));
    }
    for entry in &manifest.entries {
        if !stage_artifact_adoption_candidate_path_is_safe(&entry.relative_path) {
            return Err(format!(
                "directory candidate entry `{}` is not a safe relative path",
                entry.relative_path
            ));
        }
        if !path_has_relative_suffix(&entry.relative_path, &entry.relative_path)
            || !(entry.relative_path == manifest.root_relative_path
                || entry
                    .relative_path
                    .starts_with(&format!("{}/", manifest.root_relative_path)))
        {
            return Err(format!(
                "directory candidate entry `{}` is outside root `{}`",
                entry.relative_path, manifest.root_relative_path
            ));
        }
        let source_path = worktree_path.join(&entry.relative_path);
        if !source_path.is_file() {
            return Err(format!(
                "directory candidate entry `{}` is missing or not a file",
                entry.relative_path
            ));
        }
        let bytes = std::fs::read(&source_path).map_err(|err| {
            format!(
                "failed to read directory candidate entry `{}`: {err}",
                source_path.display()
            )
        })?;
        if bytes.len() as u64 != entry.size_bytes {
            return Err(format!(
                "directory candidate entry `{}` size mismatch: expected {}, got {}",
                entry.relative_path,
                entry.size_bytes,
                bytes.len()
            ));
        }
        let actual_sha256 = sha256_hex(&bytes);
        if actual_sha256 != entry.sha256 {
            return Err(format!(
                "directory candidate entry `{}` checksum mismatch: expected {}, got {}",
                entry.relative_path, entry.sha256, actual_sha256
            ));
        }
    }
    Ok(())
}

pub(crate) fn sha256_worker_directory_candidate_manifest_runtime(
    manifest: &WorkerDirectoryCandidateManifestRuntime,
) -> String {
    let mut rows = manifest
        .entries
        .iter()
        .map(|entry| {
            let relative = entry
                .relative_path
                .strip_prefix(&format!("{}/", manifest.root_relative_path))
                .unwrap_or(&entry.relative_path);
            format!("{}\t{}", relative, entry.sha256)
        })
        .collect::<Vec<_>>();
    rows.sort();
    sha256_hex(rows.join("\n").as_bytes())
}

pub(crate) fn materialize_worker_directory_candidate_runtime(
    workspace_root: &Path,
    worktree_path: &Path,
    manifest: &WorkerDirectoryCandidateManifestRuntime,
    target_artifact_path: &str,
    allow_existing_target: bool,
) -> Result<String, String> {
    if !stage_artifact_adoption_candidate_path_is_safe(target_artifact_path) {
        return Err(format!(
            "target directory path `{target_artifact_path}` is not safe"
        ));
    }
    let target_root = workspace_root.join(target_artifact_path);
    if target_root.exists() && !allow_existing_target {
        return if target_root.is_dir() {
            Err(format!(
                "target directory `{}` already exists; main agent must explicitly retire or replace the current canonical artifact before materializing a directory candidate",
                target_root.display()
            ))
        } else {
            Err(format!(
                "target path `{}` already exists and is not a directory; main agent must explicitly retire or replace the current canonical artifact before materializing a directory candidate",
                target_root.display()
            ))
        };
    }
    let target_parent = target_root
        .parent()
        .ok_or_else(|| format!("target directory `{}` has no parent", target_root.display()))?;
    std::fs::create_dir_all(target_parent).map_err(|err| {
        format!(
            "failed to create canonical project directory parent `{}`: {err}",
            target_parent.display()
        )
    })?;
    let staging_root = target_parent.join(format!(
        ".astra-directory-materialize-{}",
        timestamp_string()
    ));
    if staging_root.exists() {
        std::fs::remove_dir_all(&staging_root).map_err(|err| {
            format!(
                "failed to clear stale directory materialization staging path `{}`: {err}",
                staging_root.display()
            )
        })?;
    }
    std::fs::create_dir_all(&staging_root).map_err(|err| {
        format!(
            "failed to create directory materialization staging path `{}`: {err}",
            staging_root.display()
        )
    })?;
    let materialize_result = materialize_worker_directory_candidate_into_staging_runtime(
        worktree_path,
        manifest,
        &staging_root,
    );
    if let Err(reason) = materialize_result {
        let _ = std::fs::remove_dir_all(&staging_root);
        return Err(reason);
    }
    if target_root.exists() {
        let backup_root = target_parent.join(format!(
            ".astra-directory-replace-backup-{}",
            timestamp_string()
        ));
        if backup_root.exists() {
            let cleanup_result = if backup_root.is_dir() {
                std::fs::remove_dir_all(&backup_root)
            } else {
                std::fs::remove_file(&backup_root)
            };
            cleanup_result.map_err(|err| {
                format!(
                    "failed to clear stale directory replacement backup `{}`: {err}",
                    backup_root.display()
                )
            })?;
        }
        std::fs::rename(&target_root, &backup_root).map_err(|err| {
            let _ = std::fs::remove_dir_all(&staging_root);
            format!(
                "failed to move existing canonical target `{}` to replacement backup `{}`: {err}",
                target_root.display(),
                backup_root.display()
            )
        })?;
        if let Err(err) = std::fs::rename(&staging_root, &target_root) {
            let _ = std::fs::rename(&backup_root, &target_root);
            let _ = std::fs::remove_dir_all(&staging_root);
            return Err(format!(
                "failed to promote directory materialization staging path `{}` to canonical project directory `{}`: {err}",
                staging_root.display(),
                target_root.display()
            ));
        }
        let _ = if backup_root.is_dir() {
            std::fs::remove_dir_all(&backup_root)
        } else {
            std::fs::remove_file(&backup_root)
        };
    } else {
        std::fs::rename(&staging_root, &target_root).map_err(|err| {
            let _ = std::fs::remove_dir_all(&staging_root);
            format!(
                "failed to promote directory materialization staging path `{}` to canonical project directory `{}`: {err}",
                staging_root.display(),
                target_root.display()
            )
        })?;
    }
    Ok(sha256_worker_directory_candidate_manifest_runtime(manifest))
}

pub(crate) fn materialize_worker_directory_candidate_into_staging_runtime(
    worktree_path: &Path,
    manifest: &WorkerDirectoryCandidateManifestRuntime,
    staging_root: &Path,
) -> Result<(), String> {
    for entry in &manifest.entries {
        let relative_inside_root = entry
            .relative_path
            .strip_prefix(&format!("{}/", manifest.root_relative_path))
            .ok_or_else(|| {
                format!(
                    "directory candidate entry `{}` is outside root `{}`",
                    entry.relative_path, manifest.root_relative_path
                )
            })?;
        if !stage_artifact_adoption_candidate_path_is_safe(relative_inside_root) {
            return Err(format!(
                "directory candidate entry relative target `{relative_inside_root}` is not safe"
            ));
        }
        let source_path = worktree_path.join(&entry.relative_path);
        let target_path = staging_root.join(relative_inside_root);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to create directory materialization staging entry parent `{}`: {err}",
                    parent.display()
                )
            })?;
        }
        std::fs::copy(&source_path, &target_path).map_err(|err| {
            format!(
                "failed to copy directory candidate entry `{}` to staging path `{}`: {err}",
                source_path.display(),
                target_path.display()
            )
        })?;
        let copied = std::fs::read(&target_path).map_err(|err| {
            format!(
                "failed to read materialized staging directory entry `{}`: {err}",
                target_path.display()
            )
        })?;
        let copied_sha256 = sha256_hex(&copied);
        if copied_sha256 != entry.sha256 {
            return Err(format!(
                "materialized staging directory entry `{}` checksum mismatch: expected {}, got {}",
                target_path.display(),
                entry.sha256,
                copied_sha256
            ));
        }
    }
    Ok(())
}

pub(crate) fn resolve_adoption_reference_path(
    resolved: &ResolvedProject,
    reference: &str,
) -> PathBuf {
    if let Some(path) =
        crate::review_refs::review_artifact_path_in_data_dir(&resolved.data_dir, reference)
    {
        return path;
    }
    let path = Path::new(reference);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolved.workspace_root.join(path)
    }
}

fn normalize_review_artifact_refs_runtime(
    resolved: &ResolvedProject,
    refs: Vec<String>,
) -> Vec<String> {
    refs.into_iter()
        .map(|reference| {
            crate::review_refs::canonical_review_artifact_ref_in_data_dir(
                &resolved.data_dir,
                &reference,
            )
            .unwrap_or(reference)
        })
        .collect()
}

pub(crate) fn main_agent_stage_artifact_adoption_manifest_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> PathBuf {
    resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(&job.job_id)
        .join(&record.stage_execution_id)
        .join("stage_artifact_adoptions")
        .join(format!(
            "{}.json",
            sanitize_runtime_path_component(&record.adoption_ref)
        ))
}

pub(crate) fn main_agent_stage_artifact_adoption_rejection_manifest_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> PathBuf {
    resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(&job.job_id)
        .join(&record.stage_execution_id)
        .join("stage_artifact_adoption_rejections")
        .join(format!(
            "{}.json",
            sanitize_runtime_path_component(&record.adoption_ref)
        ))
}

pub(crate) fn main_agent_stage_artifact_adoption_has_non_retryable_rejection(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    record: &MainAgentStageArtifactAdoptionRecord,
) -> bool {
    let path = main_agent_stage_artifact_adoption_rejection_manifest_path(resolved, job, record);
    if !path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return true;
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return true;
    };
    let reason = value
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    !stage_artifact_adoption_rejection_is_retryable_after_evidence_refresh(reason)
}

pub(crate) fn stage_artifact_adoption_rejection_is_retryable_after_evidence_refresh(
    reason: &str,
) -> bool {
    let lowered = reason.to_ascii_lowercase();
    lowered.contains("accepted worker evidence index is missing")
        || lowered.contains("accepted worker evidence is missing required task types")
        || lowered.contains("accepted worker evidence quality floor failed")
        || lowered.contains("accepted worker semantic review floor failed")
        || lowered.contains("accepted worker active-tool evidence floor failed")
        || lowered.contains("source ref `")
}

pub(crate) fn write_main_agent_stage_artifact_adoption_rejection_manifest(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    reason: &str,
) -> Result<String, String> {
    let path = main_agent_stage_artifact_adoption_rejection_manifest_path(resolved, job, record);
    let manifest = json!({
        "schema_version": "autonomous_research.stage_artifact_adoption_rejection.v1",
        "adoption_ref": record.adoption_ref,
        "job_id": job.job_id,
        "stage_id": contract.stage_id,
        "stage_execution_id": record.stage_execution_id,
        "source_agent_id": record.source_agent_id,
        "source_task_id": record.source_task_id,
        "source_ref": record.source_ref,
        "source_artifact_path": record.source_artifact_path,
        "target_artifact_path": record.target_artifact_path,
        "reason": reason,
        "created_at": timestamp_string(),
        "authority_boundary": "main_agent_decides_source_and_target; runtime rejects invalid adoption and retires candidate evidence only for content-validation failures"
    });
    write_text_atomic_runtime(
        &path,
        &serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
    )?;
    Ok(relative_workspace_ref(&resolved.workspace_root, &path))
}

pub(crate) fn retire_accepted_worker_evidence_for_rejected_stage_adoption(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    reason: &str,
) -> Result<Option<String>, String> {
    if !stage_artifact_adoption_rejection_should_retire_worker_evidence(reason) {
        return Ok(None);
    }
    let path = autonomous_research_accepted_worker_evidence_index_path(
        resolved,
        job,
        &record.stage_execution_id,
    );
    let Some(mut index) = load_autonomous_research_accepted_worker_evidence_index_at_path(&path)
    else {
        return Ok(None);
    };
    let mut changed = false;
    for entry in &mut index.entries {
        if entry.required_output_artifact_type != contract.artifact_type {
            continue;
        }
        if !main_agent_stage_artifact_adoption_record_targets_entry(entry, record) {
            continue;
        }
        entry.active_status = Some("rejected".to_string());
        entry.decision_reason = Some(format!(
            "stage_artifact_adoption_rejected: {}",
            compact_single_line(reason, 600)
        ));
        if entry.main_agent_decision_ref.is_none() {
            entry.main_agent_decision_ref = Some(record.adoption_ref.clone());
        }
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    index.generated_at = timestamp_string();
    write_autonomous_research_accepted_worker_evidence_index(&path, &index)?;
    Ok(Some(relative_workspace_ref(
        &resolved.workspace_root,
        &path,
    )))
}

pub(crate) fn stage_artifact_adoption_rejection_should_retire_worker_evidence(
    reason: &str,
) -> bool {
    let lowered = reason.to_ascii_lowercase();
    lowered.contains("no source candidate passed stage artifact validation")
        || lowered.contains("candidate is not a clean canonical stage artifact body")
        || lowered.contains("candidate is not reviewable")
        || lowered.contains("candidate does not cite accepted worker evidence")
}

pub(crate) fn write_main_agent_stage_artifact_adoption_manifest(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    record: &MainAgentStageArtifactAdoptionRecord,
    entry: &AutonomousResearchAcceptedWorkerEvidenceEntry,
    resolved_source_ref: &str,
    stage_content: &str,
) -> Result<String, String> {
    let path = main_agent_stage_artifact_adoption_manifest_path(resolved, job, record);
    let target_ref = record.target_artifact_path.clone();
    let manifest = json!({
        "schema_version": "autonomous_research.stage_artifact_adoption_manifest.v1",
        "adoption_ref": record.adoption_ref,
        "job_id": job.job_id,
        "stage_id": contract.stage_id,
        "stage_execution_id": record.stage_execution_id,
        "source_agent_id": entry.agent_id,
        "source_task_id": entry.task_id,
        "source_task_type": entry.task_type,
        "source_ref": record.source_ref,
        "resolved_source_ref": resolved_source_ref,
        "source_artifact_path": record.source_artifact_path,
        "target_artifact_path": target_ref,
        "rationale": record.rationale,
        "cleanup_required": record.cleanup_required,
        "request_review_rerun": record.request_review_rerun,
        "accepted_worker_output_manifest_ref": entry.output_manifest_ref,
        "accepted_worker_evidence_refs": accepted_worker_evidence_entry_refs(entry),
        "content_sha256": sha256_hex(stage_content.as_bytes()),
        "review_body_sha256": sha256_hex(
            autonomous_research_stage_artifact_review_body(stage_content).as_bytes()
        ),
        "created_at": timestamp_string(),
        "authority_boundary": "main_agent_decides_source_and_target; runtime_validates_provenance_and_reviewability"
    });
    write_text_atomic_runtime(
        &path,
        &serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
    )?;
    Ok(relative_workspace_ref(&resolved.workspace_root, &path))
}

pub(crate) fn sanitize_runtime_path_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        "item".to_string()
    } else {
        sanitized
    }
}

pub(crate) fn wrap_autonomous_research_stage_artifact_body_with_docframe(
    _resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    body: &str,
) -> String {
    let source_path = contract.artifact_path.clone();
    let mut evidence_refs = vec![
        format!("auto_research_job:{}", job.job_id),
        contract.artifact_path.clone(),
    ];
    evidence_refs.sort();
    evidence_refs.dedup();
    let frame = AutonomousResearchDocFrameMarkdown {
        title: format!("Astra Stage Artifact: {}", contract.artifact_type),
        doc_type: "report".to_string(),
        lifecycle: "active".to_string(),
        summary: format!(
            "Active {} artifact for Astra autonomous research stage {}.",
            contract.artifact_type, contract.stage_id
        ),
        key_claims: vec![
            format!(
                "This is the active stage artifact reviewed for stage {}.",
                contract.stage_id
            ),
            "Claims inside this artifact are bounded by cited evidence and stage maturity."
                .to_string(),
        ],
        decisions: vec![format!("Review target path is {}.", contract.artifact_path)],
        interfaces: vec![
            "stage_plan".to_string(),
            "stage_acceptance_rubric".to_string(),
            "review_gate".to_string(),
            "research_stage_dag".to_string(),
        ],
        evidence_refs,
        next_actions: vec![format!(
            "Submit {} to the stage review gate.",
            contract.artifact_path
        )],
        non_goals: vec![
            "This artifact alone does not advance the stage without a passing review.".to_string(),
        ],
    };
    let clean_body = autonomous_research_stage_artifact_review_body(body);
    format!(
        "---\n{}\n---\n\n{}",
        autonomous_research_doc_frame_yaml(&source_path, &frame),
        clean_body.trim_start()
    )
}

pub(crate) fn autonomous_research_stage_artifact_candidate_is_reviewable(
    content: &str,
    contract: &AutonomousResearchStageContract,
) -> bool {
    autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(content, contract, None)
}

pub(crate) fn autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
    content: &str,
    contract: &AutonomousResearchStageContract,
    candidate_relative_path: Option<&str>,
) -> bool {
    let trimmed = content.trim();
    let review_body = autonomous_research_stage_artifact_review_body(trimmed);
    autonomous_research_stage_artifact_body_is_mechanically_reviewable(
        trimmed,
        review_body,
        contract,
    ) && (autonomous_research_stage_artifact_is_bound_to_active_stage(
        trimmed,
        review_body,
        contract,
    ) || autonomous_research_stage_artifact_candidate_path_binds_active_stage(
        candidate_relative_path,
        contract,
    ))
}

fn autonomous_research_stage_artifact_candidate_path_binds_active_stage(
    candidate_relative_path: Option<&str>,
    contract: &AutonomousResearchStageContract,
) -> bool {
    let Some(candidate_relative_path) = candidate_relative_path else {
        return false;
    };
    if !stage_artifact_adoption_candidate_path_is_safe(candidate_relative_path) {
        return false;
    }
    let candidate_path = normalize_runtime_path_for_suffix(candidate_relative_path);
    let target_path = normalize_runtime_path_for_suffix(&contract.artifact_path);
    candidate_path == target_path || path_has_relative_suffix(&target_path, &candidate_path)
}

fn normalize_runtime_path_for_suffix(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn autonomous_research_stage_artifact_body_is_mechanically_reviewable(
    trimmed_content: &str,
    review_body: &str,
    contract: &AutonomousResearchStageContract,
) -> bool {
    if trimmed_content.is_empty()
        || autonomous_research_content_is_runtime_diagnostic_or_meta(trimmed_content)
    {
        return false;
    }
    if !autonomous_research_stage_artifact_candidate_lifecycle_violations(trimmed_content, contract)
        .is_empty()
    {
        return false;
    }
    if review_body.split_whitespace().count() < 120 {
        return false;
    }
    if autonomous_research_content_is_runtime_diagnostic_or_meta(review_body) {
        return false;
    }
    true
}

fn autonomous_research_stage_artifact_is_bound_to_active_stage(
    trimmed_content: &str,
    review_body: &str,
    contract: &AutonomousResearchStageContract,
) -> bool {
    let raw_lower = format!("{trimmed_content}\n{review_body}").to_ascii_lowercase();
    let artifact_path = contract.artifact_path.to_ascii_lowercase();
    if !artifact_path.trim().is_empty() && raw_lower.contains(&artifact_path) {
        return true;
    }

    let searchable = format!(
        " {} ",
        normalize_match_text_runtime(&format!("{trimmed_content}\n{review_body}"))
    );
    normalized_stage_identity_pair_present(
        &searchable,
        &[
            "stage_id",
            "stage id",
            "active stage id",
            "current stage id",
            "current_stage_id",
            "target stage id",
            "target_stage_id",
        ],
        &contract.stage_id,
    ) || normalized_stage_identity_pair_present(
        &searchable,
        &[
            "artifact_type",
            "artifact type",
            "stage artifact type",
            "required output artifact type",
            "required_output_artifact_type",
            "astra stage artifact",
        ],
        &contract.artifact_type,
    ) || normalized_stage_identity_pair_present(
        &searchable,
        &[
            "artifact path",
            "artifact_path",
            "source path",
            "source_path",
        ],
        &contract.artifact_path,
    )
}

fn normalized_stage_identity_pair_present(searchable: &str, aliases: &[&str], value: &str) -> bool {
    let value_variants = stage_identity_value_variants(value);
    aliases.iter().any(|alias| {
        let alias = normalize_match_text_runtime(alias);
        value_variants
            .iter()
            .any(|value| searchable.contains(&format!(" {alias} {value} ")))
    })
}

fn stage_identity_value_variants(value: &str) -> Vec<String> {
    let mut variants = Vec::new();
    for candidate in [
        value.to_string(),
        value.replace('_', " ").replace('-', " "),
        value.replace('_', " "),
        value.replace('-', " "),
    ] {
        let normalized = normalize_match_text_runtime(&candidate);
        if !normalized.trim().is_empty() && !variants.contains(&normalized) {
            variants.push(normalized);
        }
    }
    variants
}

pub(crate) fn autonomous_research_stage_artifact_review_body(content: &str) -> &str {
    let without_front_matter = strip_markdown_front_matter(content).trim_start();
    let lower = without_front_matter.to_ascii_lowercase();
    let mut end = without_front_matter.len();
    for marker in [
        "\n## accepted worker evidence",
        "\n# accepted worker evidence",
        "## accepted worker evidence",
        "# accepted worker evidence",
    ] {
        if let Some(index) = lower.find(marker) {
            end = end.min(index);
        }
    }
    without_front_matter[..end].trim_end()
}

pub(crate) fn autonomous_research_stage_artifact_candidate_lifecycle_violations(
    content: &str,
    contract: &AutonomousResearchStageContract,
) -> Vec<String> {
    let review_body = autonomous_research_stage_artifact_review_body(content);
    let lower = review_body.to_ascii_lowercase();
    let mut violations = Vec::new();
    for (marker, reason) in [
        (
            "target_canonical_path_under_review",
            "contains candidate wrapper metadata instead of standalone stage-artifact content",
        ),
        (
            "candidate_artifact_path",
            "contains candidate wrapper metadata instead of standalone stage-artifact content",
        ),
        (
            "retention_note",
            "contains worker-retention metadata that belongs in worker evidence, not the canonical artifact",
        ),
        (
            "pass for this candidate",
            "contains preflight comparison wording instead of canonical artifact assertions",
        ),
        (
            "fail for current canonical",
            "contains preflight comparison wording instead of canonical artifact assertions",
        ),
        (
            "if main agent chooses adoption",
            "contains handoff/adoption instructions instead of canonical artifact content",
        ),
    ] {
        if lower.contains(marker) {
            violations.push(reason.to_string());
        }
    }
    if lower.contains("does not mutate") && lower.contains("canonical") {
        violations.push(
            "states that the artifact does not mutate the canonical target; canonical review targets must not carry worker-boundary notes"
                .to_string(),
        );
    }
    if autonomous_research_stage_artifact_contains_candidate_only_lifecycle_metadata(review_body) {
        violations.push(
            "contains candidate-only lifecycle metadata that belongs in worker evidence, not the canonical artifact"
                .to_string(),
        );
    }
    if contract.stage_id == "literature" && lower.contains("candidate repair") {
        violations.push(
            "literature stage artifact is framed as a candidate repair memo, not a canonical literature_matrix"
                .to_string(),
        );
    }
    if autonomous_research_content_is_main_agent_process_summary(review_body) {
        violations.push(
            "contains main-agent process-summary wording instead of canonical stage-artifact content"
                .to_string(),
        );
    }
    violations.sort();
    violations.dedup();
    violations
}

fn autonomous_research_stage_artifact_contains_candidate_only_lifecycle_metadata(
    review_body: &str,
) -> bool {
    review_body.lines().any(|line| {
        let lower = line.trim().to_ascii_lowercase();
        if !lower.contains("candidate-only") {
            return false;
        }
        let self_reference = lower.contains("this artifact")
            || lower.contains("this file")
            || lower.contains("this document")
            || lower.contains("this output");
        let worker_lifecycle_context = lower.contains("worker artifact")
            || lower.contains("worker output")
            || lower.contains("isolated worktree");
        let authority_boundary = lower.contains("not canonical")
            || lower.contains("until the main agent")
            || lower.contains("awaiting")
            || lower.contains("does not mutate");
        let worker_boundary_term = lower.contains("retention") || lower.contains("handoff");
        let adoption_handoff =
            lower.contains("adoption") && (lower.contains("awaiting") || lower.contains("handoff"));
        let worker_boundary_note = (self_reference && authority_boundary)
            || (worker_lifecycle_context
                && (authority_boundary || worker_boundary_term || adoption_handoff));
        let metadata_key_prefix = lower
            .trim_start_matches(|ch| ch == '-' || ch == '*' || ch == ' ')
            .trim_start_matches("**");
        let metadata_key = (metadata_key_prefix.starts_with("candidate-only")
            || metadata_key_prefix.starts_with("candidate_only"))
            && metadata_key_prefix
                .find(':')
                .is_some_and(|index| index <= 40)
            && (lower.contains("worker artifact")
                || lower.contains("isolated worktree")
                || lower.contains("retention")
                || lower.contains("handoff")
                || lower.contains("awaiting adoption"));
        worker_boundary_note || metadata_key
    })
}

pub(crate) fn autonomous_research_stage_required_field_present(
    contract: &AutonomousResearchStageContract,
    field: &str,
    lower_content: &str,
) -> bool {
    let normalized = field.to_ascii_lowercase();
    if lower_content.contains(&normalized)
        || lower_content.contains(&normalized.replace(' ', "_"))
        || lower_content.contains(&normalized.replace(' ', "-"))
    {
        return true;
    }
    if contract.stage_id != "literature" {
        return false;
    }
    match normalized.as_str() {
        "source entries" => {
            lower_content.contains("canonical citation ledger")
                || lower_content.contains("grounded literature matrix rows")
                || lower_content.contains("| source id |")
                || lower_content.contains("| source_id |")
        }
        "canonical verified title" => {
            lower_content.contains("canonical title")
                || lower_content.contains("canonical verified title")
                || lower_content.contains("paper title")
        }
        "source verification status" => {
            lower_content.contains("verification status")
                || lower_content.contains("venue/year verification status")
                || lower_content.contains("verified")
        }
        "key evidence" => {
            lower_content.contains("representative evidence rows")
                || lower_content.contains("what the literature actually measures")
                || lower_content.contains("accepted evidence")
        }
        "relation to current objective" => {
            lower_content.contains("relative to chaotic-context")
                || lower_content.contains("relative to current")
                || lower_content.contains("target benchmark")
                || lower_content.contains("current objective")
        }
        "closest-family coverage note" => {
            lower_content.contains("closest-family ranking")
                || lower_content.contains("closest family")
                || lower_content.contains("nearest predecessor")
        }
        "claim support boundary" => {
            lower_content.contains("allowed claim boundary")
                || lower_content.contains("allowed synthesis claim")
                || lower_content.contains("claim boundary")
        }
        "missing-source risks" => {
            lower_content.contains("provisional")
                || lower_content.contains("quarantined")
                || lower_content.contains("not used as positive")
                || lower_content.contains("missing-source risk")
        }
        _ => false,
    }
}

pub(crate) fn autonomous_research_stage_artifact_evidence_binding_failures(
    content: &str,
    contract: &AutonomousResearchStageContract,
    index: Option<&AutonomousResearchAcceptedWorkerEvidenceIndex>,
) -> Vec<String> {
    let Some(index) = index else {
        return Vec::new();
    };
    let supporting_entries =
        accepted_worker_evidence_entries_requiring_artifact_binding(contract, index);
    if supporting_entries.is_empty() {
        return Vec::new();
    }

    let review_body = autonomous_research_stage_artifact_review_body(content);
    let mut failures = Vec::new();
    let Some(ledger_body) = stage_artifact_evidence_binding_ledger_body(review_body) else {
        failures.push(
            "stage artifact body is missing a section named `Evidence Binding Ledger` for stage-generic evidence binding"
                .to_string(),
        );
        return failures;
    };
    let normalized = normalize_match_text_runtime(ledger_body);

    for (field, aliases) in [
        (
            "unit_id",
            &[
                "unit_id",
                "unit id",
                "evidence unit id",
                "row id",
                "claim id",
            ][..],
        ),
        (
            "unit_kind",
            &["unit_kind", "unit kind", "evidence unit kind"][..],
        ),
        (
            "statement_or_target",
            &[
                "statement_or_target",
                "statement or target",
                "claim or target",
                "evidence target",
            ][..],
        ),
        (
            "support_status",
            &["support_status", "support status", "evidence status"][..],
        ),
        (
            "accepted_evidence_refs",
            &[
                "accepted_evidence_refs",
                "accepted evidence refs",
                "accepted worker evidence refs",
            ][..],
        ),
        (
            "main_agent_decision_refs",
            &[
                "main_agent_decision_refs",
                "main agent decision refs",
                "main-agent decision refs",
            ][..],
        ),
        (
            "canonical_artifact_refs",
            &[
                "canonical_artifact_refs",
                "canonical artifact refs",
                "canonical project refs",
            ][..],
        ),
        (
            "support_scope",
            &["support_scope", "support scope", "claim support boundary"][..],
        ),
        (
            "limitations_or_missing_risks",
            &[
                "limitations_or_missing_risks",
                "limitations or missing risks",
                "missing evidence risks",
                "missing-source risks",
            ][..],
        ),
    ] {
        if !normalized_contains_any_alias(&normalized, aliases) {
            failures.push(format!(
                "Evidence Binding Ledger is missing mechanically readable `{field}` column/key"
            ));
        }
    }

    let decision_refs = supporting_entries
        .iter()
        .filter_map(|entry| entry.main_agent_decision_ref.as_deref())
        .filter(|decision_ref| !decision_ref.trim().is_empty())
        .collect::<Vec<_>>();
    if !decision_refs.is_empty()
        && !decision_refs
            .iter()
            .any(|decision_ref| ledger_body.contains(*decision_ref))
    {
        failures.push(
            "Evidence Binding Ledger does not cite any upstream `main_agent_worker_artifact_decision::*` ref from active accepted worker evidence"
                .to_string(),
        );
    }

    let task_refs = supporting_entries
        .iter()
        .map(|entry| format!("accepted_worker_evidence_task:{}", entry.task_id))
        .collect::<Vec<_>>();
    if !task_refs.is_empty()
        && !task_refs
            .iter()
            .any(|task_ref| ledger_body.contains(task_ref))
    {
        failures.push(
            "Evidence Binding Ledger does not cite any `accepted_worker_evidence_task:*` ref from active accepted worker evidence"
                .to_string(),
        );
    }

    failures.sort();
    failures.dedup();
    failures
}

fn accepted_worker_evidence_entries_requiring_artifact_binding<'a>(
    contract: &AutonomousResearchStageContract,
    index: &'a AutonomousResearchAcceptedWorkerEvidenceIndex,
) -> Vec<&'a AutonomousResearchAcceptedWorkerEvidenceEntry> {
    accepted_worker_evidence_current_gate_entries(index)
        .into_iter()
        .filter(|entry| {
            !accepted_worker_evidence_entry_is_adoptable_stage_synthesis(contract, entry)
        })
        .filter(|entry| {
            entry
                .main_agent_decision_ref
                .as_deref()
                .is_some_and(|decision_ref| !decision_ref.trim().is_empty())
        })
        .collect()
}

fn stage_artifact_evidence_binding_ledger_body(review_body: &str) -> Option<&str> {
    let lower = review_body.to_ascii_lowercase();
    let start = [
        "\n## evidence binding ledger",
        "\n# evidence binding ledger",
        "## evidence binding ledger",
        "# evidence binding ledger",
        "\\section{evidence binding ledger}",
        "\\subsection{evidence binding ledger}",
        "evidence binding ledger:",
    ]
    .iter()
    .filter_map(|marker| lower.find(marker))
    .min()?;
    let search_start = (start + 1).min(lower.len());
    let end = ["\n# ", "\n## ", "\n### ", "\n\\section{", "\n\\subsection{"]
        .iter()
        .filter_map(|marker| {
            lower[search_start..]
                .find(marker)
                .map(|relative| search_start + relative)
        })
        .min()
        .unwrap_or(review_body.len());
    Some(review_body[start..end].trim())
}

fn normalized_contains_any_alias(normalized_haystack: &str, aliases: &[&str]) -> bool {
    aliases
        .iter()
        .map(|alias| normalize_match_text_runtime(alias))
        .any(|alias| !alias.trim().is_empty() && normalized_haystack.contains(&alias))
}

pub(crate) fn autonomous_research_content_is_runtime_diagnostic_or_meta(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    autonomous_research_content_is_main_agent_process_summary(content)
        || lower.contains("provider echo removed")
        || lower.contains("completed prompt via")
        || lower.contains("you are astra's main autonomous research agent")
        || lower.contains("stage contract slice")
        || lower.contains("main agent output rejected by runtime boundary")
        || lower.contains("blocked_no_deterministic_research_synthesis")
        || lower.contains("main agent round interrupted by budget")
        || lower.contains("preserve_partial_output_without_research_fallback")
        || lower.contains("blocked_no_budget_fallback_synthesis")
        || lower.contains("it is not a completed research artifact")
        || lower.contains("partial main agent output")
        || lower.contains("streamed main agent output")
        || lower.contains("actions taken")
        || lower.contains("the literature matrix has been completely rewritten")
        || lower.contains("resolved blocking obligation")
        || lower.contains("requested strict stage review rerun")
        || lower.contains("requested strict review rerun")
        || lower.contains("here's a summary")
        || lower.contains("here is a summary")
        || lower.contains("round summary")
}

pub(crate) fn autonomous_research_content_is_main_agent_process_summary(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    if lower.contains("# astra stage artifact:") && lower.contains("## main agent output") {
        return true;
    }
    let process_markers = [
        "accepted the pending",
        "adopted the accepted",
        "recorded the semantic decision",
        "requested strict review rerun",
        "requested strict stage review rerun",
        "requested review rerun",
        "next expected system action",
        "current blocker was procedural",
        "resolved blocking obligation",
    ];
    let marker_count = process_markers
        .iter()
        .filter(|marker| lower.contains(**marker))
        .count();
    marker_count >= 2
        && (lower.contains("stage artifact")
            || lower.contains("canonical")
            || lower.contains("obligation")
            || lower.contains("review rerun"))
}

#[cfg(test)]
mod stage_closure_boundary_tests {
    use super::*;

    fn literature_test_contract(required_fields: Vec<String>) -> AutonomousResearchStageContract {
        AutonomousResearchStageContract {
            stage_id: "literature".to_string(),
            stage_class: "survey".to_string(),
            artifact_type: "literature_matrix".to_string(),
            artifact_path: "research/stages/job_1/literature/literature_matrix.md".to_string(),
            review_strength: "strict".to_string(),
            objective: "build grounded literature evidence".to_string(),
            required_fields,
            pass_criteria: vec!["grounded".to_string()],
            failure_signals: vec!["generic".to_string()],
            worker_task_types: vec!["paper search".to_string()],
            worker_task_requirements: Vec::new(),
            advisory_worker_task_types: Vec::new(),
            evidence_plan_status: "adopted".to_string(),
            evidence_plan_ref: None,
            paper_allowed: false,
            final_completion_stage: false,
        }
    }

    #[test]
    fn adoption_review_ref_resolves_from_separate_runtime_data_dir() {
        let workspace_root = std::env::temp_dir().join(format!(
            "research_cli_stage_review_workspace_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let data_dir = std::env::temp_dir().join(format!(
            "research_cli_stage_review_data_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let packet_path = data_dir.join("reviews").join("rev_123").join("packet.json");
        std::fs::create_dir_all(packet_path.parent().expect("packet parent should exist"))
            .expect("review dir should write");
        std::fs::write(&packet_path, "{}").expect("review packet should write");
        std::fs::create_dir_all(&workspace_root).expect("workspace should write");
        let resolved = crate::projects::current::ResolvedProject {
            project_id: "project_stage_review_ref".to_string(),
            workspace_root: workspace_root.clone(),
            workspace_hash: "hash".to_string(),
            data_dir: data_dir.clone(),
            resolution_source: "test".to_string(),
        };

        assert_eq!(
            resolve_adoption_reference_path(&resolved, "review_packet:rev_123"),
            packet_path
        );

        let _ = std::fs::remove_dir_all(workspace_root);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn accepted_worker_artifacts_are_projected_into_workspace_visible_paths() {
        let workspace_root = std::env::temp_dir().join(format!(
            "research_cli_worker_projection_workspace_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let data_dir = std::env::temp_dir().join(format!(
            "research_cli_worker_projection_data_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let agent_id = "agent_projection";
        let agent_dir = data_dir.join("agents").join(agent_id);
        let candidate_archive = agent_dir
            .join("candidate_archive")
            .join("fixture")
            .join("literature_matrix.md");
        std::fs::create_dir_all(
            candidate_archive
                .parent()
                .expect("candidate parent should exist"),
        )
        .expect("candidate dir should write");
        std::fs::write(
            &candidate_archive,
            "# Literature Matrix\n\nprojected candidate",
        )
        .expect("candidate should write");
        std::fs::write(
            agent_dir.join("output_manifest.json"),
            "{\"status\":\"complete\"}",
        )
        .expect("output manifest should write");
        std::fs::write(
            agent_dir.join("task_packet.json"),
            "{\"task_id\":\"stage synthesis\"}",
        )
        .expect("task packet should write");
        std::fs::write(agent_dir.join("provider_worker_evidence.md"), "# Evidence")
            .expect("worker evidence should write");
        std::fs::write(
            agent_dir.join("worktree_artifact_candidates.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "agent_worktree_artifact_candidate_manifest.v1",
                "agent_id": agent_id,
                "authority_scope": "worker_evidence_only",
                "adoption_status": "candidate_only",
                "workspace_binding_ref": data_dir.join("workspace_binding.json"),
                "source_workspace_root": workspace_root,
                "worktree_path": data_dir.join("worktrees").join(agent_id),
                "git_head": "abc123",
                "status_entries": ["?? research/stages/job_1/literature/literature_matrix.md"],
                "changed_paths": ["research/stages/job_1/literature/literature_matrix.md"],
                "untracked_paths": ["research/stages/job_1/literature/literature_matrix.md"],
                "candidate_entries": [{
                    "relative_path": "research/stages/job_1/literature/literature_matrix.md",
                    "path_kind": "file",
                    "artifact_kind": "stage_artifact_candidate",
                    "size_bytes": 40,
                    "sha256": sha256_hex(b"# Literature Matrix\n\nprojected candidate"),
                    "candidate_archive_ref": candidate_archive,
                    "safe_status": "safe",
                    "from_input_bundle": false,
                    "is_directory": false,
                    "captured_at": timestamp_string()
                }],
                "generated_at": timestamp_string()
            }))
            .expect("candidate manifest should serialize"),
        )
        .expect("candidate manifest should write");
        std::fs::create_dir_all(&workspace_root).expect("workspace should write");
        let resolved = crate::projects::current::ResolvedProject {
            project_id: "project_worker_projection".to_string(),
            workspace_root: workspace_root.clone(),
            workspace_hash: "hash".to_string(),
            data_dir: data_dir.clone(),
            resolution_source: "test".to_string(),
        };

        let projection = project_accepted_worker_artifacts_with_refs(
            &resolved,
            "job_1",
            "stage_1",
            agent_id,
            &[],
        )
        .expect("worker artifacts should project");

        assert!(projection
            .workspace_refs
            .iter()
            .all(|reference| !Path::new(reference).is_absolute()));
        assert!(projection
            .workspace_refs
            .iter()
            .any(|reference| reference.ends_with("output_manifest.json")));
        let candidate_ref = projection
            .workspace_refs
            .iter()
            .find(|reference| reference.ends_with("literature_matrix.md"))
            .expect("projected candidate ref should exist");
        assert_eq!(
            std::fs::read_to_string(workspace_root.join(candidate_ref))
                .expect("projected candidate should read"),
            "# Literature Matrix\n\nprojected candidate"
        );
        let projection_manifest =
            std::fs::read_to_string(workspace_root.join(&projection.projection_manifest_ref))
                .expect("projection manifest should read");
        assert!(projection_manifest.contains(&candidate_archive.display().to_string()));

        let _ = std::fs::remove_dir_all(workspace_root);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn accepted_worker_evidence_prefers_projection_refs_over_runtime_paths() {
        let workspace_root = std::env::temp_dir().join(format!(
            "research_cli_worker_projection_entry_workspace_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let data_dir = std::env::temp_dir().join(format!(
            "research_cli_worker_projection_entry_data_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let agent_id = "agent_projection_entry";
        let agent_dir = data_dir.join("agents").join(agent_id);
        std::fs::create_dir_all(&agent_dir).expect("agent dir should write");
        std::fs::write(
            agent_dir.join("output_manifest.json"),
            "{\"status\":\"complete\"}",
        )
        .expect("output manifest should write");
        std::fs::write(
            agent_dir.join("task_packet.json"),
            "{\"task_id\":\"stage synthesis\"}",
        )
        .expect("task packet should write");
        std::fs::write(agent_dir.join("provider_worker_evidence.md"), "# Evidence")
            .expect("worker evidence should write");
        std::fs::create_dir_all(&workspace_root).expect("workspace should write");
        let resolved = crate::projects::current::ResolvedProject {
            project_id: "project_worker_projection_entry".to_string(),
            workspace_root: workspace_root.clone(),
            workspace_hash: "hash".to_string(),
            data_dir: data_dir.clone(),
            resolution_source: "test".to_string(),
        };
        let mut entry = AutonomousResearchAcceptedWorkerEvidenceEntry {
            agent_id: agent_id.to_string(),
            task_id: "research_stage_task::stage_1::stage_artifact_synthesis".to_string(),
            task_type: "stage artifact synthesis".to_string(),
            worker_role: "research_synthesizer".to_string(),
            required_output_artifact_type: "literature_matrix".to_string(),
            output_manifest_ref: agent_dir.join("output_manifest.json").display().to_string(),
            task_packet_ref: agent_dir.join("task_packet.json").display().to_string(),
            evidence_refs: vec![
                agent_dir
                    .join("provider_worker_evidence.md")
                    .display()
                    .to_string(),
                "main_agent_worker_artifact_decision::decision_1".to_string(),
            ],
            matched_required_fields: Vec::new(),
            matched_acceptance_checks: Vec::new(),
            matched_quality_signals: Vec::new(),
            quality_profile: GoalStageTaskQualityProfile::default(),
            semantic_review: None,
            main_agent_acceptance: None,
            acceptance_authority: Some("main_agent_worker_artifact_decision".to_string()),
            main_agent_decision_ref: Some(
                "main_agent_worker_artifact_decision::decision_1".to_string(),
            ),
            review_required: Some(false),
            active_status: Some("active".to_string()),
            current_evidence_set_id: Some("stage_1::synthesis".to_string()),
            superseded_by_task_id: None,
            replacement_of_task_ids: Vec::new(),
            decision_reason: Some("accepted".to_string()),
            created_at: Some(timestamp_string()),
        };

        apply_accepted_worker_artifact_projection(&resolved, "job_1", "stage_1", &mut entry)
            .expect("accepted evidence should use projection");

        assert!(!Path::new(&entry.output_manifest_ref).is_absolute());
        assert!(!Path::new(&entry.task_packet_ref).is_absolute());
        assert!(entry
            .evidence_refs
            .iter()
            .all(|reference| !Path::new(reference).is_absolute()));
        assert!(entry
            .evidence_refs
            .iter()
            .any(|reference| reference.ends_with("projection.json")));
        assert!(entry
            .evidence_refs
            .contains(&"main_agent_worker_artifact_decision::decision_1".to_string()));

        let _ = std::fs::remove_dir_all(workspace_root);
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn main_agent_worker_evidence_summary_includes_input_visibility() {
        let workspace_root = std::env::temp_dir().join(format!(
            "research_cli_stage_visibility_{}_{}",
            std::process::id(),
            timestamp_string()
        ));
        let data_dir = workspace_root.join(".pmcli");
        let agent_dir = data_dir.join("agents").join("agent_visibility");
        std::fs::create_dir_all(&agent_dir).expect("agent dir should write");
        let manifest = crate::agents::AgentInputArtifactMountManifest {
            schema_version: "agent_input_artifact_mount_manifest.v1".to_string(),
            agent_id: "agent_visibility".to_string(),
            authority_scope: "runtime_input_mount_only".to_string(),
            source_workspace_root: workspace_root.display().to_string(),
            worktree_path: workspace_root.join(".worktree").display().to_string(),
            manifest_path_in_worktree: ".pmcli/worker-input-artifacts.json".to_string(),
            workspace_visibility: crate::agents::AgentWorkspaceVisibility {
                schema_version: "agent_workspace_visibility.v1".to_string(),
                visibility_scope: "worker_git_worktree_plus_mounted_inputs".to_string(),
                source_workspace_root: workspace_root.display().to_string(),
                worktree_path: workspace_root.join(".worktree").display().to_string(),
                baseline_mode: Some("initial_head".to_string()),
                git_head: "abc123".to_string(),
                tracked_root_entries: vec![".astra".to_string()],
                visible_root_entries: vec![".astra/".to_string(), ".git".to_string()],
                project_marker_paths_visible: Vec::new(),
                project_marker_status: "no_project_markers_visible".to_string(),
                notes: Vec::new(),
            },
            mounted_inputs: Vec::new(),
            unmounted_input_refs: vec!["accepted_worker_evidence:paper_search".to_string()],
            generated_at: "1".to_string(),
        };
        std::fs::write(
            agent_dir.join("input_artifact_mounts.json"),
            serde_json::to_string_pretty(&manifest).expect("manifest should serialize"),
        )
        .expect("manifest should write");
        let resolved = crate::projects::current::ResolvedProject {
            project_id: "project_visibility".to_string(),
            workspace_root: workspace_root.clone(),
            workspace_hash: "hash".to_string(),
            data_dir,
            resolution_source: "test".to_string(),
        };

        let summary = render_agent_input_visibility_for_main_prompt(&resolved, "agent_visibility")
            .expect("visibility summary should render");

        assert!(summary.contains("input_visibility"));
        assert!(summary.contains("status=`no_project_markers_visible`"));
        assert!(summary.contains("unmounted_input_refs=`1`"));
        assert!(summary.contains("project_marker_paths=`none`"));
        assert!(summary.contains("visible_root_entries=`.astra/, .git`"));
    }

    #[test]
    fn reviewable_candidate_does_not_require_semantic_field_headings() {
        let contract = literature_test_contract(vec![
            "source entries".to_string(),
            "canonical verified title".to_string(),
            "source verification status".to_string(),
            "claim support boundary".to_string(),
        ]);
        let content = format!(
            "# Review Target\n\n- stage_id: `{}`\n- artifact_type: `{}`\n\n{}",
            contract.stage_id,
            contract.artifact_type,
            "This is a clean canonical stage artifact body. It speaks about evidence routing, stage closure, adoption readiness, and the repair bridge without using wrapper metadata or runtime diagnostic language. The goal is to preserve a review target that a reviewer can inspect directly. The body avoids handoff wording, and remains intentionally generic about the final judgment so that the main agent can still decide whether the evidence is sufficient. The document remains coherent, long enough to review, and free of metadata pollution. "
                .repeat(3)
        );

        assert!(autonomous_research_stage_artifact_candidate_is_reviewable(
            &content, &contract
        ));
    }

    #[test]
    fn process_summary_wrapper_is_runtime_meta_not_stage_artifact() {
        let contract = literature_test_contract(vec![
            "research question".to_string(),
            "citation ledger".to_string(),
            "source entries".to_string(),
            "claim support boundary".to_string(),
        ]);
        let content = "# Astra Stage Artifact: literature_matrix\n\n\
            ## Stage\n\n\
            - stage_id: `literature`\n\n\
            ## Required Fields\n\n\
            - research question\n\
            - citation ledger\n\
            - source entries\n\
            - claim support boundary\n\n\
            ## Main Agent Output\n\n\
            - Accepted the pending `stage artifact synthesis` worker candidate.\n\
            - Adopted the accepted candidate into the canonical stage artifact.\n\
            - Requested strict review rerun for the `literature` stage.\n\
            - Next expected system action is the rerun review gate.\n";

        assert!(autonomous_research_content_is_runtime_diagnostic_or_meta(
            content
        ));
        assert!(!autonomous_research_stage_artifact_candidate_is_reviewable(
            content, &contract
        ));
    }

    #[test]
    fn unbound_long_candidate_is_not_reviewable() {
        let contract = literature_test_contract(Vec::new());
        let content = "This long note discusses literature broadly, project momentum, and how a reviewer might think about evidence, but it never binds itself to the active stage identity, the artifact type, or the target artifact path. It is deliberately verbose enough to clear the old length-only gate while remaining mechanically ownerless. A runtime gate should not infer that this text belongs to the current stage just because it looks coherent, uses research words, and has enough paragraphs for a human to read. "
            .repeat(4);

        assert!(!autonomous_research_stage_artifact_candidate_is_reviewable(
            &content, &contract
        ));
    }

    #[test]
    fn docframe_source_path_binds_reviewable_artifact_without_semantic_template() {
        let contract = literature_test_contract(vec![
            "source entries".to_string(),
            "canonical verified title".to_string(),
            "source verification status".to_string(),
        ]);
        let content = format!(
            "---\ndoc_frame:\n  source_path: {}\n  title: Astra Stage Artifact\n---\n\n{}",
            contract.artifact_path,
            "This body is intentionally not a literature-specific semantic template. It gives the reviewer a coherent artifact body to inspect, explains evidence routing, describes how accepted worker material will be interpreted, and keeps runtime metadata separate from the primary review target. The body is long enough for mechanical reviewability and avoids diagnostic text, prompt echo, and candidate handoff wording. "
                .repeat(4)
        );

        assert!(autonomous_research_stage_artifact_candidate_is_reviewable(
            &content, &contract
        ));
    }

    #[test]
    fn worktree_candidate_path_binds_reviewable_artifact_without_body_identity_echo() {
        let contract = literature_test_contract(vec![
            "source entries".to_string(),
            "canonical verified title".to_string(),
            "source verification status".to_string(),
            "claim support boundary".to_string(),
        ]);
        let content = format!(
            "# Literature Matrix\n\n\
             ## Research Question\n\n\
             What evidence should bound a chaotic-context decision benchmark?\n\n\
             ## Citation Ledger\n\n\
             | source id | canonical verified title | source verification status | metadata confidence |\n\
             |---|---|---|---|\n\
             | R1 | Lost in the Middle | verified official source identity | high |\n\n\
             ## Source Entries\n\n\
             {}",
            "This body is a clean literature matrix with row-level source entries, method comparison, open problems, limitations, claim support boundaries, missing-source risks, and repair routes. It intentionally does not echo the full canonical target path or stage id in prose because the worker manifest already binds the candidate by its relative path. "
                .repeat(5)
        );

        assert!(
            !autonomous_research_stage_artifact_candidate_is_reviewable(&content, &contract),
            "plain detached text should still need in-body stage identity"
        );
        assert!(
            autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                &content,
                &contract,
                Some(&contract.artifact_path)
            ),
            "a manifest-captured candidate at the exact target path is structurally bound"
        );
        assert!(
            autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                &content,
                &contract,
                Some("literature_matrix.md")
            ),
            "a worker-local artifact named like the target file should bind to the canonical stage artifact target"
        );
        assert!(
            !autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                &content,
                &contract,
                Some("notes.md")
            ),
            "an unrelated worker-local artifact name must not bind to the canonical stage artifact target"
        );
        assert!(
            !autonomous_research_stage_artifact_candidate_is_reviewable_for_adoption(
                &content,
                &contract,
                Some("../literature_matrix.md")
            ),
            "unsafe relative paths must not bind even when the file name matches"
        );
    }

    #[test]
    fn bound_repair_memo_is_still_not_reviewable() {
        let contract = literature_test_contract(Vec::new());
        let content = format!(
            "# Astra Stage Artifact: {}\n\n- stage_id: `{}`\n\n{}",
            contract.artifact_type,
            contract.stage_id,
            "This candidate repair memo is intentionally bound to the right stage, but it is still a candidate repair note instead of a canonical stage artifact body. It repeats operational handoff language, explains what would happen if main agent chooses adoption, and therefore stays outside the clean review target boundary. Length and identity cannot override lifecycle pollution. "
                .repeat(4)
        );

        assert!(!autonomous_research_stage_artifact_candidate_is_reviewable(
            &content, &contract
        ));
    }
}

pub(crate) fn relative_display_ref(value: &str) -> String {
    if let Some(index) = value.find(".pmcli/") {
        return value[index..].to_string();
    }
    if let Some(index) = value.find("research/") {
        return value[index..].to_string();
    }
    if let Some(index) = value.find("experiments/") {
        return value[index..].to_string();
    }
    if let Some(index) = value.find("papers/") {
        return value[index..].to_string();
    }
    value.to_string()
}

pub(crate) fn comma_or_none_runtime(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

pub(crate) fn build_autonomous_research_agent_context(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    prompt: &str,
) -> Vec<ChatMessage> {
    let mut system_messages = crate::providers::build_system_messages();
    let role_soul = crate::session::context_pack::resolve_soul(
        &resolved.workspace_root,
        std::env::var("HOME").ok().map(PathBuf::from).as_deref(),
        Some(crate::session::context_pack::DEFAULT_MAIN_AGENT_ROLE_PROFILE),
    )
    .map(|soul| soul.render_section())
    .unwrap_or_else(|err| {
        format!(
            "<soul-context degraded=\"true\">\nMain-agent Soul assembly failed: {err}\n</soul-context>"
        )
    });
    system_messages.push(ChatMessage {
        role: "system".to_string(),
        content: role_soul,
        tool_calls: None,
        tool_call_id: None,
    });
    system_messages.push(ChatMessage {
        role: "system".to_string(),
        content: format!(
            "Astra autonomous research supervisor context:\n\
             - project_id: {}\n\
             - workspace_root: {}\n\
             - job_id: {}\n\
             - phase: {}\n\
             - ticks_completed: {}\n\
             - review_rounds_completed: {}\n\
             - provider_rounds_completed: {}\n\
             - active_provider: {}/{}\n\
             - configured_provider_failover: {}\n\
             You must use the available tools to inspect local context, publish/update board tasks, record worker artifact decisions, route obligations, request review reruns, and request cleanup. Do not ask the human for next steps in full-auto. Do not perform agent-team research execution yourself.",
            resolved.project_id,
            resolved.workspace_root.display(),
            job.job_id,
            job.phase,
            job.ticks_completed,
            job.review_rounds_completed,
            job.provider_rounds.len(),
            provider_trace.resolved_provider,
            provider_trace.resolved_model,
            crate::config::effective_config(resolved)
                .ok()
                .map(|config| {
                    if config.effective.provider_failover.is_empty() {
                        "none".to_string()
                    } else {
                        config.effective.provider_failover.join(", ")
                    }
                })
                .unwrap_or_else(|| "unavailable".to_string())
        ),
        tool_calls: None,
        tool_call_id: None,
    });
    system_messages.push(ChatMessage {
        role: "user".to_string(),
        content: fit_autonomous_research_prompt_to_model(provider_trace, prompt),
        tool_calls: None,
        tool_call_id: None,
    });
    system_messages
}

pub(crate) fn fit_autonomous_research_prompt_to_model(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    prompt: &str,
) -> String {
    let config = crate::providers::model_window_config(&provider_trace.resolved_model);
    let max_chars = config
        .context_window
        .saturating_sub(config.output_reservation)
        .saturating_mul(3)
        .clamp(4_000, 24_000);
    truncate_for_prompt(prompt, max_chars)
}

pub(crate) fn fit_autonomous_research_review_prompt_to_model(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    prompt: &str,
) -> String {
    let config = crate::providers::model_window_config(&provider_trace.resolved_model);
    let max_chars = config
        .context_window
        .saturating_sub(config.output_reservation)
        .saturating_sub(config.system_budget)
        .saturating_mul(3)
        .saturating_mul(4)
        .checked_div(5)
        .unwrap_or(AUTONOMOUS_RESEARCH_REVIEW_PROMPT_MAX_CHARS)
        .clamp(48_000, AUTONOMOUS_RESEARCH_REVIEW_PROMPT_MAX_CHARS);
    truncate_for_prompt(prompt, max_chars)
}

pub(crate) fn render_autonomous_research_review_artifact_refs(refs: &[String]) -> String {
    if refs.is_empty() {
        return "artifact_ref_manifest: total_refs=0 included_recent_refs=0\nnone".to_string();
    }
    let mut recent = refs.iter().rev().take(128).cloned().collect::<Vec<_>>();
    recent.reverse();
    let rendered = format!(
        "artifact_ref_manifest: total_refs={} included_recent_refs={} policy=recent_bounded_manifest\n{}",
        refs.len(),
        recent.len(),
        recent.join("\n")
    );
    truncate_for_prompt(
        &rendered,
        AUTONOMOUS_RESEARCH_REVIEW_ARTIFACT_REFS_MAX_CHARS,
    )
}

pub(crate) fn render_autonomous_research_review_tick_trace(
    ticks: &[AutonomousResearchTickSummary],
) -> String {
    if ticks.is_empty() {
        return "tick_trace_summary: total_ticks=0 included_recent_ticks=0\nnone".to_string();
    }
    let total_dispatches = ticks.iter().map(|tick| tick.dispatch_count).sum::<usize>();
    let accepted_ticks = ticks.iter().filter(|tick| tick.accepted).count();
    let recent = ticks.iter().rev().take(24).collect::<Vec<_>>();
    let mut rows = recent
        .into_iter()
        .rev()
        .map(|tick| {
            format!(
                "tick {}: status={}, loop={}, dispatches={}, accepted={}, next={}",
                tick.tick_index,
                tick.status,
                tick.loop_status,
                tick.dispatch_count,
                tick.accepted,
                tick.next_recommended_action
            )
        })
        .collect::<Vec<_>>();
    rows.insert(
        0,
        format!(
            "tick_trace_summary: total_ticks={} included_recent_ticks={} total_dispatches={} accepted_ticks={} policy=aggregate_plus_recent",
            ticks.len(),
            rows.len(),
            total_dispatches,
            accepted_ticks
        ),
    );
    truncate_for_prompt(
        &rows.join("\n"),
        AUTONOMOUS_RESEARCH_REVIEW_TICK_TRACE_MAX_CHARS,
    )
}

pub(crate) fn run_autonomous_research_compact_agent_repair(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    store: &SessionStore,
    session_id: &str,
    tool_executor: &LocalToolExecutor,
    permission_policy: &crate::permissions::PermissionPolicy,
    cancel_token: &RuntimeCancelToken,
    reasoning_effort: Option<&str>,
    cause: AgentLoopError,
    round_budget: AgentLoopBudget,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<AgentLoopResult, AutonomousResearchMainAgentRoundError> {
    let tool_defs = crate::tools::main_agent_tool_definitions_for_mode(&job.permission_mode);
    let tool_defs_json = crate::tools::tool_definitions_to_openai_json(&tool_defs);
    let contract = autonomous_research_stage_contract(resolved, job);
    let prompt = format!(
        "{}\n\n\
         Supervisor retry reason: the previous provider/tool agent attempt failed with `{}`.\n\
         Continue autonomously from the compact job state. Re-establish the main-agent decision state: inspect local context, publish or update board-visible agent-team work, record artifact decisions or obligation routes, and request review only after accepted evidence exists. Do not perform worker research execution yourself and do not write the active stage artifact `{}` directly.",
        render_autonomous_research_agent_prompt(resolved, job),
        cause,
        contract.artifact_path
    );
    let context_messages =
        build_autonomous_research_agent_context(provider_trace, resolved, job, &prompt);
    let mut text_buffer = String::new();
    let mut on_delta = |delta: &str| {
        text_buffer.push_str(delta);
    };
    let mut on_tool_start = |name: &str, id: &str| {
        let _ = append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_tool_started",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tool_name": name,
                "tool_call_id": id,
                "session_id": session_id,
                "retry": true
            }),
        );
    };
    let mut on_tool_result = |event: crate::runtime::agent_loop::AgentLoopToolResultEvent| {
        let _ = append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_tool_completed",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tool_name": event.tool_name,
                "tool_call_id": event.call_id,
                "tool_status": event.status,
                "session_id": session_id,
                "retry": true
            }),
        );
    };
    crate::runtime::agent_loop::run_agent_loop_with_budget_and_timeout_policy(
        provider_trace,
        &context_messages,
        tool_defs_json,
        tool_executor,
        permission_policy,
        store,
        session_id,
        cancel_token,
        &mut on_delta,
        &mut on_tool_start,
        &mut on_tool_result,
        reasoning_effort,
        round_budget,
        timeout_policy,
    )
    .map_err(|retry_err| match retry_err {
        AgentLoopError::ProviderError(provider_error) => {
            autonomous_research_main_agent_provider_fault_from_error(
                job,
                provider_trace,
                "main_agent_compact_retry",
                &provider_error,
            )
        }
        other => AutonomousResearchMainAgentRoundError::NonProvider(format!(
            "initial agent attempt failed: {cause}; compact retry failed: {other}; partial compact output bytes={}",
            text_buffer.len()
        )),
    })
}

pub(crate) fn autonomous_research_main_agent_round_runtime_budget(
    resume_deadline_ms: Option<u128>,
    now_ms: u128,
) -> Result<(AgentLoopBudget, ProviderTimeoutPolicy), AutonomousResearchMainAgentRoundError> {
    let base = autonomous_research_main_agent_round_budget();
    let Some(deadline_ms) = resume_deadline_ms else {
        return Ok((base, ProviderTimeoutPolicy::Blocking));
    };
    let remaining_ms = deadline_ms.saturating_sub(now_ms);
    if remaining_ms < 1_000 {
        return Err(AutonomousResearchMainAgentRoundError::NonProvider(
            "runtime_budget_exhausted_before_main_agent_round".to_string(),
        ));
    }
    let budget_ms = remaining_ms.saturating_sub(500).max(1);
    let max_elapsed = Duration::from_millis(budget_ms.min(u128::from(u64::MAX)) as u64);
    Ok((
        base.with_max_elapsed(max_elapsed),
        ProviderTimeoutPolicy::TaskBudget(max_elapsed),
    ))
}

pub(crate) fn run_autonomous_research_main_agent_round(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    resume_deadline_ms: Option<u128>,
    _output_json: bool,
) -> Result<Option<AutonomousResearchAgentRound>, AutonomousResearchMainAgentRoundError> {
    let contract = autonomous_research_stage_contract(resolved, job);
    ensure_autonomous_research_stage_plan_docframe(resolved, job, &contract)
        .map_err(AutonomousResearchMainAgentRoundError::NonProvider)?;
    let prompt_defaults = resolve_prompt_request_defaults(
        resolved,
        &PromptInvocation {
            session_id: job.session_id.clone(),
            new_session: job.session_id.is_none(),
            prompt_text: job.prompt.clone(),
            provider: None,
            model: None,
            permission_mode: Some(job.permission_mode.clone()),
        },
    )
    .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    let route_selection = autonomous_research_select_provider_route_for_main_agent(
        resolved,
        &prompt_defaults,
        Some(job),
    );
    let Some(route_selection) = route_selection else {
        return Ok(None);
    };
    let effective_config = crate::config::effective_config(resolved)
        .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?
        .effective;
    let provider_trace = match resolve_provider_trace_with_profiles(
        route_selection.provider.as_deref(),
        route_selection.model.as_deref(),
        route_selection.provider_source.as_deref(),
        route_selection.model_source.as_deref(),
        &effective_config.provider_profiles,
    ) {
        Ok(trace) if trace.auth_status == "configured" => trace,
        Ok(_) | Err(_) => return Ok(None),
    };
    if route_selection.failover_used {
        append_autonomous_research_job_event(
            resolved,
            job,
            "provider_failover_selected",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "provider": provider_trace.resolved_provider,
                "model": provider_trace.resolved_model,
                "active_provider_fault": job.active_provider_fault
            }),
        )
        .map_err(AutonomousResearchMainAgentRoundError::NonProvider)?;
    }
    if let Ok(decision_refs) =
        sync_main_agent_worker_artifact_decisions_into_accepted_worker_evidence(
            resolved, job, &contract,
        )
    {
        merge_unique_strings(&mut job.artifact_refs, decision_refs);
    }
    let store = SessionStore::new(resolved.data_dir.clone());
    let session_id = match job.session_id.clone() {
        Some(session_id) => session_id,
        None => {
            let session = store
                .create_session_with_kind(
                    Some(format!("Auto research {}", job.job_id)),
                    "auto_research",
                )
                .map_err(|err| {
                    AutonomousResearchMainAgentRoundError::NonProvider(err.to_string())
                })?;
            job.session_id = Some(session.session_id.clone());
            session.session_id
        }
    };
    let continuity_packet = write_autonomous_research_continuity_packet(
        resolved,
        job,
        "main_agent_round",
        Some(&provider_trace),
    )
    .map_err(AutonomousResearchMainAgentRoundError::NonProvider)?;
    write_autonomous_research_job_state(resolved, job)
        .map_err(AutonomousResearchMainAgentRoundError::NonProvider)?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "continuity_packet_written",
        serde_json::to_value(&continuity_packet)
            .expect("continuity packet summary should serialize"),
    )
    .map_err(AutonomousResearchMainAgentRoundError::NonProvider)?;
    let prompt = render_autonomous_research_agent_prompt(resolved, job);
    let context_messages =
        build_autonomous_research_agent_context(&provider_trace, resolved, job, &prompt);
    let tool_defs = crate::tools::main_agent_tool_definitions_for_mode(&job.permission_mode);
    let tool_defs_json = crate::tools::tool_definitions_to_openai_json(&tool_defs);
    let tool_executor = LocalToolExecutor::with_autonomous_research_context(
        &resolved.workspace_root,
        LocalToolExecutorContext {
            data_dir: resolved.data_dir.clone(),
            project_id: resolved.project_id.clone(),
            job_id: Some(job.job_id.clone()),
            stage_id: Some(contract.stage_id.clone()),
            stage_execution_id: job.stage_execution_id.clone(),
            stage_artifact_type: Some(contract.artifact_type.clone()),
        },
    );
    let perm_mode = crate::permissions::PermissionMode::parse(&job.permission_mode)
        .unwrap_or(crate::permissions::PermissionMode::WorkspaceWrite);
    let permission_policy =
        crate::permissions::PermissionPolicy::new(perm_mode, &resolved.workspace_root);
    let cancel_token = RuntimeCancelToken::default();
    let mut text_buffer = String::new();
    let mut on_delta = |delta: &str| {
        text_buffer.push_str(delta);
    };
    let mut on_tool_start = |name: &str, id: &str| {
        let _ = append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_tool_started",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tool_name": name,
                "tool_call_id": id,
                "session_id": session_id
            }),
        );
    };
    let mut on_tool_result = |event: crate::runtime::agent_loop::AgentLoopToolResultEvent| {
        let _ = append_autonomous_research_job_event(
            resolved,
            job,
            "main_agent_tool_completed",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tool_name": event.tool_name,
                "tool_call_id": event.call_id,
                "tool_status": event.status,
                "session_id": session_id
            }),
        );
    };
    let (round_budget, timeout_policy) = autonomous_research_main_agent_round_runtime_budget(
        resume_deadline_ms,
        timestamp_millis(),
    )?;
    let agent_loop_result =
        crate::runtime::agent_loop::run_agent_loop_with_budget_and_timeout_policy(
            &provider_trace,
            &context_messages,
            tool_defs_json,
            &tool_executor,
            &permission_policy,
            &store,
            &session_id,
            &cancel_token,
            &mut on_delta,
            &mut on_tool_start,
            &mut on_tool_result,
            prompt_defaults.reasoning_effort.as_deref(),
            round_budget,
            timeout_policy,
        )
        .or_else(|err| match err {
            AgentLoopError::BudgetExhausted {
                iterations,
                tool_calls_made,
                partial_content,
            } => Ok(finalize_autonomous_research_budgeted_round(
                resolved,
                job,
                &text_buffer,
                iterations,
                tool_calls_made,
                partial_content,
                session_id.as_str(),
                "tool_call_budget",
            )),
            AgentLoopError::MaxIterationsExceeded {
                iterations,
                tool_calls_made,
                partial_content,
            } => Ok(finalize_autonomous_research_budgeted_round(
                resolved,
                job,
                &text_buffer,
                iterations,
                tool_calls_made,
                partial_content,
                session_id.as_str(),
                "iteration_budget",
            )),
            AgentLoopError::ProviderError(provider_error) => {
                Err(autonomous_research_main_agent_provider_fault_from_error(
                    job,
                    &provider_trace,
                    "main_agent_round",
                    &provider_error,
                ))
            }
            other => run_autonomous_research_compact_agent_repair(
                &provider_trace,
                resolved,
                job,
                &store,
                &session_id,
                &tool_executor,
                &permission_policy,
                &cancel_token,
                prompt_defaults.reasoning_effort.as_deref(),
                other,
                round_budget,
                timeout_policy,
            ),
        })?;
    let final_content = if agent_loop_result.final_content.trim().is_empty() {
        text_buffer
    } else {
        agent_loop_result.final_content.clone()
    };
    store
        .append_line(
            &session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: prompt,
            },
        )
        .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    store
        .append_line(
            &session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: final_content.clone(),
            },
        )
        .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    let artifact_path = resolved
        .workspace_root
        .join("research")
        .join("auto")
        .join(&job.job_id)
        .join(format!(
            "main-agent-round-{}.md",
            job.provider_rounds.len() + 1
        ));
    write_autonomous_research_report(&artifact_path, &final_content)
        .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    let contract = autonomous_research_stage_contract(resolved, job);
    let stage_artifact_path = resolved.workspace_root.join(&contract.artifact_path);
    let decision_index_refs =
        sync_main_agent_worker_artifact_decisions_into_accepted_worker_evidence(
            resolved, job, &contract,
        )
        .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    merge_unique_strings(&mut job.artifact_refs, decision_index_refs);
    let accepted_worker_evidence_index =
        load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    persist_autonomous_research_stage_artifact_candidate(
        resolved,
        job,
        &contract,
        &stage_artifact_path,
        &final_content,
        accepted_worker_evidence_index.as_ref(),
    )
    .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    let adopted_refs = process_main_agent_stage_artifact_adoptions(
        resolved,
        job,
        &contract,
        accepted_worker_evidence_index.as_ref(),
    )
    .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    ensure_autonomous_research_stage_delivery_artifacts(resolved, job, &contract)
        .map_err(AutonomousResearchMainAgentRoundError::NonProvider)?;
    if autonomous_research_stage_artifact_has_reviewable_content(resolved, job)
        && autonomous_research_stage_artifact_has_materialization_receipt(resolved, job, &contract)
    {
        let report_ref = relative_workspace_ref(&resolved.workspace_root, &stage_artifact_path);
        if !job
            .artifact_refs
            .iter()
            .any(|artifact| artifact == &report_ref)
        {
            job.artifact_refs.push(report_ref);
        }
    }
    merge_unique_strings(&mut job.artifact_refs, adopted_refs);
    let artifact_ref = relative_workspace_ref(&resolved.workspace_root, &artifact_path);
    let round_index = job.provider_rounds.len() + 1;
    let runtime_identity = agents::write_main_agent_runtime_identity(
        &resolved.data_dir,
        agents::MainAgentRuntimeIdentityRequest {
            job_id: job.job_id.clone(),
            session_id: session_id.clone(),
            round_index,
            provider_id: provider_trace.resolved_provider.clone(),
            model: provider_trace.resolved_model.clone(),
            artifact_ref: artifact_ref.clone(),
            lifecycle_status: "succeeded".to_string(),
            tool_scope: tool_defs
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<Vec<_>>(),
            created_at: job.created_at.clone(),
            updated_at: timestamp_string(),
        },
    )
    .map_err(|err| AutonomousResearchMainAgentRoundError::NonProvider(err.to_string()))?;
    Ok(Some(AutonomousResearchAgentRound {
        summary: AutonomousResearchAgentRoundSummary {
            round_index,
            artifact_path: artifact_ref,
            runtime_identity_ref: Some(runtime_identity.runtime_identity_ref),
            provider_id: provider_trace.resolved_provider,
            model: provider_trace.resolved_model,
            provider_route_source: route_selection.provider_source,
            execution_mode: "agent_loop".to_string(),
            iterations: agent_loop_result.iterations,
            tool_calls_made: agent_loop_result.tool_calls_made,
            finish_reason: agent_loop_result.finish_reason,
            content_sha256: sha256_hex(final_content.as_bytes()),
        },
        content: final_content,
    }))
}

pub(crate) fn render_autonomous_research_agent_prompt(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> String {
    let contract = autonomous_research_stage_contract(resolved, job);
    let stage_route_contract = render_autonomous_research_stage_route_contract(&contract.stage_id);
    let stage_plan_ref = relative_workspace_ref(
        &resolved.workspace_root,
        &autonomous_research_stage_plan_path(resolved, job, &contract),
    );
    let stage_plan = std::fs::read_to_string(resolved.workspace_root.join(&stage_plan_ref))
        .unwrap_or_else(|_| {
            "Stage plan has not been materialized yet; create it before requesting review."
                .to_string()
        });
    let automation_policy =
        crate::orchestration::GoalRunAutomationPolicyProjection::from_mode(job.automation_mode);
    let action_policies = automation_policy
        .action_policies
        .iter()
        .map(|policy| format!("- {}: {}", policy.action, policy.policy.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    let required_fields = contract
        .required_fields
        .iter()
        .map(|field| format!("- {field}"))
        .collect::<Vec<_>>()
        .join("\n");
    let pass_criteria = contract
        .pass_criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    let failure_signals = contract
        .failure_signals
        .iter()
        .map(|signal| format!("- {signal}"))
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_binding_contract =
        render_autonomous_research_stage_evidence_binding_contract(&contract);
    let worker_task_catalog = render_autonomous_research_worker_task_catalog(&contract);
    let paper_instruction = if contract.paper_allowed {
        format!(
            "Paper artifacts are allowed in this stage. Use the canonical report path `{}` only when the stage contract calls for paper planning, TeX writing, PDF compilation, or final hard review evidence.",
            job.report_path
        )
    } else {
        "Paper artifacts are not allowed in this stage. Do not write or revise the canonical paper yet; synthesize only the active stage artifact and supporting evidence.".to_string()
    };
    let review_feedback = job
        .last_review
        .as_ref()
        .map(|review| {
            format!(
                "Previous review verdict: {}\nPrevious review score: {:?}\nPrevious review feedback:\n{}\n",
                review.verdict, review.score, review.response_text
            )
        })
        .unwrap_or_else(|| "No previous review yet.\n".to_string());
    let tick_trace = job
        .tick_summaries
        .iter()
        .rev()
        .take(8)
        .map(|tick| {
            format!(
                "- tick {}: status={}, loop={}, dispatches={}, accepted={}, next={}",
                tick.tick_index,
                tick.status,
                tick.loop_status,
                tick.dispatch_count,
                tick.accepted,
                tick.next_recommended_action
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let artifacts = if job.artifact_refs.is_empty() {
        "none".to_string()
    } else {
        job.artifact_refs.join("\n")
    };
    let worker_evidence = autonomous_research_worker_evidence(resolved);
    let accepted_worker_evidence_index =
        load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let adoption_ready_candidates = render_autonomous_research_adoption_ready_candidates_for_prompt(
        resolved,
        &contract,
        job,
        accepted_worker_evidence_index.as_ref(),
    );
    let accepted_worker_evidence = render_autonomous_research_accepted_worker_evidence_index(
        accepted_worker_evidence_index.as_ref(),
    );
    let continuity_packet = render_autonomous_research_continuity_for_prompt(resolved, job);
    let recent_task_closure =
        render_autonomous_research_recent_task_closure_for_prompt(resolved, job);
    format!(
        "You are Astra's main autonomous research agent, running inside Astra's durable supervisor.\n\n\
         Goal: {prompt}\n\n\
         Job id: {job_id}\n\
         Project id: {project_id}\n\
         Workspace root: {workspace}\n\
         Current phase: {phase}\n\
         Automation mode: {automation_mode}\n\
         Review rounds completed: {review_rounds}\n\
         Active stage id: {stage_id}\n\
         Active stage class: {stage_class}\n\
         Stage artifact type: {artifact_type}\n\
         Stage artifact path: {stage_artifact_path}\n\
         Stage review strength: {review_strength}\n\
         Canonical paper path: {report_path}\n\n\
         Research DAG route contract:\n{stage_route_contract}\n\n\
         {continuity_packet}\n\n\
         Stage plan path: {stage_plan_ref}\n\
         Stage plan:\n{stage_plan}\n\n\
         Automation policy projection:\n{action_policies}\n\n\
         Stage objective:\n{stage_objective}\n\n\
         Required stage artifact fields:\n{required_fields}\n\n\
         Stage artifact evidence binding contract:\n{evidence_binding_contract}\n\n\
         Stage pass criteria:\n{pass_criteria}\n\n\
         Failure signals to avoid:\n{failure_signals}\n\n\
         Stage-local worker task catalog. Entries marked `adopted_stage_evidence_plan` are required evidence from your recorded plan; entries marked `advisory_default_not_gate` are suggestions only and do not define evidence sufficiency until you explicitly adopt them with `record_stage_evidence_plan`:\n{worker_task_catalog}\n\n\
         Existing board/agent-loop trace:\n{tick_trace}\n\n\
         Recent task closure packet, read-only projection from existing state:\n{recent_task_closure}\n\n\
         Current artifact refs:\n{artifacts}\n\n\
         Board-visible worker evidence:\n{worker_evidence}\n\n\
         Adoption-ready candidate artifacts that require main-agent decision:\n{adoption_ready_candidates}\n\n\
         Accepted stage-local worker evidence that must be cited by `{stage_artifact_path}`:\n{accepted_worker_evidence}\n\n\
         {review_feedback}\n\
         Operate autonomously within the selected policy. Your job is to decide, dispatch, inspect, accept/reject/defer candidate evidence, route obligations, request cleanup, and request review readiness. Do not ask the human for next steps in full-auto. Do not invent literature or experiment evidence. Do not fetch papers, run experiments, write code, patch files, or directly author stage artifacts as a substitute for agent-team evidence. If evidence is missing, publish or update concrete board-visible worker tasks, record a route/obligation decision, or keep the stage in repair.\n\n\
         Astra collaboration protocol: you are the only research-semantic authority in this loop. Runtime persists state, enforces schemas/tool scopes, dispatches your published tasks, records worker evidence, and projects status, but it must not invent research strategy, generate repair tasks, define required stage evidence, accept worker artifacts, decide route changes, or decide cleanup. Agent-team workers execute only your TaskPackets and return candidate evidence; they cannot publish board tasks, close obligations, change routes, request cleanup, or mutate canonical project口径. If worker evidence is incomplete, you must create/update/merge board-visible tasks yourself and then decide whether each candidate artifact is accepted, rejected, or deferred.\n\n\
         Field routing rule: put readable supporting context, failed reviews, accepted evidence, and current candidates in `input_artifact_refs`; put rejected or superseded worker artifacts only in `review_target_evidence_refs` as non-authoritative repair targets and require explicit quarantine or supersession; put the evidence or review this task targets in `review_target_evidence_refs`; for review artifacts use canonical refs like `review_packet:rev_xxx` or `review_trace:rev_xxx`, not a prefix plus a full path; put concrete failed-review findings in `review_findings_refs`; put the blocking obligation or blocked claim in `blocker_refs`; put upstream worker tasks in `depends_on_task_ids`; `depends_on_task_ids` orders dataflow but does not mount the accepted evidence bundle, so synthesis, artifact repair, or repair-task work that consumes accepted worker evidence must also cite `accepted_worker_evidence_task:*` or `accepted_worker_evidence_index:*` in `input_artifact_refs` or `review_target_evidence_refs`; put only already-existing external canonical prerequisites in `required_canonical_artifacts`. Never put `{stage_artifact_path}` in `required_canonical_artifacts` for a task that produces or repairs `{stage_artifact_path}`. `stage_closure_ledger:*` refs are main-agent-only ledger coordinates; never copy them into board-task ref arrays. Convert the concrete ledger issue into objective/acceptance_checks and, when a blocker id is needed, use `blocker_refs` with a concrete `stage_closure_blocker:*` ref. For `adopt_stage_artifact`, always include `replacement_of_artifact_ids`: [] for first adoption at the target path, or the active artifact ids from CanonicalArtifactLedger when replacing an existing canonical artifact.\n\n\
         Structured authority tools: before stage evidence coverage can be judged, read any standard-setting evidence first, then call `record_stage_evidence_plan` to adopt or raise the concrete evidence requirements for this stage from that evidence and your research judgment. The standard-setting worker only proposes a `CandidateStageEvidencePlan`; it does not decide adoption. When you create or repair agent-team work, call `publish_board_tasks` with explicit worker roles, objectives, required outputs, acceptance checks, failure signals, dependencies, and evidence standards. If a task verifies, audits, clusters, compares, extracts from, synthesizes, or otherwise consumes another worker's evidence, include the upstream board task ids in `depends_on_task_ids` and cite them in `input_artifact_refs`; do not publish implicit dependency batches. After an evidence-gap failed review has accepted post-review worker evidence, publish synthesis, artifact repair, or repair-task work with `accepted_worker_evidence_task:*` or `accepted_worker_evidence_index:*` refs, not only review summaries or task ids. Copy active job, stage, task, and evidence refs exactly from the projected context; stale `auto_research_job` refs are rejected. If existing board work is close but incomplete, call `update_board_task` instead of creating a duplicate; if tasks overlap, call `merge_board_tasks` so only the canonical target remains dispatchable. Worker worktree artifacts are candidate-only: inspect their refs, then call `record_worker_artifact_decision` to accept, reject, or defer them with rationale, review requirement, cleanup requirement, and readiness refs. An accepted decision records your research judgment; it does not automatically apply patches or mutate canonical project files. Local evidence tasks such as method comparison, source verification, and open-problem extraction must not be adopted directly as `{stage_artifact_path}`; after accepting those fragments, publish or update a `stage artifact synthesis` task for worker_role=`research_synthesizer` with explicit input_artifact_refs to the accepted evidence, accept that synthesis candidate, and only then call `adopt_stage_artifact`. When accepted research_synthesizer evidence contains a reviewable candidate artifact that should become `{stage_artifact_path}`, call `adopt_stage_artifact` with the accepted source agent, source ref or worktree candidate ref, source artifact path, target artifact path, rationale, and evidence refs; runtime validates the raw candidate body before appending provenance, so a repair memo with candidate-only/retention/preflight/adoption handoff wording is not adoptable. If a baseline-visible canonical artifact has concrete worker evidence for import, execution, test, or smoke-check pass/fail, call `record_canonical_artifact_integration_check` with the canonical target path, command, status, detail, and exact evidence refs; this records integration state but does not close the stage. If the strongest worker output is a repair memo rather than a clean canonical artifact body, publish or update synthesis work to produce the clean body before adoption. When you choose a route for a blocking obligation, call `record_obligation_decision` with the obligation id, route, rationale, task refs, evidence standard, project口径-change flag, cleanup requirement, and readiness refs. Use `request_review_rerun` only after the cited readiness refs exist. A passing review is not stage closure: before advancing, call `record_stage_closure_decision` to state whether this stage should `close_and_advance`, `continue_stage`, `rollback_or_pivot`, or `human_gate`, and include the target stage, accepted evidence refs, passing review ref, stage artifact ref, remaining risks, cleanup judgment, and why no more stage work is needed. Only after `record_stage_closure_decision(decision=\"close_and_advance\")` may you call `request_route_change(operation=\"advance\")`; route rationale alone is not a closure decision. Use `request_route_change` for rollback, pivot, fork, supersede, abandon, or advance decisions that change the research route. Use `request_cleanup_plan` whenever a route or project口径 change invalidates prior artifacts. Prose-only plans do not publish board work, do not adopt worker artifacts, do not close obligations, and do not close stages.\n\n\
         Obligation protocol: for every active blocking obligation above, explicitly decide one of: create/update a generic board task for agent-team work, merge it into an existing board task, satisfy it with accepted evidence, request rollback/pivot/supersede plus cleanup, request a human gate, or reject it with a grounded rationale. The runtime provides obligations as facts only; you own research task dispatch and must not treat a review failure as optional text. If the chosen route needs worker evidence, use `publish_board_tasks` or `update_board_task`; if it removes duplicate work, use `merge_board_tasks`; if it changes strategy or review readiness, use `record_obligation_decision` and the matching request tool.\n\n\
         Artifact repair task contract: if you publish an `artifact_repair` task for this stage, `task_type` may be `artifact_repair`, but `required_output_artifact_type` must be `{artifact_type}`. Do not set `required_output_artifact_type` to `artifact_repair`; that prevents the worker artifact from becoming accepted stage evidence or an adoption-ready candidate. Failed-review repair tasks must be surgical: cite the review/finding in `review_findings_refs`, bind the blocker or replaced task, and make `acceptance_checks` state exactly what artifact claim, citation mapping, source classification, or row-level evidence must change. For required worker-slot repair, do not encode lifecycle state in `task_type` or append repair wording to a standard `research_stage_task::<stage_execution_id>::<slot>` id. Keep both the canonical required task type and canonical slot task id, call `update_board_task` on that durable slot, and encode repair lineage in the existing reference fields.\n\n\
         MainAgentObligationDecisionContract requirement: acknowledgement is not closure. For every failed_stage_review obligation, call `record_obligation_decision` and also write or update a visible decision block before prose repair. Use this exact heading and fields so the supervisor can audit your decision without making the research decision for you:\n\
         ## MainAgentObligationDecisionContract\n\
         - obligation id: `<id>`\n\
         - source review id: `<review id>`\n\
         - failure class: `<class>`\n\
         - root-cause diagnosis: `<why previous work failed>`\n\
         - strategy change: `<what changes now compared with previous attempts>`\n\
         - chosen route: `<board task|merge existing task|rollback|pivot|supersede|cleanup|human gate|rejected with rationale>`\n\
         - concrete task/evidence plan: `<task ids, worker roles, or evidence path>`\n\
         - evidence standard: `<what accepted evidence or review pass will close it>`\n\
         - project口径 change: `<yes|no and why>`\n\
         - cleanup requirement: `<required|not required and why>`\n\
         - next review condition: `<what must be true before review reruns>`\n\n\
         Do not close a failed review by only writing another stage summary. If the reviewer requested strategy pivot, closest-family expansion, full-paper extraction, or differentiated analysis, make that route explicit in the decision contract and then create or update board-visible work through the existing task pool protocol.\n\n\
         {paper_instruction}\n\n\
         This is not a chat-only task. Before finishing this round, use structured authority tools to move the research loop: publish/update/merge board tasks, record worker artifact decisions, adopt a selected accepted worker candidate into the stage artifact, record obligation decisions, request route changes, request cleanup, or request review rerun when readiness refs exist. Do not directly write or patch `{stage_artifact_path}`. If the previous review failed, convert each required repair into board-visible worker work or an explicit route/cleanup/adoption decision. Do not return a plan-only response.\n\n\
         The supervisor will review `{stage_artifact_path}` against the active stage contract. Review pass is only evidence. To end a non-final stage, you must first record `record_stage_closure_decision(decision=\"close_and_advance\")`, then request `advance` to the current `default_next_stage_id_for_advance` named in the Research DAG route contract above.",
        prompt = job.prompt,
        job_id = job.job_id,
        project_id = resolved.project_id,
        workspace = resolved.workspace_root.display(),
        phase = job.phase,
        automation_mode = job.automation_mode.as_str(),
        review_rounds = job.review_rounds_completed,
        stage_id = contract.stage_id,
        stage_class = contract.stage_class,
        artifact_type = contract.artifact_type,
        stage_artifact_path = contract.artifact_path,
        review_strength = contract.review_strength,
        report_path = job.report_path,
        stage_route_contract = stage_route_contract,
        continuity_packet = continuity_packet,
        stage_plan_ref = stage_plan_ref,
        stage_plan = truncate_for_prompt(&stage_plan, 8_000),
        action_policies = if action_policies.is_empty() { "none".to_string() } else { action_policies },
        stage_objective = contract.objective,
        required_fields = required_fields,
        evidence_binding_contract = evidence_binding_contract,
        pass_criteria = pass_criteria,
        failure_signals = failure_signals,
        worker_task_catalog = worker_task_catalog,
        tick_trace = if tick_trace.is_empty() { "none".to_string() } else { tick_trace },
        recent_task_closure = recent_task_closure,
        artifacts = artifacts,
        worker_evidence = worker_evidence,
        adoption_ready_candidates = adoption_ready_candidates,
        accepted_worker_evidence = accepted_worker_evidence,
        review_feedback = review_feedback,
        paper_instruction = paper_instruction
    )
}

pub(crate) fn render_autonomous_research_recent_task_closure_for_prompt(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> String {
    let contract = autonomous_research_stage_contract(resolved, job);
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let accepted_count = accepted_worker_evidence_current_gate_entry_count(index.as_ref());
    let accepted_task_types = accepted_worker_evidence_current_gate_task_types(index.as_ref());
    let missing_task_types =
        accepted_worker_evidence_missing_required_task_types(&contract, index.as_ref());
    let adoption_ready_candidates =
        autonomous_research_adoption_ready_candidates(resolved, &contract, job, index.as_ref());
    let missing_synthesis_bridge =
        accepted_worker_evidence_missing_adoptable_stage_synthesis_reason(
            &contract,
            index.as_ref(),
        );
    let stage_artifact_reviewable =
        autonomous_research_stage_artifact_has_reviewable_content(resolved, job);
    let latest_tick = job
        .tick_summaries
        .last()
        .map(|tick| {
            format!(
                "tick={} status={} loop={} dispatches={} accepted={} next={}",
                tick.tick_index,
                tick.status,
                tick.loop_status,
                tick.dispatch_count,
                tick.accepted,
                tick.next_recommended_action
            )
        })
        .unwrap_or_else(|| "none".to_string());
    let next_action = autonomous_research_recent_task_closure_next_action(
        accepted_count,
        adoption_ready_candidates.len(),
        &missing_task_types,
        missing_synthesis_bridge.as_deref(),
        stage_artifact_reviewable,
    );
    format!(
        "- fact_source: `board task files, task packets, worker manifests, provider evidence, accepted worker evidence index, artifact adoption records`\n\
         - stage_id: `{}`\n\
         - stage_execution_id: `{}`\n\
         - target_stage_artifact: `{}`\n\
         - latest_tick: `{}`\n\
         - accepted_current_evidence_count: `{}`\n\
         - accepted_current_task_types: `{}`\n\
         - missing_required_task_types: `{}`\n\
         - adoption_ready_candidate_count: `{}`\n\
         - stage_artifact_reviewable_shape: `{}`\n\
         - missing_synthesis_bridge: `{}`\n\
         - next_admissible_action: `{}`\n\
         - projection_rule: `read_only_summary; do not treat this as a second ledger or scheduler`",
        contract.stage_id,
        job.stage_execution_id.as_deref().unwrap_or("unknown"),
        contract.artifact_path,
        latest_tick,
        accepted_count,
        comma_or_none_runtime(&accepted_task_types),
        comma_or_none_runtime(&missing_task_types),
        adoption_ready_candidates.len(),
        stage_artifact_reviewable,
        missing_synthesis_bridge
            .as_deref()
            .unwrap_or("none"),
        next_action
    )
}

#[allow(dead_code)]
pub(crate) fn render_autonomous_research_single_task_closure_trace(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    task_id: &str,
) -> String {
    let contract = autonomous_research_stage_contract(resolved, job);
    let board_task_path = resolved
        .data_dir
        .join("main-agent-board")
        .join("tasks")
        .join(format!("{}.json", sanitize_file_component_runtime(task_id)));
    let board_task_value = read_json_file_runtime(&board_task_path).ok();
    let board_task_status = board_task_value
        .as_ref()
        .and_then(|value| json_string_field_runtime(value, "status"))
        .unwrap_or_else(|| {
            if board_task_path.exists() {
                "published".to_string()
            } else {
                "not_found".to_string()
            }
        });
    let index = load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let accepted_entry = index.as_ref().and_then(|index| {
        index
            .entries
            .iter()
            .find(|entry| entry.task_id == task_id)
            .or_else(|| {
                index.entries.iter().find(|entry| {
                    entry
                        .evidence_refs
                        .iter()
                        .any(|reference| reference.contains(task_id))
                })
            })
    });
    let worker_agent_id = accepted_entry
        .map(|entry| entry.agent_id.as_str())
        .unwrap_or("not_found");
    let task_packet_ref = accepted_entry
        .map(|entry| entry.task_packet_ref.as_str())
        .unwrap_or("not_found");
    let output_manifest_ref = accepted_entry
        .map(|entry| entry.output_manifest_ref.as_str())
        .unwrap_or("not_found");
    let provider_evidence_refs = accepted_entry
        .map(|entry| {
            entry
                .evidence_refs
                .iter()
                .filter(|reference| {
                    reference.contains("provider_worker_evidence")
                        || reference.contains("worker_evidence")
                        || reference.ends_with(".md")
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let accepted_status = accepted_entry
        .and_then(|entry| entry.active_status.as_deref())
        .unwrap_or(if accepted_entry.is_some() {
            "accepted"
        } else {
            "not_found"
        });
    let decision = load_main_agent_worker_artifact_decision_records(resolved)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .find(|record| {
            record.task_id == task_id
                || accepted_entry.is_some_and(|entry| record.agent_id == entry.agent_id)
        });
    let adoption = load_main_agent_stage_artifact_adoption_records(resolved)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .find(|record| {
            record.source_task_id.as_deref() == Some(task_id)
                || accepted_entry.is_some_and(|entry| record.source_agent_id == entry.agent_id)
        });
    let stage_artifact_reviewable_shape = adoption
        .as_ref()
        .filter(|record| record.target_artifact_path == contract.artifact_path)
        .map(|_| autonomous_research_stage_artifact_has_reviewable_content(resolved, job))
        .map(|reviewable| reviewable.to_string())
        .unwrap_or_else(|| "not_targeted_by_task".to_string());
    let mut source_files = Vec::new();
    source_files.push(render_task_closure_trace_source_file(
        resolved,
        "board_task_ref",
        &relative_workspace_ref(&resolved.workspace_root, &board_task_path),
    ));
    if task_packet_ref != "not_found" {
        source_files.push(render_task_closure_trace_source_file(
            resolved,
            "task_packet_ref",
            task_packet_ref,
        ));
    }
    if output_manifest_ref != "not_found" {
        source_files.push(render_task_closure_trace_source_file(
            resolved,
            "output_manifest_ref",
            output_manifest_ref,
        ));
    }
    for reference in &provider_evidence_refs {
        source_files.push(render_task_closure_trace_source_file(
            resolved,
            "provider_worker_evidence",
            reference,
        ));
    }
    let decision_ref = decision
        .as_ref()
        .map(|record| record.decision_ref.as_str())
        .unwrap_or("not_found");
    let adoption_ref = adoption
        .as_ref()
        .map(|record| record.adoption_ref.as_str())
        .unwrap_or("not_found");
    let mut flags = Vec::new();
    if board_task_status == "not_found" {
        flags.push("missing_board_task");
    }
    if accepted_entry.is_none() {
        flags.push("missing_accepted_worker_evidence");
    }
    if task_packet_ref == "not_found" {
        flags.push("missing_task_packet_ref");
    }
    if output_manifest_ref == "not_found" {
        flags.push("missing_output_manifest_ref");
    }
    if provider_evidence_refs.is_empty() {
        flags.push("missing_provider_evidence_refs");
    }
    if decision.is_none() {
        flags.push("missing_main_agent_decision");
    }
    if adoption.is_none() {
        flags.push("missing_stage_artifact_adoption");
    }
    format!(
        "- trace_scope: `single_task_closure`\n\
         - projection_rule: `read_only_existing_state; not_a_ledger; not_a_scheduler`\n\
         - task_id: `{}`\n\
         - stage_id: `{}`\n\
         - stage_execution_id: `{}`\n\
         - board_task_ref: `{}`\n\
         - board_task_status: `{}`\n\
         - accepted_worker_evidence_status: `{}`\n\
         - worker_agent_id: `{}`\n\
         - task_packet_ref: `{}`\n\
         - output_manifest_ref: `{}`\n\
         - provider_worker_evidence_refs: `{}`\n\
         - main_agent_worker_artifact_decision_ref: `{}`\n\
         - stage_artifact_adoption_ref: `{}`\n\
         - stage_artifact_reviewable_shape: `{}`\n\
         - source_files: `{}`\n\
         - information_loss_flags: `{}`",
        task_id,
        contract.stage_id,
        job.stage_execution_id.as_deref().unwrap_or("unknown"),
        relative_workspace_ref(&resolved.workspace_root, &board_task_path),
        board_task_status,
        accepted_status,
        worker_agent_id,
        task_packet_ref,
        output_manifest_ref,
        comma_or_none_runtime(&provider_evidence_refs),
        decision_ref,
        adoption_ref,
        stage_artifact_reviewable_shape,
        if source_files.is_empty() {
            "none".to_string()
        } else {
            source_files.join("; ")
        },
        if flags.is_empty() {
            "none".to_string()
        } else {
            flags.join(", ")
        }
    )
}

#[allow(dead_code)]
fn render_task_closure_trace_source_file(
    resolved: &ResolvedProject,
    label: &str,
    reference: &str,
) -> String {
    let path = resolve_adoption_reference_path(resolved, reference);
    format!("{label}:{}:exists={}", reference, path.exists())
}

fn autonomous_research_recent_task_closure_next_action(
    accepted_count: usize,
    adoption_ready_count: usize,
    missing_task_types: &[String],
    missing_synthesis_bridge: Option<&str>,
    stage_artifact_reviewable: bool,
) -> &'static str {
    if adoption_ready_count > 0 {
        return "inspect adoption-ready candidate, record worker artifact decision, then call adopt_stage_artifact or reject/defer";
    }
    if !missing_task_types.is_empty() {
        return "publish or update board-visible worker tasks for missing required evidence";
    }
    if accepted_count > 0 && missing_synthesis_bridge.is_some() {
        return "publish or update research_synthesizer stage artifact synthesis task using accepted evidence refs";
    }
    if !stage_artifact_reviewable {
        return "repair or synthesize a clean stage artifact candidate before requesting review";
    }
    "request review rerun if readiness refs exist, otherwise record the remaining decision through structured tools"
}

pub(crate) fn render_autonomous_research_stage_route_contract(stage_id: &str) -> String {
    let stage_map = research::stage_execution_map();
    let route = stage_map
        .entries
        .iter()
        .map(|entry| {
            let next = entry.default_next_stage_id.as_deref().unwrap_or("none");
            format!("{} -> {}", entry.stage_id, next)
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let Some(entry) = stage_map
        .entries
        .iter()
        .find(|entry| entry.stage_id == stage_id)
    else {
        return format!(
            "- current_stage_id: `{stage_id}`\n\
             - default_next_stage_id_for_advance: `unknown`\n\
             - allowed_operations: `unknown`\n\
             - advance target rule: do not request `advance` until the stage is present in StageExecutionMap.\n\
             - default route reference: {route}"
        );
    };
    let default_next = entry.default_next_stage_id.as_deref().unwrap_or("none");
    let allowed_operations = if entry.allowed_operations.is_empty() {
        "none".to_string()
    } else {
        entry.allowed_operations.join(", ")
    };
    format!(
        "- current_stage_id: `{}`\n\
         - current_stage_class: `{}`\n\
         - default_next_stage_id_for_advance: `{}`\n\
         - allowed_operations: `{}`\n\
         - advance target rule: if you call `request_route_change` with `operation=\"advance\"`, `target_stage_id` MUST be exactly `{}`. Do not skip ahead to later stages such as `paper-write` unless that exact stage is the current default next stage.\n\
         - cleanup rule: if a route changes the active project口径 or invalidates canonical artifacts, set `cleanup_required=true` and request cleanup before route mutation.\n\
         - default route reference: {}",
        entry.stage_id,
        entry.stage_class,
        default_next,
        allowed_operations,
        default_next,
        route
    )
}

pub(crate) fn ensure_autonomous_research_stage_delivery_artifacts(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<(), String> {
    match contract.stage_id.as_str() {
        "paper-write" => ensure_autonomous_research_paper_tex(resolved, job, contract),
        "paper-compile" => ensure_autonomous_research_compiled_pdf_bundle(resolved, job, contract),
        "research-review" => ensure_autonomous_research_hard_review_packet(resolved, job, contract),
        _ => Ok(()),
    }
}

pub(crate) fn ensure_autonomous_research_paper_tex(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<(), String> {
    let tex_path = resolved.workspace_root.join(&contract.artifact_path);
    let current = std::fs::read_to_string(&tex_path).unwrap_or_default();
    if current.contains("\\documentclass") && current.contains("\\begin{document}") {
        return Ok(());
    }
    let sanitized_goal = latex_escape(&job.prompt);
    let artifact_refs = if job.artifact_refs.is_empty() {
        "No accepted artifact refs were recorded yet.".to_string()
    } else {
        job.artifact_refs
            .iter()
            .map(|artifact| format!("\\item \\texttt{{{}}}", latex_escape(artifact)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tex = format!(
        "\\documentclass[11pt]{{article}}\n\
         \\usepackage[margin=1in]{{geometry}}\n\
         \\usepackage{{hyperref}}\n\
         \\usepackage{{booktabs}}\n\
         \\title{{Astra Autonomous Research Result}}\n\
         \\author{{Astra Research Agent}}\n\
         \\date{{}}\n\
         \\begin{{document}}\n\
         \\maketitle\n\
         \\begin{{abstract}}\n\
         This TeX source is the canonical paper-stage artifact for Astra job \\texttt{{{job_id}}}. It must remain tied to accepted literature, implementation, experiment, and claim artifacts before final review may pass.\n\
         \\end{{abstract}}\n\
         \\section{{Research Goal}}\n\
         {goal}\n\
         \\section{{Evidence Contract}}\n\
         The final paper must cite accepted evidence from the Astra stage DAG. Current artifact refs:\n\
         \\begin{{itemize}}\n\
         {artifact_refs}\n\
         \\end{{itemize}}\n\
         \\section{{Method And Results}}\n\
         This section is intentionally conservative until result-to-claim has accepted the supported claim table. Unsupported claims must be deleted or narrowed before final hard review.\n\
         \\section{{Limitations}}\n\
         Missing evidence, failed runs, unavailable sources, and unsupported claims remain explicit limitations until repaired by stage-local tasks.\n\
         \\section{{Reproducibility}}\n\
         The source bundle must include TeX, build logs, implementation manifests, run manifests, experiment reports, and the claim table.\n\
         \\end{{document}}\n",
        job_id = latex_escape(&job.job_id),
        goal = sanitized_goal,
        artifact_refs = artifact_refs
    );
    write_text_atomic_runtime(&tex_path, &tex)
}

pub(crate) fn ensure_autonomous_research_compiled_pdf_bundle(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<(), String> {
    let tex_contract =
        autonomous_research_stage_artifact_path(job, "paper-write", "paper_tex_bundle");
    let tex_path = resolved.workspace_root.join(&tex_contract);
    if !tex_path.exists() {
        let advisory_worker_task_types = autonomous_research_stage_worker_task_types("paper-write")
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let tex_contract = AutonomousResearchStageContract {
            stage_id: "paper-write".to_string(),
            stage_class: "document".to_string(),
            artifact_type: "paper_tex_bundle".to_string(),
            artifact_path: autonomous_research_stage_artifact_path(
                job,
                "paper-write",
                "paper_tex_bundle",
            ),
            review_strength: "standard_stage_review".to_string(),
            objective: autonomous_research_stage_objective("paper-write").to_string(),
            required_fields: autonomous_research_stage_required_fields("paper-write")
                .into_iter()
                .map(str::to_string)
                .collect(),
            pass_criteria: autonomous_research_stage_pass_criteria("paper-write")
                .into_iter()
                .map(str::to_string)
                .collect(),
            failure_signals: autonomous_research_stage_failure_signals("paper-write")
                .into_iter()
                .map(str::to_string)
                .collect(),
            worker_task_types: Vec::new(),
            worker_task_requirements: Vec::new(),
            advisory_worker_task_types,
            evidence_plan_status: "missing".to_string(),
            evidence_plan_ref: None,
            paper_allowed: true,
            final_completion_stage: false,
        };
        ensure_autonomous_research_paper_tex(resolved, job, &tex_contract)?;
    }
    let bundle_path = resolved.workspace_root.join(&contract.artifact_path);
    let build_dir = bundle_path
        .parent()
        .unwrap_or(resolved.workspace_root.as_path())
        .join("build");
    std::fs::create_dir_all(&build_dir).map_err(|err| err.to_string())?;
    let pdf_path = build_dir.join("main.pdf");
    let log_path = build_dir.join("paper-build.log");
    let build_result = run_latex_build(&resolved.workspace_root, &tex_path, &build_dir);
    let build_log = format!(
        "build command: {}\nstatus: {}\nstdout:\n{}\n\nstderr:\n{}\n",
        build_result.command, build_result.status, build_result.stdout, build_result.stderr
    );
    write_text_atomic_runtime(&log_path, &build_log)?;
    let unresolved = if build_log
        .to_ascii_lowercase()
        .contains("undefined references")
        || build_log
            .to_ascii_lowercase()
            .contains("undefined citation")
    {
        "warnings present"
    } else {
        "none detected"
    };
    let pdf_ref = relative_workspace_ref(&resolved.workspace_root, &pdf_path);
    let log_ref = relative_workspace_ref(&resolved.workspace_root, &log_path);
    let tex_ref = relative_workspace_ref(&resolved.workspace_root, &tex_path);
    let bundle = format!(
        "# Astra Stage Artifact: compiled_pdf_bundle\n\n\
         ## Stage\n\n\
         - stage_id: `paper-compile`\n\
         - job_id: `{}`\n\n\
         ## PDF path\n\n\
         `{}`\n\n\
         ## Source bundle ref\n\n\
         `{}`\n\n\
         ## Build command\n\n\
         `{}`\n\n\
         ## Build log\n\n\
         `{}`\n\n\
         ## Unresolved references or warnings\n\n\
         {}\n\n\
         ## PDF validation result\n\n\
         {}\n",
        job.job_id,
        pdf_ref,
        tex_ref,
        build_result.command,
        log_ref,
        unresolved,
        if pdf_path.exists() && build_result.status == "success" {
            "pass"
        } else {
            "fail: PDF is missing or LaTeX build did not complete"
        }
    );
    write_autonomous_research_report(&bundle_path, &bundle).map_err(|err| err.to_string())
}

#[derive(Debug, Clone)]
pub(in crate::runtime) struct LatexBuildResult {
    command: String,
    status: String,
    stdout: String,
    stderr: String,
}

pub(in crate::runtime) fn run_latex_build(
    workspace_root: &Path,
    tex_path: &Path,
    build_dir: &Path,
) -> LatexBuildResult {
    let tex_file = tex_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("main.tex")
        .to_string();
    let tex_parent = tex_path.parent().unwrap_or(workspace_root);
    let latexmk = Command::new("latexmk")
        .arg("-pdf")
        .arg("-interaction=nonstopmode")
        .arg("-halt-on-error")
        .arg("-outdir")
        .arg(build_dir)
        .arg(&tex_file)
        .current_dir(tex_parent)
        .output();
    match latexmk {
        Ok(output) => LatexBuildResult {
            command: format!(
                "latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir {} {}",
                build_dir.display(),
                tex_file
            ),
            status: if output.status.success() {
                "success".to_string()
            } else {
                "failed".to_string()
            },
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        Err(latexmk_err) => {
            let pdflatex = Command::new("pdflatex")
                .arg("-interaction=nonstopmode")
                .arg("-halt-on-error")
                .arg("-output-directory")
                .arg(build_dir)
                .arg(&tex_file)
                .current_dir(tex_parent)
                .output();
            match pdflatex {
                Ok(output) => LatexBuildResult {
                    command: format!(
                        "pdflatex -interaction=nonstopmode -halt-on-error -output-directory {} {}",
                        build_dir.display(),
                        tex_file
                    ),
                    status: if output.status.success() {
                        "success".to_string()
                    } else {
                        "failed".to_string()
                    },
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                },
                Err(pdflatex_err) => LatexBuildResult {
                    command: "latexmk or pdflatex".to_string(),
                    status: "failed".to_string(),
                    stdout: String::new(),
                    stderr: format!(
                        "latexmk unavailable: {latexmk_err}; pdflatex unavailable: {pdflatex_err}"
                    ),
                },
            }
        }
    }
}

pub(crate) fn ensure_autonomous_research_hard_review_packet(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<(), String> {
    let packet_path = resolved.workspace_root.join(&contract.artifact_path);
    let tex_ref = autonomous_research_stage_artifact_path(job, "paper-write", "paper_tex_bundle");
    let pdf_bundle_ref =
        autonomous_research_stage_artifact_path(job, "paper-compile", "compiled_pdf_bundle");
    let packet = format!(
        "# Astra Stage Artifact: hard_review_packet\n\n\
         ## Stage\n\n\
         - stage_id: `research-review`\n\
         - job_id: `{}`\n\n\
         ## PDF and TeX refs\n\n\
         - TeX: `{}`\n\
         - compiled PDF bundle: `{}`\n\n\
         ## Claim table ref\n\n\
         `{}`\n\n\
         ## Literature matrix ref\n\n\
         `{}`\n\n\
         ## Experiment report ref\n\n\
         `{}`\n\n\
         ## Implementation manifest ref\n\n\
         `{}`\n\n\
         ## Reproducibility audit\n\n\
         Inspect build logs, source bundle, implementation manifest, run manifest, raw outputs, and metric parsing commands before pass.\n\n\
         ## Novelty audit\n\n\
         Inspect literature matrix and novelty report for closest prior overlap and unsupported novelty statements.\n\n\
         ## Verdict, score, findings, repair routes\n\n\
         Final hard review must fail unless TeX, PDF, claim table, literature matrix, experiment report, implementation manifest, and cleanup plan are present and consistent.\n",
        job.job_id,
        tex_ref,
        pdf_bundle_ref,
        autonomous_research_stage_artifact_path(job, "result-to-claim", "claim_table"),
        autonomous_research_stage_artifact_path(job, "literature", "literature_matrix"),
        autonomous_research_stage_artifact_path(job, "monitor", "experiment_report"),
        autonomous_research_stage_artifact_path(job, "implement-solution", "implementation_manifest")
    );
    write_autonomous_research_report(&packet_path, &packet).map_err(|err| err.to_string())
}

pub(crate) fn latex_escape(value: &str) -> String {
    value
        .replace('\\', "\\textbackslash{}")
        .replace('&', "\\&")
        .replace('%', "\\%")
        .replace('$', "\\$")
        .replace('#', "\\#")
        .replace('_', "\\_")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('~', "\\textasciitilde{}")
        .replace('^', "\\textasciicircum{}")
}

#[derive(Debug, Clone)]
pub(in crate::runtime) struct AutonomousResearchDocFrameMarkdown {
    title: String,
    doc_type: String,
    lifecycle: String,
    summary: String,
    key_claims: Vec<String>,
    decisions: Vec<String>,
    interfaces: Vec<String>,
    evidence_refs: Vec<String>,
    next_actions: Vec<String>,
    non_goals: Vec<String>,
}

pub(in crate::runtime) fn autonomous_research_doc_frame_yaml(
    source_path: &str,
    frame: &AutonomousResearchDocFrameMarkdown,
) -> String {
    format!(
        "doc_frame:\n\
           doc_id: {}\n\
           schema_version: autonomous_research_doc_frame.v1\n\
           source_path: {}\n\
           title: {}\n\
           doc_type: {}\n\
           lifecycle: {}\n\
           scope: project\n\
           summary: {}\n\
           key_claims:\n{}\n\
           decisions:\n{}\n\
           interfaces:\n{}\n\
           evidence_refs:\n{}\n\
           next_actions:\n{}\n\
           non_goals:\n{}\n\
           generated_by: astra_autonomous_research_runtime\n\
           updated_at: {}",
        autonomous_research_doc_id_from_path(source_path),
        runtime_yaml_scalar(source_path),
        runtime_yaml_scalar(&frame.title),
        runtime_yaml_scalar(&frame.doc_type),
        runtime_yaml_scalar(&frame.lifecycle),
        runtime_yaml_scalar(&frame.summary),
        runtime_yaml_list(&frame.key_claims),
        runtime_yaml_list(&frame.decisions),
        runtime_yaml_list(&frame.interfaces),
        runtime_yaml_list(&frame.evidence_refs),
        runtime_yaml_list(&frame.next_actions),
        runtime_yaml_list(&frame.non_goals),
        timestamp_string()
    )
}

pub(in crate::runtime) fn write_autonomous_research_markdown_with_doc_frame(
    resolved: &ResolvedProject,
    path: &Path,
    frame: AutonomousResearchDocFrameMarkdown,
    body: &str,
) -> Result<String, String> {
    let source_path = relative_workspace_ref(&resolved.workspace_root, path);
    let body = strip_markdown_front_matter(body).trim_start();
    let contents = format!(
        "---\n{}\n---\n\n{}",
        autonomous_research_doc_frame_yaml(&source_path, &frame),
        body
    );
    write_text_atomic_runtime(path, &contents)?;
    Ok(source_path)
}

pub(crate) fn autonomous_research_doc_has_valid_doc_frame(
    resolved: &ResolvedProject,
    relative_path: &str,
) -> (bool, String) {
    match docs::inspect(&resolved.workspace_root, relative_path) {
        Ok(inspection) if inspection.status == "valid" => (true, "valid".to_string()),
        Ok(inspection) => (
            false,
            format!(
                "{}: {}",
                inspection.status,
                if inspection.issues.is_empty() {
                    "DocFrame not valid".to_string()
                } else {
                    inspection.issues.join("; ")
                }
            ),
        ),
        Err(err) => (false, err.to_string()),
    }
}

pub(crate) fn strip_markdown_front_matter(contents: &str) -> &str {
    let Some(stripped) = contents.strip_prefix("---\n") else {
        return contents;
    };
    let Some(end) = stripped.find("\n---") else {
        return contents;
    };
    stripped[end + "\n---".len()..]
        .strip_prefix('\n')
        .unwrap_or(&stripped[end + "\n---".len()..])
}

pub(crate) fn runtime_yaml_list(values: &[String]) -> String {
    if values.is_empty() {
        return "    - none".to_string();
    }
    values
        .iter()
        .map(|value| format!("    - {}", runtime_yaml_scalar(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn runtime_yaml_scalar(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace(':', " -")
        .trim()
        .to_string()
}

pub(crate) fn autonomous_research_doc_id_from_path(source_path: &str) -> String {
    let id = source_path
        .trim_end_matches(".md")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '.' })
        .collect::<String>()
        .trim_matches('.')
        .to_string();
    if id.is_empty() {
        "autonomous.research.document".to_string()
    } else {
        id
    }
}

pub(crate) fn autonomous_research_stage_rubric_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> PathBuf {
    resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(&job.job_id)
        .join(&contract.stage_id)
        .join("stage_acceptance_rubric.md")
}

pub(crate) fn autonomous_research_stage_plan_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> PathBuf {
    resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(&job.job_id)
        .join(&contract.stage_id)
        .join("stage_plan.md")
}

pub(crate) fn ensure_autonomous_research_review_support_docframes(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<(String, String), String> {
    let stage_plan_ref = ensure_autonomous_research_stage_plan_docframe(resolved, job, contract)?;
    let rubric_ref =
        ensure_autonomous_research_stage_rubric_docframe(resolved, job, contract, &stage_plan_ref)?;
    Ok((stage_plan_ref, rubric_ref))
}

pub(crate) fn ensure_autonomous_research_stage_plan_docframe(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
) -> Result<String, String> {
    let path = autonomous_research_stage_plan_path(resolved, job, contract);
    let worker_tasks = if !contract.worker_task_requirements.is_empty() {
        contract
            .worker_task_requirements
            .iter()
            .map(|requirement| {
                format!(
                    "- required by adopted evidence plan: `{}` delegated through the generic goal task pool",
                    requirement.task_type
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        contract
            .advisory_worker_task_types
            .iter()
            .map(|task| format!("- advisory default only, not a gate: `{task}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let required_fields = contract
        .required_fields
        .iter()
        .map(|field| format!("- {field}"))
        .collect::<Vec<_>>()
        .join("\n");
    let pass_criteria = contract
        .pass_criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    let failure_routes = autonomous_research_stage_failure_route_notes(contract);
    let evidence_binding_contract =
        render_autonomous_research_stage_evidence_binding_contract(contract);
    let body = format!(
        "# Stage Plan\n\n\
         ## Stage Identity\n\n\
         - job_id: `{}`\n\
         - stage_id: `{}`\n\
         - stage_class: `{}`\n\
         - artifact_type: `{}`\n\
         - artifact_path: `{}`\n\n\
         ## Stage Objective\n\n\
         {}\n\n\
         ## Delegated Work\n\n\
         - evidence_plan_status: `{}`\n\
         - evidence_plan_ref: `{}`\n\
         - note: default worker task catalogs are advisory until the main agent records an adopted stage evidence plan.\n\n\
         {}\n\n\
         ## Required Evidence Before Review\n\n\
         {}\n\n\
         ## Evidence Binding Contract\n\n\
         {evidence_binding_contract}\n\n\
         ## Review Readiness Criteria\n\n\
         {}\n\n\
         ## Rollback Fork Pivot Cleanup Triggers\n\n\
         {}\n\n\
         ## Main Agent Responsibility\n\n\
         The main agent must use accepted worker evidence to synthesize `{}`. This plan does not pass the stage by itself; it records the route the main agent must execute before requesting strict review.\n",
        job.job_id,
        contract.stage_id,
        contract.stage_class,
        contract.artifact_type,
        contract.artifact_path,
        contract.objective,
        contract.evidence_plan_status,
        contract.evidence_plan_ref.as_deref().unwrap_or("none"),
        worker_tasks,
        required_fields,
        pass_criteria,
        failure_routes,
        contract.artifact_type,
        evidence_binding_contract = evidence_binding_contract
    );
    let mut evidence_refs = vec![
        format!("auto_research_job:{}", job.job_id),
        contract.artifact_path.clone(),
    ];
    if let Some(stage_execution_id) = job.stage_execution_id.as_ref() {
        evidence_refs.push(format!("stage_execution:{stage_execution_id}"));
    }
    evidence_refs.extend(job.artifact_refs.iter().take(20).cloned());
    let plan_ref = write_autonomous_research_markdown_with_doc_frame(
        resolved,
        &path,
        AutonomousResearchDocFrameMarkdown {
            title: format!("Astra Stage Plan: {}", contract.stage_id),
            doc_type: "plan".to_string(),
            lifecycle: "active".to_string(),
            summary: format!(
                "Main-agent stage plan for Astra autonomous research stage {}.",
                contract.stage_id
            ),
            key_claims: vec![
                "This plan records how the main agent intends to satisfy the active stage contract."
                    .to_string(),
                "The stage cannot advance from this plan alone; accepted evidence and strict review are still required."
                    .to_string(),
            ],
            decisions: vec![
                format!(
                    "The active stage artifact to synthesize is {}.",
                    contract.artifact_path
                ),
                "Required worker tasks come from the main-agent-adopted stage evidence plan; runtime defaults are advisory."
                    .to_string(),
            ],
            interfaces: vec![
                "autonomous_research_stage_contract".to_string(),
                "goal_task_pool".to_string(),
                "accepted_worker_evidence_index".to_string(),
                "stage_acceptance_rubric".to_string(),
                "research_stage_dag".to_string(),
                "projectops_cleanup".to_string(),
            ],
            evidence_refs,
            next_actions: vec![
                "Complete or accept required stage-local worker evidence.".to_string(),
                format!(
                    "Synthesize accepted evidence into {}.",
                    contract.artifact_path
                ),
                "Request strict stage review only after the artifact is review-ready.".to_string(),
            ],
            non_goals: vec![
                "This plan is not a second research manager.".to_string(),
                "This plan cannot waive stage gates or reviewer requirements.".to_string(),
            ],
        },
        &body,
    )?;
    merge_unique_strings(&mut job.artifact_refs, vec![plan_ref.clone()]);
    Ok(plan_ref)
}

pub(crate) fn autonomous_research_stage_failure_route_notes(
    contract: &AutonomousResearchStageContract,
) -> String {
    let mut routes = vec![
        "- Any route-changing decision, claim-scope change, fork, pivot, supersede, rollback, or abandon requires cleanup planning.".to_string(),
        "- Repeated same-class failure must trigger strategy change rather than another prose-only repair.".to_string(),
    ];
    match contract.stage_id.as_str() {
        "literature" => routes.push(
            "- Missing or weak closest-family evidence routes to more literature tasks before novelty."
                .to_string(),
        ),
        "novelty" | "refine" => routes.push(
            "- Weak novelty or infeasible mechanism routes to literature, novelty, refine, fork, pivot, or abandon."
                .to_string(),
        ),
        "experiment-plan" => routes.push(
            "- Missing baselines, metrics, ablations, or budget constraints routes to experiment-plan repair."
                .to_string(),
        ),
        "implement-solution" => routes.push(
            "- Implementation failures route to code repair, tests, or experiment-plan revision when the plan is invalid."
                .to_string(),
        ),
        "run" | "monitor" => routes.push(
            "- Failed runs, missing logs, or invalid metrics route to run, monitor, implementation, or experiment-plan repair."
                .to_string(),
        ),
        "result-to-claim" => routes.push(
            "- Unsupported claims must be narrowed, deleted, or routed back to experiments or analysis."
                .to_string(),
        ),
        "paper-plan" | "paper-write" | "paper-compile" => routes.push(
            "- Paper failures route to claim mapping, writing, compilation, or cleanup of stale paper sections."
                .to_string(),
        ),
        "research-review" => routes.push(
            "- Final hard-review blockers route to the earliest stage that owns the missing evidence."
                .to_string(),
        ),
        _ => routes.push(
            "- Route failures to the owning stage and record a human gate when the policy cannot safely continue."
                .to_string(),
        ),
    }
    routes.join("\n")
}

pub(crate) fn ensure_autonomous_research_stage_rubric_docframe(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    stage_plan_ref: &str,
) -> Result<String, String> {
    let path = autonomous_research_stage_rubric_path(resolved, job, contract);
    let required_fields = contract
        .required_fields
        .iter()
        .map(|field| format!("- {field}"))
        .collect::<Vec<_>>()
        .join("\n");
    let pass_criteria = contract
        .pass_criteria
        .iter()
        .map(|criterion| format!("- {criterion}"))
        .collect::<Vec<_>>()
        .join("\n");
    let failure_signals = contract
        .failure_signals
        .iter()
        .map(|signal| format!("- {signal}"))
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_binding_contract =
        render_autonomous_research_stage_evidence_binding_contract(contract);
    let worker_tasks = contract
        .worker_task_requirements
        .iter()
        .map(|requirement| format!("- {}", requirement.task_type))
        .collect::<Vec<_>>()
        .join("\n");
    let worker_tasks = if worker_tasks.is_empty() {
        contract
            .advisory_worker_task_types
            .iter()
            .map(|task| format!("- advisory default only, not a gate: {task}"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        worker_tasks
    };
    let body = format!(
        "# Stage Acceptance Rubric\n\n\
         ## Stage\n\n\
         - job_id: `{}`\n\
         - stage_id: `{}`\n\
         - stage_class: `{}`\n\
         - artifact_type: `{}`\n\
         - artifact_path: `{}`\n\n\
         ## Non-Negotiable Research Bottom Line\n\n\
         - The stage cannot pass without auditable evidence or explicit missing-evidence risk.\n\
         - Paper claims remain disallowed before result-to-claim acceptance.\n\
         - Unsupported claims must be deleted, narrowed, or routed to repair.\n\
         - Route-changing repair, pivot, fork, supersede, abandon, or claim-scope changes require cleanup.\n\
         - The next stage may build only on active canonical artifacts tied to this job and stage DAG.\n\n\
         ## Expert-Level Acceptance Target\n\n\
         Passing work must be strong enough that a strict research expert would allow the next stage to depend on it without knowingly inheriting unresolved defects. It must anticipate obvious reviewer attacks, expose assumptions and limitations, cite concrete evidence refs, and satisfy the stage-specific checks below.\n\n\
         ## Required Fields\n\n\
         {required_fields}\n\n\
         ## Pass Criteria\n\n\
         {pass_criteria}\n\n\
         ## Failure Signals\n\n\
         {failure_signals}\n\n\
         ## Evidence Binding Contract\n\n\
         {evidence_binding_contract}\n\n\
         ## Stage-Local Agent Team Responsibilities\n\n\
         - evidence_plan_status: `{}`\n\
         - evidence_plan_ref: `{}`\n\
         - reviewer note: only an adopted stage evidence plan defines required worker evidence; advisory defaults cannot pass or fail the stage by themselves.\n\n\
         {worker_tasks}\n\n\
         ## Main-Agent Stage Plan\n\n\
         - stage_plan_ref: `{}`\n\
         - reviewers must judge the artifact against this plan and the non-negotiable stage contract.\n\n\
         ## Review Application\n\n\
         The review agent must apply this rubric strictly. A weak but well-formatted artifact fails. A stage artifact that lacks DocFrame provenance fails. A repair loop must produce board-visible tasks or DAG decisions instead of prose-only feedback.\n",
        job.job_id,
        contract.stage_id,
        contract.stage_class,
        contract.artifact_type,
        contract.artifact_path,
        contract.evidence_plan_status,
        contract.evidence_plan_ref.as_deref().unwrap_or("none"),
        stage_plan_ref,
        evidence_binding_contract = evidence_binding_contract
    );
    let mut evidence_refs = vec![
        format!("auto_research_job:{}", job.job_id),
        contract.artifact_path.clone(),
        stage_plan_ref.to_string(),
    ];
    if let Some(stage_execution_id) = job.stage_execution_id.as_ref() {
        evidence_refs.push(format!("stage_execution:{stage_execution_id}"));
    }
    evidence_refs.extend(job.artifact_refs.iter().take(20).cloned());
    let rubric_ref = write_autonomous_research_markdown_with_doc_frame(
        resolved,
        &path,
        AutonomousResearchDocFrameMarkdown {
            title: format!("Astra Stage Acceptance Rubric: {}", contract.stage_id),
            doc_type: "review".to_string(),
            lifecycle: "active".to_string(),
            summary: format!(
                "Strict acceptance rubric for Astra autonomous research stage {}.",
                contract.stage_id
            ),
            key_claims: vec![
                "This rubric is the stage-specific acceptance standard reviewers must apply."
                    .to_string(),
                "The artifact cannot pass through formatting alone; evidence quality is mandatory."
                    .to_string(),
            ],
            decisions: vec![
                "System bottom-line rules are non-negotiable.".to_string(),
                "Stage standard setter output is represented as this bounded stage artifact."
                    .to_string(),
                format!(
                    "The active stage artifact under review is {}.",
                    contract.artifact_path
                ),
                format!("The active main-agent stage plan is {stage_plan_ref}."),
            ],
            interfaces: vec![
                "autonomous_research_stage_contract".to_string(),
                "goal_task_pool".to_string(),
                "review_gate".to_string(),
                "research_stage_dag".to_string(),
                "projectops_cleanup".to_string(),
            ],
            evidence_refs,
            next_actions: vec![
                "Apply this rubric during local preflight and live review.".to_string(),
                "Convert every blocking finding into board-visible repair work or DAG decisions."
                    .to_string(),
            ],
            non_goals: vec![
                "This rubric is not a second research manager.".to_string(),
                "This rubric cannot lower system-required bottom-line rules.".to_string(),
            ],
        },
        &body,
    )?;
    merge_unique_strings(&mut job.artifact_refs, vec![rubric_ref.clone()]);
    Ok(rubric_ref)
}

pub(crate) fn ensure_autonomous_research_stage_artifact_docframe(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    rubric_ref: &str,
    stage_plan_ref: &str,
) -> Result<Option<String>, String> {
    if contract.stage_id == "paper-write" {
        return write_autonomous_research_paper_source_manifest(
            resolved,
            job,
            contract,
            rubric_ref,
            stage_plan_ref,
        )
        .map(Some);
    }
    if !contract.artifact_path.ends_with(".md") {
        return Ok(None);
    }
    let path = resolved.workspace_root.join(&contract.artifact_path);
    let current = match std::fs::read_to_string(&path) {
        Ok(current) if !current.trim().is_empty() => current,
        _ => return Ok(None),
    };
    let mut evidence_refs = vec![
        format!("auto_research_job:{}", job.job_id),
        stage_plan_ref.to_string(),
        rubric_ref.to_string(),
        contract.artifact_path.clone(),
    ];
    evidence_refs.sort();
    evidence_refs.dedup();
    let doc_ref = write_autonomous_research_markdown_with_doc_frame(
        resolved,
        &path,
        AutonomousResearchDocFrameMarkdown {
            title: format!("Astra Stage Artifact: {}", contract.artifact_type),
            doc_type: "report".to_string(),
            lifecycle: "active".to_string(),
            summary: format!(
                "Active {} artifact for Astra autonomous research stage {}.",
                contract.artifact_type, contract.stage_id
            ),
            key_claims: vec![
                format!(
                    "This is the active stage artifact reviewed for stage {}.",
                    contract.stage_id
                ),
                "Claims inside this artifact are bounded by cited evidence and stage maturity."
                    .to_string(),
            ],
            decisions: vec![
                format!("Review target path is {}.", contract.artifact_path),
                format!("Stage plan path is {stage_plan_ref}."),
                format!("Review rubric path is {rubric_ref}."),
            ],
            interfaces: vec![
                "stage_plan".to_string(),
                "stage_acceptance_rubric".to_string(),
                "review_gate".to_string(),
                "research_stage_dag".to_string(),
            ],
            evidence_refs,
            next_actions: vec![format!(
                "Submit {} to the stage review gate.",
                contract.artifact_path
            )],
            non_goals: vec![
                "This artifact alone does not advance the stage without a passing review."
                    .to_string(),
            ],
        },
        &current,
    )?;
    Ok(Some(doc_ref))
}

pub(crate) fn autonomous_research_paper_source_manifest_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> PathBuf {
    resolved
        .workspace_root
        .join("papers")
        .join(&job.job_id)
        .join("source_bundle_manifest.md")
}

pub(crate) fn write_autonomous_research_paper_source_manifest(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    rubric_ref: &str,
    stage_plan_ref: &str,
) -> Result<String, String> {
    let manifest_path = autonomous_research_paper_source_manifest_path(resolved, job);
    let artifact_refs = if job.artifact_refs.is_empty() {
        "- none".to_string()
    } else {
        job.artifact_refs
            .iter()
            .map(|artifact| format!("- `{artifact}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let body = format!(
        "# Paper Source Bundle Manifest\n\n\
         ## Stage\n\n\
         - job_id: `{}`\n\
         - stage_id: `{}`\n\
         - artifact_type: `{}`\n\
         - TeX source path: `{}`\n\
         - stage_plan_ref: `{}`\n\
         - rubric_path: `{}`\n\n\
         ## Bundle Requirements\n\n\
         - TeX source must be current and canonical for this job.\n\
         - Claims in TeX must cite accepted evidence from result-to-claim.\n\
         - Bibliography, figures, tables, reproducibility appendix, and limitation section must be auditable before paper-compile.\n\
         - Markdown-only paper text is not an acceptable paper-write artifact.\n\n\
         ## Current Artifact Refs\n\n{}\n",
        job.job_id,
        contract.stage_id,
        contract.artifact_type,
        contract.artifact_path,
        stage_plan_ref,
        rubric_ref,
        artifact_refs
    );
    write_autonomous_research_markdown_with_doc_frame(
        resolved,
        &manifest_path,
        AutonomousResearchDocFrameMarkdown {
            title: "Astra Paper Source Bundle Manifest".to_string(),
            doc_type: "manifest".to_string(),
            lifecycle: "active".to_string(),
            summary: format!(
                "DocFrame-bearing manifest for TeX source bundle of Astra job {}.",
                job.job_id
            ),
            key_claims: vec![
                "TeX source is represented by this DocFrame-bearing source bundle manifest."
                    .to_string(),
            ],
            decisions: vec![
                "DocFrame is not embedded into TeX source.".to_string(),
                format!("TeX source path is {}.", contract.artifact_path),
                format!("Stage plan path is {stage_plan_ref}."),
            ],
            interfaces: vec![
                "stage_plan".to_string(),
                "paper-write".to_string(),
                "paper-compile".to_string(),
                "review_gate".to_string(),
            ],
            evidence_refs: vec![
                format!("auto_research_job:{}", job.job_id),
                contract.artifact_path.clone(),
                stage_plan_ref.to_string(),
                rubric_ref.to_string(),
            ],
            next_actions: vec!["Compile TeX and record compiled PDF bundle.".to_string()],
            non_goals: vec![
                "This manifest does not replace the TeX source or PDF build evidence.".to_string(),
            ],
        },
        &body,
    )
}

pub(crate) fn run_autonomous_research_review_gate(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    output_json: bool,
) -> Result<AutonomousResearchReviewGateOutcome, CommandFailureOutcome> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let target_path = contract.artifact_path.clone();
    let stage_plan_ref = ensure_autonomous_research_stage_plan_docframe(resolved, job, &contract)
        .map_err(|err| {
        internal_failure("research jobs tick", err).with_output_json(output_json)
    })?;
    let rubric_ref =
        ensure_autonomous_research_stage_rubric_docframe(resolved, job, &contract, &stage_plan_ref)
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
    let stage_docframe_ref = ensure_autonomous_research_stage_artifact_docframe(
        resolved,
        job,
        &contract,
        &rubric_ref,
        &stage_plan_ref,
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    if let Some(stage_docframe_ref) = stage_docframe_ref {
        merge_unique_strings(&mut job.artifact_refs, vec![stage_docframe_ref]);
    }
    let local_preflight = evaluate_autonomous_research_stage_artifact(
        resolved,
        job,
        &contract,
        &rubric_ref,
        &stage_plan_ref,
    );
    let (preflight_verdict, _) = parse_autonomous_research_review_verdict(&local_preflight);
    let review_text = if preflight_verdict == "pass" {
        match run_autonomous_research_live_review(
            resolved,
            job,
            &contract,
            &rubric_ref,
            &stage_plan_ref,
        ) {
            AutonomousResearchLiveReviewOutcome::Completed(review) => review,
            AutonomousResearchLiveReviewOutcome::Unavailable => {
                let quoted_local_preflight = local_preflight
                    .lines()
                    .map(|line| format!("runtime-preflight> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "verdict: fail\n\
                     score: 0\n\
                     failure class: review_provider_unavailable\n\
                     suggested operation: retry\n\
                     rollback target: none\n\
                     cleanup requirement: not required\n\n\
                     Local preflight passed, but the live strict review provider is unavailable or not configured. Runtime preflight cannot substitute for reviewer acceptance, so the stage remains blocked until a real review agent/provider produces a pass.\n\n\
                     ## Local Preflight Evidence\n\n\
                     ```text\n\
                     {quoted_local_preflight}\n\
                     ```"
                )
            }
            AutonomousResearchLiveReviewOutcome::ProviderFault(fault) => {
                return Ok(AutonomousResearchReviewGateOutcome::ProviderFault(fault));
            }
        }
    } else {
        format!(
            "{local_preflight}\n\nLocal preflight failed before live review. The stage review provider is skipped this round to avoid burning tokens on a non-reviewable stage artifact; the supervisor must run another repair loop."
        )
    };
    let (verdict, score) = parse_autonomous_research_review_verdict(&review_text);
    let accepted_worker_evidence_index =
        load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let (
        accepted_evidence_target_paths,
        accepted_evidence_blind_context,
        accepted_evidence_required,
    ) = autonomous_research_review_gate_accepted_worker_evidence_context(
        resolved,
        job,
        accepted_worker_evidence_index.as_ref(),
    );
    let mut target_paths = vec![
        target_path.clone(),
        stage_plan_ref.clone(),
        rubric_ref.clone(),
    ];
    merge_unique_strings(&mut target_paths, accepted_evidence_target_paths);
    let mut blind_context = vec![
        format!("auto_research_job:{}", job.job_id),
        format!("stage_id:{}", contract.stage_id),
        format!("artifact_type:{}", contract.artifact_type),
        format!("stage_plan_path:{stage_plan_ref}"),
        format!("rubric_path:{rubric_ref}"),
    ];
    merge_unique_strings(&mut blind_context, accepted_evidence_blind_context);
    let mut evidence_required = contract.pass_criteria.clone();
    evidence_required.push(format!(
        "valid DocFrame-bearing main-agent stage plan exists at {stage_plan_ref}"
    ));
    evidence_required.push(format!(
        "valid DocFrame-bearing stage acceptance rubric exists at {rubric_ref}"
    ));
    evidence_required
        .push("valid DocFrame-bearing review summary is recorded after review".to_string());
    merge_unique_strings(&mut evidence_required, accepted_evidence_required);
    let review = reviews::open(
        &resolved.data_dir,
        reviews::ReviewOpenRequest {
            target_paths,
            objective: format!(
                "Review Astra autonomous research job {} active stage {} against stage plan {} and rubric {}. Pass only if the {} artifact satisfies its stage contract, plan, and rubric; do not require final paper claims before result-to-claim.",
                job.job_id, contract.stage_id, stage_plan_ref, rubric_ref, contract.artifact_type
            ),
            reviewer_role: contract.review_strength.clone(),
            review_model: job
                .review_model
                .clone()
                .unwrap_or_else(|| "astra-review-gate".to_string()),
            blind_context,
            review_materials: Vec::new(),
            executor_summary: None,
            evidence_required,
            compare_against: job.last_review.as_ref().map(|review| review.review_id.clone()),
            retry_of: job.last_review.as_ref().map(|review| review.review_id.clone()),
            retry_attempt: job.review_rounds_completed as u64,
            verdict: Some(verdict.clone()),
            response_text: Some(review_text.clone()),
        },
    )
    .map_err(|err| {
        internal_failure("research jobs tick", err.to_string()).with_output_json(output_json)
    })?;
    emit_review_event_for_resolved(resolved, &opened_review_payload(&review))
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    let review_summary_ref = write_autonomous_research_review_summary(
        resolved,
        job,
        &contract,
        &review.packet.review_id,
        &verdict,
        score,
        &review_text,
        &rubric_ref,
        &stage_plan_ref,
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    merge_unique_strings(
        &mut job.artifact_refs,
        vec![
            stage_plan_ref.clone(),
            rubric_ref.clone(),
            review_summary_ref.clone(),
        ],
    );
    Ok(AutonomousResearchReviewGateOutcome::Review(
        AutonomousResearchReviewState {
            review_id: review.packet.review_id,
            verdict,
            score,
            response_text: review_text,
            target_path,
            rubric_path: Some(rubric_ref),
            review_summary_path: Some(review_summary_ref),
            created_at: timestamp_string(),
        },
    ))
}

pub(crate) fn promote_integration_verified_canonical_artifacts_after_passed_stage_gate(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review: &AutonomousResearchReviewState,
) -> Result<Vec<String>, String> {
    if review.verdict != "pass" {
        return Ok(Vec::new());
    }
    let Some(stage_execution_id) = job.stage_execution_id.as_deref() else {
        return Ok(Vec::new());
    };
    let ledger = canonical_artifacts::load_ledger(&resolved.data_dir)
        .map_err(|err| format!("canonical artifact ledger could not be loaded: {err}"))?;
    let artifact_ids = ledger
        .entries
        .iter()
        .filter(|entry| {
            entry.job_id == job.job_id
                && entry.stage_id == contract.stage_id
                && entry.stage_execution_id == stage_execution_id
                && entry.status == canonical_artifacts::CanonicalArtifactStatus::IntegrationVerified
        })
        .map(|entry| entry.artifact_id.clone())
        .collect::<Vec<_>>();
    let mut promoted_refs = Vec::new();
    for artifact_id in artifact_ids {
        let updated = canonical_artifacts::record_active_stage_evidence(
            &resolved.data_dir,
            &artifact_id,
            format!(
                "review_gate:{}:{}:{}",
                job.job_id, contract.stage_id, review.review_id
            ),
        )
        .map_err(|err| format!("canonical artifact active-stage promotion failed: {err}"))?;
        merge_unique_strings(
            &mut promoted_refs,
            vec![
                format!("canonical_artifact:{}", updated.artifact_id),
                format!("active_stage_evidence:{}", updated.target_artifact_path),
            ],
        );
    }
    Ok(promoted_refs)
}

pub(crate) fn write_autonomous_research_review_summary(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    review_id: &str,
    verdict: &str,
    score: Option<u64>,
    review_text: &str,
    rubric_ref: &str,
    stage_plan_ref: &str,
) -> Result<String, String> {
    let path = resolved
        .workspace_root
        .join("research")
        .join("stages")
        .join(&job.job_id)
        .join(&contract.stage_id)
        .join("reviews")
        .join(format!("{review_id}.md"));
    let score_text = score
        .map(|score| score.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let failure_class = extract_or_infer_failure_class(review_text, &contract.stage_id);
    let suggested_operation = extract_review_labeled_value(review_text, "suggested operation")
        .map(|value| normalize_review_operation(&value))
        .unwrap_or_else(|| infer_review_operation(&review_text.to_ascii_lowercase()).to_string());
    let rollback_target = extract_review_labeled_value(review_text, "rollback target")
        .or_else(|| infer_rollback_target(&failure_class, &contract.stage_id));
    let cleanup_requirement = extract_review_labeled_value(review_text, "cleanup requirement")
        .unwrap_or_else(|| {
            if review_requires_cleanup(
                &review_text.to_ascii_lowercase(),
                &suggested_operation,
                &failure_class,
            ) {
                "required".to_string()
            } else {
                "not required unless repair changes active project口径".to_string()
            }
        });
    let body = format!(
        "# Stage Review Summary\n\n\
         ## Verdict\n\n\
         - review_id: `{review_id}`\n\
         - verdict: `{verdict}`\n\
         - score: `{score_text}`\n\
         - stage_id: `{}`\n\
         - target_path: `{}`\n\
         - stage_plan_path: `{stage_plan_ref}`\n\
         - rubric_path: `{rubric_ref}`\n\n\
         ## Routing\n\n\
         - failure_class: `{failure_class}`\n\
         - suggested_operation: `{suggested_operation}`\n\
         - rollback_target: `{}`\n\
         - cleanup_requirement: `{cleanup_requirement}`\n\n\
         ## Required Repairs\n\n\
         Every blocking finding below must become a board-visible repair task, DAG route decision, cleanup proposal, or human gate before the stage can advance.\n\n\
         ## Raw Review Text\n\n\
         ```text\n{}\n```\n",
        contract.stage_id,
        contract.artifact_path,
        rollback_target.unwrap_or_else(|| "none".to_string()),
        review_text.trim()
    );
    write_autonomous_research_markdown_with_doc_frame(
        resolved,
        &path,
        AutonomousResearchDocFrameMarkdown {
            title: format!("Astra Stage Review Summary: {review_id}"),
            doc_type: "review".to_string(),
            lifecycle: "active".to_string(),
            summary: format!(
                "Review summary for Astra autonomous research stage {} with verdict {}.",
                contract.stage_id, verdict
            ),
            key_claims: vec![
                format!("Review verdict is {verdict}."),
                format!("Review score is {score_text}."),
            ],
            decisions: vec![
                format!("Suggested operation is {suggested_operation}."),
                format!("Failure class is {failure_class}."),
                format!("Cleanup requirement is {cleanup_requirement}."),
                format!("Stage plan under review is {stage_plan_ref}."),
            ],
            interfaces: vec![
                "stage_plan".to_string(),
                "review_gate".to_string(),
                "repair_plan".to_string(),
                "goal_task_pool".to_string(),
                "research_stage_dag".to_string(),
            ],
            evidence_refs: vec![
                format!("review:{review_id}"),
                format!("auto_research_job:{}", job.job_id),
                contract.artifact_path.clone(),
                stage_plan_ref.to_string(),
                rubric_ref.to_string(),
            ],
            next_actions: vec![
                "If verdict is fail, generate structured repair tasks and route through the DAG."
                    .to_string(),
                "If cleanup is required, create a ProjectOps cleanup summary.".to_string(),
            ],
            non_goals: vec!["This summary is not itself a repair completion artifact.".to_string()],
        },
        &body,
    )
}

pub(crate) fn autonomous_research_review_canonical_artifact_policy() -> &'static str {
    "For implementation, experiment, test, or runnable claims, only canonical project paths with baseline-visible and, when runnable, integration-verified state may count as usable project evidence. Files that exist only in `.pmcli/input-bundles` are provenance/context, not canonical project implementation."
}

pub(crate) fn run_autonomous_research_live_review(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    rubric_ref: &str,
    stage_plan_ref: &str,
) -> AutonomousResearchLiveReviewOutcome {
    let prompt_defaults = resolve_prompt_request_defaults(
        resolved,
        &PromptInvocation {
            session_id: None,
            new_session: false,
            prompt_text: job.prompt.clone(),
            provider: None,
            model: job.review_model.clone(),
            permission_mode: Some("read-only".to_string()),
        },
    )
    .ok();
    let Some(prompt_defaults) = prompt_defaults else {
        return AutonomousResearchLiveReviewOutcome::Unavailable;
    };
    let route_selection = autonomous_research_select_provider_route(
        resolved,
        prompt_defaults.provider,
        prompt_defaults.provider_source,
        prompt_defaults.model,
        prompt_defaults.model_source,
        &autonomous_research_backoff_excluded_providers(resolved, job),
    );
    let Some(route_selection) = route_selection else {
        return AutonomousResearchLiveReviewOutcome::Unavailable;
    };
    let effective_config = crate::config::effective_config(resolved)
        .map(|config| config.effective)
        .ok();
    let Some(effective_config) = effective_config else {
        return AutonomousResearchLiveReviewOutcome::Unavailable;
    };
    let provider_trace = resolve_provider_trace_with_profiles(
        route_selection.provider.as_deref(),
        route_selection.model.as_deref(),
        route_selection.provider_source.as_deref(),
        route_selection.model_source.as_deref(),
        &effective_config.provider_profiles,
    )
    .ok();
    let Some(provider_trace) = provider_trace else {
        return AutonomousResearchLiveReviewOutcome::Unavailable;
    };
    if provider_trace.auth_status != "configured" {
        return AutonomousResearchLiveReviewOutcome::Unavailable;
    }
    if route_selection.failover_used {
        let _ = append_autonomous_research_job_event(
            resolved,
            job,
            "provider_failover_selected",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "source": "review_gate",
                "provider": provider_trace.resolved_provider,
                "model": provider_trace.resolved_model,
                "active_provider_fault": job.active_provider_fault
            }),
        );
    }
    let target_path = resolved.workspace_root.join(&contract.artifact_path);
    let stage_artifact = std::fs::read_to_string(&target_path).unwrap_or_default();
    let stage_artifact_review_focus =
        autonomous_research_stage_artifact_review_focus(&stage_artifact);
    let accepted_worker_evidence_index =
        load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let accepted_worker_evidence = render_autonomous_research_accepted_worker_evidence_index(
        accepted_worker_evidence_index.as_ref(),
    );
    let canonical_artifact_ledger = render_canonical_artifact_ledger_for_prompt(&resolved.data_dir);
    let canonical_artifact_review_policy = autonomous_research_review_canonical_artifact_policy();
    let evidence_binding_contract =
        render_autonomous_research_stage_evidence_binding_contract(contract);
    let rubric = std::fs::read_to_string(resolved.workspace_root.join(rubric_ref))
        .unwrap_or_else(|_| "Rubric document could not be read.".to_string());
    let stage_plan = std::fs::read_to_string(resolved.workspace_root.join(stage_plan_ref))
        .unwrap_or_else(|_| "Stage plan document could not be read.".to_string());
    let artifact_refs = render_autonomous_research_review_artifact_refs(&job.artifact_refs);
    let tick_trace = render_autonomous_research_review_tick_trace(&job.tick_summaries);
    let prompt = format!(
        "Review this Astra autonomous research stage artifact as a strict stage reviewer.\n\n\
         You must fail unless the artifact satisfies the active stage contract, the stage acceptance rubric, cites auditable evidence or explicit missing-evidence risks, and avoids unsupported claims. Do not require final paper claims before result-to-claim. If the evidence is weak, require another autonomous repair loop.\n\n\
         Review the Stage Artifact Body section as the primary research artifact. The DocFrame/provenance excerpt and accepted-evidence index are support context only; do not let large provenance metadata hide or replace the artifact body. Only entries with acceptance_authority=`main_agent_worker_artifact_decision`, active_status=`active`, and a main_agent_decision_ref are accepted stage-local evidence. Entries replayed from goal acceptance or independent semantic review without main-agent acceptance are candidate evidence only; they must not satisfy stage coverage or support final stage review. {canonical_artifact_review_policy}\n\n\
         Return exactly this structure at the top:\n\
         verdict: pass|fail\n\
         score: <0-100>\n\n\
         Then provide concise findings, failure class, suggested operation, rollback target when applicable, cleanup requirement when applicable, and required repairs.\n\n\
         Job id: {job_id}\n\
         Goal: {goal}\n\
         Active stage id: {stage_id}\n\
         Stage artifact type: {artifact_type}\n\
         Stage artifact path: {stage_artifact_path}\n\
         Stage plan path: {stage_plan_ref}\n\
         Stage acceptance rubric path: {rubric_ref}\n\
         Review strength: {review_strength}\n\
         Required fields: {required_fields}\n\
         Pass criteria: {pass_criteria}\n\
         Failure signals: {failure_signals}\n\
         Evidence binding contract:\n{evidence_binding_contract}\n\n\
         Stage artifact under review:\n{stage_artifact_review_focus}\n\n\
         Stage acceptance rubric:\n{rubric}\n\n\
         Accepted stage-local worker evidence:\n{accepted_worker_evidence}\n\n\
         Canonical project artifact state:\n{canonical_artifact_ledger}\n\n\
         Main-agent stage plan:\n{stage_plan}\n\n\
         Bounded artifact reference manifest:\n{artifacts}\n\n\
         Bounded goal/agent loop trace:\n{tick_trace}\n\n\
         End of review packet.",
        job_id = job.job_id,
        goal = job.prompt,
        stage_id = contract.stage_id,
        artifact_type = contract.artifact_type,
        stage_artifact_path = contract.artifact_path,
        stage_plan_ref = stage_plan_ref,
        rubric_ref = rubric_ref,
        review_strength = contract.review_strength,
        required_fields = contract.required_fields.join(", "),
        pass_criteria = contract.pass_criteria.join(" | "),
        failure_signals = contract.failure_signals.join(" | "),
        evidence_binding_contract = evidence_binding_contract,
        artifacts = artifact_refs,
        stage_artifact_review_focus = stage_artifact_review_focus,
        accepted_worker_evidence = truncate_for_prompt(
            &accepted_worker_evidence,
            AUTONOMOUS_RESEARCH_REVIEW_ACCEPTED_EVIDENCE_MAX_CHARS
        ),
        canonical_artifact_review_policy = canonical_artifact_review_policy,
        canonical_artifact_ledger = truncate_for_prompt(&canonical_artifact_ledger, 8_000),
        tick_trace = tick_trace,
        stage_plan = truncate_for_prompt(&stage_plan, 10_000),
        rubric = truncate_for_prompt(&rubric, 12_000),
    );
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are Astra's stage review agent. You are skeptical, evidence-focused, and must not pass weak autonomous research outputs.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: fit_autonomous_research_review_prompt_to_model(&provider_trace, &prompt),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    match complete_prompt_streaming_live_blocking_with_cancel(
        &provider_trace,
        &messages,
        |_| {},
        || false,
    ) {
        Ok(completion) if !completion.content.trim().is_empty() => {
            AutonomousResearchLiveReviewOutcome::Completed(format!(
                "{}\n\nreview_provider: {}\nreview_model: {}\nreview_execution_mode: {}\n",
                completion.content.trim(),
                provider_trace.resolved_provider,
                provider_trace.resolved_model,
                completion.execution_mode
            ))
        }
        Ok(_) => AutonomousResearchLiveReviewOutcome::Unavailable,
        Err(err) => AutonomousResearchLiveReviewOutcome::ProviderFault(
            autonomous_research_provider_fault_outcome_from_error(
                job,
                &provider_trace,
                "review_gate",
                &err,
            ),
        ),
    }
}

pub(crate) fn evaluate_autonomous_research_stage_artifact(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    contract: &AutonomousResearchStageContract,
    rubric_ref: &str,
    stage_plan_ref: &str,
) -> String {
    let path = resolved.workspace_root.join(&contract.artifact_path);
    let artifact = std::fs::read_to_string(&path).unwrap_or_default();
    let lower = artifact.to_lowercase();
    let accepted_worker_evidence_index =
        load_autonomous_research_accepted_worker_evidence_index(resolved, job);
    let missing_upstream_artifacts =
        autonomous_research_missing_required_upstream_artifacts(resolved, job, contract);
    let evidence_binding_failures = autonomous_research_stage_artifact_evidence_binding_failures(
        &artifact,
        contract,
        accepted_worker_evidence_index.as_ref(),
    );
    let stage_artifact_adoption_snapshot_failure =
        autonomous_research_stage_artifact_adoption_snapshot_failure(
            resolved,
            job,
            contract,
            accepted_worker_evidence_index.as_ref(),
        );
    let (stage_plan_docframe_valid, stage_plan_docframe_note) =
        autonomous_research_doc_has_valid_doc_frame(resolved, stage_plan_ref);
    let (rubric_docframe_valid, rubric_docframe_note) =
        autonomous_research_doc_has_valid_doc_frame(resolved, rubric_ref);
    let stage_docframe_check = if contract.artifact_path.ends_with(".md") {
        let (valid, note) =
            autonomous_research_doc_has_valid_doc_frame(resolved, &contract.artifact_path);
        Some((
            "stage_artifact_docframe",
            valid,
            format!("DocFrame status for {}: {note}", contract.artifact_path),
        ))
    } else if contract.stage_id == "paper-write" {
        let manifest_ref = relative_workspace_ref(
            &resolved.workspace_root,
            &autonomous_research_paper_source_manifest_path(resolved, job),
        );
        let (valid, note) = autonomous_research_doc_has_valid_doc_frame(resolved, &manifest_ref);
        Some((
            "paper_source_manifest_docframe",
            valid,
            format!("DocFrame status for {manifest_ref}: {note}"),
        ))
    } else {
        None
    };
    let mut score = 0u64;
    let mut findings = Vec::new();
    let required_fields_present = contract
        .required_fields
        .iter()
        .filter(|field| autonomous_research_stage_required_field_present(contract, field, &lower))
        .count();
    let min_required_fields = contract.required_fields.len().min(4);
    let mut checks = vec![
        (
            "artifact_exists",
            path.exists() && !artifact.trim().is_empty(),
        ),
        ("stage_plan_docframe", stage_plan_docframe_valid),
        ("stage_acceptance_rubric_docframe", rubric_docframe_valid),
        (
            "stage_evidence_plan_adopted",
            contract.evidence_plan_status == "adopted",
        ),
        (
            "required_fields",
            required_fields_present >= min_required_fields,
        ),
        (
            "stage_identity",
            lower.contains(&contract.stage_id.to_ascii_lowercase())
                || lower.contains(&contract.artifact_type.to_ascii_lowercase()),
        ),
        (
            "evidence_refs",
            !job.artifact_refs.is_empty()
                && (artifact.contains(".pmcli")
                    || artifact.contains("research/")
                    || artifact.contains("experiments/")
                    || artifact.contains("papers/")
                    || artifact.contains("http://")
                    || artifact.contains("https://")
                    || lower.contains("missing-source risk")
                    || lower.contains("missing source risk")),
        ),
        (
            "limitations_or_risks",
            lower.contains("limitation")
                || lower.contains("risk")
                || lower.contains("missing")
                || lower.contains("unsupported"),
        ),
        (
            "sufficient_length",
            artifact.split_whitespace().count() >= 120 || job.provider_rounds.len() >= 2,
        ),
        (
            "goal_loop_used",
            job.tick_summaries
                .iter()
                .any(|tick| tick.dispatch_count > 0 || tick.accepted),
        ),
        (
            "accepted_worker_evidence_cited",
            accepted_worker_evidence_bound_to_stage_artifact(
                resolved,
                job,
                contract,
                accepted_worker_evidence_index.as_ref(),
            ),
        ),
        (
            "stage_artifact_adoption_snapshot",
            stage_artifact_adoption_snapshot_failure.is_none(),
        ),
        (
            "required_stage_task_coverage",
            accepted_worker_evidence_covers_required_task_types(
                contract,
                accepted_worker_evidence_index.as_ref(),
            ),
        ),
        (
            "accepted_worker_evidence_unambiguous",
            accepted_worker_evidence_ambiguity_failures(accepted_worker_evidence_index.as_ref())
                .is_empty(),
        ),
        (
            "accepted_worker_evidence_quality_floor",
            accepted_worker_evidence_quality_floor_passes(
                contract,
                accepted_worker_evidence_index.as_ref(),
            ),
        ),
        (
            "accepted_worker_semantic_review_floor",
            accepted_worker_evidence_semantic_review_floor_passes(
                contract,
                accepted_worker_evidence_index.as_ref(),
            ),
        ),
        (
            "accepted_worker_active_tool_evidence_floor",
            accepted_worker_evidence_active_tool_floor_passes(
                contract,
                accepted_worker_evidence_index.as_ref(),
            ),
        ),
        (
            "stage_synthesis_evidence_integrated",
            accepted_worker_evidence_has_adoptable_stage_synthesis(
                contract,
                accepted_worker_evidence_index.as_ref(),
            ),
        ),
        (
            "stage_artifact_evidence_binding",
            evidence_binding_failures.is_empty(),
        ),
        (
            "required_upstream_stage_artifacts",
            missing_upstream_artifacts.is_empty(),
        ),
    ];
    let mut check_notes = Vec::new();
    if !stage_plan_docframe_valid {
        check_notes.push(format!(
            "FAIL stage_plan_docframe_detail: {stage_plan_docframe_note}"
        ));
    }
    if !rubric_docframe_valid {
        check_notes.push(format!(
            "FAIL stage_acceptance_rubric_docframe_detail: {rubric_docframe_note}"
        ));
    }
    if contract.evidence_plan_status != "adopted" {
        check_notes.push(
            "FAIL stage_evidence_plan_adopted_detail: main agent has not recorded an adopted stage evidence plan; runtime advisory defaults are not an evidence gate".to_string(),
        );
    }
    let missing_required_stage_tasks = accepted_worker_evidence_missing_required_task_types(
        contract,
        accepted_worker_evidence_index.as_ref(),
    );
    if !missing_required_stage_tasks.is_empty() {
        check_notes.push(format!(
            "FAIL required_stage_task_coverage_detail: missing accepted stage-local task categories: {}",
            missing_required_stage_tasks.join(", ")
        ));
    }
    let ambiguous_accepted_evidence =
        accepted_worker_evidence_ambiguity_failures(accepted_worker_evidence_index.as_ref());
    if !ambiguous_accepted_evidence.is_empty() {
        check_notes.push(format!(
            "FAIL accepted_worker_evidence_unambiguous_detail: {}",
            ambiguous_accepted_evidence.join("; ")
        ));
    }
    let insufficient_worker_quality = accepted_worker_evidence_quality_floor_failures(
        contract,
        accepted_worker_evidence_index.as_ref(),
    );
    if !insufficient_worker_quality.is_empty() {
        check_notes.push(format!(
            "FAIL accepted_worker_evidence_quality_floor_detail: insufficient accepted worker evidence: {}",
            insufficient_worker_quality.join("; ")
        ));
    }
    let failed_semantic_reviews = accepted_worker_evidence_semantic_review_floor_failures(
        contract,
        accepted_worker_evidence_index.as_ref(),
    );
    if !failed_semantic_reviews.is_empty() {
        check_notes.push(format!(
            "FAIL accepted_worker_semantic_review_floor_detail: missing or failed semantic reviews for accepted worker evidence: {}",
            failed_semantic_reviews.join("; ")
        ));
    }
    let missing_active_tool_evidence = accepted_worker_evidence_active_tool_floor_failures(
        contract,
        accepted_worker_evidence_index.as_ref(),
    );
    if !missing_active_tool_evidence.is_empty() {
        check_notes.push(format!(
            "FAIL accepted_worker_active_tool_evidence_floor_detail: accepted worker evidence lacks active specialist tool/action evidence: {}",
            missing_active_tool_evidence.join("; ")
        ));
    }
    if let Some(reason) = accepted_worker_evidence_missing_adoptable_stage_synthesis_reason(
        contract,
        accepted_worker_evidence_index.as_ref(),
    ) {
        check_notes.push(format!(
            "FAIL stage_synthesis_evidence_integrated_detail: {reason}"
        ));
    }
    if !evidence_binding_failures.is_empty() {
        check_notes.push(format!(
            "FAIL stage_artifact_evidence_binding_detail: {}",
            evidence_binding_failures.join("; ")
        ));
    }
    if let Some(reason) = stage_artifact_adoption_snapshot_failure {
        check_notes.push(format!(
            "FAIL stage_artifact_adoption_snapshot_detail: {reason}"
        ));
    }
    if !missing_upstream_artifacts.is_empty() {
        check_notes.push(format!(
            "FAIL required_upstream_stage_artifacts_detail: {}",
            missing_upstream_artifacts.join("; ")
        ));
    }
    if let Some((name, passed, note)) = stage_docframe_check {
        checks.push((name, passed));
        if !passed {
            check_notes.push(format!("FAIL {name}_detail: {note}"));
        }
    }
    if matches!(
        contract.stage_id.as_str(),
        "implement-solution" | "run" | "monitor" | "paper-compile" | "research-review"
    ) {
        checks.push((
            "reproducibility",
            lower.contains("command")
                || lower.contains("config")
                || lower.contains("log")
                || lower.contains("reproducibility"),
        ));
    }
    if contract.stage_id == "paper-write" {
        checks.push((
            "tex_not_markdown_only",
            artifact.contains("\\documentclass") || artifact.contains("\\section"),
        ));
    }
    if contract.stage_id == "paper-compile" {
        let pdf_path = path
            .parent()
            .unwrap_or(resolved.workspace_root.as_path())
            .join("build")
            .join("main.pdf");
        checks.push((
            "pdf_bundle",
            lower.contains(".pdf")
                && lower.contains("build log")
                && lower.contains("pdf validation result")
                && pdf_path.exists()
                && !lower.contains("fail: pdf is missing"),
        ));
    }
    if contract.stage_id == "research-review" {
        let tex_path = resolved
            .workspace_root
            .join(autonomous_research_stage_artifact_path(
                job,
                "paper-write",
                "paper_tex_bundle",
            ));
        let compiled_bundle_path =
            resolved
                .workspace_root
                .join(autonomous_research_stage_artifact_path(
                    job,
                    "paper-compile",
                    "compiled_pdf_bundle",
                ));
        let pdf_path = compiled_bundle_path
            .parent()
            .unwrap_or(resolved.workspace_root.as_path())
            .join("build")
            .join("main.pdf");
        let tex_is_real = std::fs::read_to_string(&tex_path)
            .map(|tex| tex.contains("\\documentclass") && tex.contains("\\begin{document}"))
            .unwrap_or(false);
        let compiled_bundle = std::fs::read_to_string(&compiled_bundle_path).unwrap_or_default();
        checks.push((
            "final_bundle_refs",
            lower.contains("claim table")
                && lower.contains("literature matrix")
                && lower.contains("experiment report")
                && (lower.contains(".pdf") || lower.contains("compiled pdf"))
                && tex_path.exists()
                && tex_is_real
                && compiled_bundle_path.exists()
                && pdf_path.exists()
                && !compiled_bundle
                    .to_ascii_lowercase()
                    .contains("pdf is missing"),
        ));
    }
    let mut blocking_failures = Vec::new();
    for (name, passed) in checks {
        if passed {
            score += 10;
            findings.push(format!("PASS {name}"));
        } else {
            if autonomous_research_blocking_stage_check(name) {
                blocking_failures.push(name.to_string());
            }
            findings.push(format!("FAIL {name}"));
        }
    }
    findings.extend(check_notes);
    if job.last_error.is_some() {
        score = score.saturating_sub(10);
        findings.push(format!(
            "FAIL last_error_present: {}",
            job.last_error.clone().unwrap_or_default()
        ));
    }
    let required_score = if contract.stage_id == "research-review" {
        90
    } else {
        70
    };
    if !blocking_failures.is_empty() {
        findings.push(format!(
            "FAIL blocking_stage_gate: {}",
            blocking_failures.join(", ")
        ));
    }
    let verdict = if blocking_failures.is_empty() && score >= required_score {
        "pass"
    } else {
        "fail"
    };
    format!(
        "verdict: {verdict}\nscore: {score}\n\nStage review findings for `{stage_id}` / `{artifact_type}`:\n{}\n\nRequired repair if fail:\n- Fill missing required fields for the active stage artifact.\n- Cite artifacts from Astra job state, board/agent traces, source refs, or explicit missing-source risks.\n- Replace any accepted worker evidence whose quality_profile is insufficient before requesting review again.\n- Re-run or repair accepted worker evidence whose semantic_review is missing, failed, or below the required score.\n- Address limitations, risks, unsupported statements, and reproducibility requirements for this stage.\n- Suggested operation: repair.\n- Cleanup requirement: no cleanup unless the repair changes the active project口径.\n",
        findings.join("\n"),
        stage_id = contract.stage_id,
        artifact_type = contract.artifact_type,
    )
}

pub(crate) fn autonomous_research_blocking_stage_check(name: &str) -> bool {
    matches!(
        name,
        "artifact_exists"
            | "stage_plan_docframe"
            | "stage_acceptance_rubric_docframe"
            | "stage_artifact_docframe"
            | "paper_source_manifest_docframe"
            | "required_fields"
            | "accepted_worker_evidence_cited"
            | "stage_artifact_adoption_snapshot"
            | "required_stage_task_coverage"
            | "accepted_worker_evidence_quality_floor"
            | "accepted_worker_semantic_review_floor"
            | "accepted_worker_active_tool_evidence_floor"
            | "stage_synthesis_evidence_integrated"
            | "stage_artifact_evidence_binding"
            | "required_upstream_stage_artifacts"
            | "tex_not_markdown_only"
            | "pdf_bundle"
            | "final_bundle_refs"
    )
}

pub(crate) fn parse_autonomous_research_review_verdict(review_text: &str) -> (String, Option<u64>) {
    let mut verdict = None;
    let mut score = None;
    for line in review_text.lines() {
        let lower = line.trim().to_lowercase();
        if let Some(value) = lower.strip_prefix("verdict:") {
            verdict = Some(if value.trim() == "pass" {
                "pass".to_string()
            } else {
                "fail".to_string()
            });
        }
        if let Some(value) = lower.strip_prefix("score:") {
            score = parse_review_labeled_score(value.trim());
        }
        if verdict.is_some() && score.is_some() {
            break;
        }
    }
    let verdict = verdict.unwrap_or_else(|| {
        extract_review_labeled_value(review_text, "verdict")
            .map(|value| {
                if value.trim().eq_ignore_ascii_case("pass") {
                    "pass".to_string()
                } else {
                    "fail".to_string()
                }
            })
            .unwrap_or_else(|| "fail".to_string())
    });
    let score = score.or_else(|| {
        extract_review_labeled_value(review_text, "score")
            .and_then(|value| parse_review_labeled_score(&value))
    });
    (verdict, score)
}

pub(crate) fn record_autonomous_research_failed_review_pattern(
    job: &mut AutonomousResearchJobState,
    resolved: &ResolvedProject,
    review: &AutonomousResearchReviewState,
) -> AutonomousResearchFailurePattern {
    let contract = autonomous_research_stage_contract(resolved, job);
    let lowered = review.response_text.to_ascii_lowercase();
    let failure_class = extract_or_infer_failure_class(&review.response_text, &contract.stage_id);
    let operation = extract_review_labeled_value(&review.response_text, "suggested operation")
        .map(|value| normalize_review_operation(&value))
        .unwrap_or_else(|| infer_review_operation(&lowered).to_string());
    update_autonomous_research_failure_pattern(job, review, &contract, &failure_class, &operation)
}

pub(crate) fn autonomous_research_review_cleanup_required_label(cleanup_text: &str) -> bool {
    let normalized = cleanup_text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    let negative_markers = [
        "not required",
        "no cleanup",
        "cleanup not required",
        "not needed",
        "unless",
    ];
    if negative_markers
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return false;
    }
    normalized == "required"
        || normalized == "yes"
        || normalized.contains("cleanup required")
        || normalized.contains("requires cleanup")
}

pub(crate) fn parse_autonomous_research_repair_plan(
    job: &AutonomousResearchJobState,
    review: &AutonomousResearchReviewState,
    contract: &AutonomousResearchStageContract,
) -> AutonomousResearchRepairPlan {
    let lowered = review.response_text.to_ascii_lowercase();
    let suggested_operation =
        extract_review_labeled_value(&review.response_text, "suggested operation")
            .map(|value| normalize_review_operation(&value))
            .unwrap_or_else(|| infer_review_operation(&lowered).to_string());
    let failure_class = extract_or_infer_failure_class(&review.response_text, &contract.stage_id);
    let rollback_target_stage_id =
        extract_review_labeled_value(&review.response_text, "rollback target")
            .map(|value| sanitize_review_label(&value))
            .filter(|value| !value.is_empty() && value != "none")
            .or_else(|| infer_rollback_target(&failure_class, &contract.stage_id));
    let cleanup_text = extract_review_labeled_value(&review.response_text, "cleanup requirement")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let cleanup_required = if cleanup_text.trim().is_empty() {
        review_requires_cleanup(&lowered, &suggested_operation, &failure_class)
    } else {
        autonomous_research_review_cleanup_required_label(&cleanup_text)
    };
    let cleanup_reason = cleanup_required.then(|| {
        if cleanup_text.trim().is_empty() {
            format!(
                "{} after {} in stage {}",
                failure_class, suggested_operation, contract.stage_id
            )
        } else {
            cleanup_text.trim().to_string()
        }
    });
    let strategy_escalation =
        autonomous_research_strategy_escalation(job, review, contract, &failure_class);
    let mut affected_stage_id = rollback_target_stage_id
        .clone()
        .unwrap_or_else(|| contract.stage_id.clone());
    let mut suggested_operation = suggested_operation;
    let mut rollback_target_stage_id = rollback_target_stage_id;
    let mut cleanup_required = cleanup_required;
    let mut cleanup_reason = cleanup_reason;
    let mut worker_role = repair_worker_role(&contract.stage_id).to_string();
    let mut required_output_artifact_type = contract.artifact_type.clone();
    let mut acceptance_checks = contract.pass_criteria.clone();
    let mut failure_signals = contract.failure_signals.clone();
    if let Some(escalation) = strategy_escalation.as_ref() {
        affected_stage_id = escalation.recommended_stage_id.clone();
        suggested_operation = escalation.recommended_operation.clone();
        rollback_target_stage_id = Some(escalation.recommended_stage_id.clone());
        cleanup_required = true;
        cleanup_reason = Some(escalation.escalation_reason.clone());
        worker_role = "process_diagnostician".to_string();
        required_output_artifact_type = "strategy_update".to_string();
        merge_unique_strings(
            &mut acceptance_checks,
            vec![
                "diagnose repeated failure root cause".to_string(),
                "change task decomposition or research route before retry".to_string(),
                "record DAG route, cleanup requirement, and next-stage constraints".to_string(),
            ],
        );
        merge_unique_strings(
            &mut failure_signals,
            vec![
                "same failure class repeated without strategy change".to_string(),
                "repair loop keeps rewriting prose instead of changing research work".to_string(),
            ],
        );
    }
    let repair_lineage_scope = job
        .stage_execution_id
        .as_deref()
        .unwrap_or(&contract.stage_id);
    let canonical_failure_class = canonical_autonomous_research_failure_class(&failure_class);
    AutonomousResearchRepairPlan {
        schema_version: "autonomous_research_repair_plan.v1".to_string(),
        repair_task_id: format!(
            "repair_{}_{}",
            sanitize_repair_component(repair_lineage_scope),
            sanitize_repair_component(&canonical_failure_class)
        ),
        review_id: review.review_id.clone(),
        affected_stage_id,
        affected_stage_execution_id: job.stage_execution_id.clone(),
        failure_class,
        suggested_operation,
        rollback_target_stage_id,
        cleanup_required,
        cleanup_reason,
        worker_role,
        required_output_artifact_type,
        required_evidence: vec![
            format!("review:{}", review.review_id),
            contract.artifact_path.clone(),
            review
                .review_summary_path
                .clone()
                .unwrap_or_else(|| format!("review:{}", review.review_id)),
            review
                .rubric_path
                .clone()
                .unwrap_or_else(|| "stage_acceptance_rubric_missing".to_string()),
            format!("auto_research_job:{}", job.job_id),
        ],
        acceptance_checks,
        failure_signals,
        source_review_excerpt: compact_single_line(&review.response_text, 1_000),
        strategy_escalation,
        repair_plan_doc_path: None,
        created_at: timestamp_string(),
    }
}

const AUTONOMOUS_RESEARCH_FAILURE_ESCALATION_THRESHOLD: usize = 2;

pub(crate) fn autonomous_research_failure_pattern_key(
    stage_id: &str,
    failure_class: &str,
) -> String {
    format!(
        "{stage_id}::{}",
        canonical_autonomous_research_failure_class(failure_class)
    )
}

pub(crate) fn canonical_autonomous_research_failure_class(failure_class: &str) -> String {
    let normalized = failure_class
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>();
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let has = |word: &str| words.contains(&word);
    if has("evidence") && (has("binding") || has("bound") || has("auditability")) {
        "evidence_binding_gap".to_string()
    } else if has("source") && (has("coverage") || has("search") || has("verification")) {
        "source_coverage_gap".to_string()
    } else if has("unsupported") || has("overclaim") || has("overstated") {
        "unsupported_claim".to_string()
    } else if has("schema") || has("contract") || has("completeness") || has("noncompliance") {
        "artifact_contract_gap".to_string()
    } else if has("provider") || has("process") || has("transport") {
        "process_failure".to_string()
    } else {
        let fallback = words.join("_");
        if fallback.is_empty() {
            "other".to_string()
        } else {
            fallback
        }
    }
}

pub(crate) fn update_autonomous_research_failure_pattern(
    job: &mut AutonomousResearchJobState,
    review: &AutonomousResearchReviewState,
    contract: &AutonomousResearchStageContract,
    failure_class: &str,
    operation: &str,
) -> AutonomousResearchFailurePattern {
    let now = timestamp_string();
    let canonical_failure_class = canonical_autonomous_research_failure_class(failure_class);
    if let Some(pattern) = job.failure_patterns.iter_mut().find(|pattern| {
        pattern.stage_id == contract.stage_id
            && canonical_autonomous_research_failure_class(&pattern.failure_class)
                == canonical_failure_class
    }) {
        pattern.failure_class = canonical_failure_class;
        pattern.count = pattern.count.saturating_add(1);
        pattern.latest_review_id = review.review_id.clone();
        pattern.latest_score = review.score;
        pattern.last_operation = operation.to_string();
        pattern.strategy_escalated = pattern.strategy_escalated
            || pattern.count >= AUTONOMOUS_RESEARCH_FAILURE_ESCALATION_THRESHOLD;
        pattern.updated_at = now;
        return pattern.clone();
    }
    let pattern = AutonomousResearchFailurePattern {
        stage_id: contract.stage_id.clone(),
        failure_class: canonical_failure_class,
        count: 1,
        first_review_id: review.review_id.clone(),
        latest_review_id: review.review_id.clone(),
        latest_score: review.score,
        strategy_escalated: false,
        last_operation: operation.to_string(),
        updated_at: now,
    };
    job.failure_patterns.push(pattern.clone());
    pattern
}

pub(crate) fn autonomous_research_strategy_escalation(
    job: &AutonomousResearchJobState,
    _review: &AutonomousResearchReviewState,
    contract: &AutonomousResearchStageContract,
    failure_class: &str,
) -> Option<AutonomousResearchStrategyEscalation> {
    let canonical_failure_class = canonical_autonomous_research_failure_class(failure_class);
    let pattern = job.failure_patterns.iter().find(|pattern| {
        pattern.stage_id == contract.stage_id
            && canonical_autonomous_research_failure_class(&pattern.failure_class)
                == canonical_failure_class
    })?;
    if pattern.count < AUTONOMOUS_RESEARCH_FAILURE_ESCALATION_THRESHOLD {
        return None;
    }
    let recommended_stage_id = strategy_escalation_stage_for_failure(failure_class, contract);
    let policy =
        crate::orchestration::GoalRunAutomationPolicyProjection::from_mode(job.automation_mode);
    let recommended_operation = if recommended_stage_id == contract.stage_id {
        "repair"
    } else if policy.action_policy("pivot") == Some("allowed") {
        "pivot"
    } else {
        "repair"
    };
    Some(AutonomousResearchStrategyEscalation {
        schema_version: "autonomous_research_strategy_escalation.v1".to_string(),
        pattern_key: autonomous_research_failure_pattern_key(&contract.stage_id, failure_class),
        failure_count: pattern.count,
        threshold: AUTONOMOUS_RESEARCH_FAILURE_ESCALATION_THRESHOLD,
        escalation_reason: format!(
            "Repeated failure class `{}` occurred {} times in stage `{}`; another same-stage prose repair is not sufficient.",
            failure_class, pattern.count, contract.stage_id
        ),
        required_strategy_action: "create a meta-optimize style strategy update: diagnose root cause, change task decomposition or route, add specialist work, and record cleanup constraints before the next review".to_string(),
        recommended_stage_id,
        recommended_operation: recommended_operation.to_string(),
    })
}

pub(crate) fn strategy_escalation_stage_for_failure(
    failure_class: &str,
    contract: &AutonomousResearchStageContract,
) -> String {
    let _ = failure_class;
    if contract.stage_id == "meta-optimize" {
        contract.stage_id.clone()
    } else {
        "meta-optimize".to_string()
    }
}

pub(crate) fn extract_review_labeled_value(text: &str, label: &str) -> Option<String> {
    let marker = format!("{}:", label.to_ascii_lowercase());
    let lowered = text.to_ascii_lowercase();
    let matches = lowered
        .match_indices(&marker)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for index in matches.into_iter().rev() {
        if review_label_match_is_embedded(&lowered, index) {
            continue;
        }
        let value_start = index + marker.len();
        let Some(value) = text
            .get(value_start..)
            .and_then(|rest| rest.lines().next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        return Some(value.to_string());
    }
    None
}

pub(crate) fn review_label_match_is_embedded(lowered_text: &str, marker_index: usize) -> bool {
    if marker_index == 0 {
        return false;
    }
    let prefix = &lowered_text[..marker_index];
    let previous = prefix.chars().next_back().unwrap_or_default();
    if previous.is_ascii_alphanumeric() || previous == '_' {
        return true;
    }
    if previous == '.' {
        return false;
    }
    let line_start = prefix.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let line_prefix = prefix[line_start..].trim();
    if line_prefix.is_empty() {
        return false;
    }
    if matches!(line_prefix, "-" | "*" | ">") {
        return false;
    }
    if let Some(number_prefix) = line_prefix.strip_suffix('.') {
        if !number_prefix.is_empty()
            && number_prefix
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return false;
        }
    }
    true
}

pub(crate) fn parse_review_labeled_score(value: &str) -> Option<u64> {
    let value = value.trim_start_matches(|character: char| {
        character.is_ascii_whitespace()
            || matches!(character, '`' | '*' | '_' | '=' | ':' | '[' | '(')
    });
    let mut digits = String::new();
    for character in value.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
        } else if digits.is_empty() {
            return None;
        } else {
            break;
        }
    }
    digits.parse::<u64>().ok().filter(|score| *score <= 100)
}

pub(crate) fn extract_or_infer_failure_class(review_text: &str, stage_id: &str) -> String {
    extract_review_failure_class(review_text)
        .map(|value| sanitize_review_label(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            infer_failure_class(&review_text.to_ascii_lowercase(), stage_id).to_string()
        })
}

pub(crate) fn extract_review_failure_class(review_text: &str) -> Option<String> {
    extract_review_labeled_value(review_text, "failure class")
        .or_else(|| extract_review_labeled_value(review_text, "failure_class"))
        .or_else(|| extract_review_bulleted_section_value(review_text, "failure class"))
        .or_else(|| extract_review_bulleted_section_value(review_text, "failure_class"))
}

pub(crate) fn extract_review_bulleted_section_value(
    review_text: &str,
    label: &str,
) -> Option<String> {
    let target = normalize_match_text_runtime(label);
    let lines = review_text.lines().collect::<Vec<_>>();
    let heading_index = lines.iter().enumerate().rev().find_map(|(index, line)| {
        let normalized = normalize_match_text_runtime(line.trim().trim_matches('#'));
        if normalized == target {
            Some(index)
        } else {
            None
        }
    })?;
    for line in lines.into_iter().skip(heading_index + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            break;
        }
        if let Some(value) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        if !trimmed.ends_with(':') {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub(crate) fn normalize_review_operation(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    for operation in [
        "retry",
        "repair",
        "pivot",
        "fork",
        "supersede",
        "abandon",
        "human_override",
    ] {
        if lowered.contains(operation) {
            return operation.to_string();
        }
    }
    "repair".to_string()
}

pub(crate) fn infer_review_operation(lowered_review: &str) -> &'static str {
    for operation in ["pivot", "fork", "supersede", "abandon", "retry"] {
        if lowered_review.contains(&format!("suggested operation: {operation}"))
            || lowered_review.contains(&format!("operation {operation}"))
        {
            return operation;
        }
    }
    "repair"
}

pub(crate) fn sanitize_review_label(value: &str) -> String {
    let mut label = value
        .trim()
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        .to_ascii_lowercase();
    if let Some((first, _)) = label.split_once("` with ") {
        label = first.to_string();
    }
    if let Some((first, _)) = label.split_once(" with ") {
        label = first.to_string();
    }
    if let Some((first, _)) = label.split_once(" and ") {
        label = first.to_string();
    }
    label
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
        .to_string()
}

pub(crate) fn sanitize_repair_component(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        sanitized = "stage_failure".to_string();
    }
    sanitized
}

pub(crate) fn infer_failure_class(lowered_review: &str, stage_id: &str) -> &'static str {
    if lowered_review.lines().any(|line| {
        line.trim()
            .strip_prefix("fail blocking_stage_gate:")
            .is_some_and(|gates| {
                gates
                    .split(',')
                    .all(|gate| gate.trim() == "stage_artifact_adoption_snapshot")
            })
    }) {
        return "stage_artifact_adoption_snapshot_drift";
    }
    if stage_id == "literature" && review_looks_like_literature_artifact_failure(lowered_review) {
        return "literature_evidence_failure";
    }
    if lowered_review.contains("unsupported claim") || lowered_review.contains("claim stronger") {
        return "unsupported_claim";
    }
    if lowered_review.contains("missing pdf") || lowered_review.contains("build log") {
        return "paper_compile_failure";
    }
    if (stage_id == "paper-write" || stage_id == "research-review")
        && (lowered_review.contains("markdown-only")
            || lowered_review.contains(".tex")
            || lowered_review.contains("latex")
            || lowered_review.contains("tex source")
            || lowered_review.contains("paper draft"))
    {
        return "paper_write_failure";
    }
    if lowered_review.contains("missing baseline") || lowered_review.contains("missing metric") {
        return "experiment_plan_failure";
    }
    if lowered_review.contains("missing logs") || lowered_review.contains("unrecorded config") {
        return "run_evidence_failure";
    }
    if stage_id == "literature"
        && (lowered_review.contains("unverifiable source")
            || lowered_review.contains("missing-source"))
    {
        return "literature_evidence_failure";
    }
    match stage_id {
        "result-to-claim" => "unsupported_claim",
        "paper-compile" => "paper_compile_failure",
        "paper-write" => "paper_write_failure",
        "experiment-plan" => "experiment_plan_failure",
        "run" => "run_evidence_failure",
        "literature" => "literature_evidence_failure",
        _ => "stage_contract_failure",
    }
}

pub(crate) fn review_looks_like_literature_artifact_failure(lowered_review: &str) -> bool {
    lowered_review.contains("literature matrix")
        || lowered_review.contains("literature_matrix")
        || lowered_review.contains("provider echo removed")
        || lowered_review.contains("prompt pollution")
        || lowered_review.contains("stage contract slice")
        || lowered_review.contains("source-level synthesis")
        || lowered_review.contains("source entries")
        || lowered_review.contains("closest-family")
        || lowered_review.contains("missing-source")
        || lowered_review.contains("unverifiable source")
        || lowered_review.contains("not a literature")
}

pub(crate) fn infer_rollback_target(failure_class: &str, current_stage_id: &str) -> Option<String> {
    let target = match failure_class {
        "literature_evidence_failure" => "literature",
        "novelty_boundary_failure" => "novelty",
        "experiment_plan_failure" => "experiment-plan",
        "implementation_failure" => "implement-solution",
        "run_evidence_failure" => "run",
        "metric_integrity_failure" => "monitor",
        "unsupported_claim" => "result-to-claim",
        "paper_write_failure" => "paper-write",
        "paper_compile_failure" => "paper-compile",
        _ => current_stage_id,
    };
    Some(target.to_string())
}

pub(crate) fn review_requires_cleanup(
    lowered_review: &str,
    operation: &str,
    failure_class: &str,
) -> bool {
    matches!(operation, "pivot" | "fork" | "supersede" | "abandon")
        || lowered_review.contains("cleanup requirement: required")
        || lowered_review.contains("口径")
        || lowered_review.contains("claim deletion")
        || lowered_review.contains("claim narrowing")
        || lowered_review.contains("delete or narrow")
        || lowered_review.contains("replacement of canonical")
        || failure_class == "unsupported_claim"
}

pub(crate) fn repair_worker_role(stage_id: &str) -> &'static str {
    match stage_id {
        "literature" => "literature_repair_worker",
        "novelty" => "novelty_repair_reviewer",
        "experiment-plan" => "experiment_plan_repair_worker",
        "implement-solution" => "implementation_repair_worker",
        "run" => "experiment_run_repair_worker",
        "monitor" => "result_analysis_repair_worker",
        "result-to-claim" => "claim_repair_auditor",
        "paper-write" => "latex_repair_writer",
        "paper-compile" => "paper_build_repair_operator",
        "research-review" => "hard_review_repair_router",
        _ => "stage_repair_worker",
    }
}

pub(crate) fn record_autonomous_research_repair_obligation_waiting_for_main_agent_decision(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    review: &AutonomousResearchReviewState,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let contract = autonomous_research_stage_contract(resolved, job);
    let repair_hint = parse_autonomous_research_repair_plan(job, review, &contract);
    let evidence_refs = [
        format!("review:{}", review.review_id),
        format!("auto_research_job:{}", job.job_id),
        contract.artifact_path.clone(),
        review
            .review_summary_path
            .clone()
            .unwrap_or_else(|| format!("review:{}", review.review_id)),
        review
            .rubric_path
            .clone()
            .unwrap_or_else(|| "stage_acceptance_rubric_missing".to_string()),
        format!("repair_round:{}", job.review_rounds_completed + 1),
    ];
    if let Some(escalation) = repair_hint.strategy_escalation.as_ref() {
        let pattern_ref = format!("failure_pattern:{}", escalation.pattern_key);
        merge_unique_strings(&mut job.artifact_refs, vec![pattern_ref]);
    }
    merge_unique_strings(
        &mut job.artifact_refs,
        evidence_refs
            .iter()
            .filter(|reference| {
                reference.starts_with("research/")
                    || reference.starts_with(".pmcli/")
                    || reference.starts_with("papers/")
            })
            .cloned()
            .collect(),
    );
    append_autonomous_research_job_event(
        resolved,
        job,
        "repair_obligation_recorded_waiting_for_main_agent_decision",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "stage_id": contract.stage_id,
            "artifact_path": contract.artifact_path,
            "stage_execution_id": job.stage_execution_id,
            "review_id": review.review_id,
            "runtime_hint": {
                "failure_class": repair_hint.failure_class,
                "suggested_operation": repair_hint.suggested_operation,
                "rollback_target_stage_id": repair_hint.rollback_target_stage_id,
                "cleanup_required": repair_hint.cleanup_required,
                "strategy_escalation": repair_hint.strategy_escalation
            },
            "evidence_refs": evidence_refs,
            "failure_patterns": job.failure_patterns.clone()
        }),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    Ok(())
}

pub(crate) fn run_autonomous_research_cleanup(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let proposal = artifacts::cleanup_plan(&resolved.data_dir).map_err(|err| {
        internal_failure("research jobs tick", err.to_string()).with_output_json(output_json)
    })?;
    job.cleanup_plan_ids.push(proposal.plan_id.clone());
    let cleanup_summary_ref = write_autonomous_research_cleanup_summary(resolved, job, &proposal)
        .map_err(|err| {
        internal_failure("research jobs tick", err).with_output_json(output_json)
    })?;
    merge_unique_strings(&mut job.artifact_refs, vec![cleanup_summary_ref.clone()]);
    let satisfied_cleanup_obligations =
        mark_autonomous_research_cleanup_obligations_satisfied(job, &cleanup_summary_ref);
    let retired_canonical_artifact_refs =
        retire_canonical_artifacts_for_satisfied_cleanup_obligations(
            resolved,
            job,
            &satisfied_cleanup_obligations,
            &cleanup_summary_ref,
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    merge_unique_strings(
        &mut job.artifact_refs,
        retired_canonical_artifact_refs.clone(),
    );
    append_canonical_event(
        resolved,
        "cleanup_plan",
        "terminal",
        Some("succeeded"),
        "cleanup",
        &proposal.plan_id,
        None,
        serde_json::to_value(&proposal).expect("cleanup proposal should serialize"),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "cleanup_plan_created",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "plan_id": proposal.plan_id,
            "proposed_action_count": proposal.proposed_actions.len(),
            "cleanup_summary_ref": cleanup_summary_ref,
            "satisfied_cleanup_obligation_ids": satisfied_cleanup_obligations,
            "retired_canonical_artifact_refs": retired_canonical_artifact_refs
        }),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    Ok(())
}

pub(crate) fn autonomous_research_cleanup_summary_path(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> PathBuf {
    resolved
        .workspace_root
        .join("research")
        .join("auto")
        .join(&job.job_id)
        .join("cleanup-summary.md")
}

pub(crate) fn autonomous_research_final_cleanup_ready(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> bool {
    !job.cleanup_plan_ids.is_empty()
        && autonomous_research_cleanup_summary_path(resolved, job).exists()
}

pub(crate) fn write_autonomous_research_cleanup_summary(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    proposal: &artifacts::RepoCleanupProposal,
) -> Result<String, String> {
    let path = autonomous_research_cleanup_summary_path(resolved, job);
    let actions = if proposal.proposed_actions.is_empty() {
        "- none".to_string()
    } else {
        proposal
            .proposed_actions
            .iter()
            .map(|action| {
                format!(
                    "- `{}` {} -> {}",
                    action.action_id, action.action_type, action.target_path
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let body = format!(
        "# Cleanup Summary\n\n\
         ## Cleanup Plan\n\n\
         - job_id: `{}`\n\
         - plan_id: `{}`\n\
         - proposed_action_count: `{}`\n\n\
         ## Proposed Actions\n\n{}\n\n\
         ## Active Artifact Refs\n\n{}\n\n\
         ## Canonicality Rule\n\n\
         Cleanup is required when the active research口径 changes through pivot, fork, supersede, abandon, claim deletion, claim narrowing, or replacement of canonical artifacts. The project surface should expose one coherent active line of documents, code, experiments, results, and paper artifacts.\n",
        job.job_id,
        proposal.plan_id,
        proposal.proposed_actions.len(),
        actions,
        if job.artifact_refs.is_empty() {
            "- none".to_string()
        } else {
            job.artifact_refs
                .iter()
                .map(|artifact| format!("- `{artifact}`"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    );
    write_autonomous_research_markdown_with_doc_frame(
        resolved,
        &path,
        AutonomousResearchDocFrameMarkdown {
            title: format!("Astra Cleanup Summary: {}", proposal.plan_id),
            doc_type: "cleanup_summary".to_string(),
            lifecycle: "active".to_string(),
            summary: format!(
                "Cleanup summary for autonomous research job {} and plan {}.",
                job.job_id, proposal.plan_id
            ),
            key_claims: vec![
                "Cleanup keeps the active project surface aligned with the current research口径."
                    .to_string(),
            ],
            decisions: vec![
                format!("Cleanup plan id is {}.", proposal.plan_id),
                format!("Proposed action count is {}.", proposal.proposed_actions.len()),
            ],
            interfaces: vec![
                "projectops_cleanup".to_string(),
                "research_stage_dag".to_string(),
                "docframe_index".to_string(),
                "artifact_registry".to_string(),
            ],
            evidence_refs: vec![
                format!("cleanup_plan:{}", proposal.plan_id),
                format!("auto_research_job:{}", job.job_id),
            ],
            next_actions: vec![
                "Apply or inspect the cleanup plan before treating route-changing artifacts as canonical."
                    .to_string(),
            ],
            non_goals: vec![
                "This summary does not delete files by itself.".to_string(),
            ],
        },
        &body,
    )
}

pub(crate) fn emit_review_event_for_resolved(
    resolved: &ResolvedProject,
    payload: &Value,
) -> Result<(), String> {
    let review_id = payload
        .get("review_id")
        .and_then(Value::as_str)
        .unwrap_or("review");
    append_canonical_event(
        resolved,
        "review",
        "terminal",
        Some("succeeded"),
        "review",
        review_id,
        None,
        payload.clone(),
    )
}

pub(crate) fn opened_review_payload(result: &reviews::ReviewOpenResult) -> Value {
    json!({
        "schema_version": "review_event.v1",
        "review_id": result.packet.review_id,
        "status": result.status,
        "verdict": result.trace.verdict,
        "packet_path": result.packet_path,
        "latest_trace_path": result.latest_trace_path
    })
}

pub(crate) fn list_autonomous_research_jobs(
    resolved: &ResolvedProject,
) -> Result<AutonomousResearchJobListResult, String> {
    let mut jobs = Vec::new();
    let root = autonomous_research_jobs_dir(resolved);
    if root.exists() {
        for entry in std::fs::read_dir(&root).map_err(|err| err.to_string())? {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path().join("job.json");
            if path.exists() {
                jobs.push(read_autonomous_research_job_state_from_path(&path)?);
            }
        }
    }
    jobs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(AutonomousResearchJobListResult {
        schema_version: "autonomous_research_job_list.v1".to_string(),
        project_id: resolved.project_id.clone(),
        total_count: jobs.len(),
        jobs,
    })
}

pub(crate) fn tail_autonomous_research_job(
    resolved: &ResolvedProject,
    job_id: &str,
    limit: usize,
) -> Result<AutonomousResearchJobTailResult, String> {
    let job = read_autonomous_research_job_state(resolved, job_id)?;
    let path = PathBuf::from(&job.job_events_path);
    let mut events = Vec::new();
    if path.exists() {
        let contents = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
        for line in contents
            .lines()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                events.push(value);
            }
        }
    }
    Ok(AutonomousResearchJobTailResult {
        schema_version: "autonomous_research_job_tail.v1".to_string(),
        project_id: resolved.project_id.clone(),
        job_id: job.job_id,
        events_path: path.display().to_string(),
        events,
    })
}

pub(crate) fn stop_autonomous_research_job(
    resolved: &ResolvedProject,
    job_id: &str,
) -> Result<AutonomousResearchJobState, String> {
    let mut job = read_autonomous_research_job_state(resolved, job_id)?;
    let previous_pid = job.background_pid;
    job.status = "stopped".to_string();
    job.phase = "operator_stopped".to_string();
    job.stop_reason = Some("operator_stop_requested".to_string());
    job.updated_at = timestamp_string();
    if let Some(pid) = previous_pid {
        terminate_autonomous_research_process_group(pid);
    }
    write_autonomous_research_job_state(resolved, &job)?;
    append_autonomous_research_job_event(
        resolved,
        &job,
        "stopped",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "reason": job.stop_reason
        }),
    )?;
    Ok(job)
}

pub(crate) fn terminate_autonomous_research_process_group(pid: u32) {
    #[cfg(unix)]
    {
        let pgid = -(pid as libc::pid_t);
        unsafe {
            libc::kill(pgid, libc::SIGTERM);
        }
        std::thread::sleep(Duration::from_millis(250));
        unsafe {
            libc::kill(pgid, libc::SIGKILL);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

pub(crate) fn autonomous_research_job_terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "stopped")
}

pub(crate) fn autonomous_research_jobs_dir(resolved: &ResolvedProject) -> PathBuf {
    resolved.data_dir.join("research").join("jobs")
}

pub(crate) fn autonomous_research_job_dir(resolved: &ResolvedProject, job_id: &str) -> PathBuf {
    autonomous_research_jobs_dir(resolved).join(job_id)
}

pub(crate) fn read_autonomous_research_job_state(
    resolved: &ResolvedProject,
    job_id: &str,
) -> Result<AutonomousResearchJobState, String> {
    read_autonomous_research_job_state_from_path(
        &autonomous_research_job_dir(resolved, job_id).join("job.json"),
    )
}

pub(crate) fn read_autonomous_research_job_state_from_path(
    path: &Path,
) -> Result<AutonomousResearchJobState, String> {
    if !path.exists() {
        return Err(format!(
            "unknown autonomous research job: {}",
            path.display()
        ));
    }
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|err| err.to_string())?)
        .map_err(|err| err.to_string())
}

pub(crate) fn write_autonomous_research_job_state(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
) -> Result<(), String> {
    let path = autonomous_research_job_dir(resolved, &job.job_id).join("job.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut temp_path = path.clone();
    temp_path.set_extension("json.tmp");
    std::fs::write(
        &temp_path,
        serde_json::to_vec_pretty(job).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    std::fs::rename(temp_path, path).map_err(|err| err.to_string())
}

pub(crate) fn append_autonomous_research_job_event(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    event: &str,
    payload: Value,
) -> Result<(), String> {
    let path = autonomous_research_job_dir(resolved, &job.job_id).join("events.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let value = json!({
        "schema_version": "autonomous_research_job_event.v1",
        "timestamp": timestamp_string(),
        "job_id": job.job_id,
        "event": event,
        "status": job.status,
        "phase": job.phase,
        "payload": payload
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&value).map_err(|err| err.to_string())?
    )
    .map_err(|err| err.to_string())
}

pub(crate) fn merge_unique_strings(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

pub(crate) fn resolve_autonomous_research_report_path(
    resolved: &ResolvedProject,
    request: &AutonomousResearchRunRequest,
) -> Result<PathBuf, CommandFailureOutcome> {
    let relative = request
        .report_path
        .clone()
        .unwrap_or_else(|| match request.workflow_profile {
            AutonomousWorkflowProfile::ResearchPipeline => {
                "papers/astra-autonomous-research.md".to_string()
            }
            AutonomousWorkflowProfile::SystemValidation => {
                "reports/astra-system-validation.md".to_string()
            }
        });
    let path = Path::new(&relative);
    if path.is_absolute() {
        return Err(CommandFailureOutcome::usage(
            "research run".to_string(),
            "usage_invalid",
            "research run --report must be workspace-relative".to_string(),
            Some("Pass a relative report path such as `papers/result.md`.".to_string()),
        ));
    }
    let workspace_root = resolved
        .workspace_root
        .canonicalize()
        .map_err(|err| internal_failure("research run", err.to_string()))?;
    let candidate = workspace_root.join(path);
    let parent = candidate
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace_root.clone());
    let resolved_parent = if parent.exists() {
        parent
            .canonicalize()
            .map_err(|err| internal_failure("research run", err.to_string()))?
    } else {
        let existing = parent
            .ancestors()
            .find(|ancestor| ancestor.exists())
            .ok_or_else(|| {
                internal_failure(
                    "research run",
                    format!(
                        "report parent has no existing ancestor: {}",
                        parent.display()
                    ),
                )
            })?;
        existing
            .canonicalize()
            .map_err(|err| internal_failure("research run", err.to_string()))?
    };
    if !resolved_parent.starts_with(&workspace_root) {
        return Err(CommandFailureOutcome::usage(
            "research run".to_string(),
            "usage_invalid",
            "research run --report escaped the workspace".to_string(),
            Some("Pass a report path under the project workspace.".to_string()),
        ));
    }
    Ok(candidate)
}

pub(crate) fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n...[truncated]");
    truncated
}

pub(crate) fn split_autonomous_research_docframe_markdown(
    content: &str,
) -> (Option<String>, String) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (None, content.to_string());
    };
    let mut offset = 4usize;
    for line in rest.split_inclusive('\n') {
        let line_end = offset + line.len();
        if line.trim() == "---" {
            let docframe = content[..line_end].to_string();
            let body = content[line_end..]
                .trim_start_matches(&['\r', '\n'][..])
                .to_string();
            return (Some(docframe), body);
        }
        offset = line_end;
    }
    (None, content.to_string())
}

pub(crate) fn autonomous_research_stage_artifact_review_focus(content: &str) -> String {
    let (docframe, body) = split_autonomous_research_docframe_markdown(content);
    let body = if body.trim().is_empty() {
        content.to_string()
    } else {
        body
    };
    let docframe_excerpt = docframe
        .as_deref()
        .map(|value| truncate_for_prompt(value, AUTONOMOUS_RESEARCH_REVIEW_DOCFRAME_MAX_CHARS))
        .unwrap_or_else(|| "No DocFrame front matter detected.".to_string());
    format!(
        "## Stage Artifact Body (primary review target)\n\n{}\n\n## Stage Artifact DocFrame / Provenance Excerpt (secondary)\n\n{}",
        truncate_for_prompt(&body, AUTONOMOUS_RESEARCH_REVIEW_ARTIFACT_BODY_MAX_CHARS),
        docframe_excerpt
    )
}

pub(crate) fn write_autonomous_research_report(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp_path = path.to_path_buf();
    temp_path.set_extension("md.tmp");
    let mut file = File::create(&temp_path)?;
    file.write_all(contents.as_bytes())?;
    std::fs::rename(temp_path, path)?;
    Ok(())
}

pub(crate) fn write_text_atomic_runtime(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut temp_path = path.to_path_buf();
    temp_path.set_extension("tmp");
    std::fs::write(&temp_path, contents).map_err(|err| err.to_string())?;
    std::fs::rename(temp_path, path).map_err(|err| err.to_string())
}

pub(crate) fn collect_autonomous_research_artifact_refs(resolved: &ResolvedProject) -> Vec<String> {
    let mut refs = Vec::new();
    push_existing_relative_ref(
        &mut refs,
        &resolved.workspace_root,
        &resolved.data_dir.join("goals").join("mission_frame.json"),
    );
    push_existing_relative_ref(
        &mut refs,
        &resolved.workspace_root,
        &resolved.data_dir.join("orchestrations").join("active_run"),
    );
    if let Ok(Some(run)) = crate::orchestration::load_active_run(&resolved.data_dir) {
        push_existing_relative_ref(
            &mut refs,
            &resolved.workspace_root,
            &resolved
                .data_dir
                .join("orchestrations")
                .join(&run.run_id)
                .join("orchestration.json"),
        );
    }
    let scoped_agent_ids = current_goal_run_agent_ids(resolved);
    if let Ok(agents) = crate::agents::list(&resolved.data_dir) {
        for agent in agents.agents {
            if !scoped_agent_ids.is_empty() && !scoped_agent_ids.contains(&agent.agent_id) {
                continue;
            }
            push_existing_relative_ref(
                &mut refs,
                &resolved.workspace_root,
                Path::new(&agent.task_packet_ref),
            );
            push_existing_relative_ref(
                &mut refs,
                &resolved.workspace_root,
                Path::new(&agent.output_manifest_ref),
            );
            if let Ok(inspection) = crate::agents::inspect(&resolved.data_dir, &agent.agent_id) {
                push_existing_relative_ref(
                    &mut refs,
                    &resolved.workspace_root,
                    Path::new(&inspection.runtime_record.task_packet_ref),
                );
                push_existing_relative_ref(
                    &mut refs,
                    &resolved.workspace_root,
                    Path::new(&inspection.runtime_record.output_manifest_ref),
                );
                if let Some(status_ref) = inspection.runtime_record.status_ref.as_deref() {
                    push_existing_relative_ref(
                        &mut refs,
                        &resolved.workspace_root,
                        Path::new(status_ref),
                    );
                }
                for trace_ref in inspection.runtime_record.trace_refs {
                    push_existing_relative_ref(
                        &mut refs,
                        &resolved.workspace_root,
                        Path::new(&trace_ref),
                    );
                }
                for output_ref in inspection.output_manifest.output_refs {
                    push_existing_relative_ref(
                        &mut refs,
                        &resolved.workspace_root,
                        Path::new(&output_ref.r#ref),
                    );
                }
            }
        }
    }
    refs
}

pub(crate) fn autonomous_research_worker_evidence(resolved: &ResolvedProject) -> String {
    let Ok(agents) = crate::agents::list(&resolved.data_dir) else {
        return "none".to_string();
    };
    let scoped_agent_ids = current_goal_run_agent_ids(resolved);
    let mut lines = Vec::new();
    for agent in agents.agents.into_iter().take(6) {
        if !scoped_agent_ids.is_empty() && !scoped_agent_ids.contains(&agent.agent_id) {
            continue;
        }
        let Ok(inspection) = crate::agents::inspect(&resolved.data_dir, &agent.agent_id) else {
            continue;
        };
        lines.push(format!(
            "- agent `{}` role=`{}` status=`{}` intent=`{}`",
            inspection.agent_id,
            inspection.task_packet.role_profile,
            inspection.runtime_record.lifecycle_status,
            compact_single_line(&inspection.task_packet.intent, 140)
        ));
        lines.push(format!(
            "  task_packet: `{}`",
            relative_workspace_ref(
                &resolved.workspace_root,
                Path::new(&inspection.runtime_record.task_packet_ref)
            )
        ));
        lines.push(format!(
            "  output_manifest: `{}`",
            relative_workspace_ref(
                &resolved.workspace_root,
                Path::new(&inspection.runtime_record.output_manifest_ref)
            )
        ));
        if let Some(input_visibility) =
            render_agent_input_visibility_for_main_prompt(resolved, &inspection.agent_id)
        {
            lines.push(input_visibility);
        }
        for output_ref in inspection.output_manifest.output_refs.iter().take(4) {
            let path = Path::new(&output_ref.r#ref);
            let rel = relative_workspace_ref(&resolved.workspace_root, path);
            let excerpt = std::fs::read_to_string(path)
                .map(|content| compact_single_line(&content, 500))
                .unwrap_or_else(|_| "unreadable".to_string());
            lines.push(format!("  {}: `{}` | {}", output_ref.kind, rel, excerpt));
        }
    }
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

fn render_agent_input_visibility_for_main_prompt(
    resolved: &ResolvedProject,
    agent_id: &str,
) -> Option<String> {
    let manifest_path = resolved
        .data_dir
        .join("agents")
        .join(agent_id)
        .join("input_artifact_mounts.json");
    let contents = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest =
        serde_json::from_str::<crate::agents::AgentInputArtifactMountManifest>(&contents).ok()?;
    let visibility = &manifest.workspace_visibility;
    let marker_paths = if visibility.project_marker_paths_visible.is_empty() {
        "none".to_string()
    } else {
        visibility.project_marker_paths_visible.join(", ")
    };
    let visible_root_entries = if visibility.visible_root_entries.is_empty() {
        "none".to_string()
    } else {
        visibility
            .visible_root_entries
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    Some(format!(
        "  input_visibility: manifest=`{}` status=`{}` mounted_inputs=`{}` unmounted_input_refs=`{}` project_marker_paths=`{}` visible_root_entries=`{}`",
        relative_workspace_ref(&resolved.workspace_root, &manifest_path),
        empty_as_unknown_runtime(&visibility.project_marker_status),
        manifest.mounted_inputs.len(),
        manifest.unmounted_input_refs.len(),
        marker_paths,
        visible_root_entries
    ))
}

fn empty_as_unknown_runtime(value: &str) -> &str {
    if value.trim().is_empty() {
        "unknown"
    } else {
        value
    }
}

pub(crate) fn current_goal_run_agent_ids(resolved: &ResolvedProject) -> BTreeSet<String> {
    let Ok(Some(run)) = crate::orchestration::load_active_run(&resolved.data_dir) else {
        return BTreeSet::new();
    };
    let mut ids = BTreeSet::new();
    for step in run.steps {
        for artifact in step.artifacts {
            if let Some((_, agent_id)) = artifact.rsplit_once("::agent:") {
                let agent_id = agent_id.split("::").next().unwrap_or(agent_id);
                if !agent_id.trim().is_empty() {
                    ids.insert(agent_id.to_string());
                }
                continue;
            }
            if let Some(path) = artifact.strip_prefix("agent_task_packet:") {
                if let Some(agent_id) = agent_id_from_agent_artifact_path(path) {
                    ids.insert(agent_id);
                }
                continue;
            }
            if let Some(path) = artifact.strip_prefix("agent_runtime_record:") {
                if let Some(agent_id) = agent_id_from_agent_artifact_path(path) {
                    ids.insert(agent_id);
                }
            }
        }
    }
    ids
}

pub(crate) fn agent_id_from_agent_artifact_path(path: &str) -> Option<String> {
    let mut parts = Path::new(path).components();
    while let Some(part) = parts.next() {
        if part.as_os_str() == "agents" {
            let next = parts.next()?;
            let id = next.as_os_str().to_str()?;
            if !id.trim().is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

pub(crate) fn push_existing_relative_ref(
    refs: &mut Vec<String>,
    workspace_root: &Path,
    path: &Path,
) {
    if !path.exists() {
        return;
    }
    let value = relative_workspace_ref(workspace_root, path);
    if !refs.iter().any(|existing| existing == &value) {
        refs.push(value);
    }
}
