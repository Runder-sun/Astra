import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "@earendil-works/pi-agent-core";
import type { Provider } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createAstraFixtureProvider } from "../src/fixture-provider.ts";

function registeredFixtureProvider(): Provider {
	let provider: Provider | undefined;
	createAstraFixtureProvider()({
		registerProvider(value: Provider) {
			provider = value;
		},
	} as unknown as ExtensionAPI);
	if (!provider) throw new Error("fixture provider was not registered");
	return provider;
}

function createAgent(
	provider: Provider,
	onToolCall: (toolName: string, params: unknown) => void,
	toolNames = ["astra_submit_worker_output"],
): Agent {
	const model = provider.getModels()[0];
	if (!model) throw new Error("fixture provider has no model");
	return new Agent({
		streamFn: provider.streamSimple,
		initialState: {
			model,
			systemPrompt: "Astra fixture provider regression test",
			tools: toolNames.map((toolName) => ({
				name: toolName,
				label: "Submit worker output",
				description: "Fixture tool",
				parameters: Type.Object({}),
				execute: async (_toolCallId, params) => {
					onToolCall(toolName, params);
					return { content: [{ type: "text" as const, text: "submitted" }], details: {} };
				},
			})),
		},
	});
}

const tempRoots: string[] = [];

afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(tempRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("Astra fixture provider", () => {
	it("keeps ordinary parent prompts out of child-role tool paths", async () => {
		let toolCalls = 0;
		const agent = createAgent(registeredFixtureProvider(), () => toolCalls++);

		await agent.prompt("ordinary parent prompt");

		expect(toolCalls).toBe(0);
		expect(agent.state.messages.at(-1)).toMatchObject({
			role: "assistant",
			stopReason: "stop",
			content: [{ type: "text", text: "Astra fixture has no operation to perform." }],
		});
	});

	it("supports the multiple parent turns required before compaction", async () => {
		const agent = createAgent(registeredFixtureProvider(), () => {
			throw new Error("ordinary parent prompts must not call child tools");
		});

		await agent.prompt("first parent turn");
		await agent.prompt("second parent turn");
		await agent.prompt("third parent turn");

		expect(agent.state.messages.at(-1)).toMatchObject({ role: "assistant", stopReason: "stop" });
	});

	it("finishes the assistant turn after a worker tool submission", async () => {
		vi.stubEnv("ASTRA_ROLE", "worker");
		let toolCalls = 0;
		const agent = createAgent(registeredFixtureProvider(), () => toolCalls++);

		await agent.prompt("worker prompt");

		expect(toolCalls).toBe(1);
		expect(agent.state.messages.at(-1)).toMatchObject({
			role: "assistant",
			stopReason: "stop",
			content: [{ type: "text", text: "Astra fixture completed the requested operation." }],
		});
	});

	it("uses the decision tool for a main-agent evidence decision session", async () => {
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_STAGE_PLAN_ID", "");
		vi.stubEnv("ASTRA_DECISION_TYPE", "evidence");
		vi.stubEnv("ASTRA_EVIDENCE_ID", "evidence-1");
		const toolCalls: string[] = [];
		const agent = createAgent(registeredFixtureProvider(), (toolName) => toolCalls.push(toolName), [
			"astra_submit_stage_plan",
			"astra_submit_main_decision",
		]);

		await agent.prompt("decide evidence");

		expect(toolCalls).toEqual(["astra_submit_main_decision"]);
	});

	it("continues the first fixture search round once and selects in the final round", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-fixture-search-"));
		tempRoots.push(root);
		const jobId = "job_fixture_search";
		await mkdir(join(root, ".astra", "jobs", jobId), { recursive: true });
		vi.stubEnv("ASTRA_ROLE", "main-agent");
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", jobId);
		vi.stubEnv("ASTRA_STAGE_PLAN_ID", "");
		vi.stubEnv("ASTRA_DECISION_TYPE", "search-selection");
		vi.stubEnv("ASTRA_DECISION_REF", "search-fixture-decision");
		vi.stubEnv("ASTRA_SEARCH_BATCH_ID", "search-fixture-batch");
		vi.stubEnv("ASTRA_CANDIDATE_IDS", JSON.stringify(["candidate-a", "candidate-b"]));
		vi.stubEnv("ASTRA_SEARCH_ALLOW_CONTINUE", "1");
		vi.stubEnv("ASTRA_FIXTURE_CONTINUE_SEARCH_ONCE", "1");
		const submissions: Array<Record<string, unknown>> = [];
		const runDecision = async (): Promise<void> => {
			const agent = createAgent(
				registeredFixtureProvider(),
				(_toolName, params) => submissions.push(params as Record<string, unknown>),
				["astra_submit_main_decision"],
			);
			await agent.prompt("decide search");
		};

		await runDecision();
		await runDecision();

		expect(submissions[0]).toMatchObject({ continueSearch: true });
		expect(submissions[0]).not.toHaveProperty("selectedCandidateId");
		expect(submissions[1]).toMatchObject({ selectedCandidateId: "candidate-a" });
		expect(submissions[1]).not.toHaveProperty("continueSearch");
	});

	it("searches papers in source-backed worker stages", async () => {
		vi.stubEnv("ASTRA_ROLE", "worker");
		vi.stubEnv("ASTRA_STAGE_ID", "literature");
		const toolCalls: string[] = [];
		const agent = createAgent(registeredFixtureProvider(), (toolName) => toolCalls.push(toolName), [
			"astra_search_papers",
			"astra_submit_worker_output",
		]);

		await agent.prompt("literature worker prompt");

		expect(new Set(toolCalls)).toEqual(new Set(["astra_search_papers", "astra_submit_worker_output"]));
	});
});
