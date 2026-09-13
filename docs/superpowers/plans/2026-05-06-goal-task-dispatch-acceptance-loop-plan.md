# Goal Task Dispatch Acceptance Loop Plan

## Objective

把已经存在的目标任务池变成第一条真实执行写路径：
目标任务池中的可执行条目能够被主代理派发为现有 agent task packet，并能在现有 agent / review / orchestration / board 投影里被追踪。

## Non-Goals

- 不新增独立 scheduler。
- 不新增 board 数据库。
- 不新增第二套权限模型。
- 不实现完整并发领取竞态。
- 不让移动端成为运行权威。

## Implementation Order

1. 已阅读现有 `src/agents` 启动路径和 task packet 数据结构。
2. 已设计最小 goal dispatch adapter，复用现有 mock agent 创建逻辑。
3. 已在 `goals advance` 后半段接入派发逻辑，只在策略允许且 `goal_dispatch` 步骤运行时派发。
4. 已将派发结果回写 canonical event / checkpoint / 编排步骤，并确保投影能看见。
5. 已补测试并更新 trellis 记录。

## Acceptance Criteria

- `goals advance --json` 在高自治或全自动下可以派发一条低风险任务。
- 人机协同模式不会自动派发，只会建议或等待确认。
- 派发出的任务能被 `agents list` 看到。
- host research board 能把派发后的 agent task 显示出来。
- 所有改动复用现有状态源。

## Completion Notes

- 完成 `GoalTaskDispatchResult` 和目标任务选择器。
- 完成 `GoalTaskPoolEntry -> agents::start_mock -> TaskPacket/AgentRuntimeRecord/AgentOutputManifest/AgentTrace` 写链。
- 完成 `goal_dispatch` 编排步骤证据回写。
- 完成 `goal_task_dispatch` canonical event 和 reducer 已知事件登记。
- 完成 focused 单测和操作闭环测试。

## Deferred Work

- 将 mock 派发升级为真实 agent worker 领取/执行。
- 设计并实现验收入口，把 agent 输出映射回编排步骤、研究状态、评审包和阶段推进。
- 增加多 agent 并发领取、去重、重试和退回策略。
