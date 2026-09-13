# 全自动研究信息通信闭环 PRD

日期：2026-05-27

## 背景

本任务来自长时间全自动研究测试暴露的问题：系统已经有主 agent loop、任务看板、agent team、review、阶段 DAG、证据层和目录治理，但多轮运行后仍然无法稳定通过高质量门。当前问题不应被理解为“缺一个新的自动研究系统”，也不应让 runtime 替主 agent 做研究决策。真实问题是：现有组件之间的信息表达、传递、绑定、消费和回写没有形成可验证的闭环。

当前实现中已经存在多个相关对象：

- `TaskPacket` 和 `AgentStageTaskContract`：代理任务包和阶段任务合同，见 `src/agents/mod.rs`。
- `GoalStageTaskMetadata`：看板任务中的阶段任务元数据，见 `src/goals/mod.rs`。
- `AutonomousResearchAcceptedWorkerEvidenceEntry`：已接收 worker 证据账本条目，见 `src/runtime/mod.rs`。
- `GoalStageTaskSemanticReviewResult`：阶段任务语义评审结果，见 `src/goals/mod.rs`。
- 主 agent continuity packet、stage closure ledger、task pool、board task 工具和 accepted evidence index。

这些组件说明基础设施不是空白，但它们之间没有统一的责任协议。结果是：

1. 主 agent 看到的是文本化摘要和松散 refs，不能稳定看到“当前必须处理的责任对象”。
2. agent team 经常拿不到完整上游证据，只拿到路径、索引或诊断文件。
3. reviewer 的 `repair-needed`、`replacement-needed`、`unreviewable` 等结果不能稳定绑定到目标证据和后续修复责任。
4. accepted evidence 更像追加账本，没有明确的当前活跃证据集、替代关系、废弃关系。
5. 质量门检查的是账本条目状态，不一定检查主 agent 明确确认过的当前研究口径。
6. runtime 曾经倾向于补任务、做 fallback、生成诊断产物或代替主 agent 推进，这会模糊权责。

因此本 PRD 的目标不是再设计一套流程，而是补全现有系统的“信息通信闭环”，让信息在主 agent、runtime、agent team、reviewer、证据层之间完整、结构化、可追踪、可验收地流动。

## 产品目标

完成后，Astra 在全自动研究中应该满足以下能力：

1. 主 agent 永远能看到当前研究状态、阶段目标、当前活跃证据、未解决阻塞、评审结论、需要派发的责任，而不是只看到自然语言总结。
2. 主 agent 仍然是唯一研究决策者：研究计划、任务派发、review 响应、证据采纳、证据替换、阶段转移、回溯、clean up 都必须由主 agent 明确决策。
3. runtime 只负责状态持久化、结构校验、证据挂载、权限和工具边界、provider 恢复、投影和门禁执行，不生成研究任务，不决定研究方向，不替代 review，不做科研 fallback。
4. agent team 只根据主 agent 发布的任务工作。每个 agent 的能力来自共享 agent 基类、role soul、role package、工具策略、输入证据和任务合同，而不是 runtime 给某类 worker 开特殊研究通道。
5. reviewer 必须评审明确的目标证据和明确的验收标准；评审失败必须形成结构化阻塞，回到主 agent 决策，而不是被 runtime 自动改写成下一步任务。
6. 每一条被验收的证据都必须能证明：来自哪个任务、输入了哪些上游证据、输出了哪些产物、被谁评审、主 agent 是否采纳、是否仍然属于当前活跃研究口径。
7. resume 后，无论换不换模型，主 agent 都能通过持久化状态恢复同一项目目录中的研究上下文，并继续处理同一套未闭合责任。

## 非目标

- 不新增第二套 agent loop。
- 不新增独立研究管理者。
- 不新增独立看板数据库。
- 不让 runtime 生成研究任务候选并要求主 agent 选择。
- 不引入硬编码研究主题、硬编码文献搜索词、硬编码 benchmark 方向、硬编码 fallback 产物。
- 不把信息通信问题只写进 prompt。
- 不把某类 worker 做成架构外特殊通道。
- 不把 Trellis 作为 Astra 内部能力；Trellis 只作为本仓库开发过程的任务记录方式。

## 权责边界

### 主 agent

主 agent 负责研究决策和闭环控制：

- 与用户交流并维护最高研究目标。
- 读取阶段定义、研究技能、项目记忆和当前状态。
- 发布看板任务。
- 决定任务依赖、验收目标、输入证据、输出要求、worker 类型、review 要求。
- 验收 worker 输出或要求修复。
- 发起独立 review。
- 根据 review 结果发布修复、替换、补证据、回溯、推进阶段或 clean up 决策。
- 选择当前活跃证据集。
- 决定什么时候进入下一阶段。

主 agent 不能把这些职责下放给 runtime。

### runtime

runtime 负责工程执行和边界保护：

- 保存和加载 session、goal、stage、task、review、evidence、continuity。
- 校验任务包结构、权限、依赖、工具范围、证据 refs 是否可解析。
- 把主 agent 指定的输入证据完整挂载给 agent team。
- 将 worker 输出、review 输出和主 agent 决策写入持久状态。
- 执行质量门的结构性条件。
- 将阻塞、未通过 review、缺证据、provider 故障投影给主 agent。
- 管理 provider 重试、长跑恢复、超时等待和日志。

runtime 不负责：

- 设计研究计划。
- 自动生成科研任务。
- 自动决定 review 失败后怎么修。
- 自动替换研究方向。
- 自动采纳未经主 agent 明确选择的证据。
- 自动把诊断文件当作 canonical research artifact。

### agent team

agent team 负责专业执行：

- 根据任务合同完成调研、方案、实现、实验、分析、写作、review 等任务。
- 使用 role soul、role package 和工具策略定义角色能力。
- 消费主 agent 指定的输入证据。
- 输出结构化证据、产物清单、引用、风险、缺口和可复现信息。
- reviewer agent 根据明确目标证据和验收标准做严格评审。

agent team 不负责阶段推进，也不负责全局研究口径选择。

## 当前通信链路

当前一次全自动研究大致经过以下链路：

1. 主 agent 读取 continuity packet 和阶段状态。
2. 主 agent 调用 board task 工具发布阶段任务。
3. runtime 将看板任务转换成 agent task packet。
4. runtime 根据 `input_artifact_refs` 挂载可解析输入文件。
5. worker 在隔离 worktree 中执行并输出 evidence、manifest、candidate artifacts。
6. runtime 收集 worker 产物并写入 accepted worker evidence index。
7. 主 agent 或 reviewer 对产物做验收或评审。
8. runtime 尝试绑定 review 到目标 worker evidence。
9. stage gate 检查证据和评审是否满足条件。

当前链路的主要断点是：

- `AgentStageTaskContract` 只有任务目标、输入 refs、输出要求、验收检查和依赖，缺少明确的 blocker refs、review target ids、review target evidence refs、replacement/supersede/current evidence set 语义。
- `accepted_worker_evidence_task:<task_id>` 能表达语义引用，但现有挂载路径只解析文件路径、`accepted_worker_evidence_index:` 和 `readiness_ref:`，不能自动展开为完整证据包。
- semantic review 的 verdict 是字符串，解析和门禁主要围绕 pass/fail，不能稳定表达和绑定 `repair_needed`、`replacement_needed`、`unreviewable_missing_evidence`。
- accepted evidence index 记录条目，但缺少 `active`、`superseded`、`replaced_by`、`current_evidence_set_id` 这种研究口径状态。
- 主 agent continuity packet 中已经有 stage closure ledger 和 task pool，但缺少强制的“未解决责任对象”和“证据消费证明”。
- worker 可以在没有完整上游证据的情况下继续猜测，导致输出看起来完成，实际没有闭环。

## 目标通信协议

本任务需要在现有对象上补一层统一通信协议。这里的“协议”不是新系统，而是约束现有对象之间如何表达同一件事。

统一协议本身不能保证信息完备。每一类关键研究信息都必须同时通过五类证明：

| 证明类型 | 必须回答的问题 | 失败时的系统行为 |
| --- | --- | --- |
| 表达证明 | 当前状态对象能不能表达这件事？ | 补 schema 或状态对象，不允许只写自然语言 |
| 传递证明 | 目标 agent 是否实际拿到了这件事？ | 拒绝派发或生成 dispatch blocker |
| 消费证明 | 目标 agent 是否证明自己用过这件事？ | 不能采纳为高质量 evidence |
| 绑定证明 | review、blocker、repair 是否绑定到同一目标证据？ | 不允许 gate 通过，不允许自动猜测目标 |
| 回写证明 | 主 agent 的处理决策是否写回同一状态链路？ | 责任保持未闭合，继续投影给主 agent |

后续实现必须为这五类证明写测试。缺任何一类，都只能算“字段存在”，不能算信息通信闭环完成。

### 表达完整

任何会影响研究质量门的状态，都必须可以被结构化表达：

- 当前阶段目标。
- 当前阶段允许进入的下一阶段。
- 当前活跃证据集。
- 每个活跃证据的来源任务、上游输入、输出产物、review 状态和主 agent 决策。
- 每个 blocker 的目标证据、失败标准、失败原因、建议操作、是否需要 clean up。
- 每个 repair/replacement 任务对应哪个 blocker、哪个 review、哪个目标证据。
- 每个 review 的目标任务和目标证据。
- 每个被替换证据的 supersede 关系。

### 传递完整

凡是任务合同中写入的输入证据，runtime 必须做到以下之一：

1. 完整挂载给 worker/reviewer。
2. 明确拒绝派发任务，并把不可解析 refs 作为阻塞反馈给主 agent。

允许挂载的不是只有文件路径，还必须支持语义引用展开，例如：

- `accepted_worker_evidence_task:<task_id>` 展开为该任务的证据包。
- `accepted_worker_evidence_index:<path>` 展开为索引和相关条目。
- `review_packet:<id>` 展开为 review packet、latest trace 和 verdict。
- `stage_closure_ledger:<id>` 展开为阶段账本和当前缺口。

完整证据包至少包括：

- 原任务合同。
- 输出 manifest。
- worker evidence 正文。
- candidate artifact refs。
- 已绑定 review refs。
- 主 agent 采纳或拒绝决策。
- 上游输入摘要和 refs。

### 消费可证明

worker 和 reviewer 不能只“拿到”输入，还必须证明自己使用了输入：

- 输出 manifest 必须列出 consumed input refs。
- 对上游证据做判断时，必须引用具体 evidence ref 或 artifact ref。
- reviewer 必须声明 review target task ids 和 review target evidence refs。
- 如果输入缺失，worker/reviewer 必须输出 `unreviewable_missing_evidence` 或等价结构化阻塞，不能猜测。

### 评审可绑定

review 结果必须绑定到目标证据，而不是只作为自由文本报告存在。

verdict 至少需要结构化为：

- `pass`：目标证据通过指定高标准。
- `repair_needed`：目标证据方向仍可保留，但需要修复。
- `replacement_needed`：目标证据不可作为当前口径，需要替换。
- `fail`：目标证据不通过，且没有足够信息建议修复方式。
- `unreviewable_missing_evidence`：reviewer 没有拿到足够输入，不能评审。

非 pass verdict 必须形成 blocker，并回到主 agent 决定下一步。runtime 只能记录和投影 blocker，不能自动生成修复任务。

### 决策可回写

主 agent 的关键研究决策必须通过工具或结构化记录回写：

- 采纳某个 worker artifact。
- 拒绝某个 worker artifact。
- 将某个 candidate artifact 提升为 canonical artifact。
- 将某个证据设为 active。
- supersede 某个旧证据。
- 对 review 失败发布 repair/replacement 任务。
- 请求阶段 review rerun。
- 请求阶段 DAG route change。
- 触发 clean up。

自然语言说明可以存在，但不能替代结构化决策。

## 高质量门关系

统一通信协议不能保证科研质量自动变高。它只能保证必要信息完整传递。要让质量门真正有效，还必须同时满足：

1. 高标准由主 agent 组织生成，并可由标准设定类 agent 或专家 reviewer 辅助提出。
2. 验收标准不是简单数量指标，而是科研专家级的阶段完成标准。
3. reviewer 按这些标准评审明确证据。
4. review 失败被绑定为 blocker。
5. 主 agent 必须把 blocker 转成新的研究任务、替换任务、回溯或 clean up 决策。
6. stage gate 只检查当前活跃证据集和已解决 blocker。

因此，本任务解决“信息能否完备传递和闭环回写”。它为高质量科研判断提供必要底座，但不把科研判断硬编码进 runtime。

## resume 关系

项目级持久记忆和 resume 必须走同一套通信协议。

理想状态下，用户在同一个项目目录重新打开 Astra 后：

- runtime 加载同一个项目根、session、goal run、stage execution、task pool、accepted evidence、review packet、closure ledger。
- 主 agent 拿到 continuity packet，其中包含当前活跃证据集、未解决 blocker、待 review 任务、待派发责任、最近主 agent 决策和允许阶段转移。
- 主 agent 不需要依赖上一轮模型上下文，也能继续同一个研究闭环。

如果用户更换 provider 或模型，恢复上下文仍然来自持久化状态，而不是依赖模型隐式记忆。

## 用户故事

### 全自动长跑

用户在新目录输入一个研究目标并选择全自动。主 agent 读取技能和阶段定义，发布调研任务。调研 worker 输出文献证据。汇总 worker 消费这些证据产出方案。reviewer 对方案进行严格评审。若 review 失败，主 agent 看到结构化 blocker 并发布新的补调研或替换方案任务。整个过程不需要 runtime 生成科研任务。

### reviewer 缺证据

reviewer 收到任务，但 runtime 无法解析 `accepted_worker_evidence_task:<task_id>`。任务不应被派发；runtime 应把缺失输入作为阻塞反馈给主 agent。若任务已经运行且 reviewer 声明缺证据，结果必须被记录为 `unreviewable_missing_evidence`，不能被当作 pass 或普通 failure。

### 证据替换

某个方案通过主 agent 初步采纳，但独立 review 后要求 replacement。主 agent 发布 replacement 任务。新方案通过后，主 agent 明确 supersede 旧方案。stage gate 只检查新 active evidence set，不再让旧方案的失败状态污染当前研究口径。

### 回溯和 clean up

当主 agent 决定换方向或回溯阶段时，必须发出 route change 和 cleanup decision。runtime 执行目录治理，只保留当前口径下的文档、代码、实验和论文产物。旧口径通过 git 和记录可追踪，但不混在当前目录中。

## 验收标准

本任务完成后，至少满足以下验收：

1. `accepted_worker_evidence_task:<task_id>` 能展开为完整 evidence bundle，并挂载给 worker/reviewer。
2. 如果任务声明了无法解析的关键输入证据，runtime 拒绝派发并把阻塞投影给主 agent。
3. review task 必须有明确 target task ids 或 target evidence refs；缺失目标时不能进入正常执行。
4. review verdict 支持 `pass`、`repair_needed`、`replacement_needed`、`fail`、`unreviewable_missing_evidence`。
5. 非 pass review 会绑定到目标证据，并形成主 agent 可见 blocker。
6. runtime 不会把 blocker 自动改写成科研修复任务；只有主 agent 明确发布后，agent team 才执行。
7. accepted evidence index 能表达 active、superseded、replaced_by、current evidence set。
8. stage gate 只基于当前活跃证据集和已解决 blocker 判断阶段是否通过。
9. worker/reviewer 输出必须包含 consumed input refs；缺失时不能被采纳为高质量证据。
10. resume continuity packet 包含当前活跃证据、未解决 blocker、待 review、待派发责任和最近主 agent 决策。
11. 长跑测试中，系统不能再把 runtime 诊断文件当作 canonical literature artifact。
12. 长跑测试中，review 失败必须转化为主 agent 的明确下一步决策，而不是反复堆积同类低质量任务。

其中第 1、2、5、7、9、10 条必须有自动化测试或可重复本地验证命令。长跑观察只能作为补充证据，不能替代工程验收。

## 测试要求

需要新增或扩展测试覆盖：

- 语义 evidence ref 展开和挂载。
- 缺输入 fail closed。
- review target 强制校验。
- semantic review verdict 结构化解析。
- review 结果绑定目标证据。
- blocker 投影给主 agent continuity packet。
- 主 agent supersede 决策更新 active evidence set。
- stage gate 忽略 superseded evidence，只检查 active set。
- worker/reviewer consumed input refs 校验。
- autonomous protocol 下 runtime 不生成研究任务。
- resume 后 continuity packet 能恢复未闭合责任。

## 成功信号

短期成功信号：

- 相关单元测试和 conformance 测试通过。
- 在空目录自动研究测试中，worker/reviewer 的输入挂载能看到完整证据包。
- review 失败后，主 agent 能看到明确 blocker，并通过工具发布下一步任务。

中期成功信号：

- 长跑不再因为信息丢失反复派发泛化任务。
- 文献、方案、实现、实验、写作阶段都能形成 active evidence set。
- 研究方向改变时能稳定触发 route change 和 clean up。

长期成功信号：

- 同一目录中断后 resume，主 agent 可以无缝继续同一个研究闭环。
- 全自动研究可以连续运行数小时到数天，直到产出 tex/pdf 论文，并通过严格 review。
