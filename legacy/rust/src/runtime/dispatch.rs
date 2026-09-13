use super::*;

#[derive(Default)]
pub struct Runtime;

impl Runtime {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&self, args: &[String]) -> Result<i32, String> {
        let parsed = match parse_cli_args(args) {
            Ok(parsed) => parsed,
            Err(failure) => {
                emit_output(failure.output_json, &failure.envelope)
                    .map_err(|err| err.to_string())?;
                return Ok(failure.exit_code);
            }
        };

        if parsed.help_requested {
            let envelope = CommandSuccess::new_with_data(
                "help",
                serde_json::to_value(help_surface_report("cli"))
                    .expect("help surface report should serialize"),
            );
            emit_output(parsed.output_json, &envelope).map_err(|err| err.to_string())?;
            return Ok(0);
        }

        if parsed.version_requested {
            let envelope = CommandSuccess::new_with_data(
                "version",
                json!({
                    "name": "astra",
                    "version": env!("CARGO_PKG_VERSION")
                }),
            );
            emit_output(parsed.output_json, &envelope).map_err(|err| err.to_string())?;
            return Ok(0);
        }

        let cwd = env::current_dir().map_err(|err| err.to_string())?;
        let registry = ProjectRegistry::new(state_home()?.join("registry"));
        let audit_command = command_label_from_parsed(&parsed);
        let command_audit = prepare_command_audit(&registry, &cwd, &parsed, audit_command.as_str());
        if let Some(audit) = command_audit.as_ref() {
            let _ = emit_command_start(audit, audit_command.as_str());
        }

        if parsed.command_args.is_empty() {
            if parsed.continue_requested {
                return finalize_command_outcome(
                    command_audit.as_ref(),
                    parsed.output_json,
                    self.handle_continue(&registry, &cwd, &parsed, &[]),
                );
            }

            if parsed.output_json {
                return finalize_command_outcome(
                    command_audit.as_ref(),
                    parsed.output_json,
                    self.handle_interactive_launch(&registry, &cwd, &parsed, "launch"),
                );
            }

            return finalize_command_outcome(
                command_audit.as_ref(),
                parsed.output_json,
                self.handle_tui(&registry, &cwd, &parsed, &["launch".to_string()]),
            );
        }

        finalize_command_outcome(
            command_audit.as_ref(),
            parsed.output_json,
            self.dispatch(&registry, &cwd, &parsed),
        )
    }

    fn dispatch(
        &self,
        registry: &ProjectRegistry,
        cwd: &Path,
        parsed: &ParsedCliArgs,
    ) -> Result<CommandSuccess, CommandFailureOutcome> {
        let args = &parsed.command_args;
        match args.first().map(String::as_str) {
            Some("help") => self.handle_help(parsed, &args[1..]),
            Some("palette") => self.handle_palette(parsed, &args[1..]),
            Some("slash") => self.handle_slash(parsed, &args[1..]),
            Some("chat") => self.handle_chat(registry, cwd, parsed, &args[1..]),
            Some("model") => self.handle_model(registry, cwd, parsed, &args[1..]),
            Some("prompt") => self.handle_prompt(registry, cwd, parsed, &args[1..]),
            Some("resume") => self.handle_resume(registry, cwd, parsed, &args[1..]),
            Some("continue") => self.handle_continue(registry, cwd, parsed, &args[1..]),
            Some("inspect") => self.handle_inspect(registry, cwd, parsed, &args[1..]),
            Some("compact") => self.handle_compact(registry, cwd, parsed, &args[1..]),
            Some("run") => self.handle_run(registry, cwd, parsed, &args[1..]),
            Some("config") => self.handle_config(registry, cwd, parsed, &args[1..]),
            Some("permissions") => self.handle_permissions(registry, cwd, parsed, &args[1..]),
            Some("tools") => self.handle_tools(registry, cwd, parsed, &args[1..]),
            Some("usage") => self.handle_usage(registry, cwd, parsed, &args[1..]),
            Some("cost") => self.handle_cost(registry, cwd, parsed, &args[1..]),
            Some("stats") => self.handle_stats(registry, cwd, parsed, &args[1..]),
            Some("doctor") => self.handle_doctor(registry, cwd, parsed),
            Some("setup") => self.handle_setup(registry, cwd, parsed, &args[1..]),
            Some("providers") => self.handle_providers(registry, cwd, parsed, &args[1..]),
            Some("mcp") => self.handle_mcp(registry, cwd, parsed, &args[1..]),
            Some("skills") => self.handle_skills(registry, cwd, parsed, &args[1..]),
            Some("plugins") => self.handle_plugins(cwd, parsed, &args[1..]),
            Some("hooks") => self.handle_hooks(cwd, parsed, &args[1..]),
            Some("smoke") => self.handle_smoke(registry, cwd, parsed),
            Some("conformance") => self.handle_conformance(parsed, &args[1..]),
            Some("memory") => self.handle_memory(registry, cwd, parsed, &args[1..]),
            Some("feedback") => self.handle_feedback(registry, cwd, parsed, &args[1..]),
            Some("agents") => self.handle_agents(registry, cwd, parsed, &args[1..]),
            Some("branches") => self.handle_branches(registry, cwd, parsed, &args[1..]),
            Some("artifacts") => self.handle_artifacts(registry, cwd, parsed, &args[1..]),
            Some("repo") => self.handle_repo(registry, cwd, parsed, &args[1..]),
            Some("projectops") => self.handle_projectops(registry, cwd, parsed, &args[1..]),
            Some("host") => self.handle_host(registry, cwd, parsed, &args[1..]),
            Some("tui") => self.handle_tui(registry, cwd, parsed, &args[1..]),
            Some("remote") => self.handle_remote(registry, cwd, parsed, &args[1..]),
            Some("research") => self.handle_research(registry, cwd, parsed, &args[1..]),
            Some("reviews") => self.handle_reviews(registry, cwd, parsed, &args[1..]),
            Some("docs") => self.handle_docs(registry, cwd, parsed, &args[1..]),
            Some("routines") => self.handle_routines(registry, cwd, parsed, &args[1..]),
            Some("projects") => self.handle_projects(registry, cwd, parsed, &args[1..]),
            Some("goals") => self.handle_goals(registry, cwd, parsed, &args[1..]),
            Some("sessions") => self.handle_sessions(registry, cwd, parsed, &args[1..]),
            Some(other) => Err(CommandFailureOutcome::usage(
                other.to_string(),
                "usage_invalid",
                format!("unknown command: {other}"),
                Some("Run `research-cli help` to inspect available commands.".to_string()),
            )),
            None => Ok(CommandSuccess::new("noop")),
        }
    }
}

include!("dispatch/body.rs");

#[cfg(test)]
mod tests;
