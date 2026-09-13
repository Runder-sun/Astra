const AUTONOMOUS_RESEARCH_BACKGROUND_RUNNER_ENV: &str =
    "RESEARCH_CLI_AUTONOMOUS_RESEARCH_BACKGROUND_RUNNER";
const AUTONOMOUS_RESEARCH_MAX_INLINE_PROVIDER_BACKOFF_SLEEP_MS: u64 = 60_000;

pub(crate) fn start_autonomous_research_job(
    resolved: &ResolvedProject,
    request: &AutonomousResearchRunRequest,
    output_json: bool,
) -> Result<AutonomousResearchJobRunResult, CommandFailureOutcome> {
    let mut job = create_autonomous_research_job_state(resolved, request, output_json)?;
    append_autonomous_research_job_event(
        resolved,
        &job,
        "created",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "status": job.status,
            "phase": job.phase,
            "automation_mode": job.automation_mode.as_str(),
            "workflow_profile": job.workflow_profile.as_str(),
            "stage_task_semantic_review_mode": job.stage_task_semantic_review_mode.as_str(),
            "prompt_sha256": short_sha256_hex(&job.prompt),
            "max_ticks": job.max_ticks,
            "max_runtime_ms": job.max_runtime_ms
        }),
    )
    .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
    append_canonical_event(
        resolved,
        "autonomous_research_job",
        "terminal",
        Some("succeeded"),
        "autonomous_research_job",
        &job.job_id,
        None,
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "job_id": job.job_id,
            "status": job.status,
            "phase": job.phase,
            "automation_mode": job.automation_mode.as_str(),
            "workflow_profile": job.workflow_profile.as_str(),
            "stage_task_semantic_review_mode": job.stage_task_semantic_review_mode.as_str(),
            "job_state_path": job.job_state_path,
            "background": request.background
        }),
    )
    .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
    publish_project_checkpoint(
        resolved,
        &[
            "mission_frame",
            "research",
            "orchestration",
            "agents",
            "review",
            "projectops",
        ],
    )
    .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;

    let mut pid = None;
    if request.background {
        pid = Some(spawn_autonomous_research_job_runner(
            resolved,
            &job.job_id,
            output_json,
        )?);
        job.background_pid = pid;
        job.updated_at = timestamp_string();
        write_autonomous_research_job_state(resolved, &job)
            .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
        append_autonomous_research_job_event(
            resolved,
            &job,
            "background_started",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "pid": pid,
                "resume_command": ["research", "jobs", "resume", job.job_id.as_str()]
            }),
        )
        .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
    } else {
        job = run_autonomous_research_job_loop(resolved, &job.job_id, output_json)?;
    }

    Ok(AutonomousResearchJobRunResult {
        schema_version: "autonomous_research_job_run_result.v1".to_string(),
        status: if request.background {
            "started".to_string()
        } else {
            job.status.clone()
        },
        project_id: resolved.project_id.clone(),
        job_id: job.job_id.clone(),
        background: request.background,
        pid,
        job_state_path: job.job_state_path.clone(),
        job_events_path: job.job_events_path.clone(),
        runner_log_path: job.runner_log_path.clone(),
        status_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "status".to_string(),
        ],
        tail_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "tail".to_string(),
            job.job_id.clone(),
        ],
        resume_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "resume".to_string(),
            job.job_id.clone(),
        ],
        tick_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "tick".to_string(),
            job.job_id.clone(),
        ],
        job,
    })
}

pub(crate) fn create_autonomous_research_job_state(
    resolved: &ResolvedProject,
    request: &AutonomousResearchRunRequest,
    output_json: bool,
) -> Result<AutonomousResearchJobState, CommandFailureOutcome> {
    let prompt = request.prompt.trim().to_string();
    let prompt_hash = short_sha256_hex(&prompt);
    let now = timestamp_string();
    let job_id = format!("arj_{now}_{prompt_hash}");
    let automation_mode = request.automation_mode;
    let workflow_profile = request.workflow_profile;
    let initial_stage_id = match workflow_profile {
        AutonomousWorkflowProfile::ResearchPipeline => "literature",
        AutonomousWorkflowProfile::SystemValidation => "validation",
    };
    let report_path = resolve_autonomous_research_report_path(resolved, request)?;
    let report_ref = relative_workspace_ref(&resolved.workspace_root, &report_path);
    let max_ticks = if request.max_ticks_explicit {
        request.max_ticks
    } else {
        DEFAULT_AUTONOMOUS_RESEARCH_JOB_MAX_TICKS
    };
    let interval_ms = if request.interval_ms_explicit {
        request.interval_ms
    } else {
        DEFAULT_AUTONOMOUS_RESEARCH_JOB_INTERVAL_MS
    };
    let max_runtime_ms = request
        .max_runtime_ms
        .unwrap_or(DEFAULT_AUTONOMOUS_RESEARCH_JOB_RUNTIME_MS);
    let evidence_ref = format!("prompt_sha256:{prompt_hash}");
    let mut mission_evidence_refs = vec![
        crate::goals::AUTONOMOUS_RESEARCH_STAGE_PROTOCOL_REF.to_string(),
        evidence_ref.clone(),
        format!("auto_research_job:{job_id}"),
    ];
    let mission_frame_update = match workflow_profile {
        AutonomousWorkflowProfile::ResearchPipeline => MissionFrameUpdate {
            project_max_goal: format!("Run Astra's staged autonomous research protocol for: {prompt}"),
            milestone_goal: "Advance the research through stage contracts, board-visible agent work, review gates, rollback decisions, and cleanup until the final TeX/PDF paper passes hard review".to_string(),
            current_implementation_goal: format!(
                "Advance auto-research job {job_id} from the literature stage under automation mode {}",
                automation_mode.as_str()
            ),
            non_goals: vec![
                "Do not mark the job completed without an explicit passing review packet".to_string(),
                "Do not write paper claims before the result-to-claim stage accepts evidence".to_string(),
                "Do not claim external literature, experiments, or publication readiness unless backed by stage artifacts".to_string(),
                "Do not bypass the project goal loop, research board, agent dispatch, reviews, or cleanup surfaces".to_string(),
            ],
            success_criteria: vec![
                "A durable auto-research job state exists and can be resumed".to_string(),
                "The run starts at literature as the first real research stage after prompt normalization".to_string(),
                "Each stage synthesizes its required artifact and targets review at that artifact".to_string(),
                "Stage review passes before any advance along StageExecutionMap.default_next_stage_id".to_string(),
                "Paper work starts only after result-to-claim accepts the evidence-to-claim mapping".to_string(),
                "Final completion requires canonical TeX, compiled PDF, claim table, experiment report, cleanup plan, and hard review pass".to_string(),
            ],
            evidence_refs: mission_evidence_refs.clone(),
            risk_notes: vec![
                "Long-running unattended research can consume provider credits and local compute".to_string(),
                format!(
                    "Automation mode was selected from {}; policy gates still apply",
                    request.automation_mode_source
                ),
                format!(
                    "Stage-task semantic review mode is {}; provider-backed worker semantic review consumes model budget only when this mode is provider",
                    request.stage_task_semantic_review_mode
                ),
                "The bounded local goal workers execute board-visible task packets; provider-backed synthesis runs in the main supervisor loop".to_string(),
            ],
            automation_mode: Some(automation_mode),
        },
        AutonomousWorkflowProfile::SystemValidation => MissionFrameUpdate {
            project_max_goal: format!("Run Astra's staged system validation protocol for: {prompt}"),
            milestone_goal: "Produce a canonical system validation report backed by runtime-visible evidence and pass an independent validation review".to_string(),
            current_implementation_goal: format!(
                "Advance system validation job {job_id} under automation mode {}",
                automation_mode.as_str()
            ),
            non_goals: vec![
                "Do not perform literature, novelty, experiment, or paper stages".to_string(),
                "Do not claim validation success without runtime-visible accepted evidence".to_string(),
                "Do not bypass the project goal loop, research board, agent dispatch, or review surfaces".to_string(),
            ],
            success_criteria: vec![
                "A durable system validation job state exists and can be resumed".to_string(),
                "The run starts at validation and remains outside the research paper DAG".to_string(),
                "The canonical system validation report binds each requested check to accepted evidence".to_string(),
                "An independent stage review passes before completion".to_string(),
            ],
            evidence_refs: mission_evidence_refs.clone(),
            risk_notes: vec![
                "Long-running unattended validation can consume provider credits and local compute".to_string(),
                format!(
                    "Automation mode was selected from {}; policy gates still apply",
                    request.automation_mode_source
                ),
                format!(
                    "Stage-task semantic review mode is {}; provider-backed worker semantic review consumes model budget only when this mode is provider",
                    request.stage_task_semantic_review_mode
                ),
            ],
            automation_mode: Some(automation_mode),
        },
    };
    mission_evidence_refs.sort();
    mission_evidence_refs.dedup();
    let mission_status = crate::goals::set(
        &resolved.data_dir,
        &resolved.workspace_root,
        &resolved.project_id,
        MissionFrameUpdate {
            evidence_refs: mission_evidence_refs,
            ..mission_frame_update
        },
    )
    .map_err(|err| goals_failure("research run", err, resolved, output_json))?;
    emit_mission_frame_event(resolved, &mission_status)
        .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
    let mission_frame = mission_status.mission_frame.clone().ok_or_else(|| {
        internal_failure(
            "research run",
            "mission frame was not persisted".to_string(),
        )
        .with_output_json(output_json)
    })?;
    crate::goals::start_fresh_goal_run(
        &resolved.data_dir,
        &resolved.workspace_root,
        &mission_frame,
        &format!("auto research job {job_id} started a fresh goal run"),
    )
    .map_err(|err| goals_failure("research run", err, resolved, output_json))?;
    let review_route = resolve_autonomous_research_provider_route(resolved, &request.review_model);
    crate::goals::save_stage_task_semantic_review_policy(
        &resolved.data_dir,
        &crate::goals::GoalStageTaskSemanticReviewPolicy::new(
            &request.stage_task_semantic_review_mode,
            "autonomous_research_job",
            Some(job_id.clone()),
        )
        .with_route(
            review_route
                .as_ref()
                .and_then(|selection| selection.provider.clone()),
            review_route
                .as_ref()
                .and_then(|selection| selection.model.clone())
                .or_else(|| request.review_model.clone()),
        ),
    )
    .map_err(|err| goals_failure("research run", err, resolved, output_json))?;
    let worker_route = resolve_autonomous_research_agent_team_worker_route(resolved, None);
    crate::goals::save_agent_team_worker_policy(
        &resolved.data_dir,
        &crate::goals::GoalAgentTeamWorkerPolicy::new(
            if worker_route.is_some() {
                "provider"
            } else {
                "local"
            },
            "autonomous_research_job",
            Some(job_id.clone()),
        )
        .with_route(
            worker_route
                .as_ref()
                .and_then(|selection| selection.provider.clone()),
            worker_route
                .as_ref()
                .and_then(|selection| selection.model.clone()),
        ),
    )
    .map_err(|err| goals_failure("research run", err, resolved, output_json))?;

    let (record_kind, record_decisions, open_questions) = match workflow_profile {
        AutonomousWorkflowProfile::ResearchPipeline => (
            "research_goal",
            vec![
                format!("Created durable auto-research job {job_id}"),
                "Entered literature as the first real research stage after prompt normalization"
                    .to_string(),
                "Completion requires TeX/PDF hard review pass; otherwise repair, rollback, or cleanup loops continue".to_string(),
            ],
            vec![
                "Which closest literature families, gaps, and novelty risks must be established before proposal refinement?"
                    .to_string(),
            ],
        ),
        AutonomousWorkflowProfile::SystemValidation => (
            "system_validation_goal",
            vec![
                format!("Created durable system-validation job {job_id}"),
                "Entered validation without joining the research paper DAG".to_string(),
                "Completion requires a canonical validation report and passing independent review"
                    .to_string(),
            ],
            vec!["Which requested system checks still lack runtime-visible evidence?".to_string()],
        ),
    };
    let research_record = research::record(
        &resolved.data_dir,
        &resolved.project_id,
        research::ResearchRecordRequest {
            kind: record_kind.to_string(),
            title: prompt.clone(),
            stage_id: initial_stage_id.to_string(),
            mode: "ready_to_execute".to_string(),
            decisions: record_decisions,
            open_questions,
            evidence_refs: vec![evidence_ref, format!("auto_research_job:{job_id}")],
        },
    )
    .map_err(|err| research_failure("research run", err, resolved, output_json))?;

    let job_dir = autonomous_research_job_dir(resolved, &job_id);
    let job_state_path = job_dir.join("job.json");
    let job_events_path = job_dir.join("events.jsonl");
    let runner_log_path = job_dir.join("runner.log");
    let job = AutonomousResearchJobState {
        schema_version: "autonomous_research_job.v1".to_string(),
        job_id: job_id.clone(),
        project_id: resolved.project_id.clone(),
        prompt,
        workflow_profile: workflow_profile.as_str().to_string(),
        status: "running".to_string(),
        phase: "created".to_string(),
        automation_mode,
        permission_mode: request.permission_mode.clone(),
        review_model: request.review_model.clone(),
        stage_task_semantic_review_mode: request.stage_task_semantic_review_mode.clone(),
        created_at: now.clone(),
        updated_at: now,
        max_ticks,
        ticks_completed: 0,
        interval_ms,
        max_runtime_ms,
        max_review_rounds: request.max_review_rounds,
        review_rounds_completed: 0,
        report_path: report_ref,
        job_state_path: job_state_path.display().to_string(),
        job_events_path: job_events_path.display().to_string(),
        runner_log_path: runner_log_path.display().to_string(),
        thread_id: Some(research_record.thread.thread_id),
        stage_execution_id: Some(research_record.stage_execution.execution_id),
        session_id: None,
        background_pid: None,
        stop_reason: None,
        last_error: None,
        last_review: None,
        last_loop_closure: None,
        tick_summaries: Vec::new(),
        artifact_refs: vec![format!("auto_research_job:{job_id}")],
        warnings: vec![match workflow_profile {
            AutonomousWorkflowProfile::ResearchPipeline => {
                "This job is a durable supervisor loop, not the legacy fixed eight-stage pipeline."
                    .to_string()
            }
            AutonomousWorkflowProfile::SystemValidation => {
                "This job uses the bounded system-validation workflow and does not enter the research paper DAG."
                    .to_string()
            }
        }],
        cleanup_plan_ids: Vec::new(),
        failure_patterns: Vec::new(),
        obligations: Vec::new(),
        continuity_packets: Vec::new(),
        provider_faults: Vec::new(),
        active_provider_fault: None,
        provider_rounds: Vec::new(),
    };
    write_autonomous_research_job_state(resolved, &job)
        .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
    Ok(job)
}

pub(crate) fn spawn_autonomous_research_job_runner(
    resolved: &ResolvedProject,
    job_id: &str,
    output_json: bool,
) -> Result<u32, CommandFailureOutcome> {
    let exe = crate::process_bootstrap::current_cli_executable();
    let job = read_autonomous_research_job_state(resolved, job_id)
        .map_err(|err| internal_failure("research run", err).with_output_json(output_json))?;
    let log_path = PathBuf::from(&job.runner_log_path);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            internal_failure("research run", err.to_string()).with_output_json(output_json)
        })?;
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| {
            internal_failure("research run", err.to_string()).with_output_json(output_json)
        })?;
    let stderr = stdout.try_clone().map_err(|err| {
        internal_failure("research run", err.to_string()).with_output_json(output_json)
    })?;
    let mut command = std::process::Command::new(&exe);
    command
        .current_dir(&resolved.workspace_root)
        .args(["research", "jobs", "resume", job_id, "--json"])
        .stdout(stdout)
        .stderr(stderr);
    if let Ok(state_home) = env::var("RESEARCH_CLI_STATE_HOME") {
        command.env("RESEARCH_CLI_STATE_HOME", state_home);
    }
    if let Ok(config_home) = env::var("RESEARCH_CLI_CONFIG_HOME") {
        command.env("RESEARCH_CLI_CONFIG_HOME", config_home);
    }
    command.env(AUTONOMOUS_RESEARCH_BACKGROUND_RUNNER_ENV, "1");
    crate::process_bootstrap::propagate_current_cli_executable(&mut command, &exe);
    configure_detached_process(&mut command);
    let child = command.spawn().map_err(|err| {
        internal_failure("research run", err.to_string()).with_output_json(output_json)
    })?;
    Ok(child.id())
}

#[cfg(unix)]
pub(crate) fn configure_detached_process(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
pub(crate) fn configure_detached_process(_command: &mut std::process::Command) {}

pub(crate) fn resume_autonomous_research_job(
    resolved: &ResolvedProject,
    request: &AutonomousResearchResumeRequest,
    output_json: bool,
) -> Result<AutonomousResearchJobRunResult, CommandFailureOutcome> {
    let mut job = prepare_autonomous_research_resume_job(resolved, request, output_json)?;
    if request.background {
        let pid = spawn_autonomous_research_job_runner(resolved, &job.job_id, output_json)?;
        job.background_pid = Some(pid);
        job.status = "running".to_string();
        job.stop_reason = None;
        job.updated_at = timestamp_string();
        write_autonomous_research_job_state(resolved, &job).map_err(|err| {
            internal_failure("research jobs resume", err).with_output_json(output_json)
        })?;
        append_autonomous_research_job_event(
            resolved,
            &job,
            "background_resume_started",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "pid": pid,
                "max_ticks": job.max_ticks,
                "ticks_completed": job.ticks_completed,
                "max_runtime_ms": job.max_runtime_ms,
                "resume_command": ["research", "jobs", "resume", job.job_id.as_str()]
            }),
        )
        .map_err(|err| {
            internal_failure("research jobs resume", err).with_output_json(output_json)
        })?;

        return Ok(AutonomousResearchJobRunResult {
            schema_version: "autonomous_research_job_run_result.v1".to_string(),
            status: "started".to_string(),
            project_id: resolved.project_id.clone(),
            job_id: job.job_id.clone(),
            background: true,
            pid: Some(pid),
            job_state_path: job.job_state_path.clone(),
            job_events_path: job.job_events_path.clone(),
            runner_log_path: job.runner_log_path.clone(),
            status_command: vec![
                "research".to_string(),
                "jobs".to_string(),
                "status".to_string(),
            ],
            tail_command: vec![
                "research".to_string(),
                "jobs".to_string(),
                "tail".to_string(),
                job.job_id.clone(),
            ],
            resume_command: vec![
                "research".to_string(),
                "jobs".to_string(),
                "resume".to_string(),
                job.job_id.clone(),
            ],
            tick_command: vec![
                "research".to_string(),
                "jobs".to_string(),
                "tick".to_string(),
                job.job_id.clone(),
            ],
            job,
        });
    }

    let job = run_autonomous_research_job_loop(resolved, &request.job_id, output_json)?;
    Ok(AutonomousResearchJobRunResult {
        schema_version: "autonomous_research_job_run_result.v1".to_string(),
        status: job.status.clone(),
        project_id: resolved.project_id.clone(),
        job_id: job.job_id.clone(),
        background: false,
        pid: None,
        job_state_path: job.job_state_path.clone(),
        job_events_path: job.job_events_path.clone(),
        runner_log_path: job.runner_log_path.clone(),
        status_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "status".to_string(),
        ],
        tail_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "tail".to_string(),
            job.job_id.clone(),
        ],
        resume_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "resume".to_string(),
            job.job_id.clone(),
        ],
        tick_command: vec![
            "research".to_string(),
            "jobs".to_string(),
            "tick".to_string(),
            job.job_id.clone(),
        ],
        job,
    })
}

pub(crate) fn prepare_autonomous_research_resume_job(
    resolved: &ResolvedProject,
    request: &AutonomousResearchResumeRequest,
    output_json: bool,
) -> Result<AutonomousResearchJobState, CommandFailureOutcome> {
    let mut job = read_autonomous_research_job_state(resolved, &request.job_id).map_err(|err| {
        internal_failure("research jobs resume", err).with_output_json(output_json)
    })?;
    let migrated_obligation_ids =
        migrate_autonomous_research_legacy_snapshot_obligations(&mut job);
    let original_max_ticks = job.max_ticks;
    let original_max_runtime_ms = job.max_runtime_ms;
    let reopened_from_stop = job.status == "stopped";
    if reopened_from_stop {
        job.status = "running".to_string();
        job.phase = "resume_pending".to_string();
        job.stop_reason = None;
        job.last_error = None;
        job.background_pid = None;
    }

    if let Some(max_ticks) = request.max_ticks {
        if max_ticks <= job.ticks_completed {
            return Err(CommandFailureOutcome::usage(
                "research jobs resume".to_string(),
                "usage_invalid",
                format!(
                    "research jobs resume --max-ticks must be greater than current ticks_completed ({})",
                    job.ticks_completed
                ),
                Some("Use `--additional-ticks <n>` to extend from the current tick count.".to_string()),
            )
            .with_output_json(output_json));
        }
        job.max_ticks = max_ticks;
    }

    if let Some(additional_ticks) = request.additional_ticks {
        let target_max_ticks = job.ticks_completed.saturating_add(additional_ticks);
        if target_max_ticks > job.max_ticks {
            job.max_ticks = target_max_ticks;
        }
    }

    if let Some(max_runtime_ms) = request.max_runtime_ms {
        job.max_runtime_ms = max_runtime_ms;
    }

    if reopened_from_stop
        || !migrated_obligation_ids.is_empty()
        || job.max_ticks != original_max_ticks
        || job.max_runtime_ms != original_max_runtime_ms
    {
        job.updated_at = timestamp_string();
        write_autonomous_research_job_state(resolved, &job).map_err(|err| {
            internal_failure("research jobs resume", err).with_output_json(output_json)
        })?;
        if reopened_from_stop {
            append_autonomous_research_job_event(
                resolved,
                &job,
                "resume_state_reopened",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "previous_status": "stopped",
                    "status": job.status,
                    "phase": job.phase
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs resume", err).with_output_json(output_json)
            })?;
        }
    }

    if !migrated_obligation_ids.is_empty() {
        append_autonomous_research_job_event(
            resolved,
            &job,
            "resume_state_migrated",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "migration": "legacy_snapshot_only_obligation",
                "obligation_ids": migrated_obligation_ids,
                "last_review_id": job.last_review.as_ref().map(|review| review.review_id.clone())
            }),
        )
        .map_err(|err| {
            internal_failure("research jobs resume", err).with_output_json(output_json)
        })?;
    }

    if job.max_ticks != original_max_ticks || job.max_runtime_ms != original_max_runtime_ms {
        append_autonomous_research_job_event(
            resolved,
            &job,
            "resume_budget_updated",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "previous_max_ticks": original_max_ticks,
                "max_ticks": job.max_ticks,
                "ticks_completed": job.ticks_completed,
                "previous_max_runtime_ms": original_max_runtime_ms,
                "max_runtime_ms": job.max_runtime_ms
            }),
        )
        .map_err(|err| {
            internal_failure("research jobs resume", err).with_output_json(output_json)
        })?;
    }

    Ok(job)
}

fn pause_autonomous_research_job_for_runtime_budget(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    elapsed_ms: u128,
    pending_sleep_ms: Option<u64>,
    reason: &str,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    job.status = "running".to_string();
    job.phase = "time_budget_pause".to_string();
    job.stop_reason = Some("max_runtime_ms_elapsed_resume_later".to_string());
    job.background_pid = None;
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, job)
        .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "paused_for_runtime_budget",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "elapsed_ms": elapsed_ms,
            "max_runtime_ms": job.max_runtime_ms,
            "pending_sleep_ms": pending_sleep_ms,
            "active_provider_fault": job.active_provider_fault,
            "reason": reason
        }),
    )
    .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))
}

pub(crate) fn autonomous_research_should_pause_for_provider_backoff_sleep(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    sleep_ms: u64,
    background_runner: bool,
) -> bool {
    if background_runner {
        return false;
    }
    if sleep_ms <= AUTONOMOUS_RESEARCH_MAX_INLINE_PROVIDER_BACKOFF_SLEEP_MS {
        return false;
    }
    let Some(fault) = job.active_provider_fault.as_ref() else {
        return false;
    };
    if fault.operator_gate_required {
        return false;
    }
    if autonomous_research_provider_fault_can_use_recovery_route(fault)
        && autonomous_research_available_provider_recovery_route(resolved, job).is_some()
    {
        return false;
    }
    !autonomous_research_provider_fault_is_ready(fault)
}

pub(crate) fn autonomous_research_provider_backoff_sleep_ms(
    background_runner: bool,
    sleep_ms: u64,
) -> u64 {
    if background_runner {
        sleep_ms.min(AUTONOMOUS_RESEARCH_MAX_INLINE_PROVIDER_BACKOFF_SLEEP_MS)
    } else {
        sleep_ms
    }
}

fn pause_autonomous_research_job_for_provider_backoff(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    sleep_ms: u64,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    let fault = job.active_provider_fault.clone();
    let next_retry_at = fault.as_ref().and_then(|fault| fault.next_retry_at.clone());
    job.status = "running".to_string();
    job.phase = "provider_backoff".to_string();
    job.stop_reason = Some("provider_retry_backoff_resume_later".to_string());
    job.background_pid = None;
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, job)
        .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "provider_backoff_paused",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "sleep_ms": sleep_ms,
            "inline_sleep_cap_ms": AUTONOMOUS_RESEARCH_MAX_INLINE_PROVIDER_BACKOFF_SLEEP_MS,
            "next_retry_at": next_retry_at,
            "fault": fault,
            "reason": "provider retry wait is persisted instead of blocking the supervisor process"
        }),
    )
    .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))
}

fn clear_autonomous_research_background_runner_pid(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    current_pid: u32,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    if job.background_pid != Some(current_pid) {
        return Ok(());
    }
    job.background_pid = None;
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, job).map_err(|err| {
        internal_failure("research jobs resume", err).with_output_json(output_json)
    })?;
    append_autonomous_research_job_event(
        resolved,
        job,
        "background_runner_finished",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "pid": current_pid,
            "status": job.status,
            "phase": job.phase,
            "reason": job.stop_reason
        }),
    )
    .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))
}

pub(crate) fn run_autonomous_research_job_loop(
    resolved: &ResolvedProject,
    job_id: &str,
    output_json: bool,
) -> Result<AutonomousResearchJobState, CommandFailureOutcome> {
    let start_ms = timestamp_millis();
    let mut job = read_autonomous_research_job_state(resolved, job_id).map_err(|err| {
        internal_failure("research jobs resume", err).with_output_json(output_json)
    })?;
    if autonomous_research_job_terminal(&job.status) {
        return Ok(job);
    }
    let previous_background_pid = job.background_pid;
    let background_runner = env::var_os(AUTONOMOUS_RESEARCH_BACKGROUND_RUNNER_ENV).is_some();
    let current_pid = std::process::id();
    job.background_pid = if background_runner {
        Some(current_pid)
    } else {
        None
    };
    job.updated_at = timestamp_string();
    append_autonomous_research_job_event(
        resolved,
        &job,
        "resume_started",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "ticks_completed": job.ticks_completed,
            "max_ticks": job.max_ticks,
            "max_runtime_ms": job.max_runtime_ms,
            "previous_background_pid": previous_background_pid,
            "background_runner": background_runner,
            "runner_pid": current_pid
        }),
    )
    .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))?;
    let continuity_packet =
        write_autonomous_research_continuity_packet(resolved, &mut job, "resume_started", None)
            .map_err(|err| {
                internal_failure("research jobs resume", err).with_output_json(output_json)
            })?;
    write_autonomous_research_job_state(resolved, &job).map_err(|err| {
        internal_failure("research jobs resume", err).with_output_json(output_json)
    })?;
    append_autonomous_research_job_event(
        resolved,
        &job,
        "continuity_packet_written",
        serde_json::to_value(&continuity_packet)
            .expect("continuity packet summary should serialize"),
    )
    .map_err(|err| internal_failure("research jobs resume", err).with_output_json(output_json))?;

    loop {
        if let Ok(latest_job) = read_autonomous_research_job_state(resolved, job_id) {
            if autonomous_research_job_terminal(&latest_job.status) {
                append_autonomous_research_job_event(
                    resolved,
                    &latest_job,
                    "resume_observed_terminal_state",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "status": latest_job.status,
                        "phase": latest_job.phase,
                        "reason": latest_job.stop_reason
                    }),
                )
                .map_err(|err| {
                    internal_failure("research jobs resume", err).with_output_json(output_json)
                })?;
                job = latest_job;
                break;
            }
        }
        let elapsed_ms = timestamp_millis().saturating_sub(start_ms);
        if elapsed_ms >= u128::from(job.max_runtime_ms) {
            pause_autonomous_research_job_for_runtime_budget(
                resolved,
                &mut job,
                elapsed_ms,
                None,
                "runtime_budget_elapsed_before_next_tick",
                output_json,
            )?;
            break;
        }
        if job.ticks_completed >= job.max_ticks {
            job.status = "running".to_string();
            job.phase = "tick_budget_pause".to_string();
            job.stop_reason = Some("max_ticks_elapsed_resume_later".to_string());
            job.updated_at = timestamp_string();
            write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                internal_failure("research jobs resume", err).with_output_json(output_json)
            })?;
            break;
        }
        let ticks_completed_before_tick = job.ticks_completed;
        let deadline_ms = start_ms.saturating_add(u128::from(job.max_runtime_ms));
        let tick = match tick_autonomous_research_job_with_deadline(
            resolved,
            job_id,
            Some(deadline_ms),
            output_json,
        ) {
            Ok(tick) => tick,
            Err(err) => {
                let message = err.envelope.error.message.clone();
                job.status = "blocked".to_string();
                job.phase = "supervisor_error".to_string();
                job.stop_reason = Some(err.envelope.error.code.clone());
                job.last_error = Some(message.clone());
                if background_runner {
                    job.background_pid = Some(current_pid);
                } else {
                    job.background_pid = None;
                }
                job.updated_at = timestamp_string();
                write_autonomous_research_job_state(resolved, &job).map_err(|write_err| {
                    internal_failure("research jobs resume", write_err)
                        .with_output_json(output_json)
                })?;
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "supervisor_error",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "code": err.envelope.error.code,
                        "message": message,
                        "hint": err.envelope.error.hint
                    }),
                )
                .map_err(|event_err| {
                    internal_failure("research jobs resume", event_err)
                        .with_output_json(output_json)
                })?;
                break;
            }
        };
        job = tick.job;
        if tick.should_continue && job.ticks_completed == ticks_completed_before_tick {
            job.ticks_completed = job.ticks_completed.saturating_add(1);
            job.updated_at = timestamp_string();
            write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                internal_failure("research jobs resume", err).with_output_json(output_json)
            })?;
            append_autonomous_research_job_event(
                resolved,
                &job,
                "non_progress_tick_counted",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "previous_ticks_completed": ticks_completed_before_tick,
                    "ticks_completed": job.ticks_completed,
                    "phase": job.phase,
                    "reason": tick.phase
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs resume", err).with_output_json(output_json)
            })?;
        }
        if !tick.should_continue {
            break;
        }
        if job.ticks_completed >= job.max_ticks {
            job.status = "running".to_string();
            job.phase = "tick_budget_pause".to_string();
            job.stop_reason = Some("max_ticks_elapsed_resume_later".to_string());
            if background_runner {
                job.background_pid = Some(current_pid);
            }
            job.updated_at = timestamp_string();
            write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                internal_failure("research jobs resume", err).with_output_json(output_json)
            })?;
            break;
        }
        if let Ok(latest_job) = read_autonomous_research_job_state(resolved, job_id) {
            if autonomous_research_job_terminal(&latest_job.status) {
                job = latest_job;
                break;
            }
        }
        let sleep_ms = autonomous_research_next_sleep_ms(resolved, &job);
        if sleep_ms > 0 {
            let elapsed_ms = timestamp_millis().saturating_sub(start_ms);
            let remaining_runtime_ms = u128::from(job.max_runtime_ms).saturating_sub(elapsed_ms);
            if remaining_runtime_ms == 0 || u128::from(sleep_ms) >= remaining_runtime_ms {
                pause_autonomous_research_job_for_runtime_budget(
                    resolved,
                    &mut job,
                    elapsed_ms,
                    Some(sleep_ms),
                    "next_sleep_would_exceed_runtime_budget",
                    output_json,
                )?;
                break;
            }
            if autonomous_research_should_pause_for_provider_backoff_sleep(
                resolved,
                &job,
                sleep_ms,
                background_runner,
            ) {
                pause_autonomous_research_job_for_provider_backoff(
                    resolved,
                    &mut job,
                    sleep_ms,
                    output_json,
                )?;
                break;
            }
            let sleep_for_ms = autonomous_research_provider_backoff_sleep_ms(
                background_runner,
                sleep_ms,
            );
            std::thread::sleep(Duration::from_millis(sleep_for_ms));
        }
    }
    if background_runner {
        if job.background_pid != Some(current_pid) {
            job.background_pid = Some(current_pid);
        }
        clear_autonomous_research_background_runner_pid(
            resolved,
            &mut job,
            current_pid,
            output_json,
        )?;
    }
    Ok(job)
}

pub(crate) fn tick_autonomous_research_job(
    resolved: &ResolvedProject,
    job_id: &str,
    output_json: bool,
) -> Result<AutonomousResearchJobTickResult, CommandFailureOutcome> {
    tick_autonomous_research_job_with_deadline(resolved, job_id, None, output_json)
}

pub(crate) fn tick_autonomous_research_job_with_deadline(
    resolved: &ResolvedProject,
    job_id: &str,
    resume_deadline_ms: Option<u128>,
    output_json: bool,
) -> Result<AutonomousResearchJobTickResult, CommandFailureOutcome> {
    let mut job = read_autonomous_research_job_state(resolved, job_id)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    if let Some(fault) = job.active_provider_fault.clone() {
        if autonomous_research_provider_fault_is_misclassified_worker_loop_exhaustion(&fault) {
            let released_obligations =
                mark_autonomous_research_provider_fault_obligations_satisfied_by_operator_release(
                    &mut job, &fault,
                );
            job.active_provider_fault = None;
            job.status = "running".to_string();
            job.phase = "worker_loop_repair_pending".to_string();
            job.stop_reason = Some("worker_loop_exhaustion_requires_main_agent_repair".to_string());
            job.last_error = Some(fault.message.clone());
            job.updated_at = timestamp_string();
            write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
            append_autonomous_research_job_event(
                resolved,
                &job,
                "misclassified_worker_loop_exhaustion_released",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "fault": fault,
                    "satisfied_provider_fault_obligation_ids": released_obligations,
                    "reason": "agent-team loop exhaustion is a worker/task repair issue, not a provider operator gate"
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
            return Ok(AutonomousResearchJobTickResult {
                schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                status: "ticked".to_string(),
                project_id: resolved.project_id.clone(),
                job_id: job.job_id.clone(),
                tick_index: job.ticks_completed,
                job_status: job.status.clone(),
                phase: job.phase.clone(),
                should_continue: true,
                review_passed: false,
                tick_summary: None,
                review: None,
                job,
            });
        }
    }
    let provider_fault_refreshed = refresh_autonomous_research_active_provider_fault(&mut job);
    if let Some((previous_fault, refreshed_fault)) = provider_fault_refreshed.as_ref() {
        write_autonomous_research_job_state(resolved, &job).map_err(|err| {
            internal_failure("research jobs tick", err).with_output_json(output_json)
        })?;
        append_autonomous_research_job_event(
            resolved,
            &job,
            "provider_fault_policy_refreshed",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "previous_fault": previous_fault,
                "refreshed_fault": refreshed_fault,
                "reason": "active provider fault was reclassified by the current provider failure policy"
            }),
        )
        .map_err(|err| {
            internal_failure("research jobs tick", err).with_output_json(output_json)
        })?;
    }
    if let Some((fault, obligation_ids, evidence_refs)) =
        clear_active_agent_team_provider_fault_after_existing_worker_evidence(resolved, &mut job)
    {
        write_autonomous_research_job_state(resolved, &job).map_err(|err| {
            internal_failure("research jobs tick", err).with_output_json(output_json)
        })?;
        append_autonomous_research_job_event(
            resolved,
            &job,
            "provider_fault_recovered_by_accepted_worker_evidence",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "fault": fault,
                "obligation_ids": obligation_ids,
                "evidence_refs": evidence_refs,
                "reason": "accepted agent-team worker evidence superseded the active provider fault"
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    }
    if autonomous_research_job_terminal(&job.status) {
        return Ok(AutonomousResearchJobTickResult {
            schema_version: "autonomous_research_job_tick_result.v1".to_string(),
            status: "terminal".to_string(),
            project_id: resolved.project_id.clone(),
            job_id: job.job_id.clone(),
            tick_index: job.ticks_completed,
            job_status: job.status.clone(),
            phase: job.phase.clone(),
            should_continue: false,
            review_passed: job
                .last_review
                .as_ref()
                .map(|review| review.verdict == "pass")
                .unwrap_or(false),
            tick_summary: None,
            review: job.last_review.clone(),
            job,
        });
    }

    if let Some(fault) = job.active_provider_fault.clone() {
        if fault.operator_gate_required {
            if let Some(selection) =
                autonomous_research_operator_gate_recovery_route(resolved, &job, &fault)
            {
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "provider_operator_gate_bypassed_by_failover",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "fault": fault,
                        "provider": selection.provider,
                        "model": selection.model,
                        "provider_source": selection.provider_source,
                        "reason": "configured provider recovery route is available for an operator-gated provider fault"
                    }),
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
            } else if autonomous_research_provider_operator_gate_can_release(resolved, &fault) {
                let released_obligations =
                    mark_autonomous_research_provider_fault_obligations_satisfied_by_operator_release(
                        &mut job,
                        &fault,
                    );
                job.active_provider_fault = None;
                job.stop_reason = None;
                job.last_error = None;
                job.updated_at = timestamp_string();
                write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "provider_operator_gate_released",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "fault": fault,
                        "reason": "provider credential/configuration is now available",
                        "satisfied_provider_fault_obligation_ids": released_obligations
                    }),
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
            } else {
                job.status = "running".to_string();
                job.phase = "provider_waiting_for_operator".to_string();
                job.stop_reason = Some("provider_operator_gate_required".to_string());
                job.updated_at = timestamp_string();
                write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "provider_operator_gate",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "fault": fault
                    }),
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
                return Ok(AutonomousResearchJobTickResult {
                    schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                    status: "ticked".to_string(),
                    project_id: resolved.project_id.clone(),
                    job_id: job.job_id.clone(),
                    tick_index: job.ticks_completed,
                    job_status: job.status.clone(),
                    phase: job.phase.clone(),
                    should_continue: true,
                    review_passed: false,
                    tick_summary: None,
                    review: None,
                    job,
                });
            }
        } else if autonomous_research_provider_fault_can_use_recovery_route(&fault)
            && autonomous_research_available_provider_recovery_route(resolved, &job).is_some()
        {
            append_autonomous_research_job_event(
                resolved,
                &job,
                "provider_backoff_bypassed_by_failover",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "fault": fault,
                    "reason": "configured provider recovery route is ready"
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
        } else if !autonomous_research_provider_fault_is_ready(&fault) {
            job.status = "running".to_string();
            job.phase = "provider_backoff".to_string();
            job.stop_reason = Some("provider_retry_backoff_active".to_string());
            job.updated_at = timestamp_string();
            write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
            append_autonomous_research_job_event(
                resolved,
                &job,
                "provider_backoff_active",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "fault": fault
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
            return Ok(AutonomousResearchJobTickResult {
                schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                status: "ticked".to_string(),
                project_id: resolved.project_id.clone(),
                job_id: job.job_id.clone(),
                tick_index: job.ticks_completed,
                job_status: job.status.clone(),
                phase: job.phase.clone(),
                should_continue: true,
                review_passed: false,
                tick_summary: None,
                review: None,
                job,
            });
        } else {
            let released_obligations =
                mark_autonomous_research_provider_fault_obligations_satisfied_by_operator_release(
                    &mut job, &fault,
                );
            job.active_provider_fault = None;
            job.stop_reason = None;
            job.updated_at = timestamp_string();
            write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
            append_autonomous_research_job_event(
                resolved,
                &job,
                "provider_backoff_released",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "fault": fault,
                    "satisfied_provider_fault_obligation_ids": released_obligations
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
        }
    }

    let current_tick_index = job.ticks_completed;
    if let Some(applied) = apply_autonomous_research_main_agent_route_change_if_ready(
        resolved,
        &mut job,
        current_tick_index,
        output_json,
    )? {
        return Ok(AutonomousResearchJobTickResult {
            schema_version: "autonomous_research_job_tick_result.v1".to_string(),
            status: "ticked".to_string(),
            project_id: resolved.project_id.clone(),
            job_id: job.job_id.clone(),
            tick_index: job.ticks_completed,
            job_status: job.status.clone(),
            phase: job.phase.clone(),
            should_continue: true,
            review_passed: applied.previous_review_passed,
            tick_summary: None,
            review: applied.previous_review,
            job,
        });
    }

    job.status = "running".to_string();
    job.phase = "goal_tick".to_string();
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, &job)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(
        resolved,
        &job,
        "tick_started",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "tick_index": job.ticks_completed + 1,
            "phase": job.phase,
            "stage_contract": autonomous_research_stage_contract_event(&autonomous_research_stage_contract(resolved, &job))
        }),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;

    let tick_request = routines::RoutineRunDueRequest {
        trigger_kind: Some("api".to_string()),
        limit: None,
        stale_after_sec: 120,
        max_retry_attempts: routines::DEFAULT_MAX_RETRY_ATTEMPTS,
        retry_backoff_ms: routines::DEFAULT_RETRY_BACKOFF_MS,
    };
    let review_route = autonomous_research_select_provider_route_for_review_model(
        resolved,
        &job.review_model,
        Some(&job),
    );
    if review_route
        .as_ref()
        .map(|selection| selection.failover_used)
        .unwrap_or(false)
    {
        let selection = review_route.as_ref().expect("review route checked above");
        append_autonomous_research_job_event(
            resolved,
            &job,
            "provider_failover_selected",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "source": "stage_task_semantic_review",
                "provider": selection.provider,
                "model": selection.model,
                "active_provider_fault": job.active_provider_fault
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    }
    crate::goals::save_stage_task_semantic_review_policy(
        &resolved.data_dir,
        &crate::goals::GoalStageTaskSemanticReviewPolicy::new(
            &job.stage_task_semantic_review_mode,
            "autonomous_research_job_tick",
            Some(job.job_id.clone()),
        )
        .with_route(
            review_route
                .as_ref()
                .and_then(|selection| selection.provider.clone()),
            review_route
                .as_ref()
                .and_then(|selection| selection.model.clone())
                .or_else(|| job.review_model.clone()),
        ),
    )
    .map_err(|err| goals_failure("research jobs tick", err, resolved, output_json))?;
    let worker_route = resolve_autonomous_research_agent_team_worker_route(resolved, Some(&job));
    crate::goals::save_agent_team_worker_policy(
        &resolved.data_dir,
        &crate::goals::GoalAgentTeamWorkerPolicy::new(
            if worker_route.is_some() {
                "provider"
            } else {
                "local"
            },
            "autonomous_research_job_tick",
            Some(job.job_id.clone()),
        )
        .with_route(
            worker_route
                .as_ref()
                .and_then(|selection| selection.provider.clone()),
            worker_route
                .as_ref()
                .and_then(|selection| selection.model.clone()),
        ),
    )
    .map_err(|err| goals_failure("research jobs tick", err, resolved, output_json))?;
    let pre_tick_contract = autonomous_research_stage_contract(resolved, &job);
    let pre_tick_decision_refs =
        sync_main_agent_worker_artifact_decisions_into_accepted_worker_evidence(
            resolved,
            &job,
            &pre_tick_contract,
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    if !pre_tick_decision_refs.is_empty() {
        merge_unique_strings(&mut job.artifact_refs, pre_tick_decision_refs);
        job.updated_at = timestamp_string();
        write_autonomous_research_job_state(resolved, &job).map_err(|err| {
            internal_failure("research jobs tick", err).with_output_json(output_json)
        })?;
    }
    let tick = match run_goal_tick_once_with_options(
        resolved,
        &tick_request,
        output_json,
        GoalTickRunOptions {
            publish_events: false,
            publish_checkpoint: false,
        },
    ) {
        Ok(tick) => tick,
        Err(err) => {
            if let Some(fault) =
                autonomous_research_stage_task_semantic_review_provider_fault_from_failure(
                    resolved, &job, &err,
                )
            {
                let tick_index = job.ticks_completed.saturating_add(1);
                return schedule_autonomous_research_provider_backoff_tick(
                    resolved,
                    &mut job,
                    tick_index,
                    None,
                    fault,
                    "stage_task_semantic_review",
                    "stage_task_semantic_review_provider_failure_backoff",
                    output_json,
                );
            }
            return Err(err);
        }
    };
    let tick_index = job.ticks_completed + 1;
    let tick_summary = AutonomousResearchTickSummary {
        tick_index,
        status: tick.status.clone(),
        dispatch_count: tick.dispatch_count,
        accepted: tick.accepted,
        loop_status: tick.loop_closure.status.clone(),
        next_recommended_action: tick.loop_closure.next_recommended_action.clone(),
    };
    job.ticks_completed = tick_index;
    job.last_error = None;
    job.stop_reason = None;
    job.last_loop_closure = Some(tick.loop_closure.clone());
    job.tick_summaries.push(tick_summary.clone());
    merge_unique_strings(
        &mut job.artifact_refs,
        collect_autonomous_research_artifact_refs(resolved),
    );
    if let Some(acceptance) = tick.goal_advance.acceptance.as_ref() {
        if let Some(fault) =
            autonomous_research_agent_team_worker_provider_fault_from_acceptance(&job, acceptance)
        {
            return schedule_autonomous_research_provider_backoff_tick(
                resolved,
                &mut job,
                tick_index,
                Some(tick_summary),
                fault,
                "agent_team_worker",
                "agent_team_worker_provider_failure_backoff",
                output_json,
            );
        }
        if let Some(index_ref) =
            record_autonomous_research_accepted_worker_evidence(resolved, &job, acceptance)
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?
        {
            merge_unique_strings(&mut job.artifact_refs, vec![index_ref]);
        }
        if let Some((fault, obligation_ids, evidence_refs)) =
            clear_active_agent_team_provider_fault_after_worker_acceptance(&mut job, acceptance)
        {
            append_autonomous_research_job_event(
                resolved,
                &job,
                "provider_fault_recovered_by_agent_team_retry",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "fault": fault,
                    "accepted_agent_id": acceptance.agent_id,
                    "obligation_ids": obligation_ids,
                    "evidence_refs": evidence_refs,
                    "reason": "agent-team worker retry produced accepted evidence"
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
        }
    }
    let decision_contract = autonomous_research_stage_contract(resolved, &job);
    let main_agent_decision_refs =
        sync_main_agent_worker_artifact_decisions_into_accepted_worker_evidence(
            resolved,
            &job,
            &decision_contract,
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    merge_unique_strings(&mut job.artifact_refs, main_agent_decision_refs);
    let accepted_worker_evidence_index =
        load_autonomous_research_accepted_worker_evidence_index(resolved, &job);
    let adopted_stage_refs = process_main_agent_stage_artifact_adoptions(
        resolved,
        &job,
        &decision_contract,
        accepted_worker_evidence_index.as_ref(),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    merge_unique_strings(&mut job.artifact_refs, adopted_stage_refs.clone());
    let evidence_satisfied_obligations =
        mark_autonomous_research_obligations_satisfied_by_accepted_evidence(resolved, &mut job);
    if !evidence_satisfied_obligations.is_empty() {
        append_autonomous_research_job_event(
            resolved,
            &job,
            "obligations_satisfied_by_accepted_evidence",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "obligation_ids": evidence_satisfied_obligations
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    }
    let stage_artifact_repair_satisfied_obligations =
        mark_autonomous_research_obligations_satisfied_by_stage_artifact_repair(
            resolved,
            &mut job,
            &adopted_stage_refs,
        );
    if !stage_artifact_repair_satisfied_obligations.is_empty() {
        append_autonomous_research_job_event(
            resolved,
            &job,
            "obligations_satisfied_by_stage_artifact_repair",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "obligation_ids": stage_artifact_repair_satisfied_obligations,
                "stage_artifact_path": decision_contract.artifact_path,
                "adopted_refs": adopted_stage_refs
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    }

    persist_autonomous_research_job_phase(
        resolved,
        &mut job,
        "goal_tick_completed",
        "goal_tick_completed",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "tick_summary": tick_summary.clone()
        }),
        output_json,
    )?;

    if let Some((wait_reason, missing_task_types)) =
        autonomous_research_worker_acceptance_wait_reason(resolved, &job, &tick_summary)
    {
        persist_autonomous_research_job_phase(
            resolved,
            &mut job,
            "waiting_for_worker_acceptance",
            "waiting_for_worker_acceptance",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "tick_summary": tick_summary.clone(),
                "reason": wait_reason,
                "missing_required_stage_task_types": missing_task_types
            }),
            output_json,
        )?;
        return Ok(AutonomousResearchJobTickResult {
            schema_version: "autonomous_research_job_tick_result.v1".to_string(),
            status: "ticked".to_string(),
            project_id: resolved.project_id.clone(),
            job_id: job.job_id.clone(),
            tick_index,
            job_status: job.status.clone(),
            phase: job.phase.clone(),
            should_continue: !autonomous_research_job_terminal(&job.status),
            review_passed: false,
            tick_summary: Some(tick_summary),
            review: None,
            job,
        });
    }

    let should_run_main_agent =
        autonomous_research_should_run_main_agent(resolved, &job, &tick_summary);
    if should_run_main_agent {
        let dispatchable_task_ids_before_round =
            autonomous_research_dispatchable_main_agent_stage_task_ids(resolved, &job);
        job.phase = "main_agent_round".to_string();
        job.updated_at = timestamp_string();
        write_autonomous_research_job_state(resolved, &job).map_err(|err| {
            internal_failure("research jobs tick", err).with_output_json(output_json)
        })?;
        append_autonomous_research_job_event(
            resolved,
            &job,
            "main_agent_round_started",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "tick_index": tick_index,
            "reason": autonomous_research_main_agent_reason(resolved, &job, &tick_summary),
            "stage_contract": autonomous_research_stage_contract_event(&autonomous_research_stage_contract(resolved, &job))
        }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
        match run_autonomous_research_main_agent_round(
            resolved,
            &mut job,
            resume_deadline_ms,
            output_json,
        ) {
            Ok(Some(round)) => {
                let active_provider_fault_before_round = job.active_provider_fault.clone();
                if !job
                    .artifact_refs
                    .iter()
                    .any(|artifact| artifact == &round.summary.artifact_path)
                {
                    job.artifact_refs.push(round.summary.artifact_path.clone());
                }
                let acknowledged_obligations =
                    mark_autonomous_research_obligations_acknowledged_by_main_agent(
                        &mut job,
                        &round.summary,
                        &round.content,
                    );
                let strategy_decided_obligations =
                    mark_autonomous_research_failed_review_obligations_strategy_decided_by_main_agent(
                        &mut job,
                        &round.summary,
                        &round.content,
                    );
                let structured_strategy_decided_obligations =
                    mark_autonomous_research_obligations_strategy_decided_from_structured_records(
                        resolved,
                        &mut job,
                        &round.summary,
                    );
                let converted_obligations =
                    mark_autonomous_research_obligations_converted_to_board_tasks_by_main_agent(
                        resolved,
                        &mut job,
                        &round.summary,
                        &round.content,
                    );
                let repaired_obligations =
                    mark_autonomous_research_failed_review_obligations_repaired_by_main_agent_synthesis(
                        resolved,
                        &mut job,
                        &round.summary,
                    );
                let stalled_recovered_obligations =
                    mark_autonomous_research_main_agent_stalled_obligations_recovered_by_main_agent_action(
                        resolved,
                        &mut job,
                        &round.summary,
                        &round.content,
                    );
                let provider_fault_obligations =
                    mark_autonomous_research_provider_fault_obligations_satisfied_by_main_agent_round(
                        &mut job,
                        &round.summary,
                        &round.content,
                    );
                let provider_fault_route_recovery_obligations = if let Some(fault) =
                    active_provider_fault_before_round.as_ref()
                {
                    mark_autonomous_research_provider_fault_obligations_satisfied_by_provider_route(
                        &mut job,
                        fault,
                        &round.summary,
                    )
                } else {
                    Vec::new()
                };
                if active_provider_fault_before_round.is_some() {
                    job.active_provider_fault = None;
                    job.stop_reason = None;
                }
                if !acknowledged_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "obligations_acknowledged_by_main_agent",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": acknowledged_obligations,
                            "main_agent_round": round.summary.artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if !strategy_decided_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "obligations_strategy_decided_by_main_agent",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": strategy_decided_obligations,
                            "main_agent_round": round.summary.artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if !structured_strategy_decided_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "obligations_strategy_decided_by_structured_main_agent_tool",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": structured_strategy_decided_obligations,
                            "main_agent_round": round.summary.artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if !converted_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "obligations_converted_to_board_tasks_by_main_agent",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": converted_obligations,
                            "main_agent_round": round.summary.artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if !repaired_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "obligations_repaired_by_main_agent_synthesis",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": repaired_obligations,
                            "main_agent_round": round.summary.artifact_path,
                            "stage_artifact_path": autonomous_research_stage_contract(resolved, &job).artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if !stalled_recovered_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "main_agent_stalled_obligations_recovered_by_main_agent_action",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": stalled_recovered_obligations,
                            "main_agent_round": round.summary.artifact_path,
                            "stage_artifact_path": autonomous_research_stage_contract(resolved, &job).artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if !provider_fault_obligations.is_empty() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "provider_fault_obligations_satisfied_by_main_agent_round",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "obligation_ids": provider_fault_obligations,
                            "main_agent_round": round.summary.artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                if active_provider_fault_before_round.is_some() {
                    append_autonomous_research_job_event(
                        resolved,
                        &job,
                        "provider_fault_recovered_by_main_agent_round",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "fault": active_provider_fault_before_round,
                            "obligation_ids": provider_fault_route_recovery_obligations,
                            "main_agent_round": round.summary.artifact_path
                        }),
                    )
                    .map_err(|err| {
                        internal_failure("research jobs tick", err).with_output_json(output_json)
                    })?;
                }
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "main_agent_round_completed",
                    serde_json::to_value(&round.summary)
                        .expect("main agent round summary should serialize"),
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
                job.provider_rounds.push(round.summary);
                job.last_error = None;
                job.updated_at = timestamp_string();
                write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
                if let Some(applied) = apply_autonomous_research_main_agent_route_change_if_ready(
                    resolved,
                    &mut job,
                    tick_index,
                    output_json,
                )? {
                    return Ok(AutonomousResearchJobTickResult {
                        schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                        status: "ticked".to_string(),
                        project_id: resolved.project_id.clone(),
                        job_id: job.job_id.clone(),
                        tick_index,
                        job_status: job.status.clone(),
                        phase: job.phase.clone(),
                        should_continue: true,
                        review_passed: applied.previous_review_passed,
                        tick_summary: Some(tick_summary),
                        review: applied.previous_review,
                        job,
                    });
                }
                let main_agent_round_ref = job
                    .provider_rounds
                    .last()
                    .map(|round| round.artifact_path.clone());
                if let Some((wait_reason, dispatchable_stage_task_types)) =
                    autonomous_research_main_agent_published_worker_task_wait_reason(
                        resolved,
                        &job,
                        &dispatchable_task_ids_before_round,
                    )
                {
                    persist_autonomous_research_job_phase(
                        resolved,
                        &mut job,
                        "waiting_for_agent_team_after_main_agent_task_publication",
                        "waiting_for_agent_team_after_main_agent_task_publication",
                        json!({
                            "schema_version": "autonomous_research_job_event.v1",
                            "tick_index": tick_index,
                            "main_agent_round": main_agent_round_ref,
                            "reason": wait_reason,
                            "dispatchable_stage_task_types": dispatchable_stage_task_types
                        }),
                        output_json,
                    )?;
                    return Ok(AutonomousResearchJobTickResult {
                        schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                        status: "ticked".to_string(),
                        project_id: resolved.project_id.clone(),
                        job_id: job.job_id.clone(),
                        tick_index,
                        job_status: job.status.clone(),
                        phase: job.phase.clone(),
                        should_continue: !autonomous_research_job_terminal(&job.status),
                        review_passed: false,
                        tick_summary: Some(tick_summary),
                        review: None,
                        job,
                    });
                }
            }
            Ok(None) => {
                job.warnings.push(
                    "No configured provider route; job remains running and waits for provider configuration or external artifacts."
                        .to_string(),
                );
                job.updated_at = timestamp_string();
                write_autonomous_research_job_state(resolved, &job).map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
            }
            Err(AutonomousResearchMainAgentRoundError::ProviderFault(fault)) => {
                return schedule_autonomous_research_provider_backoff_tick(
                    resolved,
                    &mut job,
                    tick_index,
                    Some(tick_summary),
                    fault,
                    "main_agent_round",
                    "provider_transient_failure_backoff",
                    output_json,
                );
            }
            Err(AutonomousResearchMainAgentRoundError::NonProvider(err)) => {
                if err == "runtime_budget_exhausted_before_main_agent_round" {
                    let max_runtime_ms = job.max_runtime_ms as u128;
                    pause_autonomous_research_job_for_runtime_budget(
                        resolved,
                        &mut job,
                        max_runtime_ms,
                        None,
                        "runtime_budget_exhausted_before_main_agent_round",
                        output_json,
                    )?;
                    return Ok(AutonomousResearchJobTickResult {
                        schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                        status: "ticked".to_string(),
                        project_id: resolved.project_id.clone(),
                        job_id: job.job_id.clone(),
                        tick_index,
                        job_status: job.status.clone(),
                        phase: job.phase.clone(),
                        should_continue: false,
                        review_passed: false,
                        tick_summary: Some(tick_summary),
                        review: None,
                        job,
                    });
                }
                job.last_error = Some(err);
                job.phase = "main_agent_round_error".to_string();
                job.updated_at = timestamp_string();
                write_autonomous_research_job_state(resolved, &job).map_err(|write_err| {
                    internal_failure("research jobs tick", write_err).with_output_json(output_json)
                })?;
            }
        }
    } else {
        append_autonomous_research_job_event(
            resolved,
            &job,
            "main_agent_round_skipped",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "reason": autonomous_research_main_agent_reason(resolved, &job, &tick_summary)
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    }

    let mut review_blocking_obligations = autonomous_research_review_blocking_obligations(&job);
    if !review_blocking_obligations.is_empty() {
        if autonomous_research_stalled_ticks(&job)
            >= AUTONOMOUS_RESEARCH_STALLED_AGENT_TRIGGER_TICKS
        {
            record_autonomous_research_main_agent_stalled_obligation(
                &mut job,
                "Open blocking obligations still have not been handled by a main-agent round; block repeated review until the main agent dispatches work, changes strategy, or requests a gate.",
            );
            review_blocking_obligations = autonomous_research_review_blocking_obligations(&job);
        }
        job.status = "running".to_string();
        job.stop_reason = Some("active_blocking_obligations_require_main_agent_action".to_string());
        persist_autonomous_research_job_phase(
            resolved,
            &mut job,
            "blocked_by_open_obligations",
            "review_gate_blocked_by_open_obligations",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "obligations": review_blocking_obligations
            }),
            output_json,
        )?;
        return Ok(AutonomousResearchJobTickResult {
            schema_version: "autonomous_research_job_tick_result.v1".to_string(),
            status: "ticked".to_string(),
            project_id: resolved.project_id.clone(),
            job_id: job.job_id.clone(),
            tick_index,
            job_status: job.status.clone(),
            phase: job.phase.clone(),
            should_continue: true,
            review_passed: false,
            tick_summary: Some(tick_summary),
            review: None,
            job,
        });
    }

    let review_support_contract = autonomous_research_stage_contract(resolved, &job);
    ensure_autonomous_research_review_support_docframes(
        resolved,
        &mut job,
        &review_support_contract,
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;

    let review_readiness_blockers = autonomous_research_review_readiness_blockers(resolved, &job);
    if !review_readiness_blockers.is_empty() {
        let stage_contract_event = autonomous_research_stage_contract_event(
            &autonomous_research_stage_contract(resolved, &job),
        );
        job.status = "running".to_string();
        job.stop_reason = Some("review_readiness_blocked_requires_main_agent_action".to_string());
        persist_autonomous_research_job_phase(
            resolved,
            &mut job,
            "review_readiness_blocked",
            "review_gate_blocked_by_readiness",
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "tick_index": tick_index,
                "blockers": review_readiness_blockers,
                "stage_contract": stage_contract_event
            }),
            output_json,
        )?;
        return Ok(AutonomousResearchJobTickResult {
            schema_version: "autonomous_research_job_tick_result.v1".to_string(),
            status: "ticked".to_string(),
            project_id: resolved.project_id.clone(),
            job_id: job.job_id.clone(),
            tick_index,
            job_status: job.status.clone(),
            phase: job.phase.clone(),
            should_continue: true,
            review_passed: false,
            tick_summary: Some(tick_summary),
            review: None,
            job,
        });
    }

    job.phase = "review_gate".to_string();
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, &job)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(
        resolved,
        &job,
        "review_gate_started",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "tick_index": tick_index,
            "stage_contract": autonomous_research_stage_contract_event(&autonomous_research_stage_contract(resolved, &job))
        }),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    let review = match run_autonomous_research_review_gate(resolved, &mut job, output_json)? {
        AutonomousResearchReviewGateOutcome::Review(review) => review,
        AutonomousResearchReviewGateOutcome::ProviderFault(fault) => {
            return schedule_autonomous_research_provider_backoff_tick(
                resolved,
                &mut job,
                tick_index,
                Some(tick_summary),
                fault,
                "review_gate",
                "review_provider_failure_backoff",
                output_json,
            );
        }
    };
    let review_passed = review.verdict == "pass";
    job.last_review = Some(review.clone());
    job.review_rounds_completed = job.review_rounds_completed.saturating_add(1);
    append_autonomous_research_job_event(
        resolved,
        &job,
        "review_gate_completed",
        serde_json::to_value(&review).expect("review state should serialize"),
    )
    .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    let obligation_contract = autonomous_research_stage_contract(resolved, &job);
    let review_obligation_changes =
        record_autonomous_research_obligations_from_review(&mut job, &obligation_contract, &review);
    if !review_obligation_changes.is_empty() {
        append_autonomous_research_job_event(
            resolved,
            &job,
            if review_passed {
                "obligations_satisfied_by_review"
            } else {
                "obligations_created_from_review_failure"
            },
            json!({
                "schema_version": "autonomous_research_job_event.v1",
                "review_id": review.review_id,
                "stage_id": obligation_contract.stage_id,
                "obligation_ids": review_obligation_changes,
                "open_blocking_obligation_count": autonomous_research_open_blocking_obligations(&job).len()
            }),
        )
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    }

    if review_passed {
        let contract = autonomous_research_stage_contract(resolved, &job);
        if autonomous_research_unresolved_blocking_obligation_count(&job) > 0 {
            job.status = "running".to_string();
            job.phase = "review_passed_waiting_for_obligation_closure".to_string();
            job.stop_reason = Some("review_passed_but_unresolved_obligations_remain".to_string());
        } else {
            let promoted_active_stage_evidence =
                promote_integration_verified_canonical_artifacts_after_passed_stage_gate(
                    resolved, &job, &contract, &review,
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
            if !promoted_active_stage_evidence.is_empty() {
                merge_unique_strings(
                    &mut job.artifact_refs,
                    promoted_active_stage_evidence.clone(),
                );
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "canonical_artifacts_promoted_to_active_stage_evidence",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "review_id": review.review_id,
                        "stage_id": contract.stage_id,
                        "stage_execution_id": job.stage_execution_id.clone(),
                        "promoted_refs": promoted_active_stage_evidence,
                        "runtime_boundary": "review pass and closed obligations promote already integration-verified canonical artifacts to active stage evidence; runtime does not advance the stage"
                    }),
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
            }
            if contract.final_completion_stage {
                if autonomous_research_final_completion_ready(resolved, &job, &contract) {
                    job.status = "completed".to_string();
                    job.phase = "review_passed".to_string();
                    job.stop_reason = Some("hard_review_passed".to_string());
                } else if !autonomous_research_final_cleanup_ready(resolved, &job) {
                    job.phase = "cleanup".to_string();
                    run_autonomous_research_cleanup(resolved, &mut job, output_json)?;
                    job.status = "running".to_string();
                    job.phase = "final_review_passed_cleanup_created".to_string();
                    job.stop_reason =
                        Some("final_finish_requires_cleanup_summary_then_resume".to_string());
                } else {
                    job.status = "running".to_string();
                    job.phase = "final_review_passed_waiting_for_finish_gate".to_string();
                    job.stop_reason = Some(
                        "final_finish_requires_completion_contract_and_policy_gate".to_string(),
                    );
                }
            } else {
                if let Some(applied) = apply_autonomous_research_main_agent_route_change_if_ready(
                    resolved,
                    &mut job,
                    tick_index,
                    output_json,
                )? {
                    return Ok(AutonomousResearchJobTickResult {
                        schema_version: "autonomous_research_job_tick_result.v1".to_string(),
                        status: "ticked".to_string(),
                        project_id: resolved.project_id.clone(),
                        job_id: job.job_id.clone(),
                        tick_index,
                        job_status: job.status.clone(),
                        phase: job.phase.clone(),
                        should_continue: true,
                        review_passed: applied.previous_review_passed,
                        tick_summary: Some(tick_summary),
                        review: applied.previous_review,
                        job,
                    });
                }
                job.status = "running".to_string();
                job.phase =
                    "stage_review_passed_waiting_for_main_agent_advance_decision".to_string();
                job.stop_reason =
                    Some("stage_review_passed_requires_main_agent_advance_decision".to_string());
                append_autonomous_research_job_event(
                    resolved,
                    &job,
                    "stage_review_passed_waiting_for_main_agent_decision",
                    json!({
                        "schema_version": "autonomous_research_job_event.v1",
                        "review_id": review.review_id,
                        "stage_id": contract.stage_id,
                        "artifact_path": contract.artifact_path,
                        "runtime_boundary": "review pass is evidence only; main agent must explicitly decide whether to advance the stage"
                    }),
                )
                .map_err(|err| {
                    internal_failure("research jobs tick", err).with_output_json(output_json)
                })?;
            }
        }
    } else if job.review_rounds_completed >= job.max_review_rounds {
        job.status = "running".to_string();
        job.phase = "review_budget_pause".to_string();
        job.stop_reason = Some("review_round_budget_elapsed_resume_later".to_string());
    } else if autonomous_research_can_auto_repair_stage(&job) {
        record_autonomous_research_failed_review_pattern(&mut job, resolved, &review);
        record_autonomous_research_repair_obligation_waiting_for_main_agent_decision(
            resolved,
            &mut job,
            &review,
            output_json,
        )?;
        job.status = "running".to_string();
        job.phase = "review_failed_waiting_for_main_agent_decision".to_string();
        job.stop_reason = Some("review_failure_requires_main_agent_route_decision".to_string());
        if job
            .last_error
            .as_deref()
            .map(autonomous_research_error_needs_provider_backoff)
            .unwrap_or(false)
        {
            append_autonomous_research_job_event(
                resolved,
                &job,
                "provider_backoff_scheduled",
                json!({
                    "schema_version": "autonomous_research_job_event.v1",
                    "sleep_ms": autonomous_research_next_sleep_ms(resolved, &job),
                    "reason": "provider_rate_limit"
                }),
            )
            .map_err(|err| {
                internal_failure("research jobs tick", err).with_output_json(output_json)
            })?;
        }
    } else {
        job.status = "running".to_string();
        job.phase = "stage_review_failed_waiting_for_repair_gate".to_string();
        job.stop_reason = Some("stage_review_failed_repair_requires_approval".to_string());
    }

    job.updated_at = timestamp_string();
    merge_unique_strings(
        &mut job.artifact_refs,
        collect_autonomous_research_artifact_refs(resolved),
    );
    write_autonomous_research_job_state(resolved, &job)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    append_canonical_event(
        resolved,
        "autonomous_research_job",
        "terminal",
        Some("succeeded"),
        "autonomous_research_job",
        &job.job_id,
        None,
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "job_id": job.job_id,
            "status": job.status,
            "phase": job.phase,
            "stage_contract": autonomous_research_stage_contract_event(&autonomous_research_stage_contract(resolved, &job)),
            "ticks_completed": job.ticks_completed,
            "review_verdict": review.verdict,
            "review_rounds_completed": job.review_rounds_completed
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

    Ok(AutonomousResearchJobTickResult {
        schema_version: "autonomous_research_job_tick_result.v1".to_string(),
        status: "ticked".to_string(),
        project_id: resolved.project_id.clone(),
        job_id: job.job_id.clone(),
        tick_index,
        job_status: job.status.clone(),
        phase: job.phase.clone(),
        should_continue: !autonomous_research_job_terminal(&job.status),
        review_passed,
        tick_summary: Some(tick_summary),
        review: Some(review),
        job,
    })
}

pub(crate) fn persist_autonomous_research_job_phase(
    resolved: &ResolvedProject,
    job: &mut AutonomousResearchJobState,
    phase: &str,
    event: &str,
    payload: Value,
    output_json: bool,
) -> Result<(), CommandFailureOutcome> {
    job.phase = phase.to_string();
    job.updated_at = timestamp_string();
    write_autonomous_research_job_state(resolved, job)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))?;
    append_autonomous_research_job_event(resolved, job, event, payload)
        .map_err(|err| internal_failure("research jobs tick", err).with_output_json(output_json))
}
