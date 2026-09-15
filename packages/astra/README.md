# Astra research

## 实验版 0.1.0-alpha.1

这是面向个人本机使用的实验性自动研究框架。研究过程的程序验收不等于科学结论正确；真实全流程案例仍未完成最终验收。请先用小预算、可复核的问题测试。

下载发布页的 `earendil-works-pi-astra-0.1.0-alpha.1.tgz` 后，在新的目录安装：

```bash
mkdir astra-test
cd astra-test
npm init -y
npm install --ignore-scripts /path/to/earendil-works-pi-astra-0.1.0-alpha.1.tgz
npx --no-install astra-workbench --root ./research --port 4319
```

需要 Node.js 22.19 以上、npm、已登录的官方 Codex CLI；当前真实后端验证范围为 Linux、Codex CLI 0.153.4、`gpt-5.6-luna`。安装包沿用内部 Pi 包名，仅通过 Astra 的实验版附件分发，不代表上游 Pi 官方发布。

使用普通浏览器访问 `http://127.0.0.1:4319`。远程服务器先在自己的电脑建立 SSH 本地端口转发；内嵌网页预览尚未完成兼容验证。工作台只供单用户使用，不能作为公网服务部署。

建议第一次输入：“固定随机种子，比较均值和中位数在小型污染正态样本上的误差，保留 Python 源码、原始结果和失败记录，不声称方法创新。”先选择小任务预算且不交付论文；预算耗尽后检查已完成成果再决定是否继续。任务预算不是模型实际费用的上限，订阅额度由官方服务控制。

实验版不保证研究结论、论文质量或任务必然收敛。当前不支持服务重启后自动接管运行进程；重要研究应保留整个任务目录，并检查原始证据和未关闭问题。

`@earendil-works/pi-astra` is the Astra research product layer for the Pi agent
harness, with an optional official Codex subscription backend. By default, Pi owns
provider calls, tool execution, sessions, TUI, print, JSON, and RPC. Astra owns the durable `.astra` research graph, canonical route,
TaskPackets, candidate search, evidence/review/adoption lifecycle, obligations,
and the outer supervisor loop. Stages are a capability catalog, not a fixed DAG.

The package exposes `createAstraExtension()` for Pi's `main(args, { extensionFactories })`
composition root and `runAstra()` for the `astra` launcher.

## 本机研究工作台（开发版）

在已安装依赖的仓库中运行，无需构建或安装前端依赖：

```bash
node packages/astra/src/workbench.ts --root /path/to/my-research --port 4319
```

打开 `http://127.0.0.1:4319`。点击“新建研究”，填写目标，选择任务预算和是否交付论文，再点击“开始研究”。真实任务使用官方 Codex 登录与 `gpt-5.6-luna`；不提供模拟研究按钮。每个新任务使用根目录内独立的 `run-*` 文件夹，使用现有控制服务和验收闭环。

页面支持阶段验收条目、最新审阅及历史意见、未关闭问题、正式成果下载、暂停、补充说明后继续，以及继续时提高任务预算。执行状态、科学结论、任务覆盖分别展示；没有人为估计的完成百分比。关闭浏览器不会停止后端任务，暂停需点击“暂停研究”。首次测试默认采用保留最终确认的自动运行模式。

通过 `--watch /path/to/existing-research` 添加已有工作区的当前任务，可以重复指定。导入任务仅供查看，不会自动执行或修改。服务只监听本机回环地址，写请求检查来源和会话令牌，下载仅允许成果记录中的任务内文件。暂不支持远程多人协作、在界面登录账号、任意旧任务切换或服务器重启后的自动接管；执行进程未连接时会如实显示，仍可使用研究命令行恢复。

开发验证使用独立的模拟执行器检查新建、暂停和继续，不消耗模型额度；真实旧研究只读展示。浏览器验证覆盖桌面与手机宽度、无横向溢出、无控制台错误。该验证不表示新建研究已经完成科学验收。

## Research CLI

The launcher keeps Pi's normal interactive, print, JSON, and RPC modes. Astra's
operator commands are a thin composition layer over the same durable job state:

```bash
astra research run --automation autonomous "investigate the target question"
astra research run --automation full --require-paper "investigate and write a paper"
astra research tick
astra research status
astra research pause "waiting for human input"
astra research resume
astra research migrate
```

`research run` creates `.astra/active-job.json` and `.astra/jobs/<job>/` and
drives the outer supervisor. The selected backend owns each child provider/tool/session loop;
Astra only validates TaskPackets, manifests, reviews, decisions, obligations,
and canonical adoption.

With the default Pi backend, worker, reviewer, and main-agent child sessions use Pi's native skill loader.
They disable ambient skill discovery and explicitly bind only the packaged
skill files matching `ASTRA_STAGE_ID` and the child role, while Astra's
extension continues to project durable mission context and project-local
overrides.

`research run` and `research resume` drive the bounded outer loop until the
persistent main-agent submits an explicit `complete` route decision or a durable
user/budget gate is reached. Each capability loop freezes a rubric, dispatches
isolated subagents, independently reviews original evidence, and either
continues, searches, advances, backtracks, asks the user, or completes.
Search is bounded by durable rounds (two by default). A justified
`continueSearch` exhausts the current batch without deleting its candidates,
evidence, or reviews; the next round receives the frozen evaluations and must
test orthogonal discriminators. The final round must select a passing candidate
using the frozen criterion order and a deterministic tie break.
`collaborative` runs routine loops autonomously and asks the user only when a
scientific preference, boundary, or external fact can change the route;
`collaborative` and `autonomous` stop at `gate: user`, while `full` bypasses soft gates but never task,
turn, cost, permission, review, or destructive-operation boundaries.

Process completion is separate from scientific outcome. `frame.status=completed`
means the governed research process closed; `scientificOutcome` records whether
the primary objective was supported, partially supported, refuted, inconclusive,
or lacked evidence, while `missionCoverage` records whether the evidence was
sufficient. Unsupported claims never enter the accepted-claim index. The
result-to-claim and whole-research-review artifacts each require two independent
passing reviews and must agree on both fields.

Dispatched tasks retain the stage's acceptance checks and failure signals in
addition to the planner's requirements. Repair obligations close only after the
configured reviews and evidence acceptance; a non-passing review cannot be
overridden by additional passing votes on unchanged evidence. Backtrack reasons
become explicit checks on the replacement task and close when its reviewed
replacement is adopted. Whole-research review receives the current canonical
chain even if the plan omits an upstream result.

The shared durable review boundary requires explicit assessments for every frozen
acceptance and success criterion, exactly once. Missing assessments are never
generated from the overall verdict. It rejects contradictory judgments, invalid
scores, empty rationales or references, and repeated votes from the same reviewer
task on the same evidence. Codex preflight reuses this shared validator. These
checks validate the review contract, not the scientific truth of cited evidence.

`--require-paper` makes `paper-write` and `paper-compile` explicit completion
deliverables. Dynamic routing may skip irrelevant capabilities, but it cannot
skip required artifacts.

论文编译交付需要主机安装 Poppler 的 `pdfinfo`。预检要求可解析且至少一页的 PDF、非空编译日志、可编辑入口 `source`、完整本地输入清单 `buildInputs` 和编译命令；所有输入必须附带文件引用。解析器缺失会明确拒绝提交。预检不代替源码重编译、输入清单完整性审核或页面视觉审核。

Pi 主代理可用 `astra_read_research_object` 按当前研究的对象编号分页读取完整结构化证据、审核和路线记录；提供声明的 `fileRef` 可读取经过哈希核验的冻结 UTF-8 文件，不接受任意路径。二进制材料需要文本提取或页面预览。整体审阅允许在任务预算内检查全部材料和主张，未核验部分必须明确报告。

开放审核问题下，主代理先选择修复当前成果、回退上游或请求用户，不能直接推进或完成。回退后的修复任务使用当前有效上游，旧失败成果只作为明确声明的对照目标；原问题保留到重新审核和验收通过。

Inside Pi interactive mode, `/research-board` shows questions, hypotheses,
claims, objections, candidate scores, budget, and the next decision.
`/research-guide <text>` records user guidance in the same canonical graph, and
`/research-route` shows route lineage and search comparisons.

Global limits are explicit and can be raised when resuming a budget-gated job:

```bash
astra research run --automation full --max-tasks 80 --max-turns 300 --max-cost-usd 20 "target"
astra research resume --max-turns 400
astra research resume --max-cost-usd unlimited
```

`research migrate` only detects legacy `.pmcli` state and writes a read-only
`.astra/migrations/pmcli-import-report.json`; it never promotes `.pmcli` files
to canonical research state. New research state is always owned by `.astra`.

## Official Codex subscription backend

Install the official Codex CLI and sign in using `codex login` with a ChatGPT
account that includes Codex. Astra starts `codex app-server --listen stdio://`;
the official process owns authentication, model requests, tools, and sessions.
Astra does not import subscription tokens or provide an OpenAI-compatible proxy.
API-key-only login is rejected, and inherited API key environment variables are
removed from the child process. Remove any `openai_base_url` override from Codex
configuration: that override bypasses its normal subscription routing.
See the official [authentication guide](https://learn.chatgpt.com/docs/auth) and
[App Server protocol](https://learn.chatgpt.com/docs/app-server).

From an existing development checkout with dependencies available, run the source
launcher directly (these commands do not rebuild installed `astra` artifacts):

```bash
export ASTRA_CODEX_MODEL=gpt-5.6-luna
node packages/astra/src/launcher.ts research run --backend codex \
  --automation collaborative --max-tasks 20 --max-turns 60 "target question"
node packages/astra/src/launcher.ts research status
node packages/astra/src/launcher.ts research pause "operator requested"
node packages/astra/src/launcher.ts research resume --guidance "refine the target"
```

Once packaged, `astra research run --backend codex ...` uses the same entry point.
`ASTRA_CODEX_BIN` optionally selects the official executable. `ASTRA_CODEX_MODEL`
selects the model for all three roles; an explicit model mismatch stops execution
before a turn starts. Real backend validation uses `gpt-5.6-luna`.

Literature retrieval runs in the parent Node process and honors existing
`HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` settings without a Node command-line flag.
Pi and Codex share the same retrieval function: a valid exact-query cache first,
then OpenAlex, Crossref, and arXiv until enough distinct records are available.
Network failures, malformed responses, and exhausted quotas move to the next
channel. Every result reports channel attempts; an empty search is never reported
as scientific evidence. OpenAlex has a separate anonymous daily budget; a working
Codex subscription does not remove it. See
[OpenAlex limits](https://help.openalex.org/api/authentication/),
[Crossref's public API](https://www.crossref.org/documentation/retrieve-metadata/rest-api/),
and the [arXiv API](https://info.arxiv.org/help/api/user-manual.html).

Exact-query caches live for 24 hours, retain their original retrieval date, and
require intact source receipts. Partial or failed searches are not cached as
complete results. Identical concurrent requests share one retrieval; DOI aliases
and arXiv versions are deduplicated. HTTP 429/503 pauses that provider for the
server's `Retry-After`, or five minutes when absent, with the deadline stored in
the job. arXiv requests are serialized at least three seconds apart within one
Astra process; separate concurrent Astra processes need external coordination
to share the same arXiv rate limit.

Codex workers allowed to search papers also receive official live web search.
If metadata retrieval is insufficient, they can search original publication
pages and call `astra_list_sources` for host-recorded identifiers. Only actual
`text_result` entries returned by App Server produce receipts; a model-written
URL or an unsuccessful open-page action cannot qualify. These records explicitly
carry `retrievalLevel=web-search-result`: snippets establish a retrieved lead,
not full-text verification. Reviewers receive the source receipts and judge
whether they support the submitted claims. This uses the official subscription
tool and requires no additional model API key. Main-agent and reviewer sessions
keep web search disabled.

Backend selection is stored in `.astra/jobs/<job>/backend.json`. Status, pause,
tick, and resume use that selection automatically. Start a new job to switch
backends. Existing jobs without the marker are Pi jobs. Codex research commands
return control JSON directly without starting a Pi model session.

The Codex main agent keeps a persistent conversation. Workers use separate
conversations and each independent review starts fresh with a read-only evidence
packet. All roles return schema-constrained results, which Astra validates and
records using the same research contracts as Pi. Candidate search, evidence
validation, review gates, canonical adoption, routing, and user guidance remain
owned by Astra. Literature retrieval is an Astra dynamic tool with source
receipts. Ambient MCP servers and skills are disabled for role sessions; Astra
supplies its packaged role instructions. Worker write access is restricted to
its task workspace and resource directory; main-agent and reviewer access is
read-only through a named Codex permission profile.
On Linux, paper compilation workers also receive read-only access to existing
`/etc/texmf` and `/var/lib/texmf` directories. TeX needs these system configuration,
format, hyphenation, and font-map files; the minimal filesystem alone can leave
an installed `pdflatex` unable to compile. Build outputs and generated caches
remain in the task workspace or resource directory.
All three roles load the stage and role contracts. In particular, reviewers see
the outcome label constraints: detailed justification belongs in companion
claim, conclusion, and missing-evidence fields, not inside machine-readable labels.
Worker submission validates those labels with the same parser used for canonical
adoption, rejecting incompatible values before independent review.
Workers can call `astra_validate_submission` before returning their final JSON.
It uses the same file, field and receipted-source checks as final submission,
returns correctable errors within the existing tool budget, and does not record
evidence or scientific acceptance. Final output is always checked again.
Reviewers similarly use `astra_validate_review`: a non-passing verdict must name
a failed frozen criterion. A valid negative assessment can pass as a report
while its required repairs continue to block research completion.

Before each main-agent decision, Astra refreshes `research-summary.json` with
the active capability, artifact/evidence identifiers, review outcomes, user
guidance, open issues, and completion blockers. The agent uses this index to locate relevant
records instead of repeatedly reading the entire state. Original evidence,
review findings, and graph history remain intact in `research-context.json`
and the read-only job directory; the index is not a substitute for inspecting
the evidence needed for a decision.
Main-agent decisions allow up to ten minutes and 32 tool calls, accommodating
long-running persistent conversations while retaining a bounded failure timeout.

Upstream files retain their original directory structure under
`inputs/<artifact-or-evidence-id>/`, including direct evidence inputs for repair
tasks. Worker artifact citations can use an exact canonical ID declared in both
input lists; submission resolves it to the existing `canonical/<id>.json`
snapshot and applies the same file, workspace, and checksum checks. Unknown IDs
and missing snapshots still fail validation. Review packets include the same input paths, target evidence files, and
retrieved source receipts from every supported channel. Codex review snapshots also
list read-only runtime resource directories for the target task and its declared
inputs, so reviewers can inspect original CSV files, build outputs, and logs.
Unrelated task resources remain inaccessible. Review output schemas enumerate the
allowed evidence references to prevent invented or mistyped citation paths.
Review preparation failures mark the task failed and pause the supervisor before
another attempt, including errors that occur before a model session starts.
File submission and review reject paths that escape
the source workspace through symbolic links. Source minimums count distinct refs.
Codex records retrieved source IDs outside the worker's writable workspace so a
resumed task can cite its earlier retrievals. Downstream tasks can reuse receipts
from their declared upstream evidence; unrelated task sources are rejected.
A rate-limited reviewer resumes its existing task and conversation;
provider backoff stops subsequent phases in the same supervisor tick.

Current limits:

- One worker runs at a time. Subscription quota is shared with other Codex usage;
  exhaustion pauses the job for explicit resume, and transient capacity errors
  use the supervisor's existing backoff.
- Codex does not provide a dollar bill for subscription turns. Finite
  `--max-cost-usd` is rejected; `costAccounting=subscription-unavailable` means
  the graph's zero recorded cost is not a claim of free or unlimited usage.
  Use outer task/turn budgets and per-task time limits.
- Codex owns its internal model iterations and context compaction. Pi extension
  hooks and each task's inner `maxTurns` are not mapped onto Codex internals.
  Tool-call limits are observed from events and interrupt the turn; they cannot
  prevent a native tool that has already started from taking effect.
- The research CLI and local development workbench are available. Pi's interactive
  provider controls have not been ported to the workbench.
  Unexpected approval or interactive requests pause execution.
- The protocol integration uses experimental dynamic tools and named permission
  profiles from Codex CLI 0.153.4. Other versions need verification. The Pi
  fixture provider does not validate Codex execution.
  Codex event logs are stored under the job's `codex-events/` directory.
  They preserve both retryable and terminal server error notifications so a
  timeout can be investigated without losing preceding provider failure details.

`node scripts/audit-astra-run.mjs <workspace> [job-id]` detects the saved backend.
For Codex it checks every recorded decision log, including earlier decisions of
the persistent main thread, against the host's session ledger. Accepted sessions
need a matching official provider and permission profile, a structured final
answer, and a completed turn. Missing or incomplete accepted logs fail the audit.
Intentionally pruned candidate logs are listed separately and require a durable
pruning receipt and task archive. The report lists observed models and historical
failures. Codex recovery uses durable resume/session events; Pi recovery retains
its parent-session checkpoint check. A protocol audit does not independently
establish scientific correctness or PDF quality.

Validation through 2026-09-13: 198 tests in 22 named Astra test files plus the local
workbench integration test passed (199 total); seven existing
audit-script tests, and the root `npm run check` passed. The Codex protocol fixture completes all 14 capabilities
with OpenAlex, with an OpenAlex outage and Crossref fallback, and with all metadata
APIs unavailable and official web-search protocol fixtures. Each path covers
two search rounds, loser pruning, executable multi-file repair and
downstream execution, paper file delivery, two independent reviews at each final
quality gate, and job reopening between ticks. Fixture literature and PDF bytes
exercise the delivery contract; they are not scientific or publication evidence.
See [the review and repair audit](REVIEW_REPAIR_AUDIT.md) for the reproduced
acceptance defects, repairs, and remaining native-delivery checks.

With CLI 0.153.4 and `gpt-5.6-luna`, live checks covered a native file read,
an Astra dynamic tool, structured output, thread resume, and one validation-stage
research loop through independent review, canonical adoption, and a user gate.
The research check resumed after two integration fixes; historical failed review
sessions remain in its audit trail. This does not validate a full experiment or
paper-writing run, and no other operating system or CLI version was tested.

A subsequent native Luna check generated two Python source files with a nested
import, passed them to an isolated downstream task through direct evidence
inputs, executed the original program, and passed independent review against its
saved command/output log. That run exposed Node proxy handling and OpenAlex's
exhausted anonymous budget, now handled by the shared fallback retrieval.

The subsequent live retrieval check injected an OpenAlex outage and retrieved
three real Crossref records through the configured environment proxy. With all
host metadata APIs deliberately unavailable, native Luna web search returned
original arXiv/PMLR sources. The first attempt reached its four-minute smoke-test
budget after saving receipts; an explicit retry of the same task submitted
four source references, passed independent review, adopted
the evidence, reopened the job, and reused all four receipts in a downstream
worker with retrieval disabled. The initial timeout remains in the audit trail.
The native artifacts are bounded metadata/snippet reviews, not full-text studies.

A separate live arXiv API probe timed out on this machine. Atom parsing and
fallback are covered by offline tests, but live arXiv API success and a native
research run through final acceptance remain unverified.

A native fixed-protocol case study reached ten adopted capabilities through
paper compilation. Two real Python runs produced 300 replicate rows and
nine MAE/Monte Carlo SE summaries each; an independent implementation regenerated
the samples and statistics, and the output files were byte-identical across runs.
The repaired result-to-claim artifact passed two independent reviews. The plan,
Chinese manuscript, and a real two-page PDF also passed their stage reviews.
The compilation run exposed missing read access to installed TeX configuration;
an official-sandbox probe reproduced the failure and compiled successfully with
the two read-only system directories described above.
Visual inspection and a word-boundary check then found clipped text in the PDF,
despite successful compilation and extraction. The whole-research assessment
correctly reports a blocked delivery, including outdated manuscript status text;
its independent verification has encountered stream disconnects and timeouts.
The PDF remains a draft requiring repair, editable source delivery, recompilation,
and final review. This incremental run is not a completed end-to-end success.

The pre-Pi Cargo crate is archived under `legacy/rust/` for migration forensics
and parity checks. It is outside the default Pi build and release path and is
not a second Astra product entry point.

For an offline, deterministic end-to-end audit, use the built-in fixture
provider. It still launches real Pi JSON child processes and writes Pi session
JSONL files:

```bash
ASTRA_FIXTURE_PROVIDER=1 ASTRA_MAX_TICKS=100 \
  astra research run --automation full "offline auto research audit"
```

The audit succeeds only when runtime integrity and research quality both pass:
the event sequence is continuous, one persistent main-agent controls an
explicit route, every canonical artifact has a passing review, selected search
candidates have criterion-level comparisons, loser workspaces are pruned,
claim assessments agree with the scientific outcome, the whole-research review is positive, and no
blocking objection remains. Pass the parent Pi session directory to audit the
control composition root as well:

```bash
node scripts/audit-astra-run.mjs \
  /path/to/workspace \
  job_id \
  /path/to/pi-parent-sessions
```

The v4 report separates `runtimeIntegrity`, `researchQuality`,
`scientificResult`, and `unresolvedUncertainty`. It must contain one matching parent session whose
`astra_research_result` has the audited `jobId` and either `action=run` or, for
a recovered job, `action=resume`. It must also report
`sessions.unrecoveredFailedFiles=0`; assistant turns ending in `error` or
`aborted` always remain visible in `failedFiles`. A failed persistent session is
recovered only when a later assistant turn succeeds and its durable session
record is completed. Recovery acceptance additionally requires the matching
`resume` parent session to contain an Astra checkpoint. Real Pi compaction entries
are reported when present, but a small control-only session has nothing to compact.
