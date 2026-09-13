import { randomUUID } from "node:crypto";
import { checksum, MAX_TASK_ATTEMPTS, type ResearchJob } from "./research.ts";
import type { AstraStore } from "./store.ts";
import type {
	CandidateEvaluation,
	CriterionAssessment,
	Evidence,
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
}

export interface ReviewerRunResult {
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
	decideRoute(job: ResearchJob): Promise<MainAgentDecisionManifest>;
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
		const initialStageId = this.job.state.frame.activeStageId;
		if (this.job.state.frame.status === "completed") return this.result(initialStageId, [], false);
		const stage = this.job.state.stages[initialStageId];
		if (stage.status !== "running") throw new Error(`active research capability ${initialStageId} is not running`);
		if (this.shouldYield()) return this.result(initialStageId, [], false);
		const dispatchedTaskIds: string[] = [];
		if (await this.gateOnBudget(initialStageId)) return this.result(initialStageId, dispatchedTaskIds, false);

		for (const task of Object.values(this.job.state.tasks)) {
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
				: stage.lastRouteAction === "search" || this.shouldStartConfiguredSearch(initialStageId)
					? "search"
					: "decompose";
			let plan = this.reusablePlan(initialStageId, obligation?.id);
			if (!plan) {
				if (await this.gateOnBudget(initialStageId, { turns: 1 })) {
					return this.result(initialStageId, dispatchedTaskIds, false);
				}
				await this.job.consumeTurns(1);
				try {
					plan = await this.callAdapter(() => this.mainAgent.planStage(this.job, obligation, requestedMode));
					await this.job.recordStagePlan(plan);
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
				await this.job.applyRouteDecision(await this.callAdapter(() => this.mainAgent.decideRoute(this.job)));
			} catch (error) {
				await this.handleAdapterError(error);
			}
		}

		return this.result(initialStageId, dispatchedTaskIds, this.job.state.frame.activeStageId !== initialStageId);
	}

	private shouldYield(): boolean {
		const backoff = this.job.state.providerBackoff;
		return this.job.state.paused || Boolean(backoff && Date.parse(backoff.retryAt) > Date.now());
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
		const stage = this.job.state.stages[stageId];
		const routeArtifactId = this.job.state.canonicalRoute.stageArtifactIds[stageId];
		if (!routeArtifactId) return !this.terminalSearchFailure(stageId);
		return (
			["continue", "search"].includes(stage.lastRouteAction ?? "") && stage.lastRoutedArtifactId === routeArtifactId
		);
	}

	private shouldStartConfiguredSearch(stageId: string): boolean {
		if (!this.job.definitions[stageId].searchPolicy) return false;
		const latest = Object.values(this.job.state.searchBatches)
			.filter((batch) => batch.stageId === stageId && batch.status !== "superseded")
			.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
		return !latest || (latest.status === "exhausted" && latest.round < latest.maxRounds);
	}

	private pendingEvidence(stageId: string): Evidence[] {
		return Object.values(this.job.state.evidence).filter((evidence) => {
			if (evidence.stageId !== stageId || evidence.status === "rejected") return false;
			const task = this.job.state.tasks[evidence.taskId];
			const reviews = Object.values(this.job.state.reviews).filter((review) => review.evidenceId === evidence.id);
			if (reviews.length === 0) return true;
			if (task?.searchBatchId) {
				const batch = this.job.state.searchBatches[task.searchBatchId];
				if (batch?.status !== "selected") {
					return !Object.values(this.job.state.candidateEvaluations).some(
						(evaluation) => evaluation.evidenceId === evidence.id,
					);
				}
				if (batch.selectedCandidateId !== task.searchCandidateId) return false;
			}
			if (evidence.status === "candidate") {
				if (reviews.some((review) => review.verdict !== "pass")) return false;
				return true;
			}
			return (
				evidence.status === "accepted" &&
				!Object.values(this.job.state.canonical).some((artifact) => artifact.evidenceId === evidence.id)
			);
		});
	}

	private openStageObligation(stageId: string): Obligation | undefined {
		return this.job.state.frame.openObligationIds
			.map((id) => this.job.state.obligations[id])
			.find((obligation) => {
				const review = obligation ? this.job.state.reviews[obligation.sourceReviewId] : undefined;
				const evidence = review ? this.job.state.evidence[review.evidenceId] : undefined;
				return obligation?.status === "open" && evidence?.stageId === stageId;
			});
	}

	private activeSearch(stageId: string): SearchBatch | undefined {
		return Object.values(this.job.state.searchBatches)
			.filter((batch) => batch.stageId === stageId && ["planning", "running", "evaluating"].includes(batch.status))
			.sort((left, right) => right.createdAt.localeCompare(left.createdAt))[0];
	}

	private terminalSearchFailure(stageId: string): SearchBatch | undefined {
		return Object.values(this.job.state.searchBatches)
			.filter(
				(batch) =>
					batch.stageId === stageId &&
					batch.status === "exhausted" &&
					batch.round >= batch.maxRounds &&
					Object.values(batch.candidates).length > 0 &&
					Object.values(batch.candidates).every((candidate) => candidate.status === "failed"),
			)
			.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
	}

	private async dispatchAndRun(plan: StagePlanManifest, dispatchedTaskIds: string[]): Promise<void> {
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
			if (evidence.stageId !== stageId || evidence.status === "rejected") continue;
			const reviews = Object.values(this.job.state.reviews).filter((review) => review.evidenceId === evidence.id);
			if (reviews.some((review) => review.verdict !== "pass")) continue;
			const qualityPolicy = this.job.definitions[stageId]?.qualityPolicy;
			const requiredPassingReviews = qualityPolicy?.minPassingReviews ?? 1;
			if (
				reviews.filter(
					(review) => review.verdict === "pass" && (review.score ?? 0) >= (qualityPolicy?.minScore ?? 0.8),
				).length >= requiredPassingReviews
			) {
				continue;
			}
			if (await this.gateOnBudget(stageId, { tasks: 1, turns: 1 })) return;
			await this.job.consumeTurns(1);
			let reviewerTaskId: string | undefined;
			let reviewCommitted = false;
			try {
				const verdict = await this.callAdapter(() => this.reviewer.review(evidence, this.job));
				reviewerTaskId = verdict.reviewerTaskId;
				const sourceTask = this.job.state.tasks[evidence.taskId];
				const review = await this.job.recordReview({
					evidenceId: evidence.id,
					reviewerTaskId: verdict.reviewerTaskId,
					verdict: verdict.verdict,
					findings: verdict.findings,
					score: verdict.score,
					criteria: verdict.criteria,
					verifiedRefs: verdict.verifiedRefs,
					blocking: sourceTask?.searchBatchId === undefined,
				});
				reviewCommitted = true;
				if (reviewerTaskId) await this.job.setTaskStatus(reviewerTaskId, "succeeded");
				if (sourceTask?.searchBatchId && sourceTask.searchCandidateId) {
					await this.job.recordCandidateEvaluation({
						batchId: sourceTask.searchBatchId,
						candidateId: sourceTask.searchCandidateId,
						evidenceId: evidence.id,
						reviewId: review.id,
						verdict: review.verdict,
						score: review.score ?? 0,
						criteria: review.criteria ?? [],
						findings: review.findings,
					});
				}
			} catch (error) {
				if (reviewerTaskId && !reviewCommitted) {
					await this.job.failUncommittedReviewerTask(reviewerTaskId);
				}
				await this.handleAdapterError(error);
				if (this.shouldYield()) return;
			}
			if (await this.gateOnBudget(stageId)) return;
		}
	}

	private async decideSearch(batch: SearchBatch, stageId: string): Promise<void> {
		const evaluations = Object.values(this.job.state.candidateEvaluations).filter(
			(evaluation) => evaluation.batchId === batch.id,
		);
		if (
			Object.values(batch.candidates).some(
				(candidate) =>
					candidate.status !== "failed" &&
					!evaluations.some((evaluation) => evaluation.candidateId === candidate.id),
			)
		) {
			return;
		}
		if (await this.gateOnBudget(stageId, { turns: 1 })) return;
		await this.job.consumeTurns(1);
		try {
			const decision = await this.callAdapter(() => this.mainAgent.decideSearch(batch, evaluations, this.job));
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
			if (evidence.stageId !== stageId || evidence.status === "rejected") continue;
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
					const decision = await this.callAdapter(() => this.mainAgent.decideEvidence(evidence, this.job));
					if (decision.decision !== "defer") {
						await this.job.decideEvidence(evidence.id, decision.decision === "accept", decision.decisionRef);
					}
				} catch (error) {
					await this.handleAdapterError(error);
					if (this.shouldYield()) return;
					continue;
				}
			}
			if (this.job.state.evidence[evidence.id]?.status !== "accepted") continue;
			if (Object.values(this.job.state.canonical).some((artifact) => artifact.evidenceId === evidence.id)) continue;
			if (await this.gateOnBudget(stageId, { turns: 1 })) return;
			await this.job.consumeTurns(1);
			try {
				const decision = await this.callAdapter(() => this.mainAgent.decideAdoption(evidence, this.job));
				if (decision.adopt) await this.job.adoptEvidence(evidence.id, decision.replacementOf);
			} catch (error) {
				await this.handleAdapterError(error);
				if (this.shouldYield()) return;
			}
		}
	}

	private canRoute(stageId: string): boolean {
		if (this.openStageObligation(stageId) || this.activeSearch(stageId) || this.pendingEvidence(stageId).length > 0) {
			return false;
		}
		const artifactId = this.job.state.canonicalRoute.stageArtifactIds[stageId];
		if (!artifactId) return Boolean(this.terminalSearchFailure(stageId));
		return this.job.state.stages[stageId].lastRoutedArtifactId !== artifactId;
	}

	private async handleSettledErrors(results: PromiseSettledResult<void>[]): Promise<void> {
		for (const result of results) {
			if (result.status === "rejected") await this.handleAdapterError(result.reason);
		}
	}

	private async handleAdapterError(error: unknown): Promise<void> {
		this.recovered = true;
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

	private async callAdapter<T>(run: () => Promise<T>): Promise<T> {
		try {
			const result = await run();
			await this.job.clearProviderBackoff();
			return result;
		} catch (error) {
			if (error instanceof ProviderCapacityError) await this.job.refundTurns(1);
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
		return Object.values(this.job.state.stagePlans)
			.filter((plan) => plan.stageId === stageId && plan.obligationId === obligationId)
			.sort((left, right) => right.createdAt.localeCompare(left.createdAt))
			.find((plan) =>
				plan.tasks.some((planned) => {
					const replayKey = `stage-plan:${plan.id}:${planned.key}`;
					const latest = Object.values(this.job.state.tasks)
						.filter((task) => task.replayKey === replayKey)
						.sort((left, right) => right.attempt - left.attempt)[0];
					if (!latest || ["ready", "running"].includes(latest.status)) return true;
					return latest.status === "failed" && latest.attempt < MAX_TASK_ATTEMPTS;
				}),
			);
	}

	private async dispatchPlan(plan: StagePlanManifest): Promise<TaskPacket[]> {
		const definition = this.job.definitions[plan.stageId];
		const backtrackChecks = this.job.state.graph.unresolvedObjectionIds.flatMap((id) => {
			const objection = this.job.state.graph.nodes[id];
			return objection.stageId === plan.stageId &&
				objection.domainRef === this.job.state.stages[plan.stageId].lastRouteDecisionRef
				? [objection.statement]
				: [];
		});
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
				const searchCandidate = batch
					? Object.values(batch.candidates).find((candidate) => candidate.key === planned.key)
					: undefined;
				if (prior?.status === "failed" && prior.attempt >= MAX_TASK_ATTEMPTS) {
					if (batch && searchCandidate) await this.job.markSearchCandidateFailed(batch.id, searchCandidate.id);
					return undefined;
				}
				const attempt = prior?.status === "failed" ? prior.attempt + 1 : (prior?.attempt ?? 1);
				const taskId = `task_${checksum({ planId: plan.id, taskKey: planned.key, attempt }).slice(0, 24)}`;
				const inputArtifactRefs = [
					...new Set([
						...planned.inputArtifactRefs,
						...(plan.stageId === "research-review"
							? Object.values(this.job.state.canonicalRoute.stageArtifactIds)
							: []),
					]),
				];
				return this.job.dispatchTask({
					id: taskId,
					attempt,
					replayKey,
					stageId: plan.stageId,
					stageExecutionId: this.job.state.stages[plan.stageId].executionId ?? `stage_exec_${plan.stageId}`,
					role: "worker",
					objective: planned.objective,
					inputArtifactRefs,
					requiredCanonicalArtifacts: inputArtifactRefs.filter(
						(ref) => this.job.state.canonical[ref]?.status === "active",
					),
					requiredOutputType: definition.outputArtifactType,
					requiredOutputFields: planned.requiredOutputFields,
					acceptanceChecks: [
						...new Set([
							...definition.acceptanceChecks,
							...planned.acceptanceChecks,
							...backtrackChecks,
							...(batch?.criteria ?? []),
						]),
					],
					failureSignals: [...new Set([...definition.failureSignals, ...planned.failureSignals])],
					dependencies: [],
					scope: { workspaceRoot: this.job.state.frame.permissions.workspaceRoot, allowedPaths: ["."] },
					allowedTools: definition.workerTools,
					writeAuthority: definition.workspaceWrite ? "workspace-write" : "none",
					budget: definition.workerBudget
						? { ...definition.workerBudget }
						: plan.stageId === "research-review"
							? { maxTurns: 12, maxToolCalls: 32, maxRuntimeMs: 300_000 }
							: definition.workspaceWrite
								? { maxTurns: 16, maxToolCalls: 32, maxRuntimeMs: 300_000 }
								: { maxTurns: 8, maxToolCalls: 16, maxRuntimeMs: 300_000 },
					reviewGateRequired: true,
					resumePolicy: "resume-session",
					successCriteria: planned.successCriteria,
					...(batch && searchCandidate ? { searchBatchId: batch.id, searchCandidateId: searchCandidate.id } : {}),
					...(prior?.status === "failed" ? { supersedesTaskId: prior.id } : {}),
				});
			}),
		);
		return tasks.filter((task): task is TaskPacket => task !== undefined);
	}

	private async runWorker(task: TaskPacket): Promise<void> {
		await this.job.setTaskStatus(task.id, "running");
		try {
			const output = await this.callAdapter(() => this.worker.run(task, this.job));
			await this.job.setTaskStatus(task.id, "succeeded");
			const stagePlan = Object.values(this.job.state.stagePlans).find((plan) =>
				plan.tasks.some((planned) => task.replayKey === `stage-plan:${plan.id}:${planned.key}`),
			);
			const obligation = stagePlan?.obligationId ? this.job.state.obligations[stagePlan.obligationId] : undefined;
			const failedReview = obligation ? this.job.state.reviews[obligation.sourceReviewId] : undefined;
			const failedEvidence = failedReview ? this.job.state.evidence[failedReview.evidenceId] : undefined;
			const priorEvidence = task.inputArtifactRefs
				.map((ref) => this.job.state.evidence[ref])
				.find((evidence) => evidence !== undefined);
			await this.job.recordEvidence({
				taskId: task.id,
				stageId: task.stageId,
				type: output.artifactType,
				content: output.content,
				refs: output.refs,
				currentEvidenceSetId: failedEvidence?.currentEvidenceSetId ?? priorEvidence?.currentEvidenceSetId,
			});
		} catch (error) {
			await this.job.setTaskStatus(task.id, error instanceof ProviderCapacityError ? "ready" : "failed");
			throw error;
		}
	}
}
