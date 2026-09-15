# Astra

[项目主页](https://runder-sun.github.io/Astra/) · [当前修复状态与剩余差距](docs/superpowers/reviews/2026-09-15-joint-review-fix-status.md)

**实验版 `v0.1.0-alpha.1`。** 面向个人本机使用，真实研究尚未通过最终全流程验收。
推荐先下载安装包，从[工作台快速开始](packages/astra/README.md#实验版-010-alpha1)进行小预算测试。
使用限制、验证范围与依赖审计见 [发布说明](RELEASE_NOTES.md)。

Astra 是生长在 Pi `0.84.1` 完整 monorepo 上的自动/半自动研究 agent。Pi 统一
负责 provider、agent loop、session、CLI、TUI、print、JSON、RPC、skills 和
extensions；Astra 只负责研究领域 contract、durable state 和 outer supervisor。

## 产品入口

```bash
npm install --ignore-scripts
npm run build:offline

npm run astra --
npm run astra -- research run --automation autonomous "研究目标"
npm run astra -- research run --automation full --require-paper "研究目标并产出论文"
npm run astra -- research status
npm run astra -- research tick
npm run astra -- research pause "等待人工决策"
npm run astra -- research resume
```

`packages/astra/src/launcher.ts` 是产品入口。默认 Pi 后端通过 Pi `main()` 和扩展调用研究控制服务。
`--backend codex` 直接调用同一研究控制服务，通过官方 Codex App Server 管理登录、模型和工具；
无需额外模型 API 密钥。主控保留会话，工作任务隔离，每次独立审阅新建会话。
Astra 管理研究状态与验收，不复制后端的模型执行循环。

```bash
export ASTRA_CODEX_MODEL=gpt-5.6-luna
node packages/astra/src/workbench.ts --root ./research --port 4319
```

普通浏览器打开 `http://127.0.0.1:4319`。使用前安装并登录官方 Codex CLI。

子会话通过 Pi 原生 `--skill` loader 加载 Astra 指令：关闭环境中的默认 skill
发现，只显式绑定当前 stage 和 worker/reviewer/main-agent role 对应的 package
skill，避免无关项目 skill 污染自动研究任务。

`research run` 和 `research resume` 都会在 tick budget 内持续驱动 outer loop，直到
完成或进入持久 user/budget gate；`research tick` 只执行一个 durable supervisor
transaction，适合人工或调度器单步控制。三种模式的实际语义是：

- `collaborative`：普通研究循环自动执行，只在科研偏好、边界、风险或外部事实会改变路线时主动询问用户，并在 `gate: user` 的能力关闭前等待确认；
- `autonomous`（默认）：普通 stage 自动推进，`gate: user` 的 stage 关闭前等待确认；
- `full`：软 gate 内全自动，任务、turn、cost、权限和破坏性操作等硬边界仍会暂停。

研究过程状态和科学结论是两个不同维度。`completed` 只表示证据、审查和路线闭环已经
完成；`scientificOutcome` 单独记录 `supported`、`partially-supported`、`refuted`、
`inconclusive` 或 `insufficient-evidence`，`missionCoverage` 记录主问题证据是否充分。
`result-to-claim` 和最终 `research-review` 必须各有两次独立通过，且二者对这两个字段一致。

`--require-paper` 把 `paper-write` 和 `paper-compile` 加入显式交付物。缺少其中任一 canonical
artifact 时，main agent 不能提交完成决策。

Pi 交互模式提供 `/research-board` 查看问题、假设、claim、异议、候选评分、预算和下一决策；
`/research-guide <text>` 将用户意见写入同一研究图，`/research-route` 查看当前唯一 canonical route
及 search 比较结果。

全局预算可以在创建时设置，也可以在预算 gate 后提高再恢复：

```bash
npm run astra -- research run --automation full --max-tasks 80 --max-turns 300 --max-cost-usd 20 "研究目标"
npm run astra -- research resume --max-turns 400
```

不可恢复的 provider 认证、授权、凭据或模型配置错误会在第一次失败后持久暂停，不会继续消耗
research turns；修复 provider 配置后再显式 `research resume`。

研究事实存放在项目 `.astra/`：

- `job.json`：原子快照；
- `events.jsonl`：append-only 研究事件；
- `tasks/`：TaskPacket、worker/reviewer manifest 和 trace；
- `canonical/`：adoption/retirement artifact 与 checksum receipt；
- `sessions/`：Pi child session JSONL。

Pi transcript 是对话事实，`.astra` ledger 是研究事实；两者通过 session ref、
task id、evidence id 和 job id 连接。

## 验证

源码仓库保留上游 Pi 构建结构及 MIT 声明；Astra 发布使用独立实验版附件。
根目录的上游 `release:*`、`publish*` 脚本用于 Pi 全套包，不是 Astra 发布入口，请勿用于本实验版发布。

```bash
npm run check
npm run build:offline

ASTRA_FIXTURE_PROVIDER=1 ASTRA_MAX_TICKS=64 \
  npm run astra -- research run --automation full "Pi-native auto research audit"

node scripts/audit-astra-run.mjs \
  /path/to/workspace \
  job_id \
  /path/to/pi-parent-sessions
```

fixture provider 仍经过真实 Pi `Agent`、tool loop、JSON child process 和 session
持久化，但它只证明 runtime contract，不证明联网文献、实验或论文质量。

## Legacy Rust 边界

迁移前的完整 Cargo crate 已归档到 `legacy/rust/`。仓库根目录不再保留 Cargo
manifest、Rust CLI/TUI 或旧研究 runtime 入口；默认 npm build、test、package 和
release 都不会编译或分发该归档。

归档只用于 `.pmcli` 迁移取证、旧行为 baseline 和 parity 检查，不能继续增加产品
能力。需要手工验证旧实现时，必须显式指定
`--manifest-path legacy/rust/Cargo.toml`；新研究状态始终由 `.astra` 持有。

## 设计文档

- `docs/superpowers/specs/2026-08-12-pi-native-astra-architecture.md`
- `docs/superpowers/specs/2026-08-14-pi-rust-parity-boundary.md`
- `docs/superpowers/specs/2026-08-19-scientific-outcome-contract.md`
- `packages/astra/README.md`
