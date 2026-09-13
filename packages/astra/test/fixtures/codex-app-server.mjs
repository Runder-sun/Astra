import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { fullResearchOutput } from "./codex-full-research.mjs";

const mode = process.env.ASTRA_FAKE_CODEX_MODE;
let threadId;
let output;
let turnId;
const send = message => process.stdout.write(`${JSON.stringify(message)}\n`);
function complete() {
	const payload = mode === "invalid-output" ? "not JSON" : JSON.stringify(output);
	send({ method: "item/completed", params: { threadId: "other-thread", item: { type: "agentMessage", phase: "final_answer", text: "wrong output" } } });
	send({ method: "item/completed", params: { threadId, item: { type: "agentMessage", phase: "final_answer", text: payload } } });
	send({ method: "turn/completed", params: { threadId, turn: { id: turnId, status: "completed", error: null } } });
}
createInterface({ input: process.stdin }).on("line", line => {
	const message = JSON.parse(line);
	if (process.env.ASTRA_FAKE_CODEX_LOG) appendFileSync(process.env.ASTRA_FAKE_CODEX_LOG, `${line}\n`);
	if (message.id === "validate-review") {
		if (message.result?.success || !message.result?.contentItems[0].text.includes("failed frozen criterion")) throw new Error("Expected contradictory-verdict feedback");
		output.verdict = "pass";
		output.score = 1;
		send({ method: "item/started", params: { threadId, item: { id: "validate-review-corrected", type: "dynamicToolCall" } } });
		send({ id: "validate-review-corrected", method: "item/tool/call", params: { threadId, tool: "astra_validate_review", arguments: output } });
		return;
	}
	if (message.id === "validate-review-corrected") {
		if (!message.result?.success) throw new Error("Expected valid negative-assessment review");
		complete();
		return;
	}
	if (message.id === "validate-empty" || message.id === "validate-corrected") {
		if (message.id === "validate-empty") {
			if (message.result?.success || !message.result?.contentItems[0].text.includes("received 0")) throw new Error("Expected missing-source feedback");
			send({ method: "item/started", params: { threadId, item: { id: "validate-corrected", type: "dynamicToolCall" } } });
			send({ id: "validate-corrected", method: "item/tool/call", params: { threadId, tool: "astra_validate_submission", arguments: output } });
		} else {
			if (!message.result?.success || !JSON.parse(message.result.contentItems[0].text).valid) throw new Error("Expected corrected submission to validate");
			if (process.env.ASTRA_FAKE_CODEX_INVALID_FINAL === "1") output.contentJson = "{}";
			complete();
		}
		return;
	}
	if (message.id === "list-sources") {
		if (!message.result?.success) throw new Error("Fixture source listing failed");
		const sources = JSON.parse(message.result.contentItems[0].text).results;
		output.refs.push(...sources.map(source => ({ kind: "source", ref: source.sourceRef, summary: source.title })));
		complete();
		return;
	}
	if (message.id === "tool-call") {
		if (mode === "submission-preflight") {
			if (!message.result?.success) throw new Error("Fixture search failed");
			output.refs = JSON.parse(message.result.contentItems[0].text).results.map(source => ({ kind: "source", ref: source.sourceRef, summary: source.title }));
			send({ method: "item/started", params: { threadId, item: { id: "validate-empty", type: "dynamicToolCall" } } });
			send({ id: "validate-empty", method: "item/tool/call", params: { threadId, tool: "astra_validate_submission", arguments: { ...output, refs: [] } } });
			return;
		}
		if (mode === "full-research") {
			if (!message.result?.success) throw new Error("Fixture literature retrieval failed");
			const search = JSON.parse(message.result.contentItems[0].text);
			if (process.env.ASTRA_FAKE_CODEX_WEB === "1") {
				if (search.results.length) throw new Error("Expected all metadata providers to be unavailable");
				const item = { id: "web-search-1", type: "webSearch", query: "fixture research", action: { type: "search", query: "fixture research", queries: null }, results: [1, 2, 3].map(index => ({ type: "text_result", title: `Fixture paper ${index}`, url: `https://arxiv.org/abs/2301.0000${index}`, snippet: "Offline protocol fixture, not research evidence" })) };
				send({ method: "item/started", params: { threadId, item } });
				send({ method: "item/completed", params: { threadId, item } });
				send({ method: "item/started", params: { threadId, item: { id: "list", type: "dynamicToolCall" } } });
				send({ id: "list-sources", method: "item/tool/call", params: { threadId, tool: "astra_list_sources", arguments: {} } });
				return;
			}
			output.refs.push(...search.results.map(source => ({ kind: "source", ref: source.sourceRef, summary: source.title })));
		}
		if (mode === "literature-interrupt") send({ method: "turn/completed", params: { threadId, turn: { id: turnId, status: "failed", error: { message: "test provider refusal", codexErrorInfo: "rateLimitExceeded" } } } });
		else complete();
		return;
	}
	if (message.method === "initialize") send({ id: message.id, result: { userAgent: "codex-test" } });
	if (message.method === "account/read") send({ id: message.id, result: { account: mode === "logged-out" ? null : { type: mode === "api-key" ? "apiKey" : "chatgpt", planType: "pro" } } });
	if (message.method === "config/read") send({ id: message.id, result: { config: { openai_base_url: mode === "api-url" ? "https://api.openai.com/v1" : null, mcp_servers: { ambient: { command: "must-not-launch" } } } } });
	if (message.method === "skills/list") send({ id: message.id, result: { data: [{ cwd: process.cwd(), skills: [{ path: "/ambient/SKILL.md", enabled: true }], errors: [] }] } });
	if (["thread/start", "thread/resume"].includes(message.method)) {
		threadId = message.params.threadId ?? `test-${Date.now()}-${Math.random()}`;
		send({ id: message.id, result: { thread: { id: threadId }, model: mode === "wrong-model" ? "different-model" : message.params.model ?? "test-model", modelProvider: "openai", activePermissionProfile: { id: mode === "wrong-permissions" ? ":danger-full-access" : "astra" } } });
	}
	if (message.method === "turn/start") {
		turnId = `turn-${Date.now()}`;
		output = JSON.parse(process.env.ASTRA_FAKE_CODEX_OUTPUT ?? '{"answer":"ok"}');
		if (mode === "environment") output = { answer: ["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL"].some(key => process.env[key]) ? "leaked" : "ok" };
		if (["research", "literature-interrupt", "literature-resume", "submission-preflight", "review-preflight", "review-contradiction"].includes(mode)) {
			const fields = message.params.outputSchema.properties;
			if (fields.tasks) {
				const context = JSON.parse(readFileSync("research-context.json", "utf8"));
				const stage = context.capabilities[context.state.frame.activeStageId];
				output = { tasks: [{ key: "candidate", objective: "Bound the research question", inputArtifactRefs: [], requiredOutputFields: stage.requiredOutputFields, acceptanceChecks: stage.acceptanceChecks, failureSignals: stage.failureSignals, successCriteria: stage.acceptanceChecks, hypothesis: "A bounded test is possible" }], rationale: "Evaluate a bounded question first" };
			} else if (fields.contentJson) {
				const context = JSON.parse(readFileSync("ASTRA_TASK_CONTEXT.json", "utf8"));
				output = { artifactType: context.task.requiredOutputType, contentJson: JSON.stringify(Object.fromEntries(context.task.requiredOutputFields.map(field => [field, `fixture ${field}`]))), refs: [] };
				if (mode === "literature-resume") output.refs = [1, 2, 3].map(index => ({ kind: "source", ref: `openalex:W${index}`, summary: "Previously retrieved fixture paper" }));
			} else if (fields.verdict) {
				const packet = JSON.parse(readFileSync("review-packet.json", "utf8"));
				const refs = [`evidence:${packet.evidenceId}`, "review-packet.json", "review-target-snapshot.json"];
				if (process.env.ASTRA_FAKE_CODEX_FOREIGN_REVIEW_REF === "1") refs.push("../other-review/private.json");
				output = { verdict: "pass", score: 1, findings: ["Fixture contract is complete"], verifiedRefs: refs, criteria: [...new Set([...packet.workerContract.acceptanceChecks, ...packet.workerContract.successCriteria])].map(criterion => ({ criterion, passed: true, score: 1, evidenceRefs: refs, rationale: "Verified fixture" })) };
				if (process.env.ASTRA_FAKE_CODEX_PARAPHRASE === "1") output.criteria[0].criterion = "paraphrased criterion";
				if (mode === "review-preflight" || mode === "review-contradiction") {
					output.verdict = "fail";
					output.score = 0.68;
				}
			} else if (fields.decision) output = { decision: "accept", rationale: "Reviewed evidence" };
			else if (fields.adopt) output = { adopt: true, rationale: "Adopt reviewed evidence" };
			else if (fields.routeAction) {
				const context = JSON.parse(readFileSync("research-context.json", "utf8"));
				output = { routeAction: "ask-user", targetStageId: null, question: "Which benchmark should define the scientific scope?", newQuestions: [], evidenceRefs: Object.keys(context.state.canonical), rationale: "The benchmark choice changes the research scope" };
			}
		}
		if (process.env.ASTRA_FAKE_CODEX_OUTPUTS) {
			const path = process.env.ASTRA_FAKE_CODEX_OUTPUTS;
			const queue = JSON.parse(readFileSync(path, "utf8"));
			output = queue.shift();
			writeFileSync(path, JSON.stringify(queue));
		}
		if (mode === "full-research") output = fullResearchOutput(message.params.outputSchema);
		send({ id: message.id, result: { turn: { id: turnId, status: "inProgress" } } });
		send({ method: "turn/started", params: { threadId, turn: { id: turnId } } });
		if (mode === "review-preflight") {
			send({ method: "item/started", params: { threadId, item: { id: "validate-review", type: "dynamicToolCall" } } });
			send({ id: "validate-review", method: "item/tool/call", params: { threadId, tool: "astra_validate_review", arguments: output } });
			return;
		}
		if (mode === "submission-preflight" || mode === "literature-interrupt" || (mode === "full-research" && message.params.outputSchema.properties.contentJson && ["literature", "novelty", "paper-write", "research-review"].includes(output.artifactType))) {
			send({ method: "item/started", params: { threadId, item: { id: "search", type: "dynamicToolCall" } } });
			send({ id: "tool-call", method: "item/tool/call", params: { threadId, tool: "astra_search_literature", arguments: { query: "fixture research", limit: 3 } } });
			return;
		}
		if (mode === "timeout") return;
		if (["retry-error", "fatal-error"].includes(mode)) {
			const params = { threadId, turnId, willRetry: mode === "retry-error", error: { message: "fixture stream failure", codexErrorInfo: "streamDisconnected" } };
			send({ method: "error", params: { ...params, threadId: "other-thread" } });
			send({ method: "error", params });
			if (mode === "fatal-error") return;
		}
		if (mode === "web-search") {
			const item = { id: "web-1", type: "webSearch", query: "robust estimation", action: { type: "openPage", url: "https://arxiv.org/abs/2301.00001" }, results: null };
			send({ method: "item/completed", params: { threadId: "other-thread", item } });
			send({ method: "item/started", params: { threadId, item } });
			send({ method: "item/completed", params: { threadId, item } });
		}
		if (["quota", "rate-limit"].includes(mode)) {
			send({ method: "turn/completed", params: { threadId, turn: { id: turnId, status: "failed", error: { message: "test provider refusal", codexErrorInfo: mode === "quota" ? "usageLimitExceeded" : "rateLimitExceeded" } } } });
			return;
		}
		if (mode === "approval") {
			send({ id: "approval-call", method: "item/commandExecution/requestApproval", params: { threadId, turnId } });
			return;
		}
		if (mode === "tool" || mode === "bad-tool") {
			send({ method: "item/started", params: { threadId, item: { id: "tool-1", type: "dynamicToolCall" } } });
			send({ id: "tool-call", method: "item/tool/call", params: { threadId, tool: "audit_tool", arguments: { query: mode === "bad-tool" ? 5 : "test" } } });
			return;
		}
		if (mode === "tool-budget") {
			for (let i = 0; i < 3; i++) send({ method: "item/started", params: { threadId, item: { id: `command-${i}`, type: "commandExecution" } } });
			return;
		}
		complete();
	}
});
