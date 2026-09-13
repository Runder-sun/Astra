use crate::support::init_git_workspace;
use research_cli::session::context::{build_context_messages, transcript_to_messages};
use research_cli::session::index::ResumeSelector;
use research_cli::session::store::SessionStore;
use research_cli::session::transcript::TranscriptLine;

#[test]
fn session_store_persists_identity_and_transcript_round_trip() {
    let workspace_root = init_git_workspace("session_store_round_trip");
    let store = SessionStore::new(workspace_root.join(".pmcli"));

    let identity = store
        .create_session(Some("Bootstrap Session".to_string()))
        .expect("session should create");

    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Control {
                event: "session_open".to_string(),
            },
        )
        .expect("control line should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "hello".to_string(),
            },
        )
        .expect("message line should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::SummaryReference {
                summary_ref: "summary_001".to_string(),
            },
        )
        .expect("summary ref line should append");

    let loaded = store
        .load_session(&identity.session_id)
        .expect("session should load");
    let lines = store
        .read_transcript(&identity.session_id)
        .expect("transcript should read");

    assert_eq!(loaded.identity.session_id, identity.session_id);
    assert_eq!(loaded.identity.title.as_deref(), Some("Bootstrap Session"));
    assert_eq!(lines.len(), 3);
    assert!(matches!(lines[0], TranscriptLine::Control { .. }));
    assert!(matches!(lines[1], TranscriptLine::Message { .. }));
    assert!(matches!(lines[2], TranscriptLine::SummaryReference { .. }));
}

#[test]
fn latest_resume_target_is_project_scoped() {
    let workspace_root = init_git_workspace("session_latest");
    let store = SessionStore::new(workspace_root.join(".pmcli"));

    let first = store
        .create_session(Some("first".to_string()))
        .expect("first");
    let second = store
        .create_session(Some("second".to_string()))
        .expect("second");

    let resolved = store
        .resolve_resume_target(ResumeSelector::Latest)
        .expect("latest should resolve");

    assert_eq!(resolved.session_id, second.session_id);
    assert_ne!(resolved.session_id, first.session_id);
}

#[test]
fn ambiguous_resume_prefix_is_rejected() {
    let workspace_root = init_git_workspace("session_ambiguous");
    let store = SessionStore::new(workspace_root.join(".pmcli"));

    let first = store
        .create_session_with_id("sess_shared_a0001".to_string(), None)
        .expect("first");
    let second = store
        .create_session_with_id("sess_shared_b0002".to_string(), None)
        .expect("second");

    let err = store
        .resolve_resume_target(ResumeSelector::Prefix("sess_shared".to_string()))
        .expect_err("ambiguous prefix should fail");

    assert!(
        err.to_string().contains("ambiguous session selector"),
        "unexpected error: {err}"
    );
    assert_ne!(first.session_id, second.session_id);
}

#[test]
fn transcript_round_trip_preserves_tool_turn_structure() {
    let workspace_root = init_git_workspace("session_tool_turn_round_trip");
    let store = SessionStore::new(workspace_root.join(".pmcli"));

    let identity = store
        .create_session(Some("Tool Turn".to_string()))
        .expect("session should create");

    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "summarize this".to_string(),
            },
        )
        .expect("user message should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: "I will inspect the workspace.".to_string(),
            },
        )
        .expect("assistant message should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::ToolCall {
                tool_name: "inspect".to_string(),
                arguments: "{\"path\":\"src\"}".to_string(),
                call_id: Some("call_inspect_1".to_string()),
            },
        )
        .expect("tool call should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::ToolResult {
                tool_name: "inspect".to_string(),
                output: "found 3 files".to_string(),
                call_id: Some("call_inspect_1".to_string()),
            },
        )
        .expect("tool result should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: "Here is the summary.".to_string(),
            },
        )
        .expect("follow-up should append");

    let transcript = store
        .read_transcript(&identity.session_id)
        .expect("transcript should read");
    let messages = transcript_to_messages(&transcript, &store, &identity.session_id);

    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(
        messages[1].tool_calls.as_ref().map(|calls| calls.len()),
        Some(1)
    );
    assert_eq!(messages[2].role, "tool");
    assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_inspect_1"));
    assert_eq!(messages[3].role, "assistant");
    assert_eq!(messages[3].content, "Here is the summary.");
}

#[test]
fn build_context_messages_preserves_complete_turns_when_trimming_history() {
    let workspace_root = init_git_workspace("session_turn_trim");
    let store = SessionStore::new(workspace_root.join(".pmcli"));

    let identity = store
        .create_session(Some("Turn Trim".to_string()))
        .expect("session should create");

    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "turn one".to_string(),
            },
        )
        .expect("first user message should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: "turn one ack".to_string(),
            },
        )
        .expect("first assistant message should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "user".to_string(),
                content: "turn two".to_string(),
            },
        )
        .expect("second user message should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: "I will inspect the workspace now.".to_string(),
            },
        )
        .expect("second assistant message should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::ToolCall {
                tool_name: "inspect".to_string(),
                arguments: "{\"path\":\"src\"}".to_string(),
                call_id: Some("call_inspect_1".to_string()),
            },
        )
        .expect("tool call should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::ToolResult {
                tool_name: "inspect".to_string(),
                output: "found 3 files".to_string(),
                call_id: Some("call_inspect_1".to_string()),
            },
        )
        .expect("tool result should append");
    store
        .append_line(
            &identity.session_id,
            TranscriptLine::Message {
                role: "assistant".to_string(),
                content: "Here is the follow-up.".to_string(),
            },
        )
        .expect("follow-up should append");

    let messages = build_context_messages(
        &store,
        "project_fixture",
        &identity.session_id,
        "continue from current state",
        "unknown-model",
    );

    let history_start = messages
        .iter()
        .position(|message| message.role != "system")
        .expect("context should contain user prompt");
    let history_end = messages.len() - 1;
    let history = &messages[history_start..history_end];

    assert!(history.len() >= 6);
    let tool_assistant = history
        .iter()
        .find(|message| message.tool_calls.is_some())
        .expect("history should retain assistant tool call");
    assert_eq!(tool_assistant.role, "assistant");
    let tool_result = history
        .iter()
        .find(|message| message.role == "tool")
        .expect("history should retain matching tool result");
    assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_inspect_1"));
    assert_eq!(tool_result.content, "found 3 files");
}
