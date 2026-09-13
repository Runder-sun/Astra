# Pi-native Astra 与 Rust legacy parity 边界

日期：2026-08-14

状态：方案 2 已执行；Rust runtime 已移出产品主干。

## 1. 决策

Pi monorepo 是唯一的新产品 runtime。`packages/astra` 是 Pi 上的研究领域层。
Rust 代码只允许用于迁移取证、旧行为 baseline 和归档 parity 对照，不得继续
承载新 CLI、TUI、provider、agent loop 或 auto-research 产品能力。

用户已选择方案 2。完整旧 Cargo crate 现在位于 `legacy/rust/`；仓库根目录不再
存在 Cargo manifest、Rust CLI/TUI 或旧 runtime 入口。默认 npm build、test、
package 和 release 不编译或分发该归档。

## 2. 能力归属

| 能力 | 新 owner | Pi-native 证据 | Rust 状态 |
| --- | --- | --- | --- |
| provider/auth/model | `packages/ai` + `ModelRuntime` | Pi provider catalog、auth、retry tests | 旧实现冻结，不能新增 provider |
| agent/tool loop | `packages/agent` + coding-agent session | worker/reviewer/main-agent child JSON sessions | `legacy/rust/src/runtime/agent_loop.rs` 仅作 baseline |
| CLI/print/JSON/RPC | Pi `main()` + Astra extension flag | parent Pi session 与 `astra_research_result` | 旧 binary 源码只在显式 Cargo manifest 下手工运行 |
| TUI | `packages/tui` + `packages/coding-agent` | Astra status/widget/slash commands | Rust TUI 文件冻结 |
| skills/context/compaction | Pi resource discovery + extension hooks | stage/role skills、checkpoint tests | 旧 prompt/skill surface 仅作迁移参考 |
| research DAG/TaskPacket | `packages/astra` | 14-stage contract tests、58 TaskPackets | 旧 orchestration 不再是 canonical |
| evidence/review/adoption | `packages/astra` `.astra` ledger | 29 evidence、29 review、28 receipts | `.pmcli` 只读，不自动 promotion |
| supervisor/recovery | `packages/astra` outer loop | lease、repair obligation、restart tests | Rust supervisor 只作 parity baseline |
| remote projection | Pi RPC/TUI extension event | 同一 status/widget/control service | 旧 remote daemon 随 Cargo crate 归档，不再是产品入口 |

## 3. 迁移验收

Rust 能力只有同时满足以下条件才算迁移：

1. 新入口必经 Pi `main()`，父 session 可持久化和反向绑定 `jobId`；
2. worker/reviewer/main-agent 使用 Pi session，不调用 Rust provider loop；
3. `.astra` event sequence 连续，lease 最终释放；
4. failed review 生成 obligation 和 repair TaskPacket；
5. canonical artifact 有 adoption、replacement/retirement 和 checksum receipt；
6. 同一 fixture/benchmark 的 contract parity 有机器报告；
7. 新产品 README、npm scripts 和 release artifact 不引导用户进入 Rust runtime。

fixture E2E 只能关闭 runtime contract gate，不能关闭 live-provider 研究质量 gate。

## 4. Legacy 归档规则

归档中允许：

- 修复只读导出、迁移报告或 parity harness；
- 修复阻止读取旧项目的安全/数据损坏问题；
- 添加 deprecation 和 compatibility contract 测试。

归档中禁止：

- 新增 Rust provider、模型路由、agent loop 或 tool loop；
- 新增 Rust TUI/remote 产品功能；
- 让 npm `astra`、文档 quick start 或新 release 脚本调用 Rust binary；
- 把 `.pmcli` 恢复为新研究状态的写入真相源。

## 5. 已执行决策

2026-08-14 用户明确选择方案 2：完整 Rust 树移入 `legacy/rust/`，默认构建和
release 不包含。`scripts/check-astra-runtime-boundary.mjs` 同时校验归档源码清单、
根目录旧入口不存在，以及默认产品脚本不调用 Cargo 或 `legacy/rust/`。

这关闭了 P5 的第二套产品 runtime 缺口。归档仍可显式运行，用于迁移和 parity；
它不具备产品入口地位，也不得重新接回 npm 产品脚本。
