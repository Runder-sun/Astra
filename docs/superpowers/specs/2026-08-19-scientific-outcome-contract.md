# Astra Scientific Outcome Contract

日期：2026-08-19

状态：已实现并进入 live research 验证。

## 1. 问题

旧 completion gate 能证明研究流程经过 evidence、review 和 adoption，但不能区分：

- 研究过程是否完整；
- 主研究问题是否被支持；
- 当前证据是否充分覆盖主问题。

具体失败轨迹是：一个宽泛的可复现性问题只得到单 fixture 的 traceability 证据，系统通过
收窄 claim 完成了流程。收窄 claim 是正确的科研行为，但把该状态只显示为 `completed` 会让
用户误以为原始问题已经获得支持。

## 2. 状态模型

三个状态必须分开：

```text
process status: running | waiting-for-user | completed
scientific outcome: pending | supported | partially-supported | refuted |
                    inconclusive | insufficient-evidence
mission coverage: pending | sufficient | insufficient
```

`completed` 不再表达正面科学结论。负结果、反驳结果和严格的不确定结果都可以完成，但必须保留
其真实 `scientificOutcome` 和 `missionCoverage`。

## 3. Claim 语义

每个 result-to-claim claim 必须有独立 assessment：

```text
supported | partially-supported | refuted | unsupported | unresolved
```

只有 `supported` 和 `partially-supported` 可以进入 `acceptedClaimIds`。其他 claim 仍保留在
research graph 中，供用户、reviewer 和后续 backtrack 查看，但不能满足正面结论门槛。

## 4. Review 与完成

- `result-to-claim` 至少需要两个独立 passing reviews；
- `research-review` 至少需要两个独立 passing reviews；
- whole-research review 必须独立报告 outcome 和 coverage，并与 canonical result-to-claim 一致；
- outcome 为 supported 或 partially-supported 时，必须至少存在一个 accepted claim；
- outcome 为 refuted、inconclusive 或 insufficient-evidence 时，不强迫生成正面 claim；
- required artifact 缺失时不能完成。

`--require-paper` 当前展开为 `paper-write` 和 `paper-compile` 两个 required artifact。

## 5. 用户协同

用户指导继续作为 durable research-graph node 保存，同时进入 main-agent 的有界 route context 和
Research Board。它不再只是写入 ledger 后对实际路线不可见。

## 6. 审计

v4 audit 分开报告：

- `runtimeIntegrity`：状态、任务、session、artifact 和恢复协议；
- `researchQuality`：review quorum、search evaluation、claim/outcome 一致性和完成决策；
- `scientificResult`：outcome、coverage、accepted claim 数量和 process completion；
- `unresolvedUncertainty`：open question、objection 和 obligation。

audit 通过表示运行与科研契约自洽，不表示科学结果一定为正面。
