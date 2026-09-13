impl Runtime {
    fn handle_help(
        &self,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if !args.is_empty() {
            return Err(CommandFailureOutcome::usage(
                "help".to_string(),
                "usage_invalid",
                "help does not accept positional arguments yet".to_string(),
                Some("Use `research-cli help --json` or `research-cli --help`.".to_string()),
            )
            .with_output_json(parsed.output_json));
        }

        Ok(CommandSuccess::new_with_data(
            "help",
            serde_json::to_value(help_surface_report("cli"))
                .expect("help surface report should serialize"),
        ))
    }

    fn handle_palette(
        &self,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if !args.is_empty() {
            return Err(CommandFailureOutcome::usage(
                "palette".to_string(),
                "usage_invalid",
                "palette does not accept positional arguments".to_string(),
                Some("Use `research-cli palette --json`.".to_string()),
            )
            .with_output_json(parsed.output_json));
        }

        Ok(CommandSuccess::new_with_data(
            "palette",
            serde_json::to_value(palette_surface_report("cli"))
                .expect("palette surface report should serialize"),
        ))
    }

    fn handle_slash(
        &self,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        match args.first().map(String::as_str) {
            Some("help") if args.len() == 1 => Ok(CommandSuccess::new_with_data(
                "slash help",
                serde_json::to_value(help_surface_report("slash"))
                    .expect("slash help surface should serialize"),
            )),
            Some("help") => Err(CommandFailureOutcome::usage(
                "slash help".to_string(),
                "usage_invalid",
                "slash help does not accept positional arguments".to_string(),
                Some("Use `research-cli slash help --json`.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("slash {other}"),
                "usage_invalid",
                format!("unknown slash subcommand: {other}"),
                Some("Choose one of: help.".to_string()),
            )
            .with_output_json(parsed.output_json)),
            None => Err(CommandFailureOutcome::usage(
                "slash".to_string(),
                "usage_invalid",
                "missing slash subcommand".to_string(),
                Some("Choose one of: help.".to_string()),
            )
            .with_output_json(parsed.output_json)),
        }
    }

    fn handle_chat(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if !args.is_empty() {
            return Err(CommandFailureOutcome::usage(
                "chat".to_string(),
                "usage_invalid",
                "chat does not accept positional arguments".to_string(),
                Some("Use `research-cli chat --json` for launch inspection.".to_string()),
            )
            .with_output_json(parsed.output_json));
        }

        if !parsed.output_json {
            return Err(feature_not_graduated_failure(
                "chat",
                "runtime.interactive_text_repl",
                "M2.interactive_text_repl",
                vec![
                    "Use `research-cli chat --json` for the typed launch/preflight lane."
                        .to_string(),
                    "Graduate the interactive REPL transport before using text chat.".to_string(),
                ],
            )
            .with_output_json(parsed.output_json));
        }

        self.handle_interactive_launch(registry, cwd, parsed, "chat")
    }

    fn handle_run(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let action_text = args.first().map(String::as_str).unwrap_or("status");
        let action = RunControlAction::from_str(action_text).ok_or_else(|| {
            CommandFailureOutcome::usage(
                "run".to_string(),
                "usage_invalid",
                format!("unknown run subcommand: {action_text}"),
                Some(
                    "Choose one of: status, pause, resume, retry, skip, replan, accept, abort."
                        .to_string(),
                ),
            )
            .with_output_json(parsed.output_json)
        })?;
        let step_id = args.get(1).filter(|value| !value.starts_with("--"));
        if args
            .iter()
            .skip(if step_id.is_some() { 2 } else { 1 })
            .any(|arg| arg != "--json")
        {
            return Err(CommandFailureOutcome::usage(
                format!("run {}", action.as_str()),
                "usage_invalid",
                "run control accepts at most one optional step id".to_string(),
                Some(format!(
                    "Use `research-cli run {} [step-id] --json`.",
                    action.as_str()
                )),
            )
            .with_output_json(parsed.output_json));
        }

        let resolved = self
            .resolve_project_or_workspace(registry, cwd, parsed, "run")
            .map_err(|err| {
                project_resolution_failure("run", err).with_output_json(parsed.output_json)
            })?;
        let active_session_id = if action == RunControlAction::Status {
            active_session_id_for(registry, &resolved, "run status")
                .map_err(|err| err.with_output_json(parsed.output_json))?
        } else {
            None
        };
        let data = if action == RunControlAction::Status {
            let recovery = crate::orchestration::build_recovery_decision_context(
                &resolved.data_dir,
                &resolved.workspace_root,
                active_session_id.as_deref(),
            )
            .map_err(|err| run_failure("run status", &resolved, err, parsed.output_json))?;
            serde_json::to_value(recovery).expect("recovery context should serialize")
        } else {
            let result = crate::orchestration::apply_run_control(
                &resolved.data_dir,
                action,
                step_id.map(String::as_str),
                "operator",
            )
            .map_err(|err| run_failure("run", &resolved, err, parsed.output_json))?;
            serde_json::to_value(result).expect("run control result should serialize")
        };
        registry
            .touch_project(&resolved.project_id, None)
            .map_err(|err| {
                internal_failure("run", err.to_string()).with_output_json(parsed.output_json)
            })?;
        Ok(CommandSuccess::with_project(
            format!("run {}", action.as_str()).as_str(),
            resolved.project_id,
            data,
        ))
    }

    fn handle_prompt(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let invocation = parse_prompt_invocation(args)
            .map_err(|err| err.with_output_json(parsed.output_json))?;
        let scoped_cwd = command_cwd(cwd, parsed);
        let doctor = DoctorService::run(
            registry,
            scoped_cwd,
            parsed.project.as_deref(),
            parsed.cwd.as_deref(),
        );

        if doctor.overall_status == "blocked" {
            return Err(CommandFailureOutcome::new(
                4,
                "prompt".to_string(),
                None,
                None,
                "runtime_preflight_blocked",
                "runtime preflight blocked prompt execution".to_string(),
                doctor.repair_hints.first().cloned(),
                false,
                Some(serde_json::to_value(doctor.preflight).expect("preflight should serialize")),
                parsed.output_json,
            ));
        }

        let resolved = resolve_current_project_with_trace(
            registry,
            parsed.project.as_deref(),
            parsed.cwd.as_deref(),
            cwd,
        )
        .map_err(|err| {
            project_resolution_failure_with_trace("prompt", err)
                .with_output_json(parsed.output_json)
        })?;
        let project = resolved.resolved.clone();
        let profile_trace = resolve_profile_trace(parsed.profile.as_deref());
        let mission_frame = crate::goals::projection(
            &project.data_dir,
            &project.workspace_root,
            &project.project_id,
        )
        .map_err(|err| {
            goals_failure("prompt", err, &project, parsed.output_json)
                .with_output_json(parsed.output_json)
        })?;
        let doc_context = crate::docs::context_projection(
            &project.data_dir,
            &project.workspace_root,
            &project.project_id,
        )
        .map_err(|err| {
            docs_failure("prompt", err, &project, parsed.output_json)
                .with_output_json(parsed.output_json)
        })?;
        let prompt_defaults =
            resolve_prompt_request_defaults(&project, &invocation).map_err(|err| {
                config_error_failure("prompt", err, None).with_output_json(parsed.output_json)
            })?;
        let provider_trace = resolve_prompt_provider_trace(
            prompt_defaults.provider.as_deref(),
            prompt_defaults.model.as_deref(),
            prompt_defaults.provider_source.as_deref(),
            prompt_defaults.model_source.as_deref(),
        )
        .map_err(|err| err.with_output_json(parsed.output_json))?;

        let store = SessionStore::new(project.data_dir.clone());
        let (session, session_open_payload) = if !invocation.new_session {
            if let Some(session_id) = invocation.session_id.as_deref() {
                let session = store
                    .resolve_resume_target(ResumeSelector::Exact(session_id.to_string()))
                    .map_err(|err| {
                        session_store_failure("prompt", err, &project, Some(session_id.to_string()))
                            .with_output_json(parsed.output_json)
                    })?;
                (session, None)
            } else {
                let title = Some(prompt_session_title(&invocation.prompt_text));
                let session = store
                    .create_session_with_kind(title, "one_shot")
                    .map_err(|err| {
                        session_store_failure("prompt", err, &project, None)
                            .with_output_json(parsed.output_json)
                    })?;
                let stored = store.load_session(&session.session_id).map_err(|err| {
                    session_store_failure("prompt", err, &project, Some(session.session_id.clone()))
                        .with_output_json(parsed.output_json)
                })?;
                (session, Some(stored.transcript_path.display().to_string()))
            }
        } else {
            let title = Some(prompt_session_title(&invocation.prompt_text));
            let session = store
                .create_session_with_kind(title, "one_shot")
                .map_err(|err| {
                    session_store_failure("prompt", err, &project, None)
                        .with_output_json(parsed.output_json)
                })?;
            let stored = store.load_session(&session.session_id).map_err(|err| {
                session_store_failure("prompt", err, &project, Some(session.session_id.clone()))
                    .with_output_json(parsed.output_json)
            })?;
            (session, Some(stored.transcript_path.display().to_string()))
        };

        registry
            .touch_project(&project.project_id, Some(session.session_id.clone()))
            .map_err(|err| {
                internal_failure("prompt", err.to_string()).with_output_json(parsed.output_json)
            })?;

        let turn_id = format!("turn_{}", timestamp_string());
        let permission_request_id = format!("perm_{}", timestamp_string());
        let mutation_requested = prompt_requests_mutation(&invocation.prompt_text);
        let requires_permission =
            mutation_requested && prompt_defaults.permission_mode == "read-only";

        emit_runtime_preflight_event(&project, "prompt", &session.session_id, &doctor.preflight)
            .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
        if let Some(transcript_path) = session_open_payload.as_deref() {
            emit_session_open_event(&project, &session.session_id, transcript_path).map_err(
                |err| internal_failure("prompt", err).with_output_json(parsed.output_json),
            )?;
        }
        emit_provider_resolution_event(
            &project,
            &session.session_id,
            &turn_id,
            &provider_trace,
            &prompt_defaults,
        )
        .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
        emit_turn_event(
            &project,
            &session.session_id,
            &turn_id,
            "start",
            None,
            json!({
                "turn_id": turn_id,
                "status": if requires_permission { "awaiting_permission" } else { "running" },
                "input": invocation.prompt_text
            }),
        )
        .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;

        if requires_permission {
            let requested_at = timestamp_string();
            let expires_at_ms = permission_request_expires_at_ms();
            emit_permission_event(
                &project,
                &session.session_id,
                &permission_request_id,
                "start",
                None,
                json!({
                    "request_id": permission_request_id,
                    "session_id": session.session_id,
                    "turn_id": turn_id,
                    "tool_name": "prompt_execution",
                    "permission_mode": prompt_defaults.permission_mode,
                    "reason": "prompt requested a mutating action under read-only mode",
                    "requested_at": requested_at,
                    "expires_at_ms": expires_at_ms,
                    "status": "pending"
                }),
            )
            .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
            publish_project_checkpoint(&project, &["session", "turn", "permission"]).map_err(
                |err| internal_failure("prompt", err).with_output_json(parsed.output_json),
            )?;

            return Err(CommandFailureOutcome::new(
                5,
                "prompt".to_string(),
                Some(project.project_id.clone()),
                Some(session.session_id.clone()),
                "permission_unresolved",
                "prompt requires permission approval before execution".to_string(),
                Some(
                    "Inspect `research-cli permissions pending --json` or retry with `--permission-mode read-only`.".to_string(),
                ),
                false,
                Some(json!({
                    "request_id": permission_request_id,
                    "permission_mode": prompt_defaults.permission_mode,
                    "expires_at_ms": expires_at_ms,
                    "turn_id": turn_id,
                    "session_id": session.session_id
                })),
                parsed.output_json,
            ));
        }

        let context_messages = build_context_messages(
            &store,
            &project.project_id,
            &session.session_id,
            &invocation.prompt_text,
            &provider_trace.resolved_model,
        );

        if !crate::providers::should_execute_live_prompt(&provider_trace) {
            let completion = crate::providers::complete_prompt_streaming_with_tools(
                &provider_trace,
                &context_messages,
                prompt_defaults.reasoning_effort.as_deref(),
                None,
                |_| {},
                || false,
            )
            .map_err(|err| {
                internal_failure("prompt", err.to_string()).with_output_json(parsed.output_json)
            })?;
            let provider_completion = ProviderPromptCompletion {
                content: completion.content.clone(),
                execution_mode: completion.execution_mode,
                endpoint: completion.endpoint,
                tool_calls: completion.tool_calls,
                finish_reason: completion.finish_reason,
            };
            store
                .append_line(
                    &session.session_id,
                    TranscriptLine::Message {
                        role: "user".to_string(),
                        content: invocation.prompt_text.clone(),
                    },
                )
                .map_err(|err| {
                    session_store_failure("prompt", err, &project, Some(session.session_id.clone()))
                        .with_output_json(parsed.output_json)
                })?;
            store
                .append_line(
                    &session.session_id,
                    TranscriptLine::Message {
                        role: "assistant".to_string(),
                        content: provider_completion.content.clone(),
                    },
                )
                .map_err(|err| {
                    session_store_failure("prompt", err, &project, Some(session.session_id.clone()))
                        .with_output_json(parsed.output_json)
                })?;

            emit_turn_event(
                &project,
                &session.session_id,
                &turn_id,
                "terminal",
                Some("succeeded"),
                json!({
                    "turn_id": turn_id,
                    "status": "succeeded",
                    "outcome": "completed",
                    "provider_execution": {
                        "execution_mode": provider_completion.execution_mode,
                        "endpoint": provider_completion.endpoint
                    }
                }),
            )
            .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
            let mission_frame_value =
                serde_json::to_value(&mission_frame).expect("mission frame should serialize");
            emit_goal_alignment_trace_event(
                &project,
                &session.session_id,
                goal_alignment_trace_payload_from_text(
                    &project,
                    &session.session_id,
                    "prompt",
                    mission_frame
                        .as_ref()
                        .map(|frame| frame.mission_frame_ref.as_str())
                        .unwrap_or_default(),
                    Some(&mission_frame_value),
                    &invocation.prompt_text,
                    "prompt_text",
                ),
            )
            .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
            publish_project_checkpoint(&project, &["session", "turn"]).map_err(|err| {
                internal_failure("prompt", err).with_output_json(parsed.output_json)
            })?;
            let research_classification = crate::research::observe_turn(
                &project.data_dir,
                &project.project_id,
                &turn_id,
                &invocation.prompt_text,
            )
            .map_err(|err| {
                research_failure("prompt", err, &project, parsed.output_json)
                    .with_output_json(parsed.output_json)
            })?;
            let research_context =
                crate::research::projection(&project.data_dir, &project.project_id).map_err(
                    |err| {
                        research_failure("prompt", err, &project, parsed.output_json)
                            .with_output_json(parsed.output_json)
                    },
                )?;
            let skill_outputs = skills::context_projection(&project.data_dir).map_err(|err| {
                skill_output_failure(
                    "prompt",
                    err,
                    Some(project.project_id.clone()),
                    parsed.output_json,
                )
            })?;

            let turn_result = TurnResult {
                session_id: session.session_id.clone(),
                turn_id: turn_id.clone(),
                project_id: project.project_id.clone(),
                outcome: TURN_OUTCOME_COMPLETED.to_string(),
                project_trace: Some(resolved.trace),
                profile_trace: Some(profile_trace),
                mission_frame,
                doc_context: Some(doc_context),
                research_context,
                skill_outputs: Some(skill_outputs),
                research_classification,
                provider_trace,
                assistant_content: provider_completion.content,
                agent_iterations: None,
                agent_tool_calls: None,
            };

            return Ok(CommandSuccess::with_project_and_session(
                "prompt",
                project.project_id.clone(),
                session.session_id,
                serde_json::to_value(turn_result).expect("turn result should serialize"),
            ));
        }

        // Agent loop: multi-step tool-calling
        let tool_defs =
            crate::tools::builtin_tool_definitions_for_mode(&prompt_defaults.permission_mode);
        let tool_defs_json = crate::tools::tool_definitions_to_openai_json(&tool_defs);
        let tool_executor = LocalToolExecutor::new(&project.workspace_root);
        let perm_mode = crate::permissions::PermissionMode::parse(&prompt_defaults.permission_mode)
            .unwrap_or(crate::permissions::PermissionMode::ReadOnly);
        let permission_policy =
            crate::permissions::PermissionPolicy::new(perm_mode, &project.workspace_root);
        self::cancel::install_agent_sigint_handler();
        let (cancel_token, cancel_handle) = self::cancel::runtime_interrupt_pair();
        let ch = cancel_handle.clone();
        std::thread::spawn(move || {
            while !ch.is_cancelled() {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if self::cancel::sigint_requested() {
                    ch.interrupt();
                    break;
                }
            }
        });
        let mut stdout_delta = |delta: &str| {
            if !parsed.output_json {
                print!("{delta}");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
        };
        let mut stderr_tool_start = |name: &str, _id: &str| {
            eprintln!("  ▸ {name}");
        };
        let mut stderr_tool_result =
            |event: crate::runtime::agent_loop::AgentLoopToolResultEvent| {
                eprintln!("  ✓ {}: {}", event.tool_name, event.status);
        };

        let agent_result = run_agent_loop(
            &provider_trace,
            &context_messages,
            tool_defs_json,
            &tool_executor,
            &permission_policy,
            &store,
            &session.session_id,
            &cancel_token,
            &mut stdout_delta,
            &mut stderr_tool_start,
            &mut stderr_tool_result,
            prompt_defaults.reasoning_effort.as_deref(),
        )
        .map_err(|err| {
            internal_failure("prompt", err.to_string()).with_output_json(parsed.output_json)
        })?;

        let provider_completion = ProviderPromptCompletion {
            content: agent_result.final_content.clone(),
            execution_mode: "agent_loop".to_string(),
            endpoint: String::new(),
            tool_calls: None,
            finish_reason: Some(agent_result.finish_reason),
        };
        store
            .append_line(
                &session.session_id,
                TranscriptLine::Message {
                    role: "user".to_string(),
                    content: invocation.prompt_text.clone(),
                },
            )
            .map_err(|err| {
                session_store_failure("prompt", err, &project, Some(session.session_id.clone()))
                    .with_output_json(parsed.output_json)
            })?;
        store
            .append_line(
                &session.session_id,
                TranscriptLine::Message {
                    role: "assistant".to_string(),
                    content: provider_completion.content.clone(),
                },
            )
            .map_err(|err| {
                session_store_failure("prompt", err, &project, Some(session.session_id.clone()))
                    .with_output_json(parsed.output_json)
            })?;

        emit_turn_event(
            &project,
            &session.session_id,
            &turn_id,
            "terminal",
            Some("succeeded"),
            json!({
                "turn_id": turn_id,
                "status": "succeeded",
                "outcome": "completed",
                "provider_execution": {
                    "execution_mode": provider_completion.execution_mode,
                    "endpoint": provider_completion.endpoint
                }
            }),
        )
        .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
        let mission_frame_value =
            serde_json::to_value(&mission_frame).expect("mission frame should serialize");
        emit_goal_alignment_trace_event(
            &project,
            &session.session_id,
            goal_alignment_trace_payload_from_text(
                &project,
                &session.session_id,
                "prompt",
                mission_frame
                    .as_ref()
                    .map(|frame| frame.mission_frame_ref.as_str())
                    .unwrap_or_default(),
                Some(&mission_frame_value),
                &invocation.prompt_text,
                "prompt_text",
            ),
        )
        .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
        publish_project_checkpoint(&project, &["session", "turn"])
            .map_err(|err| internal_failure("prompt", err).with_output_json(parsed.output_json))?;
        let research_classification = crate::research::observe_turn(
            &project.data_dir,
            &project.project_id,
            &turn_id,
            &invocation.prompt_text,
        )
        .map_err(|err| {
            research_failure("prompt", err, &project, parsed.output_json)
                .with_output_json(parsed.output_json)
        })?;
        let research_context = crate::research::projection(&project.data_dir, &project.project_id)
            .map_err(|err| {
                research_failure("prompt", err, &project, parsed.output_json)
                    .with_output_json(parsed.output_json)
            })?;
        let skill_outputs = skills::context_projection(&project.data_dir).map_err(|err| {
            skill_output_failure(
                "prompt",
                err,
                Some(project.project_id.clone()),
                parsed.output_json,
            )
        })?;

        let turn_result = TurnResult {
            session_id: session.session_id.clone(),
            turn_id: turn_id.clone(),
            project_id: project.project_id.clone(),
            outcome: TURN_OUTCOME_COMPLETED.to_string(),
            project_trace: Some(resolved.trace),
            profile_trace: Some(profile_trace),
            mission_frame,
            doc_context: Some(doc_context),
            research_context,
            skill_outputs: Some(skill_outputs),
            research_classification,
            provider_trace,
            assistant_content: provider_completion.content,
            agent_iterations: None,
            agent_tool_calls: None,
        };

        Ok(CommandSuccess::with_project_and_session(
            "prompt",
            project.project_id.clone(),
            session.session_id,
            serde_json::to_value(turn_result).expect("turn result should serialize"),
        ))
    }

    fn handle_model(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, "model")
            .map_err(|err| project_resolution_failure("model", err))?;
        match args.first().map(String::as_str) {
            Some("list") => {
                let current = resolve_model_current_result(&resolved).map_err(|err| {
                    config_error_failure("model list", err, Some("default_model".to_string()))
                })?;
                Ok(CommandSuccess::new_with_data(
                    "model list",
                    serde_json::to_value(list_models(
                        Some(&current.provider_id),
                        Some(&current.model),
                    ))
                    .expect("model catalog list should serialize"),
                ))
            }
            Some("current") => Ok(CommandSuccess::new_with_data(
                "model current",
                serde_json::to_value(resolve_model_current_result(&resolved).map_err(|err| {
                    config_error_failure("model current", err, Some("default_model".to_string()))
                })?)
                .expect("model current result should serialize"),
            )),
            Some("set") => {
                let model = args.get(1).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "model set".to_string(),
                        "usage_invalid",
                        "missing model id".to_string(),
                        Some("Pass a model id such as `gpt-5.4`.".to_string()),
                    )
                })?;
                let scope = parse_flag_value(args, "--scope").ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "model set".to_string(),
                        "usage_invalid",
                        "missing --scope for model set".to_string(),
                        Some("Pass --scope global|project|private.".to_string()),
                    )
                })?;
                let parsed_scope = ConfigScope::parse(&scope).ok_or_else(|| {
                    CommandFailureOutcome::usage(
                        "model set".to_string(),
                        "usage_invalid",
                        format!("invalid config scope: {scope}"),
                        Some("Choose one of: global, project, private.".to_string()),
                    )
                })?;
                let result =
                    config_set(&resolved, "default_model", model, parsed_scope).map_err(|err| {
                        config_error_failure("model set", err, Some("default_model".to_string()))
                    })?;
                emit_config_event(&resolved, "default_model", parsed_scope.as_str(), &result)
                    .map_err(|err| internal_failure("model set", err))?;
                Ok(CommandSuccess::new_with_data(
                    "model set",
                    serde_json::to_value(result).expect("model set result should serialize"),
                ))
            }
            Some(other) => Err(CommandFailureOutcome::usage(
                format!("model {other}"),
                "usage_invalid",
                format!("unknown model subcommand: {other}"),
                Some("Choose one of: list, current, set.".to_string()),
            )),
            None => Err(CommandFailureOutcome::usage(
                "model".to_string(),
                "usage_invalid",
                "missing model subcommand".to_string(),
                Some("Choose one of: list, current, set.".to_string()),
            )),
        }
    }

    fn handle_doctor(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let scoped_cwd = command_cwd(cwd, parsed);
        let report = DoctorService::run(
            registry,
            scoped_cwd,
            parsed.project.as_deref(),
            parsed.cwd.as_deref(),
        );

        if report.overall_status == "blocked" {
            return Err(CommandFailureOutcome::new(
                4,
                "doctor".to_string(),
                None,
                None,
                "doctor_blocked",
                "doctor detected blocked preflight state".to_string(),
                report.repair_hints.first().cloned(),
                false,
                Some(serde_json::to_value(report).expect("doctor report should serialize")),
                false,
            ));
        }

        Ok(CommandSuccess::new_with_data(
            "doctor",
            serde_json::to_value(report).expect("doctor report should serialize"),
        ))
    }

}
