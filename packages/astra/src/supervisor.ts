import { randomUUID } from "node:crypto";
import {
	buildEffectiveTaskContract,
	planGenerationBasisHash,
	semanticContractHash,
	taskContractMatches,
} from "./effective-contract.ts";
import { applyPendingPauses } from "./pause-control.ts";
import {
	evidenceHasCurrentPlanApproval,
	evidenceHasCurrentPlanApprovalFromSnapshot,
	hasFrozenPlanContract,
	hasFrozenPlanContractFromSnapshot,
	planEvidence,
	planReviewStatus,
	planReviewStatusFromSnapshot,
	preparePlanEvidence,
} from "./plan-review.ts";
import { checksum, MAX_TASK_ATTEMPTS, type ResearchJob, ResearchTaskBudgetError } from "./research.ts";
import type { AstraStore } from "./store.ts";
import type {
	CandidateEvaluation,
	CriterionAssessment,
	Evidence,
	IncrementalRevision,
	MainAgentDecisionManifest,
	Obligation,
	ReviewVerdict,
	SearchBatch,
	StagePlanManifest,
	TaskPacket,
} from "./types.ts";

export interface WorkerRunResult {
	content: unknown;
	refs: string[];
	artifactType: string;
	incrementalRevision?: IncrementalRevision;
}

export interface ReviewerRunResult {
	targetVersionHash?: string;
	verdict: ReviewVerdict;
	findings: string[];
	reviewerTaskId?: string;
	score?: number;
	criteria?: CriterionAssessment[];
	verifiedRefs?: string[];
}

export class NonRetryableResearchError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "NonRetryableResearchError";
	}
}

export class ProviderCapacityError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "ProviderCapacityError";
	}
}

const PROVIDER_BACKOFF_BASE_MS = 15_000;
const PROVIDER_BACKOFF_MAX_MS = 5 * 60_000;

class ResearchPausedError extends Error {}

export interface ResearchWorkerAdapter {
	run(task: TaskPacket, job: ResearchJob): Promise<WorkerRunResult>;
}

export interface ResearchReviewerAdapter {
	review(evidence: Evidence, job: ResearchJob): Promise<ReviewerRunResult>;
}

export interface ResearchMainAgentAdapter {
	planStage(
		job: ResearchJob,
		obligation?: Obligation,
		requestedMode?: "decompose" | "search" | "repair",
	): Promise<StagePlanManifest>;
	decideEvidence(evidence: Evidence, job: ResearchJob): Promise<MainAgentDecisionManifest>;
	decideAdoption(evidence: Evidence, job: ResearchJob): Promise<MainAgentDecisionManifest>;
	decideSearch(
		batch: SearchBatch,
		evaluations: CandidateEvaluation[],
		job: ResearchJob,
	): Promise<MainAgentDecisionManifest>;
	decideRoute(job: ResearchJob, obligation?: Obligation): Promise<MainAgentDecisionManifest>;
}

export interface SupervisorOptions {
	owner?: string;
	maxParallel?: number;
	worker: ResearchWorkerAdapter;
	reviewer: ResearchReviewerAdapter;
	mainAgent: ResearchMainAgentAdapter;
}

export interface TickResult {
	stageId: string;
	dispatchedTaskIds: string[];
	completed: boolean;
	routeChanged: boolean;
	recovered: boolean;
	paused: boolean;
}

/** Pi owns each provider/tool loop. This supervisor advances only durable research protocol state. */
export class ResearchSupervisor {
	private readonly job: ResearchJob;
	private readonly store: AstraStore;
	private readonly owner: string;
	private readonly maxParallel: number;
	private readonly worker: ResearchWorkerAdapter;
	private readonly reviewer: ResearchReviewerAdapter;
	private readonly mainAgent: ResearchMainAgentAdapter;
	private recovered = false;
	private mainDeliveryPending = false;
	private pauseCheck: Promise<void> = Promise.resolve();

	constructor(job: ResearchJob, store: AstraStore, options: SupervisorOptions) {
		this.job = job;
		this.store = store;
		this.owner = options.owner ?? `supervisor_${randomUUID()}`;
		this.maxParallel = options.maxParallel ?? 2;
		this.worker = options.worker;
		this.reviewer = options.reviewer;
		this.mainAgent = options.mainAgent;
	}

	async tick(): Promise<TickResult> {
		return this.store.withJobLock(this.job.state.frame.jobId, this.owner, async () => {
			await this.job.reload();
			await this.job.acquireLease(this.owner);
			try {
				return await this.tickLeased();
			} finally {
				await this.job.releaseLease(this.owner);
			}
		});
	}

	private async tickLeased(): Promise<TickResult> {
		this.mainDeliveryPending = false;
		await this.checkPauseRequests();
		try {
			await this.job.recoverMainAgentDeliveries();
			await this.job.recoverPendingOperations();
		} catch (error) {
			this.recovered = true;
			throw error;
		}
		const initialStageId = this.job.state.frame.activeStageId;
		if (this.job.state.frame.status === "completed") return this.result(initialStageId, [], false);
		const stage = this.job.state.stages[initialStageId];
		if (stage.status !== "running") throw new Error(`active research capability ${initialStageId} is not running`);
		if (this.shouldYield()) return this.result(initialStageId, [], false);
		const dispatchedTaskIds: string[] = [];
		if (await this.gateOnExhaustedContinue(initialStageId))
			return this.result(initialStageId, dispatchedTaskIds, false);
		if (await this.gateOnBudget(initialStageId)) return this.result(initialStageId, dispatchedTaskIds, false);

		for (const task of Object.values(this.job.state.tasks)) {
			if (
				task.stageId === initialStageId &&
				task.role === "worker" &&
				task.planId &&
				["ready", "running"].includes(task.status) &&
				(planReviewStatus(this.job, task.planId) !== "passed" || !this.taskMatchesFrozenPlan(task))
			) {
				await this.job.setTaskStatus(task.id, "blocked");
				continue;
			}
			if (task.stageId === initialStageId && task.role === "worker" && task.status === "running") {
				await this.job.setTaskStatus(task.id, "failed");
				this.recovered = true;
			}
		}

		const readyTasks = Object.values(this.job.state.tasks)
			.filter((task) => task.stageId === initialStageId && task.role === "worker" && task.status === "ready")
			.slice(0, this.maxParallel);
		if (readyTasks.length > 0) {
			if (await this.gateOnBudget(initialStageId, { turns: readyTasks.length })) {
				return this.result(initialStageId, dispatchedTaskIds, false);
			}
			await this.job.consumeTurns(readyTasks.length);
			const results = await Promise.allSettled(readyTasks.map((task) => this.runWorker(task)));
			await this.handleSettledErrors(results);
			if (this.shouldYield()) return this.result(initialStageId, dispatchedTaskIds, false);
		}

		let activeSearch = this.activeSearch(initialStageId);
		if (activeSearch) {
			const plan = this.job.state.stagePlans[activeSearch.planId];
			if (plan) await this.dispatchAndRun(plan, dispatchedTaskIds);
		} else if (this.shouldPlan(initialStageId)) {
			const obligation = this.openStageObligation(initialStageId);
			const requestedMode = obligation
				? "repair"
				: this.job.unsynthesizedLocalEvidence(initialStageId).length === 0 &&
						(stage.lastRouteAction === "search" || this.shouldStartConfiguredSearch(initialStageId))
					? "search"
					: "decompose";
			let plan = this.reusablePlan(initialStageId, obligation?.id);
			if (!plan && obligation) {
				if (
					this.job.state.frame.automation !== "full" &&
					this.job.definitions[initialStageId].gate === "user" &&
					!this.job.isRouteGateApproved(initialStageId)
				) {
					await this.job.requireUserGate({
						kind: "stage",
						stageId: initialStageId,
						phase: "route",
						reason: "Approve the repair strategy decision",
					});
					return this.result(initialStageId, dispatchedTaskIds, false);
				}
				if (await this.gateOnBudget(initialStageId, { turns: 1 }))
					return this.result(initialStageId, dispatchedTaskIds, false);
				await this.job.consumeTurns(1);
				try {
					const route = await this.callMainAgent(() => this.mainAgent.decideRoute(this.job, obligation));
					if (!["continue", "backtrack", "ask-user"].includes(route.routeAction ?? ""))
						throw new NonRetryableResearchError(
							"Open repair obligations require continue, backtrack, or ask-user",
						);
					await this.applyRouteDecision(route);
					if (
						(route.routeAction !== "continue" &&
							!(route.routeAction === "backtrack" && route.targetStageId === initialStageId)) ||
						this.shouldYield()
					)
						return this.result(
							initialStageId,
							dispatchedTaskIds,
							this.job.state.frame.activeStageId !== initialStageId,
						);
				} catch (error) {
					await this.handleAdapterError(error);
					return this.result(initialStageId, dispatchedTaskIds, false);
				}
			}
			if (!plan) {
				if (await this.gateOnBudget(initialStageId, { turns: 1 })) {
					return this.result(initialStageId, dispatchedTaskIds, false);
				}
				await this.job.consumeTurns(1);
				try {
					const generationSnapshot = this.job.state;
					plan = await this.callMainAgent(() => this.mainAgent.planStage(this.job, obligation, requestedMode));
					try {
						plan = await this.job.recordStagePlan(plan, generationSnapshot);
					} catch (error) {
						throw new NonRetryableResearchError(
							`stage plan ${plan.id} rejected: ${error instanceof Error ? error.message : String(error)}`,
						);
					}
				} catch (error) {
					await this.handleAdapterError(error);
					return this.result(initialStageId, dispatchedTaskIds, false);
				}
			}
			await this.dispatchAndRun(plan, dispatchedTaskIds);
		}
		if (this.shouldYield()) return this.result(initialStageId, dispatchedTaskIds, false);

		await this.reviewPendingEvidence(initialStageId);
		if (this.shouldYield()) return this.result(initialStageId, dispatchedTaskIds, false);
		activeSearch = this.activeSearch(initialStageId);
		if (activeSearch) {
			await this.decideSearch(activeSearch, initialStageId);
			if (this.shouldYield()) return this.result(initialStageId, dispatchedTaskIds, false);
		}
		await this.promoteReviewedEvidence(initialStageId);
		if (this.shouldYield()) return this.result(initialStageId, dispatchedTaskIds, false);

		if (await this.gateOnBudget(initialStageId)) return this.result(initialStageId, dispatchedTaskIds, false);
		if (this.canRoute(initialStageId)) {
			const definition = this.job.definitions[initialStageId];
			const needsUserGate = this.job.state.frame.automation !== "full" && definition.gate === "user";
			if (needsUserGate && !this.job.isRouteGateApproved(initialStageId)) {
				await this.job.requireUserGate({
					kind: "stage",
					stageId: initialStageId,
					phase: "route",
					reason: `Approve the next research-route decision from ${initialStageId}`,
				});
				return this.result(initialStageId, dispatchedTaskIds, false);
			}
			if (await this.gateOnBudget(initialStageId, { turns: 1 })) {
				return this.result(initialStageId, dispatchedTaskIds, false);
			}
			await this.job.consumeTurns(1);
			try {
				await this.applyRouteDecision(await this.callMainAgent(() => this.mainAgent.decideRoute(this.job)));
			} catch (error) {
				await this.handleAdapterError(error);
			}
		}

		return this.result(initialStageId, dispatchedTaskIds, this.job.state.frame.activeStageId !== initialStageId);
	}

	private shouldYield(): boolean {
		const { paused, providerBackoff: backoff } = this.job.modelControl();
		return this.mainDeliveryPending || paused || Boolean(backoff && Date.parse(backoff.retryAt) > Date.now());
	}

	private async applyRouteDecision(manifest: MainAgentDecisionManifest): Promise<void> {
		try {
			await this.job.applyRouteDecision(manifest);
		} catch (error) {
			throw new NonRetryableResearchError(
				`route decision ${manifest.decisionRef} rejected: ${error instanceof Error ? error.message : String(error)}`,
			);
		}
	}

	private checkPauseRequests(): Promise<void> {
		const check = this.pauseCheck.then(() => applyPendingPauses(this.job));
		this.pauseCheck = check.catch(() => undefined);
		return check;
	}

	private shouldPlan(stageId: string): boolean {
		if (this.activeSearch(stageId)) return false;
		if (
			Object.values(this.job.state.tasks).some(
				(task) => task.stageId === stageId && task.role === "worker" && ["ready", "running"].includes(task.status),
			)
		) {
			return false;
		}
		if (this.pendingEvidence(stageId).length > 0) return false;
		if (this.openStageObligation(stageId)) return true;
		if (this.job.unsynthesizedLocalEvidence(stageId).length > 0) return true;
		const stage = this.job.state.stages[stageId];
		const continuedPlanId = this.job.state.routeDecisions[stage.lastRouteDecisionRef ?? ""]?.continuedPlanId;
		if (continuedPlanId) return this.reusablePlan(stageId)?.id === continuedPlanId;
		const routeArtifactId = this.job.state.canonicalRoute.stageArtifactIds[stageId];
		const negative = this.terminalSearchFailure(stageId);
		if (!routeArtifactId)
			return (
				!negative ||
				(stage.lastRouteAction === "continue" &&
					this.job.state.routeDecisions[stage.lastRouteDecisionRef ?? ""]?.negativeSearchBatchId === negative.id)
			);
		return (
			["continue", "search"].includes(stage.lastRouteAction ?? "") && stage.lastRoutedArtifactId === routeArtifactId
		);
	}

	private async gateOnExhaustedContinue(stageId: string): Promise<boolean> {
		const state = this.job.state;
		const route = state.routeDecisions[state.stages[stageId].lastRouteDecisionRef ?? ""];
		const plan = state.stagePlans[route?.continuedPlanId ?? ""];
		if (!plan || route.action !== "continue" || !route.negativeSearchBatchId || !route.continuedPlanId) return false;
		if (this.shouldYield() || state.frame.userGate || this.openStageObligation(stageId)) return false;
		if (
			Object.values(state.tasks).some(
				(task) => task.stageId === stageId && ["ready", "running"].includes(task.status),
			)
		)
			return false;
		if (route.negativeSearchBatchId !== this.terminalSearchFailure(stageId)?.id) return false;
		if (
			!plan.tasks.every((planned) => {
				const latest = Object.values(state.tasks)
					.filter((task) => task.replayKey === `stage-plan:${plan.id}:${planned.key}`)
					.sort((left, right) => right.attempt - left.attempt)[0];
				return (
					latest?.status === "failed" &&
					latest.attempt >= MAX_TASK_ATTEMPTS &&
					!Object.values(state.evidence).some((evidence) => evidence.taskId === latest.id)
				);
			})
		)
			return false;
		await this.job.requireUserGate({
			kind: "research",
			stageId,
			decisionRef: route.id,
			planId: plan.id,
			question:
				"Final search found no qualified result and the additional ordinary plan exhausted its execution attempts. Provide a new direction or choose a backtrack.",
			reason: `Search continuation ${route.id}, plan ${plan.id}: execution attempts exhausted`,
		});
		return true;
	}

	private shouldStartConfiguredSearch(stageId: string): boolean {
		if (this.job.finalSearchBatch(stageId)) return false;
		if (!this.job.definitions[stageId].searchPolicy) return false;
		const latest = Object.values(this.job.state.searchBatches)
			.filter(
				(batch) =>
					batch.stageId === stageId &&
					batch.status !== "superseded" &&
					hasFrozenPlanContract(this.job, batch.planId) &&
					planReviewStatus(this.job, batch.planId) !== "stale",
			)
			.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
		return !latest || (latest.status === "exhausted" && latest.round < latest.maxRounds);
	}

	private evidenceFromStalePlan(evidence: Evidence): boolean {
		return !evidenceHasCurrentPlanApproval(this.job, evidence);
	}

	private taskMatchesFrozenPlan(task: TaskPacket): boolean {
		if (!task.planId) return true;
		if (!hasFrozenPlanContract(this.job, task.planId)) return false;
		const plan = this.job.state.stagePlans[task.planId];
		const planned = plan?.tasks.find((item) => task.replayKey === `stage-plan:${task.planId}:${item.key}`);
		return Boolean(planned && taskContractMatches(task, buildEffectiveTaskContract(this.job, plan!, planned)));
	}

	private pendingEvidence(stageId: string): Evidence[] {
		const snapshot = this.job.state;
		return Object.values(snapshot.evidence).filter((evidence) => {
			if (
				evidence.type === "stage-plan" ||
				evidence.stageId !== stageId ||
				evidence.status === "rejected" ||
				!evidenceHasCurrentPlanApprovalFromSnapshot(snapshot, evidence)
			)
				return false;
			const task = snapshot.tasks[evidence.taskId];
			const reviews = Object.values(snapshot.reviews).filter((review) => review.evidenceId === evidence.id);
			if (reviews.length === 0) return true;
			if (task?.searchBatchId) {
				const batch = snapshot.searchBatches[task.searchBatchId];
				if (batch?.status !== "selected") {
					return !Object.values(snapshot.candidateEvaluations).some(
						(evaluation) => evaluation.evidenceId === evidence.id,
					);
				}
				if (batch.selectedCandidateId !== task.searchCandidateId) return false;
			}
			if (evidence.status === "candidate") {
				if (reviews.some((review) => review.verdict !== "pass")) return false;
				return true;
			}
			if (task?.deliveryKind === "local") return false;
			return (
				evidence.status === "accepted" &&
				!Object.values(snapshot.canonical).some((artifact) => artifact.evidenceId === evidence.id)
			);
		});
	}

	private openStageObligation(stageId: string): Obligation | undefined {
		const state = this.job.state;
		const transferredIssueIds = new Set(
			Object.values(state.evidence).flatMap((evidence) => {
				const task = state.tasks[evidence.taskId];
				return evidence.stageId === stageId && evidence.status === "accepted" && task?.deliveryKind === "local"
					? (task.responsibilityTransfers ?? []).flatMap((transfer) =>
							transfer.issueId ? [transfer.issueId] : [],
						)
					: [];
			}),
		);
		const open = state.frame.openObligationIds.flatMap((id) => {
			const obligation = state.obligations[id];
			const review = obligation ? state.reviews[obligation.sourceReviewId] : undefined;
			const evidence = review ? state.evidence[review.evidenceId] : undefined;
			const openItems = obligation?.items?.filter((item) => item.status === "open") ?? [];
			return obligation?.status === "open" &&
				evidence?.stageId === stageId &&
				(!obligation.items?.length || openItems.some((item) => !transferredIssueIds.has(item.id)))
				? [{ obligation, evidence }]
				: [];
		});
		let latest = open[0];
		if (!latest) return undefined;
		const lineage = latest.evidence.currentEvidenceSetId ?? latest.evidence.taskId;
		for (const candidate of open) {
			if (
				(candidate.evidence.currentEvidenceSetId ?? candidate.evidence.taskId) === lineage &&
				candidate.evidence.createdAt >= latest.evidence.createdAt
			)
				latest = candidate;
		}
		return latest.obligation;
	}

	private activeSearch(stageId: string): SearchBatch | undefined {
		const snapshot = this.job.state;
		return Object.values(snapshot.searchBatches)
			.filter(
				(batch) =>
					batch.stageId === stageId &&
					["planning", "running", "evaluating"].includes(batch.status) &&
					hasFrozenPlanContractFromSnapshot(snapshot, batch.planId) &&
					planReviewStatusFromSnapshot(snapshot, this.job.definitions, batch.planId) !== "stale",
			)
			.sort((left, right) => right.createdAt.localeCompare(left.createdAt))[0];
	}

	private terminalSearchFailure(stageId: string): SearchBatch | undefined {
		return this.job.finalSearchBatch(stageId);
	}

	private async dispatchAndRun(plan: StagePlanManifest, dispatchedTaskIds: string[]): Promise<void> {
		if (this.shouldYield()) return;
		if (
			!hasFrozenPlanContract(this.job, plan.id) &&
			plan.generationBasisHash &&
			plan.generationBasisHash !== planGenerationBasisHash(this.job.state, this.job.definitions, plan)
		)
			return;
		if (planReviewStatus(this.job, plan.id) !== "passed") {
			if (["failed", "stale"].includes(planReviewStatus(this.job, plan.id))) return;
			if (await this.gateOnBudget(plan.stageId, { tasks: planEvidence(this.job, plan.id) ? 0 : 2, turns: 1 }))
				return;
			let reviewerTaskId: string | undefined;
			let evidence: Evidence | undefined;
			try {
				evidence = await preparePlanEvidence(this.job, plan);
				await this.job.consumeTurns(1);
				const target = evidence;
				const result = await this.callAdapter(() => this.reviewer.review(target, this.job));
				reviewerTaskId = result.reviewerTaskId;
				await this.job.recordReview({ ...result, evidenceId: evidence.id, blocking: false });
				if (result.reviewerTaskId) await this.job.setTaskStatus(result.reviewerTaskId, "succeeded");
			} catch (error) {
				if (!(await this.recoverReviewBeforeError(error, evidence?.id, reviewerTaskId))) return;
			}
			if (planReviewStatus(this.job, plan.id) !== "passed") return;
		}
		if (this.shouldYield()) return;
		const plannedTasks = await this.dispatchPlan(plan);
		for (const task of plannedTasks) {
			if (["ready", "running"].includes(task.status)) dispatchedTaskIds.push(task.id);
		}
		const runnable = plannedTasks
			.filter((task) => ["ready", "running"].includes(task.status))
			.slice(0, this.maxParallel);
		if (runnable.length === 0) return;
		if (await this.gateOnBudget(plan.stageId, { turns: runnable.length })) return;
		await this.job.consumeTurns(runnable.length);
		const results = await Promise.allSettled(runnable.map((task) => this.runWorker(task)));
		await this.handleSettledErrors(results);
	}

	private async reviewPendingEvidence(stageId: string): Promise<void> {
		for (const evidence of Object.values(this.job.state.evidence)) {
			if (
				evidence.type === "stage-plan" ||
				evidence.stageId !== stageId ||
				evidence.status === "rejected" ||
				this.evidenceFromStalePlan(evidence)
			)
				continue;
			const reviews = Object.values(this.job.state.reviews).filter((review) => review.evidenceId === evidence.id);
			const sourceTask = this.job.state.tasks[evidence.taskId];
			if (sourceTask?.searchBatchId && sourceTask.searchCandidateId) {
				if (
					!["planning", "running", "evaluating"].includes(
						this.job.state.searchBatches[sourceTask.searchBatchId]?.status ?? "",
					)
				)
					continue;
				for (const review of reviews) {
					if (
						review.targetVersionHash !== evidence.versionHash ||
						!this.job.state.searchBatches[sourceTask.searchBatchId]?.criteria.every((criterion) =>
							review.criteria?.some((item) => item.criterion === criterion),
						)
					)
						continue;
					await this.job.recordCandidateEvaluationFromReview(review.id);
				}
			}
			if (!sourceTask?.searchBatchId && reviews.some((review) => review.verdict !== "pass")) continue;
			const qualityPolicy = this.job.definitions[stageId]?.qualityPolicy;
			const requiredPassingReviews = qualityPolicy?.minPassingReviews ?? 1;
			if (
				sourceTask?.searchBatchId &&
				this.job
					.searchQualification(sourceTask.searchBatchId)
					.candidates.find((candidate) => candidate.candidateId === sourceTask.searchCandidateId)?.ready
			)
				continue;
			if (
				reviews.filter(
					(review) => review.verdict === "pass" && (review.score ?? 0) >= (qualityPolicy?.minScore ?? 0.8),
				).length >= requiredPassingReviews
			) {
				continue;
			}
			if (await this.gateOnBudget(stageId, { turns: 1 })) return;
			await this.job.consumeTurns(1);
			let reviewerTaskId: string | undefined;
			try {
				const verdict = await this.callAdapter(() => this.reviewer.review(evidence, this.job));
				reviewerTaskId = verdict.reviewerTaskId;
				const sourceTask = this.job.state.tasks[evidence.taskId];
				const review = await this.job.recordReview({
					evidenceId: evidence.id,
					targetVersionHash: verdict.targetVersionHash,
					reviewerTaskId: verdict.reviewerTaskId,
					verdict: verdict.verdict,
					findings: verdict.findings,
					score: verdict.score,
					criteria: verdict.criteria,
					verifiedRefs: verdict.verifiedRefs,
					blocking: sourceTask?.searchBatchId === undefined,
				});
				if (reviewerTaskId) await this.job.setTaskStatus(reviewerTaskId, "succeeded");
				if (sourceTask?.searchBatchId && sourceTask.searchCandidateId) {
					await this.job.recordCandidateEvaluationFromReview(review.id);
				}
			} catch (error) {
				await this.recoverReviewBeforeError(error, evidence.id, reviewerTaskId);
				if (this.shouldYield()) return;
			}
			if (await this.gateOnBudget(stageId)) return;
		}
	}

	private async recoverReviewBeforeError(
		error: unknown,
		evidenceId?: string,
		reviewerTaskId?: string,
	): Promise<boolean> {
		if (evidenceId) {
			const reviews = await this.job.recoverReviewerTaskCompletions(evidenceId, reviewerTaskId);
			if (reviews.length) {
				this.recovered = true;
				const task = this.job.state.tasks[this.job.state.evidence[evidenceId].taskId];
				if (task.searchBatchId && task.searchCandidateId)
					for (const review of reviews) await this.job.recordCandidateEvaluationFromReview(review.id);
				return true;
			}
		}
		if (reviewerTaskId) await this.job.failUncommittedReviewerTask(reviewerTaskId);
		await this.handleAdapterError(error);
		return false;
	}

	private async decideSearch(batch: SearchBatch, stageId: string): Promise<void> {
		const evaluations = Object.values(this.job.state.candidateEvaluations).filter(
			(evaluation) => evaluation.batchId === batch.id,
		);
		const qualification = this.job.searchQualification(batch.id);
		if (!qualification.ready) return;
		if (!qualification.eligibleIds.length && batch.round >= batch.maxRounds) {
			await this.job.exhaustNegativeSearch(batch.id);
			return;
		}
		if (await this.gateOnBudget(stageId, { turns: 1 })) return;
		await this.job.consumeTurns(1);
		try {
			const decision = await this.callMainAgent(() => this.mainAgent.decideSearch(batch, evaluations, this.job));
			if (decision.selectedCandidateId) {
				await this.job.selectSearchCandidate(batch.id, decision.selectedCandidateId, decision.decisionRef);
			} else if (decision.continueSearch) {
				await this.job.continueSearchBatch(batch.id, decision.decisionRef, decision.rationale);
			} else {
				throw new Error("main-agent search decision must select a candidate or continue search");
			}
		} catch (error) {
			await this.handleAdapterError(error);
		}
	}

	private async promoteReviewedEvidence(stageId: string): Promise<void> {
		for (const evidence of Object.values(this.job.state.evidence)) {
			if (
				evidence.type === "stage-plan" ||
				evidence.stageId !== stageId ||
				evidence.status === "rejected" ||
				this.evidenceFromStalePlan(evidence)
			)
				continue;
			const sourceTask = this.job.state.tasks[evidence.taskId];
			if (sourceTask?.searchBatchId) {
				const batch = this.job.state.searchBatches[sourceTask.searchBatchId];
				if (batch?.status !== "selected" || batch.selectedCandidateId !== sourceTask.searchCandidateId) continue;
			}
			const policy = this.job.definitions[stageId]?.qualityPolicy;
			const passingReviews = Object.values(this.job.state.reviews).filter(
				(review) =>
					review.evidenceId === evidence.id &&
					review.verdict === "pass" &&
					(review.score ?? 0) >= (policy?.minScore ?? 0.8),
			);
			if (passingReviews.length < (policy?.minPassingReviews ?? 1)) continue;
			if (this.job.state.evidence[evidence.id]?.status === "candidate") {
				if (await this.gateOnBudget(stageId, { turns: 1 })) return;
				await this.job.consumeTurns(1);
				try {
					const decision = await this.callMainAgent(() => this.mainAgent.decideEvidence(evidence, this.job));
					if (decision.decision === "defer") {
						throw new NonRetryableResearchError(
							`evidence ${evidence.id} deferred by ${decision.decisionRef}: ${decision.rationale}; resume with guidance to replan before reconsidering unchanged evidence`,
						);
					}
					await this.job.decideEvidence(evidence.id, decision.decision === "accept", decision.decisionRef);
				} catch (error) {
					await this.handleAdapterError(error);
					if (this.shouldYield()) return;
					continue;
				}
			}
			if (this.job.state.evidence[evidence.id]?.status !== "accepted") continue;
			if (sourceTask?.deliveryKind === "local") continue;
			if (Object.values(this.job.state.canonical).some((artifact) => artifact.evidenceId === evidence.id)) continue;
			if (await this.gateOnBudget(stageId, { turns: 1 })) return;
			await this.job.consumeTurns(1);
			let replacementOf: string | undefined;
			try {
				const decision = await this.callMainAgent(() => this.mainAgent.decideAdoption(evidence, this.job));
				if (!decision.adopt) {
					if (this.job.state.mainAgentCalls?.[decision.decisionRef]?.applied) return;
					await this.job.pause(
						`evidence ${evidence.id} not adopted by ${decision.decisionRef}: ${decision.rationale}; resume with guidance to replan before reconsidering unchanged evidence`,
					);
					return;
				}
				replacementOf = decision.replacementOf;
			} catch (error) {
				await this.handleAdapterError(error);
				if (this.shouldYield()) return;
				continue;
			}
			await this.job.adoptEvidence(evidence.id, replacementOf);
		}
	}

	private canRoute(stageId: string): boolean {
		if (this.job.unsynthesizedLocalEvidence(stageId).length > 0) return false;
		if (this.openStageObligation(stageId) || this.activeSearch(stageId) || this.pendingEvidence(stageId).length > 0) {
			return false;
		}
		const artifactId = this.job.state.canonicalRoute.stageArtifactIds[stageId];
		const negative = this.terminalSearchFailure(stageId);
		if (
			negative &&
			this.job.state.routeDecisions[this.job.state.stages[stageId].lastRouteDecisionRef ?? ""]
				?.negativeSearchBatchId !== negative.id
		)
			return true;
		if (!artifactId) return false;
		return this.job.state.stages[stageId].lastRoutedArtifactId !== artifactId;
	}

	private async handleSettledErrors(results: PromiseSettledResult<void>[]): Promise<void> {
		for (const result of results) {
			if (result.status === "rejected") await this.handleAdapterError(result.reason);
		}
	}

	private async handleAdapterError(error: unknown): Promise<void> {
		if (error instanceof ResearchPausedError) return;
		this.recovered = true;
		if (error instanceof ResearchTaskBudgetError) {
			await this.gateOnBudget(this.job.state.frame.activeStageId, { tasks: 1 });
			return;
		}
		if (error instanceof ProviderCapacityError) {
			const current = this.job.state.providerBackoff;
			if (current && Date.parse(current.retryAt) > Date.now()) return;
			const attempt = (current?.attempt ?? 0) + 1;
			const delayMs = Math.min(PROVIDER_BACKOFF_BASE_MS * 2 ** (attempt - 1), PROVIDER_BACKOFF_MAX_MS);
			const startedAt = new Date().toISOString();
			await this.job.recordProviderBackoff({
				attempt,
				reason: error.message,
				startedAt,
				retryAt: new Date(Date.now() + delayMs).toISOString(),
			});
			return;
		}
		if (error instanceof NonRetryableResearchError && !this.job.state.paused) {
			await this.job.pause(`infrastructure failure: ${error.message}`);
		}
	}

	private async callMainAgent<T extends StagePlanManifest | MainAgentDecisionManifest>(
		run: () => Promise<T>,
	): Promise<T> {
		const before = new Set(Object.keys(this.job.state.mainAgentCalls ?? {}));
		let result: T | undefined;
		let failure: unknown;
		try {
			result = await this.callAdapter(run, true);
		} catch (error) {
			failure = error;
		}
		const ids = Object.keys(this.job.state.mainAgentCalls ?? {}).filter((id) => !before.has(id));
		if (ids.length) {
			try {
				const recovered = await this.job.recoverMainAgentDeliveries(ids);
				if (recovered.length) result = recovered.at(-1) as T;
			} catch (error) {
				// An appended domain event may already authorize its unfinished tail.
				try {
					const recovered = await this.job.recoverMainAgentDeliveries(ids);
					if (recovered.length) result = recovered.at(-1) as T;
				} catch {
					this.mainDeliveryPending = true;
					throw new ResearchPausedError(
						`saved main-agent delivery remains pending: ${error instanceof Error ? error.message : String(error)}`,
					);
				}
			}
			if (!result) for (const id of ids) await this.job.abandonMainAgentCall(id);
		}
		if (this.job.modelControl().paused && !ids.some((id) => this.job.state.mainAgentCalls?.[id]?.applied))
			throw new ResearchPausedError("research paused during main-agent call; saved delivery awaits permission");
		if (result) return result;
		if (failure instanceof ProviderCapacityError) await this.job.refundTurns(1);
		throw failure;
	}

	private async callAdapter<T>(run: () => Promise<T>, deferCapacityRefund = false): Promise<T> {
		let executed = false;
		try {
			await this.checkPauseRequests();
			if (this.job.modelControl().paused) throw new ResearchPausedError("research paused before model call");
			executed = true;
			const result = await run();
			await this.job.clearProviderBackoff();
			await this.checkPauseRequests();
			if (this.job.modelControl().paused)
				throw new ResearchPausedError("research paused during model call; delivery retained");
			return result;
		} catch (error) {
			if (
				(error instanceof ProviderCapacityError && !deferCapacityRefund) ||
				error instanceof ResearchTaskBudgetError ||
				(error instanceof ResearchPausedError && !executed)
			)
				await this.job.refundTurns(1);
			throw error;
		}
	}

	private async gateOnBudget(stageId: string, additional: { tasks?: number; turns?: number } = {}): Promise<boolean> {
		const block = this.job.budgetBlock(additional);
		if (!block) return false;
		await this.job.requireUserGate({
			kind: "budget",
			stageId,
			limit: block.limit,
			reason: block.reason,
		});
		return true;
	}

	private result(stageId: string, dispatchedTaskIds: string[], routeChanged: boolean): TickResult {
		return {
			stageId,
			dispatchedTaskIds,
			completed: this.job.state.frame.status === "completed",
			routeChanged,
			recovered: this.recovered,
			paused: this.job.state.paused,
		};
	}

	private reusablePlan(stageId: string, obligationId?: string): StagePlanManifest | undefined {
		const snapshot = this.job.state;
		const continuedPlanId = !obligationId
			? snapshot.routeDecisions[snapshot.stages[stageId].lastRouteDecisionRef ?? ""]?.continuedPlanId
			: undefined;
		return Object.values(snapshot.stagePlans)
			.filter(
				(plan) =>
					plan.stageId === stageId &&
					(!continuedPlanId || plan.id === continuedPlanId) &&
					plan.obligationId === obligationId &&
					(hasFrozenPlanContractFromSnapshot(snapshot, plan.id)
						? !["failed", "stale"].includes(planReviewStatusFromSnapshot(snapshot, this.job.definitions, plan.id))
						: Boolean(
								plan.generationBasisHash &&
									plan.generationBasisHash === planGenerationBasisHash(snapshot, this.job.definitions, plan),
							)),
			)
			.sort((left, right) => right.createdAt.localeCompare(left.createdAt))
			.find((plan) =>
				plan.tasks.some((planned) => {
					const replayKey = `stage-plan:${plan.id}:${planned.key}`;
					const latest = Object.values(snapshot.tasks)
						.filter((task) => task.replayKey === replayKey)
						.sort((left, right) => right.attempt - left.attempt)[0];
					if (!latest || ["ready", "running"].includes(latest.status)) return true;
					return latest.status === "failed" && latest.attempt < MAX_TASK_ATTEMPTS;
				}),
			);
	}

	private async dispatchPlan(plan: StagePlanManifest): Promise<TaskPacket[]> {
		const batch = Object.values(this.job.state.searchBatches).find((candidate) => candidate.planId === plan.id);
		const missingTaskCount = plan.tasks.filter((planned) => {
			const replayKey = `stage-plan:${plan.id}:${planned.key}`;
			const latest = Object.values(this.job.state.tasks)
				.filter((task) => task.replayKey === replayKey)
				.sort((left, right) => right.attempt - left.attempt)[0];
			return !latest || (latest.status === "failed" && latest.attempt < MAX_TASK_ATTEMPTS);
		}).length;
		if (missingTaskCount > 0 && (await this.gateOnBudget(plan.stageId, { tasks: missingTaskCount }))) return [];
		const tasks = await Promise.all(
			plan.tasks.map(async (planned) => {
				const replayKey = `stage-plan:${plan.id}:${planned.key}`;
				const prior = Object.values(this.job.state.tasks)
					.filter((task) => task.replayKey === replayKey)
					.sort((left, right) => right.attempt - left.attempt)[0];
				const contract = buildEffectiveTaskContract(this.job, plan, planned);
				const searchCandidate = batch
					? Object.values(batch.candidates).find((candidate) => candidate.key === planned.key)
					: undefined;
				if (prior?.status === "failed" && prior.attempt >= MAX_TASK_ATTEMPTS) {
					if (batch && searchCandidate) await this.job.markSearchCandidateFailed(batch.id, searchCandidate.id);
					return undefined;
				}
				const attempt = prior?.status === "failed" ? prior.attempt + 1 : (prior?.attempt ?? 1);
				const taskId = `task_${checksum({ planId: plan.id, taskKey: planned.key, attempt }).slice(0, 24)}`;
				return this.job.dispatchTask({
					planId: plan.id,
					effectiveContractHash: semanticContractHash(contract),
					responsibilityBindings: contract.responsibilityBindings,
					responsibilityTransfers: contract.responsibilityTransfers,
					repairChecks: contract.repairChecks,
					id: taskId,
					attempt,
					replayKey,
					stageId: contract.stageId,
					stageExecutionId:
						this.job.state.stages[contract.stageId].executionId ?? `stage_exec_${contract.stageId}`,
					role: "worker",
					deliveryKind: contract.deliveryKind,
					repairOfEvidenceId: contract.repairOfEvidenceId,
					objective: contract.objective,
					inputArtifactRefs: contract.inputArtifactRefs,
					requiredCanonicalArtifacts: contract.requiredCanonicalArtifacts,
					requiredOutputType: contract.requiredOutputType,
					requiredOutputFields: contract.requiredOutputFields,
					acceptanceChecks: contract.acceptanceChecks,
					failureSignals: contract.failureSignals,
					dependencies: [],
					scope: { workspaceRoot: this.job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
					allowedTools: contract.allowedTools,
					writeAuthority: contract.writeAuthority,
					budget: contract.budget,
					reviewGateRequired: contract.reviewGateRequired,
					resumePolicy: contract.resumePolicy,
					successCriteria: contract.successCriteria,
					...(contract.searchBatchId ? { searchBatchId: contract.searchBatchId } : {}),
					...(contract.searchCandidateId ? { searchCandidateId: contract.searchCandidateId } : {}),
					...(prior?.status === "failed" ? { supersedesTaskId: prior.id } : {}),
				});
			}),
		);
		return tasks.filter((task): task is TaskPacket => task !== undefined);
	}

	private async runWorker(task: TaskPacket): Promise<void> {
		await this.job.setTaskStatus(task.id, "running");
		let outputReturned = false;
		try {
			await this.callAdapter(async () => {
				const output = await this.worker.run(task, this.job);
				outputReturned = true;
				await this.job.completeWorkerTask(task.id, output);
			}, true);
		} catch (error) {
			try {
				if (await this.job.recoverWorkerTaskCompletion(task.id)) {
					await this.job.clearProviderBackoff();
					return;
				}
			} catch (recoveryError) {
				throw new NonRetryableResearchError(
					`worker failed: ${error instanceof Error ? error.message : String(error)}; output recovery failed: ${recoveryError instanceof Error ? recoveryError.message : String(recoveryError)}`,
				);
			}
			if (outputReturned)
				throw new NonRetryableResearchError(
					`worker output completion could not be saved: ${error instanceof Error ? error.message : String(error)}`,
				);
			if (error instanceof ProviderCapacityError) await this.job.refundTurns(1);
			await this.job.setTaskStatus(
				task.id,
				error instanceof ProviderCapacityError || error instanceof ResearchPausedError ? "ready" : "failed",
			);
			throw error;
		}
	}
}
