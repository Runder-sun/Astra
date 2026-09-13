# Goal Stage Repair Loop Plan

## Objective

把目标自动化闭环从“派发并验收一个成功 worker”升级为“主代理能处理成功、失败和阶段挂接”：

`goal task -> agent output -> acceptance or repair review -> orchestration evidence -> research stage context -> next goal action`

## Non-Goals

- 不新增独立 scheduler。
- 不新增 board 数据库。
- 不新增 agent runtime。
- 不自动执行高风险 publish、pivot、supersede 或 abandon。
- 不把 trellis 变成产品默认架构，trellis 只记录本次实现任务。

## Implementation Order

1. 已梳理 `goals`、`agents`、`reviews`、`research`、`orchestration` 的现有接口。
2. 已在 goal acceptance 中增加失败输出处理分支。
3. 已用 `reviews::open` 为失败或无效输出创建 repair review packet。
4. 成功验收时会完成 `goal_acceptance` 步骤，并在所有 goal steps 完成时接受目标 run。
5. 已将 active research stage ref 写入验收/修复 artifact，保持研究看板可追踪。
6. 已增加 focused tests 和 CLI conformance。

## Acceptance Criteria

- 已满足：full auto 成功输出会被验收，`goal_acceptance` 进入 done。
- 已满足：当所有 goal steps done 时，目标 run 进入 accepted。
- 已满足：full auto 失败输出不会被接受，会打开一次 repair review packet，并把 `goal_acceptance` 标记 blocked。
- 已满足：重复 `goals advance` 不会为同一个 agent output 重复开 review。
- 已满足：high autonomy 仍不会自动验收输出。
- 已满足：目标任务池能从 existing review packet / blocked step 投影修复状态。

## Test Plan

- `cargo test goals::tests:: --lib -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::goals_advance_creates_goal_run_and_advances_one_cycle -- --nocapture`
- `cargo test --test conformance operator_cli_surfaces::goals_advance_full_auto_failed_worker_opens_repair_review -- --nocapture`
- `cargo check --quiet`
- `cargo fmt --check`
- `git diff --check`

## Completion Notes

- 这轮把“主代理验收”从单纯 artifact 记录升级为目标回合状态机：成功完成，失败阻塞并开修复审查。
- 修复审查仍然只是 repair/retry/pivot 的入口，不自动执行高风险方向变化。
- 下一轮如果继续扩大目标，可以做多 agent 任务领取/去重/并发预算，而不是再补普通失败输出入口。
