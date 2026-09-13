impl Runtime {
    fn handle_agents(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("start") => {
                let runner = parse_flag_value(args, "--runner");
                let mock_requested =
                    args.iter().any(|arg| arg == "--mock") || runner.as_deref() == Some("mock");
                let local_requested = runner.as_deref() == Some("local");
                let provider_requested = runner.as_deref() == Some("provider");
                if !mock_requested && !local_requested && !provider_requested {
                    return Err(feature_not_graduated_failure(
                        "agents start",
                        "agents.start",
                        "M6.execution_runs",
                        vec![
                            "Use `research-cli agents start --mock ... --json` for the graduated proof harness."
                                .to_string(),
                            "Use `research-cli agents start --runner local --command <cmd> ... --json` for bounded local worker execution."
                                .to_string(),
                            "Graduate real worker spawning after packet-bound mock lifecycle proof."
                                .to_string(),
                        ],
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "agents")
                    .map_err(|err| project_resolution_failure("agents start", err))?;
                let result = if provider_requested {
                    let packet_ref = parse_flag_value(args, "--task-packet").ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "agents start".to_string(),
                            "usage_invalid",
                            "missing --task-packet".to_string(),
                            Some(
                                "Pass `--task-packet <path>` for provider worker execution."
                                    .to_string(),
                            ),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                    let provider_id = parse_flag_value(args, "--provider").ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "agents start".to_string(),
                            "usage_invalid",
                            "missing --provider".to_string(),
                            Some("Pass `--provider <provider-id>`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                    let model = parse_flag_value(args, "--model").ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "agents start".to_string(),
                            "usage_invalid",
                            "missing --model".to_string(),
                            Some("Pass `--model <model>`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                    let packet_path =
                        resolve_existing_task_packet_path(&resolved, &packet_ref).map_err(|err| {
                        CommandFailureOutcome::new(
                            9,
                            "agents start".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "agent_invalid_task_packet",
                            format!("task packet path is not readable: {err}"),
                            Some(
                                "Pass a workspace-relative or current project runtime --task-packet path that exists."
                                    .to_string(),
                            ),
                            false,
                            Some(json!({
                                "failure_code": "invalid_task_packet",
                                "task_packet_ref": packet_ref
                            })),
                            parsed.output_json,
                        )
                    })?;
                    agents::start_provider_from_task_packet_file(
                        &resolved.data_dir,
                        &resolved.workspace_root,
                        &packet_path,
                        &provider_id,
                        &model,
                    )
                } else if local_requested {
                    if let Some(packet_ref) = parse_flag_value(args, "--task-packet") {
                        let packet_path =
                            resolve_existing_task_packet_path(&resolved, &packet_ref).map_err(|err| {
                            CommandFailureOutcome::new(
                                9,
                                "agents start".to_string(),
                                Some(resolved.project_id.clone()),
                                None,
                                "agent_invalid_task_packet",
                                format!("task packet path is not readable: {err}"),
                                Some(
                                    "Pass a workspace-relative or current project runtime --task-packet path that exists."
                                        .to_string(),
                                ),
                                false,
                                Some(json!({
                                    "failure_code": "invalid_task_packet",
                                    "task_packet_ref": packet_ref
                                })),
                                parsed.output_json,
                            )
                        })?;
                        let packet =
                            agents::read_task_packet_file(&packet_path).map_err(|err| {
                                agent_start_failure(
                                    "agents start",
                                    err,
                                    &resolved,
                                    parsed.output_json,
                                )
                            })?;
                        agents::start_local_from_packet(
                            &resolved.data_dir,
                            &resolved.workspace_root,
                            packet,
                        )
                    } else {
                        let intent = parse_flag_value(args, "--intent").ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "agents start".to_string(),
                                "usage_invalid",
                                "missing --intent".to_string(),
                                Some("Pass `--intent <intent>` for the task packet.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let role_profile =
                            parse_flag_value(args, "--role-profile").ok_or_else(|| {
                                CommandFailureOutcome::usage(
                                    "agents start".to_string(),
                                    "usage_invalid",
                                    "missing --role-profile".to_string(),
                                    Some(
                                        "Pass `--role-profile <profile>` for the task packet."
                                            .to_string(),
                                    ),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let message = parse_flag_value(args, "--message").ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "agents start".to_string(),
                                "usage_invalid",
                                "missing --message".to_string(),
                                Some("Pass `--message <text>` for the task packet.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let command = parse_flag_value(args, "--command").ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "agents start".to_string(),
                                "usage_invalid",
                                "missing --command".to_string(),
                                Some(
                                    "Pass `--command <shell-command>` for the local runner."
                                        .to_string(),
                                ),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        agents::start_local(
                            &resolved.data_dir,
                            &resolved.workspace_root,
                            agents::LocalAgentStartRequest {
                                intent,
                                role_profile,
                                message,
                                command,
                                stage_task_contract: None,
                            },
                        )
                    }
                } else {
                    let intent = parse_flag_value(args, "--intent").ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "agents start".to_string(),
                            "usage_invalid",
                            "missing --intent".to_string(),
                            Some("Pass `--intent <intent>` for the task packet.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                    let role_profile =
                        parse_flag_value(args, "--role-profile").ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "agents start".to_string(),
                                "usage_invalid",
                                "missing --role-profile".to_string(),
                                Some(
                                    "Pass `--role-profile <profile>` for the task packet."
                                        .to_string(),
                                ),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                    let message = parse_flag_value(args, "--message").ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "agents start".to_string(),
                            "usage_invalid",
                            "missing --message".to_string(),
                            Some("Pass `--message <text>` for the task packet.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                    agents::start_mock(
                        &resolved.data_dir,
                        &resolved.workspace_root,
                        agents::AgentStartRequest {
                            intent,
                            role_profile,
                            message,
                            stage_task_contract: None,
                        },
                    )
                }
                .map_err(|err| {
                    agent_start_failure("agents start", err, &resolved, parsed.output_json)
                })?;
                publish_project_checkpoint(&resolved, &["agents", "m6"])
                    .map_err(|err| internal_failure("agents start", err))?;
                Ok(CommandSuccess::with_project(
                    "agents start",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("agents start should serialize"),
                ))
            }
            Some("list") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "agents")
                    .map_err(|err| project_resolution_failure("agents list", err))?;
                let result = agents::list(&resolved.data_dir)
                    .map_err(|err| internal_failure("agents list", err.to_string()))?;
                Ok(CommandSuccess::with_project(
                    "agents list",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("agents list should serialize"),
                ))
            }
            Some("inspect") => {
                let agent_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "agents inspect".to_string(),
                        "usage_invalid",
                        "missing agent id".to_string(),
                        Some("Pass an agent id after `agents inspect`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "agents")
                    .map_err(|err| project_resolution_failure("agents inspect", err))?;
                let result =
                    agents::inspect(&resolved.data_dir, agent_id).map_err(|err| match err {
                        agents::AgentError::UnknownAgent(_) => CommandFailureOutcome::new(
                            6,
                            "agents inspect".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "agent_not_found",
                            err.to_string(),
                            Some(
                                "Run `research-cli agents list --json` to inspect valid agent ids."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("agents inspect", other.to_string()),
                    })?;
                Ok(CommandSuccess::with_project(
                    "agents inspect",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("agents inspect should serialize"),
                ))
            }
            Some("stop") => {
                let agent_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "agents stop".to_string(),
                        "usage_invalid",
                        "missing agent id".to_string(),
                        Some("Pass an agent id after `agents stop`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "agents")
                    .map_err(|err| project_resolution_failure("agents stop", err))?;
                let result =
                    agents::stop(&resolved.data_dir, agent_id).map_err(|err| match err {
                        agents::AgentError::UnknownAgent(_) => CommandFailureOutcome::new(
                            6,
                            "agents stop".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "agent_not_found",
                            err.to_string(),
                            Some(
                                "Run `research-cli agents list --json` to inspect valid agent ids."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("agents stop", other.to_string()),
                    })?;
                Ok(CommandSuccess::with_project(
                    "agents stop",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("agents stop should serialize"),
                ))
            }
            Some("replay") => {
                let agent_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "agents replay".to_string(),
                        "usage_invalid",
                        "missing agent id".to_string(),
                        Some("Pass an agent id after `agents replay`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "agents")
                    .map_err(|err| project_resolution_failure("agents replay", err))?;
                let result = agents::replay(&resolved.data_dir, &resolved.workspace_root, agent_id)
                    .map_err(|err| match err {
                        agents::AgentError::UnknownAgent(_) => CommandFailureOutcome::new(
                            6,
                            "agents replay".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "agent_not_found",
                            err.to_string(),
                            Some(
                                "Run `research-cli agents list --json` to inspect valid agent ids."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => agent_start_failure(
                            "agents replay",
                            other,
                            &resolved,
                            parsed.output_json,
                        ),
                    })?;
                publish_project_checkpoint(&resolved, &["agents", "m6"])
                    .map_err(|err| internal_failure("agents replay", err))?;
                Ok(CommandSuccess::with_project(
                    "agents replay",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("agents replay should serialize"),
                ))
            }
            Some("traces") => {
                let agent_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "agents traces".to_string(),
                        "usage_invalid",
                        "missing agent id".to_string(),
                        Some("Pass an agent id after `agents traces`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "agents")
                    .map_err(|err| project_resolution_failure("agents traces", err))?;
                let result =
                    agents::traces(&resolved.data_dir, agent_id).map_err(|err| match err {
                        agents::AgentError::UnknownAgent(_) => CommandFailureOutcome::new(
                            6,
                            "agents traces".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "agent_not_found",
                            err.to_string(),
                            Some(
                                "Run `research-cli agents list --json` to inspect valid agent ids."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("agents traces", other.to_string()),
                    })?;
                Ok(CommandSuccess::with_project(
                    "agents traces",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("agents traces should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("agents {other}"),
                "usage_invalid",
                format!("unknown agents subcommand: {other}"),
                Some("Choose one of: start, list, inspect, stop, replay, traces.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "agents".to_string(),
                "usage_invalid",
                "missing agents subcommand".to_string(),
                Some("Choose one of: start, list, inspect, stop, replay, traces.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_artifacts(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("list") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "artifacts")
                    .map_err(|err| project_resolution_failure("artifacts list", err))?;
                let result = artifacts::list(
                    &resolved.data_dir,
                    parse_flag_value(args, "--family").as_deref(),
                    parse_flag_value(args, "--status").as_deref(),
                    has_flag(args, "--include-archive"),
                );
                Ok(CommandSuccess::with_project(
                    "artifacts list",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("artifacts list should serialize"),
                ))
            }
            Some("inspect") => {
                let target = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "artifacts inspect".to_string(),
                        "usage_invalid",
                        "missing artifact family or path".to_string(),
                        Some(
                            "Pass a family or artifact path after `artifacts inspect`.".to_string(),
                        ),
                    )
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "artifacts")
                    .map_err(|err| project_resolution_failure("artifacts inspect", err))?;
                let result =
                    artifacts::inspect(&resolved.data_dir, &resolved.workspace_root, target)
                        .map_err(|err| {
                            let (code, hint) = match err {
                        artifacts::ArtifactError::ScopeViolation(_) => (
                            "workspace_scope_violation",
                            "Pass a workspace-relative artifact path inside the project."
                                .to_string(),
                        ),
                        _ => (
                            "artifact_unknown",
                            "Run `research-cli artifacts list --json` to inspect valid families."
                                .to_string(),
                        ),
                    };
                            CommandFailureOutcome::new(
                                6,
                                "artifacts inspect".to_string(),
                                Some(resolved.project_id.clone()),
                                None,
                                code,
                                err.to_string(),
                                Some(hint),
                                false,
                                None,
                                parsed.output_json,
                            )
                        })?;
                Ok(CommandSuccess::with_project(
                    "artifacts inspect",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("artifacts inspect should serialize"),
                ))
            }
            Some("candidates") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "artifacts")
                    .map_err(|err| project_resolution_failure("artifacts candidates", err))?;
                let result = artifacts::promotion_candidates(&resolved.data_dir)
                    .map_err(|err| internal_failure("artifacts candidates", err.to_string()))?;
                Ok(CommandSuccess::with_project(
                    "artifacts candidates",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("artifact candidates should serialize"),
                ))
            }
            Some("promote") => {
                let candidate_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "artifacts promote".to_string(),
                        "usage_invalid",
                        "missing promotion candidate id".to_string(),
                        Some("Pass a candidate id after `artifacts promote`.".to_string()),
                    )
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "artifacts")
                    .map_err(|err| project_resolution_failure("artifacts promote", err))?;
                ensure_canonicality_promotion_gate(&resolved, parsed.output_json)?;
                let result = artifacts::promote_candidate(&resolved.data_dir, candidate_id)
                    .map_err(|err| match err {
                        artifacts::ArtifactError::UnknownCandidate(_)
                        | artifacts::ArtifactError::UnknownTarget(_) => CommandFailureOutcome::new(
                            6,
                            "artifacts promote".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "promotion_candidate_invalid",
                            err.to_string(),
                            Some(
                                "List candidates with `research-cli artifacts candidates --json`."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        artifacts::ArtifactError::ScopeViolation(_) => CommandFailureOutcome::new(
                            6,
                            "artifacts promote".to_string(),
                            Some(resolved.project_id.clone()),
                            None,
                            "workspace_scope_violation",
                            err.to_string(),
                            Some("Promotions only accept project-local artifacts.".to_string()),
                            false,
                            None,
                            parsed.output_json,
                        ),
                        other => internal_failure("artifacts promote", other.to_string()),
                    })?;
                Ok(CommandSuccess::with_project(
                    "artifacts promote",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("artifact promotion should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("artifacts {other}"),
                "usage_invalid",
                format!("unknown artifacts subcommand: {other}"),
                Some("Choose one of: list, inspect, candidates, promote.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "artifacts".to_string(),
                "usage_invalid",
                "missing artifacts subcommand".to_string(),
                Some("Choose one of: list, inspect, candidates, promote.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_branches(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("search") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches search")
                    .map_err(|err| {
                        project_resolution_failure("branches search", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let objective = parse_flag_value(args, "--objective").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches search".to_string(),
                        "usage_invalid",
                        "missing --objective".to_string(),
                        Some(
                            "Use `research-cli branches search --objective <text> [--max-branches <n>] [--strategy evolutionary] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let strategy = parse_flag_value(args, "--strategy")
                    .unwrap_or_else(|| "evolutionary".to_string());
                let max_branches = parse_optional_usize_flag(args, "--max-branches")
                    .map_err(|message| {
                        CommandFailureOutcome::usage(
                            "branches search".to_string(),
                            "usage_invalid",
                            message,
                            Some("--max-branches must be a positive integer.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?
                    .unwrap_or(3);
                let result = branches::search(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    &objective,
                    &strategy,
                    max_branches,
                )
                .map_err(|err| {
                    branch_failure("branches search", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "branches search",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches search should serialize"),
                ))
            }
            Some("list") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches list")
                    .map_err(|err| project_resolution_failure("branches list", err))?;
                let result = branches::list(&resolved.data_dir).map_err(|err| {
                    branch_failure("branches list", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "branches list",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches list should serialize"),
                ))
            }
            Some("inspect") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches inspect".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches inspect`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches inspect")
                    .map_err(|err| project_resolution_failure("branches inspect", err))?;
                let result = branches::inspect(&resolved.data_dir, branch_id).map_err(|err| {
                    branch_failure("branches inspect", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "branches inspect",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches inspect should serialize"),
                ))
            }
            Some("evaluate") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches evaluate".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches evaluate`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches evaluate")
                    .map_err(|err| project_resolution_failure("branches evaluate", err))?;
                let eval_commands = parse_flag_values(args, "--eval-command");
                let result = branches::evaluate(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    branch_id,
                    &eval_commands,
                )
                .map_err(|err| {
                    branch_failure("branches evaluate", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "branches evaluate",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches evaluate should serialize"),
                ))
            }
            Some("mutate") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches mutate".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches mutate`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let llm_command = parse_flag_value(args, "--llm-command").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches mutate".to_string(),
                        "usage_invalid",
                        "missing --llm-command".to_string(),
                        Some(
                            "Use `branches mutate <branch-id> --llm-command <cmd> --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches mutate")
                    .map_err(|err| project_resolution_failure("branches mutate", err))?;
                let result =
                    branches::mutate(&resolved.data_dir, branch_id, &llm_command).map_err(
                        |err| {
                            branch_failure(
                                "branches mutate",
                                err,
                                &resolved,
                                parsed.output_json,
                            )
                        },
                    )?;
                Ok(CommandSuccess::with_project(
                    "branches mutate",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches mutate should serialize"),
                ))
            }
            Some("debate") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches debate".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches debate`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let against = parse_flag_value(args, "--against").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches debate".to_string(),
                        "usage_invalid",
                        "missing --against".to_string(),
                        Some(
                            "Use `branches debate <branch-id> --against <branch-id>`.".to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches debate")
                    .map_err(|err| project_resolution_failure("branches debate", err))?;
                let result =
                    branches::debate(&resolved.data_dir, branch_id, &against).map_err(|err| {
                        branch_failure("branches debate", err, &resolved, parsed.output_json)
                    })?;
                Ok(CommandSuccess::with_project(
                    "branches debate",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches debate should serialize"),
                ))
            }
            Some("verify") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches verify".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches verify`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let against = parse_flag_value(args, "--against").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches verify".to_string(),
                        "usage_invalid",
                        "missing --against".to_string(),
                        Some(
                            "Use `branches verify <branch-id> --against <branch-id>`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let repetitions = parse_flag_value(args, "--repetitions")
                    .map(|value| {
                        value.parse::<usize>().map_err(|_| {
                            CommandFailureOutcome::usage(
                                "branches verify".to_string(),
                                "usage_invalid",
                                "invalid --repetitions".to_string(),
                                Some("--repetitions must be a positive integer.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })
                    })
                    .transpose()?
                    .unwrap_or(3);
                let criteria = parse_flag_values(args, "--criterion");
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches verify")
                    .map_err(|err| project_resolution_failure("branches verify", err))?;
                let result = branches::verify(
                    &resolved.data_dir,
                    branch_id,
                    &against,
                    repetitions,
                    &criteria,
                )
                .map_err(|err| {
                    branch_failure("branches verify", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "branches verify",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches verify should serialize"),
                ))
            }
            Some("promote") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches promote".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches promote`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches promote")
                    .map_err(|err| project_resolution_failure("branches promote", err))?;
                let result = branches::promote(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    branch_id,
                    !has_flag(args, "--no-merge"),
                    has_flag(args, "--require-verifier"),
                )
                .map_err(|err| {
                    branch_failure("branches promote", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "branches promote",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches promote should serialize"),
                ))
            }
            Some("archive") => {
                let branch_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches archive".to_string(),
                        "usage_invalid",
                        "missing branch id".to_string(),
                        Some("Pass a branch id after `branches archive`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let reason = parse_flag_value(args, "--reason").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "branches archive".to_string(),
                        "usage_invalid",
                        "missing --reason".to_string(),
                        Some("Use `branches archive <branch-id> --reason <text>`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "branches archive")
                    .map_err(|err| project_resolution_failure("branches archive", err))?;
                let result =
                    branches::archive(&resolved.data_dir, branch_id, &reason).map_err(|err| {
                        branch_failure("branches archive", err, &resolved, parsed.output_json)
                    })?;
                Ok(CommandSuccess::with_project(
                    "branches archive",
                    resolved.project_id,
                    serde_json::to_value(result).expect("branches archive should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("branches {other}"),
                "usage_invalid",
                format!("unknown branches subcommand: {other}"),
                Some(
                    "Choose one of: search, list, inspect, mutate, evaluate, debate, promote, archive."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "branches".to_string(),
                "usage_invalid",
                "missing branches subcommand".to_string(),
                Some(
                    "Choose one of: search, list, inspect, mutate, evaluate, debate, promote, archive."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_research(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("run") => {
                let request = parse_autonomous_research_run_request(&args[1..]).map_err(
                    |message| {
                        CommandFailureOutcome::usage(
                            "research run".to_string(),
                            "usage_invalid",
                            message,
                            Some("Use `astra research run --prompt <goal> [--workflow research-pipeline|system-validation] --automation-mode <human_in_the_loop|high_autonomy|full_auto> [--stage-task-semantic-review provider] [--background|--max-runtime-ms <ms>|--duration-hours <n>] [--max-ticks <n>] [--interval-ms <ms>] [--report <path>] --json`. Provider-backed LLM review is the default; local is a hidden offline diagnostic mode.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    },
                )?;
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "research run")
                    .map_err(|err| {
                        project_resolution_failure("research run", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result = start_autonomous_research_job(&resolved, &request, parsed.output_json)
                    .map_err(|err| err.with_output_json(parsed.output_json))?;
                let text = format!(
                    "research run job: {}\nproject_id: {}\njob_id: {}\nstate: {}\npid: {}",
                    result.status,
                    result.project_id,
                    result.job_id,
                    result.job_state_path,
                    result
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "foreground".to_string())
                );
                Ok(CommandSuccess::with_project(
                    "research run",
                    resolved.project_id.clone(),
                    serde_json::to_value(result).expect("research job result should serialize"),
                )
                .with_text_override(text))
            }
            Some("jobs") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research jobs")
                    .map_err(|err| {
                        project_resolution_failure("research jobs", err)
                            .with_output_json(parsed.output_json)
                    })?;
                match args.get(1).map(String::as_str) {
                    Some("list") | Some("status") => {
                        let result =
                            list_autonomous_research_jobs(&resolved).map_err(|err| {
                                internal_failure("research jobs status", err)
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "research jobs status",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("research jobs status should serialize"),
                        ))
                    }
                    Some("tick") => {
                        let job_id = args.get(2).ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "research jobs tick".to_string(),
                                "usage_invalid",
                                "missing job id".to_string(),
                                Some("Use `astra research jobs tick <job-id> --json`.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let result =
                            tick_autonomous_research_job(&resolved, job_id, parsed.output_json)
                                .map_err(|err| err.with_output_json(parsed.output_json))?;
                        Ok(CommandSuccess::with_project(
                            "research jobs tick",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("research jobs tick should serialize"),
                        ))
                    }
                    Some("resume") => {
                        let request = parse_autonomous_research_resume_request(args).map_err(
                            |err| {
                                CommandFailureOutcome::usage(
                                    "research jobs resume".to_string(),
                                    "usage_invalid",
                                    err,
                                    Some(
                                        "Use `astra research jobs resume <job-id> [--background] [--additional-ticks <n>|--max-ticks <n>] [--duration-hours <n>|--duration-minutes <n>|--max-runtime-ms <ms>] --json`."
                                            .to_string(),
                                    ),
                                )
                                .with_output_json(parsed.output_json)
                            },
                        )?;
                        if request.job_id.trim().is_empty() {
                            return Err(CommandFailureOutcome::usage(
                                "research jobs resume".to_string(),
                                "usage_invalid",
                                "missing job id".to_string(),
                                Some(
                                    "Use `astra research jobs resume <job-id> [--background] --json`."
                                        .to_string(),
                                ),
                            )
                            .with_output_json(parsed.output_json));
                        }
                        let result =
                            resume_autonomous_research_job(&resolved, &request, parsed.output_json)
                                .map_err(|err| err.with_output_json(parsed.output_json))?;
                        Ok(CommandSuccess::with_project(
                            "research jobs resume",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("research jobs resume should serialize"),
                        ))
                    }
                    Some("tail") => {
                        let job_id = args.get(2).ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "research jobs tail".to_string(),
                                "usage_invalid",
                                "missing job id".to_string(),
                                Some("Use `astra research jobs tail <job-id> --json`.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let limit = parse_optional_usize_flag(&args[3..], "--limit")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "research jobs tail".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some(
                                        "Use `astra research jobs tail <job-id> [--limit <n>] --json`."
                                            .to_string(),
                                    ),
                                )
                                .with_output_json(parsed.output_json)
                            })?
                            .unwrap_or(40);
                        let result =
                            tail_autonomous_research_job(&resolved, job_id, limit).map_err(
                                |err| {
                                    internal_failure("research jobs tail", err)
                                        .with_output_json(parsed.output_json)
                                },
                            )?;
                        Ok(CommandSuccess::with_project(
                            "research jobs tail",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("research jobs tail should serialize"),
                        ))
                    }
                    Some("stop") => {
                        let job_id = args.get(2).ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "research jobs stop".to_string(),
                                "usage_invalid",
                                "missing job id".to_string(),
                                Some("Use `astra research jobs stop <job-id> --json`.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let job = stop_autonomous_research_job(&resolved, job_id).map_err(|err| {
                            internal_failure("research jobs stop", err)
                                .with_output_json(parsed.output_json)
                        })?;
                        Ok(CommandSuccess::with_project(
                            "research jobs stop",
                            resolved.project_id,
                            serde_json::to_value(job)
                                .expect("research jobs stop should serialize"),
                        ))
                    }
                    Some(other) => Err(CommandFailureOutcome::usage(
                        format!("research jobs {other}"),
                        "usage_invalid",
                        format!("unknown research jobs subcommand: {other}"),
                        Some(
                            "Choose one of: status, list, tick, resume, tail, stop.".to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)),
                    None => Err(CommandFailureOutcome::usage(
                        "research jobs".to_string(),
                        "usage_invalid",
                        "missing research jobs subcommand".to_string(),
                        Some(
                            "Choose one of: status, list, tick, resume, tail, stop.".to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)),
                }
            }
            Some("status") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research status")
                    .map_err(|err| {
                        project_resolution_failure("research status", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result = research::status(&resolved.data_dir, &resolved.project_id).map_err(
                    |err| research_failure("research status", err, &resolved, parsed.output_json),
                )?;
                Ok(CommandSuccess::with_project(
                    "research status",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research status should serialize"),
                ))
            }
            Some("board") => {
                if has_unrecognized_positionals(&args[1..], &[]) {
                    return Err(CommandFailureOutcome::usage(
                        "research board".to_string(),
                        "usage_invalid",
                        "research board does not accept positional arguments".to_string(),
                        Some("Use `research-cli research board --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research board")
                    .map_err(|err| {
                        project_resolution_failure("research board", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let state_home =
                    state_home().map_err(|err| internal_failure("research board", err))?;
                let projection = host_surface::projection(
                    &state_home,
                    &resolved,
                    active_session_id_for(registry, &resolved, "research board")?,
                )
                .map_err(|err| {
                    host_surface_failure("research board", &resolved, err, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "research board",
                    resolved.project_id,
                    serde_json::to_value(projection.research.board)
                        .expect("research board should serialize"),
                ))
            }
            Some("record") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research record")
                    .map_err(|err| {
                        project_resolution_failure("research record", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_research_record_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "research record".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `research-cli research record --kind <kind> --title <title> --stage <stage> [--decision <text>] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = research::record(&resolved.data_dir, &resolved.project_id, request)
                    .map_err(|err| {
                        research_failure("research record", err, &resolved, parsed.output_json)
                    })?;
                Ok(CommandSuccess::with_project(
                    "research record",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research record should serialize"),
                ))
            }
            Some("decide") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research decide")
                    .map_err(|err| {
                        project_resolution_failure("research decide", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_research_decision_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "research decide".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `research-cli research decide --thread <id> --operation <op> --decision approve|reject|revise --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = research::decide(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    request,
                )
                .map_err(|err| {
                    research_failure("research decide", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "research decide",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research decision should serialize"),
                ))
            }
            Some("classify") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research classify")
                    .map_err(|err| {
                        project_resolution_failure("research classify", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_research_classify_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "research classify".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `research-cli research classify --text <turn> --dry-run --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = research::classify(&resolved.data_dir, request).map_err(|err| {
                    research_failure("research classify", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "research classify",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research classification should serialize"),
                ))
            }
            Some("middleware") if args.get(1).map(String::as_str) == Some("plan") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research middleware plan")
                    .map_err(|err| {
                        project_resolution_failure("research middleware plan", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request =
                    parse_research_middleware_plan_request(&args[2..]).map_err(|message| {
                        CommandFailureOutcome::usage(
                            "research middleware plan".to_string(),
                            "usage_invalid",
                            message,
                            Some(
                                "Use `research-cli research middleware plan --role <role> --intent <text> [--candidate-tool <tool>] --json`."
                                    .to_string(),
                            ),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let result = research::plan_middleware(&resolved.data_dir, request).map_err(
                    |err| {
                        research_failure(
                            "research middleware plan",
                            err,
                            &resolved,
                            parsed.output_json,
                        )
                    },
                )?;
                Ok(CommandSuccess::with_project(
                    "research middleware plan",
                    resolved.project_id,
                    serde_json::to_value(result)
                        .expect("research middleware result should serialize"),
                ))
            }
            Some("threads") if args.get(1).map(String::as_str) == Some("list") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research threads list")
                    .map_err(|err| {
                        project_resolution_failure("research threads list", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result = research::list_threads(&resolved.data_dir).map_err(|err| {
                    research_failure("research threads list", err, &resolved, parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "research threads list",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research thread list should serialize"),
                ))
            }
            Some("thread") if args.get(1).map(String::as_str) == Some("inspect") => {
                let thread_id = args.get(2).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "research thread inspect".to_string(),
                        "usage_invalid",
                        "missing thread id".to_string(),
                        Some("Pass a thread id after `research thread inspect`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research thread inspect")
                    .map_err(|err| {
                        project_resolution_failure("research thread inspect", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result =
                    research::inspect_thread(&resolved.data_dir, thread_id).map_err(|err| {
                        research_failure(
                            "research thread inspect",
                            err,
                            &resolved,
                            parsed.output_json,
                        )
                    })?;
                Ok(CommandSuccess::with_project(
                    "research thread inspect",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research thread inspect should serialize"),
                ))
            }
            Some("stage") if args.get(1).map(String::as_str) == Some("inspect") => {
                let execution_id = args.get(2).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "research stage inspect".to_string(),
                        "usage_invalid",
                        "missing stage execution id".to_string(),
                        Some("Pass an execution id after `research stage inspect`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research stage inspect")
                    .map_err(|err| {
                        project_resolution_failure("research stage inspect", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result =
                    research::inspect_stage(&resolved.data_dir, execution_id).map_err(|err| {
                        research_failure(
                            "research stage inspect",
                            err,
                            &resolved,
                            parsed.output_json,
                        )
                    })?;
                Ok(CommandSuccess::with_project(
                    "research stage inspect",
                    resolved.project_id,
                    serde_json::to_value(result).expect("research stage inspect should serialize"),
                ))
            }
            Some("stage") if args.get(1).map(String::as_str) == Some("map") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "research stage map")
                    .map_err(|err| {
                        project_resolution_failure("research stage map", err)
                            .with_output_json(parsed.output_json)
                    })?;
                Ok(CommandSuccess::with_project(
                    "research stage map",
                    resolved.project_id,
                    serde_json::to_value(research::stage_execution_map())
                        .expect("research stage map should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("research {other}"),
                "usage_invalid",
                format!("unknown research subcommand: {other}"),
                Some("Choose one of: run, jobs, status, board, record, decide, classify, threads list, thread inspect, stage inspect, stage map.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "research".to_string(),
                "usage_invalid",
                "missing research subcommand".to_string(),
                Some("Choose one of: run, jobs, status, board, record, decide, classify, threads list, thread inspect, stage inspect, stage map.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

}

fn resolve_existing_task_packet_path(
    resolved: &ResolvedProject,
    packet_ref: &str,
) -> Result<PathBuf, String> {
    let packet_path = Path::new(packet_ref);
    if !packet_path.is_absolute() {
        return crate::workspace::path::resolve_existing_workspace_path(
            &resolved.workspace_root,
            packet_ref,
        )
        .map_err(|err| err.to_string());
    }

    let data_dir = resolved.data_dir.canonicalize().map_err(|err| {
        format!(
            "project runtime data dir is not readable: {}: {err}",
            resolved.data_dir.display()
        )
    })?;
    let resolved_packet = packet_path
        .canonicalize()
        .map_err(|err| format!("task packet path does not exist: {}: {err}", packet_path.display()))?;
    if !resolved_packet.starts_with(&data_dir) {
        return Err(format!(
            "absolute paths must be inside the current project runtime data dir: {}",
            packet_path.display()
        ));
    }

    Ok(resolved_packet)
}
