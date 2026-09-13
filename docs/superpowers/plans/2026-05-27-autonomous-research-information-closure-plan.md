# 全自动研究信息通信闭环实施计划

日期：2026-05-27

## 目标

把当前全自动研究中的信息传递从“文本摘要 + 松散 refs + 追加账本”升级为“主 agent 可决策、agent team 可消费、reviewer 可绑定、runtime 可校验、质量门可验收”的闭环。

本计划实现 `docs/superpowers/specs/2026-05-27-autonomous-research-information-closure-prd.md`，不新增第二套 agent loop，不让 runtime 越权做科研决策。

## 当前代码入口

- `src/agents/mod.rs`
  - `TaskPacket`
  - `AgentStageTaskContract`
  - worker input artifact mounting
- `src/goals/mod.rs`
  - `GoalStageTaskMetadata`
  - task pool projection
  - semantic review policy/result
  - accepted evidence refs from task pool
- `src/runtime/mod.rs`
  - autonomous research continuity packet
  - stage closure ledger
  - accepted worker evidence index
  - semantic review binding
  - stage gate and canonical artifact adoption
- `src/tools/mod.rs`
  - board task publication/update
  - main agent artifact decision
  - route change, review rerun and cleanup control tools
- `schemas/task_packet.schema.json`
  - task packet schema conformance

## 实施原则

1. 先补结构，再补 prompt。不能只靠提示词要求 agent 自觉传递信息。
2. runtime 只做校验、挂载、记录、投影和门禁，不生成研究任务。
3. 主 agent 必须通过结构化工具表达关键研究决策。
4. agent team 的增强必须通过 role soul、role package、tool policy 和输入证据装配完成，不能开架构外特殊通道。
5. review 失败不是普通文本失败，而是绑定到目标证据的 blocker。
6. stage gate 只看当前活跃证据集，不让旧口径证据污染当前判断。
7. 每一步都用测试证明，不用长跑结果倒推实现是否正确。

## 阶段 0：红线测试和现状锁定

### 修改范围

- 新增或扩展 `tests/conformance` 和 `src/*` 单元测试。
- 不改业务逻辑，先写失败测试。

### 任务

1. 测试 `accepted_worker_evidence_task:<task_id>` 当前不能展开完整证据包。
2. 测试 review task 缺少 target task/evidence 时应被拒绝。
3. 测试 `repair_needed` review 结果必须绑定目标证据并投影为 blocker。
4. 测试 superseded evidence 不应进入 active stage gate。
5. 测试 consumed input refs 缺失时不能采纳为高质量证据。
6. 测试 autonomous protocol 下 runtime 不创建科研 follow-up task，只投影 blocker 给主 agent。

### 验收

- 至少 4 个目标测试在实现前失败，失败原因与 PRD 缺口一致。

## 阶段 1：扩展任务合同和看板元数据

### 修改范围

- `src/agents/mod.rs`
- `src/goals/mod.rs`
- `schemas/task_packet.schema.json`
- board task 工具参数校验处

### 任务

在 `AgentStageTaskContract` 和 `GoalStageTaskMetadata` 增加可选结构字段：

- `blocker_refs`
- `review_target_task_ids`
- `review_target_evidence_refs`
- `supersedes_task_ids`
- `replacement_of_task_ids`
- `current_evidence_set_id`
- `consumed_input_required`

更新 board task 发布和更新校验：

- review 类任务必须声明 review target。
- repair/replacement 类任务必须声明 blocker 或 replacement target。
- 消费上游证据的任务必须有可解析输入 refs 或明确依赖。

### 验收

- schema 和 Rust 结构一致。
- 老任务包可反序列化，新字段默认空。
- 缺 review target 的 review task 被拒绝。
- repair/replacement 任务没有目标时被拒绝。

## 阶段 2：实现完整 evidence bundle 挂载

### 修改范围

- `src/agents/mod.rs`
- 可能新增小模块或 helper，用于 evidence bundle 解析。

### 任务

扩展 `resolve_mountable_input_artifact` 的能力，但不要把复杂逻辑塞成路径解析 hack。建议拆成两层：

1. `resolve_input_artifact_ref`
   - 判断 ref 类型。
   - 返回一个或多个 source 文件。
2. `mount_resolved_input_artifact_bundle`
   - 为语义 ref 创建 bundle 目录。
   - 写入 bundle manifest。
   - 拷贝相关证据文件。

支持：

- `accepted_worker_evidence_task:<task_id>`
- `accepted_worker_evidence_index:<path>`
- `review_packet:<id>`
- `review_trace:<path>`
- `stage_closure_ledger:<id or path>`

如果关键输入 refs 无法解析：

- 不派发任务。
- 写入结构化 dispatch blocker。
- 投影给主 agent。

### 验收

- worker worktree 中出现 `.pmcli/worker-input-artifacts.json`。
- 对语义 ref，manifest 中能看到 bundle 目录和 bundle manifest。
- review worker 能读取目标任务合同、worker evidence、review trace 和主 agent decision。
- 不再出现 reviewer 只能看到 continuity JSON 却看不到目标 artifact 的情况。

## 阶段 3：结构化 semantic review verdict 和 blocker

### 修改范围

- `src/goals/mod.rs`
- `src/runtime/mod.rs`
- review result parsing and binding tests

### 任务

将 semantic review verdict 标准化为受控集合：

- `pass`
- `repair_needed`
- `replacement_needed`
- `fail`
- `unreviewable_missing_evidence`

保留兼容解析，把历史文本 verdict 归一化到新集合。

绑定逻辑改为：

- 所有 verdict 都可以绑定到目标证据。
- 只有 `pass` 清除 review floor。
- 其他 verdict 生成 blocker。
- `unreviewable_missing_evidence` 生成输入挂载或任务合同 blocker。
- blocker 必须包含 target task ids、target evidence refs、failed criterion、findings、suggested operation、cleanup_required。

### 验收

- `repair-needed`、`repair_needed`、`needs repair` 都能归一化。
- 非 pass review 不会丢失。
- blocker 出现在主 agent continuity packet。
- stage gate 因 blocker 保持不通过，但能给出明确下一步责任。

## 阶段 4：active evidence set 和 supersede 关系

### 修改范围

- `src/runtime/mod.rs`
- main agent artifact decision tool handling
- stage gate logic

### 任务

扩展 `AutonomousResearchAcceptedWorkerEvidenceEntry`：

- `active_status`: `candidate | active | superseded | rejected`
- `current_evidence_set_id`
- `superseded_by_task_id`
- `replacement_of_task_ids`
- `decision_reason`

主 agent 通过现有或扩展工具明确改变状态：

- 采纳为 active。
- 拒绝为 rejected。
- 用新任务 supersede 旧任务。
- 将多个 active evidence 组成 current evidence set。

stage gate 改为：

- 只检查 active evidence set。
- superseded/rejected 不参与当前质量门。
- 但历史记录仍保留，用于 DAG 和 clean up 追踪。

### 验收

- 新证据 supersede 旧证据后，旧证据不再阻塞当前 stage gate。
- 没有主 agent 决策时，runtime 不自动把 candidate 设为 active。
- active evidence set 可在 continuity packet 中直接展示。

## 阶段 5：主 agent continuity packet 升级

### 修改范围

- `src/runtime/mod.rs`
- main agent prompt/continuity rendering

### 任务

continuity packet 必须结构化包含：

- 当前阶段。
- 当前允许进入的下一阶段。
- 当前 active evidence set。
- unresolved blockers。
- review results by target evidence。
- waiting tasks。
- needs review tasks。
- ready tasks。
- recently failed dispatch refs。
- last main agent decisions。
- required main agent obligations。

主 agent prompt 必须明确：

- runtime 只投影 blocker，不生成科研任务。
- 主 agent 必须把 blocker 转换为 board task、route change、cleanup 或 review rerun decision。
- 主 agent 不能忽略 unresolved blockers。
- 进入下一阶段前必须说明 active evidence set 如何满足 closure ledger。

### 验收

- continuity packet 中能直接定位每个未解决 blocker 的目标证据。
- resume 后主 agent 看到同一套未闭合责任。
- 主 agent 可以在不依赖旧模型上下文的情况下继续派发修复任务。

## 阶段 6：worker/reviewer 消费证明

### 修改范围

- `src/agents/mod.rs`
- output manifest validation
- accepted worker evidence ingestion

### 任务

要求 worker/reviewer 输出 manifest 包含：

- `consumed_input_refs`
- `consumed_input_artifact_paths`
- `missing_input_refs`
- `review_target_task_ids`
- `review_target_evidence_refs`

采纳逻辑：

- 若 `consumed_input_required=true` 且 consumed refs 为空，则不能采纳为 active。
- reviewer 若缺目标 evidence refs，则结果为 `unreviewable_missing_evidence` 或直接阻塞。

### 验收

- 没有消费证明的上游依赖任务不能被高质量采纳。
- review 报告必须能追到目标证据。

## 阶段 7：回归和真实长跑验证

### 编译和测试

建议执行：

```bash
cargo fmt --check
cargo test -q --lib
cargo test -q --test conformance
cargo build --release
```

如全量测试过慢，先执行相关目标测试，再跑 release build。

### 真实测试

新建空目录，使用真实二进制和配置 provider 运行全自动研究主题：

```text
做一个 llm 在超级混乱上下文情况下的最优决策能力的测评 benchmark。
```

重点观察：

- 主 agent 初始派发任务是否专业。
- worker 是否拿到完整输入 evidence bundle。
- literature artifact 是否来自 agent team，而非 runtime 诊断文件。
- reviewer 是否绑定明确目标证据。
- repair/replacement 是否回到主 agent 决策。
- stage gate 是否只看 active evidence set。
- resume 后是否无缝继续。

### 长跑验收

短跑通过不等于完成。至少需要一次持续运行，观察到：

- 出现 review 失败。
- 主 agent 根据失败发布明确修复或替换任务。
- 新任务消费旧证据。
- 新 evidence supersede 或修复旧 evidence。
- stage gate 状态发生合理变化。

## 阶段 8：实现后防偏审计

### 目标

防止实现过程中把通信闭环问题修成新的硬编码、fallback 或 runtime 越权。

### 审计项

1. 全仓扫描 runtime 是否新增研究主题词、benchmark 主题词、文献搜索词或固定科研方案。
2. 全仓扫描 runtime 是否新增自动 board task 生成逻辑。
3. 检查 provider failure 处理是否只做恢复和投影，不生成科研替代产物。
4. 检查 literature、method、experiment、paper worker 的增强是否来自 role soul、role package、tool policy 或输入 evidence bundle。
5. 检查 review 失败是否只形成 blocker，后续任务必须来自主 agent board task。
6. 检查 canonical artifact 写入是否有主 agent 决策 ref。
7. 检查 active evidence set 是否可从 resume continuity packet 恢复。

### 验收

- 审计结论写入对应 review 文档或提交说明。
- 若发现 runtime 越权，必须先修复再进行长跑。
- 若发现硬编码研究主题，必须删除或迁移到角色 skill/tool/prompt 配置中。

## 预期提交边界

建议拆成 3 个提交：

1. `Add autonomous research information closure PRD`
   - 只包含 PRD、计划和自审文档。
2. `Add evidence communication contract tests`
   - 失败测试或最小实现测试。
3. `Implement autonomous research information closure`
   - 结构字段、挂载、review binding、active evidence set、continuity packet、验收。

如果实现规模过大，可以进一步拆分为：

- task contract and schema
- evidence bundle mounting
- review blocker binding
- active evidence gate
- continuity and resume projection
