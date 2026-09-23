# 开发指南

普通用户请使用[发布包快速开始](../getting-started.md)。本页面向修改 main 源码的开发者；main 不等于已发布的 alpha.1。

## 从源码启动

完整源码开发建议使用本轮验证过的 Node.js 25.9.0。Astra 本身声明最低 22.19，但一个上游示例要求 23.6 以上；22.19 的安装告警与实际检查范围见[验证记录](public-release-validation.md)。

```bash
git clone https://github.com/Runder-sun/Astra.git
cd Astra
npm ci --ignore-scripts
npm run build:offline
node packages/astra/dist/workbench.js --root ./research --port 4319
```

工作台仍需可用的官方 Codex 登录，启动页面本身不会创建模型任务。不要把开发工作区指向重要私人数据。

## 修改与检查

```bash
npm run check
node --test scripts/check-astra-runtime-boundary.test.mjs scripts/prepare-astra-source.test.mjs
cd packages/astra
node ../../node_modules/vitest/dist/cli.js --run test/workbench.test.ts test/research.test.ts
```

`npm run check` 包含自动格式修复，运行后检查差异。首次运行指定测试前先完成离线构建，因为工作区包通过编译产物互相引用。需要全部非端到端测试时使用仓库根目录的 `./test.sh`；不要直接运行完整 Vitest 或 `npm test`。

## 结构

- `packages/astra/`：研究调度、状态、验收、工作台和研究技能。
- 其余 Pi 包：当前执行所需的上游组件，暂不进行大规模拆分。
- `docs/`：用户文档、开发参考与历史记录。
- `scripts/`：构建、检查和发布辅助工具。

旧 Rust 运行时由[历史标签](../history/README.md)保留，当前构建与检查不依赖它。`.pmcli` 只读迁移报告仍由当前 Node 实现处理。

[研究内核与命令行参考](research-kernel.md) · [发布流程](releases.md) · [贡献规则](../../CONTRIBUTING.md)
