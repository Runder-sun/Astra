impl Runtime {
    fn handle_tools(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("run") => self.handle_tools_run(registry, cwd, parsed, &args[1..]),
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("tools {other}"),
                "usage_invalid",
                format!("unknown tools subcommand: {other}"),
                Some("Choose one of: run.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "tools".to_string(),
                "usage_invalid",
                "missing tools subcommand".to_string(),
                Some("Choose one of: run.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_tools_run(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, "tools run")
            .map_err(|err| project_resolution_failure("tools run", err))?;
        let tool_name = args.first().ok_or_else(|| {
            CommandFailureOutcome::usage(
                "tools run".to_string(),
                "usage_invalid",
                "missing tool name for tools run".to_string(),
                Some("Use `research-cli tools run read_file --path README.md --json`.".to_string()),
            )
            .with_output_json(parsed.output_json)
        })?;
        let flag_args = &args[1..];
        if has_unrecognized_positionals(
            flag_args,
            &[
                "--path",
                "--content",
                "--pattern",
                "--old",
                "--new",
                "--command",
                "--url",
                "--allow-private-network",
                "--permission-mode",
                "--permission-request",
            ],
        ) {
            return Err(CommandFailureOutcome::usage(
                "tools run".to_string(),
                "usage_invalid",
                "tools run accepts one tool name followed by recognized flags".to_string(),
                Some(
                    "Recognized flags: --path, --content, --pattern, --old, --new, --command, --url, --allow-private-network, --permission-mode, --permission-request."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json));
        }

        let tool_registry = builtin_registry();
        let tool_spec = tool_registry.get(tool_name).ok_or_else(|| {
            CommandFailureOutcome::usage(
                "tools run".to_string(),
                "usage_invalid",
                format!("unknown tool: {tool_name}"),
                Some(
                    "Use one of the built-in tools: read_file, list_files, search, fetch, write_file, apply_patch, delete_file, shell."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)
        })?;
        let target_path = parse_flag_value(flag_args, "--path");
        let content = parse_flag_value(flag_args, "--content")
            .or_else(|| parse_flag_value(flag_args, "--command"));
        let effective_tool_spec = if tool_name == "shell" {
            let command = parse_flag_value(flag_args, "--command")
                .or_else(|| parse_flag_value(flag_args, "--content"))
                .unwrap_or_default();
            if is_read_only_shell_command(&command) {
                ToolSpec::new(tool_name, ToolClassification::ReadOnly)
            } else {
                tool_spec.clone()
            }
        } else {
            tool_spec.clone()
        };
        let permission_mode = parse_flag_value(flag_args, "--permission-mode")
            .or_else(|| {
                effective_config(&resolved)
                    .ok()
                    .and_then(|cfg| cfg.effective.permission_mode)
            })
            .unwrap_or_else(|| "read-only".to_string());
        let mode =
            crate::permissions::PermissionMode::parse(&permission_mode).ok_or_else(|| {
                CommandFailureOutcome::usage(
                    "tools run".to_string(),
                    "usage_invalid",
                    format!("invalid permission mode: {permission_mode}"),
                    Some(
                        "Choose one of: read-only, workspace-write, danger-full-access."
                            .to_string(),
                    ),
                )
                .with_output_json(parsed.output_json)
            })?;
        let policy = crate::permissions::PermissionPolicy::new(mode, &resolved.workspace_root);
        let evaluation = policy.evaluate(&effective_tool_spec, target_path.as_deref());
        let input_payload = tool_input_payload(tool_name, flag_args);
        let input_digest = permission_input_digest(&input_payload, &permission_mode, &evaluation);

        let permission_request_id = parse_flag_value(flag_args, "--permission-request");
        if !evaluation.allowed {
            if evaluation.requires_approval && permission_request_id.is_none() {
                let request_id = format!("perm_{}", timestamp_string());
                let session_id = format!("tool_session_{}", timestamp_string());
                let turn_id = format!("tool_turn_{}", timestamp_string());
                let requested_at = timestamp_string();
                let expires_at_ms = permission_request_expires_at_ms();
                let request_payload = json!({
                    "request_id": request_id,
                    "session_id": session_id,
                    "turn_id": turn_id,
                    "tool_name": tool_name,
                    "target_path": target_path.clone(),
                    "tool_input": input_payload,
                    "tool_input_digest": input_digest,
                    "permission_mode": permission_mode,
                    "reason": evaluation.reason,
                    "reason_code": evaluation.reason_code,
                    "requested_at": requested_at,
                    "expires_at_ms": expires_at_ms,
                    "status": "pending",
                    "trace": permission_trace_payload(
                        &request_id,
                        tool_name,
                        "permission_request",
                        target_path.as_deref(),
                        &permission_mode,
                        &evaluation,
                        false,
                    )
                });
                emit_permission_event(
                    &resolved,
                    &session_id,
                    &request_id,
                    "start",
                    None,
                    request_payload,
                )
                .map_err(|err| {
                    internal_failure("tools run", err).with_output_json(parsed.output_json)
                })?;
                publish_project_checkpoint(&resolved, &["permission"]).map_err(|err| {
                    internal_failure("tools run", err).with_output_json(parsed.output_json)
                })?;

                return Err(CommandFailureOutcome::new(
                    5,
                    "tools run".to_string(),
                    Some(resolved.project_id.clone()),
                    Some(session_id),
                    "permission_unresolved",
                    format!("tool '{tool_name}' requires permission approval before execution"),
                    Some("Approve the request with `research-cli permissions approve <request_id> --json`, then retry with `--permission-request <request_id>`.".to_string()),
                    false,
                    Some(json!({
                        "request_id": request_id,
                        "tool_name": tool_name,
                        "target_path": target_path,
                        "permission_mode": permission_mode,
                        "expires_at_ms": expires_at_ms,
                        "reason_code": evaluation.reason_code
                    })),
                    parsed.output_json,
                ));
            }

            if !evaluation.requires_approval {
                return Err(CommandFailureOutcome::new(
                7,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                None,
                "policy_refusal",
                evaluation.reason.clone(),
                Some("Retry with a workspace-relative path or danger-full-access mode where appropriate.".to_string()),
                false,
                Some(json!({
                    "policy_refusal": {
                        "policy_kind": "permission",
                        "policy_source": "permission_policy",
                        "reason_code": evaluation.reason_code,
                        "reason": evaluation.reason,
                        "requires_approval": evaluation.requires_approval,
                        "workspace_boundary_ok": evaluation.workspace_boundary_ok
                    }
                })),
                parsed.output_json,
                ));
            }
        }

        if let Some(request_id) = permission_request_id.as_deref() {
            self.validate_tool_permission_request(
                &resolved,
                request_id,
                tool_name,
                target_path.as_deref(),
                &input_digest,
                parsed.output_json,
            )?;
        }

        let mut call = ToolCall::new(tool_name);
        call.target_path = target_path;
        call.content = content;
        for (key, value) in tool_parameters_from_args(flag_args) {
            call.parameters.insert(key, value);
        }
        let executor = LocalToolExecutor::new(&resolved.workspace_root);
        let result = executor.execute(&call).map_err(|err| {
            if err.reason_code() == "workspace_scope_violation" {
                return CommandFailureOutcome::new(
                    7,
                    "tools run".to_string(),
                    Some(resolved.project_id.clone()),
                    None,
                    "policy_refusal",
                    err.to_string(),
                    Some("Retry with a workspace-relative path that does not traverse symlinks outside the workspace.".to_string()),
                    false,
                    Some(json!({
                        "policy_refusal": {
                            "policy_kind": "permission",
                            "policy_source": "tool_executor",
                            "reason_code": err.reason_code(),
                            "reason": err.to_string(),
                            "requires_approval": false,
                            "workspace_boundary_ok": false
                        }
                    })),
                    parsed.output_json,
                );
            }
            CommandFailureOutcome::new(
                10,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                None,
                "tool_execution_failed",
                err.to_string(),
                Some("Inspect the tool arguments and local workspace files.".to_string()),
                false,
                None,
                parsed.output_json,
            )
        })?;
        let mut result_value = serde_json::to_value(&result).expect("tool result should serialize");
        if let Some(request_id) = permission_request_id.as_deref() {
            result_value["permission_request_id"] = json!(request_id);
        }
        result_value["permission"] = permission_trace_payload(
            permission_request_id.as_deref().unwrap_or(""),
            tool_name,
            "tool_execution",
            call.target_path.as_deref(),
            &permission_mode,
            &evaluation,
            true,
        );
        append_canonical_event(
            &resolved,
            "tool_call",
            "terminal",
            Some("succeeded"),
            "tool",
            tool_name,
            None,
            result_value.clone(),
        )
        .map_err(|err| internal_failure("tools run", err).with_output_json(parsed.output_json))?;
        publish_project_checkpoint(&resolved, &["tool"]).map_err(|err| {
            internal_failure("tools run", err).with_output_json(parsed.output_json)
        })?;

        Ok(CommandSuccess::with_project(
            "tools run",
            resolved.project_id.clone(),
            result_value,
        ))
    }

    fn validate_tool_permission_request(
        &self,
        resolved: &ResolvedProject,
        request_id: &str,
        tool_name: &str,
        target_path: Option<&str>,
        input_digest: &str,
        output_json: bool,
    ) -> Result<(), CommandFailureOutcome> {
        let event_log_path = resolved.data_dir.join("events").join("events.jsonl");
        let events = read_events_from(&event_log_path)
            .map_err(|err| internal_failure("tools run", err.to_string()))?;
        let request = events.iter().find(|event| {
            event.event_name == "permission"
                && event.phase == "start"
                && event.object_id == request_id
        });
        let Some(request) = request else {
            return Err(CommandFailureOutcome::new(
                6,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                None,
                "permission_request_not_found",
                format!("unknown permission request: {request_id}"),
                Some("Run `research-cli permissions pending --json` first.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            ));
        };
        let request_tool = request.payload.get("tool_name").and_then(Value::as_str);
        let request_path = request.payload.get("target_path").and_then(Value::as_str);
        let decisions = events
            .iter()
            .filter(|event| {
                event.event_name == "permission"
                    && event.phase == "terminal"
                    && event.object_id == request_id
            })
            .collect::<Vec<_>>();
        let consumed = events.iter().any(|event| {
            event.event_name == "permission_consumed"
                && event.phase == "terminal"
                && event.object_id == request_id
        });
        if consumed {
            return Err(CommandFailureOutcome::new(
                6,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                request.session_id.clone(),
                "permission_request_resolved",
                format!("permission request is already resolved: {request_id}"),
                Some("Re-run the action to create a fresh permission request.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            ));
        }
        if decisions.len() > 1 {
            return Err(CommandFailureOutcome::new(
                6,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                request.session_id.clone(),
                "permission_request_resolved",
                format!("permission request has conflicting decisions: {request_id}"),
                Some("Inspect `research-cli permissions history --json`.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            ));
        }
        let request_digest = request
            .payload
            .get("tool_input_digest")
            .and_then(Value::as_str);
        if request_tool != Some(tool_name)
            || request_path != target_path
            || request_digest != Some(input_digest)
        {
            return Err(CommandFailureOutcome::new(
                6,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                None,
                "permission_request_mismatch",
                format!("permission request does not match tool invocation: {request_id}"),
                Some(
                    "Retry with the tool and path that created the permission request.".to_string(),
                ),
                false,
                Some(json!({
                    "request_id": request_id,
                    "requested_tool": request_tool,
                    "requested_path": request_path,
                    "requested_digest": request_digest,
                    "tool_name": tool_name,
                    "target_path": target_path,
                    "tool_input_digest": input_digest
                })),
                output_json,
            ));
        }
        if permission_request_is_expired(&request.payload) {
            return Err(CommandFailureOutcome::new(
                6,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                request.session_id.clone(),
                "permission_request_expired",
                format!("permission request expired: {request_id}"),
                Some("Re-run the action to create a fresh permission request.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            ));
        }
        let Some(decision) = decisions.first() else {
            return Err(CommandFailureOutcome::new(
                5,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                request.session_id.clone(),
                "permission_unresolved",
                format!("permission request is still pending: {request_id}"),
                Some("Approve the request before retrying the tool call.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            ));
        };
        if decision.payload.get("decision").and_then(Value::as_str) != Some("approved") {
            return Err(CommandFailureOutcome::new(
                6,
                "tools run".to_string(),
                Some(resolved.project_id.clone()),
                request.session_id.clone(),
                "permission_denied",
                format!("permission request was not approved: {request_id}"),
                Some("Re-run the action to create a fresh permission request.".to_string()),
                false,
                Some(json!({ "request_id": request_id })),
                output_json,
            ));
        }
        let consume_payload = json!({
            "request_id": request_id,
            "status": "consumed",
            "session_id": request.session_id,
            "tool_name": tool_name,
            "target_path": target_path,
            "tool_input_digest": input_digest,
            "consumed_at": timestamp_string(),
            "trace": {
                "request_id": request_id,
                "tool_name": tool_name,
                "action": "permission_consumed",
                "requested_path": target_path,
                "required_mode": "workspace-write",
                "current_mode": request.payload.get("permission_mode").and_then(Value::as_str).unwrap_or("read-only"),
                "allowlist_match": false,
                "workspace_boundary_ok": true,
                "branch_boundary_ok": true,
                "destructive": false,
                "approved": true,
                "reason": "approved permission consumed by tool execution"
            }
        });
        append_canonical_event(
            resolved,
            "permission_consumed",
            "terminal",
            Some("succeeded"),
            "permission",
            request_id,
            request.session_id.as_deref(),
            consume_payload,
        )
        .map_err(|err| internal_failure("tools run", err))?;
        Ok(())
    }

    fn handle_conformance(
        &self,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if has_unrecognized_positionals(args, &["--family"]) {
            return Err(CommandFailureOutcome::usage(
                "conformance".to_string(),
                "usage_invalid",
                "conformance does not accept positional arguments".to_string(),
                Some("Use `research-cli conformance --json`.".to_string()),
            )
            .with_output_json(parsed.output_json));
        }

        let requested_family =
            parse_flag_value(args, "--family").unwrap_or_else(|| "foundation".to_string());
        if env::var("RESEARCH_CLI_CONFORMANCE_SELFTEST")
            .ok()
            .as_deref()
            == Some("1")
        {
            let result = ConformanceResult {
                scope: requested_family,
                execution_mode: "selftest_guard".to_string(),
                passed_families: vec![
                    "schema_registry".to_string(),
                    "runner_entrypoints".to_string(),
                    "ci_floor".to_string(),
                ],
                failed_families: Vec::new(),
                failure_refs: Vec::new(),
            };
            return Ok(CommandSuccess::new_with_data(
                "conformance",
                serde_json::to_value(result).expect("conformance result should serialize"),
            ));
        }

        let mut passed_families = Vec::new();
        let mut failed_families = Vec::new();
        let mut failure_refs = Vec::new();
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let schema_assets = [
            "schemas/command_success.schema.json",
            "schemas/command_failure.schema.json",
            "schemas/help_surface_report.schema.json",
            "schemas/palette_surface_report.schema.json",
            "schemas/project_registry_entry.schema.json",
            "schemas/current_project_pointer.schema.json",
            "schemas/project_resolution_trace.schema.json",
            "schemas/project_registry_list.schema.json",
            "schemas/project_status.schema.json",
            "schemas/runtime_preflight_report.schema.json",
            "schemas/doctor_report.schema.json",
            "schemas/smoke_result.schema.json",
            "schemas/provider_resolution_trace.schema.json",
            "schemas/provider_catalog_refresh_result.schema.json",
            "schemas/usage_summary.schema.json",
            "schemas/stats_summary.schema.json",
            "schemas/setup_status_report.schema.json",
            "schemas/migrate_check_report.schema.json",
            "schemas/repair_hints_report.schema.json",
            "schemas/install_routes_report.schema.json",
            "schemas/skill_list_result.schema.json",
            "schemas/skill_inspect_result.schema.json",
            "schemas/skill_paths_result.schema.json",
            "schemas/skill_validation_result.schema.json",
            "schemas/plugin_list_result.schema.json",
            "schemas/plugin_inspection_result.schema.json",
            "schemas/plugin_validation_result.schema.json",
            "schemas/hook_list_result.schema.json",
            "schemas/hook_inspection_result.schema.json",
            "schemas/hook_test_result.schema.json",
            "schemas/mcp_list_result.schema.json",
            "schemas/mcp_inspection_result.schema.json",
            "schemas/mcp_test_result.schema.json",
            "schemas/mcp_refresh_result.schema.json",
            "schemas/artifact_list_result.schema.json",
            "schemas/artifact_inspection_result.schema.json",
            "schemas/artifact_promotion_candidate.schema.json",
            "schemas/artifact_promotion_queue_result.schema.json",
            "schemas/artifact_promotion_result.schema.json",
            "schemas/repo_cleanup_proposal.schema.json",
            "schemas/canonical_lineage_manifest.schema.json",
            "schemas/doc_context_projection.schema.json",
            "schemas/experiment_supervisor_lease.schema.json",
            "schemas/wake_event.schema.json",
            "schemas/projectops_status_report.schema.json",
            "schemas/host_surface_projection.schema.json",
            "schemas/host_surface_action.schema.json",
            "schemas/tui_launch_result.schema.json",
            "schemas/tailscale_status_report.schema.json",
            "schemas/remote_terminal_projection.schema.json",
            "schemas/agent_runtime_identity.schema.json",
        ];
        let missing_schema_assets = schema_assets
            .iter()
            .filter(|path| !repo_root.join(path).exists())
            .map(|path| (*path).to_string())
            .collect::<Vec<_>>();
        if missing_schema_assets.is_empty() {
            passed_families.push("schema_registry".to_string());
        } else {
            failed_families.push("schema_registry".to_string());
            failure_refs.extend(missing_schema_assets);
        }

        let runs = [
            (
                "runner_entrypoints",
                vec!["bash", "tests/conformance/run_conformance.sh"],
            ),
            ("golden_runner", vec!["bash", "tests/golden/run_golden.sh"]),
            (
                "mock_parity_harness",
                vec!["bash", "scripts/run_mock_parity_harness.sh"],
            ),
        ];
        for (family, command) in runs {
            let output = std::process::Command::new(command[0])
                .args(&command[1..])
                .current_dir(&repo_root)
                .env("RESEARCH_CLI_CONFORMANCE_SELFTEST", "1")
                .output();
            match output {
                Ok(output) if output.status.success() => passed_families.push(family.to_string()),
                Ok(output) => {
                    failed_families.push(family.to_string());
                    failure_refs.push(format!(
                        "{} exited with status {}: {}",
                        family,
                        output.status.code().unwrap_or_default(),
                        compact_subprocess_output(&output.stdout, &output.stderr)
                    ));
                }
                Err(err) => {
                    failed_families.push(family.to_string());
                    failure_refs.push(format!("{family}: {err}"));
                }
            }
        }

        if repo_root.join(".github/workflows/pr-fast.yml").exists() {
            passed_families.push("ci_floor".to_string());
        } else {
            failed_families.push("ci_floor".to_string());
            failure_refs.push(".github/workflows/pr-fast.yml".to_string());
        }

        let result = ConformanceResult {
            scope: requested_family,
            execution_mode: "live_harness".to_string(),
            passed_families,
            failed_families: failed_families.clone(),
            failure_refs,
        };

        if !failed_families.is_empty() {
            return Err(CommandFailureOutcome::new(
                10,
                "conformance".to_string(),
                None,
                None,
                "conformance_failed",
                "local conformance entrypoint checks failed".to_string(),
                Some("Inspect failure_refs and repair the missing conformance assets.".to_string()),
                false,
                Some(serde_json::to_value(result).expect("conformance result should serialize")),
                parsed.output_json,
            ));
        }

        Ok(CommandSuccess::new_with_data(
            "conformance",
            serde_json::to_value(result).expect("conformance result should serialize"),
        ))
    }

    fn handle_memory(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("append") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory append")
                    .map_err(|err| {
                        project_resolution_failure("memory append", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_working_memory_append_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "memory append".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `research-cli memory append --content <text> [--kind <kind>] [--pin] [--support-ref <ref>] [--limit <n>] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = memory::append(&resolved.data_dir, request).map_err(|err| {
                    internal_failure("memory append", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "memory append",
                    resolved.project_id,
                    serde_json::to_value(result).expect("memory append result should serialize"),
                ))
            }
            Some("trajectory") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory trajectory")
                    .map_err(|err| {
                        project_resolution_failure("memory trajectory", err)
                            .with_output_json(parsed.output_json)
                    })?;
                match args.get(1).map(String::as_str) {
                    Some("ingest") => {
                        let request =
                            parse_trajectory_ingest_request(&args[2..]).map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory trajectory ingest".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory trajectory ingest <path> --source-kind <kind> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let result =
                            trajectory::ingest(&resolved.data_dir, &resolved.project_id, request)
                                .map_err(|err| {
                                    trajectory_failure(
                                        "memory trajectory ingest",
                                        err,
                                        parsed.output_json,
                                    )
                                })?;
                        Ok(CommandSuccess::with_project(
                            "memory trajectory ingest",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("trajectory ingest result should serialize"),
                        ))
                    }
                    Some("segment") => {
                        let ingest_id = parse_positional(&args[2..], "ingest id")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory trajectory segment".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory trajectory segment <ingest-id> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let result = trajectory::segment(&resolved.data_dir, &ingest_id)
                            .map_err(|err| {
                                internal_failure("memory trajectory segment", err.to_string())
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "memory trajectory segment",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("trajectory segment result should serialize"),
                        ))
                    }
                    Some("label") => {
                        let segment_id = parse_positional(&args[2..], "segment id")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory trajectory label".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory trajectory label <segment-id> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let result = trajectory::label(&resolved.data_dir, &segment_id)
                            .map_err(|err| {
                                internal_failure("memory trajectory label", err.to_string())
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "memory trajectory label",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("trajectory label result should serialize"),
                        ))
                    }
                    Some("extract") => {
                        let segment_id = parse_positional(&args[2..], "segment id")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory trajectory extract".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory trajectory extract <segment-id> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let result = trajectory::extract(&resolved.data_dir, &segment_id)
                            .map_err(|err| {
                                internal_failure("memory trajectory extract", err.to_string())
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "memory trajectory extract",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("trajectory extract result should serialize"),
                        ))
                    }
                    Some("status") => {
                        let result = trajectory::status(&resolved.data_dir).map_err(|err| {
                            internal_failure("memory trajectory status", err.to_string())
                                .with_output_json(parsed.output_json)
                        })?;
                        Ok(CommandSuccess::with_project(
                            "memory trajectory status",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("trajectory status result should serialize"),
                        ))
                    }
                    Some("explain") => {
                        let segment_id = parse_positional(&args[2..], "segment id")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory trajectory explain".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory trajectory explain <segment-id> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let result = trajectory::explain_segment(&resolved.data_dir, &segment_id)
                            .map_err(|err| {
                                internal_failure("memory trajectory explain", err.to_string())
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "memory trajectory explain",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("trajectory explain result should serialize"),
                        ))
                    }
                    Some(other) => Err(CommandFailureOutcome::usage(
                        format!("memory trajectory {other}"),
                        "usage_invalid",
                        format!("unknown memory trajectory subcommand: {other}"),
                        Some(
                            "Choose one of: ingest, segment, label, extract, status, explain."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)),
                    None => Err(CommandFailureOutcome::usage(
                        "memory trajectory".to_string(),
                        "usage_invalid",
                        "missing memory trajectory subcommand".to_string(),
                        Some(
                            "Choose one of: ingest, segment, label, extract, status, explain."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)),
                }
            }
            Some("adoption") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory adoption")
                    .map_err(|err| {
                        project_resolution_failure("memory adoption", err)
                            .with_output_json(parsed.output_json)
                    })?;
                match args.get(1).map(String::as_str) {
                    Some("record") => {
                        let memory_id = parse_positional(&args[2..], "memory record id")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory adoption record".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory adoption record <memory-id> --event <kind> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let event = parse_flag_value(&args[2..], "--event").ok_or_else(|| {
                            CommandFailureOutcome::usage(
                                "memory adoption record".to_string(),
                                "usage_invalid",
                                "memory adoption record requires --event".to_string(),
                                Some("Use `research-cli memory adoption record <memory-id> --event <kind> --json`.".to_string()),
                            )
                            .with_output_json(parsed.output_json)
                        })?;
                        let result =
                            trajectory::record_adoption(&resolved.data_dir, &memory_id, &event)
                                .map_err(|err| {
                                    internal_failure("memory adoption record", err.to_string())
                                        .with_output_json(parsed.output_json)
                                })?;
                        Ok(CommandSuccess::with_project(
                            "memory adoption record",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("memory adoption result should serialize"),
                        ))
                    }
                    Some(other) => Err(CommandFailureOutcome::usage(
                        format!("memory adoption {other}"),
                        "usage_invalid",
                        format!("unknown memory adoption subcommand: {other}"),
                        Some("Choose one of: record.".to_string()),
                    )
                    .with_output_json(parsed.output_json)),
                    None => Err(CommandFailureOutcome::usage(
                        "memory adoption".to_string(),
                        "usage_invalid",
                        "missing memory adoption subcommand".to_string(),
                        Some("Choose one of: record.".to_string()),
                    )
                    .with_output_json(parsed.output_json)),
                }
            }
            Some("tier") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory tier")
                    .map_err(|err| {
                        project_resolution_failure("memory tier", err)
                            .with_output_json(parsed.output_json)
                    })?;
                match args.get(1).map(String::as_str) {
                    Some("rebalance") => {
                        let result =
                            trajectory::rebalance_tiers(&resolved.data_dir).map_err(|err| {
                                internal_failure("memory tier rebalance", err.to_string())
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "memory tier rebalance",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("memory tier rebalance result should serialize"),
                        ))
                    }
                    Some("explain") => {
                        let memory_id = parse_positional(&args[2..], "memory record id")
                            .map_err(|message| {
                                CommandFailureOutcome::usage(
                                    "memory tier explain".to_string(),
                                    "usage_invalid",
                                    message,
                                    Some("Use `research-cli memory tier explain <memory-id> --json`.".to_string()),
                                )
                                .with_output_json(parsed.output_json)
                            })?;
                        let result = trajectory::explain_tier(&resolved.data_dir, &memory_id)
                            .map_err(|err| {
                                internal_failure("memory tier explain", err.to_string())
                                    .with_output_json(parsed.output_json)
                            })?;
                        Ok(CommandSuccess::with_project(
                            "memory tier explain",
                            resolved.project_id,
                            serde_json::to_value(result)
                                .expect("memory tier explain result should serialize"),
                        ))
                    }
                    Some(other) => Err(CommandFailureOutcome::usage(
                        format!("memory tier {other}"),
                        "usage_invalid",
                        format!("unknown memory tier subcommand: {other}"),
                        Some("Choose one of: rebalance, explain.".to_string()),
                    )
                    .with_output_json(parsed.output_json)),
                    None => Err(CommandFailureOutcome::usage(
                        "memory tier".to_string(),
                        "usage_invalid",
                        "missing memory tier subcommand".to_string(),
                        Some("Choose one of: rebalance, explain.".to_string()),
                    )
                    .with_output_json(parsed.output_json)),
                }
            }
            Some("status") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory status")
                    .map_err(|err| {
                        project_resolution_failure("memory status", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let limit =
                    parse_optional_usize_flag(&args[1..], "--limit").map_err(|message| {
                        CommandFailureOutcome::usage(
                            "memory status".to_string(),
                            "usage_invalid",
                            message,
                            Some(
                                "Use `research-cli memory status [--limit <n>] --json`."
                                    .to_string(),
                            ),
                        )
                        .with_output_json(parsed.output_json)
                    })?;
                let result = memory::status(&resolved.data_dir, limit).map_err(|err| {
                    internal_failure("memory status", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "memory status",
                    resolved.project_id,
                    serde_json::to_value(result).expect("memory status result should serialize"),
                ))
            }
            Some("promote") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory promote")
                    .map_err(|err| {
                        project_resolution_failure("memory promote", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_memory_promote_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "memory promote".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `research-cli memory promote --candidate-id <id> [--trust supported|trusted] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = memory::promote(&resolved.data_dir, &resolved.project_id, request)
                    .map_err(|err| {
                        internal_failure("memory promote", err.to_string())
                            .with_output_json(parsed.output_json)
                    })?;
                Ok(CommandSuccess::with_project(
                    "memory promote",
                    resolved.project_id,
                    serde_json::to_value(result).expect("memory promote result should serialize"),
                ))
            }
            Some("query") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory query")
                    .map_err(|err| {
                        project_resolution_failure("memory query", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let request = parse_memory_query_request(&args[1..]).map_err(|message| {
                    CommandFailureOutcome::usage(
                        "memory query".to_string(),
                        "usage_invalid",
                        message,
                        Some(
                            "Use `research-cli memory query <text> [--limit <n>] [--include-inactive] --json`."
                                .to_string(),
                        ),
                    )
                    .with_output_json(parsed.output_json)
                })?;
                let result = memory::query(&resolved.data_dir, request).map_err(|err| {
                    internal_failure("memory query", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "memory query",
                    resolved.project_id,
                    serde_json::to_value(result).expect("memory query result should serialize"),
                ))
            }
            Some("explain") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory explain")
                    .map_err(|err| {
                        project_resolution_failure("memory explain", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let record_id = parse_flag_value(&args[1..], "--record-id")
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "memory explain".to_string(),
                            "usage_invalid",
                            "memory explain requires --record-id".to_string(),
                            Some(
                                "Use `research-cli memory explain --record-id <id> --json`."
                                    .to_string(),
                            ),
                        )
                    })
                    .map_err(|err| err.with_output_json(parsed.output_json))?;
                let result = memory::explain(&resolved.data_dir, &record_id).map_err(|err| {
                    internal_failure("memory explain", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "memory explain",
                    resolved.project_id,
                    serde_json::to_value(result).expect("memory explain result should serialize"),
                ))
            }
            Some("invalidate") => {
                let resolved = self
                    .resolve_project(registry, cwd, parsed, "memory invalidate")
                    .map_err(|err| {
                        project_resolution_failure("memory invalidate", err)
                            .with_output_json(parsed.output_json)
                    })?;
                let record_id = parse_flag_value(&args[1..], "--record-id")
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "memory invalidate".to_string(),
                            "usage_invalid",
                            "memory invalidate requires --record-id".to_string(),
                            Some("Use `research-cli memory invalidate --record-id <id> --reason <text> --json`.".to_string()),
                        )
                    })
                    .map_err(|err| err.with_output_json(parsed.output_json))?;
                let reason = parse_flag_value(&args[1..], "--reason")
                    .ok_or_else(|| {
                        CommandFailureOutcome::usage(
                            "memory invalidate".to_string(),
                            "usage_invalid",
                            "memory invalidate requires --reason".to_string(),
                            Some("Use `research-cli memory invalidate --record-id <id> --reason <text> --json`.".to_string()),
                        )
                    })
                    .map_err(|err| err.with_output_json(parsed.output_json))?;
                let status = parse_flag_value(&args[1..], "--status")
                    .unwrap_or_else(|| "invalidated".to_string());
                let result = memory::invalidate(&resolved.data_dir, &record_id, &status, &reason)
                    .map_err(|err| {
                    internal_failure("memory invalidate", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "memory invalidate",
                    resolved.project_id,
                    serde_json::to_value(result)
                        .expect("memory invalidate result should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("memory {other}"),
                "usage_invalid",
                format!("unknown memory subcommand: {other}"),
                Some(
                    "Choose one of: append, trajectory, adoption, tier, status, promote, query, explain, invalidate."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "memory".to_string(),
                "usage_invalid",
                "missing memory subcommand".to_string(),
                Some(
                    "Choose one of: append, trajectory, adoption, tier, status, promote, query, explain, invalidate."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_feedback(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project_or_workspace(registry, cwd, parsed, "feedback")
            .map_err(|err| {
                project_resolution_failure("feedback", err).with_output_json(parsed.output_json)
            })?;
        match args.first().map(String::as_str) {
            Some("collect") => {
                reject_unknown_flags(
                    "feedback collect",
                    &args[1..],
                    &["--json"],
                    parsed.output_json,
                )?;
                let result = feedback::collect(&resolved.data_dir).map_err(|err| {
                    internal_failure("feedback collect", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "feedback collect",
                    resolved.project_id,
                    serde_json::to_value(result).expect("feedback collect should serialize"),
                ))
            }
            Some("calibrate") => {
                reject_unknown_flags(
                    "feedback calibrate",
                    &args[1..],
                    &["--json"],
                    parsed.output_json,
                )?;
                let result = feedback::calibrate(&resolved.data_dir).map_err(|err| {
                    internal_failure("feedback calibrate", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "feedback calibrate",
                    resolved.project_id,
                    serde_json::to_value(result).expect("feedback calibrate should serialize"),
                ))
            }
            Some("status") => {
                reject_unknown_flags(
                    "feedback status",
                    &args[1..],
                    &["--json"],
                    parsed.output_json,
                )?;
                let result = feedback::status(&resolved.data_dir).map_err(|err| {
                    internal_failure("feedback status", err.to_string())
                        .with_output_json(parsed.output_json)
                })?;
                Ok(CommandSuccess::with_project(
                    "feedback status",
                    resolved.project_id,
                    serde_json::to_value(result).expect("feedback status should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("feedback {other}"),
                "usage_invalid",
                format!("unknown feedback subcommand: {other}"),
                Some("Choose one of: collect, calibrate, status.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "feedback".to_string(),
                "usage_invalid",
                "missing feedback subcommand".to_string(),
                Some("Choose one of: collect, calibrate, status.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

}
