# Goal Task Claim Budget Loop Plan

## Objective

把目标任务池派发升级成可追踪的 claim 生命周期：

`task pool entry -> claim -> agent task -> output -> acceptance/repair -> claim closed or blocked`

## Non-Goals

- 不新增独立 scheduler。
- 不新增 kanban 数据库。
- 不新增 agent runtime。
- 不做多机分布式锁。
- 不自动执行高风险 pivot / publish / supersede。

## Implementation Order

1. 阅读现有 `goals` 派发、验收、repair review 实现。
2. 定义 claim artifact 格式和 active claim 判断。
3. 为 dispatch result 增加 claim 信息。
4. 派发前检查同源 active claim 和 goal run 并发预算。
5. 验收成功关闭 claim；失败修复保留 blocked claim。
6. 增加 focused unit tests 和 operator CLI tests。

## Acceptance Criteria

- 同一 task pool entry 已有 active claim 时，不会重复派发。
- goal run 已有 active worker claim 时，不会派发第二个并发 claim。
- dispatch result 暴露 claim id / entry id / budget decision。
- 成功验收会关闭对应 claim，目标 run 可继续完成。
- 失败修复会保留 blocked claim，并继续通过 repair review 入口处理。
- high autonomy 仍不自动验收。

## Test Plan

- `cargo test goals::tests:: --lib -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::goals_advance_goal_task_claim_blocks_duplicate_dispatch_until_acceptance -- --nocapture`
- `cargo check --quiet`
- `cargo fmt --check`
- `git diff --check`

## Completion Notes

- Implemented the claim lifecycle on top of existing orchestration artifacts.
- Reused the existing goal task pool projection and research board source; active claims now appear as `goal_task_claim` task pool entries.
- Preserved the existing main-agent loop and agent runtime contracts.
- Kept high autonomy from auto-accepting worker output.
- Kept failed worker output behind the existing repair review gate while retaining the blocked claim as active.
