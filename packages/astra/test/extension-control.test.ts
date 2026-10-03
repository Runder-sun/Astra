import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionCommandContext, ExtensionFactory } from "@earendil-works/pi-coding-agent";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ResearchControlRequest, ResearchControlResult } from "../src/research-control.ts";

const control = vi.hoisted(() => ({
	run: vi.fn<(request: ResearchControlRequest, cwd: string) => Promise<ResearchControlResult>>(),
}));

vi.mock("../src/research-control.ts", () => ({
	decodeResearchControl: (value: string) => JSON.parse(value) as ResearchControlRequest,
	parseResearchControlArgs: (args: string[]): ResearchControlRequest => {
		const requestedAction = args[0] ?? "status";
		const action = requestedAction === "continue" ? "resume" : requestedAction;
		const remainder = args.slice(1).join(" ").trim();
		return {
			action: action as ResearchControlRequest["action"],
			...(action === "run" && remainder ? { objective: remainder } : {}),
			...(action === "pause" && remainder ? { reason: remainder } : {}),
		};
	},
	runResearchControl: control.run,
}));

import { createAstraExtension } from "../src/extension.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";

type CommandHandler = (args: string, ctx: ExtensionCommandContext) => Promise<void> | void;

interface ExtensionFixture {
	api: ExtensionAPI;
	commands: Map<string, CommandHandler>;
	context: ExtensionCommandContext;
	appendEntry: ReturnType<typeof vi.fn<ExtensionAPI["appendEntry"]>>;
	sendMessage: ReturnType<typeof vi.fn<ExtensionAPI["sendMessage"]>>;
}

function createFixture(factory: ExtensionFactory, cwd = "/workspace"): ExtensionFixture {
	const commands = new Map<string, CommandHandler>();
	const appendEntry = vi.fn<ExtensionAPI["appendEntry"]>();
	const sendMessage = vi.fn<ExtensionAPI["sendMessage"]>();
	const api = {
		appendEntry,
		getFlag: vi.fn(() => undefined),
		on: vi.fn(),
		registerCommand(name: string, command: { handler: CommandHandler }) {
			commands.set(name, command.handler);
		},
		registerFlag: vi.fn(),
		registerTool: vi.fn(),
		sendMessage,
	} as unknown as ExtensionAPI;
	const context = {
		cwd,
		ui: {
			notify: vi.fn(),
			setStatus: vi.fn(),
			setWidget: vi.fn(),
		},
	} as unknown as ExtensionCommandContext;
	void factory(api);
	return { api, commands, context, appendEntry, sendMessage };
}

async function runCommand(fixture: ExtensionFixture, name: string, args = ""): Promise<void> {
	const handler = fixture.commands.get(name);
	if (!handler) throw new Error(`missing command: ${name}`);
	await handler(args, fixture.context);
}

beforeEach(() => {
	control.run.mockReset();
	control.run.mockImplementation(async (request) => ({ action: request.action, jobId: "job_control" }));
});

describe("Astra research control commands", () => {
	it("routes run, pause, and continue through the shared control service", async () => {
		const fixture = createFixture(createAstraExtension());

		await runCommand(fixture, "research", "run durable literature review");
		await runCommand(fixture, "research", "pause operator requested");
		await runCommand(fixture, "research", "continue");

		expect(control.run).toHaveBeenNthCalledWith(
			1,
			{ action: "run", objective: "durable literature review" },
			"/workspace",
			expect.any(Function),
		);
		expect(control.run).toHaveBeenNthCalledWith(
			2,
			{ action: "pause", reason: "operator requested", jobId: "job_control" },
			"/workspace",
		);
		expect(control.run).toHaveBeenNthCalledWith(3, { action: "resume", jobId: "job_control" }, "/workspace");
		expect(fixture.appendEntry).toHaveBeenCalledTimes(3);
		expect(fixture.sendMessage).toHaveBeenCalledTimes(3);
	});

	it("routes compatibility command aliases through the same control service", async () => {
		const fixture = createFixture(createAstraExtension());

		await runCommand(fixture, "research-status");
		await runCommand(fixture, "research-pause", "manual gate");
		await runCommand(fixture, "research-resume");

		expect(control.run).toHaveBeenNthCalledWith(1, { action: "status" }, "/workspace");
		expect(control.run).toHaveBeenNthCalledWith(2, { action: "pause", reason: "manual gate" }, "/workspace");
		expect(control.run).toHaveBeenNthCalledWith(3, { action: "resume" }, "/workspace");
	});

	it("shares board, guidance, and route state through research commands", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-extension-board-"));
		try {
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				jobId: "job_extension_board",
				objective: "collaborate on a research route",
				workspaceRoot: root,
			});
			await mkdir(join(root, ".astra"), { recursive: true });
			await writeFile(
				join(root, ".astra", "active-job.json"),
				`${JSON.stringify({ jobId: job.state.frame.jobId })}\n`,
			);
			const fixture = createFixture(createAstraExtension(), root);

			await runCommand(fixture, "research-guide", "Prefer a reproducible one-GPU route");
			await runCommand(fixture, "research-board");
			await runCommand(fixture, "research-route");

			const reopened = await ResearchJob.open(store, job.state.frame.jobId);
			expect(
				Object.values(reopened?.state.graph.nodes ?? {}).some((node) => node.statement.includes("one-GPU")),
			).toBe(true);
			expect(fixture.commands.has("research-board")).toBe(true);
			expect(fixture.commands.has("research-route")).toBe(true);
			expect(await readFile(join(root, ".astra", "active-job.json"), "utf8").then(JSON.parse)).toEqual({
				jobId: job.state.frame.jobId,
			});
		} finally {
			await rm(root, { recursive: true, force: true });
		}
	});
});
