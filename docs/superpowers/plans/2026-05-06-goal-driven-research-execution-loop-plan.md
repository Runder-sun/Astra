# Goal-Driven Research Execution Loop Plan

> 这一轮不是再做一个新看板，而是把已经存在的任务池变成主代理每轮都能消费的上下文，并在合适的自动化等级下让目标运行自动推进。

## Objective

把上一轮的投影型控制平面升级成执行型控制平面：
主代理在普通对话中持续看到目标任务池，并在高自治/全自动模式下按回合推进、派发、验收和继续。

## Slice 1: Context Fan-In

- 在 `src/session/context.rs` 和 `src/session/context_pack.rs` 里增加目标任务池摘要。
- 目标任务池摘要应来自现有 `goals::status`，而不是新状态源。
- 动态上下文里要明确显示当前自动化等级、任务池统计和下一步建议。

## Slice 2: Turn-End Auto Advance

- 在 prompt / continue 回合成功结束后，按自动化等级触发一次轻量 `goals advance`。
- `human_in_the_loop` 只投影，不自动前进。
- `high_autonomy` 和 `full_auto` 自动前进一格，但仍保留现有预算、权限和评审门。

## Slice 3: Visibility And Tests

- 给上下文构建和自动推进钩子补单测。
- 确保推进后的状态会刷新到主机投影和后续回合上下文。
- 不引入新调度器、新 board DB 或新权限模型。

