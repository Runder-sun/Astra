import { chmod, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createReadTool } from "@earendil-works/pi-coding-agent";
import { afterEach, expect, it, vi } from "vitest";
import { fauxAssistantMessage, fauxToolCall } from "../../ai/src/providers/faux.ts";
import { createHarness } from "../../coding-agent/test/suite/harness.ts";
import { reviewerManifestPath } from "../src/contracts.ts";
import { createAstraExtension } from "../src/extension.ts";
import { PiChildSessionRunner, PiReviewerAdapter } from "../src/pi-child-session.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import type { ReviewPacket } from "../src/types.ts";

afterEach(() => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
});

it.each(["integrity", "integrity-read-first", "integrity-submit-first", "assessment"])(
	"the real Pi faux tool loop handles %s without confusing file failure with assessment correction",
	async (fault) => {
		const harness = await createHarness({
			tools: [createReadTool(process.cwd())],
			extensionFactories: [createAstraExtension({ role: "reviewer", jobId: "job_pi_faux" })],
		});
		try {
			const root = harness.tempDir;
			const store = new JsonlAstraStore(root);
			const job = await ResearchJob.create(store, {
				jobId: "job_pi_faux",
				objective: "offline Pi frozen review",
				workspaceRoot: root,
			});
			const task = await job.dispatchTask({
				stageId: "validation",
				stageExecutionId: "validation",
				role: "worker",
				objective: "frozen worker",
				inputArtifactRefs: [],
				requiredCanonicalArtifacts: [],
				requiredOutputType: "validation",
				requiredOutputFields: ["content"],
				acceptanceChecks: ["verified"],
				successCriteria: ["complete"],
				repairChecks: [{ issueId: "synthetic", criterion: "repair-only exact check" }],
				failureSignals: [],
				dependencies: [],
				scope: { workspaceRoot: root, allowedPaths: ["."] },
				allowedTools: ["read"],
				writeAuthority: "none",
				budget: { maxTurns: 3, maxToolCalls: 4, maxRuntimeMs: 1000 },
				reviewGateRequired: true,
				resumePolicy: "resume-session",
			});
			const workspace = join(root, ".astra/jobs", task.jobId, "workspaces", task.id);
			await mkdir(workspace, { recursive: true });
			await writeFile(join(workspace, "result.json"), '{"metric":1}\n');
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: "validation",
				content: { content: "target" },
				refs: ["result.json"],
			});
			const runner = new PiChildSessionRunner();
			let turns = 0;
			let receivedCriteria: string[] = [];
			const requestSignals: boolean[] = [];
			vi.spyOn(runner, "run").mockImplementation(
				async (cwd, jobId, taskId, _attempt, _role, actualPrompt, env = {}) => {
					for (const [key, value] of Object.entries(env)) if (value !== undefined) vi.stubEnv(key, value);
					vi.stubEnv("ASTRA_PROJECT_ROOT", root);
					vi.stubEnv("ASTRA_TASK_PACKET", join(cwd, "task-packet.json"));
					const packet = JSON.parse(await readFile(join(cwd, "review-packet.json"), "utf8")) as ReviewPacket;
					if (fault.startsWith("integrity")) {
						const ref = packet.resolvedEvidenceRefs.find((item) => item.sourceRef === "result.json")!;
						await chmod(join(cwd, ref.path), 0o600);
						await writeFile(join(cwd, ref.path), '{"metric":999}\n');
					}
					const refs = [`evidence:${evidence.id}`];
					const expectedCriteria = ["verified", "complete", "repair-only exact check"];
					const valid = {
						evidenceId: evidence.id,
						verdict: "pass",
						findings: [],
						score: 1,
						verifiedRefs: refs,
						criteria: expectedCriteria.map((criterion) => ({
							criterion,
							passed: true,
							score: 1,
							evidenceRefs: refs,
							rationale: "local assessment",
						})),
					};
					const invalid = { ...valid, criteria: [...valid.criteria, valid.criteria[0]] };
					harness.setResponses([
						(context, options) => {
							turns++;
							requestSignals.push(options?.signal?.aborted ?? false);
							const visiblePrompt = context.messages
								.filter((message) => message.role === "user")
								.map((message) =>
									typeof message.content === "string"
										? message.content
										: message.content
												.filter((part) => part.type === "text")
												.map((part) => part.text)
												.join("\n"),
								)
								.join("\n");
							const encodedCriteria = visiblePrompt.match(/Frozen review criteria: (\[[^\n]+\])/u)?.[1];
							const visibleCriteria = encodedCriteria ? (JSON.parse(encodedCriteria) as string[]) : [];
							receivedCriteria = visibleCriteria;
							expect(visibleCriteria).toEqual(expectedCriteria);
							expect(actualPrompt).toContain(JSON.stringify(expectedCriteria));
							valid.criteria = visibleCriteria.map((criterion) => ({
								criterion,
								passed: true,
								score: 1,
								evidenceRefs: refs,
								rationale: "local assessment",
							}));
							invalid.criteria = [...valid.criteria, valid.criteria[0]];
							const submit = fauxToolCall("astra_submit_review", fault === "assessment" ? invalid : valid);
							const read = fauxToolCall("read", { path: join(cwd, "review-packet.json") });
							return fauxAssistantMessage(
								fault === "integrity-read-first"
									? [read, submit]
									: fault === "integrity-submit-first"
										? [submit, read]
										: [submit],
								{ stopReason: "toolUse" },
							);
						},
						(_context, options) => {
							turns++;
							requestSignals.push(options?.signal?.aborted ?? false);
							return fauxAssistantMessage([fauxToolCall("astra_submit_review", valid)], {
								stopReason: "toolUse",
							});
						},
					]);
					await harness.session.bindExtensions({});
					await harness.session.prompt(actualPrompt);
					expect(receivedCriteria).toEqual(expectedCriteria);
					if (fault.startsWith("integrity")) expect(requestSignals).toEqual([false]);
					expect(turns).toBe(fault.startsWith("integrity") ? 1 : 2);
					expect(harness.eventsOfType("agent_end")).toHaveLength(1);
					expect(harness.eventsOfType("turn_start")).toHaveLength(harness.eventsOfType("turn_end").length);
					if (fault.startsWith("integrity")) {
						expect(
							harness
								.eventsOfType("tool_execution_end")
								.find((event) => event.toolName === "astra_submit_review")?.result,
						).toHaveProperty("terminate", true);
						if (fault === "integrity-read-first")
							expect(
								harness.eventsOfType("tool_execution_end").find((event) => event.toolName === "read")?.isError,
							).toBe(false);
						await expect(readFile(reviewerManifestPath(root, jobId, taskId))).rejects.toMatchObject({
							code: "ENOENT",
						});
					}
					return { exitCode: 0, stdout: "", stderr: "", jsonEvents: harness.events, costUsd: 0 };
				},
			);
			const adapter = new PiReviewerAdapter(runner);
			if (fault.startsWith("integrity")) {
				await expect(adapter.review(evidence, job)).rejects.toThrow(/integrity|version/i);
				expect(
					Object.values(job.state.tasks)
						.filter((item) => item.role === "reviewer")
						.every((item) => item.status === "failed"),
				).toBe(true);
				expect(Object.values(job.state.sessions).every((item) => item.status === "failed")).toBe(true);
				expect(
					(await store.readEvents(job.state.frame.jobId)).filter(
						(saved) => saved.event.type === "review_delivery_rejected",
					),
				).toHaveLength(0);
			} else {
				const review = await adapter.review(evidence, job);
				expect(review.verdict).toBe("pass");
				expect(review.criteria?.map((criterion) => criterion.criterion)).toEqual([
					"verified",
					"complete",
					"repair-only exact check",
				]);
				const registered = await job.recordReview({ ...review, evidenceId: evidence.id });
				const reopened = await ResearchJob.open(store, job.state.frame.jobId);
				if (!reopened) throw new Error("offline review job disappeared");
				await reopened.recoverPendingOperations();
				expect(reopened.state.reviews[registered.id]).toMatchObject({ verdict: "pass", criteria: review.criteria });
				expect(turns).toBe(2);
			}
		} finally {
			harness.cleanup();
		}
	},
);
