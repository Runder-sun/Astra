# 全自动研究信息通信闭环文档自审

日期：2026-05-27

## 审核对象

- `docs/superpowers/specs/2026-05-27-autonomous-research-information-closure-prd.md`
- `docs/superpowers/plans/2026-05-27-autonomous-research-information-closure-plan.md`

## 审核结论

当前 PRD 和计划方向正确：它没有另起一套自动研究系统，而是在现有主 agent loop、任务看板、agent team、review、evidence、DAG 和 cleanup 之上补信息通信闭环。它也明确了 runtime 不能越权做科研决策。

但为了真正指导实现，还需要补强以下点。

## 发现 1：协议完整不等于信息完整

风险：

只定义统一协议，不能自动保证信息完整。信息完整至少需要四个证明：

1. 表达证明：状态对象能表达当前责任。
2. 传递证明：runtime 确实把对象传给了目标 agent。
3. 消费证明：agent 输出证明自己用过这些输入。
4. 回写证明：review 和主 agent 决策写回同一个目标证据。

补充要求：

- PRD 已经写入“表达完整、传递完整、消费可证明、评审可绑定、决策可回写”。
- 计划中需要每一阶段都绑定测试，不允许只靠长跑观察。

状态：已覆盖，但实现阶段必须严格保持。

## 发现 2：高质量标准不能由 runtime 写死

风险：

如果把 stage gate 的科研标准写进 runtime，就会回到硬编码研究逻辑。runtime 可以校验结构和门禁，但不能决定“顶尖论文级别”的科研标准内容。

补充要求：

- 高标准应由主 agent 组织生成，可以调用标准设定类 agent 或专家 reviewer 辅助。
- runtime 只要求标准结构存在、review 绑定目标证据、非 pass 形成 blocker。
- 具体科研标准不能写死成数量指标。

状态：PRD 已覆盖“高质量门关系”，后续实现要避免把 benchmark、文献数量、方法类型写死。

## 发现 3：worker 能力增强不能变成特殊通道

风险：

之前调研质量差时，容易直接给 literature worker 加特殊逻辑或 runtime 搜索能力。这会破坏共享 agent 基类 + role soul + role package + tool policy 的架构。

补充要求：

- 每类 agent 的增强应通过 role soul、role package、tool policy 和 evidence bundle 装配完成。
- runtime 只根据任务合同装配上下文和工具，不为某个研究主题生成特殊内容。

状态：PRD 和计划已写入，但实现时需要扫描是否仍有 runtime 搜索、fallback 或主题硬编码。

## 发现 4：review 失败必须形成主 agent 责任，而不是 runtime follow-up

风险：

如果 runtime 把 review 失败自动变成 follow-up task，本质上仍然是 runtime 越权。正确做法是 runtime 投影 blocker，主 agent 根据全局上下文决定补调研、重写方案、回溯、替换或 clean up。

补充要求：

- 非 pass review 绑定目标证据并生成 blocker。
- blocker 进入 continuity packet。
- 主 agent 必须通过 board task 或 control tool 明确处理。

状态：PRD 和计划已覆盖。

## 发现 5：active evidence set 是避免旧口径污染的关键

风险：

如果 accepted evidence 只是追加账本，旧实验、旧方案、旧 review failure 会一直污染当前阶段，导致质量门反复卡住，主 agent 也不知道哪些证据已经不属于当前口径。

补充要求：

- 每条 evidence 必须有 active/superseded/rejected/candidate 状态。
- stage gate 只检查 current active evidence set。
- supersede 必须由主 agent 明确决策。
- clean up 和 DAG route change 必须能引用 supersede/replacement 关系。

状态：PRD 和计划已覆盖。

## 发现 6：resume 要基于持久状态，而不是模型上下文

风险：

如果 resume 依赖上一轮模型对话，换模型或 provider 后就不能无缝继续。理想状态是项目目录中有足够持久化状态，任何 provider 都能重建研究上下文。

补充要求：

- continuity packet 必须包含 active evidence、blockers、pending reviews、ready tasks、last decisions、allowed transitions。
- main agent prompt 只消费这个 packet 即可继续。

状态：PRD 已覆盖，计划中阶段 5 已覆盖。

## 仍需实现时重点检查的问题

1. 是否还有 runtime 自动生成科研 follow-up task。
2. 是否还有 runtime 搜索、总结、诊断后直接写 canonical artifact。
3. 是否还有 review task 没有目标 evidence 也能运行。
4. 是否还有 worker 在没有挂载上游证据时继续猜测。
5. 是否有 accepted evidence 没有主 agent 决策却被当作 active。
6. 是否有 stage gate 混用了 superseded evidence。
7. 是否有 resume packet 只给摘要、不给责任对象。

## 审核后补充结论

本 PRD 能解决当前实现暴露的本质缺陷：不是再加一个更强的 runtime，而是让主 agent、agent team、reviewer 和 runtime 之间的信息成为可传递、可消费、可绑定、可回写的责任对象。

下一步不应该直接长跑，而应该先按计划补测试和结构实现。否则长跑很可能继续消耗轮次，却只得到更多低质量或无法绑定的产物。

## 审核后已补充内容

- 在 PRD 中加入“五类证明矩阵”：表达证明、传递证明、消费证明、绑定证明、回写证明。
- 在 PRD 验收标准中明确：关键闭环必须有自动化测试或可重复本地验证，不能只靠长跑观察。
- 在计划中加入“阶段 8：实现后防偏审计”，专门防止 runtime 越权、研究主题硬编码、fallback 伪闭环和 canonical artifact 无主 agent 决策。
