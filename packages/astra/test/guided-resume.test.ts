import { describe, expect, it, vi } from "vitest";
import { buildEffectiveTaskContract, semanticContractHash } from "../src/effective-contract.ts";
import { preparePlanEvidence } from "../src/plan-review.ts";
import { ResearchJob } from "../src/research.ts";
import { MemoryAstraStore } from "../src/store.ts";
import { NonRetryableResearchError, ResearchSupervisor } from "../src/supervisor.ts";
import type { StagePlanManifest } from "../src/types.ts";
import { reviewFixture } from "./review-fixture.ts";

describe("guided resume after worker review", () => {
	it.each(["candidate", "accepted", "deferred", "not-adopted"])(
		"replans without reconsidering old %s evidence",
		async (status) => {
			const store = new MemoryAstraStore();
			const job = await ResearchJob.create(store, {
				workspaceRoot: "/workspace",
				objective: "repair implementation after an external audit",
				automation: "full",
			});
			const plan: StagePlanManifest = {
				schemaVersion: "astra.stage_plan_manifest.v1",
				id: "plan_before_guidance",
				jobId: job.state.frame.jobId,
				stageId: "validation",
				decisionRef: "decision_before_guidance",
				tasks: [
					{
						key: "implementation",
						objective: "deliver implementation",
						inputArtifactRefs: [],
						requiredOutputFields: job.definitions.validation.requiredOutputFields,
						acceptanceChecks: ["verified"],
						failureSignals: ["incorrect"],
						successCriteria: ["verified"],
					},
				],
				rationale: "original plan",
				sessionRef: "fixture:main",
				createdAt: new Date().toISOString(),
			};
			await job.recordStagePlan(plan);
			const planEvidence = await preparePlanEvidence(job, plan);
			await job.recordReview(reviewFixture(job, { evidenceId: planEvidence.id, verdict: "pass", findings: [] }));
			const contract = buildEffectiveTaskContract(job, plan, plan.tasks[0]!);
			const task = await job.dispatchTask({
				...contract,
				effectiveContractHash: semanticContractHash(contract),
				replayKey: `stage-plan:${plan.id}:implementation`,
			});
			await job.setTaskStatus(task.id, "succeeded");
			const evidence = await job.recordEvidence({
				taskId: task.id,
				stageId: "validation",
				type: "validation",
				refs: [],
				content: { content: "old code" },
			});
			const review = await job.recordReview(
				reviewFixture(job, { evidenceId: evidence.id, verdict: "pass", findings: [] }),
			);
			if (status === "accepted" || status === "not-adopted") await job.decideEvidence(evidence.id, true);
			if (status === "not-adopted") {
				const decideAdoption = vi
					.fn()
					.mockResolvedValue({ adopt: false, rationale: "repair the incomplete contract comparison" });
				const supervisor = new ResearchSupervisor(job, store, {
					worker: { run: vi.fn() },
					reviewer: { review: vi.fn() },
					mainAgent: {
						planStage: vi.fn(),
						decideEvidence: vi.fn(),
						decideAdoption,
						decideSearch: vi.fn(),
						decideRoute: vi.fn(),
					},
				});
				await supervisor.tick();
				expect(job.state.paused).toBe(true);
				expect(job.state.frame.nextAction).toContain("repair the incomplete contract comparison");
				await supervisor.tick();
				expect(decideAdoption).toHaveBeenCalledOnce();
			}
			if (status === "deferred") {
				const decideEvidence = vi.fn().mockResolvedValue({
					decision: "defer",
					rationale: "repair the replay manifest",
					decisionRef: "decision_deferred",
				});
				const supervisor = new ResearchSupervisor(job, store, {
					worker: { run: vi.fn() },
					reviewer: { review: vi.fn() },
					mainAgent: {
						planStage: vi.fn(),
						decideEvidence,
						decideAdoption: vi.fn(),
						decideSearch: vi.fn(),
						decideRoute: vi.fn(),
					},
				});
				await supervisor.tick();
				expect(job.state.paused).toBe(true);
				expect(job.state.frame.nextAction).toContain("repair the replay manifest");
				expect(job.state.frame.nextAction).toContain("decision_deferred");
				await supervisor.tick();
				expect(decideEvidence).toHaveBeenCalledOnce();
			}
			await job.pause("external audit found a defect");
			await job.resumeWithGuidance("Repair the verified defect before adopting this implementation");
			const reopened = (await ResearchJob.open(store, job.state.frame.jobId))!;
			const mainAgent = {
				planStage: vi.fn().mockRejectedValue(new NonRetryableResearchError("fixture stops at replanning")),
				decideEvidence: vi.fn(),
				decideAdoption: vi.fn(),
				decideSearch: vi.fn(),
				decideRoute: vi.fn(),
			};
			const supervisor = new ResearchSupervisor(reopened, store, {
				worker: { run: vi.fn() },
				reviewer: { review: vi.fn() },
				mainAgent,
			});
			await supervisor.tick();
			expect(mainAgent.planStage).toHaveBeenCalledOnce();
			expect(mainAgent.decideEvidence).not.toHaveBeenCalled();
			expect(mainAgent.decideAdoption).not.toHaveBeenCalled();
			expect(reopened.state.evidence[evidence.id]).toMatchObject({
				status: status === "deferred" ? "candidate" : status === "not-adopted" ? "accepted" : status,
				content: { content: "old code" },
			});
			expect(reopened.state.reviews[review.id]).toEqual(review);
			expect(reopened.state.tasks[task.id].status).toBe("succeeded");
			const repairPlan = { ...plan, id: "plan_after_guidance", decisionRef: "decision_after_guidance" };
			await reopened.resume();
			await reopened.recordStagePlan(repairPlan);
			const repairEvidence = await preparePlanEvidence(reopened, repairPlan);
			expect(repairEvidence.content).toMatchObject({
				userGuidance: [
					expect.objectContaining({
						statement: "User guidance: Repair the verified defect before adopting this implementation",
					}),
				],
			});
			expect(reopened.state.evidence[planEvidence.id].content).toEqual(planEvidence.content);
		},
	);
});
