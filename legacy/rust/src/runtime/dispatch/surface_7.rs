impl Runtime {
    fn handle_host(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("surface") if args.get(1).map(String::as_str) == Some("status") => {
                if has_unrecognized_positionals(&args[2..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "host surface status".to_string(),
                        "usage_invalid",
                        "host surface status does not accept positional arguments".to_string(),
                        Some("Use `research-cli host surface status --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "host surface status")
                    .map_err(|err| project_resolution_failure("host surface status", err))?;
                let state_home =
                    state_home().map_err(|err| internal_failure("host surface status", err))?;
                let result = host_surface::status(
                    &state_home,
                    &resolved,
                    active_session_id_for(registry, &resolved, "host surface status")?,
                )
                .map_err(|err| {
                    host_surface_failure("host surface status", &resolved, err, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "host surface status",
                    resolved.project_id,
                    serde_json::to_value(result).expect("host surface status should serialize"),
                ))
            }
            Some("surface") => Err(CommandFailureOutcome::usage(
                "host surface".to_string(),
                "usage_invalid",
                "missing or unknown host surface subcommand".to_string(),
                Some("Choose one of: status.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("host {other}"),
                "usage_invalid",
                format!("unknown host subcommand: {other}"),
                Some("Choose one of: surface status.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "host".to_string(),
                "usage_invalid",
                "missing host subcommand".to_string(),
                Some("Choose one of: surface status.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_tui(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let (subcommand, tui_tail): (&str, &[String]) = match args.first().map(String::as_str) {
            Some("launch") | Some("snapshot") | Some("actions") => (args[0].as_str(), &args[1..]),
            None => ("launch", &[]),
            Some(other) => {
                return Err(CommandFailureOutcome::usage(
                    format!("tui {other}"),
                    "usage_invalid",
                    format!("unknown tui subcommand: {other}"),
                    Some("Choose one of: launch, snapshot, actions.".to_string()),
                )
                .with_output_json(parsed.output_json));
            }
        };

        {
            let fullscreen_requested = subcommand == "launch"
                && !tui_tail.is_empty()
                && tui_tail
                    .iter()
                    .all(|arg| arg == "--fullscreen" || arg == "--split-pane");
            let invalid_tui_args = if subcommand == "launch" {
                !fullscreen_requested && !tui_tail.is_empty()
            } else {
                has_unrecognized_positionals(tui_tail, &[])
            };
            if invalid_tui_args {
                return Err(CommandFailureOutcome::usage(
                        format!("tui {subcommand}"),
                        "usage_invalid",
                        "tui projection commands do not accept positional arguments".to_string(),
                        Some("Use `research-cli tui launch [--fullscreen]`, `research-cli tui snapshot --json`, or `research-cli tui actions --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
            }
            let command = format!("tui {subcommand}");
            let resolved = self
                .resolve_project_or_workspace(registry, cwd, parsed, &command)
                .map_err(|err| {
                    project_resolution_failure(&command, err).with_output_json(parsed.output_json)
                })?;
            let state_home = state_home().map_err(|err| internal_failure(&command, err))?;
            let active_session_id = active_session_id_for(registry, &resolved, &command)?;
            let data = match subcommand {
                "launch" => serde_json::to_value(
                    host_surface::tui_launch_with_display_mode(
                        &state_home,
                        &resolved,
                        active_session_id.clone(),
                        fullscreen_requested,
                    )
                    .map_err(|err| {
                        host_surface_failure(&command, &resolved, err, parsed.output_json)
                    })?,
                )
                .expect("tui launch should serialize"),
                "snapshot" => serde_json::to_value(
                    host_surface::status(&state_home, &resolved, active_session_id.clone())
                        .map_err(|err| {
                            host_surface_failure(&command, &resolved, err, parsed.output_json)
                        })?,
                )
                .expect("tui snapshot should serialize"),
                "actions" => serde_json::to_value(
                    host_surface::tui_actions(&state_home, &resolved, active_session_id.clone())
                        .map_err(|err| {
                            host_surface_failure(&command, &resolved, err, parsed.output_json)
                        })?,
                )
                .expect("tui actions should serialize"),
                _ => unreachable!("handled by match guard"),
            };
            let mut success =
                CommandSuccess::with_project(&command, resolved.project_id.clone(), data);
            if subcommand == "launch" && !parsed.output_json {
                let launch: host_surface::TuiLaunchResult =
                    serde_json::from_value(success.data.clone().unwrap_or(Value::Null))
                        .map_err(|err| internal_failure(&command, err.to_string()))?;
                let tui_session_id = std::sync::Arc::new(std::sync::Mutex::new(active_session_id));
                let registry_for_tui = registry.clone();
                let resolved_for_tui = resolved.clone();
                let session_for_tui = std::sync::Arc::clone(&tui_session_id);
                let session_for_tui_config = std::sync::Arc::clone(&tui_session_id);
                let registry_for_tui_config = registry.clone();
                let resolved_for_tui_config = resolved.clone();
                let resolved_for_tui_permission = resolved.clone();
                let tui_prompt_executor = std::sync::Arc::new(std::sync::Mutex::new(
                    move |prompt: &str,
                          stream: tui::TuiStreamSender,
                          cancel_token: RuntimeCancelToken| {
                        let session_id = {
                            let mut session_guard = session_for_tui
                                .lock()
                                .map_err(|_| "TUI session state lock poisoned".to_string())?;
                            ensure_tui_prompt_session(
                                &registry_for_tui,
                                &resolved_for_tui,
                                &mut session_guard,
                            )?
                        };
                        run_tui_prompt_turn_streaming_with_cancel(
                            &registry_for_tui,
                            &resolved_for_tui,
                            &session_id,
                            prompt,
                            |delta| stream.send_delta(delta),
                            cancel_token,
                        )
                        .map(|turn| tui_execution_from_turn_result(&turn))
                    },
                ));
                success = success.with_text_override(
                    tui::run_inline_tui_with_cancellable_streaming_and_action_executors(
                        &launch,
                        tui_prompt_executor,
                        move |action| {
                            apply_bound_tui_action(
                                &registry_for_tui_config,
                                &resolved_for_tui_config,
                                &session_for_tui_config,
                                action,
                            )
                        },
                        move |action| {
                            apply_tui_permission_action(&resolved_for_tui_permission, action)
                        },
                    )
                    .map_err(|err| internal_failure(&command, err.to_string()))?,
                );
            }
            Ok(success)
        }
    }

    fn handle_remote(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("tailscale") if args.get(1).map(String::as_str) == Some("status") => {
                if has_unrecognized_positionals(&args[2..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote tailscale status".to_string(),
                        "usage_invalid",
                        "remote tailscale status does not accept positional arguments".to_string(),
                        Some("Use `research-cli remote tailscale status --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                Ok(CommandSuccess::new_with_data(
                    "remote tailscale status",
                    serde_json::to_value(host_surface::tailscale_status())
                        .expect("tailscale status should serialize"),
                ))
            }
            Some("tailscale") if args.get(1).map(String::as_str) == Some("serve-plan") => {
                if has_unrecognized_positionals(&args[2..], &["--daemon-port"]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote tailscale serve-plan".to_string(),
                        "usage_invalid",
                        "remote tailscale serve-plan accepts only --daemon-port".to_string(),
                        Some(
                            "Use `research-cli remote tailscale serve-plan --daemon-port 8787 --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let port = parse_flag_value(args, "--daemon-port")
                    .map(|value| {
                        value.parse::<u16>().map_err(|_| {
                            CommandFailureOutcome::usage(
                                "remote tailscale serve-plan".to_string(),
                                "usage_invalid",
                                format!("invalid daemon port: {value}"),
                                Some("Pass a TCP port between 0 and 65535.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })
                    })
                    .transpose()?
                    .unwrap_or(8787);
                Ok(CommandSuccess::new_with_data(
                    "remote tailscale serve-plan",
                    serde_json::to_value(host_surface::tailscale_serve_plan(port))
                        .expect("tailscale serve plan should serialize"),
                ))
            }
            Some("terminal") if args.get(1).map(String::as_str) == Some("attach") => {
                if has_unrecognized_positionals(&args[2..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote terminal attach".to_string(),
                        "usage_invalid",
                        "remote terminal attach does not accept positional arguments".to_string(),
                        Some("Use `research-cli remote terminal attach --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote terminal attach")
                    .map_err(|err| project_resolution_failure("remote terminal attach", err))?;
                reject_remote_not_ready(
                    "remote terminal attach",
                    "terminal_attach",
                    &resolved,
                    registry,
                    parsed.output_json,
                )?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote terminal attach", err))?;
                let result = host_surface::terminal_attach(
                    &state_home,
                    &resolved,
                    active_session_id_for(registry, &resolved, "remote terminal attach")?,
                )
                .map_err(|err| {
                    host_surface_failure("remote terminal attach", &resolved, err, parsed.output_json)
                })?;
                append_canonical_event(
                    &resolved,
                    "remote_terminal_attach",
                    "terminal",
                    Some("succeeded"),
                    "remote_terminal",
                    &result.terminal.terminal_id,
                    result.terminal.active_session_id.as_deref(),
                    serde_json::to_value(&result)
                        .expect("remote terminal attach result should serialize"),
                )
                .map_err(|err| internal_failure("remote terminal attach", err))?;
                publish_project_checkpoint(&resolved, &["remote", "terminal"])
                    .map_err(|err| internal_failure("remote terminal attach", err))?;
                Ok(CommandSuccess::with_project(
                    "remote terminal attach",
                    resolved.project_id,
                    serde_json::to_value(result)
                        .expect("remote terminal attach should serialize"),
                ))
            }
            Some("terminal") if args.get(1).map(String::as_str) == Some("replay") => {
                if has_unrecognized_positionals(&args[2..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote terminal replay".to_string(),
                        "usage_invalid",
                        "remote terminal replay does not accept positional arguments".to_string(),
                        Some("Use `research-cli remote terminal replay --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote terminal replay")
                    .map_err(|err| project_resolution_failure("remote terminal replay", err))?;
                reject_remote_not_ready(
                    "remote terminal replay",
                    "terminal_replay",
                    &resolved,
                    registry,
                    parsed.output_json,
                )?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote terminal replay", err))?;
                let result = host_surface::terminal_replay(
                    &state_home,
                    &resolved,
                    active_session_id_for(registry, &resolved, "remote terminal replay")?,
                )
                .map_err(|err| {
                    host_surface_failure("remote terminal replay", &resolved, err, parsed.output_json)
                })?;
                append_canonical_event(
                    &resolved,
                    "remote_terminal_replay",
                    "terminal",
                    Some("succeeded"),
                    "remote_terminal",
                    result.replay.cursor.as_str(),
                    result
                        .replay
                        .events
                        .first()
                        .and_then(|event| event.get("active_session_id"))
                        .and_then(|value| value.as_str()),
                    serde_json::to_value(&result)
                        .expect("remote terminal replay result should serialize"),
                )
                .map_err(|err| internal_failure("remote terminal replay", err))?;
                publish_project_checkpoint(&resolved, &["remote", "terminal"])
                    .map_err(|err| internal_failure("remote terminal replay", err))?;
                Ok(CommandSuccess::with_project(
                    "remote terminal replay",
                    resolved.project_id,
                    serde_json::to_value(result)
                        .expect("remote terminal replay should serialize"),
                ))
            }
            Some("daemon") => {
                if has_unrecognized_positionals(&args[1..], &["--host", "--port", "--control-token"]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote daemon".to_string(),
                        "usage_invalid",
                        "remote daemon accepts only --host, --port, and --control-token".to_string(),
                        Some(
                            "Use `research-cli remote daemon --host 0.0.0.0 --port 8787 --control-token <token> --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let host =
                    parse_flag_value(args, "--host").unwrap_or_else(|| "127.0.0.1".to_string());
                let port = parse_flag_value(args, "--port")
                    .map(|value| {
                        value.parse::<u16>().map_err(|_| {
                            CommandFailureOutcome::usage(
                                "remote daemon".to_string(),
                                "usage_invalid",
                                format!("invalid remote daemon port: {value}"),
                                Some("Pass a TCP port between 0 and 65535.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })
                    })
                    .transpose()?
                    .unwrap_or(8787);
                let state_home =
                    state_home().map_err(|err| internal_failure("remote daemon", err))?;
                let default_cwd = parsed.cwd.clone().unwrap_or_else(|| cwd.to_path_buf());
                let context = remote::daemon::DaemonContext::new(state_home, default_cwd);
                let control_token = parse_flag_value(args, "--control-token")
                    .or_else(|| env::var("RESEARCH_CLI_REMOTE_DAEMON_TOKEN").ok())
                    .filter(|token| !token.trim().is_empty());
                if is_non_loopback_bind_host(&host) && control_token.is_none() {
                    return Err(CommandFailureOutcome::usage(
                        "remote daemon".to_string(),
                        "usage_invalid",
                        "remote daemon requires a control token when binding to a non-loopback host"
                            .to_string(),
                        Some(
                            "Pass `--control-token <token>` or set RESEARCH_CLI_REMOTE_DAEMON_TOKEN."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let context = if let Some(token) = control_token {
                    context.with_control_token(token)
                } else {
                    context
                };
                remote::daemon::serve(
                    context,
                    &host,
                    port,
                    parsed.output_json,
                )
                .map_err(|err| internal_failure("remote daemon", err.to_string()))?;
                Ok(CommandSuccess::new("remote daemon"))
            }
            Some("status") => {
                if has_unrecognized_positionals(&args[1..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote status".to_string(),
                        "usage_invalid",
                        "remote status does not accept positional arguments".to_string(),
                        Some("Use `research-cli remote status --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote status")
                    .map_err(|err| project_resolution_failure("remote status", err))?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote status", err))?;
                let active_session_id =
                    active_session_id_for(registry, &resolved, "remote status")?;
                let report = remote::status(&state_home, &resolved, active_session_id)
                    .map_err(|err| internal_failure("remote status", err.to_string()))?;
                Ok(CommandSuccess::with_project(
                    "remote status",
                    resolved.project_id,
                    serde_json::to_value(report).expect("remote status should serialize"),
                ))
            }
            Some("pair") => {
                let client_id = parse_flag_value(args, "--client").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote pair".to_string(),
                        "usage_invalid",
                        "missing --client for remote pair".to_string(),
                        Some("Pass `--client <client-id>` for the remote client.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let ticket_id = parse_flag_value(args, "--ticket").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote pair".to_string(),
                        "usage_invalid",
                        "missing --ticket for remote pair".to_string(),
                        Some("Pass `--ticket <ticket-id>` for pairing lineage.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                if has_unrecognized_positionals(
                    &args[1..],
                    &[
                        "--client",
                        "--ticket",
                        "--lease-expires-at",
                        "--ticket-expires-at",
                    ],
                ) {
                    return Err(CommandFailureOutcome::usage(
                        "remote pair".to_string(),
                        "usage_invalid",
                        "remote pair accepts only --client, --ticket, --lease-expires-at, and --ticket-expires-at"
                            .to_string(),
                        Some(
                            "Use `research-cli remote pair --client <id> --ticket <ticket> --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote pair")
                    .map_err(|err| project_resolution_failure("remote pair", err))?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote pair", err))?;
                let active_session_id = active_session_id_for(registry, &resolved, "remote pair")?;
                remote::issue_pair_ticket(
                    &state_home,
                    &resolved,
                    &client_id,
                    &ticket_id,
                    parse_flag_value(args, "--ticket-expires-at").as_deref(),
                )
                .map_err(|err| remote_failure("remote pair", &resolved, err, parsed.output_json))?;
                let report = remote::pair(
                    &state_home,
                    &resolved,
                    &client_id,
                    &ticket_id,
                    parse_flag_value(args, "--lease-expires-at").as_deref(),
                    parse_flag_value(args, "--ticket-expires-at").as_deref(),
                    active_session_id,
                )
                .map_err(|err| remote_failure("remote pair", &resolved, err, parsed.output_json))?;
                Ok(CommandSuccess::with_project(
                    "remote pair",
                    resolved.project_id,
                    serde_json::to_value(report).expect("remote pair should serialize"),
                ))
            }
            Some("attach") => {
                if has_unrecognized_remote_args(
                    &args[1..],
                    &["--session", "--strategy"],
                    &["--execute"],
                ) {
                    return Err(CommandFailureOutcome::usage(
                        "remote attach".to_string(),
                        "usage_invalid",
                        "remote attach accepts only --session, --strategy, and --execute"
                            .to_string(),
                        Some(
                            "Use `research-cli remote attach --session <id> --execute --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote attach")
                    .map_err(|err| project_resolution_failure("remote attach", err))?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote attach", err))?;
                let remote_report = remote::status(
                    &state_home,
                    &resolved,
                    active_session_id_for(registry, &resolved, "remote attach")?,
                )
                .map_err(|err| internal_failure("remote attach", err.to_string()))?;
                if !remote_report.remote_ready {
                    let rejection = match remote_report.revocation_reason.as_deref() {
                        Some("lease_expired") => remote::lease_expired_rejection("attach"),
                        Some("revoked") => remote::binding_revoked_rejection("attach"),
                        _ => remote::attach_rejection("attach"),
                    };
                    return Err(CommandFailureOutcome::new(
                        12,
                        "remote attach".to_string(),
                        Some(resolved.project_id),
                        None,
                        "remote_action_rejected",
                        rejection.reason.clone(),
                        rejection.next_steps.first().cloned(),
                        rejection.retryable,
                        Some(
                            serde_json::to_value(rejection)
                                .expect("remote rejection should serialize"),
                        ),
                        parsed.output_json,
                    ));
                }
                let session_id = parse_flag_value(args, "--session")
                    .or_else(|| {
                        active_session_id_for(registry, &resolved, "remote attach")
                            .ok()
                            .flatten()
                    })
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "remote attach".to_string(),
                            "usage_invalid",
                            "missing --session for remote attach".to_string(),
                            Some("Pass `--session <session-id>`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let store = SessionStore::new(resolved.data_dir.clone());
                store.load_session(&session_id).map_err(|err| {
                    session_store_failure("remote attach", err, &resolved, Some(session_id.clone()))
                })?;
                let result = remote::attach(
                    &state_home,
                    &resolved,
                    &session_id,
                    parse_flag_value(args, "--strategy").as_deref(),
                    has_flag(args, "--execute"),
                )
                .map_err(|err| {
                    remote_failure("remote attach", &resolved, err, parsed.output_json)
                })?;
                if has_flag(args, "--execute") {
                    append_canonical_event(
                        &resolved,
                        "remote_attach",
                        "terminal",
                        Some("succeeded"),
                        "session",
                        &session_id,
                        Some(&session_id),
                        serde_json::to_value(&result)
                            .expect("remote attach result should serialize"),
                    )
                    .map_err(|err| internal_failure("remote attach", err))?;
                }
                Ok(CommandSuccess::with_project_and_session(
                    "remote attach",
                    resolved.project_id,
                    session_id,
                    serde_json::to_value(result).expect("remote attach result should serialize"),
                ))
            }
            Some("handoff") => {
                if has_unrecognized_remote_args(
                    &args[1..],
                    &["--session", "--target-client"],
                    &["--execute"],
                ) {
                    return Err(CommandFailureOutcome::usage(
                        "remote handoff".to_string(),
                        "usage_invalid",
                        "remote handoff accepts only --session, --target-client, and --execute"
                            .to_string(),
                        Some(
                            "Use `research-cli remote handoff --session <id> --target-client <client> --execute --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote handoff")
                    .map_err(|err| project_resolution_failure("remote handoff", err))?;
                reject_remote_not_ready(
                    "remote handoff",
                    "handoff",
                    &resolved,
                    registry,
                    parsed.output_json,
                )?;
                let session_id = parse_flag_value(args, "--session")
                    .or_else(|| {
                        active_session_id_for(registry, &resolved, "remote handoff")
                            .ok()
                            .flatten()
                    })
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "remote handoff".to_string(),
                            "usage_invalid",
                            "missing --session for remote handoff".to_string(),
                            Some("Pass `--session <session-id>`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let target_client = parse_flag_value(args, "--target-client").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote handoff".to_string(),
                        "usage_invalid",
                        "missing --target-client for remote handoff".to_string(),
                        Some("Pass `--target-client <client-id>`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let store = SessionStore::new(resolved.data_dir.clone());
                store.load_session(&session_id).map_err(|err| {
                    session_store_failure(
                        "remote handoff",
                        err,
                        &resolved,
                        Some(session_id.clone()),
                    )
                })?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote handoff", err))?;
                let result = remote::handoff(
                    &state_home,
                    &resolved,
                    &session_id,
                    &target_client,
                    has_flag(args, "--execute"),
                )
                .map_err(|err| {
                    remote_failure("remote handoff", &resolved, err, parsed.output_json)
                })?;
                if has_flag(args, "--execute") {
                    append_canonical_event(
                        &resolved,
                        "remote_handoff",
                        "terminal",
                        Some("succeeded"),
                        "session",
                        &session_id,
                        Some(&session_id),
                        serde_json::to_value(&result)
                            .expect("remote handoff result should serialize"),
                    )
                    .map_err(|err| internal_failure("remote handoff", err))?;
                }
                Ok(CommandSuccess::with_project_and_session(
                    "remote handoff",
                    resolved.project_id,
                    session_id,
                    serde_json::to_value(result).expect("remote handoff result should serialize"),
                ))
            }
            Some("takeover") => {
                if has_unrecognized_positionals(&args[1..], &["--session", "--client"]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote takeover".to_string(),
                        "usage_invalid",
                        "remote takeover accepts only --session and --client".to_string(),
                        Some(
                            "Use `research-cli remote takeover --session <id> --client <client> --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote takeover")
                    .map_err(|err| project_resolution_failure("remote takeover", err))?;
                reject_remote_not_ready(
                    "remote takeover",
                    "takeover",
                    &resolved,
                    registry,
                    parsed.output_json,
                )?;
                let session_id = parse_flag_value(args, "--session")
                    .or_else(|| {
                        active_session_id_for(registry, &resolved, "remote takeover")
                            .ok()
                            .flatten()
                    })
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "remote takeover".to_string(),
                            "usage_invalid",
                            "missing --session for remote takeover".to_string(),
                            Some("Pass `--session <session-id>`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let client_id = parse_flag_value(args, "--client").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote takeover".to_string(),
                        "usage_invalid",
                        "missing --client for remote takeover".to_string(),
                        Some("Pass `--client <client-id>`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let store = SessionStore::new(resolved.data_dir.clone());
                store.load_session(&session_id).map_err(|err| {
                    session_store_failure(
                        "remote takeover",
                        err,
                        &resolved,
                        Some(session_id.clone()),
                    )
                })?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote takeover", err))?;
                let result = remote::takeover(&state_home, &resolved, &session_id, &client_id)
                    .map_err(|err| {
                        remote_failure("remote takeover", &resolved, err, parsed.output_json)
                    })?;
                append_canonical_event(
                    &resolved,
                    "remote_takeover",
                    "terminal",
                    Some("succeeded"),
                    "session",
                    &session_id,
                    Some(&session_id),
                    serde_json::to_value(&result).expect("remote takeover result should serialize"),
                )
                .map_err(|err| internal_failure("remote takeover", err))?;
                Ok(CommandSuccess::with_project_and_session(
                    "remote takeover",
                    resolved.project_id,
                    session_id,
                    serde_json::to_value(result).expect("remote takeover result should serialize"),
                ))
            }
            Some("notify") => {
                if has_unrecognized_positionals(&args[1..], &["--kind", "--message"]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote notify".to_string(),
                        "usage_invalid",
                        "remote notify accepts only --kind and --message".to_string(),
                        Some(
                            "Use `research-cli remote notify --kind <kind> --message <text> --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote notify")
                    .map_err(|err| project_resolution_failure("remote notify", err))?;
                reject_remote_not_ready(
                    "remote notify",
                    "notify",
                    &resolved,
                    registry,
                    parsed.output_json,
                )?;
                let kind = parse_flag_value(args, "--kind").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote notify".to_string(),
                        "usage_invalid",
                        "missing --kind for remote notify".to_string(),
                        Some("Pass `--kind <notification-kind>`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let message = parse_flag_value(args, "--message").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote notify".to_string(),
                        "usage_invalid",
                        "missing --message for remote notify".to_string(),
                        Some("Pass `--message <text>`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote notify", err))?;
                let result =
                    remote::notify(&state_home, &resolved, &kind, &message).map_err(|err| {
                        remote_failure("remote notify", &resolved, err, parsed.output_json)
                    })?;
                let active_session_id = active_session_id_for(registry, &resolved, "remote notify")
                    .ok()
                    .flatten();
                append_canonical_event(
                    &resolved,
                    "remote_notify",
                    "terminal",
                    Some("succeeded"),
                    "project",
                    &resolved.project_id,
                    active_session_id.as_deref(),
                    serde_json::to_value(&result).expect("remote notify result should serialize"),
                )
                .map_err(|err| internal_failure("remote notify", err))?;
                Ok(CommandSuccess::with_project(
                    "remote notify",
                    resolved.project_id,
                    serde_json::to_value(result).expect("remote notify result should serialize"),
                ))
            }
            Some("revoke") => {
                if has_unrecognized_positionals(&args[1..], &["--client"]) {
                    return Err(CommandFailureOutcome::usage(
                        "remote revoke".to_string(),
                        "usage_invalid",
                        "remote revoke accepts only --client".to_string(),
                        Some("Use `research-cli remote revoke --client <client> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let client_id = parse_flag_value(args, "--client").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "remote revoke".to_string(),
                        "usage_invalid",
                        "missing --client for remote revoke".to_string(),
                        Some("Pass `--client <client-id>`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "remote revoke")
                    .map_err(|err| project_resolution_failure("remote revoke", err))?;
                let state_home =
                    state_home().map_err(|err| internal_failure("remote revoke", err))?;
                let result = remote::revoke(&state_home, &resolved, &client_id)
                    .map_err(|err| remote_failure("remote revoke", &resolved, err, parsed.output_json))?;
                append_canonical_event(
                    &resolved,
                    "remote_lease_revoked",
                    "terminal",
                    Some("succeeded"),
                    "remote",
                    &client_id,
                    None,
                    serde_json::to_value(&result).expect("remote revoke result should serialize"),
                )
                .map_err(|err| internal_failure("remote revoke", err))?;
                append_canonical_event(
                    &resolved,
                    "remote_binding_revoked",
                    "terminal",
                    Some("succeeded"),
                    "remote",
                    &client_id,
                    None,
                    serde_json::to_value(&result).expect("remote revoke result should serialize"),
                )
                .map_err(|err| internal_failure("remote revoke", err))?;
                publish_project_checkpoint(&resolved, &["remote"])
                    .map_err(|err| internal_failure("remote revoke", err))?;
                Ok(CommandSuccess::with_project(
                    "remote revoke",
                    resolved.project_id,
                    serde_json::to_value(result).expect("remote revoke result should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("remote {other}"),
                "usage_invalid",
                format!("unknown remote subcommand: {other}"),
                Some(
                    "Choose one of: daemon, status, pair, attach, handoff, takeover, notify, revoke, tailscale status, tailscale serve-plan, terminal attach, terminal replay."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "remote".to_string(),
                "usage_invalid",
                "missing remote subcommand".to_string(),
                Some(
                    "Choose one of: daemon, status, pair, attach, handoff, takeover, notify, revoke, tailscale status, tailscale serve-plan, terminal attach, terminal replay."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_reviews(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("open") => {
                let objective = parse_flag_value(args, "--objective").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "reviews open".to_string(),
                        "usage_invalid",
                        "missing --objective".to_string(),
                        Some("Pass `--objective <text>` to describe the review task.".to_string()),
                    )
                })?;
                let target_paths = parse_flag_values(args, "--target");
                if target_paths.is_empty() {
                    return Err(CommandFailureOutcome::usage(
                        "reviews open".to_string(),
                        "usage_invalid",
                        "missing --target".to_string(),
                        Some("Pass one or more `--target <path>` values for review.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }

                let resolved = self
                    .resolve_project(registry, cwd, parsed, "reviews")
                    .map_err(|err| project_resolution_failure("reviews open", err))?;
                for target in &target_paths {
                    if let Err(err) = crate::workspace::path::resolve_existing_workspace_path(
                        &resolved.workspace_root,
                        target,
                    ) {
                        let (code, message) = match err {
                            crate::workspace::path::WorkspacePathError::AbsolutePath(_)
                            | crate::workspace::path::WorkspacePathError::ScopeViolation {
                                ..
                            } => ("workspace_scope_violation", err.to_string()),
                            _ => (
                                "review_target_not_found",
                                format!("review target does not exist: {target}"),
                            ),
                        };
                        return Err(CommandFailureOutcome::new(
                            6,
                            "reviews open".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            code,
                            message,
                            Some(
                                "Pass a workspace-relative path that exists before opening the review."
                                    .to_string(),
                            ),
                            false,
                            Some(json!({ "target_path": target })),
                            parsed.output_json,
                        ));
                    }
                }

                let result = reviews::open(
                    &resolved.data_dir,
                    reviews::ReviewOpenRequest {
                        target_paths,
                        objective,
                        reviewer_role: parse_flag_value(args, "--reviewer-role")
                            .unwrap_or_else(|| "external_reviewer".to_string()),
                        review_model: parse_flag_value(args, "--review-model")
                            .unwrap_or_else(|| "gpt-5.4".to_string()),
                        blind_context: parse_flag_values(args, "--context"),
                        review_materials: Vec::new(),
                        executor_summary: parse_flag_value(args, "--executor-summary"),
                        evidence_required: parse_flag_values(args, "--evidence"),
                        compare_against: parse_flag_value(args, "--compare-against"),
                        retry_of: None,
                        retry_attempt: 0,
                        verdict: parse_flag_value(args, "--verdict"),
                        response_text: parse_flag_value(args, "--response"),
                    },
                )
                .map_err(|err| match err {
                    reviews::ReviewError::InvalidCompareTarget(_) => CommandFailureOutcome::new(
                        6,
                        "reviews open".to_string(),
                        Some(resolved.project_id.clone()),
                        None,
                        "review_compare_target_unknown",
                        err.to_string(),
                        Some(
                            "Pass an existing review id to `--compare-against`, or omit it."
                                .to_string(),
                        ),
                        false,
                        None,
                        parsed.output_json,
                    ),
                    other => internal_failure("reviews open", other.to_string()),
                })?;

                emit_review_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    json!({
                        "review_id": result.packet.review_id,
                        "status": result.status,
                        "verdict": result.trace.verdict,
                        "trace_path": result.trace.trace_path
                    }),
                    result.packet.review_id.as_str(),
                )
                .map_err(|err| internal_failure("reviews open", err))?;
                let mission_frame = crate::goals::projection(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                )
                .map_err(|err| goals_failure("reviews open", err, &resolved, parsed.output_json))?;
                let mission_frame_value =
                    serde_json::to_value(&mission_frame).expect("mission frame should serialize");
                let review_alignment_text = format!(
                    "{} {}",
                    result.packet.objective,
                    result.packet.target_paths.join(" ")
                );
                emit_goal_alignment_trace_event(
                    &resolved,
                    result.packet.review_id.as_str(),
                    goal_alignment_trace_payload_from_text(
                        &resolved,
                        result.packet.review_id.as_str(),
                        "reviews open",
                        mission_frame
                            .as_ref()
                            .map(|frame| frame.mission_frame_ref.as_str())
                            .unwrap_or_default(),
                        Some(&mission_frame_value),
                        &review_alignment_text,
                        "review_packet",
                    ),
                )
                .map_err(|err| internal_failure("reviews open", err))?;
                publish_project_checkpoint(&resolved, &["review"])
                    .map_err(|err| internal_failure("reviews open", err))?;
                Ok(CommandSuccess::with_project(
                    "reviews open",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("review open should serialize"),
                ))
            }
            Some("retry") => {
                let review_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "reviews retry".to_string(),
                        "usage_invalid",
                        "missing review id".to_string(),
                        Some("Pass a review id after `reviews retry`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "reviews")
                    .map_err(|err| project_resolution_failure("reviews retry", err))?;
                let force = args.iter().any(|arg| arg == "--force");
                let result =
                    reviews::retry(&resolved.data_dir, review_id, force).map_err(|err| {
                        match err {
                            reviews::ReviewError::UnknownReview(_) => CommandFailureOutcome::new(
                                6,
                                "reviews retry".to_string(),
                                Some(resolved.project_id.clone()),
                                None,
                                "review_not_found",
                                err.to_string(),
                                Some(
                                    "Run `research-cli reviews list --json` to inspect valid review ids."
                                        .to_string(),
                                ),
                                false,
                                None,
                                parsed.output_json,
                            ),
                            reviews::ReviewError::RetryNotAllowed { review_id, verdict } => {
                                CommandFailureOutcome::new(
                                    9,
                                    "reviews retry".to_string(),
                                    Some(resolved.project_id.clone()),
                                    None,
                                    "review_retry_not_allowed",
                                    format!(
                                        "review retry is not allowed for verdict {verdict}"
                                    ),
                                    Some("Retry is allowed for timeout, transport_failure, or explicit `--force` rerun policy.".to_string()),
                                    false,
                                    Some(json!({
                                        "failure_code": "review_retry_not_allowed",
                                        "review_id": review_id,
                                        "verdict": verdict
                                    })),
                                    parsed.output_json,
                                )
                            }
                            reviews::ReviewError::InvalidCompareTarget(_) => {
                                CommandFailureOutcome::new(
                                    6,
                                    "reviews retry".to_string(),
                                    Some(resolved.project_id.clone()),
                                    None,
                                    "review_compare_target_unknown",
                                    err.to_string(),
                                    Some("Repair the source review compare linkage before retrying.".to_string()),
                                    false,
                                    None,
                                    parsed.output_json,
                                )
                            }
                            other => internal_failure("reviews retry", other.to_string()),
                        }
                    })?;

                emit_review_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    json!({
                        "review_id": result.packet.review_id,
                        "retry_of": result.packet.retry_of,
                        "status": result.status,
                        "verdict": result.trace.verdict,
                        "trace_path": result.trace.trace_path
                    }),
                    result.packet.review_id.as_str(),
                )
                .map_err(|err| internal_failure("reviews retry", err))?;
                publish_project_checkpoint(&resolved, &["review"])
                    .map_err(|err| internal_failure("reviews retry", err))?;
                Ok(CommandSuccess::with_project(
                    "reviews retry",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("reviews retry should serialize"),
                ))
            }
            Some("resolve") => {
                let review_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "reviews resolve".to_string(),
                        "usage_invalid",
                        "missing review id".to_string(),
                        Some("Pass a review id after `reviews resolve`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let verdict = parse_flag_value(args, "--verdict").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "reviews resolve".to_string(),
                        "usage_invalid",
                        "missing --verdict".to_string(),
                        Some(
                            "Pass `--verdict pass|fail|approved|rejected|ok` to resolve the review."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let response_text = parse_flag_value(args, "--response").unwrap_or_default();
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "reviews")
                    .map_err(|err| project_resolution_failure("reviews resolve", err))?;
                let result = reviews::resolve(
                    &resolved.data_dir,
                    review_id,
                    reviews::ReviewResolveRequest {
                        verdict,
                        response_text,
                    },
                )
                .map_err(|err| match err {
                    reviews::ReviewError::UnknownReview(_) => CommandFailureOutcome::new(
                        6,
                        "reviews resolve".to_string(),
                        Some(resolved.project_id.clone()),
                        None,
                        "review_not_found",
                        err.to_string(),
                        Some(
                            "Run `research-cli reviews list --json` to inspect valid review ids."
                                .to_string(),
                        ),
                        false,
                        None,
                        parsed.output_json,
                    ),
                    reviews::ReviewError::ResolveNotAllowed { review_id, verdict } => {
                        CommandFailureOutcome::new(
                            9,
                            "reviews resolve".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "review_resolve_not_allowed",
                            format!("review resolve is not allowed for verdict {verdict}"),
                            Some("Only pending reviews can be resolved.".to_string()),
                            false,
                            Some(json!({
                                "failure_code": "review_resolve_not_allowed",
                                "review_id": review_id,
                                "verdict": verdict
                            })),
                            parsed.output_json,
                        )
                    }
                    reviews::ReviewError::InvalidVerdict(verdict) => CommandFailureOutcome::new(
                        6,
                        "reviews resolve".to_string(),
                        Some(resolved.project_id.clone()),
                        None,
                        "review_verdict_invalid",
                        format!("invalid review verdict: {verdict}"),
                        Some("Use a non-empty verdict token other than pending.".to_string()),
                        false,
                        Some(json!({
                            "failure_code": "review_verdict_invalid",
                            "verdict": verdict
                        })),
                        parsed.output_json,
                    ),
                    other => internal_failure("reviews resolve", other.to_string()),
                })?;

                emit_review_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    json!({
                        "review_id": result.packet.review_id,
                        "status": result.status,
                        "verdict": result.trace.verdict,
                        "trace_path": result.trace.trace_path
                    }),
                    result.packet.review_id.as_str(),
                )
                .map_err(|err| internal_failure("reviews resolve", err))?;
                publish_project_checkpoint(&resolved, &["review"])
                    .map_err(|err| internal_failure("reviews resolve", err))?;
                Ok(CommandSuccess::with_project(
                    "reviews resolve",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("reviews resolve should serialize"),
                ))
            }
            Some("list") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "reviews")
                    .map_err(|err| project_resolution_failure("reviews list", err))?;
                let result = reviews::list(
                    &resolved.data_dir,
                    parse_flag_value(args, "--status").as_deref(),
                    parse_flag_value(args, "--verdict").as_deref(),
                )
                .map_err(|err| internal_failure("reviews list", err.to_string()))?;
                Ok(CommandSuccess::with_project(
                    "reviews list",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("reviews list should serialize"),
                ))
            }
            Some("inspect") => {
                let review_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "reviews inspect".to_string(),
                        "usage_invalid",
                        "missing review id".to_string(),
                        Some("Pass a review id after `reviews inspect`.".to_string()),
                    )
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "reviews")
                    .map_err(|err| project_resolution_failure("reviews inspect", err))?;
                let result = reviews::inspect(&resolved.data_dir, review_id).map_err(|err| {
                    match err {
                        reviews::ReviewError::UnknownReview(_) => CommandFailureOutcome::new(
                            6,
                            "reviews inspect".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "review_not_found",
                            err.to_string(),
                            Some(
                                "Run `research-cli reviews list --json` to inspect valid review ids."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("reviews inspect", other.to_string()),
                    }
                })?;
                Ok(CommandSuccess::with_project(
                    "reviews inspect",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("reviews inspect should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("reviews {other}"),
                "usage_invalid",
                format!("unknown reviews subcommand: {other}"),
                Some("Choose one of: open, retry, resolve, list, inspect.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "reviews".to_string(),
                "usage_invalid",
                "missing reviews subcommand".to_string(),
                Some("Choose one of: open, retry, resolve, list, inspect.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_docs(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("index") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "docs")
                    .map_err(|err| project_resolution_failure("docs index", err))?;
                let result = docs::index(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                )
                .map_err(|err| docs_failure("docs index", err, &resolved, parsed.output_json))?;
                publish_project_checkpoint(&resolved, &["docs"]).map_err(|err| {
                    internal_failure("docs index", err).with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "docs index",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("docs index should serialize"),
                ))
            }
            Some("inspect") => {
                let target = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "docs inspect".to_string(),
                        "usage_invalid",
                        "missing document path".to_string(),
                        Some(
                            "Pass a workspace-relative markdown path after `docs inspect`."
                                .to_string(),
                        ),
                    )
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "docs")
                    .map_err(|err| project_resolution_failure("docs inspect", err))?;
                let result = docs::inspect(&resolved.workspace_root, target).map_err(|err| {
                    docs_failure("docs inspect", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "docs inspect",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("docs inspect should serialize"),
                ))
            }
            Some("frame") => match args.get(1).map(String::as_str) {
                Some("refresh") => {
                    let target = args.get(2).ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "docs frame refresh".to_string(),
                            "usage_invalid",
                            "missing document path".to_string(),
                            Some(
                                "Pass a workspace-relative markdown path after `docs frame refresh`."
                                    .to_string(),
                            ),
                        )
                    })?;
                    let dry_run = has_flag(args, "--dry-run");
                    let resolved = self
                        .resolve_project(registry, cwd, parsed, "docs")
                        .map_err(|err| project_resolution_failure("docs frame refresh", err))?;
                    let result = docs::frame_refresh(&resolved.workspace_root, target, dry_run)
                        .map_err(|err| {
                            docs_failure("docs frame refresh", err, &resolved, parsed.output_json)
                        })?;
                    Ok(CommandSuccess::with_project(
                        "docs frame refresh",
                        resolved.project_id.clone(),
                        serde_json::to_value(result).expect("docs frame refresh should serialize"),
                    ))
                }
                Some(other) => Err(CommandFailureOutcome::usage(
                    format!("docs frame {other}"),
                    "usage_invalid",
                    format!("unknown docs frame subcommand: {other}"),
                    Some("Choose one of: refresh.".to_string()),
                )
                .with_output_json(parsed.output_json)),
                None => Err(CommandFailureOutcome::usage(
                    "docs frame".to_string(),
                    "usage_invalid",
                    "missing docs frame subcommand".to_string(),
                    Some("Choose one of: refresh.".to_string()),
                )
                .with_output_json(parsed.output_json)),
            },
            Some("publish-candidate") => {
                let target = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "docs publish-candidate".to_string(),
                        "usage_invalid",
                        "missing document path".to_string(),
                        Some(
                            "Use `docs publish-candidate <path> --family <family> [--kind <kind>] [--policy <policy>] [--human-gate] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "docs publish-candidate")
                    .map_err(|err| {
                        project_resolution_failure("docs publish-candidate", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request =
                    parse_docs_publish_candidate_request(target, &args[2..]).map_err(|message| {
                        CommandFailureOutcome::usage(
                            "docs publish-candidate".to_string(),
                            "usage_invalid",
                            message,
                            Some(
                                "Use `docs publish-candidate <path> --family <family> [--kind <kind>] [--policy <policy>] [--human-gate] --json`."
                                    .to_string(),
                            ),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let result = skills::submit_native_output(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    request,
                )
                .map_err(|err| {
                    skill_output_failure(
                        "docs publish-candidate",
                        err,
                        Some(resolved.project_id.clone()),
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::with_project(
                    "docs publish-candidate",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("docs publish-candidate should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("docs {other}"),
                "usage_invalid",
                format!("unknown docs subcommand: {other}"),
                Some(
                    "Choose one of: index, inspect, frame refresh, publish-candidate.".to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "docs".to_string(),
                "usage_invalid",
                "missing docs subcommand".to_string(),
                Some(
                    "Choose one of: index, inspect, frame refresh, publish-candidate.".to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_sessions(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, "sessions")
            .map_err(|err| project_resolution_failure("sessions", err))?;
        let store = SessionStore::new(resolved.data_dir.clone());

        match args.first().map(String::as_str) {
            Some("create") => {
                let title = parse_flag_value(args, "--title");
                let session = store.create_session(title).map_err(|err| {
                    session_store_failure("sessions create", err, &resolved, None)
                })?;
                registry
                    .touch_project(&resolved.project_id, Some(session.session_id.clone()))
                    .map_err(|err| internal_failure("sessions create", err.to_string()))?;
                let stored = store.load_session(&session.session_id).map_err(|err| {
                    session_store_failure(
                        "sessions create",
                        err,
                        &resolved,
                        Some(session.session_id.clone()),
                    )
                })?;
                emit_session_open_event(
                    &resolved,
                    &session.session_id,
                    &stored.transcript_path.display().to_string(),
                )
                .map_err(|err| internal_failure("sessions create", err))?;
                publish_project_checkpoint(&resolved, &["session"])
                    .map_err(|err| internal_failure("sessions create", err))?;
                Ok(CommandSuccess::with_project_and_session(
                    "sessions create",
                    resolved.project_id.clone(),
                    session.session_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "session": session
                    }),
                ))
            }
            Some("list") => {
                let sessions = store
                    .list_sessions()
                    .map_err(|err| session_store_failure("sessions list", err, &resolved, None))?;
                Ok(CommandSuccess::with_project(
                    "sessions list",
                    resolved.project_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "sessions": sessions
                    }),
                ))
            }
            Some("browse") => {
                let limit =
                    parse_flag_value(args, "--limit").and_then(|value| value.parse::<usize>().ok());
                let sessions = store.browse_sessions(limit).map_err(|err| {
                    session_store_failure("sessions browse", err, &resolved, None)
                })?;
                Ok(CommandSuccess::with_project(
                    "sessions browse",
                    resolved.project_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "sessions": sessions
                    }),
                ))
            }
            Some("rename") => {
                let session_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "sessions rename".to_string(),
                        "usage_invalid",
                        "missing session id for rename".to_string(),
                        Some(
                            "Use `research-cli sessions rename <session-id> --title <title> --json`."
                                .to_string(),
                        ),
                    )
                })?;
                let title = parse_flag_value(args, "--title").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "sessions rename".to_string(),
                        "usage_invalid",
                        "missing --title for sessions rename".to_string(),
                        Some("Pass the replacement title with `--title`.".to_string()),
                    )
                })?;
                let session = store
                    .rename_session(session_id, Some(title))
                    .map_err(|err| {
                        session_store_failure(
                            "sessions rename",
                            err,
                            &resolved,
                            Some(session_id.clone()),
                        )
                    })?;
                publish_project_checkpoint(&resolved, &["session"])
                    .map_err(|err| internal_failure("sessions rename", err))?;
                Ok(CommandSuccess::with_project_and_session(
                    "sessions rename",
                    resolved.project_id.clone(),
                    session.session_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "session": session
                    }),
                ))
            }
            Some("delete") => {
                let session_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "sessions delete".to_string(),
                        "usage_invalid",
                        "missing session id for delete".to_string(),
                        Some("Use `research-cli sessions delete <session-id> --json`.".to_string()),
                    )
                })?;
                store.delete_session(session_id).map_err(|err| {
                    session_store_failure(
                        "sessions delete",
                        err,
                        &resolved,
                        Some(session_id.clone()),
                    )
                })?;
                if registry
                    .get_by_project_id(&resolved.project_id)
                    .map_err(|err| internal_failure("sessions delete", err.to_string()))?
                    .and_then(|entry| entry.active_session_id)
                    .as_deref()
                    == Some(session_id.as_str())
                {
                    registry
                        .touch_project(&resolved.project_id, None)
                        .map_err(|err| internal_failure("sessions delete", err.to_string()))?;
                }
                publish_project_checkpoint(&resolved, &["session"])
                    .map_err(|err| internal_failure("sessions delete", err))?;
                Ok(CommandSuccess::with_project(
                    "sessions delete",
                    resolved.project_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "deleted_session_id": session_id,
                        "deleted": true
                    }),
                ))
            }
            Some("logs") => {
                let selector_raw = args.get(1).map(String::as_str).unwrap_or("latest");
                let selector = match selector_raw {
                    "latest" => ResumeSelector::Latest,
                    value if value.starts_with("sess_") => ResumeSelector::Exact(value.to_string()),
                    value => ResumeSelector::Prefix(value.to_string()),
                };
                let session = store.resolve_resume_target(selector).map_err(|err| {
                    session_store_failure(
                        "sessions logs",
                        err,
                        &resolved,
                        Some(selector_raw.to_string()),
                    )
                })?;
                let request_seq = parse_flag_value(args, "--request-seq")
                    .and_then(|value| value.parse::<usize>().ok());
                let kind = parse_flag_value(args, "--kind");
                if let Some(kind_value) = kind.as_deref() {
                    if !matches!(
                        kind_value,
                        "request" | "response_stream" | "response" | "tool_results"
                    ) {
                        return Err(CommandFailureOutcome::usage(
                            "sessions logs".to_string(),
                            "usage_invalid",
                            format!("invalid log kind: {kind_value}"),
                            Some(
                                "Choose one of: request, response_stream, response, tool_results."
                                    .to_string(),
                            ),
                        ));
                    }
                }
                let logs = store
                    .session_logs_filtered(
                        &session.session_id,
                        &resolved.project_id,
                        request_seq,
                        kind.as_deref(),
                    )
                    .map_err(|err| {
                        session_store_failure(
                            "sessions logs",
                            err,
                            &resolved,
                            Some(session.session_id.clone()),
                        )
                    })?;
                Ok(CommandSuccess::with_project_and_session(
                    "sessions logs",
                    resolved.project_id.clone(),
                    session.session_id.clone(),
                    serde_json::to_value(logs).expect("session logs should serialize"),
                ))
            }
            Some("export") => {
                let session_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "sessions export".to_string(),
                        "usage_invalid",
                        "missing session id for export".to_string(),
                        Some("Pass a concrete session id after `sessions export`.".to_string()),
                    )
                })?;
                let exported = store.export_session(session_id).map_err(|err| {
                    session_store_failure(
                        "sessions export",
                        err,
                        &resolved,
                        Some(session_id.clone()),
                    )
                })?;
                let logs = store
                    .session_logs(session_id, &resolved.project_id)
                    .map_err(|err| {
                        session_store_failure(
                            "sessions export",
                            err,
                            &resolved,
                            Some(session_id.clone()),
                        )
                    })?;
                Ok(CommandSuccess::with_project_and_session(
                    "sessions export",
                    resolved.project_id.clone(),
                    exported.session.session_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "session": exported.session,
                        "lineage": exported.lineage,
                        "transcript": exported.transcript,
                        "transcript_path": exported.transcript_path,
                        "operator_logs": logs.operator_logs,
                        "derived_read_models": logs.derived_read_models
                    }),
                ))
            }
            Some("stats") => {
                let stats = store
                    .stats()
                    .map_err(|err| session_store_failure("sessions stats", err, &resolved, None))?;
                Ok(CommandSuccess::with_project(
                    "sessions stats",
                    resolved.project_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "session_count": stats.session_count,
                        "active_count": stats.active_count,
                        "titled_count": stats.titled_count
                    }),
                ))
            }
            Some("prune") => {
                let apply = has_flag(args, "--apply");
                let result = store.prune_sessions(apply).map_err(|err| {
                    session_store_failure("sessions prune", err, &resolved, None)
                })?;
                if apply {
                    if let Some(active_session_id) = registry
                        .get_by_project_id(&resolved.project_id)
                        .map_err(|err| internal_failure("sessions prune", err.to_string()))?
                        .and_then(|entry| entry.active_session_id)
                    {
                        if result
                            .pruned_session_ids
                            .iter()
                            .any(|session_id| session_id == &active_session_id)
                        {
                            registry
                                .touch_project(&resolved.project_id, None)
                                .map_err(|err| {
                                    internal_failure("sessions prune", err.to_string())
                                })?;
                        }
                    }
                    publish_project_checkpoint(&resolved, &["session"])
                        .map_err(|err| internal_failure("sessions prune", err))?;
                }
                Ok(CommandSuccess::with_project(
                    "sessions prune",
                    resolved.project_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "dry_run": result.dry_run,
                        "candidate_session_ids": result.candidate_session_ids,
                        "pruned_session_ids": result.pruned_session_ids,
                        "blocking_conflicts": result.blocking_conflicts
                    }),
                ))
            }
            Some("search") => {
                let query = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "sessions search".to_string(),
                        "usage_invalid",
                        "missing search query".to_string(),
                        Some("Pass a search term after `sessions search`.".to_string()),
                    )
                })?;
                let hits = store.search_sessions(query).map_err(|err| {
                    session_store_failure("sessions search", err, &resolved, Some(query.clone()))
                })?;
                Ok(CommandSuccess::with_project(
                    "sessions search",
                    resolved.project_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "query": query,
                        "hits": hits
                    }),
                ))
            }
            Some("resume") => {
                let selector_raw = args.get(1).map(String::as_str).unwrap_or("latest");
                let selector = match selector_raw {
                    "latest" => ResumeSelector::Latest,
                    value if value.starts_with("sess_") => ResumeSelector::Exact(value.to_string()),
                    value => ResumeSelector::Prefix(value.to_string()),
                };
                let session = store.resolve_resume_target(selector).map_err(|err| {
                    session_store_failure(
                        "sessions resume",
                        err,
                        &resolved,
                        Some(selector_raw.to_string()),
                    )
                })?;
                registry
                    .touch_project(&resolved.project_id, Some(session.session_id.clone()))
                    .map_err(|err| internal_failure("sessions resume", err.to_string()))?;
                emit_session_resume_event(
                    &resolved,
                    "sessions resume",
                    &session.session_id,
                    json!({
                        "project_id": resolved.project_id,
                        "session_id": session.session_id
                    }),
                )
                .map_err(|err| internal_failure("sessions resume", err))?;
                publish_project_checkpoint(&resolved, &["session"])
                    .map_err(|err| internal_failure("sessions resume", err))?;
                Ok(CommandSuccess::with_project_and_session(
                    "sessions resume",
                    resolved.project_id.clone(),
                    session.session_id.clone(),
                    json!({
                        "project_id": resolved.project_id,
                        "session": session
                    }),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("sessions {other}"),
                "usage_invalid",
                format!("unknown sessions subcommand: {other}"),
                Some(
                    "Choose one of: create, list, browse, search, logs, export, stats, rename, delete, prune, resume."
                        .to_string(),
                ),
            )),
            None => Err(CommandFailureOutcome::usage(
                "sessions".to_string(),
                "usage_invalid",
                "missing sessions subcommand".to_string(),
                Some(
                    "Choose one of: create, list, browse, search, logs, export, stats, rename, delete, prune, resume."
                        .to_string(),
                ),
            )),
        }
    }

}
