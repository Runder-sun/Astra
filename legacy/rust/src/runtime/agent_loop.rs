use super::*;

pub struct AgentLoopResult {
    pub final_content: String,
    pub iterations: usize,
    pub tool_calls_made: usize,
    pub finish_reason: String,
}

#[derive(Debug, Clone)]
pub struct AgentLoopToolResultEvent {
    pub tool_name: String,
    pub call_id: String,
    pub arguments: String,
    pub status: String,
    pub output_text: String,
    pub result: Option<crate::tools::ToolResult>,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentLoopBudget {
    pub max_iterations: usize,
    pub max_tool_calls: usize,
    pub max_elapsed: Option<Duration>,
    pub finalize_on_tool_budget_exhaustion: bool,
    pub closeout_tool_call_threshold: usize,
}

impl AgentLoopBudget {
    pub fn new(max_iterations: usize, max_tool_calls: usize) -> Self {
        Self {
            max_iterations: max_iterations.max(1),
            max_tool_calls: max_tool_calls.max(1),
            max_elapsed: None,
            finalize_on_tool_budget_exhaustion: false,
            closeout_tool_call_threshold: 0,
        }
    }

    pub fn with_max_elapsed(mut self, max_elapsed: Duration) -> Self {
        self.max_elapsed = Some(max_elapsed.max(Duration::from_millis(1)));
        self
    }

    pub fn with_tool_budget_finalization(mut self, closeout_threshold: usize) -> Self {
        self.finalize_on_tool_budget_exhaustion = true;
        self.closeout_tool_call_threshold = closeout_threshold;
        self
    }
}

impl Default for AgentLoopBudget {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_AGENT_ITERATIONS, usize::MAX)
    }
}

#[derive(Debug)]
pub enum AgentLoopError {
    ProviderError(ProviderExecutionError),
    Cancelled {
        partial_content: String,
        iterations: usize,
    },
    MaxIterationsExceeded {
        iterations: usize,
        tool_calls_made: usize,
        partial_content: String,
    },
    BudgetExhausted {
        iterations: usize,
        tool_calls_made: usize,
        partial_content: String,
    },
}

impl fmt::Display for AgentLoopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderError(err) => write!(f, "{err}"),
            Self::Cancelled {
                partial_content,
                iterations,
            } => {
                write!(f, "agent loop cancelled after {iterations} iteration(s), partial content: {} chars", partial_content.len())
            }
            Self::MaxIterationsExceeded {
                iterations,
                tool_calls_made,
                partial_content,
            } => {
                write!(
                    f,
                    "agent loop exceeded max iterations ({iterations}) after {tool_calls_made} tool call(s), partial content: {} chars",
                    partial_content.len()
                )
            }
            Self::BudgetExhausted {
                iterations,
                tool_calls_made,
                partial_content,
            } => {
                write!(
                    f,
                    "agent loop exhausted budget after {iterations} iteration(s), {tool_calls_made} tool call(s), partial content: {} chars",
                    partial_content.len()
                )
            }
        }
    }
}

impl std::error::Error for AgentLoopError {}

/// Run the multi-step agent loop: call LLM with tools → execute tool_calls → feed results → repeat.
pub fn run_agent_loop(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    context_messages: &[ChatMessage],
    tool_definitions: serde_json::Value,
    tool_executor: &LocalToolExecutor,
    permission_policy: &crate::permissions::PermissionPolicy,
    store: &SessionStore,
    session_id: &str,
    cancel_token: &RuntimeCancelToken,
    on_text_delta: &mut dyn FnMut(&str),
    on_tool_call_start: &mut dyn FnMut(&str, &str),
    on_tool_result: &mut dyn FnMut(AgentLoopToolResultEvent),
    reasoning_effort: Option<&str>,
) -> Result<AgentLoopResult, AgentLoopError> {
    run_agent_loop_with_budget(
        provider_trace,
        context_messages,
        tool_definitions,
        tool_executor,
        permission_policy,
        store,
        session_id,
        cancel_token,
        on_text_delta,
        on_tool_call_start,
        on_tool_result,
        reasoning_effort,
        AgentLoopBudget::default(),
    )
}

/// Run the multi-step agent loop with an explicit budget.
pub fn run_agent_loop_with_budget(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    context_messages: &[ChatMessage],
    tool_definitions: serde_json::Value,
    tool_executor: &LocalToolExecutor,
    permission_policy: &crate::permissions::PermissionPolicy,
    store: &SessionStore,
    session_id: &str,
    cancel_token: &RuntimeCancelToken,
    on_text_delta: &mut dyn FnMut(&str),
    on_tool_call_start: &mut dyn FnMut(&str, &str),
    on_tool_result: &mut dyn FnMut(AgentLoopToolResultEvent),
    reasoning_effort: Option<&str>,
    budget: AgentLoopBudget,
) -> Result<AgentLoopResult, AgentLoopError> {
    run_agent_loop_with_budget_and_timeout_policy(
        provider_trace,
        context_messages,
        tool_definitions,
        tool_executor,
        permission_policy,
        store,
        session_id,
        cancel_token,
        on_text_delta,
        on_tool_call_start,
        on_tool_result,
        reasoning_effort,
        budget,
        ProviderTimeoutPolicy::Interactive,
    )
}

pub fn run_agent_loop_with_budget_and_timeout_policy(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    context_messages: &[ChatMessage],
    tool_definitions: serde_json::Value,
    tool_executor: &LocalToolExecutor,
    permission_policy: &crate::permissions::PermissionPolicy,
    store: &SessionStore,
    session_id: &str,
    cancel_token: &RuntimeCancelToken,
    on_text_delta: &mut dyn FnMut(&str),
    on_tool_call_start: &mut dyn FnMut(&str, &str),
    on_tool_result: &mut dyn FnMut(AgentLoopToolResultEvent),
    reasoning_effort: Option<&str>,
    budget: AgentLoopBudget,
    timeout_policy: ProviderTimeoutPolicy,
) -> Result<AgentLoopResult, AgentLoopError> {
    let mut messages: Vec<ChatMessage> = context_messages.to_vec();
    let mut iteration = 0usize;
    let mut total_tool_calls = 0usize;
    let mut accumulated_content = String::new();
    let max_iterations = budget.max_iterations;
    let max_tool_calls = budget.max_tool_calls;
    let max_elapsed = budget.max_elapsed;
    let finalize_on_tool_budget = budget.finalize_on_tool_budget_exhaustion;
    let closeout_tool_call_threshold = budget.closeout_tool_call_threshold;
    let mut closeout_notice_sent = false;
    let mut empty_stop_repairs = 0usize;
    let started_at = Instant::now();
    let allowed_tool_names = allowed_tool_names_from_openai_tool_definitions(&tool_definitions);

    loop {
        if cancel_token.is_cancelled() {
            return Err(AgentLoopError::Cancelled {
                partial_content: accumulated_content,
                iterations: iteration,
            });
        }
        if let Some(limit) = max_elapsed.filter(|limit| started_at.elapsed() >= *limit) {
            if finalize_on_tool_budget {
                return finalize_agent_loop_without_tools(
                    provider_trace,
                    &mut messages,
                    store,
                    session_id,
                    cancel_token,
                    &mut *on_text_delta,
                    reasoning_effort,
                    agent_loop_provider_timeout_policy_for_remaining(
                        timeout_policy,
                        started_at,
                        max_elapsed,
                    ),
                    agent_loop_elapsed_budget_finalization_notice(
                        started_at.elapsed(),
                        limit,
                        total_tool_calls,
                    ),
                    iteration + 1,
                    total_tool_calls,
                    "elapsed_budget_finalized",
                );
            }
            return Err(AgentLoopError::BudgetExhausted {
                iterations: iteration,
                tool_calls_made: total_tool_calls,
                partial_content: accumulated_content,
            });
        }
        if iteration >= max_iterations {
            return Err(AgentLoopError::MaxIterationsExceeded {
                iterations: iteration,
                tool_calls_made: total_tool_calls,
                partial_content: accumulated_content,
            });
        }
        if total_tool_calls >= max_tool_calls {
            if finalize_on_tool_budget {
                return finalize_agent_loop_without_tools(
                    provider_trace,
                    &mut messages,
                    store,
                    session_id,
                    cancel_token,
                    &mut *on_text_delta,
                    reasoning_effort,
                    agent_loop_provider_timeout_policy_for_remaining(
                        timeout_policy,
                        started_at,
                        max_elapsed,
                    ),
                    agent_loop_tool_budget_finalization_notice(total_tool_calls, max_tool_calls),
                    iteration + 1,
                    total_tool_calls,
                    "tool_budget_finalized",
                );
            }
            return Err(AgentLoopError::BudgetExhausted {
                iterations: iteration,
                tool_calls_made: total_tool_calls,
                partial_content: accumulated_content,
            });
        }
        if finalize_on_tool_budget
            && !closeout_notice_sent
            && closeout_tool_call_threshold > 0
            && max_tool_calls != usize::MAX
        {
            let remaining_tool_calls = max_tool_calls.saturating_sub(total_tool_calls);
            if remaining_tool_calls > 0 && remaining_tool_calls <= closeout_tool_call_threshold {
                let notice = agent_loop_tool_budget_closeout_notice(remaining_tool_calls);
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: notice.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
                let _ = store.append_line(
                    session_id,
                    TranscriptLine::Message {
                        role: "user".to_string(),
                        content: notice,
                    },
                );
                closeout_notice_sent = true;
            }
        }

        let completion = complete_prompt_streaming_with_tools_and_timeout_policy(
            provider_trace,
            &messages,
            reasoning_effort,
            Some(tool_definitions.clone()),
            &mut *on_text_delta,
            || cancel_token.is_cancelled(),
            agent_loop_provider_timeout_policy_for_remaining(
                timeout_policy,
                started_at,
                max_elapsed,
            ),
        )
        .map_err(AgentLoopError::ProviderError)?;

        let finish = completion.finish_reason.as_deref().unwrap_or("stop");
        accumulated_content = completion.content.clone();
        if agent_loop_content_has_malformed_control_markup(&completion.content) {
            return Err(AgentLoopError::ProviderError(ProviderExecutionError::Parse(
                format!(
                    "provider emitted malformed control markup in assistant content: finish_reason={finish}; chars={}",
                    completion.content.len()
                ),
            )));
        }

        if let Some(limit) = max_elapsed.filter(|limit| started_at.elapsed() >= *limit) {
            if finalize_on_tool_budget {
                return finalize_agent_loop_without_tools(
                    provider_trace,
                    &mut messages,
                    store,
                    session_id,
                    cancel_token,
                    &mut *on_text_delta,
                    reasoning_effort,
                    agent_loop_provider_timeout_policy_for_remaining(
                        timeout_policy,
                        started_at,
                        max_elapsed,
                    ),
                    agent_loop_elapsed_budget_finalization_notice(
                        started_at.elapsed(),
                        limit,
                        total_tool_calls,
                    ),
                    iteration + 1,
                    total_tool_calls,
                    "elapsed_budget_finalized",
                );
            }
            return Err(AgentLoopError::BudgetExhausted {
                iterations: iteration + 1,
                tool_calls_made: total_tool_calls,
                partial_content: accumulated_content,
            });
        }

        let tool_calls = completion.tool_calls.clone().unwrap_or_default();

        if tool_calls.is_empty() {
            if !completion.content.trim().is_empty() || matches!(finish, "length") {
                return Ok(AgentLoopResult {
                    final_content: completion.content,
                    iterations: iteration + 1,
                    tool_calls_made: total_tool_calls,
                    finish_reason: finish.to_string(),
                });
            }
            if empty_stop_repairs >= 2 {
                return Err(AgentLoopError::ProviderError(
                    ProviderExecutionError::EmptyResponseWithDiagnostics {
                        summary: format!(
                            "agent loop received empty assistant content with no tool calls after {} repair prompt(s); finish_reason={finish}; iterations={}",
                            empty_stop_repairs,
                            iteration + 1
                        ),
                    },
                ));
            }
            empty_stop_repairs += 1;
            let notice = agent_loop_empty_output_repair_notice(empty_stop_repairs);
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: notice.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
            let _ = store.append_line(
                session_id,
                TranscriptLine::Message {
                    role: "user".to_string(),
                    content: notice,
                },
            );
            iteration += 1;
            continue;
        }

        // Append assistant message with tool_calls to conversation
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: completion.content.clone(),
            tool_calls: Some(tool_calls.clone()),
            tool_call_id: None,
        });

        let registry = builtin_registry();
        let mut agent_turn_complete_after_batch: Option<String> = None;

        for tc in &tool_calls {
            if let Some(limit) = max_elapsed.filter(|limit| started_at.elapsed() >= *limit) {
                if finalize_on_tool_budget {
                    return finalize_agent_loop_without_tools(
                        provider_trace,
                        &mut messages,
                        store,
                        session_id,
                        cancel_token,
                        &mut *on_text_delta,
                        reasoning_effort,
                        agent_loop_provider_timeout_policy_for_remaining(
                            timeout_policy,
                            started_at,
                            max_elapsed,
                        ),
                        agent_loop_elapsed_budget_finalization_notice(
                            started_at.elapsed(),
                            limit,
                            total_tool_calls,
                        ),
                        iteration + 1,
                        total_tool_calls,
                        "elapsed_budget_finalized",
                    );
                }
                return Err(AgentLoopError::BudgetExhausted {
                    iterations: iteration + 1,
                    tool_calls_made: total_tool_calls,
                    partial_content: accumulated_content,
                });
            }
            if cancel_token.is_cancelled() {
                return Err(AgentLoopError::Cancelled {
                    partial_content: accumulated_content,
                    iterations: iteration,
                });
            }

            let call_id = tc.id.clone();
            let tool_name = tc.function.name.clone();
            if total_tool_calls >= max_tool_calls {
                if finalize_on_tool_budget {
                    let error_msg = agent_loop_tool_budget_denial_notice();
                    on_tool_result(AgentLoopToolResultEvent {
                        tool_name: tool_name.clone(),
                        call_id: call_id.clone(),
                        arguments: tc.function.arguments.clone(),
                        status: "budget_exhausted".to_string(),
                        output_text: error_msg.clone(),
                        result: None,
                    });
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: error_msg.clone(),
                        tool_calls: None,
                        tool_call_id: Some(call_id.clone()),
                    });
                    let _ = store.append_line(
                        session_id,
                        TranscriptLine::ToolCall {
                            tool_name: tool_name.clone(),
                            arguments: tc.function.arguments.clone(),
                            call_id: Some(call_id.clone()),
                        },
                    );
                    let _ = store.append_line(
                        session_id,
                        TranscriptLine::ToolResult {
                            tool_name: tool_name.clone(),
                            output: error_msg,
                            call_id: Some(call_id),
                        },
                    );
                    continue;
                }
                return Err(AgentLoopError::BudgetExhausted {
                    iterations: iteration + 1,
                    tool_calls_made: total_tool_calls,
                    partial_content: accumulated_content,
                });
            }

            on_tool_call_start(&tool_name, &call_id);

            if !allowed_tool_names.contains(&tool_name) {
                let error_msg = format!(
                    "Permission denied: tool '{tool_name}' is not part of this agent runtime tool scope"
                );
                on_tool_result(AgentLoopToolResultEvent {
                    tool_name: tool_name.clone(),
                    call_id: call_id.clone(),
                    arguments: tc.function.arguments.clone(),
                    status: "denied".to_string(),
                    output_text: error_msg.clone(),
                    result: None,
                });
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: error_msg.clone(),
                    tool_calls: None,
                    tool_call_id: Some(call_id.clone()),
                });
                let _ = store.append_line(
                    session_id,
                    TranscriptLine::ToolCall {
                        tool_name: tool_name.clone(),
                        arguments: tc.function.arguments.clone(),
                        call_id: Some(call_id.clone()),
                    },
                );
                let _ = store.append_line(
                    session_id,
                    TranscriptLine::ToolResult {
                        tool_name,
                        output: error_msg,
                        call_id: Some(call_id),
                    },
                );
                continue;
            }

            // Permission check
            let tool_spec = registry.get(&tool_name);
            let target_path = crate::tools::extract_target_path_from_args(&tc.function.arguments);
            let perm = if let Some(spec) = tool_spec {
                permission_policy.evaluate(spec, target_path.as_deref())
            } else {
                crate::permissions::PermissionCheckResult {
                    allowed: false,
                    reason: format!("unknown tool: {tool_name}"),
                    requires_approval: false,
                    mode: permission_policy.mode,
                    reason_code: "unknown_tool".to_string(),
                    workspace_boundary_ok: false,
                }
            };

            if !perm.allowed {
                let error_msg = format!("Permission denied: {}", perm.reason);
                on_tool_result(AgentLoopToolResultEvent {
                    tool_name: tool_name.clone(),
                    call_id: call_id.clone(),
                    arguments: tc.function.arguments.clone(),
                    status: "denied".to_string(),
                    output_text: error_msg.clone(),
                    result: None,
                });
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: error_msg.clone(),
                    tool_calls: None,
                    tool_call_id: Some(call_id.clone()),
                });
                let _ = store.append_line(
                    session_id,
                    TranscriptLine::ToolCall {
                        tool_name: tool_name.clone(),
                        arguments: tc.function.arguments.clone(),
                        call_id: Some(call_id.clone()),
                    },
                );
                let _ = store.append_line(
                    session_id,
                    TranscriptLine::ToolResult {
                        tool_name: tool_name.clone(),
                        output: error_msg,
                        call_id: Some(call_id),
                    },
                );
                continue;
            }

            // Execute tool
            let internal_call = crate::tools::convert_api_tool_call_to_tool_call(tc);
            match tool_executor.execute(&internal_call) {
                Ok(result) => {
                    let result_text = crate::tools::format_tool_result_for_llm(&result);
                    on_tool_result(AgentLoopToolResultEvent {
                        tool_name: tool_name.clone(),
                        call_id: call_id.clone(),
                        arguments: tc.function.arguments.clone(),
                        status: result.status.clone(),
                        output_text: result_text.clone(),
                        result: Some(result.clone()),
                    });
                    let agent_turn_complete =
                        agent_loop_tool_result_requests_main_agent_turn_complete(&result);
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: result_text.clone(),
                        tool_calls: None,
                        tool_call_id: Some(call_id.clone()),
                    });
                    let _ = store.append_line(
                        session_id,
                        TranscriptLine::ToolCall {
                            tool_name: tool_name.clone(),
                            arguments: tc.function.arguments.clone(),
                            call_id: Some(call_id.clone()),
                        },
                    );
                    let _ = store.append_line(
                        session_id,
                        TranscriptLine::ToolResult {
                            tool_name: tool_name.clone(),
                            output: result_text,
                            call_id: Some(call_id),
                        },
                    );
                    if let Some(reason) = agent_turn_complete {
                        agent_turn_complete_after_batch.get_or_insert(reason);
                    }
                }
                Err(err) => {
                    let error_msg = format!("Tool execution error: {err}");
                    on_tool_result(AgentLoopToolResultEvent {
                        tool_name: tool_name.clone(),
                        call_id: call_id.clone(),
                        arguments: tc.function.arguments.clone(),
                        status: "error".to_string(),
                        output_text: error_msg.clone(),
                        result: None,
                    });
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: error_msg.clone(),
                        tool_calls: None,
                        tool_call_id: Some(call_id.clone()),
                    });
                    let _ = store.append_line(
                        session_id,
                        TranscriptLine::ToolCall {
                            tool_name: tool_name.clone(),
                            arguments: tc.function.arguments.clone(),
                            call_id: Some(call_id.clone()),
                        },
                    );
                    let _ = store.append_line(
                        session_id,
                        TranscriptLine::ToolResult {
                            tool_name: tool_name.clone(),
                            output: error_msg,
                            call_id: Some(call_id),
                        },
                    );
                }
            }

            total_tool_calls += 1;
        }

        if let Some(reason) = agent_turn_complete_after_batch {
            let final_content = if completion.content.trim().is_empty() {
                format!(
                    "Astra main-agent turn complete: {reason}. Board-visible worker tasks are now the next authority surface."
                )
            } else {
                format!(
                    "{}\n\nAstra main-agent turn complete: {reason}. Board-visible worker tasks are now the next authority surface.",
                    completion.content
                )
            };
            return Ok(AgentLoopResult {
                final_content,
                iterations: iteration + 1,
                tool_calls_made: total_tool_calls,
                finish_reason: "tool_main_agent_turn_complete".to_string(),
            });
        }

        iteration += 1;
    }
}

pub(crate) fn agent_loop_tool_result_requests_main_agent_turn_complete(
    result: &crate::tools::ToolResult,
) -> Option<String> {
    let control = result.structured.as_ref()?.get("agent_loop_control")?;
    if !control
        .get("main_agent_turn_complete")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    control
        .get("reason")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(ToString::to_string)
        .or_else(|| Some("main-agent turn completion requested by tool result".to_string()))
}

pub(crate) fn agent_loop_provider_timeout_policy_for_remaining(
    timeout_policy: ProviderTimeoutPolicy,
    started_at: Instant,
    max_elapsed: Option<Duration>,
) -> ProviderTimeoutPolicy {
    let Some(max_elapsed) = max_elapsed else {
        return timeout_policy;
    };
    let remaining = max_elapsed
        .checked_sub(started_at.elapsed())
        .unwrap_or_else(|| Duration::from_millis(1))
        .max(Duration::from_millis(1));
    match timeout_policy {
        ProviderTimeoutPolicy::Blocking => ProviderTimeoutPolicy::TaskBudget(remaining),
        ProviderTimeoutPolicy::Interactive => {
            ProviderTimeoutPolicy::TaskBudget(remaining.min(Duration::from_secs(120)))
        }
        ProviderTimeoutPolicy::TaskBudget(limit) => {
            ProviderTimeoutPolicy::TaskBudget(limit.min(remaining))
        }
    }
}

pub(crate) fn finalize_agent_loop_without_tools(
    provider_trace: &crate::providers::ProviderResolutionTrace,
    messages: &mut Vec<ChatMessage>,
    store: &SessionStore,
    session_id: &str,
    cancel_token: &RuntimeCancelToken,
    on_text_delta: &mut dyn FnMut(&str),
    reasoning_effort: Option<&str>,
    timeout_policy: ProviderTimeoutPolicy,
    notice: String,
    iterations: usize,
    tool_calls_made: usize,
    finish_prefix: &str,
) -> Result<AgentLoopResult, AgentLoopError> {
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: notice.clone(),
        tool_calls: None,
        tool_call_id: None,
    });
    let _ = store.append_line(
        session_id,
        TranscriptLine::Message {
            role: "user".to_string(),
            content: notice,
        },
    );
    let completion = complete_prompt_streaming_with_tools_and_timeout_policy(
        provider_trace,
        messages,
        reasoning_effort,
        None,
        on_text_delta,
        || cancel_token.is_cancelled(),
        timeout_policy,
    )
    .map_err(AgentLoopError::ProviderError)?;
    let finish = completion.finish_reason.as_deref().unwrap_or("stop");
    if agent_loop_content_has_malformed_control_markup(&completion.content) {
        return Err(AgentLoopError::ProviderError(ProviderExecutionError::Parse(
            format!(
                "provider emitted malformed control markup in assistant content: finish_reason={finish}; chars={}",
                completion.content.len()
            ),
        )));
    }
    Ok(AgentLoopResult {
        final_content: completion.content,
        iterations,
        tool_calls_made,
        finish_reason: format!("{finish_prefix}:{finish}"),
    })
}

pub(crate) fn agent_loop_tool_budget_closeout_notice(remaining_tool_calls: usize) -> String {
    format!(
        "Runtime closeout notice: only {remaining_tool_calls} worker tool call(s) remain. Stop broad exploration now. If an artifact is still needed, use the remaining call(s) only to write the task-local artifact, then return final Markdown evidence with concrete refs, known gaps, and repair tasks. Do not start new retrieval branches."
    )
}

pub(crate) fn agent_loop_elapsed_budget_finalization_notice(
    elapsed: Duration,
    max_elapsed: Duration,
    tool_calls_made: usize,
) -> String {
    format!(
        "Runtime finalization notice: worker wall-clock budget is exhausted (elapsed={} ms, limit={} ms, tool calls made={tool_calls_made}). No more tools are available. Return final Markdown evidence now, using only already observed tool results and explicitly listing missing evidence, blocked items, and repair tasks. Do not ask for more tools and do not claim unsupported results.",
        elapsed.as_millis(),
        max_elapsed.as_millis()
    )
}

pub(crate) fn agent_loop_tool_budget_finalization_notice(
    tool_calls_made: usize,
    max_tool_calls: usize,
) -> String {
    format!(
        "Runtime finalization notice: worker tool budget is exhausted ({tool_calls_made}/{max_tool_calls}). No more tools are available. Return final Markdown evidence now, using only already observed tool results and explicitly listing missing evidence, blocked items, and repair tasks. Do not ask for more tools and do not claim unsupported results."
    )
}

pub(crate) fn agent_loop_tool_budget_denial_notice() -> String {
    "Tool budget exhausted before this requested tool call. No more tool calls will be executed in this worker run; prepare final Markdown evidence from already observed results, with explicit missing-evidence risks and repair tasks.".to_string()
}

pub(crate) fn agent_loop_empty_output_repair_notice(attempt: usize) -> String {
    format!(
        "Runtime output repair notice #{attempt}: your previous assistant turn contained no tool calls and no final text. This is invalid. Continue now by either using an available tool for the assigned task or returning non-empty final Markdown evidence. The final evidence must cite concrete refs or explicitly list missing evidence, blocked items, and repair tasks. Do not return an empty message."
    )
}

pub(crate) fn agent_loop_content_has_malformed_control_markup(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    let think_close_count = lowered.matches("</think>").count();
    let tool_call_count = lowered.matches("<tool_call>").count();
    let combined = think_close_count + tool_call_count;
    combined >= 20
        || think_close_count >= 12
        || tool_call_count >= 12
        || (combined >= 6
            && lowered
                .trim_start_matches(|character: char| {
                    character.is_ascii_digit()
                        || character.is_ascii_punctuation()
                        || character.is_whitespace()
                })
                .starts_with("</think>"))
}

pub(crate) fn allowed_tool_names_from_openai_tool_definitions(
    tool_definitions: &serde_json::Value,
) -> BTreeSet<String> {
    tool_definitions
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(|name| name.as_str())
        })
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_timeout_policy_uses_remaining_elapsed_budget() {
        let started_at = Instant::now();

        let policy = agent_loop_provider_timeout_policy_for_remaining(
            ProviderTimeoutPolicy::TaskBudget(Duration::from_secs(300)),
            started_at,
            Some(Duration::from_millis(250)),
        );
        match policy {
            ProviderTimeoutPolicy::TaskBudget(limit) => {
                assert!(limit <= Duration::from_millis(250));
                assert!(limit >= Duration::from_millis(1));
            }
            other => panic!("expected remaining task budget, got {other:?}"),
        }

        let policy = agent_loop_provider_timeout_policy_for_remaining(
            ProviderTimeoutPolicy::Blocking,
            started_at,
            Some(Duration::from_millis(200)),
        );
        match policy {
            ProviderTimeoutPolicy::TaskBudget(limit) => {
                assert!(limit <= Duration::from_millis(200));
                assert!(limit >= Duration::from_millis(1));
            }
            other => panic!("expected blocking policy to be bounded, got {other:?}"),
        }

        let policy = agent_loop_provider_timeout_policy_for_remaining(
            ProviderTimeoutPolicy::Interactive,
            started_at,
            Some(Duration::from_secs(300)),
        );
        assert_eq!(
            policy,
            ProviderTimeoutPolicy::TaskBudget(Duration::from_secs(120))
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finalize_autonomous_research_budgeted_round(
    resolved: &ResolvedProject,
    job: &AutonomousResearchJobState,
    text_buffer: &str,
    iterations: usize,
    tool_calls_made: usize,
    partial_content: String,
    session_id: &str,
    budget_kind: &str,
) -> AgentLoopResult {
    let _ = append_autonomous_research_job_event(
        resolved,
        job,
        "main_agent_round_budget_exhausted",
        json!({
            "schema_version": "autonomous_research_job_event.v1",
            "budget_kind": budget_kind,
            "iterations": iterations,
            "tool_calls_made": tool_calls_made,
            "partial_content_chars": partial_content.len(),
            "runtime_action": "preserve_partial_output_without_research_fallback",
            "session_id": session_id
        }),
    );
    let final_content = if !partial_content.trim().is_empty() {
        format!(
            "# Main Agent Round Interrupted By Budget\n\n\
             This file preserves partial main-agent output from a budget interruption. It is not a completed research artifact and must not be treated as a stage decision unless a later main-agent round explicitly incorporates it.\n\n\
             - job_id: `{}`\n\
             - budget_kind: `{}`\n\
             - iterations: `{}`\n\
             - tool_calls_made: `{}`\n\
             - runtime_action: `preserve_partial_output_without_research_fallback`\n\n\
             ## Partial Main Agent Output\n\n{}\n",
            job.job_id,
            budget_kind,
            iterations,
            tool_calls_made,
            partial_content.trim()
        )
    } else if !text_buffer.trim().is_empty() {
        format!(
            "# Main Agent Round Interrupted By Budget\n\n\
             This file preserves streamed main-agent text from a budget interruption. It is not a completed research artifact and requires a later main-agent decision before review readiness.\n\n\
             - job_id: `{}`\n\
             - budget_kind: `{}`\n\
             - iterations: `{}`\n\
             - tool_calls_made: `{}`\n\
             - runtime_action: `preserve_partial_output_without_research_fallback`\n\n\
             ## Streamed Main Agent Output\n\n{}\n",
            job.job_id,
            budget_kind,
            iterations,
            tool_calls_made,
            text_buffer.trim()
        )
    } else {
        let contract = autonomous_research_stage_contract(resolved, job);
        format!(
            "# Main Agent Round Interrupted By Budget\n\n\
             This file is a runtime diagnostic, not a completed research artifact. The main-agent round reached its `{budget_kind}` limit before producing usable content. Runtime preserved the interruption and requires a later main-agent round to decide the next research action.\n\n\
             - job_id: `{job_id}`\n\
             - stage_id: `{stage_id}`\n\
             - stage_artifact_path: `{artifact_path}`\n\
             - iterations: `{iterations}`\n\
             - tool_calls_made: `{tool_calls_made}`\n\
             - runtime_action: `blocked_no_budget_fallback_synthesis`\n\n",
            budget_kind = budget_kind,
            stage_id = contract.stage_id,
            artifact_path = contract.artifact_path,
            job_id = job.job_id,
        )
    };
    AgentLoopResult {
        final_content,
        iterations,
        tool_calls_made,
        finish_reason: format!("agent_round_{budget_kind}_exhausted"),
    }
}
