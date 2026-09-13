# 长跑阻塞记录：review ref 通过看板更新后无法被 dispatch 解析

日期：2026-07-13

## 摘要

新一轮真实用户形态长跑在 65 个 tick 后停止，状态为 `blocked/supervisor_error`。

这不是 `93c6f02` 修复的 runtime task-packet 绝对路径问题复发。provider worker 已经能从全局 runtime root 的 task packet 正常启动，并且本轮 14 个 worker 全部完成且 manifest 为 `complete/valid`。

当前阻塞是新的系统逻辑 bug：`update_board_task` 允许 main agent 将 `review_packet:*` 和 `review_trace:*` 写入 board task refs，但后续 dispatch 生成 task packet 时无法解析同一批 refs，导致 supervisor 直接阻塞退出。

## 运行现场

- install root: `/tmp/astra_release_install_fix_H11jca/research-cli-0.1.0-x86_64-unknown-linux-gnu`
- user root: `/tmp/astra_real_user_research_fullauto_YbCW1Y`
- state root: `/tmp/astra_real_user_state_fullauto_p6rPxl`
- project id: `proj_astra_real_user_research_fullauto_YbCW1Y_f53da6cadfc7`
- job id: `arj_1783879065266_a382f60b3ac8c334`
- mode: `full_auto`
- provider/model: `openai/gpt-5.4`
- final status: `blocked`
- final phase: `supervisor_error`
- ticks completed: `65`
- stop reason: `mission_frame_invalid`

Final error:

```text
invalid task packet: required input artifact refs could not be resolved for dispatch: review_packet:rev_1783881922936, review_trace:rev_1783881922936
```

## Evidence

Job state:

- `/tmp/astra_real_user_state_fullauto_p6rPxl/projects/proj_astra_real_user_research_fullauto_YbCW1Y_f53da6cadfc7/research/jobs/arj_1783879065266_a382f60b3ac8c334/job.json`

Relevant event:

- `/tmp/astra_real_user_state_fullauto_p6rPxl/projects/proj_astra_real_user_research_fullauto_YbCW1Y_f53da6cadfc7/research/jobs/arj_1783879065266_a382f60b3ac8c334/events.jsonl`
- event `supervisor_error`
- payload code `mission_frame_invalid`
- payload message contains unresolved `review_packet:rev_1783881922936` and `review_trace:rev_1783881922936`

Board task that triggered dispatch failure:

- `/tmp/astra_real_user_state_fullauto_p6rPxl/projects/proj_astra_real_user_research_fullauto_YbCW1Y_f53da6cadfc7/main-agent-board/tasks/research_stage_task__stage_1783879065267_0__source_verification.json`

The task contains these refs:

```text
input_artifact_refs:
- accepted_worker_evidence_task:research_stage_task::stage_1783879065267_0::paper_search
- review_packet:rev_1783881922936
- review_trace:rev_1783881922936

review_target_evidence_refs:
- review_packet:rev_1783881922936

review_findings_refs:
- review:rev_1783881922936:wrong-stage-evidence
```

The refs were written by a successful main-agent tool call:

- session transcript: `/tmp/astra_real_user_state_fullauto_p6rPxl/projects/proj_astra_real_user_research_fullauto_YbCW1Y_f53da6cadfc7/sessions/sess_1783879065277641703/transcript.jsonl`
- tool: `update_board_task`
- call id: `call_yjQ3n202ByvRaErlQYfXyBSH`
- tool status: succeeded

## Why This Is A System Bug

This is not a provider fault, worker fault, or self-healing queue state:

- `provider_faults=[]`
- all observed worker manifests are `complete/valid`
- background runner exited after `supervisor_error`
- job status is `blocked`, not `running`
- `last_loop_closure.should_continue=false`

The contract is inconsistent:

1. Main-agent board update accepts `review_packet:*` and `review_trace:*` refs.
2. Orchestration/task pool can project the updated task as dispatchable.
3. Dispatch then cannot resolve the same refs into task-packet input artifacts.

The error is also misclassified:

- observed code: `mission_frame_invalid`
- actual class: board task ref resolution / task packet input artifact resolution failure

This misclassification sends operators toward `goals status` and mission frame fields even though the MissionFrame is not the root cause.

## Impact

Long-running full-auto research can halt after a failed worker review is routed into a repair task. The main agent can correctly update a task with review context, but the runtime cannot mount that context for the next worker. This blocks recovery from review failures, which is a core loop for autonomous stage progression.

The bug likely affects any board task update that uses review refs in fields consumed by dispatch, especially:

- `input_artifact_refs`
- `review_target_evidence_refs`
- possibly `review_findings_refs`, depending on dispatch resolver policy

## Fix Direction

Preferred fix:

1. Make the board-task ref validator and dispatch input resolver share one canonical ref resolver for review refs.
2. Ensure `review_packet:<id>` resolves to the persisted review packet path/body.
3. Ensure `review_trace:<id>` resolves to the persisted review trace path/body.
4. Keep `review:<id>:<finding>` as a finding coordinate, not necessarily a mounted artifact unless the resolver explicitly supports it.
5. If a ref is allowed in `update_board_task`, dispatch must either resolve it or fail the update immediately with a precise validation error.
6. Reclassify this failure away from `mission_frame_invalid`; use a task/dispatch-specific failure code such as `task_input_ref_unresolved` or `board_task_ref_unresolved`.

## Regression Tests To Add

Minimum tests:

1. A board task updated with valid `review_packet:<id>` and `review_trace:<id>` in `input_artifact_refs` can be dispatched into a worker task packet.
2. `update_board_task` rejects a syntactically valid but nonexistent `review_packet:<id>` if dispatch cannot resolve it.
3. Supervisor error classification for unresolved task input refs is not `mission_frame_invalid`.
4. Full-auto recovery from a worker review failure can publish/update a repair task that consumes the failed review packet and continue to dispatch.

## Related Baseline

This run used the package built after:

- `93c6f02 Allow runtime task packet paths for provider workers`

That fix is considered validated by this run: provider workers started from global runtime-root task packets and completed valid manifests. The present bug is the next blocker beyond that baseline.
