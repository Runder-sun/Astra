# Continuous Research Automation Loop Plan

## Objective

把目标驱动研究自动化从“可手动推进、可局部恢复”推进到“能沿同一条控制链持续运行”的阶段：

`goal advance -> task pool -> routine ingress/run-due -> agent execution -> acceptance/recovery -> continue`

## Constraints

- 复用现有 goals、routines、agents、ProjectOps、host_surface、remote 和 mobile。
- 不引入新的 board 数据库。
- 不引入新的调度器内核。
- 不把移动端或远程端变成新的状态权威。

## Implementation Order

1. 已完成：设计统一的持续推进入口 `goals tick`。
2. 已完成：将移动端和远程端动作接到同一条控制链上。
3. 已完成：为持续推进和 remote action 链路补 conformance 测试。
4. 已完成：补出本地轮询入口 `goals watch`，复用同一条 tick 链路和单实例锁。
5. 待完成：补早期失败、拒绝和恢复台账。
6. 待完成：引入统一的退避和重试预算。

## Acceptance Criteria

- 单一目标可以被持续推进，而不是只靠一次性命令。
- 任务池、routine、验收和恢复之间可以相互追踪。
- 早期失败不会丢失。
- 移动端和远程端能触发同一套控制动作。
- 不出现第二套状态源。

## Deferred Work

- 常驻守护进程或钩子式触发。
- 高吞吐场景下的共享事务型去重存储。

## Implemented Slice

- `goals tick` combines `routines run-due` and `goals advance` without adding a second scheduler.
- `goals watch` provides a local polling lane over the same tick path with a single-instance lock.
- `advance_research_loop` is projected through host actions and executable through `/api/tui/action`.
- The implementation keeps existing goals, routines, agents, ProjectOps, host surface, and mobile/remote authority boundaries.
