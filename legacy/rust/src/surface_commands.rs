use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceCommandRequest {
    pub raw: String,
    pub kind: SurfaceCommandKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

impl SurfaceCommandRequest {
    pub fn target_id(&self) -> Option<&str> {
        self.action_id.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceCommandKind {
    PromptTurn,
    ProjectedAction,
    SkillList,
    SkillInspect,
    SkillRun,
    Help,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceCommandSpec {
    pub typed: String,
    pub action_id: String,
    pub label: String,
    pub category: String,
    pub summary: String,
}

pub fn product_command_specs() -> Vec<SurfaceCommandSpec> {
    command_specs(false)
}

fn all_surface_command_specs() -> Vec<SurfaceCommandSpec> {
    command_specs(true)
}

fn command_specs(include_advanced: bool) -> Vec<SurfaceCommandSpec> {
    let mut specs = vec![
        spec(
            "/help",
            "open_help",
            "Help",
            "navigation",
            "Show product command help",
        ),
        spec(
            "/palette",
            "open_palette",
            "Command Palette",
            "navigation",
            "Browse executable product commands",
        ),
        spec(
            "/chat",
            "submit_prompt",
            "Chat",
            "conversation",
            "Keep typing directly in the coding agent lane",
        ),
        spec(
            "/prompt <text>",
            "submit_prompt",
            "Prompt",
            "conversation",
            "Run an explicit governed prompt turn",
        ),
        spec(
            "/exc",
            "interrupt_turn",
            "Interrupt",
            "conversation",
            "Request interruption of the active turn",
        ),
        spec(
            "/exit",
            "exit_tui",
            "Exit",
            "conversation",
            "Close the interactive TUI",
        ),
        spec(
            "/sessions",
            "switch_session",
            "Sessions",
            "navigation",
            "Browse and switch saved sessions",
        ),
        spec(
            "/model",
            "select_model",
            "Model",
            "navigation",
            "Inspect or change model selection",
        ),
        spec(
            "/reasoning",
            "select_reasoning",
            "Reasoning",
            "configuration",
            "Inspect or change reasoning effort",
        ),
        spec(
            "/language <zh|en>",
            "select_language",
            "Language",
            "navigation",
            "Switch interface language",
        ),
        spec(
            "/theme <light|dark>",
            "select_theme",
            "Theme",
            "navigation",
            "Switch day or night theme",
        ),
        spec(
            "/permissions",
            "inspect_permissions",
            "Permissions",
            "permissions",
            "Inspect pending approvals and permission history",
        ),
        spec(
            "/approve <request-id>",
            "approve_permission",
            "Approve Permission",
            "permissions",
            "Approve a pending permission request",
        ),
        spec(
            "/deny <request-id>",
            "deny_permission",
            "Deny Permission",
            "permissions",
            "Deny a pending permission request",
        ),
        spec(
            "/terminal",
            "terminal_attach",
            "Terminal",
            "work",
            "Attach the governed terminal stream",
        ),
        spec(
            "/terminal replay",
            "terminal_replay",
            "Terminal Replay",
            "work",
            "Replay terminal scrollback for the active session",
        ),
        spec(
            "/fold",
            "fold_output",
            "Fold Output",
            "output",
            "Collapse long command, test, diff, and log blocks",
        ),
        spec(
            "/expand",
            "expand_output",
            "Expand Output",
            "output",
            "Expand collapsed output blocks in the current conversation",
        ),
        spec(
            "/output",
            "show_output",
            "Output",
            "output",
            "Show the latest structured command, test, diff, and error blocks",
        ),
        spec(
            "/logs",
            "show_logs",
            "Logs",
            "output",
            "Show recent structured log and streaming status blocks",
        ),
        spec(
            "/artifacts",
            "open_artifact",
            "Artifacts",
            "work",
            "Open governed artifacts, diffs, and result previews",
        ),
        spec(
            "/memory",
            "inspect_memory",
            "Memory",
            "context",
            "Inspect durable project and research memory",
        ),
        spec(
            "/research",
            "open_research",
            "Research Brief",
            "research",
            "Show active research thread, evidence, gap, and next action",
        ),
        spec(
            "/research board",
            "open_research_board",
            "Hermes Board",
            "research",
            "Inspect the derived Hermes research board projection",
        ),
        spec(
            "/routines",
            "open_routines",
            "Routines",
            "research",
            "Inspect background routines and supervised trigger history",
        ),
        spec(
            "/run status",
            "run_status",
            "Run Status",
            "orchestration",
            "Show active orchestration run progress and recovery context",
        ),
        spec(
            "/run pause",
            "run_pause",
            "Pause Run",
            "orchestration",
            "Pause the active orchestration run",
        ),
        spec(
            "/run resume",
            "run_resume",
            "Resume Run",
            "orchestration",
            "Resume the active orchestration run",
        ),
        spec(
            "/run retry <step-id>",
            "run_retry",
            "Retry Step",
            "orchestration",
            "Retry a failed or blocked orchestration step",
        ),
        spec(
            "/run skip <step-id>",
            "run_skip",
            "Skip Step",
            "orchestration",
            "Skip a pending or blocked orchestration step",
        ),
        spec(
            "/run replan",
            "run_replan",
            "Replan Run",
            "orchestration",
            "Move the active orchestration run into replanning",
        ),
        spec(
            "/run accept",
            "run_accept",
            "Accept Run",
            "orchestration",
            "Mark the active orchestration run accepted",
        ),
        spec(
            "/run abort",
            "run_abort",
            "Abort Run",
            "orchestration",
            "Abort the active orchestration run",
        ),
        spec(
            "/status",
            "inspect_status",
            "Status",
            "session",
            "Show current session and workspace status",
        ),
        spec(
            "/continue",
            "continue_session",
            "Continue",
            "session",
            "Continue the latest saved session",
        ),
        spec(
            "/diff",
            "inspect_diff",
            "Diff",
            "source_control",
            "Show current workspace changes",
        ),
        spec(
            "/commit",
            "stage_commit",
            "Commit",
            "source_control",
            "Prepare staged changes and commit workflow",
        ),
    ];

    if include_advanced {
        specs.extend([
            spec(
                "/interrupt",
                "interrupt_turn",
                "Interrupt",
                "conversation",
                "Compatibility alias for /exc",
            ),
            spec(
                "/slash",
                "open_slash_help",
                "Slash Commands",
                "navigation",
                "Inspect the slash command surface",
            ),
            spec(
                "/model <model>",
                "select_model",
                "Model",
                "navigation",
                "Select a model from the provider catalog",
            ),
            spec(
                "/reasoning <auto|low|medium|high>",
                "select_reasoning",
                "Reasoning",
                "configuration",
                "Select reasoning effort",
            ),
            spec(
                "/cost",
                "inspect_cost",
                "Cost",
                "usage",
                "Show session token and cost summary",
            ),
            spec(
                "/usage",
                "inspect_usage",
                "Usage",
                "usage",
                "Show detailed API usage statistics",
            ),
            spec(
                "/doctor",
                "run_doctor",
                "Doctor",
                "diagnostics",
                "Diagnose setup and environment health",
            ),
            spec(
                "/providers",
                "inspect_providers",
                "Providers",
                "configuration",
                "Inspect configured model providers",
            ),
            spec(
                "/config",
                "inspect_config",
                "Config",
                "configuration",
                "Inspect active CLI configuration",
            ),
            spec(
                "/tools",
                "inspect_tools",
                "Tools",
                "tools",
                "Inspect available governed tools",
            ),
            spec(
                "/mcp",
                "inspect_mcp",
                "MCP",
                "tools",
                "Inspect configured MCP servers",
            ),
        ]);
    }

    specs
}

pub fn parse_surface_command(input: &str) -> SurfaceCommandRequest {
    let raw = input.trim().to_string();
    if raw.is_empty() {
        return unknown(raw, Vec::new());
    }
    if let Some(prompt) = raw.strip_prefix("/prompt ") {
        let prompt = prompt.trim().to_string();
        let prompt_empty = prompt.is_empty();
        return SurfaceCommandRequest {
            raw,
            kind: if prompt_empty {
                SurfaceCommandKind::Unknown
            } else {
                SurfaceCommandKind::PromptTurn
            },
            action_id: Some("submit_prompt".to_string()),
            skill_id: None,
            args: Vec::new(),
            prompt: if prompt_empty { None } else { Some(prompt) },
            suggestions: if prompt_empty {
                vec!["/prompt <text>".to_string()]
            } else {
                Vec::new()
            },
        };
    }
    if !raw.starts_with('/') && !raw.starts_with('$') {
        return SurfaceCommandRequest {
            raw: raw.clone(),
            kind: SurfaceCommandKind::PromptTurn,
            action_id: Some("submit_prompt".to_string()),
            skill_id: None,
            args: Vec::new(),
            prompt: Some(raw),
            suggestions: Vec::new(),
        };
    }
    if raw.starts_with('$') {
        return parse_skill_command(raw);
    }
    parse_slash_command(raw)
}

fn parse_slash_command(raw: String) -> SurfaceCommandRequest {
    match raw.as_str() {
        "/help" | "/palette" | "/slash" => SurfaceCommandRequest {
            action_id: Some(
                match raw.as_str() {
                    "/palette" => "open_palette",
                    "/slash" => "open_slash_help",
                    _ => "open_help",
                }
                .to_string(),
            ),
            raw,
            kind: SurfaceCommandKind::Help,
            skill_id: None,
            args: Vec::new(),
            prompt: None,
            suggestions: Vec::new(),
        },
        "/quit" => SurfaceCommandRequest {
            raw,
            kind: SurfaceCommandKind::ProjectedAction,
            action_id: Some("exit_tui".to_string()),
            skill_id: None,
            args: Vec::new(),
            prompt: None,
            suggestions: Vec::new(),
        },
        _ if raw.starts_with("/language ") => {
            let args = raw
                .split_whitespace()
                .skip(1)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            SurfaceCommandRequest {
                raw,
                kind: SurfaceCommandKind::ProjectedAction,
                action_id: Some("select_language".to_string()),
                skill_id: None,
                args,
                prompt: None,
                suggestions: Vec::new(),
            }
        }
        _ if raw.starts_with("/theme ") => {
            let args = raw
                .split_whitespace()
                .skip(1)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            SurfaceCommandRequest {
                raw,
                kind: SurfaceCommandKind::ProjectedAction,
                action_id: Some("select_theme".to_string()),
                skill_id: None,
                args,
                prompt: None,
                suggestions: Vec::new(),
            }
        }
        _ if raw.starts_with("/approve ") || raw.starts_with("/deny ") => {
            let mut parts = raw.split_whitespace().map(ToString::to_string);
            let command = parts.next().unwrap_or_default();
            let args = parts.collect::<Vec<_>>();
            SurfaceCommandRequest {
                raw,
                kind: SurfaceCommandKind::ProjectedAction,
                action_id: Some(
                    if command == "/approve" {
                        "approve_permission"
                    } else {
                        "deny_permission"
                    }
                    .to_string(),
                ),
                skill_id: None,
                args,
                prompt: None,
                suggestions: Vec::new(),
            }
        }
        _ if product_command_for_raw(&raw).is_some() => {
            let spec = product_command_for_raw(&raw).expect("checked product command");
            let exact_match = spec.typed == raw;
            let raw_args = raw
                .split_whitespace()
                .skip(1)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let args = if exact_match {
                Vec::new()
            } else {
                args_for_spec(&spec, raw_args)
            };
            SurfaceCommandRequest {
                raw,
                kind: SurfaceCommandKind::ProjectedAction,
                action_id: Some(spec.action_id),
                skill_id: None,
                args,
                prompt: None,
                suggestions: Vec::new(),
            }
        }
        _ => unknown(
            raw,
            vec![
                "/help".to_string(),
                "/sessions".to_string(),
                "/permissions".to_string(),
                "/terminal".to_string(),
                "$list".to_string(),
            ],
        ),
    }
}
fn product_command_for_raw(raw: &str) -> Option<SurfaceCommandSpec> {
    let mut specs = all_surface_command_specs();
    specs.sort_by(|left, right| {
        right
            .typed
            .len()
            .cmp(&left.typed.len())
            .then_with(|| left.typed.cmp(&right.typed))
    });
    specs
        .into_iter()
        .find(|spec| product_command_matches_raw(spec, raw))
}

fn product_command_matches_raw(spec: &SurfaceCommandSpec, raw: &str) -> bool {
    if spec.typed == raw {
        return true;
    }

    let spec_tokens = spec.typed.split_whitespace().collect::<Vec<_>>();
    let raw_tokens = raw.split_whitespace().collect::<Vec<_>>();
    let Some(command) = spec_tokens.first().copied() else {
        return false;
    };
    if raw == command {
        return matches!(
            command,
            "/prompt" | "/language" | "/theme" | "/model" | "/reasoning" | "/routines"
        );
    }
    if !spec_tokens
        .iter()
        .skip(1)
        .any(|token| is_argument_hint(token))
    {
        return false;
    }
    if raw_tokens.len() < spec_tokens.len() {
        return false;
    }
    spec_tokens
        .iter()
        .zip(raw_tokens.iter())
        .all(|(spec_token, raw_token)| is_argument_hint(spec_token) || spec_token == raw_token)
}

fn is_argument_hint(token: &str) -> bool {
    token.starts_with('<') || token.starts_with('[')
}

fn args_for_spec(spec: &SurfaceCommandSpec, raw_args: Vec<String>) -> Vec<String> {
    let fixed_tail_len = spec
        .typed
        .split_whitespace()
        .skip(1)
        .take_while(|token| !is_argument_hint(token))
        .count();
    raw_args.into_iter().skip(fixed_tail_len).collect()
}

fn parse_skill_command(raw: String) -> SurfaceCommandRequest {
    let rest = raw.trim_start_matches('$').trim().to_string();
    if rest == "list" {
        return SurfaceCommandRequest {
            raw,
            kind: SurfaceCommandKind::SkillList,
            action_id: None,
            skill_id: None,
            args: Vec::new(),
            prompt: None,
            suggestions: Vec::new(),
        };
    }
    if let Some(skill_id) = rest
        .strip_prefix("inspect ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    {
        return SurfaceCommandRequest {
            raw,
            kind: SurfaceCommandKind::SkillInspect,
            action_id: None,
            skill_id: Some(skill_id),
            args: Vec::new(),
            prompt: None,
            suggestions: Vec::new(),
        };
    }
    let mut parts = rest.split_whitespace().map(ToString::to_string);
    let Some(skill_id) = parts.next().filter(|value| !value.is_empty()) else {
        return unknown(
            raw,
            vec!["$list".to_string(), "$inspect <skill>".to_string()],
        );
    };
    let args = parts.collect::<Vec<_>>();
    SurfaceCommandRequest {
        raw,
        kind: if args.is_empty() {
            SurfaceCommandKind::SkillInspect
        } else {
            SurfaceCommandKind::SkillRun
        },
        action_id: None,
        skill_id: Some(skill_id),
        args,
        prompt: None,
        suggestions: Vec::new(),
    }
}

fn unknown(raw: String, suggestions: Vec<String>) -> SurfaceCommandRequest {
    SurfaceCommandRequest {
        raw,
        kind: SurfaceCommandKind::Unknown,
        action_id: None,
        skill_id: None,
        args: Vec::new(),
        prompt: None,
        suggestions,
    }
}

fn spec(
    typed: &str,
    action_id: &str,
    label: &str,
    category: &str,
    summary: &str,
) -> SurfaceCommandSpec {
    SurfaceCommandSpec {
        typed: typed.to_string(),
        action_id: action_id.to_string(),
        label: label.to_string(),
        category: category.to_string(),
        summary: summary.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_surface_prompt_and_commands() {
        assert_eq!(
            parse_surface_command("fix the failing test").kind,
            SurfaceCommandKind::PromptTurn
        );
        assert_eq!(
            parse_surface_command("/prompt fix it").kind,
            SurfaceCommandKind::PromptTurn
        );
        assert_eq!(
            parse_surface_command("/sessions").target_id(),
            Some("switch_session")
        );
        assert_eq!(
            parse_surface_command("/terminal replay").target_id(),
            Some("terminal_replay")
        );
        assert_eq!(
            parse_surface_command("/language en").target_id(),
            Some("select_language")
        );
        let model = parse_surface_command("/model gpt 5.5");
        assert_eq!(model.target_id(), Some("select_model"));
        assert_eq!(model.args, vec!["gpt", "5.5"]);

        let reasoning = parse_surface_command("/reasoning high");
        assert_eq!(reasoning.target_id(), Some("select_reasoning"));
        assert_eq!(reasoning.args, vec!["high"]);

        assert_eq!(
            parse_surface_command("/fold").target_id(),
            Some("fold_output")
        );
        assert_eq!(
            parse_surface_command("/expand").target_id(),
            Some("expand_output")
        );
        assert_eq!(
            parse_surface_command("/output").target_id(),
            Some("show_output")
        );
        assert_eq!(
            parse_surface_command("/logs").target_id(),
            Some("show_logs")
        );
        let run_pause = parse_surface_command("/run pause");
        assert_eq!(run_pause.target_id(), Some("run_pause"));
        assert_eq!(run_pause.args, Vec::<String>::new());
        let run_retry = parse_surface_command("/run retry step_2");
        assert_eq!(run_retry.target_id(), Some("run_retry"));
        assert_eq!(run_retry.args, vec!["step_2"]);
        assert_eq!(
            parse_surface_command("/routines").target_id(),
            Some("open_routines")
        );
        assert_eq!(
            parse_surface_command("$list").kind,
            SurfaceCommandKind::SkillList
        );
        assert_eq!(
            parse_surface_command("$research-lit transformers").kind,
            SurfaceCommandKind::SkillRun
        );
    }

    #[test]
    fn parses_canonical_product_slash_commands_to_action_ids() {
        for (typed, action_id) in [
            ("/status", "inspect_status"),
            ("/exc", "interrupt_turn"),
            ("/interrupt", "interrupt_turn"),
            ("/continue", "continue_session"),
            ("/diff", "inspect_diff"),
            ("/commit", "stage_commit"),
            ("/cost", "inspect_cost"),
            ("/usage", "inspect_usage"),
            ("/doctor", "run_doctor"),
            ("/providers", "inspect_providers"),
            ("/config", "inspect_config"),
            ("/tools", "inspect_tools"),
            ("/mcp", "inspect_mcp"),
        ] {
            let parsed = parse_surface_command(typed);
            assert_eq!(parsed.kind, SurfaceCommandKind::ProjectedAction, "{typed}");
            assert_eq!(parsed.target_id(), Some(action_id), "{typed}");
        }
    }

    #[test]
    fn unknown_slash_and_skill_inputs_are_not_prompts() {
        let slash = parse_surface_command("/nope");
        assert_eq!(slash.kind, SurfaceCommandKind::Unknown);
        assert!(slash.prompt.is_none());
        assert!(slash.suggestions.contains(&"/help".to_string()));

        let skill = parse_surface_command("$");
        assert_eq!(skill.kind, SurfaceCommandKind::Unknown);
        assert!(skill.prompt.is_none());
    }
}
