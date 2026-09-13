impl Runtime {
    fn handle_resume(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if args.len() > 1 || args.iter().any(|arg| arg.starts_with("--")) {
            return Err(CommandFailureOutcome::usage(
                "resume".to_string(),
                "usage_invalid",
                "resume accepts at most one session selector".to_string(),
                Some("Use `research-cli resume <session-id|prefix|latest> --json`.".to_string()),
            ));
        }

        self.handle_resume_command(
            registry,
            cwd,
            parsed,
            "resume",
            args.first().map(String::as_str),
        )
    }

    fn handle_continue(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if !args.is_empty() {
            return Err(CommandFailureOutcome::usage(
                "continue".to_string(),
                "usage_invalid",
                "continue does not accept positional arguments".to_string(),
                Some(
                    "Use `research-cli continue --json` to resume the latest session.".to_string(),
                ),
            ));
        }

        self.handle_resume_command(registry, cwd, parsed, "continue", Some("latest"))
    }

    fn handle_resume_command(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        command: &str,
        selector_raw: Option<&str>,
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let resolved = self
            .resolve_project(registry, cwd, parsed, command)
            .map_err(|err| project_resolution_failure(command, err))?;
        let store = SessionStore::new(resolved.data_dir.clone());
        let selector_label = selector_raw.unwrap_or("latest");
        let session = store
            .resolve_resume_target(parse_resume_selector(selector_label))
            .map_err(|err| {
                session_store_failure(command, err, &resolved, Some(selector_label.to_string()))
            })?;
        let stored = store.load_session(&session.session_id).map_err(|err| {
            session_store_failure(command, err, &resolved, Some(session.session_id.clone()))
        })?;
        let recap = store
            .build_resume_recap_for_session(&session.session_id)
            .map_err(|err| {
                session_store_failure(command, err, &resolved, Some(session.session_id.clone()))
            })?;
        let mission_frame = crate::goals::projection(
            &resolved.data_dir,
            &resolved.workspace_root,
            &resolved.project_id,
        )
        .map_err(|err| goals_failure(command, err, &resolved, parsed.output_json))?;
        let doc_context = crate::docs::context_projection(
            &resolved.data_dir,
            &resolved.workspace_root,
            &resolved.project_id,
        )
        .map_err(|err| docs_failure(command, err, &resolved, parsed.output_json))?;
        let research_context =
            crate::research::projection(&resolved.data_dir, &resolved.project_id)
                .map_err(|err| research_failure(command, err, &resolved, parsed.output_json))?;
        let skill_outputs = skills::context_projection(&resolved.data_dir).map_err(|err| {
            skill_output_failure(
                command,
                err,
                Some(resolved.project_id.clone()),
                parsed.output_json,
            )
        })?;

        registry
            .touch_project(&resolved.project_id, Some(session.session_id.clone()))
            .map_err(|err| internal_failure(command, err.to_string()))?;

        let data = json!({
            "session_id": session.session_id,
            "project_id": resolved.project_id,
            "session_kind": stored.identity.session_kind,
            "recap": recap,
            "lineage": stored.lineage,
            "mission_frame": mission_frame,
            "doc_context": doc_context,
            "research_context": research_context,
            "skill_outputs": skill_outputs
        });
        let session_id = data["session_id"]
            .as_str()
            .expect("resume result should expose session id")
            .to_string();
        emit_session_resume_event(&resolved, command, &session_id, data.clone())
            .map_err(|err| internal_failure(command, err))?;
        publish_project_checkpoint(&resolved, &["session"])
            .map_err(|err| internal_failure(command, err))?;

        Ok(CommandSuccess::with_project_and_session(
            command,
            resolved.project_id.clone(),
            session_id,
            data,
        ))
    }

    fn handle_inspect(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let inspect_project = has_flag(args, "--project");
        let unknown_flags: Vec<String> = args
            .iter()
            .filter(|arg| arg.starts_with("--") && arg.as_str() != "--project")
            .cloned()
            .collect();
        let positionals: Vec<&String> = args.iter().filter(|arg| !arg.starts_with("--")).collect();

        if !unknown_flags.is_empty() {
            return Err(CommandFailureOutcome::usage(
                "inspect".to_string(),
                "usage_invalid",
                format!("unknown flag(s): {}", unknown_flags.join(", ")),
                Some("Use only `--project` with `inspect`.".to_string()),
            ));
        }

        if inspect_project && !positionals.is_empty() {
            return Err(CommandFailureOutcome::usage(
                "inspect".to_string(),
                "usage_invalid",
                "inspect --project cannot be combined with a session selector".to_string(),
                Some("Use either `research-cli inspect --project --json` or `research-cli inspect <session-id> --json`.".to_string()),
            ));
        }

        if positionals.len() > 1 {
            return Err(CommandFailureOutcome::usage(
                "inspect".to_string(),
                "usage_invalid",
                "inspect accepts at most one session selector".to_string(),
                Some("Pass a single session id, prefix, or `latest`.".to_string()),
            ));
        }

        let resolved = self
            .resolve_project(registry, cwd, parsed, "inspect")
            .map_err(|err| project_resolution_failure("inspect", err))?;
        let store = SessionStore::new(resolved.data_dir.clone());

        if inspect_project {
            let entry = registry
                .get_by_project_id(&resolved.project_id)
                .map_err(|err| internal_failure("inspect", err.to_string()))?
                .ok_or_else(|| {
                    internal_failure(
                        "inspect",
                        format!("missing registry entry for project {}", resolved.project_id),
                    )
                })?;
            let degraded_features = entry
                .repo_health
                .as_deref()
                .filter(|value| *value != "healthy")
                .map(|_| vec!["repo_health".to_string()])
                .unwrap_or_default();
            let project_inspection = ProjectInspection {
                project_id: entry.project_id.clone(),
                registry_entry: serde_json::to_value(&entry)
                    .expect("project registry entry should serialize"),
                init_state: entry.init_state.clone(),
                active_session_id: entry.active_session_id.clone(),
                project_continuity: Some(
                    serde_json::to_value(autonomous_research_project_continuity_snapshot(
                        &resolved,
                        entry.active_session_id.clone(),
                    ))
                    .expect("project continuity snapshot should serialize"),
                ),
                degraded_features,
                section_status: json!({
                    "projectops": "available",
                    "memory": "durable_memory_available",
                    "branches": "guarded",
                    "reviews": "available",
                    "active_runs": "not_graduated",
                    "cleanup": "available",
                    "canonicality": "available"
                }),
            };
            let data = serde_json::to_value(project_inspection)
                .expect("project inspection should serialize");
            emit_inspection_event(
                &resolved,
                "project",
                &resolved.project_id,
                None,
                data.clone(),
            )
            .map_err(|err| internal_failure("inspect", err))?;
            return Ok(CommandSuccess::with_project(
                "inspect",
                resolved.project_id.clone(),
                data,
            ));
        }

        let selector_label = positionals
            .first()
            .map(|value| value.as_str())
            .unwrap_or("latest");
        let session = store
            .resolve_resume_target(parse_resume_selector(selector_label))
            .map_err(|err| {
                session_store_failure("inspect", err, &resolved, Some(selector_label.to_string()))
            })?;
        let inspection = store
            .inspect_session(&session.session_id, &resolved.project_id)
            .map_err(|err| {
                session_store_failure("inspect", err, &resolved, Some(session.session_id.clone()))
            })?;
        let data = serde_json::to_value(inspection).expect("session inspection should serialize");
        emit_inspection_event(
            &resolved,
            "session",
            &session.session_id,
            Some(session.session_id.clone()),
            data.clone(),
        )
        .map_err(|err| internal_failure("inspect", err))?;
        Ok(CommandSuccess::with_project_and_session(
            "inspect",
            resolved.project_id.clone(),
            session.session_id,
            data,
        ))
    }

    fn handle_compact(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        args: &[String],
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        if args.len() > 1 || args.iter().any(|arg| arg.starts_with("--")) {
            return Err(CommandFailureOutcome::usage(
                "compact".to_string(),
                "usage_invalid",
                "compact accepts at most one session selector".to_string(),
                Some("Use `research-cli compact [session-id|prefix|latest] --json`.".to_string()),
            ));
        }

        let resolved = self
            .resolve_project(registry, cwd, parsed, "compact")
            .map_err(|err| project_resolution_failure("compact", err))?;
        let store = SessionStore::new(resolved.data_dir.clone());
        let selector_label = args.first().map(String::as_str).unwrap_or("latest");
        let session = store
            .resolve_resume_target(parse_resume_selector(selector_label))
            .map_err(|err| {
                session_store_failure("compact", err, &resolved, Some(selector_label.to_string()))
            })?;
        let mission_frame = crate::goals::projection(
            &resolved.data_dir,
            &resolved.workspace_root,
            &resolved.project_id,
        )
        .map_err(|err| goals_failure("compact", err, &resolved, parsed.output_json))?;
        let doc_context = crate::docs::context_projection(
            &resolved.data_dir,
            &resolved.workspace_root,
            &resolved.project_id,
        )
        .map_err(|err| docs_failure("compact", err, &resolved, parsed.output_json))?;
        let research_context =
            crate::research::projection(&resolved.data_dir, &resolved.project_id)
                .map_err(|err| research_failure("compact", err, &resolved, parsed.output_json))?;
        let skill_outputs = skills::context_projection(&resolved.data_dir).map_err(|err| {
            skill_output_failure(
                "compact",
                err,
                Some(resolved.project_id.clone()),
                parsed.output_json,
            )
        })?;
        let compacted = store
            .compact_session(
                &session.session_id,
                &resolved.project_id,
                mission_frame,
                Some(doc_context),
                research_context,
                Some(skill_outputs),
            )
            .map_err(|err| {
                session_store_failure("compact", err, &resolved, Some(session.session_id.clone()))
            })?;

        registry
            .touch_project(&resolved.project_id, Some(session.session_id.clone()))
            .map_err(|err| internal_failure("compact", err.to_string()))?;

        let data = serde_json::to_value(compacted).expect("compact result should serialize");
        emit_session_compaction_event(&resolved, &session.session_id, data.clone())
            .map_err(|err| internal_failure("compact", err))?;
        emit_goal_alignment_trace_event(
            &resolved,
            &session.session_id,
            goal_alignment_trace_payload(&resolved, &session.session_id, "compact", &data),
        )
        .map_err(|err| internal_failure("compact", err))?;
        publish_project_checkpoint(&resolved, &["session"])
            .map_err(|err| internal_failure("compact", err))?;
        Ok(CommandSuccess::with_project_and_session(
            "compact",
            resolved.project_id.clone(),
            session.session_id,
            data,
        ))
    }

    fn handle_interactive_launch(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        command: &str,
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
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
                command.to_string(),
                None,
                None,
                "runtime_preflight_blocked",
                "runtime preflight blocked interactive launch".to_string(),
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
            project_resolution_failure_with_trace(command, err).with_output_json(parsed.output_json)
        })?;
        let project = resolved.resolved.clone();
        let store = SessionStore::new(project.data_dir.clone());
        let session = store
            .create_session(Some("Interactive Session".to_string()))
            .map_err(|err| {
                session_store_failure(command, err, &project, None)
                    .with_output_json(parsed.output_json)
            })?;
        let stored = store.load_session(&session.session_id).map_err(|err| {
            session_store_failure(command, err, &project, Some(session.session_id.clone()))
                .with_output_json(parsed.output_json)
        })?;

        registry
            .touch_project(&project.project_id, Some(session.session_id.clone()))
            .map_err(|err| {
                internal_failure(command, err.to_string()).with_output_json(parsed.output_json)
            })?;

        let profile_trace = resolve_profile_trace(parsed.profile.as_deref());
        let launch_result = InteractiveLaunchResult {
            session_id: session.session_id.clone(),
            project_id: project.project_id.clone(),
            workspace_root: project.workspace_root.display().to_string(),
            preflight: doctor.preflight,
            project_trace: Some(resolved.trace),
            profile_trace: Some(profile_trace),
            launch_disposition: if parsed.output_json {
                "inspect_exit".to_string()
            } else {
                "interactive_skeleton_exit".to_string()
            },
            interactive_eligible: true,
            reused_session_id: String::new(),
        };

        emit_runtime_preflight_event(
            &project,
            command,
            &session.session_id,
            &launch_result.preflight,
        )
        .map_err(|err| internal_failure(command, err).with_output_json(parsed.output_json))?;
        emit_session_open_event(
            &project,
            &session.session_id,
            &stored.transcript_path.display().to_string(),
        )
        .map_err(|err| internal_failure(command, err).with_output_json(parsed.output_json))?;
        publish_project_checkpoint(&project, &["session"])
            .map_err(|err| internal_failure(command, err).with_output_json(parsed.output_json))?;

        Ok(CommandSuccess::with_project_and_session(
            command,
            project.project_id.clone(),
            session.session_id,
            serde_json::to_value(launch_result).expect("launch result should serialize"),
        ))
    }

    fn resolve_project(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        command: &str,
    ) -> Result<ResolvedProject, ResolveCurrentProjectError> {
        let _ = command;
        resolve_current_project(
            registry,
            parsed.project.as_deref(),
            parsed.cwd.as_deref(),
            cwd,
        )
    }

    fn resolve_project_or_workspace(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
        command: &str,
    ) -> Result<ResolvedProject, ResolveCurrentProjectError> {
        if parsed.project.is_none() {
            let scoped_cwd = command_cwd(cwd, parsed);
            if let Ok(binding) = resolve_workspace_from(scoped_cwd) {
                match registry.get_by_workspace_root(&binding.workspace_root) {
                    Ok(Some(entry)) => {
                        return Ok(ResolvedProject {
                            project_id: entry.project_id,
                            workspace_root: entry.workspace_root,
                            workspace_hash: entry.workspace_hash,
                            data_dir: entry.data_dir,
                            resolution_source: "workspace_detection".to_string(),
                        });
                    }
                    Ok(None) => {
                        crate::workspace::resolve::ensure_git_workspace_baseline(
                            &binding.workspace_root,
                        )
                        .map_err(ResolveCurrentProjectError::Workspace)?;
                        let project_id = stable_project_id(&binding.workspace_root)
                            .map_err(ResolveCurrentProjectError::Registry)?;
                        let entry = registry
                            .entry_for_workspace(project_id, binding.workspace_root.clone())
                            .map_err(ResolveCurrentProjectError::Registry)?;
                        registry
                            .register(entry.clone())
                            .map_err(ResolveCurrentProjectError::Registry)?;
                        registry
                            .set_current_project(entry.to_pointer())
                            .map_err(ResolveCurrentProjectError::Registry)?;
                        return Ok(ResolvedProject {
                            project_id: entry.project_id,
                            workspace_root: entry.workspace_root,
                            workspace_hash: entry.workspace_hash,
                            data_dir: entry.data_dir,
                            resolution_source: "workspace_autoregistered".to_string(),
                        });
                    }
                    Err(err) => return Err(ResolveCurrentProjectError::Registry(err)),
                }
            }
            let binding = resolve_or_create_workspace_from(scoped_cwd)
                .map_err(ResolveCurrentProjectError::Workspace)?;
            crate::workspace::resolve::ensure_git_workspace_baseline(&binding.workspace_root)
                .map_err(ResolveCurrentProjectError::Workspace)?;
            let project_id = stable_project_id(&binding.workspace_root)
                .map_err(ResolveCurrentProjectError::Registry)?;
            let entry = registry
                .entry_for_workspace(project_id, binding.workspace_root.clone())
                .map_err(ResolveCurrentProjectError::Registry)?;
            registry
                .register(entry.clone())
                .map_err(ResolveCurrentProjectError::Registry)?;
            registry
                .set_current_project(entry.to_pointer())
                .map_err(ResolveCurrentProjectError::Registry)?;
            return Ok(ResolvedProject {
                project_id: entry.project_id,
                workspace_root: entry.workspace_root,
                workspace_hash: entry.workspace_hash,
                data_dir: entry.data_dir,
                resolution_source: "workspace_autoregistered".to_string(),
            });
        }
        self.resolve_project(registry, cwd, parsed, command)
    }
}
