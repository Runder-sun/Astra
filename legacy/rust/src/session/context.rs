use super::context_pack::{assemble_context_pack, ContextPackInput};
use super::store::SessionStore;
use super::transcript::TranscriptLine;
use crate::providers::{
    build_system_messages, estimate_tokens, model_window_config, ChatMessage, ChatToolCall,
    ChatToolFunction,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Convert transcript lines into OpenAI-compatible chat messages.
///
/// - Control lines are skipped.
/// - Message lines map directly to user/assistant messages.
/// - Adjacent ToolCall lines are merged into the preceding assistant message's `tool_calls`.
/// - ToolResult lines become `role: "tool"` messages with matching `tool_call_id`.
/// - SummaryReference lines load the summary markdown and become assistant messages.
pub fn transcript_to_messages(
    lines: &[TranscriptLine],
    store: &SessionStore,
    session_id: &str,
) -> Vec<ChatMessage> {
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut tool_call_counter: usize = 0;
    let mut pending_tool_ids: Vec<(String, String)> = Vec::new();

    for line in lines {
        match line {
            TranscriptLine::Control { .. } => {}

            TranscriptLine::Message { role, content } => {
                messages.push(ChatMessage {
                    role: role.clone(),
                    content: content.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }

            TranscriptLine::ToolCall {
                tool_name,
                arguments,
                call_id: explicit_call_id,
            } => {
                let call_id = explicit_call_id.clone().unwrap_or_else(|| {
                    let id = format!("tc_{}", tool_call_counter);
                    tool_call_counter += 1;
                    id
                });

                pending_tool_ids.push((tool_name.clone(), call_id.clone()));

                let tool_call = ChatToolCall {
                    id: call_id,
                    r#type: "function".to_string(),
                    function: ChatToolFunction {
                        name: tool_name.clone(),
                        arguments: arguments.clone(),
                    },
                };

                if let Some(last) = messages.last_mut() {
                    if last.role == "assistant" {
                        if last.tool_calls.is_none() {
                            last.tool_calls = Some(Vec::new());
                        }
                        last.tool_calls.as_mut().unwrap().push(tool_call);
                    } else {
                        messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: " ".to_string(),
                            tool_calls: Some(vec![tool_call]),
                            tool_call_id: None,
                        });
                    }
                } else {
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: " ".to_string(),
                        tool_calls: Some(vec![tool_call]),
                        tool_call_id: None,
                    });
                }
            }

            TranscriptLine::ToolResult {
                tool_name,
                output,
                call_id: explicit_call_id,
            } => {
                let call_id = explicit_call_id.clone().unwrap_or_else(|| {
                    pending_tool_ids
                        .iter()
                        .position(|(name, _)| name == tool_name)
                        .map(|pos| {
                            let (_, id) = pending_tool_ids.remove(pos);
                            id
                        })
                        .unwrap_or_else(|| {
                            let id = format!("tc_{}", tool_call_counter);
                            tool_call_counter += 1;
                            id
                        })
                });

                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: output.clone(),
                    tool_calls: None,
                    tool_call_id: Some(call_id),
                });
            }

            TranscriptLine::SummaryReference { summary_ref } => {
                let content = load_summary_content(store, session_id, summary_ref)
                    .unwrap_or_else(|| format!("[Previous conversation summary: {summary_ref}]"));
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: format!("[Context summary]\n{content}"),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
    }

    messages
}

fn load_summary_content(
    store: &SessionStore,
    session_id: &str,
    summary_ref: &str,
) -> Option<String> {
    let summary_path = store
        .data_dir()
        .join("sessions")
        .join(session_id)
        .join("summaries")
        .join(format!("{summary_ref}.md"));
    if summary_path.exists() {
        fs::read_to_string(&summary_path).ok()
    } else {
        None
    }
}

/// Build the complete messages array for an LLM call with token budget enforcement.
///
/// Order: system messages → transcript history (trimmed) → user message (with memory/mission injection)
pub fn build_context_messages(
    store: &SessionStore,
    project_id: &str,
    session_id: &str,
    current_prompt: &str,
    model: &str,
) -> Vec<ChatMessage> {
    let config = model_window_config(model);
    let mut system_messages = build_system_messages();
    let workspace_root = infer_workspace_root(store.data_dir());
    let mission_summary = load_mission_summary(store.data_dir());
    let goal_task_pool_summary =
        load_goal_task_pool_summary(store.data_dir(), &workspace_root, project_id);
    let memory_injection = MemoryInjection::from_store(store.data_dir(), current_prompt);
    let memory_summaries = memory_injection.summaries();
    let context_pack = assemble_context_pack(ContextPackInput {
        workspace_root,
        user_home: std::env::var("HOME").ok().map(PathBuf::from),
        role_profile: Some(
            crate::session::context_pack::DEFAULT_MAIN_AGENT_ROLE_PROFILE.to_string(),
        ),
        current_prompt: current_prompt.to_string(),
        session_summary: load_recent_session_summary(store, session_id),
        mission_summary: mission_summary.clone(),
        goal_task_pool_summary,
        memory_summaries,
        capability_summaries: default_capability_summaries(),
        evidence_summaries: Vec::new(),
        orchestration_summary: crate::orchestration::load_active_run_progress_summary(
            store.data_dir(),
        ),
    });

    if let Ok(pack) = context_pack.as_ref() {
        let stable_context = pack.render_stable_system_context();
        if !stable_context.is_empty() {
            system_messages.insert(
                1,
                ChatMessage {
                    role: "system".to_string(),
                    content: stable_context,
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        }
    }

    let system_tokens: usize = system_messages
        .iter()
        .map(|m| estimate_tokens(&m.content))
        .sum();

    // Build user message with optional memory/mission context injection
    let dynamic_context = match context_pack.as_ref() {
        Ok(pack) => {
            let rendered = pack.render_dynamic_user_context();
            (!rendered.trim().is_empty()).then_some(rendered)
        }
        Err(err) => Some(format!(
            "<context-pack degraded=\"true\">\nContextPack assembly failed: {err}\nLegacy mission and memory context fallback remains active for this API call.\n</context-pack>"
        )),
    };
    let user_content = inject_context_into_prompt(
        current_prompt,
        memory_injection,
        mission_summary,
        dynamic_context,
    );

    let user_tokens = estimate_tokens(&user_content);

    let history_budget = config
        .context_window
        .saturating_sub(config.output_reservation)
        .saturating_sub(system_tokens)
        .saturating_sub(user_tokens);

    let lines = store.read_transcript(session_id).unwrap_or_default();
    let history = transcript_to_messages(&lines, store, session_id);

    let user_msg = ChatMessage {
        role: "user".to_string(),
        content: user_content,
        tool_calls: None,
        tool_call_id: None,
    };

    if history.is_empty() {
        let mut result = system_messages;
        result.push(user_msg);
        return result;
    }

    let trimmed = trim_history_to_budget(&history, history_budget);

    let mut result = system_messages;
    result.extend(trimmed);
    result.push(user_msg);
    result
}

/// Inject mission and memory context into the user prompt.
///
/// Order: mission → memory → original prompt.
fn inject_context_into_prompt(
    prompt: &str,
    memory: MemoryInjection,
    mission: Option<String>,
    context_pack_dynamic: Option<String>,
) -> String {
    let mut parts = Vec::new();
    let include_legacy_fallback = context_pack_dynamic
        .as_ref()
        .map(|context| context.contains("degraded=\"true\""))
        .unwrap_or(false);
    let has_context_pack = context_pack_dynamic.is_some();
    if let Some(context) = context_pack_dynamic {
        parts.push(format!(
            "<context-pack>\nThis context is assembled by the runtime for the current API call and is not persisted as user input.\n{context}\n</context-pack>"
        ));
    }
    if !has_context_pack || include_legacy_fallback {
        if let Some(m) = mission {
            parts.push(format!("<mission>\n{m}\n</mission>"));
        }
        match memory {
            MemoryInjection::Records(records) if !records.is_empty() => {
                let mem_block = records
                    .iter()
                    .map(|r| format!("- {} (source: {})", r.summary, r.kind))
                    .collect::<Vec<_>>()
                    .join("\n");
                parts.push(format!("<memory-context>\n{mem_block}\n</memory-context>"));
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        return prompt.to_string();
    }
    parts.push(prompt.to_string());
    parts.join("\n\n")
}

/// Memory recall result — simplified view for injection.
struct MemoryRecallItem {
    summary: String,
    kind: String,
}

enum MemoryInjection {
    Records(Vec<MemoryRecallItem>),
    Unavailable,
}

impl MemoryInjection {
    fn summaries(&self) -> Vec<String> {
        match self {
            Self::Records(records) => records
                .iter()
                .map(|record| format!("{} (source: {})", record.summary, record.kind))
                .collect(),
            Self::Unavailable => Vec::new(),
        }
    }

    fn from_store(data_dir: &std::path::Path, query: &str) -> Self {
        let request = crate::memory::MemoryQueryRequest {
            query: query.to_string(),
            limit: Some(5),
            include_inactive: false,
        };
        match crate::memory::query(data_dir, request) {
            Ok(result) => {
                let items: Vec<MemoryRecallItem> = result
                    .matched_records
                    .into_iter()
                    .take(5)
                    .map(|m| MemoryRecallItem {
                        summary: m.record.summary,
                        kind: m.record.kind,
                    })
                    .collect();
                MemoryInjection::Records(items)
            }
            Err(_) => MemoryInjection::Unavailable,
        }
    }
}

fn infer_workspace_root(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| data_dir.to_path_buf())
}

fn load_recent_session_summary(store: &SessionStore, session_id: &str) -> Option<String> {
    let lines = store.read_transcript(session_id).ok()?;
    let mut previews = Vec::new();
    for line in lines.iter().rev() {
        if previews.len() >= 6 {
            break;
        }
        match line {
            TranscriptLine::Message { role, content } => {
                previews.push(format!("{role}: {}", compact_preview(content, 240)));
            }
            TranscriptLine::ToolCall { tool_name, .. } => {
                previews.push(format!("tool_call: {tool_name}"));
            }
            TranscriptLine::ToolResult {
                tool_name, output, ..
            } => {
                previews.push(format!(
                    "tool_result: {tool_name} -> {}",
                    compact_preview(output, 160)
                ));
            }
            TranscriptLine::Control { .. } | TranscriptLine::SummaryReference { .. } => {}
        }
    }
    if previews.is_empty() {
        None
    } else {
        previews.reverse();
        Some(previews.join("\n"))
    }
}

fn compact_preview(content: &str, max_chars: usize) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut preview: String = trimmed.chars().take(max_chars).collect();
    preview.push_str("...");
    preview
}

fn default_capability_summaries() -> Vec<String> {
    let tool_names = crate::tools::builtin_registry()
        .list()
        .into_iter()
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        format!("builtin tools: {tool_names}"),
        "runtime loop: model-driven tool calls with permission policy and transcript tool-call/result pairing".to_string(),
        "run control: orchestration model supports checklist progress, gates, artifacts, continuation points, and pause/resume/cancel command records".to_string(),
    ]
}

/// Load a compact mission frame summary if available.
fn load_mission_summary(data_dir: &std::path::Path) -> Option<String> {
    let frame_path = crate::goals::frame_path(data_dir);
    if !frame_path.exists() {
        return None;
    }
    // Read the frame directly, bypassing project_id validation
    let content = fs::read_to_string(&frame_path).ok()?;
    let frame: serde_json::Value = serde_json::from_str(&content).ok()?;
    let max_goal = frame.get("project_max_goal")?.as_str()?;
    let milestone = frame.get("milestone_goal")?.as_str()?;
    let current = frame.get("current_implementation_goal")?.as_str()?;
    Some(format!(
        "Goal: {max_goal}\nMilestone: {milestone}\nCurrent: {current}"
    ))
}

fn load_goal_task_pool_summary(
    data_dir: &std::path::Path,
    workspace_root: &Path,
    project_id: &str,
) -> Option<String> {
    let status = crate::goals::status(data_dir, workspace_root, project_id).ok()?;
    let task_pool = status.task_pool;
    let recovery_governance = crate::routines::recovery_governance(data_dir).ok();
    let loop_closure =
        crate::goals::loop_closure_projection(&task_pool, recovery_governance.as_ref(), 0, false);
    let mut lines = vec![
        format!("Automation mode: {}", task_pool.automation_mode.as_str()),
        format!(
            "Task counts: total {} | ready {} | running {} | needs_approval {} | needs_review {} | blocked {}",
            task_pool.summary.total,
            task_pool.summary.ready_to_run,
            task_pool.summary.running,
            task_pool.summary.needs_approval,
            task_pool.summary.needs_review,
            task_pool.summary.blocked
        ),
        format!("Next recommended action: {}", task_pool.next_recommended_action),
        format!("Loop closure: {}", loop_closure.compact_line()),
        format!(
            "Main-agent instruction: {}",
            loop_closure.main_agent_instruction
        ),
    ];
    if let Some(target_run) = task_pool.target_run.as_ref() {
        let current = target_run.current_step_title.as_deref().unwrap_or("none");
        lines.push(format!(
            "Target run: {} | status {} | progress {}% | current {}",
            target_run.objective, target_run.status, target_run.progress_percent, current
        ));
    }
    let visible_entries = task_pool.entries.iter().take(5).collect::<Vec<_>>();
    if !visible_entries.is_empty() {
        lines.push("Visible items:".to_string());
        for entry in visible_entries {
            lines.push(format!(
                "- [{}] {} :: {}",
                entry.bucket_id,
                entry.title,
                compact_preview(&entry.summary, 120)
            ));
        }
    }
    Some(lines.join("\n"))
}

/// Trim history to fit within the token budget, preserving complete turns rather than partial tails.
fn trim_history_to_budget(history: &[ChatMessage], budget: usize) -> Vec<ChatMessage> {
    if history.is_empty() {
        return Vec::new();
    }

    let segments = history_segments(history);
    if segments.is_empty() {
        return Vec::new();
    }

    let mut kept_segments = Vec::new();
    let mut used = 0usize;

    for segment in segments.iter().rev() {
        if used + segment.token_count <= budget {
            kept_segments.push(*segment);
            used += segment.token_count;
        }
    }

    if kept_segments.is_empty() {
        return Vec::new();
    }

    kept_segments.sort_by_key(|segment| segment.start);

    let mut kept = Vec::new();
    for segment in kept_segments {
        kept.extend_from_slice(&history[segment.start..segment.end]);
    }
    kept
}

#[derive(Debug, Clone, Copy)]
struct HistorySegment {
    start: usize,
    end: usize,
    token_count: usize,
}

fn history_segments(history: &[ChatMessage]) -> Vec<HistorySegment> {
    if history.is_empty() {
        return Vec::new();
    }

    let user_indices: Vec<usize> = history
        .iter()
        .enumerate()
        .filter_map(|(index, message)| (message.role == "user").then_some(index))
        .collect();

    if user_indices.is_empty() {
        return vec![HistorySegment {
            start: 0,
            end: history.len(),
            token_count: history.iter().map(|m| estimate_tokens(&m.content)).sum(),
        }];
    }

    let mut segments = Vec::new();

    if user_indices[0] > 0 {
        segments.push(HistorySegment {
            start: 0,
            end: user_indices[0],
            token_count: history[..user_indices[0]]
                .iter()
                .map(|m| estimate_tokens(&m.content))
                .sum(),
        });
    }

    for (index, start) in user_indices.iter().enumerate() {
        let end = user_indices
            .get(index + 1)
            .copied()
            .unwrap_or(history.len());
        segments.push(HistorySegment {
            start: *start,
            end,
            token_count: history[*start..end]
                .iter()
                .map(|m| estimate_tokens(&m.content))
                .sum(),
        });
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant_tool(content: &str, call_id: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
            tool_calls: Some(vec![ChatToolCall {
                id: call_id.to_string(),
                r#type: "function".to_string(),
                function: ChatToolFunction {
                    name: "inspect".to_string(),
                    arguments: "{\"path\":\"src\"}".to_string(),
                },
            }]),
            tool_call_id: None,
        }
    }

    fn tool_result(call_id: &str, output: &str) -> ChatMessage {
        ChatMessage {
            role: "tool".to_string(),
            content: output.to_string(),
            tool_calls: None,
            tool_call_id: Some(call_id.to_string()),
        }
    }

    #[test]
    fn trim_history_to_budget_keeps_complete_latest_turn_segments() {
        let history = vec![
            msg("user", "turn one"),
            msg("assistant", "turn one ack"),
            msg("user", "turn two"),
            assistant_tool("I will inspect the workspace now.", "call_inspect_1"),
            tool_result("call_inspect_1", "found 3 files"),
            msg("assistant", "Here is the follow-up."),
        ];
        let latest_turn_budget = history[2..]
            .iter()
            .map(|message| estimate_tokens(&message.content))
            .sum();
        let trimmed = trim_history_to_budget(&history, latest_turn_budget);

        assert_eq!(trimmed.len(), 4);
        assert_eq!(trimmed[0].role, "user");
        assert_eq!(trimmed[0].content, "turn two");
        assert_eq!(trimmed[1].role, "assistant");
        assert_eq!(
            trimmed[1]
                .tool_calls
                .as_ref()
                .expect("tool call should stay with assistant")
                .len(),
            1
        );
        assert_eq!(trimmed[2].role, "tool");
        assert_eq!(trimmed[2].tool_call_id.as_deref(), Some("call_inspect_1"));
        assert_eq!(trimmed[3].role, "assistant");
    }
}
