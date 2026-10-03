import type { AssistantMessageEventStream, Message, Model, UserMessage } from "@earendil-works/pi-ai";
import { describe, expect, it } from "vitest";
import { Agent } from "../src/agent.ts";
import { agentLoop, runAgentLoop } from "../src/agent-loop.ts";
import type { AgentEvent, AgentLoopConfig, AgentMessage } from "../src/types.ts";

const model: Model<"openai-responses"> = {
	id: "offline-abort",
	name: "offline-abort",
	api: "openai-responses",
	provider: "openai",
	baseUrl: "https://example.invalid",
	reasoning: false,
	input: ["text"],
	cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
	contextWindow: 8192,
	maxTokens: 2048,
};
const prompt: UserMessage = { role: "user", content: "offline cancellation", timestamp: 1 };

function convert(messages: AgentMessage[]): Message[] {
	return messages.filter((message) => ["user", "assistant", "toolResult"].includes(message.role)) as Message[];
}

function assertLifecycle(events: AgentEvent[]): void {
	expect(events.filter((event) => event.type === "agent_end")).toHaveLength(1);
	expect(events.filter((event) => event.type === "turn_start")).toHaveLength(1);
	expect(events.filter((event) => event.type === "turn_end")).toHaveLength(1);
	expect(events.filter((event) => event.type === "message_start")).toHaveLength(2);
	expect(events.filter((event) => event.type === "message_end")).toHaveLength(2);
	expect(events.at(-1)?.type).toBe("agent_end");
}

describe("AB01/AB02 provider cancellation boundary", () => {
	for (const lifecycle of ["stream", "Agent"] as const) {
		it.each(["pre-aborted", "transformContext", "convertToLlm", "getApiKey"] as const)(
			`${lifecycle} settles cancellation at %s without entering the provider`,
			async (boundary) => {
				const controller = new AbortController();
				let providerCalls = 0;
				let keyCalls = 0;
				let entered = () => {};
				let release = () => {};
				const started = new Promise<void>((resolve) => {
					entered = resolve;
				});
				const held = new Promise<void>((resolve) => {
					release = resolve;
				});
				const config: AgentLoopConfig = {
					model,
					convertToLlm: convert,
					getApiKey: () => {
						keyCalls++;
						return "offline-key";
					},
				};
				if (boundary === "transformContext") {
					config.transformContext = async (messages) => {
						entered();
						await held;
						return messages;
					};
				} else if (boundary === "convertToLlm") {
					config.convertToLlm = async (messages) => {
						entered();
						await held;
						return convert(messages);
					};
				} else if (boundary === "getApiKey") {
					config.getApiKey = async () => {
						keyCalls++;
						entered();
						await held;
						return "offline-key";
					};
				}
				const provider = (): AssistantMessageEventStream => {
					providerCalls++;
					throw new Error("cancelled provider must not be entered");
				};
				const events: AgentEvent[] = [];
				if (lifecycle === "stream") {
					if (boundary === "pre-aborted") controller.abort();
					const stream = agentLoop(
						[prompt],
						{ systemPrompt: "", messages: [], tools: [] },
						config,
						controller.signal,
						provider,
					);
					const iteration = (async () => {
						for await (const event of stream) events.push(event);
					})();
					if (boundary !== "pre-aborted") {
						await started;
						controller.abort();
						release();
					}
					const [messages] = await Promise.all([stream.result(), iteration]);
					expect(messages.at(-1)).toMatchObject({ role: "assistant", stopReason: "aborted" });
				} else {
					const agent = new Agent({
						initialState: { model },
						streamFn: provider,
						convertToLlm: config.convertToLlm,
						transformContext: config.transformContext,
						getApiKey: config.getApiKey,
					});
					agent.subscribe((event) => {
						events.push(event);
						if (boundary === "pre-aborted" && event.type === "agent_start") agent.abort();
					});
					const running = agent.prompt(prompt);
					const idle = agent.waitForIdle();
					if (boundary !== "pre-aborted") {
						await started;
						agent.abort();
						release();
					}
					await Promise.all([running, idle]);
					expect(agent.state.messages.at(-1)).toMatchObject({ role: "assistant", stopReason: "aborted" });
					expect(agent.state.isStreaming).toBe(false);
					expect(agent.state.streamingMessage).toBeUndefined();
					expect(agent.state.pendingToolCalls.size).toBe(0);
					expect(agent.signal).toBeUndefined();
				}
				assertLifecycle(events);
				expect(providerCalls).toBe(0);
				if (boundary === "pre-aborted") expect(keyCalls).toBe(0);
			},
		);
	}

	it("AC06 preserves low-level runAgentLoop rejection for a pre-aborted signal", async () => {
		const controller = new AbortController();
		controller.abort();
		let providerCalls = 0;
		await expect(
			runAgentLoop(
				[prompt],
				{ systemPrompt: "", messages: [], tools: [] },
				{ model, convertToLlm: convert },
				() => {},
				controller.signal,
				() => {
					providerCalls++;
					throw new Error("unexpected provider");
				},
			),
		).rejects.toMatchObject({ name: "AbortError" });
		expect(providerCalls).toBe(0);
	});
});
