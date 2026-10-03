import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";

const state = vi.hoisted(() => ({ main: vi.fn(), control: vi.fn() }));
vi.mock("@earendil-works/pi-coding-agent", () => ({ main: state.main }));
vi.mock("../src/research-control.ts", async () => {
	const actual = await vi.importActual<Record<string, unknown>>("../src/research-control.ts");
	return { ...actual, runResearchControl: state.control };
});

import { runAstra } from "../src/launcher.ts";

const roots: string[] = [];
afterEach(async () => {
	vi.unstubAllEnvs();
	vi.restoreAllMocks();
	state.main.mockReset();
	state.control.mockReset();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

it.each(["pi", "codex"] as const)(
	"launcher selects the environment-pinned %s backend and project root once",
	async (backend) => {
		const root = await mkdtemp(join(tmpdir(), "astra-launcher-target-"));
		roots.push(root);
		const store = new JsonlAstraStore(root);
		await ResearchJob.create(store, { jobId: "job_pin", objective: "pin", workspaceRoot: root });
		await ResearchJob.create(store, { jobId: "job_active", objective: "active", workspaceRoot: root });
		await writeFile(join(root, ".astra/active-job.json"), JSON.stringify({ jobId: "job_active" }));
		await writeFile(join(root, ".astra/jobs/job_pin/backend.json"), JSON.stringify({ backend }));
		await writeFile(
			join(root, ".astra/jobs/job_active/backend.json"),
			JSON.stringify({ backend: backend === "pi" ? "codex" : "pi" }),
		);
		vi.stubEnv("ASTRA_PROJECT_ROOT", root);
		vi.stubEnv("ASTRA_JOB_ID", "job_pin");
		state.control.mockResolvedValue({ action: "status", jobId: "job_pin", backend });
		vi.spyOn(console, "log").mockImplementation(() => {});
		await runAstra(["research", "status"]);
		if (backend === "codex") {
			expect(state.control).toHaveBeenCalledWith({ action: "status", jobId: "job_pin" }, root);
			expect(state.main).not.toHaveBeenCalled();
		} else {
			expect(state.main).toHaveBeenCalledOnce();
			const args = state.main.mock.calls[0][0] as string[];
			expect(JSON.parse(args[args.indexOf("--astra-research-control") + 1])).toEqual({
				action: "status",
				jobId: "job_pin",
			});
			expect(state.control).not.toHaveBeenCalled();
		}
	},
);
