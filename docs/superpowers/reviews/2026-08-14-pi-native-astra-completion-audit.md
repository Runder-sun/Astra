# Pi-native Astra 目标完成度审计

日期：2026-08-14

结论：核心 Pi-native 产品、完整 runtime-contract auto-research 和 Rust 产品边界
已完成并通过验证。原始目标只剩一个硬缺口：当前没有有效 provider 凭据，尚未
完成真实联网研究质量验收。

## 1. 目标与证据

| 原始要求 | 权威证据 | 结论 |
| --- | --- | --- |
| 理解 Astra 自动/半自动研究设计 | `docs/superpowers/specs/2026-08-12-pi-native-astra-architecture.md` 定义 MissionFrame、14-stage DAG、TaskPacket、evidence/review/adoption、supervisor、memory 和三档 automation | 已完成 |
| 学习 Pi 官方和全网实践 | 架构文档第 2 节交叉引用 Pi extension/SDK/harness，以及 `pi-chat`、`pi-messenger`、`rho`、`pi-hermes-memory`、`senpi`、`pi-web-access`、`pi-interactive-shell` | 已完成 |
| 以完整 Pi 源码为产品主干 | 根目录为 Pi monorepo；`packages/astra/src/launcher.ts` 调用 Pi `main()`；provider/session/CLI/TUI/RPC 均由 Pi package 持有 | 已完成 |
| 使用 Pi CLI、TUI、RPC 的统一表面 | `/tmp/astra-pi-tui-smoke.7O8XN8/cli-status.jsonl`、`tui-status.txt`、`rpc-status.jsonl` 均读取 `job_97e21daf-de2b-4007-9b5a-15faadeb2db2`、`eventSeq=4` 和同一 validation gate | 已完成 |
| 使用 Pi 原生 skills | `packages/astra/skills/astra/*.md` 是 Pi 可加载 skill；`PiChildSessionRunner` 用 `--no-skills` 加当前 stage/role 的显式 `--skill`；`pi-skill-loading.test.ts` 验证 loader 和子进程参数 | 已完成 |
| Pi inner loop + Astra multi-agent outer loop | `packages/astra/src/pi-child-session.ts` 使用独立 Pi worker/reviewer/main-agent session；`supervisor.ts` 只负责 durable 调度、lease、恢复和校验 | 已完成 |
| 自动/半自动 policy、预算和恢复 | `automation-policy.test.ts`、`research-control.test.ts`、恢复 E2E 覆盖 collaborative/autonomous/full、task/turn/cost gate、resume、compaction 和 ready task 恢复 | 已完成 |
| 完整 auto-research runtime 测试 | `/tmp/astra-pi-skills-e2e.wfK3tF/audit.json`：14/14 stages、848/848 连续事件、58 tasks、29 evidence/reviews、1 repair、0 open obligation、28 matching receipts、129 Pi sessions、0 failed | 已完成（fixture contract） |
| 完整真实联网 auto-research 质量测试 | `openai/gpt-5.4-mini` 最小请求返回 401；Pi OAuth provider 均未配置。fixture 不证明文献、实验和论文质量 | 未完成 |
| 不再存在第二套产品 runtime | 用户选择方案 2；完整 Cargo crate 位于 `legacy/rust/`，根目录无 Cargo/Rust 产品入口，默认 npm build/release 不引用归档 | 已完成 |

## 2. 最新验证

- `npm run check`：通过，1083 files，无自动修复。
- `npm run build:offline`：通过，包括 Pi packages 和 `@earendil-works/pi-astra`。
- `PATH="$HOME/.pi/agent/bin:$PATH" ./test.sh`：退出码 0。
- Astra：7 files，30 tests 全通过。
- coding-agent：219 files，1926 tests 全通过，49 skipped。
- `check:astra-runtime-boundary`：98 个 Rust source 全部位于 `legacy/rust/`；
  根目录旧 runtime 路径和默认产品脚本引用均为 0。
- Rust 归档：`cargo fmt --check`、735 个 lib tests、packaging、2 个 parity
  harness tests 和用户原有 operator regression 全部通过。
- `@earendil-works/pi-astra` npm pack dry-run：70 个文件，只包含 launcher、
  Pi-native runtime 和 source/dist skills，不包含 Rust 归档。
- 新 fixture 产品 E2E job：`job_ada253d5-2d42-4d8a-b3d6-ecc503792b74`。
- E2E audit：`/tmp/astra-pi-skills-e2e.wfK3tF/audit.json`。

宿主环境没有系统 `fd`；Pi 已将其安装到 `~/.pi/agent/bin/fd`。Vitest 默认
`PI_OFFLINE=1`，因此全量测试必须让该目录进入 PATH。这是环境前提，不是 Astra
回归。

## 3. 剩余关闭条件

1. 在执行环境配置有效 provider 凭据，不在聊天中传递密钥。
2. 使用真实 provider 运行有 task/turn/cost 硬上限的 14-stage full research，
   审查真实 source refs、实验产物、claim 支持关系和最终研究质量。
3. 真实运行通过后重新执行 completion audit，确认没有 live-quality 缺口，再
   标记目标完成。
