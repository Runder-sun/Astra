impl Runtime {
    fn handle_plugins(
        &self,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let state_home = state_home().map_err(|err| internal_failure("plugins", err))?;
        let snapshot = plugins::discover(&state_home, command_cwd(cwd, parsed));
        match args.first().map(String::as_str) {
            Some("list") => Ok(CommandSuccess::new_with_data(
                "plugins list",
                serde_json::to_value(plugins::list(&snapshot))
                    .expect("plugins list should serialize"),
            )),
            Some("inspect") => {
                let plugin_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "plugins inspect".to_string(),
                        "usage_invalid",
                        "missing plugin id".to_string(),
                        Some("Pass a plugin id after `plugins inspect`.".to_string()),
                    )
                })?;
                let inspected = plugins::inspect(&snapshot, plugin_id).map_err(|err| {
                    CommandFailureOutcome::new(
                        6,
                        "plugins inspect".to_string(),
                        None,
                        None,
                        "plugin_unknown",
                        err.to_string(),
                        Some(
                            "Run `research-cli plugins list --json` to inspect known plugins."
                                .to_string(),
                        ),
                        false,
                        None,
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::new_with_data(
                    "plugins inspect",
                    serde_json::to_value(inspected).expect("plugins inspect should serialize"),
                ))
            }
            Some("validate") => {
                let result = plugins::validate(&snapshot, args.get(1).map(String::as_str))
                    .map_err(|err| {
                        CommandFailureOutcome::new(
                            6,
                            "plugins validate".to_string(),
                            None,
                            None,
                            "plugin_unknown",
                            err.to_string(),
                            Some(
                                "Run `research-cli plugins list --json` to inspect known plugins."
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
                        "plugins validate".to_string(),
                        None,
                        None,
                        "followup_required",
                        "one or more plugins are degraded".to_string(),
                        Some(
                            "Inspect the validation issues and repair degraded plugin manifests."
                                .to_string(),
                        ),
                        false,
                        Some(
                            serde_json::to_value(result)
                                .expect("plugins validate should serialize"),
                        ),
                        parsed.output_json,
                    ));
                }
                Ok(CommandSuccess::new_with_data(
                    "plugins validate",
                    serde_json::to_value(result).expect("plugins validate should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("plugins {other}"),
                "usage_invalid",
                format!("unknown plugins subcommand: {other}"),
                Some("Choose one of: list, inspect, validate.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "plugins".to_string(),
                "usage_invalid",
                "missing plugins subcommand".to_string(),
                Some("Choose one of: list, inspect, validate.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_hooks(
        &self,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let state_home = state_home().map_err(|err| internal_failure("hooks", err))?;
        let snapshot = plugins::discover(&state_home, command_cwd(cwd, parsed));
        match args.first().map(String::as_str) {
            Some("list") => Ok(CommandSuccess::new_with_data(
                "hooks list",
                serde_json::to_value(plugins::list_hooks(&snapshot))
                    .expect("hooks list should serialize"),
            )),
            Some("inspect") => {
                let hook_id = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "hooks inspect".to_string(),
                        "usage_invalid",
                        "missing hook id".to_string(),
                        Some("Pass a hook id after `hooks inspect`.".to_string()),
                    )
                })?;
                let inspected = plugins::inspect_hook(&snapshot, hook_id).map_err(|err| {
                    CommandFailureOutcome::new(
                        6,
                        "hooks inspect".to_string(),
                        None,
                        None,
                        "hook_unknown",
                        err.to_string(),
                        Some(
                            "Run `research-cli hooks list --json` to inspect known hooks."
                                .to_string(),
                        ),
                        false,
                        None,
                        parsed.output_json,
                    )
                })?;
                Ok(CommandSuccess::new_with_data(
                    "hooks inspect",
                    serde_json::to_value(inspected).expect("hooks inspect should serialize"),
                ))
            }
            Some("test") => {
                let result = plugins::test_hooks(&snapshot, args.get(1).map(String::as_str))
                    .map_err(|err| {
                        CommandFailureOutcome::new(
                            6,
                            "hooks test".to_string(),
                            None,
                            None,
                            "hook_unknown",
                            err.to_string(),
                            Some(
                                "Run `research-cli hooks list --json` to inspect known hooks."
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
                        "hooks test".to_string(),
                        None,
                        None,
                        "followup_required",
                        "one or more hooks are degraded".to_string(),
                        Some(
                            "Inspect the hook test result and repair missing hook paths."
                                .to_string(),
                        ),
                        false,
                        Some(serde_json::to_value(result).expect("hooks test should serialize")),
                        parsed.output_json,
                    ));
                }
                Ok(CommandSuccess::new_with_data(
                    "hooks test",
                    serde_json::to_value(result).expect("hooks test should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("hooks {other}"),
                "usage_invalid",
                format!("unknown hooks subcommand: {other}"),
                Some("Choose one of: list, inspect, test.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "hooks".to_string(),
                "usage_invalid",
                "missing hooks subcommand".to_string(),
                Some("Choose one of: list, inspect, test.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_usage(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if has_unrecognized_positionals(args, &["--scope"]) {
            return Err(CommandFailureOutcome::usage(
                "usage".to_string(),
                "usage_invalid",
                "usage does not accept positional arguments".to_string(),
                Some("Use `research-cli usage --scope turn|session|project --json`.".to_string()),
            ));
        }
        let scope = parse_flag_value(args, "--scope").unwrap_or_else(|| "project".to_string());
        if !matches!(scope.as_str(), "turn" | "session" | "project") {
            return Err(CommandFailureOutcome::usage(
                "usage".to_string(),
                "usage_invalid",
                format!("invalid usage scope: {scope}"),
                Some("Choose one of: turn, session, project.".to_string()),
            ));
        }
        let resolved = self
            .resolve_project(registry, cwd, parsed, "usage")
            .map_err(|err| project_resolution_failure("usage", err))?;
        let summary = telemetry::usage_summary(&resolved, &scope)
            .map_err(|err| telemetry_failure("usage", err, &resolved))?;
        Ok(CommandSuccess::new_with_data(
            "usage",
            serde_json::to_value(summary).expect("usage summary should serialize"),
        ))
    }

    fn handle_cost(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if has_unrecognized_positionals(args, &["--scope"]) {
            return Err(CommandFailureOutcome::usage(
                "cost".to_string(),
                "usage_invalid",
                "cost does not accept positional arguments".to_string(),
                Some("Use `research-cli cost --scope turn|session|project --json`.".to_string()),
            ));
        }
        let scope = parse_flag_value(args, "--scope").unwrap_or_else(|| "project".to_string());
        if !matches!(scope.as_str(), "turn" | "session" | "project") {
            return Err(CommandFailureOutcome::usage(
                "cost".to_string(),
                "usage_invalid",
                format!("invalid cost scope: {scope}"),
                Some("Choose one of: turn, session, project.".to_string()),
            ));
        }
        let resolved = self
            .resolve_project(registry, cwd, parsed, "cost")
            .map_err(|err| project_resolution_failure("cost", err))?;
        let summary = telemetry::cost_summary(&resolved, &scope)
            .map_err(|err| telemetry_failure("cost", err, &resolved))?;
        Ok(CommandSuccess::new_with_data(
            "cost",
            serde_json::to_value(summary).expect("cost summary should serialize"),
        ))
    }

    fn handle_stats(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if has_unrecognized_positionals(args, &["--scope"]) {
            return Err(CommandFailureOutcome::usage(
                "stats".to_string(),
                "usage_invalid",
                "stats does not accept positional arguments".to_string(),
                Some("Use `research-cli stats --scope runtime|project --json`.".to_string()),
            ));
        }
        let scope = parse_flag_value(args, "--scope").unwrap_or_else(|| "runtime".to_string());
        if !matches!(scope.as_str(), "runtime" | "project") {
            return Err(CommandFailureOutcome::usage(
                "stats".to_string(),
                "usage_invalid",
                format!("invalid stats scope: {scope}"),
                Some("Choose one of: runtime, project.".to_string()),
            ));
        }
        let resolved = if scope == "project" {
            Some(
                self.resolve_project(registry, cwd, parsed, "stats")
                    .map_err(|err| project_resolution_failure("stats", err))?,
            )
        } else {
            None
        };
        let summary = telemetry::stats_summary(registry, resolved.as_ref(), &scope).map_err(
            |err| match (resolved.as_ref(), err) {
                (Some(project), err) => telemetry_failure("stats", err, project),
                (None, telemetry::TelemetryError::Registry(inner)) => CommandFailureOutcome::new(
                    10,
                    "stats".to_string(),
                    None,
                    None,
                    "telemetry_unavailable",
                    inner.to_string(),
                    Some("Inspect the project registry and retry.".to_string()),
                    false,
                    Some(json!({
                        "scope": scope
                    })),
                    false,
                ),
                (None, telemetry::TelemetryError::SessionStore(inner)) => {
                    internal_failure("stats", inner.to_string())
                }
            },
        )?;
        Ok(CommandSuccess::new_with_data(
            "stats",
            serde_json::to_value(summary).expect("stats summary should serialize"),
        ))
    }

    fn handle_smoke(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let scoped_cwd = command_cwd(cwd, parsed);
        let result = SmokeService::run(
            registry,
            scoped_cwd,
            parsed.project.as_deref(),
            parsed.cwd.as_deref(),
        )
        .map_err(|err| internal_failure("smoke", err))?;

        if result.overall_status == "blocked" && result.session_id.is_none() {
            return Err(CommandFailureOutcome::new(
                4,
                "smoke".to_string(),
                None,
                None,
                "smoke_blocked",
                "smoke preflight is blocked".to_string(),
                result.preflight.recovery_hints.first().cloned(),
                false,
                Some(serde_json::to_value(result).expect("smoke result should serialize")),
                false,
            ));
        }

        if !result.source_mutation_free {
            return Err(CommandFailureOutcome::new(
                10,
                "smoke".to_string(),
                None,
                None,
                "smoke_source_mutation_detected",
                "smoke mutated paths outside .pmcli".to_string(),
                Some(
                    "Inspect mutated_paths and repair the smoke path before retrying.".to_string(),
                ),
                false,
                Some(serde_json::to_value(result).expect("smoke result should serialize")),
                false,
            ));
        }

        Ok(CommandSuccess::new_with_data(
            "smoke",
            serde_json::to_value(result).expect("smoke result should serialize"),
        ))
    }

}
