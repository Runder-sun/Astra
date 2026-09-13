const el = (id) => document.getElementById(id);
const names = { validation: "问题确认", literature: "文献检索", idea: "候选想法", novelty: "查新", refine: "方法细化", "experiment-plan": "实验计划", "implement-solution": "代码实现", run: "实验运行", monitor: "运行检查", "result-to-claim": "结果与主张", "paper-plan": "论文规划", "paper-write": "论文写作", "paper-compile": "论文编译", "research-review": "整体审阅" };
const outcomes = { pending: "尚未评估", supported: "得到支持", "partially-supported": "部分支持", refuted: "被结果反驳", inconclusive: "尚无定论", "insufficient-evidence": "证据不足", sufficient: "覆盖充分", insufficient: "覆盖不足" };
let token = "";
let selected = "";
let stageId = "";
let stages = [];
let current;
let creating = false;
let fetching = false;
let renderedVersion = "";
function node(tag, text, className) { const item = document.createElement(tag); if (text !== undefined) item.textContent = text; if (className) item.className = className; return item; }
function error(message) { el("error").textContent = message; el("error").hidden = !message; }
async function api(path, input) {
	const response = await fetch(path, input === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json", "X-Astra-Token": token }, body: JSON.stringify(input) });
	const data = await response.json(); if (!response.ok) throw new Error(data.error || "请求失败"); return data;
}
function status(job) { if (job.error) return "需要检查"; if (job.paused) return "已暂停 / 等待确认"; if (job.frame?.status === "completed") return "流程已完成"; if (job.running) return "正在执行"; return job.readonly ? "外部任务 · 只读" : "执行进程未连接"; }
function renderDetails() {
	const state = current?.snapshot;
	if (!state) return;
	const definition = stages.find(stage => stage.id === stageId);
	el("stage-title").textContent = names[stageId] || stageId;
	el("gate").textContent = `至少 ${definition?.qualityPolicy?.minPassingReviews || 1} 次通过 · ≥ 0.8`;
	const panel = el("stage-detail"); panel.replaceChildren();
	const evidence = Object.values(state.evidence).filter(item => item.stageId === stageId).sort((a,b) => b.createdAt.localeCompare(a.createdAt))[0];
	const task = evidence && state.tasks[evidence.taskId];
	const reviews = evidence ? Object.values(state.reviews).filter(review => review.evidenceId === evidence.id).sort((a,b) => b.createdAt.localeCompare(a.createdAt)) : [];
	const review = reviews[0];
	const checks = [...new Set([...(task?.acceptanceChecks || definition?.acceptanceChecks || []), ...(task?.successCriteria || [])])];
	panel.append(node("p", evidence ? `当前候选：已收到 ${reviews.length} 份审阅。${state.canonicalRoute.stageArtifactIds[stageId] ? "本阶段已有正式成果。" : "当前阶段尚未采用正式成果。"}` : "该阶段尚无候选成果。以下为默认要求，执行时会加入本任务的具体标准。", "muted"));
	panel.append(node("h3", "逐项验收 · 最新一份审阅"));
	for (const check of checks) {
		const result = review?.criteria?.find(item => item.criterion === check);
		const row = node("div", undefined, "check");
		row.append(node("strong", `${result ? result.passed ? "通过 · " : "待修复 · " : "待检查 · "}${check}`, result ? result.passed ? "good" : "bad" : ""));
		if (result) { row.append(node("p", result.rationale)); const refs = node("details"); refs.append(node("summary", `查看 ${result.evidenceRefs.length} 条证据引用`), node("p", result.evidenceRefs.join(" · "))); row.append(refs); }
		panel.append(row);
	}
	const obligations = Object.values(state.obligations).filter(item => item.status === "open" && state.evidence[state.reviews[item.sourceReviewId]?.evidenceId]?.stageId === stageId);
	if (obligations.length) { panel.append(node("h3", "尚未关闭的修复问题")); for (const issue of obligations) { const details = node("details", undefined, "finding warning"); details.append(node("summary", issue.description.length > 120 ? `${issue.description.slice(0,120)}…` : issue.description), node("p", issue.description)); panel.append(details); } }
	if (reviews.length) {
		const history = node("details"); history.append(node("summary", `查看 ${reviews.length} 份独立审阅记录`));
		for (const item of reviews) { history.append(node("p", `${item.createdAt} · ${item.verdict === "pass" ? "通过" : "未通过"} · ${item.score}`)); for (const finding of item.findings) history.append(node("p", finding, "muted")); }
		panel.append(history);
	}
	const artifact = state.canonical[state.canonicalRoute.stageArtifactIds[stageId]];
	if (artifact) {
		panel.append(node("h3", "正式交付"));
		for (const ref of state.evidence[artifact.evidenceId]?.refs || []) {
			if (!ref.startsWith(".astra/")) continue;
			const link = node("a", ref.split("/").pop()); link.href = `/api/file?${new URLSearchParams({id: selected, artifact: artifact.id, ref})}`; link.target = "_blank"; link.rel = "noopener";
			const row = node("p"); row.append(link); panel.append(row);
		}
		const details = node("details"); details.append(node("summary", "查看成果内容与编号"), node("p", artifact.id), node("pre", JSON.stringify(artifact.content, null, 2))); panel.append(details);
	}
}
function renderJob() {
	const state = current.snapshot;
	el("research").hidden = creating;
	if (!state) { el("title").textContent = current.error ? "研究未能启动" : "正在创建研究任务…"; for (const id of ["stage-detail", "stages", "full-objective", "next-action", "usage", "directory"]) el(id).replaceChildren(); for (const id of ["execution", "outcome", "coverage"]) el(id).textContent = "等待初始化"; el("continue").hidden = true; el("pause").hidden = true; el("output").textContent = current.output || ""; if (current.error) error(current.error); return; }
	el("title").textContent = state.frame.objective.length > 120 ? `${state.frame.objective.slice(0,120)}…` : state.frame.objective;
	el("full-objective").textContent = state.frame.objective;
	el("job-label").textContent = current.readonly ? "已有研究 · 只读查看" : "本机研究";
	el("execution").textContent = status({ ...current, frame: state.frame, paused: state.paused });
	el("outcome").textContent = outcomes[state.frame.scientificOutcome] || state.frame.scientificOutcome;
	el("coverage").textContent = outcomes[state.frame.missionCoverage] || state.frame.missionCoverage;
	const active = Object.values(state.sessions).filter(session => session.status === "running").at(-1);
	el("next-action").textContent = state.paused ? state.frame.userGate?.question || state.frame.userGate?.reason || state.frame.nextAction : active ? `${names[state.frame.activeStageId]}：${active.role === "reviewer" ? "正在独立审阅证据" : active.role === "worker" ? "正在执行任务" : "正在规划下一步"}` : state.frame.nextAction;
	el("usage").textContent = `已创建 ${Object.keys(state.tasks).length} / ${state.frame.budget.maxTasks} 个任务 · 已用 ${state.budgetUsage?.turnsUsed || 0} / ${state.frame.budget.maxTurns} 轮 · ${state.frame.openObligationIds.length} 项待修复问题`;
	el("continue").hidden = current.readonly || !state.paused || current.running;
	el("pause").hidden = current.readonly || !current.running;
	el("directory").textContent = current.root;
	el("output").textContent = current.error ? `${current.error}\n${current.output}` : current.output || "尚无进程输出；执行详情以阶段状态和审阅记录为准。";
	if (!stageId) stageId = state.frame.activeStageId;
	el("stages").replaceChildren();
	for (const definition of stages) {
		const button = node("button", undefined, `stage${stageId === definition.id ? " active" : ""}`); button.type = "button";
		button.append(node("span", names[definition.id] || definition.id));
		const adopted = state.canonicalRoute.stageArtifactIds[definition.id];
		button.append(node("small", adopted ? "已采用" : state.frame.activeStageId === definition.id ? "当前" : "未采用"));
		button.onclick = () => { stageId = definition.id; renderJob(); }; el("stages").append(button);
	}
	renderDetails();
}
async function refresh() {
	if (fetching) return; fetching = true;
	try {
		const data = await api("/api/jobs"); token = data.token; stages = data.stages;
		el("jobs").replaceChildren();
		if (!selected && data.jobs.length) selected = data.jobs[0].id;
		for (const job of data.jobs) {
			const button = node("button", undefined, selected === job.id && !creating ? "active" : ""); button.type = "button";
			const title = job.frame?.objective || "新研究 · 正在初始化";
			button.append(node("span", title.length > 50 ? `${title.slice(0,50)}…` : title), node("small", status(job)));
			button.onclick = () => { selected = job.id; stageId = ""; creating = false; el("create").hidden = true; void refresh(); }; el("jobs").append(button);
		}
		if (!data.jobs.length) { el("jobs").append(node("p", "还没有研究任务", "muted")); creating = true; el("create").hidden = false; }
		if (selected && !creating) { current = await api(`/api/job?id=${selected}`); const version = `${selected}:${current.snapshot?.eventSeq}:${current.running}:${current.error}`; if (version !== renderedVersion || el("research").hidden) { renderJob(); renderedVersion = version; } }
		el("connection").textContent = `已同步 ${new Date().toLocaleTimeString("zh-CN")}`;
	} catch (err) { error(err.message); el("connection").textContent = "连接中断 · 保留上次数据"; }
	finally { fetching = false; }
}
el("new").onclick = () => { creating = true; el("create").hidden = false; el("research").hidden = true; el("objective").focus(); };
el("example").onclick = () => { el("objective").value = "做一个固定范围的小规模复现实验：比较样本均值、中位数与两端各截去 10 个值的截尾均值估计真实位置 0 的误差。样本量 101，标准正态数据，污染比例 0、0.1、0.2，将前 floor(101×污染比例) 个值加 10，每种条件重复 100 次。使用固定种子并记录精确协议，仅用 Python 标准库和本机 CPU。保留可执行代码、行为测试、逐次数据、MAE 与蒙特卡洛标准误、命令和失败日志，并独立核对结果。检索至少三条可追溯文献。只作固定协议下的描述性结论，不声称创新或普遍优越；最后给出经过审阅的研究报告。"; };
el("create-form").onsubmit = async event => { event.preventDefault(); error(""); el("start").disabled = true; try { const data = await api("/api/run", { objective: el("objective").value, maxTasks: Number(el("budget").value), requirePaper: el("paper").checked }); selected = data.id; stageId = ""; creating = false; el("create").hidden = true; await refresh(); } catch (err) { error(err.message); } finally { el("start").disabled = false; } };
el("resume").onclick = async () => { error(""); el("resume").disabled = true; try { await api(`/api/resume?id=${selected}`, { guidance: el("guidance").value, ...(el("resume-budget").value ? { maxTasks: Number(el("resume-budget").value) } : {}) }); el("guidance").value = ""; await refresh(); } catch (err) { error(err.message); } finally { el("resume").disabled = false; } };
el("pause").onclick = async () => { el("pause").disabled = true; try { await api(`/api/pause?id=${selected}`, {}); el("next-action").textContent = "已请求暂停，正在保存任务状态…"; } catch (err) { error(err.message); } finally { el("pause").disabled = false; } };
el("refresh").onclick = refresh;
el("theme").onclick = () => { document.documentElement.dataset.theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark"; };
void refresh();
setInterval(refresh, 5000);
