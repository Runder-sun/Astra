import fsPromises, { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext, ToolDefinition } from "@earendil-works/pi-coding-agent";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createAstraExtension } from "../src/extension.ts";
import { appendJobMemory, loadStageSkills, readJobMemory } from "../src/memory.ts";
import { ResearchJob } from "../src/research.ts";
import { researchBackend, runResearchControl } from "../src/research-control.ts";
import { JsonlAstraStore } from "../src/store.ts";

const roots: string[] = [];
async function fixture(pin?: string) {
	const root = await mkdtemp(join(tmpdir(), "astra-context-repair-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const first = await ResearchJob.create(store, {
		jobId: "job_first",
		objective: "first mission",
		workspaceRoot: root,
	});
	const second = await ResearchJob.create(store, {
		jobId: "job_second",
		objective: "second mission",
		workspaceRoot: root,
	});
	await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: first.state.frame.jobId }));
	const handlers = new Map<string, (event: never, ctx: ExtensionContext) => Promise<unknown>>();
	const tools = new Map<string, ToolDefinition>();
	const commands = new Map<string, (args: string, ctx: ExtensionContext) => Promise<void>>();
	const appendEntry = vi.fn();
	const getFlag = vi.fn((): string | undefined => undefined);
	const api = {
		on: (name: string, handler: (event: never, ctx: ExtensionContext) => Promise<unknown>) =>
			handlers.set(name, handler),
		registerTool: (tool: ToolDefinition) => tools.set(tool.name, tool),
		registerCommand: (name: string, command: { handler: (args: string, ctx: ExtensionContext) => Promise<void> }) =>
			commands.set(name, command.handler),
		registerFlag() {},
		getFlag,
		appendEntry,
		sendMessage() {},
	} as unknown as ExtensionAPI;
	createAstraExtension({ jobId: pin })(api);
	const ctx = { cwd: root, ui: { setStatus() {}, setWidget() {}, notify: vi.fn() } } as unknown as ExtensionContext;
	const invoke = (name: string, event: unknown = {}) => handlers.get(name)!(event as never, ctx);
	const status = () => tools.get("research_status")!.execute("fixture", {}, undefined, undefined, ctx);
	return { root, store, first, second, ctx, invoke, status, tools, commands, appendEntry, getFlag };
}

afterEach(async () => {
	vi.restoreAllMocks();
	syncBuiltinESMExports();
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

describe("fresh session target and isolated auxiliary context", () => {
	it("successful run publication switches the pin even when research driving fails before its first tick", async () => {
		const f = await fixture("job_first");
		vi.stubEnv("ASTRA_MAX_TICKS", "0");
		await expect(f.commands.get("research")!("run published before drive failure", f.ctx)).rejects.toThrow(/0 ticks/);
		const active = JSON.parse(await readFile(join(f.root, ".astra/active-job.json"), "utf8")) as { jobId: string };
		expect(active.jobId).not.toBe("job_first");
		expect((await f.status()).details).toMatchObject({ jobId: active.jobId });
	});
	it("the carried control target remains the session target when a later environment pin differs", async () => {
		const f = await fixture("job_second");
		f.getFlag.mockReturnValue(JSON.stringify({ action: "pause", jobId: "job_first", reason: "carried target" }));
		await f.invoke("session_start");
		expect((await f.status()).details).toMatchObject({ jobId: "job_first", paused: true });
		expect((await ResearchJob.open(f.store, "job_second"))!.state.paused).toBe(false);
	});
	it.each(["extension", "control"])(
		"failed %s atomic binding publication preserves the old binding, pin and unpublished job",
		async (entry) => {
			const f = await fixture("job_first");
			const target = join(f.root, ".astra/active-job.json");
			const original = await readFile(target, "utf8");
			const rename = fsPromises.rename;
			let injected = 0;
			const fault = vi.spyOn(fsPromises, "rename").mockImplementation(async (from, to) => {
				if (String(to) === target) {
					injected++;
					throw Object.assign(new Error("injected binding publication EIO"), { code: "EIO" });
				}
				return rename(from, to);
			});
			syncBuiltinESMExports();
			try {
				const operation =
					entry === "extension"
						? f.tools
								.get("research_start")!
								.execute("start", { objective: "unpublished new mission" }, undefined, undefined, f.ctx)
						: runResearchControl({ action: "run", objective: "unpublished new mission" }, f.root);
				await expect(operation).rejects.toMatchObject({ code: "EIO" });
				expect(injected).toBe(1);
				expect(await readFile(target, "utf8")).toBe(original);
				expect((await f.status()).details).toMatchObject({ jobId: "job_first" });
				expect(
					(await readdir(join(f.root, ".astra/jobs"))).filter((id) => id !== "job_first" && id !== "job_second"),
				).toHaveLength(1);
			} finally {
				fault.mockRestore();
				syncBuiltinESMExports();
			}
			if (entry === "extension") {
				const started = await f.tools
					.get("research_start")!
					.execute("start", { objective: "published new mission" }, undefined, undefined, f.ctx);
				expect((await f.status()).details).toEqual(started.details);
			}
		},
	);

	it("failed atomic memory publication exposes no partial entry and does not alter legacy bytes", async () => {
		const f = await fixture();
		const root = join(f.root, ".astra/jobs/job_first/memory");
		await mkdir(root);
		const bytes = "{legacy partial";
		await writeFile(join(root, "entries.jsonl"), bytes);
		const rename = fsPromises.rename;
		let injected = 0;
		const fault = vi.spyOn(fsPromises, "rename").mockImplementation(async (from, to) => {
			if (String(to).startsWith(root) && String(to).endsWith(".json")) {
				injected++;
				throw Object.assign(new Error("injected memory publication EIO"), { code: "EIO" });
			}
			return rename(from, to);
		});
		syncBuiltinESMExports();
		try {
			await expect(
				appendJobMemory(f.root, "job_first", { kind: "note", content: "never published", sourceRefs: [] }),
			).rejects.toMatchObject({ code: "EIO" });
			expect(injected).toBe(1);
			expect(await readJobMemory(f.root, "job_first")).toEqual([]);
			expect(await readFile(join(root, "entries.jsonl"), "utf8")).toBe(bytes);
		} finally {
			fault.mockRestore();
			syncBuiltinESMExports();
		}
	});
	it("orders healthy legacy timestamps by instant and returns no entries for a zero limit", async () => {
		const f = await fixture();
		const root = join(f.root, ".astra/jobs/job_first/memory");
		await mkdir(root);
		const base = {
			schemaVersion: "astra.memory_entry.v1",
			scope: "job",
			jobId: "job_first",
			kind: "note",
			content: "healthy",
			sourceRefs: [],
		};
		await writeFile(
			join(root, "entries.jsonl"),
			`${JSON.stringify({ ...base, id: "older", createdAt: "2026-10-03T08:30:00+08:00" })}\n${JSON.stringify({ ...base, id: "newer", createdAt: "2026-10-03T02:00:00Z" })}`,
		);
		expect((await readJobMemory(f.root, "job_first", 1)).map((entry) => entry.id)).toEqual(["newer"]);
		expect(await readJobMemory(f.root, "job_first", 0)).toEqual([]);
	});
	it("refreshes external journal changes and active job rebinding through status, context and guidance", async () => {
		const f = await fixture();
		await f.status();
		await f.first.pause("external operator");
		expect((await f.status()).details).toMatchObject({ paused: true, eventSeq: f.first.state.eventSeq });
		await writeFile(join(f.root, ".astra/active-job.json"), JSON.stringify({ jobId: "job_second" }));
		expect((await f.status()).details).toMatchObject({ jobId: "job_second" });
		expect(await f.invoke("before_agent_start", { systemPrompt: "base" })).toMatchObject({
			systemPrompt: expect.stringContaining("second mission"),
		});
		await f.commands.get("research-guide")!("choose second route", f.ctx);
		expect(
			Object.values((await ResearchJob.open(f.store, "job_second"))!.state.graph.nodes).some((node) =>
				node.statement.includes("choose second"),
			),
		).toBe(true);
		await f.invoke("session_before_compact");
		await f.invoke("session_shutdown");
		expect(await readJobMemory(f.root, "job_second")).toHaveLength(2);
	});

	it("carries the pinned target through backend selection and public control", async () => {
		const f = await fixture("job_first");
		await writeFile(join(f.root, ".astra/active-job.json"), JSON.stringify({ jobId: "job_second" }));
		await writeFile(join(f.root, ".astra/jobs/job_first/backend.json"), JSON.stringify({ backend: "codex" }));
		const request = { action: "status" as const, jobId: "job_first" };
		expect(await researchBackend(request, f.root)).toBe("codex");
		expect(await runResearchControl(request, f.root)).toMatchObject({ jobId: "job_first", backend: "codex" });
		await f.commands.get("research-pause")!("pinned pause", f.ctx);
		expect((await ResearchJob.open(f.store, "job_first"))!.state.paused).toBe(true);
		expect((await ResearchJob.open(f.store, "job_second"))!.state.paused).toBe(false);
	});

	it("blocks tools for a missing explicit job while allowing ordinary unbound coding", async () => {
		const f = await fixture("job_missing");
		expect(await f.invoke("tool_call", { toolName: "read", input: { path: "/outside" } })).toMatchObject({
			block: true,
		});
		const unbound = await fixture();
		await rm(join(unbound.root, ".astra/active-job.json"));
		expect(await unbound.invoke("tool_call", { toolName: "read", input: { path: "/outside" } })).toBeUndefined();
	});

	it("preserves healthy legacy memory around malformed, null and wrong-job lines without rewriting it", async () => {
		const f = await fixture();
		const memory = join(f.root, ".astra/jobs/job_first/memory");
		await mkdir(memory);
		const entry = {
			schemaVersion: "astra.memory_entry.v1",
			id: "memory_good",
			scope: "job",
			jobId: "job_first",
			kind: "note",
			content: "retained memory",
			sourceRefs: [],
			createdAt: "2026-10-03T00:00:00.000Z",
		};
		const bytes = `${JSON.stringify(entry)}\n{broken\nnull\n${JSON.stringify({ ...entry, id: "memory_wrong", jobId: "job_second" })}\n${JSON.stringify({ ...entry, id: "memory_last" })}`;
		await writeFile(join(memory, "entries.jsonl"), bytes);
		const diagnostics = vi.fn();
		expect(await readJobMemory(f.root, "job_first", 24, diagnostics)).toEqual([
			entry,
			{ ...entry, id: "memory_last" },
		]);
		expect(diagnostics.mock.calls.map(([diagnostic]) => diagnostic.line)).toEqual([2, 3, 4]);
		expect(await readFile(join(memory, "entries.jsonl"), "utf8")).toBe(bytes);
		expect(await f.invoke("before_agent_start", { systemPrompt: "base" })).toMatchObject({
			systemPrompt: expect.stringContaining("first mission"),
		});
		expect(f.appendEntry.mock.calls.filter(([kind]) => kind === "astra_auxiliary_diagnostic")).toHaveLength(3);
	});
	it.each(["schemaVersion", "id", "scope", "jobId", "kind", "content", "sourceRefs", "createdAt", "role", "stageId"])(
		"isolates malformed %s in an independent memory entry and reports its failure",
		async (field) => {
			const f = await fixture();
			const directory = join(f.root, ".astra/jobs/job_first/memory");
			await mkdir(directory);
			const entry: Record<string, unknown> = {
				schemaVersion: "astra.memory_entry.v1",
				id: "bad",
				scope: "job",
				jobId: "job_first",
				kind: "note",
				content: "healthy",
				sourceRefs: [],
				createdAt: "2026-10-03T00:00:00Z",
			};
			entry[field] = field === "role" || field === "stageId" ? 123 : null;
			await writeFile(join(directory, "bad.json"), JSON.stringify(entry));
			const diagnostics = vi.fn();
			expect(await readJobMemory(f.root, "job_first", 24, diagnostics)).toEqual([]);
			expect(diagnostics).toHaveBeenCalledOnce();
			const context = await f.invoke("before_agent_start", { systemPrompt: "base" });
			expect(context).toMatchObject({ systemPrompt: expect.stringContaining("first mission") });
			expect(JSON.stringify(context)).not.toContain("undefined");
		},
	);

	it.each(["kind", "role"])(
		"rejects array-shaped memory %s while preserving healthy strings and core context",
		async (field) => {
			const f = await fixture();
			const directory = join(f.root, ".astra/jobs/job_first/memory");
			await mkdir(directory);
			const healthy = {
				schemaVersion: "astra.memory_entry.v1",
				id: "healthy-string",
				scope: "job",
				jobId: "job_first",
				kind: "note",
				role: "worker",
				content: "retained healthy string memory",
				sourceRefs: [],
				createdAt: "2026-10-03T00:00:00Z",
			};
			const invalid = {
				...healthy,
				id: "invalid-array",
				content: "bad array memory",
				[field]: field === "kind" ? ["note"] : ["worker"],
			};
			const bytes = `${JSON.stringify(healthy)}\n${JSON.stringify(invalid)}\n`;
			await writeFile(join(directory, "entries.jsonl"), bytes);
			const diagnostics = vi.fn();
			expect(await readJobMemory(f.root, "job_first", 24, diagnostics)).toEqual([healthy]);
			expect(diagnostics).toHaveBeenCalledOnce();
			expect(diagnostics.mock.calls[0][0].line).toBe(2);
			const context = await f.invoke("before_agent_start", { systemPrompt: "base" });
			expect(context).toMatchObject({ systemPrompt: expect.stringContaining("first mission") });
			expect(JSON.stringify(context)).toContain(healthy.content);
			expect(JSON.stringify(context)).not.toContain(invalid.content);
			expect(await readFile(join(directory, "entries.jsonl"), "utf8")).toBe(bytes);
		},
	);
	it("publishes 128 independent atomic memory entries and excludes temporary or invalid entries before limiting", async () => {
		const f = await fixture();
		const ids = await Promise.all(
			Array.from({ length: 128 }, (_, index) =>
				appendJobMemory(f.root, "job_first", { kind: "note", content: `memory ${index}`, sourceRefs: [] }),
			),
		);
		const root = join(f.root, ".astra/jobs/job_first/memory");
		expect(new Set(ids).size).toBe(128);
		expect((await readdir(root)).filter((name) => name.endsWith(".json"))).toHaveLength(128);
		await writeFile(join(root, "partial.json.tmp"), "{broken");
		await writeFile(join(root, "invalid.json"), "null");
		expect(await readJobMemory(f.root, "job_first", 128)).toHaveLength(128);
	});

	it("retains healthy skills and mission when one skill file fails, and checkpoint when memory append fails", async () => {
		const f = await fixture();
		const skills = join(f.root, ".pi/skills/astra");
		await mkdir(skills, { recursive: true });
		await writeFile(join(skills, "validation.md"), "healthy stage skill");
		await mkdir(join(skills, "main-agent.md"));
		expect(await loadStageSkills(f.root, "validation", "main-agent")).toContain("healthy stage skill");
		expect(await f.invoke("before_agent_start", { systemPrompt: "base" })).toMatchObject({
			systemPrompt: expect.stringContaining("healthy stage skill"),
		});
		await writeFile(join(f.root, ".astra/jobs/job_first/memory"), "not a directory");
		expect(await f.invoke("session_before_compact")).toMatchObject({
			customInstructions: expect.stringContaining("first mission"),
		});
		await f.invoke("session_shutdown");
		expect(f.appendEntry).toHaveBeenCalledWith("astra_checkpoint", expect.objectContaining({ jobId: "job_first" }));
		expect(f.appendEntry).toHaveBeenCalledWith("astra_shutdown", expect.objectContaining({ jobId: "job_first" }));
	});
});
