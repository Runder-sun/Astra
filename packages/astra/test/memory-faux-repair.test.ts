import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, it } from "vitest";
import { fauxAssistantMessage } from "../../ai/src/providers/faux.ts";
import { createHarness } from "../../coding-agent/test/suite/harness.ts";
import { createAstraExtension } from "../src/extension.ts";
import { appendJobMemory } from "../src/memory.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";

it.each(["memory-file", "skill-file"])(
	"the local faux model receives mission and healthy auxiliary context with an unreadable %s",
	async (fault) => {
		const jobId = "job_faux_auxiliary";
		const harness = await createHarness({ extensionFactories: [createAstraExtension({ jobId })] });
		const observed: Array<{ mission: boolean; memory: boolean; skill: boolean; status: boolean }> = [];
		const errors: string[] = [];
		try {
			await ResearchJob.create(new JsonlAstraStore(harness.tempDir), {
				jobId,
				objective: "retained faux mission",
				workspaceRoot: harness.tempDir,
			});
			await appendJobMemory(harness.tempDir, jobId, {
				kind: "note",
				content: "retained healthy memory",
				sourceRefs: [],
			});
			const skills = join(harness.tempDir, ".pi/skills/astra");
			await mkdir(skills, { recursive: true });
			await writeFile(join(skills, "validation.md"), "retained healthy skill");
			if (fault === "memory-file")
				await mkdir(join(harness.tempDir, ".astra/jobs", jobId, "memory/unreadable.json"));
			else await mkdir(join(skills, "main-agent.md"));
			await harness.session.bindExtensions({ onError: (error) => errors.push(error.event) });
			harness.setResponses([
				(context) => {
					observed.push({
						mission: context.systemPrompt?.includes("retained faux mission") ?? false,
						memory: context.systemPrompt?.includes("retained healthy memory") ?? false,
						skill: context.systemPrompt?.includes("retained healthy skill") ?? false,
						status: context.messages.some((message) => JSON.stringify(message).includes("Astra status:")),
					});
					return fauxAssistantMessage("local response");
				},
			]);
			await harness.session.prompt("verify auxiliary isolation using only the local faux provider");
			expect(observed).toEqual([{ mission: true, memory: true, skill: true, status: true }]);
			expect(errors).toEqual([]);
		} finally {
			harness.cleanup();
		}
	},
);
