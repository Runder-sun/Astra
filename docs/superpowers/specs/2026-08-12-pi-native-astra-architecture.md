# Pi-native Astra 自动研究 Agent 架构

日期：2026-08-12

状态：Pi-native 重构实施基线；P1-P4 已在 `packages/astra` 落地并有测试/真实 fixture E2E。P5 已按方案 2 完成：旧 Rust runtime 整体归档到 `legacy/rust/`，默认产品 build/release 排除。

## 1. 目标与非目标

Astra 的目标不是给现有 Rust CLI 换一个模型循环，也不是把 Pi 当作一个
外部 provider。目标是把 Pi 的完整源码树作为新产品主干，在 Pi 的 session、
agent loop、CLI、TUI、RPC、skills、extensions 和 provider runtime 之上，
生长出 Astra 的自动/半自动研究系统。

最终用户面对的是 Pi 的统一产品表面：同一个 `pi`/`astra` 进程、同一种
session、同一套 TUI/print/JSON/RPC 事件。研究能力通过内置 Astra extension、
研究 store、supervisor 和领域 skills 提供。

非目标：

- 不保留 Rust runtime 作为新的 agent 真相源。
- 不复制 Pi 的 TUI、CLI、session 或 provider loop。
- 不把研究状态塞进提示词、Markdown 看板或模型记忆中。
- 不把 Pi 尚未完成的 `AgentHarness` API 当成当前生产能力。
- 不用“启动很多 Pi 进程”替代 TaskPacket、证据、评审和恢复协议。

## 2. 研究结论与证据等级

### 2.1 Pi 当前可直接复用的成熟能力

上游 Pi `0.84.1` 的 `packages/coding-agent` 已经提供：

- `AgentSessionRuntime`：同一 session 的 new/resume/fork/switch 生命周期。
- `AgentSession` 与 `Agent`：消息、工具调用、steer/follow-up、abort、
  compaction 和事件流。
- `main(args, { extensionFactories })`：交互 TUI、print、JSON、RPC 共用
  一个 runtime composition root。
- `ExtensionAPI`：`before_agent_start`、`context`、`tool_call`、
  `tool_result`、`turn_end`、`session_before_compact`、`session_start`、
  `session_shutdown` 等生命周期钩子。
- `registerTool`、`registerCommand`、`registerFlag`、`setActiveTools`、
  自定义 renderer/widget/status。
- project/user skills 与 prompt templates 的资源发现和 trust 流程。
- JSONL session tree、branch/fork、compaction summary、RPC integration。
- `ModelRuntime`/`pi-ai` 的多 provider、认证、重试和模型目录。

这些能力直接成为 Astra 新 runtime 的基础，不再由 Rust 重写。

一手文档：Pi 的
[extension 指南](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md)、
[SDK 指南](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sdk.md) 和
[harness 设计](https://github.com/earendil-works/pi/blob/main/packages/agent/docs/harness.md)。
实现判断以本分支锁定的 `0.84.1` 源码和测试为准，不以 README 的未来计划为准。

### 2.2 Pi 文档中有价值但当前不能直接依赖的能力

上游 `packages/agent/src/harness` 和 senpi 的 `harness-v2`/`durable-harness`
文档提出了 durable operation、lane/ref、单写者、checkpoint、幂等恢复和
subagent parent linkage。这些设计非常适合 Astra，但当前上游实现仍有
`HarnessNotImplemented` 路径。因此 Astra 要在 Pi 源码上补齐一套最小、可测的
durable supervisor/lane 能力，或者先在 `packages/astra` 实现等价的 store 和
driver；不能假装这些 API 已经完成。

### 2.3 外部 Pi 生态验证出的实践

以下实践均在 2026-08-14 由公开源码和 README 交叉核验；它们不是官方示例，
而是 Pi 生态中已经运行的产品和扩展：

| 来源 | 实际模式 | Astra 吸收方式 |
| --- | --- | --- |
| [`pi-chat`](https://github.com/earendil-works/pi-chat) | 每个 channel 有独立 workspace、Gondolin VM、持久日志、memory、skills、worker status；remote stop/compact/new | 长任务使用隔离 worker workspace；研究事件和状态可远程投影；不把 channel log 当研究事实 |
| [`pi-messenger`](https://github.com/nicobailon/pi-messenger) | planner -> DAG tasks -> bounded parallel waves -> automatic reviewer；project store、registry、progress、artifacts、file reservation | TaskPacket 依赖图、并发上限、worker registry、review retry、artifact manifest；研究版增加证据身份和 canonical adoption |
| [`rho`](https://github.com/mikeyobrien/rho) | daemon/heartbeat/任务队列/持续记忆与前台 session 解耦 | supervisor lease、wake event、heartbeat、restart recovery；前台 Pi session 可以随时接管 |
| [`pi-hermes-memory`](https://github.com/chandra447/pi-hermes-memory) | extension lifecycle 负责项目检测、两级 memory、skills、后台 consolidation 和 session search | 分层 research memory、阶段 skill discovery、session/compaction writeback、可审计 consolidation；禁止把 recall 当 hard permission |
| [`senpi`](https://github.com/code-yeongyu/senpi) | fork Pi 但把产品策略优先放 builtin extensions；修改目录维护 `changes.md`；加入 permission、continuation loop、动态 prompt、compaction 保护 | Pi core 改动最小且记录 upstream 差异；Astra 策略进入 builtin extension；对 provider/工具/compaction 加显式 guard |
| [`pi-web-access`](https://github.com/nicobailon/pi-web-access) | 多检索 provider、SSRF policy、缓存、source check、PDF extraction | 文献检索工具必须有来源、缓存、SSRF/域策略、原文 ref 和提取质量，不直接把搜索结果当证据 |
| [`pi-interactive-shell`](https://github.com/nicobailon/pi-interactive-shell) | 可观察 PTY、interactive/dispatch/monitor 模式、token-efficient output、用户随时 takeover | 实验/长命令通过可观察 worker runtime；危险或交互式操作保留人工接管边界；事件驱动监控优先于轮询 prompt |

这些项目证明的是工程模式，不是 Astra 可以直接复制的领域模型。

外部实践的共同约束是：extension 适合注入策略和投影，独立 session 适合隔离
上下文和权限，长期 loop 必须有进程外 durable state；没有一个项目证明“多开
几个 agent”本身就能替代任务 contract、证据 identity、单写者和恢复协议。

## 3. Astra 必须保留的核心设计

### 3.1 目标运行与 MissionFrame

用户目标必须被解析为结构化 goal run：目标、边界、自动化等级、预算、权限、
验收条件、当前 stage、下一动作、阻塞和用户 gate。目标不能由模型通过改写
system prompt 隐式改变。

自动化等级保留三档：

- `collaborative`：规划和建议自动化，派发/关键推进需要确认。
- `autonomous`：低风险任务和普通 review 自动推进，方向、预算、最终 claim
  仍受 gate。
- `full`：在明确预算、权限、阶段和验收条件内自动推进，遇到硬边界暂停。

等级只是一组默认 policy；每个 action 还要经过 hard boundary、用户 override、
goal policy 的统一解析。

当前 policy 的可执行语义如下：

| 模式 | worker 派发 | 普通 stage 关闭 | `gate: user` stage 关闭 | 硬预算/权限边界 |
| --- | --- | --- | --- | --- |
| `collaborative` | 每个 stage 先确认 | 再次确认 | 再次确认 | 必须暂停并显式调整 |
| `autonomous` | 自动 | main-agent 决策后自动 | 用户确认后再由 main-agent 决策 | 必须暂停并显式调整 |
| `full` | 自动 | main-agent 决策后自动 | main-agent 决策后自动 | 必须暂停并显式调整 |

gate 是 durable event，不是 TUI 临时状态。`resume` 对 stage gate 只批准当前
stage/phase；对 budget gate，必须先把对应上限提高到已消耗值之上，不能用普通
resume 绕过。

### 3.2 Stage DAG 与研究语义

研究阶段是领域 DAG，不是 Pi 的 prompt template：

```text
validation -> literature -> idea -> novelty -> refine -> experiment-plan
 -> implement-solution -> run -> monitor -> result-to-claim -> paper-plan
 -> paper-write -> paper-compile -> research-review
```

`rebuttal` 和 `meta-optimize` 是 review 后的可选 workflow，尚未进入默认 stage
DAG。只有补齐输入输出 contract、回退边和验收测试后才能加入主链。

每个 stage 定义输入 artifact、worker task family、输出 artifact type、字段、
acceptance checks、failure signals、默认 edge、human gate 和可用 skills。
转段只能由主 agent 提交结构化 closure decision，supervisor 负责校验和持久化。

### 3.3 Main agent authority

主 agent 是唯一研究决策者，Pi agent loop 只是执行它的 turn：

- 可以读取状态、分析证据、发布 TaskPacket、接受/拒绝 worker evidence、
  请求 review、决定 route/repair/adoption/cleanup。
- 不直接代替 worker 完成被委派的检索、实验或实现任务。
- 不能绕过 review、预算、权限、replacement id 或用户 gate。

Runtime/supervisor 只做调度、校验、持久化、投影、lease、恢复和阻断，不能
推断研究方向或自动生成隐藏任务。

### 3.4 TaskPacket 与角色边界

每个 worker 输入必须是可恢复的 TaskPacket，而不是自由文本：

- task id、job id、stage/stage execution id、role、objective
- input artifact refs、required canonical artifacts
- required output artifact type/fields、acceptance checks、failure signals
- dependencies、review target、blocker、supersedes/replacement refs
- workspace/path scope、tool policy、write authority、budget、resume policy
- output manifest、review gate、replay/idempotency key

角色边界：

- `main-agent`：研究控制工具和全局读工具。
- `worker`：分配的 Pi 工具、局部写入和证据产物；没有 route/adoption/cleanup
  控制工具。
- `reviewer`：只读证据与 review 工具；不能修改 canonical artifact 或关掉
  blocking obligation。
- `supervisor`：非 LLM 进程/服务，不能作研究判断。

### 3.5 Evidence -> Review -> Adoption -> Canonical

worker 输出必须按以下顺序流动：

```text
worker output
  -> output manifest + trace
  -> stage-local accepted-evidence candidate
  -> independent semantic review
  -> main-agent accept/reject/defer decision
  -> explicit adoption record
  -> canonical artifact materialization
  -> integration check / baseline visibility
```

review failure 转成 blocking obligation；不能只写评论。已有 canonical target
被替换时必须指定 `replacement_of_artifact_ids`，旧内容明确 retired，禁止静默
覆盖。目录 artifact 必须 manifest-backed 且路径安全。

### 3.6 Supervisor、lease 与恢复

长期 auto-research 需要外层 supervisor loop：

- 读取 canonical job state 和 event ledger。
- 获取/续租单写者 `research_supervisor_lease`。
- 根据状态派发 ready TaskPacket，收集 worker manifest，触发主 agent round/review。
- 写 heartbeat；超时产生 wake/recovery event。
- 每个 transition 使用 deterministic idempotency key。
- crash 后从最后 durable boundary 恢复，不重跑未知安全性的非幂等 tool。

Pi 的 inner loop 仍负责一次模型 turn 的 assistant/tool loop；Astra 的 outer
loop 负责研究阶段和任务状态。两者不能合并成一个大 while-loop。

MissionFrame 全局预算与 TaskPacket 局部预算分开：`maxTasks` 计算整个 job 的
worker/reviewer TaskPacket；`maxTurns` 计算 worker/reviewer/main-agent Pi 子会话
调用；`maxCostUsd` 从 Pi JSON 的 authoritative assistant `message_end` usage
累计。单个 TaskPacket 的 `maxTurns/maxToolCalls/maxRuntimeMs` 仍只约束该子会话。
子调用可能使 cost 从上限以下跨到上限以上，因此调用结束后先持久化实际 cost
和已经产生的领域 transition，再在下一 action 前进入 budget gate。

### 3.7 Memory 与 skills

skills 是可发现的指令/工具知识，不是 canonical state。按 stage/role 分层：

- `literature`：source retrieval、citation、closest-family comparison。
- `experiment`：run protocol、metric、integrity、promotion/discard。
- `review`：claim/evidence/reviewer rubric。
- `implementation`：code/test/manifest。

研究 memory 分为 hot mission/current stage、session summary、project durable
decisions、artifact/evidence index、research wiki、failure lessons。写入必须有
来源、时间、scope 和 promotion policy；模型不能通过普通文本把 unsupported
claim 写进长期记忆。

实现中 package stage/role 指令是带标准 frontmatter 的 Pi skills。每个 Pi child
session 关闭 ambient skill discovery，只通过显式 `--skill` 绑定当前 stage 和
worker/reviewer/main-agent role 对应文件；Astra extension 仍负责 durable mission
context 和 project-local override，不复制 Pi skill loader。

## 4. Pi-native 总体架构

```text
Pi source monorepo
├── packages/ai                 provider/auth/stream primitives
├── packages/agent              Agent + loop + (Astra durable additions)
├── packages/coding-agent       SessionRuntime + CLI/TUI/RPC + extension API
├── packages/tui                Pi terminal UI
└── packages/astra              domain product layer
    ├── extension/              builtin Astra extension/hooks/tools
    ├── store/                  .astra canonical state + JSONL event ledger
    ├── research/               stages, contracts, evidence, reviews, adoption
    ├── supervisor/              lease, tick, wake, recovery, dispatch
    ├── workers/                 child Pi process/session adapters
    ├── memory/                  stage/project memory and promotion
    ├── cli/                     Astra flags/subcommands routed to Pi runtime
    └── tests/                   fake provider, contract, integration, e2e
```

### 4.1 Pi core 修改边界

只在 Pi core 增加真正通用的能力：

1. durable operation/checkpoint primitives；
2. deterministic child session linkage；
3. retry/abort/recovery records；
4. extension-facing lane/ref API（仅在实现和测试完整后开放）。

这些修改必须保持 `packages/*/changes.md`，每次 upstream sync 有独立记录。
研究 stage、TaskPacket、evidence、claim 和 canonical ledger 不进入 Pi core，
全部放 `packages/astra`。

### 4.2 Astra extension

内置 extension 负责把领域能力接入 Pi：

- `session_start/shutdown`：加载/flush `.astra` store，恢复 job/lease。
- `before_agent_start`：注入当前 mission/stage/obligation 的短 context，
  不把全量 ledger 塞进 prompt。
- `context`：按 stage、role 和 token budget 选择可见 evidence。
- `tool_call`：统一 permission、scope、role、budget、replacement 和
  destructive gate。
- `tool_result/turn_end`：记录 receipts、round summary、主 agent decision
  候选和 supervisor wake。
- `session_before_compact`：生成保留 goal/current stage/open obligations/
  evidence refs/next action 的研究 checkpoint summary。
- `session_shutdown`：flush event ledger、heartbeat、pending state。

### 4.3 Pi TUI/CLI/RPC

不再维护 Rust TUI/CLI 两套行为：

- `astra` launcher 调用 Pi `main()`，内置 Astra extension factory。
- 交互 mode 使用 Pi TUI 的 transcript、footer、status、widget、overlay。
- print/json 使用 Pi 的输出模式。
- RPC 使用 Pi JSONL command/event protocol，增加 Astra domain event payload。
- `/research`、`/research-status`、`/research-tasks`、`/research-review`、
  `/research-continue`、`/research-pause` 是 extension commands。
- shell 自动化命令只做 Pi runtime 的启动/查询投影，不创建第二个 agent runtime。

当前实现状态：`createAstraExtension()` 已注册 status/widget、`/research`、
`/research-status`、`/research-tasks`、`/research-review`、`/research-continue`、
`/research-pause`、`/research-resume`；Pi RPC UI request 会收到同一组 status/widget
投影。shell `astra research ...` 被翻译成 extension flag 后进入 Pi `main()`；
`session_start` 和 slash command 再调用同一个 `runResearchControl()`，不存在绕过
Pi composition root 的第二条 shell control 路径。

```text
astra research run
  -> Pi main()
  -> AgentSessionRuntime + persisted parent session
  -> Astra builtin extension flag
  -> runResearchControl()
  -> ResearchSupervisor
  -> Pi child sessions
```

## 5. Agent 拓扑与执行策略

### 5.1 主 session

一个用户/项目主 Pi session 绑定一个 `main` ref。它可以对话，也可以在
auto-research 中执行 supervisor 触发的 main-agent round。主 session 的 transcript
仍是用户可见的对话；研究状态通过 custom entries 和 `.astra` refs 关联。

### 5.2 Worker session

worker 默认使用独立 Pi 子进程 + 独立持久 session：

- 子进程通过 Pi JSON/RPC 启动，不复制 agent loop。
- session id 由 `job_id + task_packet_id + attempt` 确定，重启可重连。
- worktree/workspace 由 TaskPacket scope 创建，主仓库不直接写入。
- worker 输出写入 artifact directory 和 output manifest；事件回传 supervisor。
- 并发由 supervisor 限制，默认小于等于 4，可按预算配置。

在无需进程隔离的低风险、短任务中可以使用同一 Node 进程创建 child
`AgentSession`，但仍必须有独立 session id、工具集和 artifact refs；这是优化，
不是默认路径。

### 5.3 Reviewer session

reviewer 是独立 session，接收 stage artifact/evidence snapshot，只读运行。它的
review record 必须引用 task/evidence set，返回 structured verdict：`pass`、
`fail`、`partial`、`blocked` 和 findings/required repairs。

### 5.4 不采用的拓扑

- 不让 worker 共享主 session transcript，避免上下文和权限泄漏。
- 不让所有 worker 直接写同一目录。
- 不让多个进程同时成为一个 job 的 supervisor。
- 不让 reviewer 与被审 worker 共用同一上下文，以免失去独立性。

## 6. 两层 Loop 设计

### 6.1 Pi inner loop

Pi `Agent` 负责：provider request -> assistant message -> tool batch -> tool result
-> next turn，具备 steering/follow-up、abort、retry、compaction。Astra 不重新
实现该循环，只通过 hooks 和 tools 约束行为。

### 6.2 Astra outer loop

研究 supervisor 每次 tick 是一个可恢复事务：

1. load job snapshot + event tail；
2. acquire lease，校验 budget/deadline/automation policy；
3. reconcile worker sessions/manifest/review/obligation；
4. find ready tasks；
5. dispatch bounded worker wave；
6. call main-agent round to decide publish/accept/review/route；
7. apply only validated transition；
8. write event, snapshot, heartbeat, next wake;
9. release/renew lease。

外层 loop 不直接生成研究内容，不在没有 main-agent decision 的情况下自动补任务。

## 7. Canonical state 与目录

Pi `.pi/` 只存 Pi 配置、资源和 session；Astra canonical state 使用项目根的
`.astra/`：

```text
.astra/
├── project.json                    # project identity + migration metadata
├── jobs/<job_id>/
│   ├── job.json                    # current snapshot, policy, stage, budget
│   ├── events.jsonl                # append-only domain event ledger
│   ├── lease.json                  # supervisor lease/heartbeat
│   ├── stages/<execution_id>/
│   │   ├── contract.json
│   │   ├── closure.json
│   │   ├── accepted-evidence/index.json
│   │   ├── reviews/
│   │   └── artifacts/
│   ├── tasks/<task_id>/
│   │   ├── task-packet.json
│   │   ├── directive.md
│   │   ├── status.json
│   │   └── output-manifest.json
│   ├── sessions/                    # child Pi session refs/metadata
│   └── memory/
├── canonical/                      # adopted project artifacts + ledger
├── memory/                         # project durable memory
└── migrations/                     # .pmcli import reports
```

权威边界：

- Pi session JSONL：对话、Pi message/tool/compaction history。
- `.astra` event ledger/snapshot：goal、stage、task、evidence、review、adoption、
  lease、budget、cleanup 等研究事实。
- artifact 文件：内容本身；ledger 记录其 checksum、来源和 lifecycle。
- TUI/RPC/board：只读投影，不能成为第三份真相。

## 8. 权限与安全

Pi 官方明确没有 sandbox；project trust 也不是执行隔离。因此 Astra 必须：

- 在 `tool_call` hook 做 role/tool/path/permission/budget preflight。
- 用 Pi builtin tool allowlist + Astra custom tool allowlist 分离 main/worker/reviewer。
- worker 默认 worktree scope；canonical adoption 只能由 main-agent tool 执行。
- destructive、外部发布、高成本 provider、方向 pivot、最终 claim 必须 gate。
- unattended 运行使用容器/VM/OS policy，不能把 Pi extension 当 sandbox。
- web/literature tools 使用 SSRF、域名、大小、超时、缓存和 source provenance policy。
- 每个写操作有 deterministic call id、receipt、checksum；非幂等工具默认不自动重放。

## 9. 迁移矩阵

| 当前 Rust/Astra 能力 | 新归属 | 处理 |
| --- | --- | --- |
| `legacy/rust/src/runtime/agent_loop.rs` | Pi `packages/agent` | 从产品主干移除重复实现；只在 Pi loop primitive 上补通用 durable/retry 语义 |
| Rust provider/config/model routing | Pi `packages/ai` + `ModelRuntime` | 迁移配置和兼容模型映射，删除重复 provider loop |
| Rust TUI/remote projections | Pi `packages/coding-agent` + RPC/TUI | 删除独立渲染真相；Astra 只提供 widgets/events/commands |
| `goals` / `orchestration` | `packages/astra/research` + store | 保留 goal/stage 语义，重写存储和状态转换 |
| `TaskPacket` / role packages | `packages/astra/workers` | 保留字段和 authority boundary，适配 Pi child sessions |
| accepted evidence / canonical artifacts | `packages/astra/research` | 保留完整 lifecycle、checksum、replacement、retirement |
| review/obligations/route changes | `packages/astra/research` | 保留 main-agent decision gate，runtime 只校验 |
| provider worker runner | `packages/astra/workers` | 改为 Pi JSON/RPC child session；不再嵌入 provider-specific loop |
| Rust `.pmcli` state | migration adapter | 只读导入并生成 `.astra/migrations/*` 报告；导入后 `.astra` 为新权威 |
| skills/role souls | `.pi/skills`, `.pi/agents`, package resources | 使用 Pi discovery；内容改成 stage/role skill，状态不放 skill |

明确从产品主干移除：重复 CLI parser、重复 TUI renderer、重复 provider tool loop、
自由文本 orchestration markdown 作为真相、runtime 自动推导研究任务的 fallback。
旧实现只在 `legacy/rust/` 保留为迁移和 parity 归档。

## 10. 分阶段实施

### P0：设计与 upstream 基线

- 固定 Pi commit/version 和源码 import 方式。
- 建立 `packages/*/changes.md` 和 upstream sync 规则。
- 跑 Pi `npm run check` 与关键 coding-agent tests。
- 为 Astra 记录 Rust 行为 baseline，不继续扩展 Rust 功能。

### P1：Pi-native product shell

- 新建 `packages/astra` package、内置 extension factory、launcher。
- 让 TUI/print/JSON/RPC 都能加载 Astra extension。
- 加 `.astra` project store、event ledger、project/session binding、read-only
  `.pmcli` migration。

### P2：领域 contract 与工具

- 实现 Goal/MissionFrame、Stage DAG、TaskPacket、role/tool policy。
- 实现 main/worker/reviewer custom tools 和 permission middleware。
- 实现 evidence/review/adoption/canonical lifecycle。

### P3：Pi child sessions 与 supervisor

- 实现 deterministic worker session、bounded concurrency、registry、artifact manifest。
- 实现 lease/heartbeat/wake/recovery、idempotent transitions。
- 实现 main-agent round 只通过 Pi session/tool loop 进行。

### P4：完整研究 loop 与 UI

- 实现 `/research:*` 命令、TUI status/widget/overlay、RPC domain events。
- 实现 stage skills、research memory、compaction checkpoint。
- 实现 `astra research run/tick/status/resume/pause` 兼容 wrapper。

### P5：迁移与删除旧 runtime

- 导入已有 `.pmcli` 项目并核对 artifact/evidence/review refs。
- 同一 benchmark 上对比新旧运行结果。
- 用户明确选择方案 2后，将完整 Cargo crate 移入 `legacy/rust/`；根目录和默认
  Pi build、test、package、release 不再包含 Rust 产品入口。
- 冻结归档中的 Rust agent loop、独立 TUI/CLI、重复 provider worker 路径，禁止
  新增产品能力；只读迁移和 parity 工具保留在归档内。

## 11. 测试与完成标准

### 11.1 Pi 基线

- Pi core、ai、coding-agent、tui 的 `npm run check` 和相关单测通过。
- Pi TUI/print/JSON/RPC 在 Astra extension 开启时均能启动。
- 没有把 fake provider 测试误称为 live provider 成功。

### 11.2 Astra contract tests

- goal policy 正确限制 collaborative/autonomous/full action。
- worker/reviewer tool surface 与 main-agent disjoint。
- stale task/job/evidence refs 被拒绝。
- failed review 变成 blocking obligation。
- 未 review 的 evidence 不能 adoption。
- replacement 没有旧 artifact id 时被拒绝。
- unsafe directory manifest、partial copy、checksum mismatch 被拒绝。
- lease 冲突、过期 heartbeat、重复 event、crash recovery 可重放且不重复写。
- compaction summary 保留 mission/stage/obligation/evidence/next action。

### 11.3 完整 auto-research 验收

在空项目、隔离 provider fixture、无人工介入条件下运行一条真实 Pi-native job：

```text
validation -> literature -> idea -> novelty -> refine -> experiment-plan
-> implement-solution -> run -> monitor -> result-to-claim -> paper-plan
-> paper-write -> paper-compile -> research-review
```

fixture provider 必须驱动真实 Pi Agent/tool loop，而不是直接写结果文件。运行中
至少包含：

- 一个并行 worker wave；
- 一个 review failure -> repair TaskPacket -> rerun；
- 一个 explicit main-agent adoption 和 replacement/retirement；
- 一次 provider/worker restart 或 supervisor crash recovery；
- 一次 compaction/resume；
- 最终 canonical report、claim/evidence binding、event ledger、session refs、
  cleanup summary。

验收以 `.astra/jobs/<job_id>` 和 Pi session JSONL 为准，逐项检查每个 transition
的 authority、evidence refs、checksum、review refs、时间和状态，而不是只检查
最终 Markdown 是否存在。

2026-08-14 的最新 fixture 验收还要求 `audit-astra-run.mjs` 读取父 Pi session
目录，确认至少一个 `astra_research_result` entry 匹配当前 `jobId`，且 action 为
正常完成的 `run` 或中断后完成的 `resume`。恢复型验收还要求同一 job 的父 session
包含 Astra checkpoint 和成功 Pi compaction。这证明控制链经过并持久化在父 Pi
composition root，而不只证明 child session 使用了 Pi。

## 12. 设计决策摘要

1. **Pi 是 runtime 主干，Astra 是领域产品层。**
2. **Pi inner loop 与 Astra outer research loop 分离。**
3. **主 agent 决策，worker 执行，reviewer 独立，supervisor 只调度和校验。**
4. **研究事实进入 `.astra` canonical event store，Pi transcript 是对话事实；两者通过 refs 连接。**
5. **最小化 Pi core fork；通用 durability 才进入 Pi core，研究语义全部在 Astra package。**
6. **TUI/CLI/RPC/skills 使用 Pi 原生表面；Astra 不再维护平行 UI/runtime。**
7. **完成标准是可恢复、可审计的 full auto research，而不是一次成功的模型回答。**

## 13. 当前实现边界

当前分支已经提供：

- `packages/astra` workspace package；
- `.astra/jobs/<job>/job.json` 原子快照和 `events.jsonl` append-only ledger；
- MissionFrame、默认 stage DAG、TaskPacket 幂等派发、supervisor lease；
- candidate evidence -> main-agent decision -> independent review -> adoption ->
  canonical replacement；review failure -> blocking obligation；
- Pi lifecycle extension、研究工具、权限 hook、slash commands 和 Pi `main()`
  composition root；
- fixture worker/reviewer adapter 驱动的 bounded outer-loop 测试；
- 真实 Pi JSON child worker/reviewer/main-agent session、review packet/trace、
  canonical materialization receipt、stage/role skills、job memory、checkpoint
  和 `.pmcli` 只读迁移报告；
- 完整 14-stage fixture auto-research：并行 worker wave、review failure repair、
  explicit adoption/replacement、fresh main-agent sessions、session JSONL 和
  continuous event ledger；父 Pi control session 与 `jobId` 可反向审计；tick
  budget 中断后的新 Pi session 可从 durable snapshot `resume`，并已验证
  compaction 前 Astra checkpoint。
- `collaborative/autonomous/full` 已进入 supervisor policy；stage dispatch/closure
  gate、全局 task/turn/cost budget、Pi usage cost 累计和 gate approval 都写入
  `.astra` event ledger，可跨进程恢复。

当前明确没有宣称完成：

- live provider 的联网研究结果质量或论文正确性。fixture E2E 只关闭 runtime
  contract gate；仍需有效 provider 完成一次有硬预算上限的真实 14-stage 研究。
