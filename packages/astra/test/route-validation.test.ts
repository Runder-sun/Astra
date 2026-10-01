import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CodexResearchAdapters } from "../src/codex-adapters.ts";
import { CodexAppServerRunner } from "../src/codex-app-server.ts";
import { ResearchJob } from "../src/research.ts";
import { JsonlAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import type { MainAgentDecisionManifest, StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

const roots: string[] = [];
const fixture = fileURLToPath(new URL("./fixtures/codex-app-server.mjs", import.meta.url));

afterEach(async () => {
	vi.unstubAllEnvs();
	await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function reviewedJob() {
	const root = await mkdtemp(join(tmpdir(), "astra-route-validation-"));
	roots.push(root);
	const store = new JsonlAstraStore(root);
	const job = await ResearchJob.create(store, {
		workspaceRoot: root,
		objective: "route reviewed work",
		automation: "full",
	});
	const task = await job.dispatchTask({
		stageId: "validation",
		stageExecutionId: "validation",
		role: "worker",
		objective: "define a bounded question",
		inputArtifactRefs: [],
		requiredCanonicalArtifacts: [],
		requiredOutputType: "validation",
		requiredOutputFields: ["content"],
		acceptanceChecks: ["bounded question"],
		failureSignals: [],
		dependencies: [],
		scope: { workspaceRoot: root, allowedPaths: ["."] },
		allowedTools: ["read"],
		writeAuthority: "none",
		budget: { maxTurns: 2, maxToolCalls: 4, maxRuntimeMs: 1000 },
		reviewGateRequired: true,
		resumePolicy: "resume-session",
		successCriteria: ["bounded question"],
	});
	await job.setTaskStatus(task.id, "succeeded");
	const evidence = await job.recordEvidence({
		taskId: task.id,
		stageId: "validation",
		type: "validation",
		content: { content: "bounded question" },
		refs: [],
	});
	const review = await job.recordReview(
		reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
	);
	await job.decideEvidence(evidence.id, true);
	const artifact = await job.adoptEvidence(evidence.id);
	return { job, store, evidence, review, artifact };
}

describe("route decision validation", () => {
	it("retains an invalid plan error and stops repeated planning", async () => {
		const root = await mkdtemp(join(tmpdir(), "astra-plan-validation-"));
		roots.push(root);
		const store = new JsonlAstraStore(root);
		const job = await ResearchJob.create(store, {
			workspaceRoot: root,
			objective: "validate plan references",
			automation: "full",
		});
		const plan: StagePlanManifest = {
			schemaVersion: "astra.stage_plan_manifest.v1",
			id: "plan_invalid_input",
			jobId: job.state.frame.jobId,
			stageId: "validation",
			decisionRef: "plan_invalid_input",
			sessionRef: "fixture:plan",
			createdAt: new Date().toISOString(),
			tasks: [
				{
					key: "validate",
					objective: "validate question",
					inputArtifactRefs: ["artifact_typo"],
					requiredOutputFields: job.definitions.validation.requiredOutputFields,
					acceptanceChecks: ["verified"],
					failureSignals: ["invalid"],
					successCriteria: ["verified"],
				},
			],
			rationale: "reproduce unknown input",
		};
		const mainAgent = {
			planStage: vi.fn().mockResolvedValue(plan),
			decideEvidence: vi.fn(),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn(),
		};
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: vi.fn() },
			reviewer: { review: vi.fn() },
			mainAgent,
		});
		await supervisor.tick();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(reopened.state.paused).toBe(true);
		expect(reopened.state.frame.nextAction).toContain("plan_invalid_input");
		expect(reopened.state.frame.nextAction).toContain("unknown input artifact artifact_typo");
		expect(reopened.state.stagePlans[plan.id]).toBeUndefined();
		await supervisor.tick();
		expect(mainAgent.planStage).toHaveBeenCalledOnce();
	});

	it.each(["empty", "new", "artifact", "evidence", "all", "repeated", "unknown"])(
		"constrains plan inputs before submission (%s)",
		async (mode) => {
			const { job, store, evidence, artifact } = await reviewedJob();
			const target =
				mode === "new"
					? await ResearchJob.create(store, {
							workspaceRoot: job.state.frame.permissions.workspaceRoot,
							objective: "initial plan",
							automation: "full",
						})
					: job;
			const refs =
				mode === "repeated"
					? [artifact.id, artifact.id, artifact.id]
					: mode === "all"
						? [artifact.id, evidence.id]
						: ["empty", "new"].includes(mode)
							? []
							: [mode === "artifact" ? artifact.id : mode === "evidence" ? evidence.id : "artifact_typo"];
			vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "ok");
			vi.stubEnv(
				"ASTRA_FAKE_CODEX_OUTPUT",
				JSON.stringify({
					tasks: [
						{
							key: "validate",
							objective: "validate question",
							deliveryKind: "stage",
							inputArtifactRefs: refs,
							requiredOutputFields: job.definitions.validation.requiredOutputFields,
							acceptanceChecks: ["verified"],
							failureSignals: ["invalid"],
							successCriteria: ["verified"],
							hypothesis: "bounded",
							responsibilityBindings: [],
							responsibilityTransfers: [],
						},
					],
					rationale: "test reference validation",
				}),
			);
			const adapters = new CodexResearchAdapters(
				new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
			);
			if (["unknown", "repeated"].includes(mode))
				await expect(adapters.planStage(target)).rejects.toBeInstanceOf(NonRetryableResearchError);
			else expect((await adapters.planStage(target)).tasks[0].inputArtifactRefs).toEqual(refs);
		},
	);

	it("updates the next action after adoption and does not request a fresh dispatch on resume", async () => {
		const { job, store, artifact } = await reviewedJob();
		expect(job.status().nextAction).toBe("decide route from validation");
		await job.pause("inspect execution");
		await job.resume();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(reopened.status().nextAction).toBe("resume validation from saved progress");
		expect(reopened.state.canonicalRoute.stageArtifactIds.validation).toBe(artifact.id);
		expect(reopened.state.frame.activeStageId).toBe("validation");
	});

	it("persists a rejected route and stops repeated model calls without discarding reviewed work", async () => {
		const { job, store, review, artifact } = await reviewedJob();
		const decision: MainAgentDecisionManifest = {
			schemaVersion: "astra.main_agent_decision_manifest.v1",
			manifestId: "decision_invalid_ref",
			jobId: job.state.frame.jobId,
			decisionType: "route",
			decisionRef: "decision_invalid_ref",
			stageId: "validation",
			routeAction: "advance",
			targetStageId: "literature",
			evidenceRefs: [artifact.id, review.id],
			rationale: "advance based on the reviewed artifact",
			sessionRef: "fixture:route",
			createdAt: new Date().toISOString(),
		};
		const mainAgent = {
			planStage: vi.fn(),
			decideEvidence: vi.fn(),
			decideAdoption: vi.fn(),
			decideSearch: vi.fn(),
			decideRoute: vi.fn().mockResolvedValue(decision),
		};
		const supervisor = new ResearchSupervisor(job, store, {
			worker: { run: vi.fn() },
			reviewer: { review: vi.fn() },
			mainAgent,
		});
		await supervisor.tick();
		const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
		expect(reopened.state.paused).toBe(true);
		expect(reopened.state.frame.nextAction).toContain("route decision references unknown evidence");
		expect(reopened.state.frame.nextAction).toContain(decision.decisionRef);
		expect(reopened.state.canonicalRoute.stageArtifactIds.validation).toBe(artifact.id);
		await supervisor.tick();
		expect(mainAgent.decideRoute).toHaveBeenCalledOnce();
	});

	it.each(["valid", "review", "unknown"])("constrains Codex route references before submission (%s)", async (mode) => {
		const { job, evidence, review, artifact } = await reviewedJob();
		const refs =
			mode === "valid"
				? [artifact.id, evidence.id, job.state.graph.openQuestionIds[0]]
				: [mode === "review" ? review.id : "unknown_evidence"];
		vi.stubEnv("ASTRA_FAKE_CODEX_MODE", "ok");
		vi.stubEnv(
			"ASTRA_FAKE_CODEX_OUTPUT",
			JSON.stringify({
				routeAction: "advance",
				targetStageId: "literature",
				evidenceRefs: refs,
				question: null,
				newQuestions: [],
				rationale: "advance reviewed work",
			}),
		);
		const adapters = new CodexResearchAdapters(
			new CodexAppServerRunner({ executable: process.execPath, prefixArgs: [fixture] }),
		);
		if (mode === "valid") {
			await job.applyRouteDecision(await adapters.decideRoute(job));
			expect(job.state.frame.activeStageId).toBe("literature");
		} else {
			await expect(adapters.decideRoute(job)).rejects.toBeInstanceOf(NonRetryableResearchError);
			expect(Object.values(job.state.sessions).at(-1)?.status).toBe("failed");
		}
	});
});
