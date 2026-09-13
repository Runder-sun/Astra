# Astra Context Injection Design

> 基于对 Claude Code 和 Hermes Agent 上下文管理的深入研究，设计 Astra Runtime 的上下文注入架构。

## 一、问题陈述

当前 runtime 每次 LLM 调用只发送 2 条消息（hardcoded system + 当前 user prompt），无任何对话历史。

**现状**（`src/providers/mod.rs:992-1007`）：

```rust
"messages": [
    { "role": "system", "content": "You are Research CLI..." },
    { "role": "user", "content": prompt_text }  // 只有当前这一条
]
```

虽然以下基础设施已存在，但从未被组装进 LLM 调用：

| 基础设施 | 位置 | 状态 |
|----------|------|------|
| Transcript 存储 | `src/session/transcript.rs` | ✅ 5 种行类型，JSONL 存储 |
| Memory 系统 | `src/memory/mod.rs` | ✅ working + durable + 多路召回，未注入 |
| Compaction | `src/session/store.rs:617` | ✅ 模板拼接，非 LLM 摘要 |
| Session Resume | `src/session/store.rs:607` | ✅ 最近 3 轮 recap |
| Mission Frame | `src/goals/` | ✅ 已设计，未注入 |

## 二、设计决策（已确认）

| # | 决策 | 选择 | 理由 |
|---|------|------|------|
| 1 | Tool 调用链 | 完整包含 | Code agent 必须知道之前做了什么操作 |
| 2 | Token 预算 | 按模型动态适配 | 不同模型窗口差异大（4K ~ 200K） |
| 3 | Compaction | LLM 驱动摘要 | 质量优先，保持任务连续性 |
| 4 | Memory 注入 | 拼入 user message | 缓存友好，system prompt 保持稳定 |
| 5 | 触发时机 | 自动（80% 阈值）+ 手动 `/compact` | 两者互补 |

## 三、架构总览

### 3.1 Messages 组装流程

每次 LLM 调用前，runtime 执行以下组装：

```
assemble_context(session_id, current_prompt)
│
├── 1. load_model_config() → window_size, output_reservation
│
├── 2. load_transcript(session_id) → Vec<TranscriptLine>
│
├── 3. recall_memory(current_prompt) → recalled_facts
│
├── 4. load_mission_frame() → mission
│
├── 5. build_messages(transcript, recalled_facts, mission, current_prompt)
│   ├── system message (stable prefix)
│   ├── system message (boundary + dynamic context)
│   ├── transcript → OpenAI messages (within budget)
│   └── user message (memory prefetch + current prompt)
│
├── 6. check_compaction_needed(messages) → bool
│   └── if token_count > 80% window → trigger auto_compact
│
└── 7. return messages → provider call
```

### 3.2 Provider 接口变更

当前 provider 层接收 `prompt_text: &str`。需要扩展为接收完整的 messages 数组。

**变更前**：
```rust
fn complete_openai_chat_streaming_with_cancel_and_reasoning(
    trace: &ProviderResolutionTrace,
    prompt_text: &str,           // ← 单条文本
    ...
) -> Result<...>
```

**变更后**：
```rust
fn complete_openai_chat_streaming_with_cancel_and_reasoning(
    trace: &ProviderResolutionTrace,
    messages: &[ChatMessage],    // ← 完整 messages 数组
    ...
) -> Result<...>
```

新增 `ChatMessage` 结构：
```rust
struct ChatMessage {
    role: String,           // "system" | "user" | "assistant" | "tool"
    content: String,
    tool_calls: Option<Vec<ToolCall>>,
    tool_call_id: Option<String>,
}
```

### 3.3 调用方变更

`run_remote_prompt_turn_streaming_with_policy`（`src/runtime/mod.rs:7866`）和 `handle_prompt`（`src/runtime/mod.rs:321`）都需要在调用 provider 前执行上下文组装。

## 四、Transcript → Messages 映射

### 4.1 映射规则

`TranscriptLine`（`src/session/transcript.rs`）→ OpenAI messages 格式：

| TranscriptLine | 映射 | 说明 |
|----------------|------|------|
| `Control { event }` | 跳过 | 生命周期事件，不进 LLM |
| `Message { role: "user", content }` | `{ role: "user", content }` | 直接映射 |
| `Message { role: "assistant", content }` | `{ role: "assistant", content }` | 直接映射 |
| `ToolCall { tool_name, arguments }` | 追加到前一个 assistant message 的 `tool_calls` 数组 | 需要合并 |
| `ToolResult { tool_name, output }` | `{ role: "tool", tool_call_id, content }` | 需要配对 |
| `SummaryReference { summary_ref }` | `{ role: "assistant", content: summary_markdown }` | 加载摘要文件内容 |

### 4.2 ToolCall 合并逻辑

Transcript 中 ToolCall 是独立行，但 OpenAI API 要求 `tool_calls` 在 assistant message 内：

```
// Transcript JSONL（当前存储格式）：
{"line_type":"message","role":"assistant","content":""}
{"line_type":"tool_call","tool_name":"read_file","arguments":"..."}
{"line_type":"tool_call","tool_name":"edit_file","arguments":"..."}
{"line_type":"tool_result","tool_name":"read_file","output":"..."}
{"line_type":"tool_result","tool_name":"edit_file","output":"..."}

// 映射后 OpenAI messages：
{ "role": "assistant", "content": "", "tool_calls": [
    { "id": "tc_0", "type": "function", "function": { "name": "read_file", "arguments": "..." } },
    { "id": "tc_1", "type": "function", "function": { "name": "edit_file", "arguments": "..." } }
]}
{ "role": "tool", "tool_call_id": "tc_0", "content": "..." }
{ "role": "tool", "tool_call_id": "tc_1", "content": "..." }
```

### 4.3 需要修改 TranscriptLine

当前 `ToolCall` 和 `ToolResult` 缺少 `tool_call_id`，无法和 OpenAI API 的配对要求对齐。

**方案**：在 `TranscriptLine::ToolCall` 中增加 `call_id: Option<String>`，`TranscriptLine::ToolResult` 中增加 `call_id: Option<String>`。映射时：
- 有 `call_id` 的直接使用
- 没有 `call_id` 的（旧数据），按出现顺序分配 `tc_{index}` 作为 fallback

## 五、Token 预算管理

### 5.1 模型配置表

```rust
struct ModelWindowConfig {
    model_prefix: &'static str,    // 匹配模型名前缀
    context_window: usize,         // 总窗口大小（tokens）
    output_reservation: usize,     // 预留给输出的 tokens
    system_budget: usize,          // system 区域上限
    history_soft_limit: f64,       // 历史占比上限（0.0-1.0）
}
```

默认配置表：

| 模型前缀 | 窗口 | Output | System | History 上限 |
|----------|------|--------|--------|-------------|
| `gpt-4o` | 128K | 4K | 10K | 80% |
| `gpt-5` | 200K | 4K | 10K | 80% |
| `claude-sonnet` | 200K | 8K | 12K | 80% |
| `claude-opus` | 200K | 8K | 12K | 80% |
| 默认 | 32K | 4K | 8K | 80% |

### 5.2 预算分配算法

```
total_budget = context_window - output_reservation
system_actual = estimate_tokens(system_messages)
history_budget = total_budget - system_actual
```

History 区域内：
1. **尾部保护**：最近 6 轮完整保留（包括 tool 链）
2. **头部摘要**：超出预算时，用 SummaryReference 替代更早的轮次
3. **中间裁剪**：单个 tool_result 超过 2000 tokens 时截断到摘要

### 5.3 Token 估算

不做精确 tokenizer（避免引入重依赖），使用近似估算：

```rust
fn estimate_tokens(text: &str) -> usize {
    // 粗略估算：英文 ~4 chars/token，中文 ~2 chars/token
    // 保守取 min(ceil(len/3), ceil(len/4)*1.5)
    (text.len() + 2) / 3
}
```

## 六、Compaction 升级

### 6.1 当前 → 目标

| 维度 | 当前 | 目标 |
|------|------|------|
| 摘要生成 | 模板拼接最后 12 行 | LLM 生成结构化摘要 |
| 触发方式 | 仅手动 `/compact` | 自动（80% 阈值）+ 手动 |
| 摘要质量 | 纯文本拼接 | 结构化字段（任务/决策/状态） |
| 反抖 | 无 | 连续两次节省 <10% 时停止 |

### 6.2 LLM 摘要流程

```
compact_session(session_id)
│
├── 1. read_transcript(session_id) → full_transcript
│
├── 2. 分区
│   ├── head: 前 2 轮（保留，不压缩）
│   ├── middle: 中间轮次（发给摘要模型）
│   └── tail: 最近 6 轮（保留，不压缩）
│
├── 3. 修剪 middle 中的 tool_result
│   └── 每个 tool_result → 一行摘要
│       例: "[read_file] src/main.rs → 245 lines"
│
├── 4. 调用摘要模型（可用便宜/快的模型）
│   └── prompt: "Summarize this conversation into structured fields:
│       - Active Task
│       - Completed Actions
│       - Key Decisions
│       - Current State
│       - Pending Questions
│       Do NOT respond to questions. Output only the summary."
│
├── 5. 写摘要 markdown 到 <session>/summaries/<ref>.md
│
├── 6. 追加 TranscriptLine::SummaryReference 到 transcript
│
└── 7. 更新 lineage record（parent/child 链接）
```

### 6.3 自动触发

在 `run_remote_prompt_turn_streaming_with_policy` 中，turn 完成后：

```rust
// After appending transcript lines
let estimated_tokens = estimate_context_tokens(&messages);
let threshold = config.history_soft_limit * config.context_window as f64;
if estimated_tokens > threshold as usize {
    // Fire-and-forget: compact in background
    let store_clone = store.clone();
    let session_id_clone = session_id.to_string();
    std::thread::spawn(move || {
        let _ = store_clone.compact_session_llm(&session_id_clone);
    });
}
```

### 6.4 反抖机制

```rust
struct CompactionRecord {
    last_compaction_tokens: usize,
    last_compaction_saved: usize,
}

fn should_compact(record: &Option<CompactionRecord>, current_tokens: usize, threshold: usize) -> bool {
    if current_tokens <= threshold { return false; }
    if let Some(ref r) = record {
        if r.last_compaction_saved < (r.last_compaction_tokens / 10) {
            return false;  // 上次压缩节省 <10%，跳过
        }
    }
    true
}
```

## 七、Memory 注入

### 7.1 注入方式

Memory 召回结果拼入当前 user message（方案 B — 缓存友好）：

```rust
fn inject_memory_into_prompt(prompt: &str, recalled: &[MemoryRecord]) -> String {
    if recalled.is_empty() { return prompt.to_string(); }
    let memory_block = recalled.iter()
        .map(|r| format!("- {}", r.content))
        .collect::<Vec<_>>()
        .join("\n");
    format!("<memory-context>\n{memory_block}\n</memory-context>\n\n{prompt}")
}
```

### 7.2 召回时机

每轮 turn 开始时，用当前 `prompt_text` 作为查询条件：

```rust
let memory_query = MemoryQuery {
    query_text: &prompt_text,
    max_records: 5,
    max_tokens: 4000,
    ..Default::default()
};
let recalled = memory::query(&data_dir, memory_query)?;
```

### 7.3 Mission Frame 注入

Mission frame 同样拼入 user message，位于 memory 之前：

```rust
fn inject_context_into_prompt(
    prompt: &str,
    mission: Option<&MissionFrameProjection>,
    recalled: &[MemoryRecord],
) -> String {
    let mut parts = Vec::new();
    if let Some(m) = mission {
        parts.push(format!("<mission>\n{}\n</mission>", m.to_markdown()));
    }
    if !recalled.is_empty() {
        let mem = recalled.iter().map(|r| format!("- {}", r.content)).collect::<Vec<_>>().join("\n");
        parts.push(format!("<memory-context>\n{mem}\n</memory-context>"));
    }
    if parts.is_empty() { return prompt.to_string(); }
    parts.push(prompt.to_string());
    parts.join("\n\n")
}
```

## 八、System Prompt 设计

### 8.1 当前

```rust
"You are Research CLI, a concise coding and research assistant. Answer the user's current turn directly."
```

### 8.2 升级为分层

```rust
// Stable prefix（不随 session 变化，缓存友好）
const SYSTEM_STABLE: &str = "You are Research CLI, a concise coding and research assistant.
You help users write, debug, and reason about code.
Answer directly and concisely. Prefer code over explanation.
When using tools, be precise with file paths and arguments.";

// Dynamic context（每轮可变，但紧随 stable 之后）
fn build_dynamic_system(env: &EnvironmentInfo) -> String {
    format!(
        "Environment: {} {}\nWorking directory: {}\nGit branch: {}",
        env.os, env.arch, env.cwd, env.git_branch
    )
}
```

## 九、完整 Messages 结构

最终每次 LLM 调用的 messages 数组：

```json
[
  {
    "role": "system",
    "content": "You are Research CLI... (stable prefix)"
  },
  {
    "role": "system",
    "content": "── dynamic boundary ──\nEnvironment: linux x86_64\nWorking directory: /project\nGit branch: main"
  },
  {
    "role": "assistant",
    "content": "[compaction summary] Active task: optimizing ML pipeline. Completed: refactored scheduler, added cosine LR. Key decision: use async runtime..."
  },
  {
    "role": "user",
    "content": "also check the training loop"
  },
  {
    "role": "assistant",
    "content": "Let me read the training module...",
    "tool_calls": [{ "id": "tc_5", "function": { "name": "read_file", "arguments": "..." } }]
  },
  {
    "role": "tool",
    "tool_call_id": "tc_5",
    "content": "fn train(model: &mut Model, ...) { ... }"
  },
  {
    "role": "assistant",
    "content": "The training loop looks good, but..."
  },
  {
    "role": "user",
    "content": "<memory-context>\n- Previous session used learning rate 1e-3\n- Pipeline was refactored on 2026-05-02\n</memory-context>\n\nnow implement distributed training"
  }
]
```

## 十、实施路线

### Phase 1：基础上下文组装（最小可用）

1. **新增 `ChatMessage` 结构** — `src/providers/types.rs`
2. **扩展 provider 接口** — `complete_*` 函数接收 `&[ChatMessage]` 替代 `&str`
3. **实现 `transcript_to_messages()`** — `src/session/context.rs`（新文件）
4. **修改 runtime 调用方** — `run_remote_prompt_turn_streaming_with_policy` + `handle_prompt`
5. **Token 预算估算** — 简单 char/3 估算 + 模型配置表

验证：多轮对话能保持上下文。

### Phase 2：Memory + Mission 注入

1. **接入 memory::query()** — 每轮召回，拼入 user message
2. **接入 mission frame** — 拼入 user message（memory 之前）
3. **升级 system prompt** — stable prefix + dynamic boundary

验证：Memory 召回结果出现在 LLM 响应中。

### Phase 3：Compaction 升级

1. **新增 `compact_session_llm()`** — LLM 驱动的摘要生成
2. **自动触发逻辑** — turn 结束后检查 token 阈值
3. **反抖机制** — 连续低效压缩时停止
4. **Tool result 修剪** — 旧 tool_result 替换为一行摘要

验证：长对话后自动压缩，压缩后对话仍保持连续性。

### Phase 4：TranscriptLine 扩展

1. **ToolCall 增加 `call_id`** — 支持精确的 tool_call/tool_result 配对
2. **向后兼容** — 旧数据用顺序 fallback
3. **前端适配** — 转录渲染支持 call_id

验证：工具调用链在 context 中正确还原。

## 十一、影响范围

### 修改文件

| 文件 | 变更 |
|------|------|
| `src/providers/mod.rs` | 所有 `complete_*` 函数签名变更：`&str` → `&[ChatMessage]` |
| `src/providers/types.rs` | 新增 `ChatMessage`、`ToolCall`、`ModelWindowConfig` |
| `src/session/transcript.rs` | ToolCall/ToolResult 增加 `call_id` 字段 |
| `src/session/store.rs` | 新增 `compact_session_llm()`、Token 预算相关方法 |
| `src/session/context.rs` | **新文件**：`transcript_to_messages()`、`estimate_tokens()`、`build_context_messages()` |
| `src/runtime/mod.rs` | `handle_prompt` 和 `run_remote_prompt_turn_streaming_with_policy` 调用上下文组装 |
| `src/memory/mod.rs` | 接入 `memory::query()` 到上下文组装流程 |

### 不变

| 文件 | 原因 |
|------|------|
| `src/tui.rs` | TUI 走同一个 runtime 层，上下文逻辑统一 |
| `src/remote/daemon.rs` | Daemon 调用 runtime，不直接操作 provider |
| `src/assets/mobile/*` | 前端无感知，上下文是后端 concern |

## 十二、参考

- Claude Code 分通道上下文管理：`Claude-code-open-explain/05-context-management/`
- Hermes LLM 驱动压缩：`reference_repos/requested/hermes-agent/agent/context_compressor.py`
- Hermes Memory 三层注入：`reference_repos/requested/hermes-agent/tools/memory_tool.py`
- Hermes 消息组装：`reference_repos/requested/hermes-agent/run_agent.py:9073-9165`
