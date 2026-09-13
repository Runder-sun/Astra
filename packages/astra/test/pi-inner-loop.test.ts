import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "@earendil-works/pi-agent-core";
// Use the built Pi core packages directly. This bypasses the coding-agent model
// catalog so the fixture exercises the actual provider -> tool -> next-turn loop.
import { createFauxCore, fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai/providers/faux";
import { Type } from "typebox";
import { describe, expect, it } from "vitest";
import { PiChildSessionRunner, providerErrorFromJsonEvents } from "../src/pi-child-session.ts";

describe("Pi inner loop fixture", () => {
	it.each([
		"OpenAI API error (401): invalid_api_key",
		"Provider request failed with HTTP 403",
		"Authentication failure for the selected provider",
		"No API key found for anthropic",
		"Model openai/not-configured was not found",
	])("classifies permanent provider configuration errors: %s", (errorMessage) => {
		expect(
			providerErrorFromJsonEvents([
				{
					type: "message_end",
					message: { role: "assistant", stopReason: "error", errorMessage },
				},
			]),
		).toEqual({ kind: "configuration", message: errorMessage });
	});

	it.each([
		"Provider request failed with HTTP 429",
		"Upstream rate limit exceeded, please retry later",
		"Our servers are currently overloaded. Please try again later.",
		"Provider returned 503 temporarily_unavailable",
	])("classifies transient provider capacity errors: %s", (errorMessage) => {
		expect(
			providerErrorFromJsonEvents([
				{
					type: "message_end",
					message: { role: "assistant", stopReason: "error", errorMessage },
				},
			]),
		).toEqual({ kind: "capacity", message: errorMessage });
	});

	it("ignores an unclassified provider failure", () => {
		expect(
			providerErrorFromJsonEvents([
				{
					type: "message_end",
					message: { role: "assistant", stopReason: "error", errorMessage: "Malformed response body" },
				},
			]),
		).toBeUndefined();
	});

	it("terminates a child session as soon as a fatal provider event is streamed", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-provider-stream-"));
		try {
			const launcherPath = join(root, "fatal-provider.mjs");
			await writeFile(
				launcherPath,
				`console.log(JSON.stringify({type:"message_end",message:{role:"assistant",stopReason:"error",errorMessage:"Our servers are currently overloaded. Please try again later."}}));\nsetInterval(() => {}, 1000);\n`,
				"utf8",
			);
			const startedAt = Date.now();
			const result = await new PiChildSessionRunner({ launcherPath }).run(
				root,
				"job-provider-stream",
				"task-provider-stream",
				1,
				"worker",
				"run",
				{},
				1_500,
			);

			expect(Date.now() - startedAt).toBeLessThan(1_000);
			expect(providerErrorFromJsonEvents(result.jsonEvents)).toMatchObject({
				kind: "capacity",
				message: expect.stringContaining("currently overloaded"),
			});
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
	it("runs assistant -> tool batch -> tool result -> next assistant turn", async () => {
		const faux = createFauxCore({});
		faux.setResponses([
			fauxAssistantMessage(fauxToolCall("emit_evidence", { value: "worker output" }, { id: "emit-1" })),
			fauxAssistantMessage("worker completed"),
		]);
		const seen: string[] = [];
		const agent = new Agent({
			streamFn: faux.streamSimple,
			initialState: {
				model: faux.getModel(),
				systemPrompt: "You are an Astra worker. Use emit_evidence once.",
				tools: [
					{
						name: "emit_evidence",
						label: "Emit evidence",
						description: "Emit a structured worker result",
						parameters: Type.Object({ value: Type.String() }),
						execute: async (_id: string, params: unknown) => {
							const value = (params as { value: string }).value;
							seen.push(value);
							return {
								content: [{ type: "text", text: `recorded ${value}` }],
								details: { value },
							};
						},
					},
				],
			},
		});
		await agent.prompt("Produce the worker evidence.");
		const last = agent.state.messages.at(-1);
		expect(seen).toEqual(["worker output"]);
		expect(last?.role).toBe("assistant");
		expect(faux.state.callCount).toBe(2);
	});

	it("stops without another provider turn when a terminal tool succeeds", async () => {
		const faux = createFauxCore({});
		faux.setResponses([
			fauxAssistantMessage(fauxToolCall("submit", {}, { id: "submit-1" })),
			fauxAssistantMessage("should not run"),
		]);
		const agent = new Agent({
			streamFn: faux.streamSimple,
			initialState: {
				model: faux.getModel(),
				systemPrompt: "Submit exactly once.",
				tools: [
					{
						name: "submit",
						label: "Submit",
						description: "Persist the final result",
						parameters: Type.Object({}),
						execute: async () => ({
							content: [{ type: "text", text: "submitted" }],
							details: {},
							terminate: true,
						}),
					},
				],
			},
		});

		await agent.prompt("Submit now.");

		expect(faux.state.callCount).toBe(1);
		expect(agent.state.messages.at(-1)).toMatchObject({ role: "toolResult" });
	});
});
