# Pi-native Astra Research Kernel v2

日期：2026-08-17

状态：已实现并进入验证。本文替代把固定 14-stage DAG 当作研究主状态的设计；Pi runtime
归属和 Rust legacy 边界保持不变。

## 1. 问题

当前实现已经证明 Pi 可以承载 Astra 的 provider、tool loop、session、CLI、TUI、
RPC 和 child-agent runtime，但它把自动研究收缩成了不可逆的 stage pipeline：

- main-agent 每次决策使用新 session，没有持续的研究主体；
- stage 是主状态，问题、假设、反例、争议和候选路线不是一等对象；
- reviewer 主要检查 evidence snapshot，不能独立复现 worker 结果；
- stage 之间通过摘要和相邻 artifact 传递信息；
- repair 只在当前 stage 重试，不能根据新证据回退研究路线；
- bounded worker wave 没有 candidate search、比较、淘汰和 winner promotion；
- collaborative mode 是批准/恢复控制，不是人与 main-agent 共同研究。

这使系统能够完成 workflow contract，但不能保证形成高质量研究闭环。

## 2. 核心不变量

### 2.1 持续 Main Agent

一个 research job 只有一个持久 main-agent Pi session/ref。main-agent 持续维护问题
理解、未决问题、候选假设、证据冲突和路线选择。supervisor 只执行预算、lease、
权限、幂等和状态转换，不能替代 main-agent 作研究判断。

### 2.2 Canonical Research Graph

研究真相源是 graph，不是 stage 列表。graph 的一等对象至少包括：

- question、hypothesis、claim、evidence、objection、decision、artifact；
- supports、contradicts、tests、derives、refines、resolves、supersedes 边；
- open question、active hypothesis、unresolved objection 和 accepted claim 索引。

Stage 是 graph 上的执行窗口。Stage projection 可以压缩，但不能替代或覆盖 graph
中的原始节点、artifact 和 provenance。

### 2.3 无损 Artifact Bus

TaskPacket 只携带引用和 scope；每个引用必须能在隔离 workspace 中解析为：

- immutable canonical content；
- worker 产生的原始文件和 checksum；
- source receipt；
- 产生它的 task/session/review lineage。

上下文压缩只能生成 derived view。任何 main-agent、worker 或 reviewer 都可以在权限
范围内按 ref 读取原始对象，不能依赖前一 stage 重新讲述全部历史。

### 2.4 Stage Research Loop

每个 stage 执行同一个研究协议：

```text
main frame + quality rubric
  -> search policy creates candidate branches
  -> subagents execute in isolated workspaces
  -> deterministic verification + independent review
  -> compare / debate / select
  -> main integrate, continue search, backtrack, ask user, or close
```

Stage closure 不是默认动作。只有 quality gate 满足，且 main-agent 提交显式 route decision
时才能切换能力或完成研究。Review objection 可以重开当前或任意上游 stage，并使依赖它的下游
canonical artifacts 失效但保持可审计。

### 2.5 Agentic Search

`SearchBatch`、`SearchCandidate`、`CandidateEvaluation` 和 `SearchDecision` 是 durable
对象。Search policy 定义候选数量、diversity 目标、evaluation criteria、预算和停止
条件。候选可以表示研究问题、方法、实现、实验设计或论文叙事，不限于代码 branch。

默认搜索最多两轮。第一轮只有在 main-agent 给出具体未决判别条件时才能继续；旧 batch
标记为 exhausted，候选、证据、逐 criterion review 和决策理由全部保留，hypothesis 标记为
superseded。下一轮必须看到这些评估并提出正交判别方案，不得重复旧 hypothesis。最终轮禁止
继续搜索，必须按冻结 criterion 顺序、总分和稳定 candidate id 确定性选择通过门槛的 winner。

没有 evaluation 和 main-agent selection 的候选不能进入 canonical surface。单纯重试
失败任务不算 search。

### 2.6 Independent Verification

独立 session 只是隔离条件，不是 review 完成条件。严格 review 同时要求：

- reviewer 看到 immutable evidence bundle 和可解析原始 refs；
- deterministic checks 先于 LLM judgment；
- stage rubric 在 worker 执行前冻结；
- reviewer verdict 记录 criterion-level evidence；
- 高风险 stage 支持多 reviewer、复现和 pairwise comparison；
- reviewer objection 进入 research graph，而不是只留在 review prose。

### 2.7 Human Research Workbench

用户和 main-agent 共享同一个 Research Board。Board 投影 questions、hypotheses、
claims、evidence、objections、search candidates、budget 和 next decision。用户可以：

- 回答 open question、补充边界和证据；
- 修改或否决候选路线；
- 要求继续搜索、比较或回退；
- 在 pivot、预算、最终 claim 和外部发布前作决定。

`collaborative` 不再等于每步确认；它表示 main-agent 在关键研究不确定性上主动向用户
请求输入，并保留完整 decision lineage。

## 3. Pi 与 Astra 归属

| 能力 | Owner |
| --- | --- |
| provider、model、tool loop、session、compaction、CLI/TUI/RPC | Pi |
| persistent main-agent ref 和 child session adapter | Astra on Pi |
| research graph、stage loop、search、review、route、canonicality | Astra |
| research board projection 和协同命令 | Astra extension |
| process lease、budget、idempotency、artifact materialization | Astra supervisor/store |

Pi session fork 不是 research search branch；Pi RPC 不是 research workbench。Astra 必须
在 Pi primitives 上保留这些领域语义。

## 4. Rust 核心迁移矩阵

| Rust 核心 | v2 目标 | 优先级 |
| --- | --- | --- |
| `ResearchThread` / `DeliberationSpan` | research graph + persistent main-agent | P0 |
| research board | Pi TUI/RPC board projection | P0 |
| `SearchBatchRecord` / `BranchRunRecord` | search batch/candidate lineage | P0 |
| `EvaluationPacket` / verifier tournament | candidate evaluation + comparison | P0 |
| route change / stage execution map | dynamic backtrack/reopen | P0 |
| role packages / stage standard setter | stage rubric + role-bound Pi skills | P0 |
| project/worktree lineage | candidate workspace lineage | P1 |
| project/research memory | graph-backed durable memory | P1 |
| trajectory/evolution/feedback | reviewed outer-loop optimization | P2 |
| remote/mobile | board/control projection over Pi RPC | P2 |
| old provider/TUI/MCP/plugin/doctor runtime | Pi owner; migrate behavior only when Pi lacks it | no Rust runtime port |

## 5. 完成标准

Research Kernel v2 只有在以下行为由测试和 live run 同时证明后才完成：

1. 同一 job 的 main-agent round 使用同一 Pi session，compaction 后保持 graph identity；
2. 至少一个 stage 运行多候选 search，独立评估后只提升一个 winner；
3. reviewer 能读取并校验原始 artifact，而不是只审摘要；
4. review objection 能回退上游 stage，并退休所有依赖旧结论的下游 artifact；
5. 用户可以从 board 查看并影响 question、candidate、objection 和 route；
6. full run 不以 stage count 为成功条件，而以 graph 中没有 blocking objection、
   quality gate 满足和 main-agent 最终决策为条件；
7. audit 分别报告 runtime integrity、research quality 和 unresolved uncertainty。
