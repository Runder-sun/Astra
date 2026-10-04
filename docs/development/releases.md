# Astra 发布流程

本流程用于 Astra 附件分发，与上游 Pi 的全包发布独立。当前对外版本为 v0.1.0-alpha.2；验证范围见[版本记录](../releases/v0.1.0-alpha.2.md)。更新 main 不会改变旧附件。

## 发布前

1. 在隔离工作区完成待发布修改，审阅差异并确定新版本。不要用相同版本覆盖旧附件。
2. 更新 Astra 包版本、锁文件和版本说明；主依赖应固定到验证过的版本。现有内部包名尚未迁移，不能声称属于上游官方发布。
3. 使用 `npm ci --ignore-scripts` 安装，运行 `npm run build:offline`、`npm run check` 和相关指定测试。
4. 在独立目录安装打出的包，检查命令入口、页面资源、默认执行器解析；最低 Node 版本也要验证。
5. 真实模型验证单独记录账号后端、CLI、模型、实验协议及失败。未经最终验收的研究和论文不得写成成功案例。

## 本地打包

```bash
npm run astra:package
npm run astra:source -- /absolute/path/to/new-source-directory
```

`astra:package` 检查已有编译入口并只打包 Astra，不发布到注册表、不打标签、不推送。先完成上面的离线构建。输出目录与包版本绑定，已存在则拒绝覆盖。

`astra:source` 从当前工作区导出允许的源码和用户文档，排除旧 Rust 与本机运行目录，生成文件校验清单。清单会标明是否包含未提交修改；正式发布应使用已经审阅并提交的干净源码。源码自带模型目录数据以支持离线构建。

## 发布附件

上传新的安装包、源码包、SHA256SUMS 与该版本验证记录。发布说明、官网和用户指南必须指向同一版本。GitHub 自动生成的源码压缩包与自定义源码附件应分别识别。

旧版 `RELEASE_NOTES.md` 和 `RELEASE_VALIDATION.md` 是 alpha.1 历史记录，不用新的测试数字覆盖。为新版本另写记录。原始 `SOURCE_MANIFEST.json` 已移至 `docs/history/alpha.1-source-manifest.json`，仅用于追踪首版导出来源。

## 上游维护工具

根目录上游发布命令使用 `upstream:` 前缀，只供维护 Pi 组件时参考，不是 Astra 发布流程，也不应在本仓库触发上游 npm 发布。Astra 的公开自动检查不包含发布、推送或真实模型调用。
