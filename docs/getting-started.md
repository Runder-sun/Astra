# 快速开始

[返回项目首页](../README.md) · [版本与支持](support.md)

本页默认安装已发布的 **v0.1.0-alpha.1**。开发源码使用另一套[开发步骤](development/README.md)，不要混用源码命令和已安装的程序。

## 环境要求

| 项目 | 要求与验证范围 |
| --- | --- |
| 系统 | 当前验证范围为 Linux；macOS 和 Windows 尚未验证 |
| Node.js | 声明最低 22.19；原始 alpha.1 发布在 25.9.0 验证，后续检查见[版本与支持](support.md) |
| npm | 用于安装固定版本附件 |
| 模型账号 | 工作台使用已登录的官方 Codex CLI；需账号具有 Codex 使用权限 |
| CLI 与模型 | 历史真实验证：Codex CLI 0.153.4、gpt-5.6-luna；不代表其他版本或账号都可用 |
| 实验工具 | 根据研究问题准备，例如 Python；第一次建议只用标准库 |
| 论文工具 | 可选；需要适合稿件的 TeX 环境与 Poppler 的 pdfinfo，首次试用先关闭论文交付 |

先按[官方 Codex CLI 文档](https://developers.openai.com/codex/cli/)安装，在终端执行 `codex login` 完成登录；账号说明见[官方认证文档](https://developers.openai.com/codex/auth/)。Astra 工作台不提供网页内登录。不要把个人令牌写入研究目标或问题报告。

## 安装固定版本

以下命令用于新的 Linux 目录，下载的是发布附件，不是整个开发仓库：

```bash
mkdir astra-test
cd astra-test
npm init -y
npm install --ignore-scripts https://github.com/Runder-sun/Astra/releases/download/v0.1.0-alpha.1/earendil-works-pi-astra-0.1.0-alpha.1.tgz
npx --no-install astra-workbench --root ./research --port 4319
```

在本机普通浏览器打开 `http://127.0.0.1:4319`。保留安装生成的 `package-lock.json`，便于以后恢复相同依赖。也可从[发布页](https://github.com/Runder-sun/Astra/releases/tag/v0.1.0-alpha.1)手动下载 tgz 和 SHA256SUMS，校验后将安装命令中的网址换为下载文件的绝对路径。

## 第一次研究

1. 点击“新建研究”，输入下方示例或自己有明确边界的问题。
2. 选择 **24 个任务**，暂不勾选“交付论文与 PDF”。这个预算可能不足以完成研究，但适合先检查运行过程。
3. 点击“开始研究”，观察阶段验收项、审阅意见和未关闭问题。
4. 遇到确认或预算等待时，先检查已有成果，再决定是否补充说明或提高预算继续。
5. 下载已采纳成果，同时检查完整任务目录中的代码、原始结果和失败记录。

> 固定随机种子，比较均值和中位数在小型污染正态样本上的误差。仅用 Python 标准库，在本机做小规模实验。保留源码、原始结果、运行命令和失败记录，不声称方法创新。

任务数量包含执行和审阅，不等于模型内部调用次数，也不是实际费用上限。订阅额度由官方服务控制。文献数量或审阅次数不能代替证据质量判断。

## 暂停、继续和备份

- **暂停**：点击“暂停研究”，等待界面显示已暂停。不要把关闭网页当作暂停；暂停调度也不应被当作终止所有外部进程的保证。
- **继续**：在同一任务中填写补充说明后点击“继续研究”；需要时提高任务预算。
- **备份**：先暂停并确认执行情况，再备份整个 `research/run-*` 目录，包括隐藏的 `.astra` 和任务资源。只保存 PDF 会丢失研究过程与复现材料。
- **服务重启**：没有自动接管保障。检查“任务目录与运行输出”，确认没有原执行进程后，进入对应任务目录，使用安装位置的程序恢复（替换下方两处绝对路径）：

```bash
cd /absolute/path/to/research/run-id
/absolute/path/to/astra-test/node_modules/.bin/astra research status
/absolute/path/to/astra-test/node_modules/.bin/astra research resume
```

研究命令使用当前目录定位任务，具体参数见[技术参考](development/research-kernel.md)。恢复会再次调用模型，请先确认研究目标与预算。

## 常见问题

| 现象 | 先检查什么 |
| --- | --- |
| 浏览器打不开 | 终端是否仍在运行；端口是否为 4319；是否使用本机普通浏览器 |
| 程序在远程服务器 | 在自己的电脑运行 `ssh -L 4319:127.0.0.1:4319 user@server`，再打开本地地址；不要把服务监听地址改为公网 |
| 登录、模型或额度错误 | 先在同一机器确认官方 CLI 可用及账号额度；保留错误后修复配置，再显式继续 |
| 文献检索超时 | 检查网络和代理；来源记录区分检索失败、摘要片段与全文，不要把无结果当证据 |
| 没有论文或 PDF | 是否选择交付论文、编译工具是否完整、是否还有未关闭审阅问题；编译成功后仍需检查页面 |
| 任务一直修复 | 检查重复驳回原因，收窄目标或暂停；本实验版不保证收敛 |

无法解决时，按[贡献指南](../CONTRIBUTING.md)提交脱敏后的复现步骤。
