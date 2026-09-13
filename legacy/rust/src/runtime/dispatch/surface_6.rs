impl Runtime {
    fn handle_repo(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("cleanup-plan") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "repo")
                    .map_err(|err| project_resolution_failure("repo cleanup-plan", err))?;
                let (extra_gates, extra_actions) = if has_flag(args, "--canonical") {
                    let report = canonicality::audit_canonical(
                        &resolved.workspace_root,
                        &resolved.data_dir,
                        &resolved.project_id,
                    )
                    .map_err(|err| internal_failure("repo cleanup-plan", err.to_string()))?;
                    let unresolved_count =
                        artifacts::canonicality_unresolved_cleanup_violation_count(&report);
                    let actions = artifacts::cleanup_actions_for_canonicality_report(
                        &resolved.data_dir,
                        &resolved.workspace_root,
                        &report,
                    )
                    .map_err(|err| internal_failure("repo cleanup-plan", err.to_string()))?;
                    (
                        vec![canonicality_cleanup_gate(unresolved_count, actions.len())],
                        actions,
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
                let proposal = artifacts::cleanup_plan_with_gates_and_actions(
                    &resolved.data_dir,
                    extra_gates,
                    extra_actions,
                )
                .map_err(|err| internal_failure("repo cleanup-plan", err.to_string()))?;
                emit_cleanup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "cleanup_plan",
                    "repo cleanup-plan",
                    serde_json::to_value(&proposal).expect("cleanup plan should serialize"),
                )
                .map_err(|err| internal_failure("repo cleanup-plan", err))?;
                Ok(CommandSuccess::with_project(
                    "repo cleanup-plan",
                    resolved.project_id.clone(),
                    serde_json::to_value(proposal).expect("cleanup plan should serialize"),
                ))
            }
            Some("cleanup-apply") => {
                let plan_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "repo cleanup-apply".to_string(),
                        "usage_invalid",
                        "missing cleanup plan id".to_string(),
                        Some("Pass a plan id after `repo cleanup-apply`.".to_string()),
                    )
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "repo")
                    .map_err(|err| project_resolution_failure("repo cleanup-apply", err))?;
                let proposal = artifacts::cleanup_apply(&resolved.data_dir, plan_id).map_err(
                    |err| match err {
                        artifacts::ArtifactError::UnknownPlan(_)
                        | artifacts::ArtifactError::StalePlan(_)
                        | artifacts::ArtifactError::GateUnsatisfied(_) => {
                            CommandFailureOutcome::new(
                                9,
                                "repo cleanup-apply".to_string(),
                                Some(resolved.project_id.clone()),
                                None,
                                "cleanup_plan_invalid",
                                err.to_string(),
                                Some("Regenerate the cleanup plan and retry apply.".to_string()),
                                false,
                                None,
                                parsed.output_json,
                            )
                        }
                        artifacts::ArtifactError::ScopeViolation(_) => CommandFailureOutcome::new(
                            6,
                            "repo cleanup-apply".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "workspace_scope_violation",
                            err.to_string(),
                            Some(
                                "Cleanup apply only removes archive files or moves canonical superseded public files into the project archive."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("repo cleanup-apply", other.to_string()),
                    },
                )?;
                let touched_paths = proposal
                    .applied_actions
                    .iter()
                    .flat_map(|action| {
                        [action.target_path.clone(), action.destination_path.clone()]
                            .into_iter()
                            .filter(|path| !path.trim().is_empty())
                    })
                    .collect::<Vec<_>>();
                memory::invalidate_records_referencing_paths(
                    &resolved.data_dir,
                    &touched_paths,
                    "repo cleanup apply invalidated memory with moved or removed artifact refs",
                )
                .map_err(|err| internal_failure("repo cleanup-apply", err.to_string()))?;
                emit_cleanup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "cleanup_apply",
                    "repo cleanup-apply",
                    serde_json::to_value(&proposal).expect("cleanup apply should serialize"),
                )
                .map_err(|err| internal_failure("repo cleanup-apply", err))?;
                Ok(CommandSuccess::with_project(
                    "repo cleanup-apply",
                    resolved.project_id.clone(),
                    serde_json::to_value(proposal).expect("cleanup apply should serialize"),
                ))
            }
            Some("cleanup-restore") => {
                let plan_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "repo cleanup-restore".to_string(),
                        "usage_invalid",
                        "missing cleanup plan id".to_string(),
                        Some("Pass a plan id after `repo cleanup-restore`.".to_string()),
                    )
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "repo")
                    .map_err(|err| project_resolution_failure("repo cleanup-restore", err))?;
                let result = artifacts::cleanup_restore(&resolved.data_dir, plan_id).map_err(
                    |err| match err {
                        artifacts::ArtifactError::UnknownPlan(_)
                        | artifacts::ArtifactError::UnknownTarget(_)
                        | artifacts::ArtifactError::GateUnsatisfied(_) => {
                            CommandFailureOutcome::new(
                                9,
                                "repo cleanup-restore".to_string(),
                                Some(resolved.project_id.clone()),
                                None,
                                "cleanup_restore_invalid",
                                err.to_string(),
                                Some(
                                    "Regenerate the cleanup plan or inspect the rollback snapshot."
                                        .to_string(),
                                ),
                                false,
                                None,
                                parsed.output_json,
                            )
                        }
                        artifacts::ArtifactError::ScopeViolation(_) => CommandFailureOutcome::new(
                            6,
                            "repo cleanup-restore".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "workspace_scope_violation",
                            err.to_string(),
                            Some(
                                "Cleanup restore only writes archive rollback files or restores canonical superseded public files from the project archive."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("repo cleanup-restore", other.to_string()),
                    },
                )?;
                let restored_paths = result
                    .restored_actions
                    .iter()
                    .map(|action| action.target_path.clone())
                    .collect::<Vec<_>>();
                memory::refresh_invalidated_records_referencing_paths(
                    &resolved.data_dir,
                    &restored_paths,
                    "repo cleanup restore invalidated memory with restored artifact refs",
                )
                .map_err(|err| internal_failure("repo cleanup-restore", err.to_string()))?;
                emit_cleanup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "cleanup_restore",
                    "repo cleanup-restore",
                    serde_json::to_value(&result).expect("cleanup restore should serialize"),
                )
                .map_err(|err| internal_failure("repo cleanup-restore", err))?;
                Ok(CommandSuccess::with_project(
                    "repo cleanup-restore",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("cleanup restore should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("repo {other}"),
                "usage_invalid",
                format!("unknown repo subcommand: {other}"),
                Some("Choose one of: cleanup-plan, cleanup-apply, cleanup-restore.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "repo".to_string(),
                "usage_invalid",
                "missing repo subcommand".to_string(),
                Some("Choose one of: cleanup-plan, cleanup-apply, cleanup-restore.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_projectops(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("status") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projectops status")
                    .map_err(|err| project_resolution_failure("projectops status", err))?;
                let status = crate::projectops::status(&resolved.data_dir, &resolved.project_id)
                    .map_err(|err| internal_failure("projectops status", err.to_string()))?;
                Ok(CommandSuccess::with_project(
                    "projectops status",
                    resolved.project_id,
                    serde_json::to_value(status).expect("projectops status should serialize"),
                ))
            }
            Some("lease") if args.get(1).map(String::as_str) == Some("acquire") => {
                let run_id = parse_flag_value(args, "--run-id").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops lease acquire".to_string(),
                        "usage_invalid",
                        "missing --run-id".to_string(),
                        Some("Use `projectops lease acquire --run-id <id> --owner-agent <id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let owner_agent_id = parse_flag_value(args, "--owner-agent").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops lease acquire".to_string(),
                        "usage_invalid",
                        "missing --owner-agent".to_string(),
                        Some("Use `projectops lease acquire --run-id <id> --owner-agent <id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let stale_after_sec = parse_flag_value(args, "--stale-after-sec")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(900);
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projectops lease acquire")
                    .map_err(|err| project_resolution_failure("projectops lease acquire", err))?;
                let lease = crate::projectops::acquire_supervisor_lease(
                    &resolved.data_dir,
                    &run_id,
                    &owner_agent_id,
                    stale_after_sec,
                )
                .map_err(|err| internal_failure("projectops lease acquire", err.to_string()))?;
                append_canonical_event(
                    &resolved,
                    "projectops_lease",
                    "terminal",
                    Some("succeeded"),
                    "projectops_lease",
                    &lease.lease_id,
                    None,
                    serde_json::to_value(&lease).expect("lease should serialize"),
                )
                .map_err(|err| internal_failure("projectops lease acquire", err))?;
                Ok(CommandSuccess::with_project(
                    "projectops lease acquire",
                    resolved.project_id,
                    serde_json::to_value(lease).expect("lease should serialize"),
                ))
            }
            Some("wake") if args.get(1).map(String::as_str) == Some("emit") => {
                let run_id = parse_flag_value(args, "--run-id").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops wake emit".to_string(),
                        "usage_invalid",
                        "missing --run-id".to_string(),
                        Some("Use `projectops wake emit --run-id <id> --lease-id <id> --owner-agent <id> --kind <kind> --urgency <urgency> --summary <text> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let lease_id = parse_flag_value(args, "--lease-id").unwrap_or_default();
                let no_owner_reason = parse_flag_value(args, "--no-owner-reason").unwrap_or_default();
                let owner_agent_id = parse_flag_value(args, "--owner-agent").or_else(|| {
                    if lease_id.is_empty() && !no_owner_reason.is_empty() {
                        Some("unassigned".to_string())
                    } else {
                        None
                    }
                }).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops wake emit".to_string(),
                        "usage_invalid",
                        "missing --owner-agent".to_string(),
                        Some("Use `projectops wake emit --run-id <id> --lease-id <id> --owner-agent <id> --kind <kind> --summary <text> --json`, or provide --no-owner-reason for an unowned wake.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let kind =
                    parse_flag_value(args, "--kind").unwrap_or_else(|| "blocked".to_string());
                let urgency =
                    parse_flag_value(args, "--urgency").unwrap_or_else(|| "normal".to_string());
                let summary = parse_flag_value(args, "--summary")
                    .unwrap_or_else(|| "supervised run requires operator attention".to_string());
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projectops wake emit")
                    .map_err(|err| project_resolution_failure("projectops wake emit", err))?;
                let wake = crate::projectops::emit_wake_event(
                    &resolved.data_dir,
                    &run_id,
                    &lease_id,
                    &owner_agent_id,
                    &kind,
                    &urgency,
                    &summary,
                    has_flag(args, "--requires-main-system"),
                    &no_owner_reason,
                )
                .map_err(|err| internal_failure("projectops wake emit", err.to_string()))?;
                append_canonical_event(
                    &resolved,
                    "projectops_wake",
                    "terminal",
                    Some("succeeded"),
                    "projectops_wake",
                    &wake.wake_id,
                    None,
                    serde_json::to_value(&wake).expect("wake should serialize"),
                )
                .map_err(|err| internal_failure("projectops wake emit", err))?;
                Ok(CommandSuccess::with_project(
                    "projectops wake emit",
                    resolved.project_id,
                    serde_json::to_value(wake).expect("wake should serialize"),
                ))
            }
            Some("wake") if args.get(1).map(String::as_str) == Some("ack") => {
                let wake_id = parse_flag_value(args, "--wake-id").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops wake ack".to_string(),
                        "usage_invalid",
                        "missing --wake-id".to_string(),
                        Some("Use `projectops wake ack --wake-id <id> --actor <actor> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let actor = parse_flag_value(args, "--actor").unwrap_or_else(|| "operator".to_string());
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projectops wake ack")
                    .map_err(|err| project_resolution_failure("projectops wake ack", err))?;
                let wake = crate::projectops::acknowledge_wake_event(
                    &resolved.data_dir,
                    &wake_id,
                    &actor,
                )
                .map_err(|err| internal_failure("projectops wake ack", err.to_string()))?;
                append_canonical_event(
                    &resolved,
                    "projectops_wake",
                    "terminal",
                    Some("succeeded"),
                    "projectops_wake",
                    &wake.wake_id,
                    None,
                    serde_json::to_value(&wake).expect("wake should serialize"),
                )
                .map_err(|err| internal_failure("projectops wake ack", err))?;
                Ok(CommandSuccess::with_project(
                    "projectops wake ack",
                    resolved.project_id,
                    serde_json::to_value(wake).expect("wake should serialize"),
                ))
            }
            Some("wake") if args.get(1).map(String::as_str) == Some("resolve") => {
                let wake_id = parse_flag_value(args, "--wake-id").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops wake resolve".to_string(),
                        "usage_invalid",
                        "missing --wake-id".to_string(),
                        Some("Use `projectops wake resolve --wake-id <id> --resolution <text> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolution = parse_flag_value(args, "--resolution")
                    .unwrap_or_else(|| "wake resolved by operator".to_string());
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projectops wake resolve")
                    .map_err(|err| project_resolution_failure("projectops wake resolve", err))?;
                let wake = crate::projectops::resolve_wake_event(
                    &resolved.data_dir,
                    &wake_id,
                    &resolution,
                )
                .map_err(|err| internal_failure("projectops wake resolve", err.to_string()))?;
                append_canonical_event(
                    &resolved,
                    "projectops_wake",
                    "terminal",
                    Some("succeeded"),
                    "projectops_wake",
                    &wake.wake_id,
                    None,
                    serde_json::to_value(&wake).expect("wake should serialize"),
                )
                .map_err(|err| internal_failure("projectops wake resolve", err))?;
                Ok(CommandSuccess::with_project(
                    "projectops wake resolve",
                    resolved.project_id,
                    serde_json::to_value(wake).expect("wake should serialize"),
                ))
            }
            Some("wake") if args.get(1).map(String::as_str) == Some("escalate") => {
                let wake_id = parse_flag_value(args, "--wake-id").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "projectops wake escalate".to_string(),
                        "usage_invalid",
                        "missing --wake-id".to_string(),
                        Some("Use `projectops wake escalate --wake-id <id> --reason <text> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let reason = parse_flag_value(args, "--reason")
                    .unwrap_or_else(|| "wake escalated for operator review".to_string());
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projectops wake escalate")
                    .map_err(|err| project_resolution_failure("projectops wake escalate", err))?;
                let wake = crate::projectops::escalate_wake_event(
                    &resolved.data_dir,
                    &wake_id,
                    &reason,
                    has_flag(args, "--requires-main-system"),
                )
                .map_err(|err| internal_failure("projectops wake escalate", err.to_string()))?;
                append_canonical_event(
                    &resolved,
                    "projectops_wake",
                    "terminal",
                    Some("succeeded"),
                    "projectops_wake",
                    &wake.wake_id,
                    None,
                    serde_json::to_value(&wake).expect("wake should serialize"),
                )
                .map_err(|err| internal_failure("projectops wake escalate", err))?;
                Ok(CommandSuccess::with_project(
                    "projectops wake escalate",
                    resolved.project_id,
                    serde_json::to_value(wake).expect("wake should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("projectops {other}"),
                "usage_invalid",
                format!("unknown projectops subcommand: {other}"),
                Some("Choose one of: status, lease acquire, wake emit, wake ack, wake resolve, wake escalate.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "projectops".to_string(),
                "usage_invalid",
                "missing projectops subcommand".to_string(),
                Some("Choose one of: status, lease acquire, wake emit, wake ack, wake resolve, wake escalate.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_routines(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("create") => {
                let request =
                    parse_routine_create_request(args).map_err(|message| {
                        CommandFailureOutcome::usage(
                            "routines create".to_string(),
                            "usage_invalid",
                            message,
                            Some("Use `routines create --name <name> --trigger-kind <manual|schedule|webhook|api> --intent <text> --role-profile <profile> --message <text> --command <shell> --json`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines create")
                    .map_err(|err| project_resolution_failure("routines create", err))?;
                let result = routines::create(&resolved.data_dir, request).map_err(|err| {
                    routine_failure("routines create", err, &resolved, parsed.output_json)
                })?;
                append_canonical_event(
                    &resolved,
                    "routine_definition",
                    "terminal",
                    Some("succeeded"),
                    "routine_definition",
                    &result.routine.routine_id,
                    None,
                    serde_json::to_value(&result).expect("routine create result should serialize"),
                )
                .map_err(|err| internal_failure("routines create", err))?;
                Ok(CommandSuccess::with_project(
                    "routines create",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine create result should serialize"),
                ))
            }
            Some("ingress") => {
                let request = parse_routine_ingress_request(args).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "routines ingress".to_string(),
                        "usage_invalid",
                        message,
                        Some("Use `routines ingress <routine-id> --trigger-kind <manual|schedule|webhook|api> --source <text> [--dedupe-key <text>] [--ready-at <ms>] --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines ingress")
                    .map_err(|err| project_resolution_failure("routines ingress", err))?;
                let result = routines::ingress(&resolved.data_dir, request).map_err(|err| {
                    routine_failure("routines ingress", err, &resolved, parsed.output_json)
                })?;
                append_canonical_event(
                    &resolved,
                    "routine_ingress",
                    "terminal",
                    Some("succeeded"),
                    "routine_ingress",
                    &result.ingress.ingress_id,
                    None,
                    serde_json::to_value(&result).expect("routine ingress result should serialize"),
                )
                .map_err(|err| internal_failure("routines ingress", err))?;
                publish_project_checkpoint(&resolved, &["routines"])
                    .map_err(|err| internal_failure("routines ingress", err))?;
                Ok(CommandSuccess::with_project(
                    "routines ingress",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine ingress result should serialize"),
                ))
            }
            Some("run-due") => {
                let request = parse_routine_run_due_request(args).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "routines run-due".to_string(),
                        "usage_invalid",
                        message,
                        Some("Use `routines run-due [--trigger-kind <manual|schedule|webhook|api>] [--limit <n>] [--stale-after-sec <sec>] --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines run-due")
                    .map_err(|err| project_resolution_failure("routines run-due", err))?;
                let result =
                    routines::run_due(&resolved.data_dir, &resolved.workspace_root, request)
                        .map_err(|err| match err {
                            routines::RoutineError::Agent(agent_err) => agent_start_failure(
                                "routines run-due",
                                agent_err,
                                &resolved,
                                parsed.output_json,
                            ),
                            other => routine_failure(
                                "routines run-due",
                                other,
                                &resolved,
                                parsed.output_json,
                            ),
                        })?;
                append_canonical_event(
                    &resolved,
                    "routine_run_due",
                    "terminal",
                    Some("succeeded"),
                    "routine_run_due",
                    "routine_run_due",
                    None,
                    serde_json::to_value(&result).expect("routine run due result should serialize"),
                )
                .map_err(|err| internal_failure("routines run-due", err))?;
                publish_project_checkpoint(&resolved, &["routines", "agents", "projectops"])
                    .map_err(|err| internal_failure("routines run-due", err))?;
                Ok(CommandSuccess::with_project(
                    "routines run-due",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine run due result should serialize"),
                ))
            }
            Some("retry") => {
                let request = parse_routine_retry_request(args).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "routines retry".to_string(),
                        "usage_invalid",
                        message,
                        Some("Use `routines retry <trigger-id> --source <text> [--stale-after-sec <sec>] --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines retry")
                    .map_err(|err| project_resolution_failure("routines retry", err))?;
                let result =
                    routines::retry_trigger(&resolved.data_dir, &resolved.workspace_root, request)
                        .map_err(|err| match err {
                            routines::RoutineError::Agent(agent_err) => agent_start_failure(
                                "routines retry",
                                agent_err,
                                &resolved,
                                parsed.output_json,
                            ),
                            other => routine_failure(
                                "routines retry",
                                other,
                                &resolved,
                                parsed.output_json,
                            ),
                        })?;
                append_canonical_event(
                    &resolved,
                    "routine_retry",
                    "terminal",
                    Some("succeeded"),
                    "routine_retry",
                    &result.retry.trigger.trigger_id,
                    None,
                    serde_json::to_value(&result).expect("routine retry result should serialize"),
                )
                .map_err(|err| internal_failure("routines retry", err))?;
                publish_project_checkpoint(&resolved, &["routines", "agents", "projectops"])
                    .map_err(|err| internal_failure("routines retry", err))?;
                Ok(CommandSuccess::with_project(
                    "routines retry",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine retry result should serialize"),
                ))
            }
            Some("trigger") => {
                let request =
                    parse_routine_trigger_request(args).map_err(|message| {
                        CommandFailureOutcome::usage(
                            "routines trigger".to_string(),
                            "usage_invalid",
                            message,
                            Some("Use `routines trigger <routine-id> --trigger-kind <manual|schedule|webhook|api> [--source <text>] --json`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines trigger")
                    .map_err(|err| project_resolution_failure("routines trigger", err))?;
                let result =
                    routines::trigger(&resolved.data_dir, &resolved.workspace_root, request)
                        .map_err(|err| match err {
                            routines::RoutineError::Agent(agent_err) => agent_start_failure(
                                "routines trigger",
                                agent_err,
                                &resolved,
                                parsed.output_json,
                            ),
                            other => routine_failure(
                                "routines trigger",
                                other,
                                &resolved,
                                parsed.output_json,
                            ),
                        })?;
                append_canonical_event(
                    &resolved,
                    "routine_trigger",
                    "terminal",
                    Some("succeeded"),
                    "routine_trigger",
                    &result.trigger.trigger_id,
                    None,
                    serde_json::to_value(&result).expect("routine trigger result should serialize"),
                )
                .map_err(|err| internal_failure("routines trigger", err))?;
                publish_project_checkpoint(&resolved, &["routines", "agents", "projectops"])
                    .map_err(|err| internal_failure("routines trigger", err))?;
                Ok(CommandSuccess::with_project(
                    "routines trigger",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine trigger result should serialize"),
                ))
            }
            Some("list") => {
                if has_unrecognized_positionals(&args[1..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "routines list".to_string(),
                        "usage_invalid",
                        "routines list does not accept positional arguments".to_string(),
                        Some("Use `routines list --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines list")
                    .map_err(|err| project_resolution_failure("routines list", err))?;
                let result = routines::list(&resolved.data_dir).map_err(|err| {
                    routine_failure("routines list", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "routines list",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine list result should serialize"),
                ))
            }
            Some("inspect") => {
                let routine_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "routines inspect".to_string(),
                        "usage_invalid",
                        "missing routine id".to_string(),
                        Some("Pass a routine id after `routines inspect`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "routines inspect")
                    .map_err(|err| project_resolution_failure("routines inspect", err))?;
                let result = routines::inspect(&resolved.data_dir, routine_id).map_err(|err| {
                    routine_failure("routines inspect", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "routines inspect",
                    resolved.project_id,
                    serde_json::to_value(result).expect("routine inspect result should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("routines {other}"),
                "usage_invalid",
                format!("unknown routines subcommand: {other}"),
                Some(
                    "Choose one of: create, ingress, run-due, retry, trigger, list, inspect."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "routines".to_string(),
                "usage_invalid",
                "missing routines subcommand".to_string(),
                Some(
                    "Choose one of: create, ingress, run-due, retry, trigger, list, inspect."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

}
