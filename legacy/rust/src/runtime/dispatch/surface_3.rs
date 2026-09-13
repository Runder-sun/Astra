impl Runtime {
    fn handle_providers(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("list") => Ok(CommandSuccess::new_with_data(
                "providers list",
                serde_json::to_value(list_providers())
                    .expect("provider status list should serialize"),
            )),
            Some("auth-status") => Ok(CommandSuccess::new_with_data(
                "providers auth-status",
                serde_json::to_value({
                    let result = provider_auth_status();
                    emit_provider_auth_event_if_possible(registry, cwd, parsed, &result)
                        .map_err(|err| internal_failure("providers auth-status", err))?;
                    result
                })
                .expect("provider auth status result should serialize"),
            )),
            Some("test") => {
                let provider = args.get(1).map(String::as_str);
                let tested = test_provider(provider).map_err(provider_test_failure)?;
                emit_provider_test_event_if_possible(registry, cwd, parsed, &tested)
                    .map_err(|err| internal_failure("providers test", err))?;
                Ok(CommandSuccess::new_with_data(
                    "providers test",
                    serde_json::to_value(tested).expect("provider test result should serialize"),
                ))
            }
            Some("refresh-catalog") => {
                let source = args.get(1).map(String::as_str).unwrap_or("embedded");
                if source == "remote" {
                    return Err(feature_not_graduated_failure(
                        "providers refresh-catalog",
                        "providers.refresh_catalog",
                        "M2.provider_remote_catalog_refresh",
                        vec![
                            "Use `providers refresh-catalog embedded --json` for the frozen baseline.".to_string(),
                            "Land remote catalog transport and replay fixtures before graduating this lane.".to_string(),
                        ],
                    ));
                }
                if source == "degraded-fixture" {
                    return Err(degraded_but_loadable_failure(
                        "providers refresh-catalog",
                        "providers.refresh_catalog",
                        "M2.provider_catalog_degraded_fixture",
                        vec![
                            "Use `providers refresh-catalog embedded --json` for the ready baseline."
                                .to_string(),
                            "Inspect the degraded catalog source before treating it as authoritative."
                                .to_string(),
                        ],
                    )
                    .with_output_json(parsed.output_json));
                }
                let refreshed = if source == "embedded" {
                    refresh_catalog(source)
                } else {
                    refresh_catalog_from_file(source).map_err(|err| {
                        CommandFailureOutcome::new(
                            4,
                            "providers refresh-catalog".to_string(),
                            None,
                            None,
                            "provider_catalog_invalid",
                            format!("failed to load provider catalog: {err}"),
                            Some(
                                "Pass `embedded`, `remote`, or a readable JSON catalog file path."
                                    .to_string(),
                            ),
                            false,
                            Some(json!({ "requested_source": source })),
                            parsed.output_json,
                        )
                    })?
                };
                emit_provider_catalog_event_if_possible(registry, cwd, parsed, &refreshed)
                    .map_err(|err| internal_failure("providers refresh-catalog", err))?;
                Ok(CommandSuccess::new_with_data(
                    "providers refresh-catalog",
                    serde_json::to_value(refreshed)
                        .expect("provider catalog refresh result should serialize"),
                ))
            }
            Some("inspect") => {
                let provider = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "providers inspect".to_string(),
                        "usage_invalid",
                        "missing provider id".to_string(),
                        Some("Pass a provider id such as `openai`.".to_string()),
                    )
                })?;
                let inspected = inspect_provider(provider).map_err(|err| {
                    CommandFailureOutcome::new(
                        6,
                        "providers inspect".to_string(),
                        None,
                        None,
                        "provider_not_found",
                        err.to_string(),
                        Some("Inspect the embedded provider catalog instead of inventing a provider id.".to_string()),
                        false,
                        Some(json!({
                            "provider_id": provider
                        })),
                        false,
                    )
                })?;
                Ok(CommandSuccess::new_with_data(
                    "providers inspect",
                    serde_json::to_value(inspected).expect("provider trace should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("providers {other}"),
                "usage_invalid",
                format!("unknown providers subcommand: {other}"),
                Some(
                    "Choose one of: list, inspect, auth-status, test, refresh-catalog.".to_string(),
                ),
            )),
            None => Err(CommandFailureOutcome::usage(
                "providers".to_string(),
                "usage_invalid",
                "missing providers subcommand".to_string(),
                Some(
                    "Choose one of: list, inspect, auth-status, test, refresh-catalog.".to_string(),
                ),
            )),
        }
    }

    fn handle_config(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, "config")
            .map_err(|err| project_resolution_failure("config", err))?;
        match args.first().map(String::as_str) {
            Some("get") => {
                let key = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "config get".to_string(),
                        "usage_invalid",
                        "missing config key".to_string(),
                        Some("Pass a config key such as `default_model`.".to_string()),
                    )
                })?;
                let result = config_get(&resolved, key)
                    .map_err(|err| config_error_failure("config get", err, Some(key.clone())))?;
                Ok(CommandSuccess::new_with_data(
                    "config get",
                    serde_json::to_value(result).expect("config get result should serialize"),
                ))
            }
            Some("set") => {
                let key = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "config set".to_string(),
                        "usage_invalid",
                        "missing config key".to_string(),
                        Some("Pass a config key such as `default_model`.".to_string()),
                    )
                })?;
                let raw_value = args.get(2).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "config set".to_string(),
                        "usage_invalid",
                        "missing config value".to_string(),
                        Some("Pass a JSON literal or string value after the key.".to_string()),
                    )
                })?;
                let scope = parse_flag_value(args, "--scope").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "config set".to_string(),
                        "usage_invalid",
                        "missing --scope for config set".to_string(),
                        Some("Pass --scope global|project|private.".to_string()),
                    )
                })?;
                let parsed_scope = ConfigScope::parse(&scope).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "config set".to_string(),
                        "usage_invalid",
                        format!("invalid config scope: {scope}"),
                        Some("Choose one of: global, project, private.".to_string()),
                    )
                })?;
                let result = config_set(&resolved, key, raw_value, parsed_scope)
                    .map_err(|err| config_error_failure("config set", err, Some(key.clone())))?;
                emit_config_event(&resolved, key, parsed_scope.as_str(), &result)
                    .map_err(|err| internal_failure("config set", err))?;
                Ok(CommandSuccess::new_with_data(
                    "config set",
                    serde_json::to_value(result).expect("config set result should serialize"),
                ))
            }
            Some("effective") => Ok(CommandSuccess::new_with_data(
                "config effective",
                serde_json::to_value(
                    effective_config(&resolved)
                        .map_err(|err| internal_failure("config effective", err.to_string()))?,
                )
                .expect("effective config should serialize"),
            )),
            Some("sources") => Ok(CommandSuccess::new_with_data(
                "config sources",
                serde_json::to_value(
                    config::config_sources(&resolved)
                        .map_err(|err| internal_failure("config sources", err.to_string()))?,
                )
                .expect("config source report should serialize"),
            )),
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("config {other}"),
                "usage_invalid",
                format!("unknown config subcommand: {other}"),
                Some("Choose one of: get, set, effective, sources.".to_string()),
            )),
            None => Err(CommandFailureOutcome::usage(
                "config".to_string(),
                "usage_invalid",
                "missing config subcommand".to_string(),
                Some("Choose one of: get, set, effective, sources.".to_string()),
            )),
        }
    }

    fn handle_projects(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("init") => {
                let binding = resolve_or_create_workspace_from(command_cwd(cwd, parsed))
                    .map_err(|err| project_resolution_failure("projects init", err.into()))?;
                crate::workspace::resolve::ensure_git_workspace_baseline(&binding.workspace_root)
                    .map_err(|err| project_resolution_failure("projects init", err.into()))?;
                let project_id = stable_project_id(&binding.workspace_root)
                    .map_err(|err| internal_failure("projects init", err.to_string()))?;
                let entry = registry
                    .entry_for_workspace(project_id, binding.workspace_root.clone())
                    .map_err(|err| internal_failure("projects init", err.to_string()))?;
                registry
                    .register(entry.clone())
                    .map_err(|err| internal_failure("projects init", err.to_string()))?;
                registry
                    .set_current_project(entry.to_pointer())
                    .map_err(|err| internal_failure("projects init", err.to_string()))?;
                let resolved = ResolvedProject {
                    project_id: entry.project_id.clone(),
                    workspace_root: entry.workspace_root.clone(),
                    workspace_hash: entry.workspace_hash.clone(),
                    data_dir: entry.data_dir.clone(),
                    resolution_source: "projects_init".to_string(),
                };
                emit_project_event(
                    &resolved,
                    "projects init",
                    json!({
                        "project_id": entry.project_id.clone(),
                        "workspace_root": entry.workspace_root.clone(),
                        "workspace_hash": entry.workspace_hash.clone(),
                        "data_dir": entry.data_dir.clone(),
                        "init_state": entry.init_state.clone()
                    }),
                )
                .map_err(|err| internal_failure("projects init", err))?;
                publish_project_checkpoint(&resolved, &["project"])
                    .map_err(|err| internal_failure("projects init", err))?;

                Ok(CommandSuccess::with_project(
                    "projects init",
                    entry.project_id.clone(),
                    serde_json::to_value(project_status_payload(&resolved, entry, 0))
                        .expect("project status should serialize"),
                ))
            }
            Some("current") => {
                let resolved = resolve_current_project_with_trace(
                    registry,
                    parsed.project.as_deref(),
                    parsed.cwd.as_deref(),
                    cwd,
                )
                .map_err(|err| project_resolution_failure_with_trace("projects current", err))?;
                Ok(CommandSuccess::with_project(
                    "projects current",
                    resolved.resolved.project_id.clone(),
                    serde_json::to_value(resolved.trace)
                        .expect("project resolution trace should serialize"),
                ))
            }
            Some("list") => {
                let entries = registry
                    .list()
                    .map_err(|err| internal_failure("projects list", err.to_string()))?;
                let total_count = entries.len();
                let current_project_id = registry
                    .get_current_project()
                    .map_err(|err| internal_failure("projects list", err.to_string()))?
                    .map(|pointer| pointer.project_id);
                Ok(CommandSuccess::new_with_data(
                    "projects list",
                    serde_json::to_value(ProjectRegistryListPayload {
                        projects: entries,
                        current_project_id,
                        total_count,
                    })
                    .expect("project registry list should serialize"),
                ))
            }
            Some("status") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projects status")
                    .map_err(|err| project_resolution_failure("projects status", err))?;
                let session_store = SessionStore::new(resolved.data_dir.clone());
                let sessions = session_store.list_sessions().map_err(|err| {
                    session_store_failure("projects status", err, &resolved, None)
                })?;
                let registry_entry = registry
                    .get_by_project_id(&resolved.project_id)
                    .map_err(|err| internal_failure("projects status", err.to_string()))?
                    .ok_or_else(|| {
                        internal_failure(
                            "projects status",
                            format!("missing registry entry for {}", resolved.project_id),
                        )
                    })?;
                Ok(CommandSuccess::with_project(
                    "projects status",
                    resolved.project_id.clone(),
                    serde_json::to_value(project_status_payload(
                        &resolved,
                        registry_entry,
                        sessions.len(),
                    ))
                    .expect("project status should serialize"),
                ))
            }
            Some("audit") => {
                if !has_flag(args, "--canonical") {
                    return Err(CommandFailureOutcome::usage(
                        "projects audit".to_string(),
                        "usage_invalid",
                        "missing --canonical for projects audit".to_string(),
                        Some("Run `research-cli projects audit --canonical --json`.".to_string()),
                    ));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "projects audit")
                    .map_err(|err| project_resolution_failure("projects audit", err))?;
                let report = canonicality::audit_canonical_with_mode(
                    &resolved.workspace_root,
                    &resolved.data_dir,
                    &resolved.project_id,
                    has_flag(args, "--release"),
                )
                .map_err(|err| internal_failure("projects audit", err.to_string()))?;
                Ok(CommandSuccess::with_project(
                    "projects audit",
                    resolved.project_id.clone(),
                    serde_json::to_value(report)
                        .expect("canonicality audit report should serialize"),
                ))
            }
            Some("prune") => {
                let apply = has_flag(args, "--apply");
                let result = registry
                    .prune_missing(apply)
                    .map_err(|err| internal_failure("projects prune", err.to_string()))?;
                Ok(CommandSuccess::new_with_data(
                    "projects prune",
                    serde_json::to_value::<RegistryPruneResult>(result)
                        .expect("prune result should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("projects {other}"),
                "usage_invalid",
                format!("unknown projects subcommand: {other}"),
                Some("Choose one of: init, current, list, status, audit, prune.".to_string()),
            )),
            None => Err(CommandFailureOutcome::usage(
                "projects".to_string(),
                "usage_invalid",
                "missing projects subcommand".to_string(),
                Some("Choose one of: init, current, list, status, audit, prune.".to_string()),
            )),
        }
    }

    fn handle_permissions(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, "permissions")
            .map_err(|err| project_resolution_failure("permissions", err))?;
        let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
        match args.first().map(String::as_str) {
            Some("mode") => {
                if let Some(mode) = args.get(1) {
                    let scope = parse_flag_value(args, "--scope").ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "permissions mode".to_string(),
                            "usage_invalid",
                            "missing --scope for permissions mode mutation".to_string(),
                            Some("Pass --scope global|project|private.".to_string()),
                        )
                    })?;
                    if !is_valid_permission_mode(mode) {
                        return Err(CommandFailureOutcome::usage(
                            "permissions mode".to_string(),
                            "usage_invalid",
                            format!("invalid permission mode: {mode}"),
                            Some(
                                "Choose one of: read-only, workspace-write, danger-full-access."
                                    .to_string(),
                            ),
                        ));
                    }
                    let parsed_scope = ConfigScope::parse(&scope).ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "permissions mode".to_string(),
                            "usage_invalid",
                            format!("invalid config scope: {scope}"),
                            Some("Choose one of: global, project, private.".to_string()),
                        )
                    })?;
                    let result = config_set(&resolved, "permission_mode", mode, parsed_scope)
                        .map_err(|err| {
                            config_error_failure(
                                "permissions mode",
                                err,
                                Some("permission_mode".to_string()),
                            )
                        })?;
                    emit_permission_mode_event(&resolved, mode, parsed_scope.as_str())
                        .map_err(|err| internal_failure("permissions mode", err))?;
                    publish_project_checkpoint(&resolved, &["permission"])
                        .map_err(|err| internal_failure("permissions mode", err))?;
                    Ok(CommandSuccess::with_project(
                        "permissions mode",
                        resolved.project_id.clone(),
                        serde_json::to_value(result)
                            .expect("permission mode result should serialize"),
                    ))
                } else {
                    let effective = effective_config(&resolved).map_err(|err| {
                        config_error_failure(
                            "permissions mode",
                            err,
                            Some("permission_mode".to_string()),
                        )
                    })?;
                    let current_mode = effective
                        .effective
                        .permission_mode
                        .unwrap_or_else(|| "read-only".to_string());
                    let source = config::effective_value_source(&resolved, "permission_mode")
                        .map_err(|err| {
                            config_error_failure(
                                "permissions mode",
                                err,
                                Some("permission_mode".to_string()),
                            )
                        })?;
                    Ok(CommandSuccess::with_project(
                        "permissions mode",
                        resolved.project_id.clone(),
                        json!({
                            "permission_mode": current_mode,
                            "value_source": source.unwrap_or_else(|| "built_in_default".to_string()),
                            "resolved_scope": "project"
                        }),
                    ))
                }
            }
            Some("pending") => {
                let mut requests = Vec::new();
                let mut resolved_request_ids = std::collections::BTreeSet::new();
                for event in read_events_from(&event_log_path)
                    .map_err(|err| internal_failure("permissions pending", err.to_string()))?
                {
                    if event.event_name != "permission" {
                        continue;
                    }
                    if event.phase == "start" {
                        requests.push(event.payload);
                    } else if event.phase == "terminal" {
                        resolved_request_ids.insert(event.object_id);
                    }
                }
                requests.retain(|request| {
                    request
                        .get("request_id")
                        .and_then(Value::as_str)
                        .map(|request_id| !resolved_request_ids.contains(request_id))
                        .unwrap_or(true)
                });
                let data = PermissionPendingList {
                    total_count: requests.len(),
                    requests,
                };
                Ok(CommandSuccess::with_project(
                    "permissions pending",
                    resolved.project_id.clone(),
                    serde_json::to_value(data).expect("permission pending list should serialize"),
                ))
            }
            Some("history") => {
                let mut decisions = read_events_from(&event_log_path)
                    .map_err(|err| internal_failure("permissions history", err.to_string()))?
                    .into_iter()
                    .filter(|event| event.event_name == "permission" && event.phase == "terminal")
                    .map(|event| event.payload)
                    .collect::<Vec<_>>();
                if let Some(limit) =
                    parse_flag_value(args, "--limit").and_then(|value| value.parse::<usize>().ok())
                {
                    decisions.truncate(limit);
                }
                let data = PermissionHistoryResult {
                    total_count: decisions.len(),
                    decisions,
                };
                Ok(CommandSuccess::with_project(
                    "permissions history",
                    resolved.project_id.clone(),
                    serde_json::to_value(data).expect("permission history should serialize"),
                ))
            }
            Some("approve") | Some("deny") => {
                let decision = args.first().expect("permission decision subcommand exists");
                let request_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        format!("permissions {decision}"),
                        "usage_invalid",
                        format!("missing request id for permissions {decision}"),
                        Some("Run `research-cli permissions pending --json` first.".to_string()),
                    )
                })?;
                if args.len() > 2 {
                    return Err(CommandFailureOutcome::usage(
                        format!("permissions {decision}"),
                        "usage_invalid",
                        format!("permissions {decision} accepts exactly one request id"),
                        Some("Run `research-cli permissions pending --json` first.".to_string()),
                    ));
                }

                let payload = resolve_permission_decision(
                    &resolved,
                    decision,
                    request_id,
                    parsed.output_json,
                )?;

                Ok(CommandSuccess::with_project(
                    &format!("permissions {decision}"),
                    resolved.project_id.clone(),
                    payload,
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("permissions {other}"),
                "usage_invalid",
                format!("unknown permissions subcommand: {other}"),
                Some("Choose one of: mode, pending, history, approve, deny.".to_string()),
            )),
            None => Err(CommandFailureOutcome::usage(
                "permissions".to_string(),
                "usage_invalid",
                "missing permissions subcommand".to_string(),
                Some("Choose one of: mode, pending, history, approve, deny.".to_string()),
            )),
        }
    }

    fn handle_goals(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, "goals")
            .map_err(|err| project_resolution_failure("goals", err))?;
        match args.first().map(String::as_str) {
            Some("status") => {
                if args.len() > 1 {
                    return Err(CommandFailureOutcome::usage(
                        "goals status".to_string(),
                        "usage_invalid",
                        "goals status does not accept positional arguments".to_string(),
                        Some("Use `research-cli goals status --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let result = crate::goals::status(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                )
                .map_err(|err| goals_failure("goals status", err, &resolved, parsed.output_json))?;
                Ok(CommandSuccess::with_project(
                    "goals status",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("mission frame status should serialize"),
                ))
            }
            Some("set") => {
                let update = parse_mission_frame_update(&args[1..]).map_err(|err| {
                    CommandFailureOutcome::usage(
                        "goals set".to_string(),
                        "usage_invalid",
                        err,
                        Some(
                            "Pass --project-max-goal, --milestone-goal, and --current-implementation-goal."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = crate::goals::set(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    update,
                )
                .map_err(|err| goals_failure("goals set", err, &resolved, parsed.output_json))?;
                emit_mission_frame_event(&resolved, &result)
                    .map_err(|err| internal_failure("goals set", err))?;
                publish_project_checkpoint(&resolved, &["mission_frame"])
                    .map_err(|err| internal_failure("goals set", err))?;
                Ok(CommandSuccess::with_project(
                    "goals set",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("mission frame status should serialize"),
                ))
            }
            Some("advance") => {
                if args.iter().skip(1).any(|arg| arg != "--json") {
                    return Err(CommandFailureOutcome::usage(
                        "goals advance".to_string(),
                        "usage_invalid",
                        "goals advance accepts only --json".to_string(),
                        Some("Use `research-cli goals advance --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let result = crate::goals::advance(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                )
                .map_err(|err| {
                    goals_failure("goals advance", err, &resolved, parsed.output_json)
                })?;
                publish_goal_advance_result(&resolved, None, &result)
                    .map_err(|err| internal_failure("goals advance", err))?;
                Ok(CommandSuccess::with_project(
                    "goals advance",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("goal advance result should serialize"),
                ))
            }
            Some("tick") => {
                let request = parse_goal_tick_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "goals tick".to_string(),
                        "usage_invalid",
                        message,
                        Some("Use `research-cli goals tick [--trigger-kind <manual|schedule|webhook|api>] [--limit <n>] [--stale-after-sec <sec>]`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = run_goal_tick_once(&resolved, &request, parsed.output_json)
                    .map_err(|err| err.with_output_json(parsed.output_json))?;
                Ok(CommandSuccess::with_project(
                    "goals tick",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("goal tick result should serialize"),
                ))
            }
            Some("watch") => {
                match args.get(1).map(String::as_str) {
                    Some("plan") => {
                        parse_goal_watch_no_args("goals watch plan", &args[2..]).map_err(
                            |message| {
                                CommandFailureOutcome::usage(
                                    "goals watch plan".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli goals watch plan --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            },
                        )?;
                        let status = crate::goals::status(
                            &resolved.data_dir,
                            &resolved.workspace_root,
                            &resolved.project_id,
                        )
                        .map_err(|err| {
                            goals_failure("goals watch plan", err, &resolved, parsed.output_json)
                        })?;
                        return Ok(CommandSuccess::with_project(
                            "goals watch plan",
                            resolved.project_id.clone(),
                            serde_json::to_value(status.watch_plan)
                                .expect("goal watch plan should serialize"),
                        ));
                    }
                    Some("install") => {
                        let install_options = parse_goal_watch_install_request(
                            "goals watch install",
                            &args[2..],
                        )
                        .map_err(|message| {
                            CommandFailureOutcome::usage(
                                "goals watch install".to_string(),
                                "usage_invalid",
                                message,
                                Some("Use `research-cli goals watch install [--dry-run|--write-files|--apply-cron|--apply-systemd-user] --json`.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let status = crate::goals::status(
                            &resolved.data_dir,
                            &resolved.workspace_root,
                            &resolved.project_id,
                        )
                        .map_err(|err| {
                            goals_failure("goals watch install", err, &resolved, parsed.output_json)
                        })?;
                        let executable_path = current_cli_executable_hint();
                        let result = crate::goals::install_goal_watch_plan(
                            &resolved.data_dir,
                            &resolved.project_id,
                            &resolved.workspace_root,
                            status.task_pool.automation_mode,
                            &executable_path,
                            install_options,
                        )
                        .map_err(|err| {
                            goals_failure("goals watch install", err, &resolved, parsed.output_json)
                        })?;
                        return Ok(CommandSuccess::with_project(
                            "goals watch install",
                            resolved.project_id.clone(),
                            serde_json::to_value(result)
                                .expect("goal watch install result should serialize"),
                        ));
                    }
                    Some("status") => {
                        parse_goal_watch_no_args("goals watch status", &args[2..]).map_err(
                            |message| {
                                CommandFailureOutcome::usage(
                                    "goals watch status".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some(
                                        "Use `research-cli goals watch status --json`.".to_string(),
                                    ),
                                )
                                .with_output_json(parsed.output_json)
                            },
                        )?;
                        let status = crate::goals::status(
                            &resolved.data_dir,
                            &resolved.workspace_root,
                            &resolved.project_id,
                        )
                        .map_err(|err| {
                            goals_failure("goals watch status", err, &resolved, parsed.output_json)
                        })?;
                        let install_status = crate::goals::goal_watch_install_status(
                            &resolved.data_dir,
                            &resolved.project_id,
                            &resolved.workspace_root,
                            status.task_pool.automation_mode,
                        )
                        .map_err(|err| {
                            goals_failure("goals watch status", err, &resolved, parsed.output_json)
                        })?;
                        return Ok(CommandSuccess::with_project(
                            "goals watch status",
                            resolved.project_id.clone(),
                            serde_json::to_value(install_status)
                                .expect("goal watch install status should serialize"),
                        ));
                    }
                    Some("uninstall") => {
                        let uninstall_options = parse_goal_watch_install_request(
                            "goals watch uninstall",
                            &args[2..],
                        )
                        .map_err(|message| {
                            CommandFailureOutcome::usage(
                                "goals watch uninstall".to_string(),
                                "usage_invalid",
                                message,
                                Some("Use `research-cli goals watch uninstall [--dry-run|--write-files|--apply-cron|--apply-systemd-user] --json`.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let result = crate::goals::uninstall_goal_watch_plan(
                            &resolved.data_dir,
                            &resolved.project_id,
                            uninstall_options,
                        )
                        .map_err(|err| {
                            goals_failure(
                                "goals watch uninstall",
                                err,
                                &resolved,
                                parsed.output_json,
                            )
                        })?;
                        return Ok(CommandSuccess::with_project(
                            "goals watch uninstall",
                            resolved.project_id.clone(),
                            serde_json::to_value(result)
                                .expect("goal watch uninstall result should serialize"),
                        ));
                    }
                    _ => {}
                }
                let request = parse_goal_watch_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "goals watch".to_string(),
                        "usage_invalid",
                        message,
                        Some("Use `research-cli goals watch [--once] [--interval-ms <ms>] [--trigger-kind <manual|schedule|webhook|api>] [--limit <n>] [--stale-after-sec <sec>]`, `research-cli goals watch plan`, `research-cli goals watch install`, `research-cli goals watch status`, or `research-cli goals watch uninstall`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = self.handle_goal_watch(&resolved, parsed, request)?;
                Ok(result)
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("goals {other}"),
                "usage_invalid",
                format!("unknown goals subcommand: {other}"),
                Some("Choose one of: status, set, advance, tick, watch.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "goals".to_string(),
                "usage_invalid",
                "missing goals subcommand".to_string(),
                Some("Choose one of: status, set, advance, tick, watch.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_goal_watch(
        &self,
        resolved: &ResolvedProject,
        parsed: &ParsedCliArgs,
        request: GoalWatchRequest,
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let lock_path = goal_watch_lock_path(&resolved.data_dir);
        let state_path = crate::goals::goal_watch_state_path(&resolved.data_dir);
        let _lock_guard = acquire_goal_watch_lock(&resolved.data_dir)
            .map_err(|err| goal_watch_failure("goals watch", err, resolved, parsed.output_json))?;

        self::cancel::install_agent_sigint_handler();

        let mut ticks_completed = 0usize;
        let mut last_tick = None;
        save_goal_watch_health(
            resolved,
            "running",
            request.interval_ms,
            ticks_completed,
            "started",
            &lock_path,
            None,
            parsed.output_json,
        )?;

        let stop_reason = loop {
            if self::cancel::sigint_requested() {
                break "signal".to_string();
            }

            match run_goal_tick_once(resolved, &request.tick_request, parsed.output_json) {
                Ok(tick) => {
                    ticks_completed += 1;
                    if !parsed.output_json {
                        eprintln!("goals watch: tick {} => {}", ticks_completed, tick.status);
                    }
                    let tick_value =
                        serde_json::to_value(&tick).expect("goal tick result should serialize");
                    last_tick = Some(tick);
                    save_goal_watch_health(
                        resolved,
                        "running",
                        request.interval_ms,
                        ticks_completed,
                        "running",
                        &lock_path,
                        Some(tick_value),
                        parsed.output_json,
                    )?;
                }
                Err(err) => {
                    if err.envelope.error.retryable {
                        if !parsed.output_json {
                            eprintln!(
                                "goals watch: retryable tick failure: {}",
                                err.render_for_stderr()
                            );
                        }
                        sleep_goal_watch_interval(request.interval_ms);
                        continue;
                    }
                    return Err(err);
                }
            }

            if request.once {
                break "once".to_string();
            }

            if let Some(stop_reason) = last_tick
                .as_ref()
                .and_then(goal_watch_stop_reason_from_tick)
            {
                break stop_reason;
            }

            sleep_goal_watch_interval(request.interval_ms);
        };

        let status = if matches!(stop_reason.as_str(), "once" | "loop_completed") {
            "completed".to_string()
        } else {
            "stopped".to_string()
        };
        let last_tick_value = last_tick
            .as_ref()
            .map(|tick| serde_json::to_value(tick).expect("goal tick result should serialize"));
        save_goal_watch_health(
            resolved,
            &status,
            request.interval_ms,
            ticks_completed,
            &stop_reason,
            &lock_path,
            last_tick_value,
            parsed.output_json,
        )?;
        let result = GoalWatchResult {
            schema_version: "goal_watch_result.v1".to_string(),
            status,
            project_id: resolved.project_id.clone(),
            interval_ms: request.interval_ms,
            ticks_completed,
            stop_reason: stop_reason.clone(),
            watch_lock_path: lock_path.display().to_string(),
            watch_state_path: state_path.display().to_string(),
            last_tick,
        };
        let mut envelope = CommandSuccess::with_project(
            "goals watch",
            resolved.project_id.clone(),
            serde_json::to_value(result).expect("goal watch result should serialize"),
        );
        if !parsed.output_json {
            let text = goal_watch_text_summary(&envelope);
            envelope = envelope.with_text_override(text);
        }
        Ok(envelope)
    }
}
