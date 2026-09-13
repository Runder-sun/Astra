use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "line_type", rename_all = "snake_case")]
pub enum TranscriptLine {
    Control {
        event: String,
    },
    Message {
        role: String,
        content: String,
    },
    ToolCall {
        tool_name: String,
        arguments: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    ToolResult {
        tool_name: String,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
    },
    SummaryReference {
        summary_ref: String,
    },
}
