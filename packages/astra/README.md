# Astra 研究工作台

面向个人本机使用的实验性自动研究框架。组织文献、实验、独立审阅和成果交付，保留结论依据。程序验收通过不等于科学结论正确。

## 实验版 0.1.0-alpha.1

当前下载版为 `v0.1.0-alpha.1`。Linux 是目前已验证的平台；需要 Node.js 22.19 或更高版本、npm 和已登录的官方 Codex CLI。历史真实后端验证使用 Codex CLI 0.153.4、`gpt-5.6-luna`，其他系统和 CLI 版本尚未验证。

```bash
mkdir astra-test
cd astra-test
npm init -y
npm install --ignore-scripts https://github.com/Runder-sun/Astra/releases/download/v0.1.0-alpha.1/earendil-works-pi-astra-0.1.0-alpha.1.tgz
npx --no-install astra-workbench --root ./research --port 4319
```

在本机浏览器打开 `http://127.0.0.1:4319`。首次选择 24 个任务，先不交付论文，使用可复核的小问题。任务预算不是实际费用上限。安装包沿用内部 Pi 包名，仅通过 Astra 附件分发，不代表上游 Pi 官方发布。

工作台支持新建、查看、暂停、补充说明后继续及正式成果下载。关闭浏览器不会停止研究；服务重启后不自动接管任务。请保留整个研究目录，并核验原始结果与未关闭问题。

## 文档

- [安装、环境要求与第一次研究](https://github.com/Runder-sun/Astra/blob/main/docs/getting-started.md)
- [版本差异、验证记录与已知限制](https://github.com/Runder-sun/Astra/blob/main/docs/support.md)
- [研究示例及尚未完成的案例](https://github.com/Runder-sun/Astra/blob/main/docs/examples/README.md)
- [开发指南](https://github.com/Runder-sun/Astra/blob/main/docs/development/README.md)
- [内核、命令行和后端技术参考](https://github.com/Runder-sun/Astra/blob/main/docs/development/research-kernel.md)
- [发布版原始说明](https://github.com/Runder-sun/Astra/releases/tag/v0.1.0-alpha.1)

`main` 是开发源码，不能把开发能力视为 alpha.1 附件已包含的能力。真实全流程最终验收尚未完成；本版本不保证研究结论、任务收敛或论文质量。只供单用户本机访问，不能作为公网服务部署。

基于 Pi，遵循 MIT 许可证；保留上游版权声明。
