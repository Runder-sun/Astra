# Goal Worker Acceptance Loop Plan

## Objective

把目标任务派发闭环从 mock 写链升级为最小真实执行与验收闭环：

`GoalTaskPoolEntry -> TaskPacket -> local agent run -> AgentOutputManifest -> main-agent acceptance -> orchestration evidence`

## Non-Goals

- 不新增 scheduler。
- 不新增 board 数据库。
- 不新增第二套 worker runtime。
- 不自动推进研究阶段。
- 不自动接受失败任务、外部发布、方向 pivot 或需要评审的任务。

## Implementation Order

1. 已阅读 `agents::start_local` 约束和 `reviews`/`orchestration` 现有结构。
2. 已为 goal dispatch 增加 runner kind，并在 full auto 下调用 local runner。
3. 已增加 goal acceptance result 和选择 helper。
4. 已在 `goals advance` 中，当 `goal_acceptance` 运行且策略允许时验收成功 agent 输出。
5. 已增加 canonical event、reducer 已知事件和 focused tests。

## Acceptance Criteria

- `goals advance --json` 在 full auto 下能派发 local agent worker。
- 第二次 `goals advance --json` 能验收该 worker 的有效 output manifest。
- 高自治不会自动验收普通任务。
- 验收结果能在 orchestration run artifact / continuation 中追踪。
- `goal_task_acceptance` event 可重放且不会破坏 reducer。

## Completion Notes

- Full auto 的派发现在走 `agents::start_local`，由现有 git worktree isolation 和 manifest validation 提供执行边界。
- High autonomy 仍派发 mock proof packet，不自动验收，保留人工参与边界。
- 验收只接受 `succeeded` + `complete` + `valid` 的 agent 输出。
- 验收结果写回 `goal_acceptance` 编排步骤，并记录 `goal_task_acceptance` event。

## Deferred Work

- 多 agent task pool 领取/去重/并发控制。
- 失败输出的 repair / retry / review 流程。
- 从验收结果自动推进 research stage 或 finish goal。
