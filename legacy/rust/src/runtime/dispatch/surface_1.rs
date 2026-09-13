impl Runtime {
    fn handle_setup(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let state_home = state_home().map_err(|err| internal_failure("setup", err))?;
        let scoped_cwd = command_cwd(cwd, parsed);
        match args.first().map(String::as_str) {
            Some("status") => {
                let report = crate::setup::status(&state_home, scoped_cwd);
                emit_setup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "setup status",
                    serde_json::to_value(&report).expect("setup status should serialize"),
                )
                .map_err(|err| internal_failure("setup status", err))?;
                Ok(CommandSuccess::new_with_data(
                    "setup status",
                    serde_json::to_value(report).expect("setup status should serialize"),
                ))
            }
            Some("migrate-check") => {
                let report = crate::setup::migrate_check(&state_home, scoped_cwd);
                emit_setup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "setup migrate-check",
                    serde_json::to_value(&report).expect("migrate-check should serialize"),
                )
                .map_err(|err| internal_failure("setup migrate-check", err))?;
                Ok(CommandSuccess::new_with_data(
                    "setup migrate-check",
                    serde_json::to_value(report).expect("migrate-check should serialize"),
                ))
            }
            Some("repair-hints") => {
                let doctor = DoctorService::run(
                    registry,
                    scoped_cwd,
                    parsed.project.as_deref(),
                    parsed.cwd.as_deref(),
                );
                let mut components = Vec::new();
                components.push(RepairHintComponent {
                    component: "workspace".to_string(),
                    status: doctor.workspace.status.clone(),
                    hints: if doctor.workspace.status == "ready" {
                        Vec::new()
                    } else {
                        vec![doctor.workspace.detail.clone()]
                    },
                });
                components.push(RepairHintComponent {
                    component: "project".to_string(),
                    status: doctor.project.status.clone(),
                    hints: if doctor.project.status == "ready" {
                        Vec::new()
                    } else {
                        vec![doctor.project.detail.clone()]
                    },
                });
                components.push(RepairHintComponent {
                    component: "providers".to_string(),
                    status: if doctor.providers.iter().all(|check| check.status == "ready") {
                        "ready".to_string()
                    } else {
                        "degraded".to_string()
                    },
                    hints: doctor.repair_hints.clone(),
                });
                components.push(RepairHintComponent {
                    component: "plugins".to_string(),
                    status: if doctor.plugins.iter().all(|check| check.status == "ready") {
                        "ready".to_string()
                    } else {
                        "degraded".to_string()
                    },
                    hints: doctor
                        .plugins
                        .iter()
                        .filter(|check| check.status != "ready")
                        .map(|check| check.detail.clone())
                        .collect(),
                });
                components.push(RepairHintComponent {
                    component: "mcp".to_string(),
                    status: if doctor.mcp.iter().all(|check| check.status == "ready") {
                        "ready".to_string()
                    } else {
                        "degraded".to_string()
                    },
                    hints: doctor
                        .mcp
                        .iter()
                        .filter(|check| check.status != "ready")
                        .map(|check| check.detail.clone())
                        .collect(),
                });
                let report = crate::setup::repair_hints(components);
                emit_setup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "setup repair-hints",
                    serde_json::to_value(&report).expect("repair-hints should serialize"),
                )
                .map_err(|err| internal_failure("setup repair-hints", err))?;
                Ok(CommandSuccess::new_with_data(
                    "setup repair-hints",
                    serde_json::to_value(report).expect("repair-hints should serialize"),
                ))
            }
            Some("install-routes") => {
                let report = crate::setup::install_routes(&state_home, scoped_cwd);
                emit_setup_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    "setup install-routes",
                    serde_json::to_value(&report).expect("install-routes should serialize"),
                )
                .map_err(|err| internal_failure("setup install-routes", err))?;
                Ok(CommandSuccess::new_with_data(
                    "setup install-routes",
                    serde_json::to_value(report).expect("install-routes should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("setup {other}"),
                "usage_invalid",
                format!("unknown setup subcommand: {other}"),
                Some(
                    "Choose one of: status, migrate-check, repair-hints, install-routes."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "setup".to_string(),
                "usage_invalid",
                "missing setup subcommand".to_string(),
                Some(
                    "Choose one of: status, migrate-check, repair-hints, install-routes."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_mcp(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let state_home = state_home().map_err(|err| internal_failure("mcp", err))?;
        let snapshot = mcp::discover(&state_home, command_cwd(cwd, parsed));
        match args.first().map(String::as_str) {
            Some("list") => Ok(CommandSuccess::new_with_data(
                "mcp list",
                serde_json::to_value(mcp::list(&snapshot)).expect("mcp list should serialize"),
            )),
            Some("inspect") => {
                let server_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "mcp inspect".to_string(),
                        "usage_invalid",
                        "missing server id".to_string(),
                        Some("Pass a server id after `mcp inspect`.".to_string()),
                    )
                })?;
                let inspected = mcp::inspect(&snapshot, server_id).map_err(|err| {
                    CommandFailureOutcome::new(
                        6,
                        "mcp inspect".to_string(),
                        None,
                        None,
                        "mcp_server_unknown",
                        err.to_string(),
                        Some(
                            "Run `research-cli mcp list --json` to inspect known servers."
                                .to_string(),
                        ),
                        false,
                        None,
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::new_with_data(
                    "mcp inspect",
                    serde_json::to_value(inspected).expect("mcp inspect should serialize"),
                ))
            }
            Some("test") => {
                let result =
                    mcp::test(&snapshot, args.get(1).map(String::as_str)).map_err(|err| {
                        CommandFailureOutcome::new(
                            6,
                            "mcp test".to_string(),
                            None,
                            None,
                            "mcp_server_unknown",
                            err.to_string(),
                            Some(
                                "Run `research-cli mcp list --json` to inspect known servers."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        )
                    })?;
                if result.overall_status != "ready" {
                    return Err(CommandFailureOutcome::new(
                        11,
                        "mcp test".to_string(),
                        None,
                        None,
                        "followup_required",
                        "one or more MCP servers are degraded".to_string(),
                        Some(
                            "Inspect the MCP test result and repair degraded startup lanes."
                                .to_string(),
                        ),
                        false,
                        Some(serde_json::to_value(result).expect("mcp test should serialize")),
                        parsed.output_json,
                    ));
                }
                Ok(CommandSuccess::new_with_data(
                    "mcp test",
                    serde_json::to_value(result).expect("mcp test should serialize"),
                ))
            }
            Some("refresh") => {
                let result = mcp::refresh(&snapshot);
                emit_mcp_registry_event_if_possible(
                    registry,
                    cwd,
                    parsed,
                    serde_json::to_value(&result).expect("mcp refresh should serialize"),
                )
                .map_err(|err| internal_failure("mcp refresh", err))?;
                Ok(CommandSuccess::new_with_data(
                    "mcp refresh",
                    serde_json::to_value(result).expect("mcp refresh should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("mcp {other}"),
                "usage_invalid",
                format!("unknown mcp subcommand: {other}"),
                Some("Choose one of: list, inspect, test, refresh.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "mcp".to_string(),
                "usage_invalid",
                "missing mcp subcommand".to_string(),
                Some("Choose one of: list, inspect, test, refresh.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_skills(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let state_home = state_home().map_err(|err| internal_failure("skills", err))?;
        let snapshot = skills::discover(&state_home, command_cwd(cwd, parsed));
        match args.first().map(String::as_str) {
            Some("list") => {
                let filter = parse_skill_list_filter(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "skills list".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `skills list [--stage <stage>] [--include-disabled] [--source <path>] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::new_with_data(
                    "skills list",
                    serde_json::to_value(skills::list_with_filter(&snapshot, &filter))
                        .expect("skills list should serialize"),
                ))
            }
            Some("inspect") => {
                let skill_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills inspect".to_string(),
                        "usage_invalid",
                        "missing skill id".to_string(),
                        Some("Pass a skill id after `skills inspect`.".to_string()),
                    )
                })?;
                let inspected = skills::inspect(&snapshot, skill_id).map_err(|err| {
                    CommandFailureOutcome::new(
                        6,
                        "skills inspect".to_string(),
                        None,
                        None,
                        "skill_unknown",
                        err.to_string(),
                        Some(
                            "Run `research-cli skills list --json` to inspect known skills."
                                .to_string(),
                        ),
                        false,
                        None,
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::new_with_data(
                    "skills inspect",
                    serde_json::to_value(inspected).expect("skills inspect should serialize"),
                ))
            }
            Some("paths") => Ok(CommandSuccess::new_with_data(
                "skills paths",
                serde_json::to_value(skills::paths(&snapshot))
                    .expect("skills paths should serialize"),
            )),
            Some("validate") => {
                let result = skills::validate(&snapshot, args.get(1).map(String::as_str)).map_err(
                    |err| {
                        CommandFailureOutcome::new(
                            6,
                            "skills validate".to_string(),
                            None,
                            None,
                            "skill_unknown",
                            err.to_string(),
                            Some(
                                "Run `research-cli skills list --json` to inspect known skills."
                                    .to_string(),
                            ),
                            false,
                            None,
                            parsed.output_json,
                        )
                    },
                )?;
                if result.overall_status != "ready" {
                    return Err(CommandFailureOutcome::new(
                        11,
                        "skills validate".to_string(),
                        None,
                        None,
                        "followup_required",
                        "one or more skills are degraded".to_string(),
                        Some(
                            "Inspect the validation issues and repair degraded skill manifests."
                                .to_string(),
                        ),
                        false,
                        Some(
                            serde_json::to_value(result).expect("skills validate should serialize"),
                        ),
                        parsed.output_json,
                    ));
                }
                Ok(CommandSuccess::new_with_data(
                    "skills validate",
                    serde_json::to_value(result).expect("skills validate should serialize"),
                ))
            }
            Some("outputs") if args.get(1).map(String::as_str) == Some("list") => {
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "skills outputs list")
                    .map_err(|err| {
                        project_resolution_failure("skills outputs list", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result = skills::list_outputs(&resolved.data_dir).map_err(|err| {
                    skill_output_failure(
                        "skills outputs list",
                        err,
                        Some(resolved.project_id.clone()),
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::with_project(
                    "skills outputs list",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skills outputs list should serialize"),
                ))
            }
            Some("output") if args.get(1).map(String::as_str) == Some("submit") => {
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "skills output submit")
                    .map_err(|err| {
                        project_resolution_failure("skills output submit", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_skill_output_submit_request(&args[2..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "skills output submit".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `skills output submit --skill <id> --kind <kind> --artifact <path> --family <family> [--doc-frame <path>] [--policy <policy>] [--human-gate] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = skills::submit_output(
                    &snapshot,
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    request,
                )
                .map_err(|err| {
                    skill_output_failure(
                        "skills output submit",
                        err,
                        Some(resolved.project_id.clone()),
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::with_project(
                    "skills output submit",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skills output submit should serialize"),
                ))
            }
            Some("output") if args.get(1).map(String::as_str) == Some("inspect") => {
                let envelope_id = args.get(2).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills output inspect".to_string(),
                        "usage_invalid",
                        "missing envelope id".to_string(),
                        Some("Use `skills output inspect <envelope-id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "skills output inspect")
                    .map_err(|err| {
                        project_resolution_failure("skills output inspect", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result = skills::inspect_output(&resolved.data_dir, envelope_id).map_err(
                    |err| {
                        skill_output_failure(
                            "skills output inspect",
                            err,
                            Some(resolved.project_id.clone()),
                            parsed.output_json,
                        )
                    },
                )?;
                Ok(CommandSuccess::with_project(
                    "skills output inspect",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skills output inspect should serialize"),
                ))
            }
            Some("run") => {
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "skills run")
                    .map_err(|err| {
                        project_resolution_failure("skills run", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_skill_run_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "skills run".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `skills run --skill <id> --adapter local-command --command <cmd> --kind <kind> --artifact <path> --family <family> [--doc-frame <path>] [--policy <policy>] [--human-gate] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = skills::run_skill(
                    &snapshot,
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    request,
                )
                .map_err(|err| {
                    skill_output_failure(
                        "skills run",
                        err,
                        Some(resolved.project_id.clone()),
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::with_project(
                    "skills run",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skills run should serialize"),
                ))
            }
            Some("publish") => {
                let execute = args.iter().any(|arg| arg == "--execute");
                let inspect = args.iter().any(|arg| arg == "--inspect");
                if !execute && !inspect {
                    return Err(CommandFailureOutcome::usage(
                        "skills publish".to_string(),
                        "usage_invalid",
                        "missing --execute or --inspect".to_string(),
                        Some("Use `skills publish --execute <envelope-id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json));
                }
                let mode_index = args
                    .iter()
                    .position(|arg| arg == "--execute" || arg == "--inspect")
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "skills publish".to_string(),
                            "usage_invalid",
                            "missing publication mode".to_string(),
                            Some("Use `skills publish --execute <envelope-id> --json`.".to_string()),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let envelope_id = args.get(mode_index + 1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills publish".to_string(),
                        "usage_invalid",
                        "missing envelope id".to_string(),
                        Some("Use `skills publish --execute <envelope-id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let resolved = self
                    .resolve_project_or_workspace(registry, cwd, parsed, "skills publish")
                    .map_err(|err| {
                        project_resolution_failure("skills publish", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let result = skills::publish_output(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    &resolved.project_id,
                    envelope_id,
                    skills::SkillPublishRequest {
                        execute,
                        human_gate_approved: args.iter().any(|arg| arg == "--approve-human-gate"),
                    },
                )
                .map_err(|err| {
                        skill_output_failure(
                            "skills publish",
                            err,
                            Some(resolved.project_id.clone()),
                            parsed.output_json,
                        )
                    })?;
                Ok(CommandSuccess::with_project(
                    "skills publish",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skills publish should serialize"),
                ))
            }
            Some("evolve") => self.handle_skill_evolution(registry, cwd, parsed, &args[1..]),
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("skills {other}"),
                "usage_invalid",
                format!("unknown skills subcommand: {other}"),
                Some(
                    "Choose one of: list, inspect, paths, validate, outputs list, output submit, output inspect, run, publish, evolve."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "skills".to_string(),
                "usage_invalid",
                "missing skills subcommand".to_string(),
                Some(
                    "Choose one of: list, inspect, paths, validate, outputs list, output submit, output inspect, run, publish, evolve."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_skill_evolution(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project_or_workspace(registry, cwd, parsed, "skills evolve")
            .map_err(|err| {
                project_resolution_failure("skills evolve", err)
                    .with_output_json(parsed.output_json)
            })?;
        match args.first().map(String::as_str) {
            Some("cluster") => {
                let min_members = parse_optional_usize_flag(&args[1..], "--min-members")
                    .map_err(|message| {
                        CommandFailureOutcome::usage(
                            "skills evolve cluster".to_string(),
                            "usage_invalid",
                            message,
                            Some(
                                "Use `skills evolve cluster [--min-members <n>] --json`."
                                    .to_string(),
                            ),
                        )
                        .with_output_json(parsed.output_json)
                    })?
                    .unwrap_or(2);
                reject_unknown_flags(
                    "skills evolve cluster",
                    &args[1..],
                    &["--min-members", "--json"],
                    parsed.output_json,
                )?;
                let result =
                    evolution::cluster(&resolved.data_dir, &resolved.project_id, min_members)
                        .map_err(|err| {
                            internal_failure("skills evolve cluster", err.to_string())
                                .with_output_json(parsed.output_json)
                        })?;
                Ok(CommandSuccess::with_project(
                    "skills evolve cluster",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skill clusters should serialize"),
                ))
            }
            Some("crystallize") => {
                let cluster_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills evolve crystallize".to_string(),
                        "usage_invalid",
                        "missing cluster id".to_string(),
                        Some("Use `skills evolve crystallize <cluster-id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                reject_unknown_positionals_after_first(
                    "skills evolve crystallize",
                    &args[1..],
                    parsed.output_json,
                )?;
                let result = evolution::crystallize(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    cluster_id,
                )
                .map_err(|err| {
                    internal_failure("skills evolve crystallize", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "skills evolve crystallize",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skill candidate should serialize"),
                ))
            }
            Some("verify") => {
                let candidate_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills evolve verify".to_string(),
                        "usage_invalid",
                        "missing candidate id".to_string(),
                        Some("Use `skills evolve verify <candidate-id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                reject_unknown_positionals_after_first(
                    "skills evolve verify",
                    &args[1..],
                    parsed.output_json,
                )?;
                let result =
                    evolution::verify(&resolved.data_dir, candidate_id).map_err(|err| {
                        internal_failure("skills evolve verify", err.to_string())
                            .with_output_json(parsed.output_json)
                    })?;
                Ok(CommandSuccess::with_project(
                    "skills evolve verify",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skill verification should serialize"),
                ))
            }
            Some("submit") => {
                let candidate_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills evolve submit".to_string(),
                        "usage_invalid",
                        "missing candidate id".to_string(),
                        Some("Use `skills evolve submit <candidate-id> --json`.".to_string()),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                reject_unknown_positionals_after_first(
                    "skills evolve submit",
                    &args[1..],
                    parsed.output_json,
                )?;
                let request =
                    evolution::submit_request(&resolved.data_dir, candidate_id).map_err(|err| {
                        internal_failure("skills evolve submit", err.to_string())
                            .with_output_json(parsed.output_json)
                    })?;
                let result = skills::submit_native_output(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    request,
                )
                .map_err(|err| {
                    skill_output_failure(
                        "skills evolve submit",
                        err,
                        Some(resolved.project_id.clone()),
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::with_project(
                    "skills evolve submit",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skill submit should serialize"),
                ))
            }
            Some("install") => {
                let candidate_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "skills evolve install".to_string(),
                        "usage_invalid",
                        "missing candidate id".to_string(),
                        Some(
                            "Use `skills evolve install <candidate-id> --approve-human-gate --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                reject_unknown_positionals_and_flags_after_first(
                    "skills evolve install",
                    &args[1..],
                    &["--approve-human-gate", "--json"],
                    parsed.output_json,
                )?;
                let result = evolution::install(
                    &resolved.data_dir,
                    &resolved.workspace_root,
                    candidate_id,
                    args.iter().any(|arg| arg == "--approve-human-gate"),
                )
                .map_err(|err| {
                    evolution_failure(
                        "skills evolve install",
                        err,
                        Some(resolved.project_id.clone()),
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::with_project(
                    "skills evolve install",
                    resolved.project_id,
                    serde_json::to_value(result).expect("skill install should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("skills evolve {other}"),
                "usage_invalid",
                format!("unknown skills evolve subcommand: {other}"),
                Some("Choose one of: cluster, crystallize, verify, submit, install.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "skills evolve".to_string(),
                "usage_invalid",
                "missing skills evolve subcommand".to_string(),
                Some("Choose one of: cluster, crystallize, verify, submit, install.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

}
