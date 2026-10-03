import { createHash, randomUUID } from "node:crypto";
import { access, lstat, mkdir, readFile, realpath, rm } from "node:fs/promises";
import { join, relative, resolve } from "node:path";
import { archiveAndPruneTasks, cleanupTaskFiles } from "./cleanup-files.ts";
import {
	assertAstraId,
	atomicWriteJson,
	canonicalArtifactPath,
	canonicalReceiptPath,
	readMainAgentDelivery,
	readWorkerOutputManifest,
	reviewerManifestPath,
	reviewSnapshotPath,
	sha256,
	taskDir,
	taskStageContract,
} from "./contracts.ts";
import {
	backtrackChecksFromSnapshot,
	buildEffectiveTaskContract,
	EffectiveContractUnavailableError,
	type EffectiveTaskContract,
	planGenerationBasisHash,
	repairContext,
	resolveRepairInputFromSnapshot,
	searchBatchForPlan,
	sourceTaskContractHash,
	taskContractMatches,
} from "./effective-contract.ts";
import { sourceReceiptFilename } from "./literature.ts";
import {
	evidenceHasCurrentPlanApprovalFromSnapshot,
	evidenceMatchesHistoricalPlanFromSnapshot,
	planEvidence as planReviewEvidence,
	planReviewStatus,
	taskHasBoundRepairAncestor,
	taskIsCurrentFromSnapshot,
} from "./plan-review.ts";
import { captureGitVersion } from "./project-version.ts";
import { classifyProviderErrorMessage } from "./provider-errors.ts";
import {
	addEdgeToGraph,
	addNodeToGraph,
	createResearchEdge,
	createResearchGraph,
	createResearchNode,
	updateNodeStatus,
} from "./research-graph.ts";
import {
	frozenReviewCriteria,
	ReviewAssessmentError,
	ReviewIntegrityError,
	validateReviewAssessment,
	validateReviewReferences,
} from "./review-validation.ts";
import { DEFAULT_STAGES, stageMap } from "./stages.ts";
import type { AstraStore } from "./store.ts";
import {
	freezeEvidenceFiles,
	readVersionedFile,
	taskInputResources,
	taskWorkspacePath,
	verifyReviewEvidenceBundle,
} from "./task-workspace.ts";
import type {
	AdoptionCompletion,
	AstraEvent,
	AutomationLevel,
	BudgetLimit,
	CandidateEvaluation,
	CanonicalArtifact,
	ChildSessionRecord,
	ClaimAssessment,
	CleanupIntent,
	DiscardedCandidateReceipt,
	DiscardedEvidenceReceipt,
	Evidence,
	EvidenceAcceptanceConsequences,
	EvidenceCompletion,
	JobSnapshot,
	Lease,
	MainAgentCall,
	MainAgentDecisionManifest,
	MissionCoverage,
	MissionFrame,
	Obligation,
	OutputRef,
	ProviderBackoffState,
	ResearchNode,
	RetiredArtifactReceipt,
	Review,
	ReviewConsequences,
	ReviewDeliveryRejection,
	ReviewerOutputManifest,
	ReviewPacket,
	RouteConsequences,
	ScientificOutcome,
	SearchBatch,
	StageDefinition,
	StagePlanManifest,
	StageReopening,
	StageRouteDecision,
	StageState,
	StoredEvent,
	TaskPacket,
	TaskStatus,
	TaskVersion,
	UserGateRequest,
	WorkerOutputManifest,
} from "./types.ts";

import { validateWorkerSubmission } from "./worker-submission.ts";

interface PreparedEvidenceCompletion {
	schemaVersion: "astra.evidence_completion.v1";
	task: Omit<TaskPacket, "status">;
	evidence: Evidence;
	completion: EvidenceCompletion;
	preparationHash: string;
}

const DEFAULT_SEARCH_MAX_ROUNDS = 2;
export const MAX_TASK_ATTEMPTS = 3;
export class StaleResearchJobError extends Error {}

export class StaleResearchInputError extends Error {}

export class ResearchTaskBudgetError extends Error {
	constructor() {
		super("research task budget exhausted");
		this.name = "ResearchTaskBudgetError";
	}
}
const SCIENTIFIC_OUTCOMES = new Set<ScientificOutcome>([
	"supported",
	"partially-supported",
	"refuted",
	"inconclusive",
	"insufficient-evidence",
]);
const MISSION_COVERAGE_VALUES = new Set<MissionCoverage>(["sufficient", "insufficient"]);

function normalizedLabel(value: unknown): string {
	return typeof value === "string"
		? value
				.trim()
				.toLocaleLowerCase()
				.replace(/[\s_]+/g, "-")
		: "";
}

function claimAssessment(value: unknown): ClaimAssessment {
	const normalized = normalizedLabel(value);
	if (normalized === "partially-supported" || normalized === "partial") return "partially-supported";
	if (normalized === "supported") return "supported";
	if (normalized === "refuted" || normalized === "contradicted") return "refuted";
	if (normalized === "unsupported") return "unsupported";
	return "unresolved";
}

export function validateClaimAssessments(content: unknown): void {
	if (content === null || typeof content !== "object" || Array.isArray(content)) return;
	const claims = (content as Record<string, unknown>).claims;
	if (claims === undefined) return;
	if (!Array.isArray(claims)) throw new Error("claims must be an array of objects with explicit assessment");
	for (const claim of claims) {
		const assessment =
			claim !== null && typeof claim === "object" && !Array.isArray(claim)
				? (claim as Record<string, unknown>).assessment
				: undefined;
		if (
			!["supported", "partially-supported", "refuted", "unsupported", "unresolved"].includes(
				normalizedLabel(assessment),
			)
		)
			throw new Error(
				"each claim requires an explicit assessment: supported, partially-supported, refuted, unsupported, or unresolved",
			);
	}
}

export function scientificAssessment(content: unknown):
	| {
			outcome: ScientificOutcome;
			missionCoverage: MissionCoverage;
			reason: string;
	  }
	| undefined {
	if (content === null || typeof content !== "object" || Array.isArray(content)) return undefined;
	const record = content as Record<string, unknown>;
	const outcome = normalizedLabel(record.scientificOutcome) as ScientificOutcome;
	const missionCoverage = normalizedLabel(record.missionCoverage) as MissionCoverage;
	if (!SCIENTIFIC_OUTCOMES.has(outcome) || !MISSION_COVERAGE_VALUES.has(missionCoverage)) return undefined;
	const reason =
		typeof record.conclusion === "string" ? record.conclusion : `${outcome}; mission coverage ${missionCoverage}`;
	return { outcome, missionCoverage, reason };
}

function stableJson(value: unknown): string {
	if (value === null || typeof value !== "object") return JSON.stringify(value);
	if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
	return `{${Object.entries(value as Record<string, unknown>)
		.sort(([left], [right]) => left.localeCompare(right))
		.map(([key, entry]) => `${JSON.stringify(key)}:${stableJson(entry)}`)
		.join(",")}}`;
}

export function checksum(value: unknown): string {
	return createHash("sha256").update(stableJson(value)).digest("hex");
}

function reviewBody(review: Omit<Review, "createdAt">): string {
	return checksum(JSON.parse(JSON.stringify(review)));
}

function checkReviewHistory(events: StoredEvent[]): Map<string, Review> {
	const reviews = new Map<string, Review>();
	for (const stored of events) {
		if (stored.event.type !== "review_recorded") continue;
		const review = stored.event.review;
		const existing = reviews.get(review.id);
		if (existing && reviewBody(existing) !== reviewBody(review))
			throw new Error(`review history integrity conflict for id ${review.id}`);
		reviews.set(review.id, existing ?? review);
	}
	return reviews;
}

export interface CreateJobOptions {
	jobId?: string;
	goalId?: string;
	objective: string;
	workspaceRoot: string;
	automation?: AutomationLevel;
	boundaries?: string[];
	acceptance?: string[];
	maxTasks?: number;
	maxTurns?: number;
	maxCostUsd?: number;
	allowDestructive?: boolean;
	allowedTools?: string[];
	requiredArtifactTypes?: string[];
	definitions?: StageDefinition[];
}

export interface JobStatus {
	progress: Array<{ stageId: string; status: StageState["status"]; artifactId?: string; invalidatedBy?: string }>;
	jobId: string;
	objective: string;
	status: MissionFrame["status"];
	automation: AutomationLevel;
	activeStageId: string;
	paused: boolean;
	openObligations: number;
	readyTasks: number;
	openQuestions: number;
	activeHypotheses: number;
	unresolvedObjections: number;
	activeSearches: number;
	scientificOutcome: ScientificOutcome;
	missionCoverage: MissionCoverage;
	requiredArtifactTypes: string[];
	missingRequiredArtifactTypes: string[];
	eventSeq: number;
	nextAction: string;
	userGate?: MissionFrame["userGate"];
	providerBackoff?: ProviderBackoffState;
	budget: MissionFrame["budget"] & { tasksUsed: number; turnsUsed: number; costUsdUsed: number };
}

export interface BudgetBlock {
	limit: BudgetLimit;
	reason: string;
}

function validateBudget(budget: MissionFrame["budget"]): void {
	if (!Number.isInteger(budget.maxTasks) || budget.maxTasks <= 0) {
		throw new Error("maxTasks must be a positive integer");
	}
	if (!Number.isInteger(budget.maxTurns) || budget.maxTurns <= 0) {
		throw new Error("maxTurns must be a positive integer");
	}
	if (budget.maxCostUsd !== undefined && (!Number.isFinite(budget.maxCostUsd) || budget.maxCostUsd < 0)) {
		throw new Error("maxCostUsd must be a non-negative number");
	}
}

function mainSessionId(jobId: string): string {
	return `astra-${jobId}-main`.replace(/[^A-Za-z0-9._-]/g, "-");
}

function normalizeSnapshot(snapshot: JobSnapshot): JobSnapshot {
	snapshot.cleanupIntents ??= {};
	snapshot.reviewDeliveryRejections ??= {};
	snapshot.stagePlans ??= {};
	snapshot.graph ??= createResearchGraph(snapshot.frame.jobId, snapshot.frame.objective);
	snapshot.searchBatches ??= {};
	snapshot.candidateEvaluations ??= {};
	snapshot.routeDecisions ??= {};
	snapshot.retiredArtifacts ??= {};
	snapshot.discardedCandidates ??= {};
	snapshot.discardedEvidence ??= {};
	snapshot.frame.status ??= snapshot.frame.nextAction === "research complete" ? "completed" : "running";
	snapshot.frame.requiredArtifactTypes ??= [];
	snapshot.frame.scientificOutcome ??= "pending";
	snapshot.frame.missionCoverage ??= "pending";
	snapshot.mainAgentSessionId ??= mainSessionId(snapshot.frame.jobId);
	for (const batch of Object.values(snapshot.searchBatches)) {
		batch.round ??= 1;
		batch.maxRounds ??= DEFAULT_SEARCH_MAX_ROUNDS;
	}
	snapshot.canonicalRoute ??= {
		id: "canonical",
		revision: 1,
		stageArtifactIds: Object.fromEntries(
			Object.values(snapshot.canonical)
				.filter((artifact) => artifact.status === "active")
				.flatMap((artifact) => {
					const stageId = snapshot.evidence[artifact.evidenceId]?.stageId;
					return stageId ? [[stageId, artifact.id]] : [];
				}),
		),
		selectedCandidateIds: {},
		updatedAt: snapshot.updatedAt,
	};
	for (const stage of Object.values(snapshot.stages)) stage.revision ??= 1;
	if (snapshot.frame.scientificOutcome === "pending") {
		const resultArtifact = Object.values(snapshot.canonical).find(
			(artifact) =>
				artifact.status === "active" && artifact.adoptionCompletedAt && artifact.type === "result-to-claim",
		);
		const assessment = scientificAssessment(resultArtifact?.content);
		if (assessment) {
			snapshot.frame.scientificOutcome = assessment.outcome;
			snapshot.frame.missionCoverage = assessment.missionCoverage;
			snapshot.frame.scientificOutcomeReason = assessment.reason;
		}
	}
	return snapshot;
}

function dependentEvidenceRefs(snapshot: JobSnapshot, initialRefs: Iterable<string>): Set<string> {
	const refs = new Set(initialRefs);
	let changed = true;
	while (changed) {
		changed = false;
		for (const evidence of Object.values(snapshot.evidence)) {
			if (refs.has(evidence.id) || !snapshot.tasks[evidence.taskId]?.inputArtifactRefs.some((ref) => refs.has(ref)))
				continue;
			refs.add(evidence.id);
			changed = true;
		}
		for (const artifact of Object.values(snapshot.canonical)) {
			if (refs.has(artifact.id) && !refs.has(artifact.evidenceId)) {
				refs.add(artifact.evidenceId);
				changed = true;
			}
			if (refs.has(artifact.evidenceId) && !refs.has(artifact.id)) {
				refs.add(artifact.id);
				changed = true;
			}
		}
	}
	return refs;
}

export class ResearchJob {
	private snapshot: JobSnapshot;
	private readonly store: AstraStore;
	readonly definitions: Record<string, StageDefinition>;
	private commitChain: Promise<void> = Promise.resolve();

	private constructor(snapshot: JobSnapshot, store: AstraStore, definitions: Record<string, StageDefinition>) {
		this.snapshot = snapshot;
		this.store = store;
		this.definitions = definitions;
	}

	static async create(store: AstraStore, options: CreateJobOptions): Promise<ResearchJob> {
		const definitions = stageMap(options.definitions ?? DEFAULT_STAGES);
		const jobId = options.jobId ?? `job_${randomUUID()}`;
		const firstStage = (options.definitions ?? DEFAULT_STAGES)[0]?.id ?? "validation";
		const budget = {
			maxTasks: options.maxTasks ?? 64,
			maxTurns: options.maxTurns ?? 256,
			maxCostUsd: options.maxCostUsd,
		};
		validateBudget(budget);
		const frame: MissionFrame = {
			goalId: options.goalId ?? `goal_${randomUUID()}`,
			jobId,
			objective: options.objective,
			boundaries: options.boundaries ?? [],
			automation: options.automation ?? "autonomous",
			budget,
			permissions: {
				allowedTools: options.allowedTools ?? [
					"read",
					"grep",
					"find",
					"ls",
					"write",
					"edit",
					"bash",
					"astra_search_papers",
				],
				workspaceRoot: options.workspaceRoot,
				allowDestructive: options.allowDestructive ?? false,
			},
			acceptance: options.acceptance ?? [
				"at least one claim is supported by reviewed evidence",
				"the whole-research review passes",
				"no blocking objection remains",
			],
			requiredArtifactTypes: [...new Set(options.requiredArtifactTypes ?? [])],
			status: "running",
			scientificOutcome: "pending",
			missionCoverage: "pending",
			activeStageId: firstStage,
			nextAction: `dispatch ${firstStage}`,
			openObligationIds: [],
		};
		const stages: Record<string, StageState> = Object.fromEntries(
			Object.keys(definitions).map((id) => [id, { definitionId: id, status: "pending" } satisfies StageState]),
		) as Record<string, StageState>;
		stages[firstStage].status = "running";
		const snapshot: JobSnapshot = {
			cleanupIntents: {},
			version: 1,
			stageDefinitions: structuredClone(definitions),
			frame,
			stages,
			stagePlans: {},
			tasks: {},
			evidence: {},
			reviews: {},
			obligations: {},
			canonical: {},
			retiredArtifacts: {},
			discardedCandidates: {},
			discardedEvidence: {},
			sessions: {},
			graph: createResearchGraph(jobId, options.objective),
			searchBatches: {},
			candidateEvaluations: {},
			routeDecisions: {},
			mainAgentSessionId: mainSessionId(jobId),
			canonicalRoute: {
				id: "canonical",
				revision: 1,
				stageArtifactIds: {},
				selectedCandidateIds: {},
				updatedAt: new Date().toISOString(),
			},
			budgetUsage: { turnsUsed: 0, costUsdUsed: 0 },
			paused: false,
			eventSeq: 0,
			updatedAt: new Date().toISOString(),
		};
		const job = new ResearchJob(snapshot, store, definitions);
		await job.commit({ type: "job_created", snapshot });
		return job;
	}

	static async open(store: AstraStore, jobId: string): Promise<ResearchJob | undefined> {
		let snapshot = await store.loadSnapshot(jobId);
		const events = await store.readEvents(jobId);
		checkReviewHistory(events);
		for (const [index, stored] of events.entries()) {
			if (stored.jobId !== jobId || stored.seq !== index + 1)
				throw new Error(`invalid research event sequence for ${jobId}`);
		}
		if (!snapshot && events[0]?.event.type === "job_created") snapshot = structuredClone(events[0].event.snapshot);
		if (!snapshot) return undefined;
		if (snapshot.eventSeq > events.length)
			throw new Error(`research snapshot is ahead of event sequence for ${jobId}`);
		const job = new ResearchJob(
			normalizeSnapshot(snapshot),
			store,
			structuredClone(snapshot.stageDefinitions ?? stageMap()),
		);
		for (const stored of events.slice(snapshot.eventSeq)) {
			job.apply(stored.event, stored.timestamp);
			job.snapshot.eventSeq = stored.seq;
			job.snapshot.updatedAt = stored.timestamp;
		}
		job.snapshot = normalizeSnapshot(job.snapshot);
		return job;
	}

	get state(): JobSnapshot {
		return structuredClone(this.snapshot);
	}

	modelControl(): { paused: boolean; providerBackoff?: ProviderBackoffState } {
		return { paused: this.snapshot.paused, providerBackoff: structuredClone(this.snapshot.providerBackoff) };
	}

	async reload(): Promise<void> {
		return this.enqueue(async () => {
			const latest = await ResearchJob.open(this.store, this.snapshot.frame.jobId);
			if (!latest) throw new Error(`research job not found: ${this.snapshot.frame.jobId}`);
			this.snapshot = latest.snapshot;
		});
	}

	status(): JobStatus {
		const budgetUsage = this.snapshot.budgetUsage ?? { turnsUsed: 0, costUsdUsed: 0 };
		const activeArtifactTypes = new Set(
			Object.values(this.snapshot.canonical)
				.filter((artifact) => artifact.status === "active")
				.map((artifact) => artifact.type),
		);
		return {
			jobId: this.snapshot.frame.jobId,
			progress: Object.entries(this.snapshot.stages).map(([stageId, stage]) => ({
				stageId,
				status: stage.status,
				artifactId: this.snapshot.canonicalRoute.stageArtifactIds[stageId],
				invalidatedBy: stage.invalidatedBy,
			})),
			objective: this.snapshot.frame.objective,
			status: this.snapshot.frame.status,
			automation: this.snapshot.frame.automation,
			activeStageId: this.snapshot.frame.activeStageId,
			paused: this.snapshot.paused,
			openObligations: this.snapshot.frame.openObligationIds.length,
			readyTasks: Object.values(this.snapshot.tasks).filter((task) => task.status === "ready").length,
			openQuestions: this.snapshot.graph.openQuestionIds.length,
			activeHypotheses: this.snapshot.graph.activeHypothesisIds.length,
			unresolvedObjections: this.snapshot.graph.unresolvedObjectionIds.length,
			activeSearches: Object.values(this.snapshot.searchBatches).filter((batch) =>
				["planning", "running", "evaluating"].includes(batch.status),
			).length,
			scientificOutcome: this.snapshot.frame.scientificOutcome,
			missionCoverage: this.snapshot.frame.missionCoverage,
			requiredArtifactTypes: [...this.snapshot.frame.requiredArtifactTypes],
			missingRequiredArtifactTypes: this.snapshot.frame.requiredArtifactTypes.filter(
				(type) => !activeArtifactTypes.has(type),
			),
			eventSeq: this.snapshot.eventSeq,
			nextAction: this.snapshot.frame.nextAction,
			userGate: structuredClone(this.snapshot.frame.userGate),
			providerBackoff: structuredClone(this.snapshot.providerBackoff),
			budget: {
				...this.snapshot.frame.budget,
				tasksUsed: Object.keys(this.snapshot.tasks).length,
				turnsUsed: budgetUsage.turnsUsed,
				costUsdUsed: budgetUsage.costUsdUsed,
			},
		};
	}

	budgetBlock(additional: { tasks?: number; turns?: number } = {}): BudgetBlock | undefined {
		const tasks = Object.keys(this.snapshot.tasks).length;
		const usage = this.snapshot.budgetUsage ?? { turnsUsed: 0, costUsdUsed: 0 };
		const additionalTasks = additional.tasks ?? 0;
		const additionalTurns = additional.turns ?? 0;
		if (tasks + additionalTasks > this.snapshot.frame.budget.maxTasks) {
			return {
				limit: "maxTasks",
				reason: `task budget requires ${tasks + additionalTasks}, limit is ${this.snapshot.frame.budget.maxTasks}`,
			};
		}
		if (usage.turnsUsed + additionalTurns > this.snapshot.frame.budget.maxTurns) {
			return {
				limit: "maxTurns",
				reason: `turn budget requires ${usage.turnsUsed + additionalTurns}, limit is ${this.snapshot.frame.budget.maxTurns}`,
			};
		}
		const maxCostUsd = this.snapshot.frame.budget.maxCostUsd;
		if (maxCostUsd !== undefined && usage.costUsdUsed >= maxCostUsd) {
			return {
				limit: "maxCostUsd",
				reason: `cost budget used $${usage.costUsdUsed.toFixed(6)}, limit is $${maxCostUsd.toFixed(6)}`,
			};
		}
		return undefined;
	}

	async consumeTurns(turns: number): Promise<void> {
		return this.exclusive(async () => {
			if (!Number.isInteger(turns) || turns <= 0) throw new Error("turn usage must be a positive integer");
			const block = this.budgetBlock({ turns });
			if (block) throw new Error(block.reason);
			await this.appendEvent({ type: "budget_usage_recorded", turns, costUsd: 0 });
		});
	}

	async recordCost(costUsd: number): Promise<void> {
		if (!Number.isFinite(costUsd) || costUsd < 0) throw new Error("cost usage must be a non-negative number");
		if (costUsd === 0) return;
		await this.commit({ type: "budget_usage_recorded", turns: 0, costUsd });
	}

	async refundTurns(turns: number): Promise<void> {
		return this.exclusive(async () => {
			if (!Number.isInteger(turns) || turns <= 0) throw new Error("turn refund must be a positive integer");
			const used = this.snapshot.budgetUsage?.turnsUsed ?? 0;
			if (turns > used) throw new Error("turn refund cannot exceed turns used");
			await this.appendEvent({ type: "budget_turns_refunded", turns });
		});
	}

	async recordProviderBackoff(backoff: ProviderBackoffState): Promise<void> {
		if (!Number.isInteger(backoff.attempt) || backoff.attempt <= 0) {
			throw new Error("provider backoff attempt must be a positive integer");
		}
		if (!backoff.reason.trim()) throw new Error("provider backoff requires a reason");
		if (
			!Number.isFinite(Date.parse(backoff.startedAt)) ||
			Date.parse(backoff.retryAt) <= Date.parse(backoff.startedAt)
		) {
			throw new Error("provider backoff retryAt must be after startedAt");
		}
		await this.commit({ type: "provider_backoff_started", backoff });
	}

	async clearProviderBackoff(): Promise<void> {
		if (!this.snapshot.providerBackoff) return;
		await this.commit({ type: "provider_backoff_cleared" });
	}

	async updateBudget(update: Partial<MissionFrame["budget"]>): Promise<void> {
		return this.exclusive(async () => {
			const budget = { ...this.snapshot.frame.budget, ...update };
			validateBudget(budget);
			const usage = this.snapshot.budgetUsage ?? { turnsUsed: 0, costUsdUsed: 0 };
			if (budget.maxTasks < Object.keys(this.snapshot.tasks).length) {
				throw new Error("maxTasks cannot be lower than tasks already created");
			}
			if (budget.maxTurns < usage.turnsUsed) throw new Error("maxTurns cannot be lower than turns already used");
			if (budget.maxCostUsd !== undefined && budget.maxCostUsd < usage.costUsdUsed) {
				throw new Error("maxCostUsd cannot be lower than cost already used");
			}
			await this.appendEvent({ type: "budget_updated", budget });
		});
	}

	async setAutomation(automation: AutomationLevel): Promise<void> {
		await this.commit({ type: "automation_updated", automation });
	}

	isRouteGateApproved(stageId: string): boolean {
		return this.snapshot.stages[stageId]?.routeApproval !== undefined;
	}

	async requireUserGate(input: UserGateRequest): Promise<void> {
		const current = this.snapshot.frame.userGate;
		if (
			current?.kind === input.kind &&
			current.stageId === input.stageId &&
			(current.kind === "budget"
				? input.kind === "budget" && current.limit === input.limit
				: current.kind === "stage"
					? input.kind === "stage" && current.phase === input.phase
					: input.kind === "research" &&
						current.question === input.question &&
						current.decisionRef === input.decisionRef &&
						current.planId === input.planId)
		) {
			return;
		}
		if (current) throw new Error(`research job already waits at user gate: ${current.reason}`);
		await this.commit({
			type: "user_gate_required",
			gate: { ...input, requiredAt: new Date().toISOString() },
		});
	}

	async acquireLease(owner: string, ttlMs = 30_000): Promise<Lease> {
		return this.exclusive(async () => {
			const now = Date.now();
			const existing = this.snapshot.lease;
			if (existing && Date.parse(existing.expiresAt) > now && existing.owner !== owner) {
				throw new Error(`research supervisor lease held by ${existing.owner}`);
			}
			const lease: Lease = {
				owner,
				heartbeatAt: new Date(now).toISOString(),
				expiresAt: new Date(now + ttlMs).toISOString(),
			};
			await this.appendEvent({ type: "lease_acquired", lease });
			return lease;
		});
	}

	async releaseLease(owner: string): Promise<void> {
		return this.exclusive(async () => {
			if (this.snapshot.lease?.owner !== owner) return;
			await this.appendEvent({ type: "lease_released", owner });
		});
	}

	private mainAgentBasis(snapshot: JobSnapshot): string {
		const { nextAction: _next, status: _status, userGate: _gate, budget: _budget, ...frame } = snapshot.frame;
		return checksum(
			JSON.parse(
				JSON.stringify({
					definitions: this.definitions,
					frame,
					stages: Object.fromEntries(
						Object.entries(snapshot.stages).map(([id, { routeApproval: _approval, ...stage }]) => [
							id,
							{ ...stage, revision: stage.revision ?? 1 },
						]),
					),
					stagePlans: snapshot.stagePlans,
					tasks: snapshot.tasks,
					evidence: snapshot.evidence,
					reviews: snapshot.reviews,
					obligations: snapshot.obligations,
					canonical: snapshot.canonical,
					retiredArtifacts: snapshot.retiredArtifacts,
					discardedCandidates: snapshot.discardedCandidates,
					discardedEvidence: snapshot.discardedEvidence,
					graph: snapshot.graph,
					searchBatches: Object.fromEntries(
						Object.entries(snapshot.searchBatches).map(([id, { acceptanceCompleted: _completed, ...batch }]) => [
							id,
							batch,
						]),
					),
					candidateEvaluations: snapshot.candidateEvaluations,
					routeDecisions: snapshot.routeDecisions,
					canonicalRoute: snapshot.canonicalRoute,
				}),
			),
		);
	}

	async registerMainAgentCall(
		input: Pick<MainAgentCall, "id" | "type" | "manifestRef"> &
			Partial<
				Pick<MainAgentCall, "planId" | "mode" | "obligationId" | "evidenceId" | "searchBatchId" | "manifestId">
			>,
	): Promise<void> {
		return this.exclusive(async () => {
			if (this.snapshot.paused) throw new Error("main-agent call cannot begin while paused");
			if (input.type === "evidence" || input.type === "adoption") this.assertEvidenceCurrent(input.evidenceId!);
			const stageId = this.snapshot.frame.activeStageId;
			const stage = this.snapshot.stages[stageId];
			if (input.type === "route" && input.obligationId) {
				const obligation = this.snapshot.obligations[input.obligationId];
				const evidence = this.snapshot.evidence[this.snapshot.reviews[obligation?.sourceReviewId]?.evidenceId];
				if (
					!obligation ||
					obligation.status !== "open" ||
					evidence?.stageId !== stageId ||
					(obligation.stageId && obligation.stageId !== stageId)
				)
					throw new Error("repair route requires a current same-stage open obligation");
			}
			const call: MainAgentCall = {
				...input,
				jobId: this.snapshot.frame.jobId,
				stageId,
				stageRevision: stage.revision ?? 1,
				stageExecutionId: stage.executionId,
				basisEventSeq: this.snapshot.eventSeq,
				basisHash: this.mainAgentBasis(this.snapshot),
			};
			if (this.snapshot.mainAgentCalls?.[call.id]) throw new Error("main-agent call id already registered");
			await this.appendEvent({ type: "main_agent_call_recorded", call });
		});
	}

	private async originalMainAgentBasis(call: MainAgentCall): Promise<JobSnapshot> {
		const history = (await this.store.readEvents(call.jobId)).filter((saved) => saved.seq <= call.basisEventSeq);
		const initial = history[0]?.event;
		if (initial?.type !== "job_created") throw new Error("main-agent original event prefix unavailable");
		const replay = new ResearchJob(structuredClone(initial.snapshot), this.store, this.definitions);
		for (const saved of history) {
			replay.apply(saved.event, saved.timestamp);
			replay.snapshot.eventSeq = saved.seq;
			replay.snapshot.updatedAt = saved.timestamp;
		}
		replay.snapshot = normalizeSnapshot(replay.snapshot);
		if (replay.snapshot.eventSeq !== call.basisEventSeq || this.mainAgentBasis(replay.snapshot) !== call.basisHash)
			throw new Error("main-agent original basis integrity failed");
		return replay.state;
	}

	async readMainAgentCallDelivery(callId: string): Promise<StagePlanManifest | MainAgentDecisionManifest | undefined> {
		const call = this.snapshot.mainAgentCalls?.[callId];
		if (!call || call.abandoned) return undefined;
		let manifest: StagePlanManifest | MainAgentDecisionManifest;
		try {
			manifest = await readMainAgentDelivery(this.snapshot.frame.permissions.workspaceRoot, call);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT" && !call.deliveryHash && !call.applied)
				return undefined;
			throw error;
		}
		const deliveryHash = checksum(manifest);
		if (call.deliveryHash && call.deliveryHash !== deliveryHash)
			throw new Error("main-agent saved delivery digest changed");
		if (!call.deliveryHash) await this.commit({ type: "main_agent_delivery_recorded", callId, deliveryHash });
		return manifest;
	}

	async abandonMainAgentCall(callId: string): Promise<void> {
		await this.commit({ type: "main_agent_call_finished", callId, abandoned: true });
	}

	private assertMainAgentPermission(decisionRef: string): void {
		const call = this.snapshot.mainAgentCalls?.[decisionRef];
		if (call && !call.applied && this.snapshot.paused)
			throw new Error("saved main-agent delivery awaits explicit resume");
	}

	/** Saved calls precede budgets and new runners. Domain events prove application, not this tail marker. */
	async recoverMainAgentDeliveries(
		callIds = Object.keys(this.snapshot.mainAgentCalls ?? {}),
	): Promise<Array<StagePlanManifest | MainAgentDecisionManifest>> {
		const recovered: Array<StagePlanManifest | MainAgentDecisionManifest> = [];
		for (const callId of callIds) {
			let call = this.snapshot.mainAgentCalls?.[callId];
			if (!call || call.completed || call.abandoned) continue;
			const manifest = await this.readMainAgentCallDelivery(callId);
			if (!manifest) continue;
			call = this.snapshot.mainAgentCalls![callId];
			const session = Object.values(this.snapshot.sessions).find(
				(saved) => saved.role === "main-agent" && saved.taskId === callId,
			);
			if (session && (session.status !== "completed" || session.manifestRef !== call.manifestRef))
				await this.recordChildSession({
					...session,
					status: "completed",
					error: undefined,
					manifestRef: call.manifestRef,
					updatedAt: new Date().toISOString(),
				});
			if (!call.applied) {
				if (this.snapshot.paused) {
					recovered.push(manifest);
					continue;
				}
				if (this.mainAgentBasis(this.snapshot) !== call.basisHash) {
					await this.originalMainAgentBasis(call);
					await this.exclusive(async () => {
						const current = this.snapshot.mainAgentCalls![callId];
						if (
							!current.applied &&
							!current.completed &&
							!current.abandoned &&
							!this.snapshot.paused &&
							this.mainAgentBasis(this.snapshot) !== current.basisHash
						)
							await this.appendEvent({ type: "main_agent_call_finished", callId, abandoned: true });
					});
					call = this.snapshot.mainAgentCalls![callId];
					if (call.abandoned || call.completed) continue;
					if (!call.applied && this.snapshot.paused) {
						recovered.push(manifest);
						continue;
					}
				}
				if (!call.applied) {
					if ("tasks" in manifest) await this.recordStagePlan(manifest, await this.originalMainAgentBasis(call));
					else if (call.type === "evidence") {
						if (manifest.decision === "defer")
							await this.commit({
								type: "job_paused",
								decisionRef: callId,
								reason: `evidence ${call.evidenceId} deferred by ${callId}: ${manifest.rationale}; resume with guidance to replan`,
							});
						else await this.decideEvidence(call.evidenceId!, manifest.decision === "accept", callId);
					} else if (call.type === "adoption") {
						if (!manifest.adopt)
							await this.commit({
								type: "job_paused",
								decisionRef: callId,
								reason: `evidence ${call.evidenceId} not adopted by ${callId}: ${manifest.rationale}; resume with guidance to replan`,
							});
						else await this.adoptEvidence(call.evidenceId!, manifest.replacementOf, callId);
					} else if (call.type === "search-selection") {
						if (manifest.selectedCandidateId)
							await this.selectSearchCandidate(call.searchBatchId!, manifest.selectedCandidateId, callId);
						else await this.continueSearchBatch(call.searchBatchId!, callId, manifest.rationale);
					} else await this.applyRouteDecision(manifest);
				}
			}
			await this.recoverPendingOperations();
			await this.commit({ type: "main_agent_call_finished", callId });
			recovered.push(manifest);
		}
		return recovered;
	}

	async recordStagePlan(plan: StagePlanManifest, generationSnapshot?: JobSnapshot): Promise<StagePlanManifest> {
		return this.exclusive(async () => {
			this.assertMainAgentPermission(plan.decisionRef);
			plan = { ...plan };
			delete plan.generationBasisHash;
			const existing = this.snapshot.stagePlans[plan.id];
			if (existing) {
				if (
					checksum({ ...existing, generationBasisHash: undefined }) !==
					checksum({ ...plan, generationBasisHash: undefined })
				)
					throw new Error(`stage plan id ${plan.id} already has different content`);
				if (
					plan.mode === "search" &&
					!Object.values(this.snapshot.searchBatches).some((batch) => batch.planId === plan.id)
				)
					await this.appendEvent({
						type: "search_batch_recorded",
						...this.searchRegistration(plan, this.definitions[plan.stageId]),
					});
				return structuredClone(existing);
			}
			if (plan.jobId !== this.snapshot.frame.jobId) throw new Error("stage plan job id does not match active job");
			if (plan.stageId !== this.snapshot.frame.activeStageId)
				throw new Error("stage plan does not target the active stage");
			if (this.snapshot.stages[plan.stageId]?.status !== "running")
				throw new Error("stage plan requires a running stage");
			assertAstraId(plan.id, "stage plan id");
			const maxTasks = plan.mode === "search" ? 4 : 2;
			if (plan.tasks.length === 0 || plan.tasks.length > maxTasks) {
				throw new Error(`stage plan must contain one to ${maxTasks} tasks`);
			}
			if (plan.mode === "search" && plan.tasks.length < 2) {
				throw new Error("search stage plan must contain at least two candidates");
			}
			if (plan.obligationId && plan.tasks.length !== 1) {
				throw new Error("repair stage plan must contain exactly one task");
			}
			if (new Set(plan.tasks.map((task) => task.key)).size !== plan.tasks.length) {
				throw new Error("stage plan task keys must be unique");
			}
			const definition = this.definitions[plan.stageId];
			if (!definition) throw new Error(`unknown stage ${plan.stageId}`);
			const searchPolicy = definition.searchPolicy ?? {
				strategy: "diverse-candidates" as const,
				minCandidates: 2,
				maxCandidates: 4,
				criteria: definition.acceptanceChecks,
			};
			if (plan.mode === "search" && plan.tasks.length < searchPolicy.minCandidates) {
				throw new Error(`search stage plan requires at least ${searchPolicy.minCandidates} candidates`);
			}
			if (plan.mode === "search" && plan.tasks.length > searchPolicy.maxCandidates)
				throw new Error(`search stage plan exceeds maximum ${searchPolicy.maxCandidates} candidates`);
			if (plan.mode === "search" && this.finalSearchBatch(plan.stageId))
				throw new Error(
					"search maximum rounds reached in current revision; backtrack or continue with ordinary planning",
				);
			const currentStage = this.snapshot.stages[plan.stageId];
			const continuedRoute = this.snapshot.routeDecisions[currentStage.lastRouteDecisionRef ?? ""];
			const continueDecisionRef =
				plan.mode === "decompose" &&
				!plan.obligationId &&
				continuedRoute?.action === "continue" &&
				Boolean(continuedRoute.negativeSearchBatchId) &&
				continuedRoute.negativeSearchBatchId === this.finalSearchBatch(plan.stageId)?.id
					? continuedRoute.id
					: undefined;
			if (continueDecisionRef) {
				if (continuedRoute.continuedPlanId)
					throw new Error("negative-search continue already consumed by another plan");
				if (
					generationSnapshot &&
					(generationSnapshot.stages[plan.stageId].lastRouteDecisionRef !== continueDecisionRef ||
						(generationSnapshot.stages[plan.stageId].revision ?? 1) !== (currentStage.revision ?? 1) ||
						generationSnapshot.stages[plan.stageId].executionId !== currentStage.executionId)
				)
					throw new Error("continued plan generation basis no longer matches the current route");
			}
			for (const task of plan.tasks) {
				assertAstraId(task.key, "planned task key");
				if (!task.objective.trim()) throw new Error("planned task objective cannot be empty");
				if (task.requiredOutputFields.length === 0) throw new Error("planned task requires output fields");
				const missingContractFields = definition.requiredOutputFields.filter(
					(field) => !task.requiredOutputFields.includes(field),
				);
				if (task.deliveryKind !== "local" && missingContractFields.length > 0) {
					throw new Error(
						`stage plan task ${task.key} omits contract output fields: ${missingContractFields.join(", ")}`,
					);
				}
				if (task.deliveryKind === "local" && plan.mode === "search")
					throw new Error("search candidates require complete stage deliveries");
				this.assertDeliveryInputs(plan.stageId, task.deliveryKind, task.inputArtifactRefs, plan.obligationId);
				for (const ref of task.inputArtifactRefs) {
					if (!this.snapshot.canonical[ref] && !this.snapshot.evidence[ref]) {
						throw new Error(`stage plan references unknown input artifact ${ref}`);
					}
				}
				const bindingIds = task.responsibilityBindings?.map((binding) => binding.nodeId) ?? [];
				if (new Set(bindingIds).size !== bindingIds.length) {
					throw new Error(`stage plan task ${task.key} duplicates a responsibility binding`);
				}
				const transferIds = (task.responsibilityTransfers ?? []).map(
					(transfer) => transfer.nodeId ?? transfer.issueId,
				);
				if (new Set(transferIds).size !== transferIds.length)
					throw new Error("stage plan duplicates a responsibility transfer");
				if ((task.responsibilityTransfers?.length ?? 0) > 0) {
					for (const transfer of task.responsibilityTransfers ?? []) {
						const sourceTask = this.snapshot.tasks[transfer.sourceTaskId];
						const sourceEvidence = Object.values(this.snapshot.evidence).find(
							(evidence) => evidence.taskId === transfer.sourceTaskId,
						);
						if (
							task.deliveryKind !== "local" ||
							!sourceTask ||
							sourceTask.stageId !== plan.stageId ||
							sourceTask.status !== "succeeded" ||
							!sourceEvidence ||
							!task.inputArtifactRefs.includes(sourceEvidence.id) ||
							transfer.sourceContractHash !== sourceTaskContractHash(sourceTask) ||
							transfer.sourceField !== "acceptanceChecks" ||
							!Number.isInteger(transfer.sourceIndex) ||
							sourceTask.acceptanceChecks[transfer.sourceIndex] !== transfer.exactCriterion ||
							transfer.destinationStageId !== plan.stageId ||
							transfer.destinationPhase !== "synthesis" ||
							!transfer.rationale.trim() ||
							Boolean(transfer.nodeId) === Boolean(transfer.issueId) ||
							!(transfer.nodeId
								? this.backtrackChecks(plan.stageId).some(
										(check) =>
											check.nodeId === transfer.nodeId && check.criterion === transfer.exactCriterion,
									) &&
									(!sourceTask.responsibilityBindings?.length ||
										sourceTask.responsibilityBindings.some(
											(binding) => binding.nodeId === transfer.nodeId && binding.stageId === plan.stageId,
										))
								: Object.values(this.snapshot.obligations).some((obligation) =>
										(obligation.items ?? []).some(
											(item) =>
												item.id === transfer.issueId &&
												item.status === "open" &&
												item.criterion === transfer.exactCriterion &&
												this.snapshot.evidence[
													this.snapshot.reviews[obligation.sourceReviewId]?.evidenceId ?? ""
												]?.taskId === sourceTask.id,
										),
									))
						)
							throw new Error("stage plan responsibility transfer does not match a bound source requirement");
					}
				}

				for (const binding of task.responsibilityBindings ?? []) {
					const node = this.snapshot.graph.nodes[binding.nodeId];
					const expectedPhase = task.deliveryKind === "synthesis" ? "synthesis" : "stage";
					if (
						task.deliveryKind === "local" ||
						binding.stageId !== plan.stageId ||
						binding.phase !== expectedPhase ||
						!node ||
						node.stageId !== binding.stageId ||
						node.status !== "open" ||
						!this.backtrackChecks(plan.stageId).some((check) => check.nodeId === binding.nodeId)
					) {
						throw new Error(
							`stage plan task ${task.key} has an invalid responsibility binding ${binding.nodeId}`,
						);
					}
				}
			}
			if (plan.obligationId) {
				const obligation = this.snapshot.obligations[plan.obligationId];
				if (!obligation || obligation.status !== "open")
					throw new Error("repair stage plan requires an open obligation");
				const failed = this.snapshot.evidence[this.snapshot.reviews[obligation.sourceReviewId]?.evidenceId];
				const failedTask = failed ? this.snapshot.tasks[failed.taskId] : undefined;
				if (
					failedTask &&
					(plan.tasks[0].deliveryKind ?? "stage") !== (failedTask.deliveryKind ?? "stage") &&
					!(plan.tasks[0].deliveryKind === "local" && (plan.tasks[0].responsibilityTransfers?.length ?? 0) > 0)
				)
					throw new Error("repair plan must preserve the failed task delivery kind");
			}
			if (plan.tasks.some((task) => task.deliveryKind === "synthesis") && plan.tasks.length !== 1)
				throw new Error("synthesis requires one task after local reviews");
			if (
				plan.tasks.some((task) => task.deliveryKind === "local") &&
				plan.tasks.some((task) => task.deliveryKind !== "local")
			)
				throw new Error("local and stage deliveries require separate planning rounds");
			const previousBatch = plan.mode === "search" ? this.pendingSearchContinuation(plan.stageId) : undefined;
			if (previousBatch) {
				const previousHypotheses = new Set(
					Object.values(previousBatch.candidates).map((candidate) =>
						this.normalizeHypothesis(candidate.hypothesis),
					),
				);
				const repeated = plan.tasks.find((task) =>
					previousHypotheses.has(this.normalizeHypothesis(task.hypothesis ?? task.objective)),
				);
				if (repeated)
					throw new Error(`continued search repeats a hypothesis from ${previousBatch.id}: ${repeated.key}`);
			}
			if (generationSnapshot)
				plan.generationBasisHash = planGenerationBasisHash(generationSnapshot, this.definitions, plan);
			await this.appendEvent({
				type: "stage_plan_recorded",
				plan,
				...(continueDecisionRef ? { continueDecisionRef } : {}),
				...(plan.mode === "search" ? { search: this.searchRegistration(plan, definition) } : {}),
			});
			return structuredClone(plan);
		});
	}

	private searchRegistration(plan: StagePlanManifest, definition: StageDefinition) {
		const batch = searchBatchForPlan(this.snapshot, definition, plan);
		batch.stageRevision = this.snapshot.stages[plan.stageId].revision ?? 1;
		batch.stageExecutionId = this.snapshot.stages[plan.stageId].executionId;
		const nodes = Object.values(batch.candidates).map((candidate) =>
			createResearchNode({
				id: candidate.graphNodeId,
				kind: "hypothesis",
				statement: candidate.hypothesis,
				stageId: plan.stageId,
				domainRef: candidate.id,
				sourceRefs: [`stage-plan:${plan.id}`, plan.sessionRef],
			}),
		);
		const edges = nodes.map((node) =>
			createResearchEdge({
				fromNodeId: this.snapshot.graph.rootQuestionId,
				toNodeId: node.id,
				kind: "refines",
				sourceRefs: [`stage-plan:${plan.id}`],
			}),
		);
		return { batch, nodes, edges };
	}

	private pendingSearchContinuation(stageId: string): SearchBatch | undefined {
		const latest = Object.values(this.snapshot.searchBatches)
			.filter((batch) => batch.stageId === stageId && batch.status !== "superseded")
			.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
		return latest?.status === "exhausted" && latest.round < latest.maxRounds ? latest : undefined;
	}

	private normalizeHypothesis(value: string): string {
		return value.trim().toLocaleLowerCase().replace(/\s+/g, " ");
	}

	async recordUserGuidance(guidance: string, resume = false): Promise<ResearchNode> {
		return this.exclusive(async () => {
			const statement = guidance.trim();
			if (!statement) throw new Error("research guidance cannot be empty");
			if (resume && this.snapshot.frame.userGate && this.snapshot.frame.userGate.kind !== "research")
				throw new Error("guided resume cannot bypass a stage or budget gate");
			const stageId = this.snapshot.frame.activeStageId;
			const node = createResearchNode({
				kind: "decision",
				statement: `User guidance: ${statement}`,
				status: "accepted",
				stageId,
				sourceRefs: [`user-guidance:${this.snapshot.eventSeq + 1}`],
			});
			node.actor = "user";
			const release = resume || this.snapshot.frame.userGate?.kind === "research";
			const gate = this.snapshot.frame.userGate;
			const continuedRoute = this.snapshot.routeDecisions[gate?.kind === "research" ? (gate.decisionRef ?? "") : ""];
			const continueDecisionRef =
				gate?.kind === "research" &&
				continuedRoute?.action === "continue" &&
				Boolean(continuedRoute.continuedPlanId) &&
				continuedRoute.continuedPlanId === gate.planId &&
				this.snapshot.stages[stageId].lastRouteDecisionRef === continuedRoute.id
					? continuedRoute.id
					: undefined;
			const batchIds = Object.values(this.snapshot.searchBatches)
				.filter(
					(batch) => batch.stageId === stageId && ["planning", "running", "evaluating"].includes(batch.status),
				)
				.map((batch) => batch.id);
			const taskIds = Object.values(this.snapshot.tasks)
				.filter(
					(task) =>
						task.stageId === stageId &&
						task.role === "worker" &&
						(["ready", "running"].includes(task.status) ||
							(task.status === "failed" && batchIds.includes(task.searchBatchId ?? ""))),
				)
				.map((task) => task.id);
			await this.appendEvent({
				type: "user_guidance_recorded",
				node,
				...(release
					? {
							supersession: {
								stageId,
								taskIds,
								batchIds,
								resume,
								...(continueDecisionRef ? { continueDecisionRef } : {}),
							},
						}
					: {}),
			});
			return node;
		});
	}

	async reopenStage(targetStageId: string, decisionRef: string, reason: string): Promise<void> {
		return this.exclusive(async () => {
			if (!this.definitions[targetStageId]) throw new Error(`unknown stage ${targetStageId}`);
			const already = Object.values(this.snapshot.graph.nodes).some(
				(node) => node.kind === "objection" && node.domainRef === decisionRef && node.stageId === targetStageId,
			);
			if (already) {
				await this.finishCleanupsInternal();
				return;
			}
			await this.appendEvent({
				type: "stage_reopened",
				...(await this.prepareStageReopening(targetStageId, decisionRef, reason)),
			});
			await this.finishCleanupsInternal();
		});
	}

	private async prepareStageReopening(
		targetStageId: string,
		decisionRef: string,
		reason: string,
	): Promise<StageReopening> {
		const invalidRefs = dependentEvidenceRefs(
			this.snapshot,
			Object.values(this.snapshot.evidence)
				.filter((evidence) => evidence.stageId === targetStageId)
				.map((evidence) => evidence.id),
		);
		const invalidArtifactIds = Object.values(this.snapshot.canonical)
			.filter((artifact) => artifact.status === "active" && invalidRefs.has(artifact.id))
			.map((artifact) => artifact.id);
		const affectedStageIds = [...new Set([targetStageId, ...this.dependentStageIds(invalidRefs)])];
		const cleanups = await Promise.all(invalidArtifactIds.map((id) => this.retirementIntent(id)));
		const objection = createResearchNode({
			kind: "objection",
			statement: reason,
			status: "open",
			stageId: targetStageId,
			domainRef: decisionRef,
			sourceRefs: [decisionRef],
		});
		return { targetStageId, affectedStageIds, decisionRef, reason, objection, cleanups };
	}

	private async retirementIntent(artifactId: string, replacementId?: string): Promise<CleanupIntent> {
		const artifact = this.snapshot.canonical[artifactId];
		if (!artifact || artifact.status === "retired") throw new Error("retirement requires an existing artifact");
		const evidence = this.snapshot.evidence[artifact.evidenceId];
		if (!evidence) throw new Error(`canonical artifact ${artifactId} has no evidence lineage`);
		const reviewIds = Object.values(this.snapshot.reviews)
			.filter((review) => review.evidenceId === evidence.id)
			.map((review) => review.id);
		const reviewerTaskIds = reviewIds.flatMap((reviewId) => {
			const reviewerTaskId = this.snapshot.reviews[reviewId]?.reviewerTaskId;
			return reviewerTaskId ? [reviewerTaskId] : [];
		});
		const receipt: RetiredArtifactReceipt = {
			artifactId,
			type: artifact.type,
			evidenceId: artifact.evidenceId,
			taskId: evidence.taskId,
			reviewIds,
			checksum: artifact.checksum,
			replacementId,
			materializationReceiptRef: canonicalReceiptPath(
				this.snapshot.frame.permissions.workspaceRoot,
				this.snapshot.frame.jobId,
				artifactId,
			),
			retiredAt: new Date().toISOString(),
		};
		receipt.cleanupStatus = "pending";
		return {
			id: `retirement:${artifactId}`,
			kind: "retirement",
			receipt,
			materializationRef: artifact.materializationRef,
			status: "pending",
			tasks: await cleanupTaskFiles(this.snapshot, [evidence.taskId, ...reviewerTaskIds]),
		};
	}

	/** Intent supplies stable paths even after its logical pruning removed live evidence/reviews. */
	private async finishCleanupsInternal(): Promise<void> {
		for (const intent of Object.values(this.snapshot.cleanupIntents ?? {})) {
			if (intent.status === "completed") continue;
			const archiveRefs = await archiveAndPruneTasks(this.snapshot, intent.tasks);
			if (intent.kind === "retirement") {
				if (intent.materializationRef) await rm(intent.materializationRef, { force: true });
				if (intent.receipt.materializationReceiptRef) {
					const receiptPath = intent.receipt.materializationReceiptRef;
					const expected = JSON.parse(
						JSON.stringify({
							schemaVersion: "astra.retired_artifact_receipt.v1",
							...intent.receipt,
							archiveRefs,
							cleanupStatus: "completed",
						}),
					) as Record<string, unknown>;
					let existing: Record<string, unknown> | undefined;
					try {
						existing = JSON.parse(await readFile(receiptPath, "utf8")) as Record<string, unknown>;
					} catch (error) {
						if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
					}
					if (existing && checksum(existing) !== checksum(expected)) {
						if (
							existing.schemaVersion !== "astra.materialization_receipt.v1" ||
							existing.artifactId !== intent.receipt.artifactId ||
							existing.sourceSha256 !== intent.receipt.checksum ||
							existing.targetPath !== intent.materializationRef
						)
							throw new Error(`retirement receipt conflicts with intent: ${receiptPath}`);
					}
					if (!existing || checksum(existing) !== checksum(expected)) await atomicWriteJson(receiptPath, expected);
					if (checksum(JSON.parse(await readFile(receiptPath, "utf8"))) !== checksum(expected))
						throw new Error(`retirement receipt readback failed: ${receiptPath}`);
				}
			}
			await this.appendEvent({
				type: "cleanup_completed",
				intentId: intent.id,
				archiveRefs,
				completedAt: new Date().toISOString(),
			});
		}
	}

	async dispatchTask(
		input: Omit<
			TaskPacket,
			| "schemaVersion"
			| "id"
			| "jobId"
			| "agentId"
			| "runnerKind"
			| "outputManifestRequired"
			| "replayKey"
			| "attempt"
			| "status"
			| "createdAt"
		> & { id?: string; agentId?: string; replayKey?: string; attempt?: number },
	): Promise<TaskPacket> {
		return this.exclusive(async () => {
			await this.finishCleanupsInternal();
			if (this.snapshot.paused) throw new Error("research job is paused");
			const replayKey = input.replayKey ?? `${this.snapshot.frame.jobId}:${input.stageId}:${input.objective}`;
			this.assertCurrentInputs(input.inputArtifactRefs, input.repairOfEvidenceId);
			let reviewedContract: ReturnType<typeof buildEffectiveTaskContract> | undefined;
			let legacyReviewedPlan = false;
			if (input.role === "worker" && input.planId) {
				if (planReviewStatus(this, input.planId) !== "passed")
					throw new Error("worker dispatch requires an independently approved plan");
				const plan = this.snapshot.stagePlans[input.planId];
				const planned = plan?.tasks.find((task) => input.replayKey === `stage-plan:${input.planId}:${task.key}`);
				if (!plan || !planned) throw new Error("worker contract differs from the reviewed plan");
				const frozen = planReviewEvidence(this, input.planId);
				legacyReviewedPlan = !Array.isArray(
					(frozen?.content as { effectiveContracts?: unknown } | undefined)?.effectiveContracts,
				);
				if (!legacyReviewedPlan) reviewedContract = buildEffectiveTaskContract(this, plan, planned);
			}
			const duplicate = Object.values(this.snapshot.tasks).find(
				(task) => task.replayKey === replayKey && task.status !== "failed",
			);
			if (duplicate) {
				if (reviewedContract && !taskContractMatches(duplicate, reviewedContract))
					throw new Error("existing worker task differs from the frozen reviewed plan");
				if (legacyReviewedPlan && duplicate.planId !== input.planId)
					throw new Error("legacy reviewed plan cannot dispatch a different worker task");
				return structuredClone(duplicate);
			}
			if (legacyReviewedPlan)
				throw new Error("legacy reviewed plan cannot dispatch or retry workers without a frozen contract");
			if (Object.keys(this.snapshot.tasks).length >= this.snapshot.frame.budget.maxTasks) {
				throw new ResearchTaskBudgetError();
			}
			if (input.role === "worker") {
				const repairObligationId = input.repairOfEvidenceId
					? this.snapshot.frame.openObligationIds.find(
							(id) =>
								this.snapshot.reviews[this.snapshot.obligations[id].sourceReviewId]?.evidenceId ===
								input.repairOfEvidenceId,
						)
					: undefined;
				this.assertDeliveryInputs(input.stageId, input.deliveryKind, input.inputArtifactRefs, repairObligationId);
			}
			const task: TaskPacket = {
				...input,
				stageRevision: this.snapshot.stages[input.stageId]?.revision ?? 1,
				schemaVersion: "astra.task_packet.v1",
				id: input.id ?? `task_${randomUUID()}`,
				jobId: this.snapshot.frame.jobId,
				agentId: input.agentId ?? `${input.role}_${randomUUID()}`,
				runnerKind: "pi-session",
				outputManifestRequired: true,
				replayKey,
				attempt: input.attempt ?? 1,
				status: "ready",
				createdAt: new Date().toISOString(),
			};
			if (
				input.role === "worker" &&
				input.planId &&
				(!reviewedContract || !taskContractMatches(task, reviewedContract))
			)
				throw new Error("worker contract differs from the frozen reviewed plan");
			await this.appendEvent({ type: "task_dispatched", task });
			return task;
		});
	}

	unsynthesizedLocalEvidence(stageId: string): Evidence[] {
		const artifact = this.snapshot.canonical[this.snapshot.canonicalRoute.stageArtifactIds[stageId]];
		const sourceTask = artifact
			? this.snapshot.tasks[this.snapshot.evidence[artifact.evidenceId]?.taskId]
			: undefined;
		const consumed = new Set(sourceTask?.inputArtifactRefs ?? []);
		return Object.values(this.snapshot.evidence).filter((evidence) => {
			const task = this.snapshot.tasks[evidence.taskId];
			if (
				!(
					evidence.stageId === stageId &&
					evidence.status === "accepted" &&
					task?.deliveryKind === "local" &&
					(task.stageRevision ?? 1) === (this.snapshot.stages[stageId]?.revision ?? 1) &&
					!consumed.has(evidence.id)
				)
			)
				return false;
			try {
				this.assertEvidenceCurrent(evidence.id);
				return true;
			} catch (error) {
				if (error instanceof StaleResearchInputError || error instanceof EffectiveContractUnavailableError)
					return false;
				throw error;
			}
		});
	}

	private dependentStageIds(invalidRefs: Set<string>): string[] {
		return [
			...new Set(
				Object.values(this.snapshot.evidence)
					.filter((evidence) => invalidRefs.has(evidence.id))
					.map((evidence) => evidence.stageId),
			),
		].filter((stageId) => {
			const artifact = this.snapshot.canonical[this.snapshot.canonicalRoute.stageArtifactIds[stageId]];
			return !artifact || artifact.status !== "active" || invalidRefs.has(artifact.id);
		});
	}

	private assertDeliveryInputs(
		stageId: string,
		kind: TaskPacket["deliveryKind"],
		refs: string[],
		repairObligationId?: string,
	): void {
		if (kind !== undefined && !["stage", "local", "synthesis"].includes(kind))
			throw new Error("unknown task delivery kind");
		if (kind === "local") return;
		const localInputs = refs.flatMap((ref) => {
			const evidence = this.snapshot.evidence[ref];
			return evidence && this.snapshot.tasks[evidence.taskId]?.deliveryKind === "local" ? [evidence] : [];
		});
		const pending = this.unsynthesizedLocalEvidence(stageId);
		if (kind !== "synthesis") {
			if (localInputs.length || pending.length)
				throw new Error("local evidence requires explicit synthesis before stage delivery");
			return;
		}
		if (
			!localInputs.length ||
			localInputs.some((evidence) => evidence.stageId !== stageId || evidence.status !== "accepted")
		)
			throw new Error("synthesis requires accepted local evidence from the same stage");
		if (pending.some((evidence) => !refs.includes(evidence.id)))
			throw new Error("synthesis omits accepted local evidence");
		if (
			Object.values(this.snapshot.tasks).some(
				(task) =>
					task.stageId === stageId &&
					task.deliveryKind === "local" &&
					(task.stageRevision ?? 1) === (this.snapshot.stages[stageId]?.revision ?? 1) &&
					taskIsCurrentFromSnapshot(this.snapshot, task) &&
					(["ready", "running"].includes(task.status) ||
						(task.status === "succeeded" &&
							!Object.values(this.snapshot.evidence).some(
								(evidence) => evidence.taskId === task.id && evidence.status !== "candidate",
							))),
			)
		)
			throw new Error("synthesis must wait for unfinished local tasks and reviews");
		const transferredIssueIds = new Set(
			localInputs.flatMap((evidence) => {
				const task = this.snapshot.tasks[evidence.taskId];
				return (task?.responsibilityTransfers ?? []).flatMap((transfer) =>
					transfer.issueId ? [transfer.issueId] : [],
				);
			}),
		);
		if (
			this.snapshot.frame.openObligationIds.some((id) => {
				const obligation = this.snapshot.obligations[id];
				const review = this.snapshot.reviews[obligation?.sourceReviewId];
				return (
					id !== repairObligationId &&
					obligation?.status === "open" &&
					this.snapshot.evidence[review?.evidenceId]?.stageId === stageId &&
					(obligation.items ?? []).some((item) => item.status === "open" && !transferredIssueIds.has(item.id))
				);
			})
		)
			throw new Error("synthesis must wait for local review repairs");
	}

	resolveRepairInput(ref: string): string {
		return resolveRepairInputFromSnapshot(this.snapshot, ref);
	}

	assertTaskCurrent(taskId: string, packet?: TaskPacket): void {
		const task = this.snapshot.tasks[taskId];
		if (!task) throw new Error(`unknown task ${taskId}`);
		if (packet) {
			const { status: _status, version: _version, ...binding } = packet;
			const { status: _currentStatus, version: _currentVersion, ...current } = task;
			if (
				checksum(binding) !== checksum(current) ||
				(packet.version && checksum(packet.version) !== checksum(task.version))
			)
				throw new StaleResearchInputError(`task packet differs from its registered identity: ${taskId}`);
		}
		if (!taskIsCurrentFromSnapshot(this.snapshot, task))
			throw new StaleResearchInputError(`task identity, inputs or approved plan is stale: ${taskId}`);
		this.assertCurrentInputs(task.inputArtifactRefs, task.repairOfEvidenceId, new Set(), task);
	}

	assertEvidenceCurrent(evidenceId: string, supplied?: Evidence): void {
		const evidence = this.snapshot.evidence[evidenceId];
		if (!evidence) throw new Error(`unknown evidence ${evidenceId}`);
		if (supplied) {
			const immutable = (value: Evidence) => {
				const {
					status: _status,
					acceptanceAuthority: _authority,
					mainAgentDecisionRef: _decision,
					supersededByTaskId: _superseded,
					...binding
				} = value;
				return JSON.parse(JSON.stringify(binding)) as unknown;
			};
			if (checksum(immutable(supplied)) !== checksum(immutable(evidence)))
				throw new StaleResearchInputError(`evidence differs from its registered identity: ${evidenceId}`);
		}
		if (!evidenceHasCurrentPlanApprovalFromSnapshot(this.snapshot, evidence))
			throw new StaleResearchInputError(`evidence identity, inputs or approved plan is stale: ${evidenceId}`);
		this.assertTaskCurrent(evidence.taskId);
	}

	private assertCurrentInputs(
		refs: string[],
		repairOfEvidenceId?: string,
		checked = new Set<string>(),
		comparisonTask?: TaskPacket,
	): void {
		for (const ref of refs) {
			if (
				ref === repairOfEvidenceId &&
				Object.values(this.snapshot.obligations).some(
					(issue) => issue.evidenceId === ref || this.snapshot.reviews[issue.sourceReviewId]?.evidenceId === ref,
				)
			) {
				const historical = this.snapshot.evidence[ref];
				if (historical) {
					for (const oldRef of this.snapshot.tasks[historical.taskId].inputArtifactRefs) {
						const currentRef = this.resolveRepairInput(oldRef);
						if (!refs.includes(currentRef)) {
							if (currentRef !== oldRef)
								throw new StaleResearchInputError(`Repair omits current input ${currentRef}`);
							throw new Error(`Repair omits current input ${currentRef}`);
						}
					}
					// The failed snapshot is a comparison target, not a current scientific input.
					continue;
				}
			}
			if (checked.has(ref)) continue;
			checked.add(ref);
			if (this.snapshot.retiredArtifacts[ref] || this.snapshot.canonical[ref]?.status === "stale")
				throw new StaleResearchInputError(`task input version is stale: ${ref}`);
			const retiredEvidence = Object.values(this.snapshot.retiredArtifacts).some(
				(receipt) => receipt.evidenceId === ref,
			);
			const staleEvidence = Object.values(this.snapshot.canonical).some(
				(artifact) => artifact.evidenceId === ref && artifact.status === "stale",
			);
			if (retiredEvidence || staleEvidence) throw new StaleResearchInputError(`task input version is stale: ${ref}`);
			if (comparisonTask && taskHasBoundRepairAncestor(this.snapshot, comparisonTask, ref)) continue;
			const evidence = this.snapshot.evidence[this.snapshot.canonical[ref]?.evidenceId ?? ref];
			if (
				evidence &&
				(evidence.status === "rejected" ||
					evidence.supersededByTaskId ||
					!evidenceHasCurrentPlanApprovalFromSnapshot(this.snapshot, evidence))
			)
				throw new StaleResearchInputError(`task input evidence is stale: ${ref}`);
			if (
				evidence &&
				!Object.values(this.snapshot.canonical).some(
					(artifact) => artifact.evidenceId === ref && artifact.status === "active",
				)
			)
				this.assertCurrentInputs(
					this.snapshot.tasks[evidence.taskId].inputArtifactRefs,
					this.snapshot.tasks[evidence.taskId].repairOfEvidenceId,
					checked,
					this.snapshot.tasks[evidence.taskId],
				);
		}
	}

	private taskVersionContractHash(task: TaskPacket, stage = this.definitions[task.stageId]): string {
		return checksum({
			planId: task.planId,
			repairChecks: task.repairChecks,
			deliveryKind: task.deliveryKind,
			stageRevision: task.stageRevision,
			repairOfEvidenceId: task.repairOfEvidenceId,
			stage,
			objective: task.objective,
			requiredOutputFields: task.requiredOutputFields,
			acceptanceChecks: task.acceptanceChecks,
			failureSignals: task.failureSignals,
			successCriteria: task.successCriteria,
		});
	}

	private taskVersionContractMatches(task: TaskPacket): boolean {
		const original = this.definitions[task.stageId];
		const normalized = JSON.parse(JSON.stringify(original)) as StageDefinition;
		const stages = [original, normalized];
		// Default stage definitions historically hashed this explicit undefined key before JSON storage removed it.
		if (!Object.hasOwn(normalized, "searchPolicy")) stages.push({ ...normalized, searchPolicy: undefined });
		return stages.some((stage) => this.taskVersionContractHash(task, stage) === task.version?.contractHash);
	}

	async captureTaskVersion(taskId: string): Promise<TaskVersion> {
		const task = this.snapshot.tasks[taskId];
		if (!task) throw new Error(`unknown task ${taskId}`);
		this.assertCurrentInputs(task.inputArtifactRefs, task.repairOfEvidenceId, new Set(), task);
		const git = await captureGitVersion(
			task.scope.workspaceRoot,
			join(task.scope.workspaceRoot, ".astra", "jobs", task.jobId, "versions", "git"),
		);
		if (task.version && !this.taskVersionContractMatches(task))
			throw new Error(`task version changed; create a new attempt: ${taskId}`);
		const contractHash =
			task.version?.contractHash ??
			this.taskVersionContractHash(
				task,
				JSON.parse(JSON.stringify(this.definitions[task.stageId])) as StageDefinition,
			);
		const inputs = task.inputArtifactRefs.map((ref) => {
			const artifact = this.snapshot.canonical[ref];
			const evidence = this.snapshot.evidence[artifact?.evidenceId ?? ref];
			return { ref, checksum: evidence?.versionHash ?? artifact?.checksum ?? evidence?.checksum ?? "unresolved" };
		});
		const value = {
			contractHash,
			inputs,
			git,
			runtime: { node: process.version, platform: process.platform, arch: process.arch },
		};
		const version = { ...value, hash: checksum(value) };
		if (task.version) {
			if (task.version.hash !== version.hash)
				throw new Error(`task version changed; create a new attempt: ${taskId}`);
			return structuredClone(task.version);
		}
		await this.commit({ type: "task_version_recorded", taskId, version });
		return version;
	}

	async setTaskStatus(taskId: string, status: TaskStatus): Promise<void> {
		return this.exclusive(async () => {
			const task = this.snapshot.tasks[taskId];
			if (!task) throw new Error(`unknown task ${taskId}`);
			if (task.status === "succeeded" && status !== "succeeded")
				throw new Error("succeeded task cannot move backwards");
			await this.appendEvent({ type: "task_status", taskId, status });
			if (
				status === "failed" &&
				task.role === "worker" &&
				task.searchBatchId &&
				task.searchCandidateId &&
				task.attempt >= MAX_TASK_ATTEMPTS
			) {
				await this.markSearchCandidateFailedInternal(task.searchBatchId, task.searchCandidateId);
			}
		});
	}

	private async markSearchCandidateFailedInternal(batchId: string, candidateId: string): Promise<void> {
		const batch = this.snapshot.searchBatches[batchId];
		const candidate = batch?.candidates[candidateId];
		if (!batch || !candidate || !["planning", "running", "evaluating"].includes(batch.status)) return;
		if (candidate.status !== "failed")
			await this.appendEvent({
				type: "search_candidate_updated",
				batchId,
				candidate: { ...candidate, status: "failed" },
			});
		const refreshedBatch = this.snapshot.searchBatches[batchId];
		if (
			refreshedBatch &&
			["planning", "running", "evaluating"].includes(refreshedBatch.status) &&
			Object.values(refreshedBatch.candidates).every((entry) => entry.status === "failed")
		) {
			await this.appendEvent({
				type: "search_batch_exhausted",
				batchId,
				rationale: `All search candidates failed after ${MAX_TASK_ATTEMPTS} worker attempts`,
			});
		}
	}

	async markSearchCandidateFailed(batchId: string, candidateId: string): Promise<void> {
		return this.exclusive(() => this.markSearchCandidateFailedInternal(batchId, candidateId));
	}

	async failUncommittedReviewerTask(taskId: string): Promise<void> {
		return this.exclusive(async () => {
			const task = this.snapshot.tasks[taskId];
			if (!task) throw new Error(`unknown task ${taskId}`);
			if (task.role !== "reviewer") throw new Error("only reviewer tasks may use uncommitted review recovery");
			if (Object.values(this.snapshot.reviews).some((review) => review.reviewerTaskId === taskId)) {
				throw new Error("committed reviewer task cannot move backwards");
			}
			if (task.status === "failed") return;
			await this.appendEvent({ type: "task_status", taskId, status: "failed" });
		});
	}

	async recordChildSession(session: ChildSessionRecord): Promise<void> {
		await this.commit({ type: "child_session_recorded", session });
	}

	private async prepareEvidence(
		input: Omit<Evidence, "id" | "checksum" | "createdAt" | "status"> & { id?: string },
		completeWorker = false,
	): Promise<Evidence> {
		const task = this.snapshot.tasks[input.taskId];
		if (!task) throw new Error(`unknown evidence task ${input.taskId}`);
		if (!completeWorker && task.status !== "succeeded") throw new Error("evidence requires a succeeded task");
		if (input.type !== task.requiredOutputType || input.stageId !== task.stageId)
			throw new Error("evidence does not match task delivery contract");
		if (task.role !== "worker" && !(task.role === "main-agent" && input.type === "stage-plan"))
			throw new Error("evidence source task has the wrong role");
		this.assertTaskCurrent(task.id);
		this.assertCurrentInputs(task.inputArtifactRefs, task.repairOfEvidenceId, new Set(), task);
		if (input.incrementalRevision) {
			const revision = input.incrementalRevision;
			const base = this.snapshot.evidence[revision.baseEvidenceId];
			if (
				!base ||
				base.id !== task.repairOfEvidenceId ||
				!task.inputArtifactRefs.includes(base.id) ||
				base.stageId !== task.stageId ||
				base.currentEvidenceSetId !== input.currentEvidenceSetId ||
				checksum(base.content) !== revision.baseHash ||
				checksum(input.content) !== revision.resultHash
			)
				throw new Error("incremental evidence no longer matches its declared base or merged content");
		}
		const boundSources =
			task.version && input.refs.some((ref) => sourceReceiptFilename(ref))
				? (await this.recoverWorkerManifest(task))?.sourceRefs
				: undefined;
		const files = await freezeEvidenceFiles(task, input.refs, boundSources);
		const evidence: Evidence = {
			...input,
			files,
			taskVersion: task.version,
			versionHash: checksum({
				content: input.content,
				refs: input.refs,
				files,
				taskVersion: task.version,
				...(input.incrementalRevision ? { incrementalRevision: input.incrementalRevision } : {}),
			}),
			id: input.id ?? `evidence_${randomUUID()}`,
			checksum: checksum(
				input.incrementalRevision
					? { content: input.content, incrementalRevision: input.incrementalRevision }
					: input.content,
			),
			createdAt: new Date().toISOString(),
			status: "candidate",
			currentEvidenceSetId: input.currentEvidenceSetId ?? `${input.stageId}::${input.taskId}`,
		};
		return evidence;
	}

	private evidenceCompletion(evidence: Evidence, taskSucceeded: boolean): EvidenceCompletion {
		if (evidence.type === "stage-plan") return { taskSucceeded };
		const task = this.snapshot.tasks[evidence.taskId];
		const candidate = task.searchBatchId
			? this.snapshot.searchBatches[task.searchBatchId]?.candidates[task.searchCandidateId ?? ""]
			: undefined;
		const node = createResearchNode({
			id: `research_evidence_${evidence.id}`,
			kind: "evidence",
			statement: `${evidence.type} evidence from task ${evidence.taskId}`,
			status: "active",
			stageId: evidence.stageId,
			domainRef: evidence.id,
			sourceRefs: evidence.refs,
		});
		node.createdAt = evidence.createdAt;
		node.updatedAt = evidence.createdAt;
		const edge = createResearchEdge({
			fromNodeId: node.id,
			toNodeId: candidate?.graphNodeId ?? this.snapshot.graph.rootQuestionId,
			kind: "tests",
			sourceRefs: [evidence.id],
		});
		edge.createdAt = evidence.createdAt;
		return { taskSucceeded, node, edge };
	}

	async recordEvidence(
		input: Omit<Evidence, "id" | "checksum" | "createdAt" | "status"> & { id?: string },
	): Promise<Evidence> {
		return this.exclusive(async () => {
			const evidence = await this.prepareEvidence(input);
			await this.appendEvent({
				type: "evidence_recorded",
				evidence,
				completion: this.evidenceCompletion(evidence, false),
			});
			return evidence;
		});
	}

	private completionTaskBinding(task: TaskPacket): Omit<TaskPacket, "status"> {
		const { status: _status, ...binding } = task;
		return JSON.parse(JSON.stringify(binding)) as Omit<TaskPacket, "status">;
	}

	/** Only these fixed current-task paths are valid result recovery inputs. */
	private async readTaskCompletionFile(
		task: TaskPacket,
		filename:
			| "evidence-completion.json"
			| "output-manifest.json"
			| "task-packet.json"
			| "review-packet.json"
			| "review-target-snapshot.json"
			| "review-criteria.json"
			| "review-target-evidence.json"
			| "review-manifest.json",
	): Promise<string | undefined> {
		const project = resolve(task.scope.workspaceRoot);
		if (project !== resolve(this.snapshot.frame.permissions.workspaceRoot))
			throw new Error("completion task workspace identity does not match job");
		const root = taskDir(project, task.jobId, task.id);
		try {
			if (
				(await lstat(root)).isSymbolicLink() ||
				(await realpath(root)) !== join(await realpath(project), relative(project, root))
			)
				throw new Error("completion path may not escape through symbolic links");
			const path = join(root, filename);
			const metadata = await lstat(path);
			if (
				!metadata.isFile() ||
				metadata.isSymbolicLink() ||
				(await realpath(path)) !== join(await realpath(root), filename)
			)
				throw new Error("completion result may not use symbolic links or non-files");
			return await readFile(path, "utf8");
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
			throw error;
		}
	}

	private assertCompletionTask(task: TaskPacket, evidence: Evidence): void {
		if (
			task.jobId !== this.snapshot.frame.jobId ||
			evidence.taskId !== task.id ||
			evidence.stageId !== task.stageId ||
			evidence.type !== task.requiredOutputType ||
			checksum({ version: evidence.taskVersion }) !== checksum({ version: task.version }) ||
			task.role !== "worker" ||
			task.status === "blocked" ||
			Object.values(this.snapshot.tasks).some(
				(next) =>
					next.supersedesTaskId === task.id || (next.replayKey === task.replayKey && next.attempt > task.attempt),
			) ||
			Object.values(this.snapshot.retiredArtifacts).some((receipt) => receipt.taskId === task.id) ||
			Object.values(this.snapshot.discardedEvidence).some((receipt) => receipt.taskId === task.id) ||
			Object.values(this.snapshot.discardedCandidates).some((receipt) => receipt.taskId === task.id) ||
			(task.stageRevision ?? 1) !== (this.snapshot.stages[task.stageId]?.revision ?? 1)
		)
			throw new StaleResearchInputError(`completion task identity, attempt or stage is stale: ${task.id}`);
		this.assertCurrentInputs(task.inputArtifactRefs, task.repairOfEvidenceId, new Set(), task);
		if (
			task.planId &&
			!evidenceHasCurrentPlanApprovalFromSnapshot(
				{ ...this.snapshot, evidence: { ...this.snapshot.evidence, [evidence.id]: evidence } },
				evidence,
			)
		)
			throw new StaleResearchInputError(`completion requires its frozen approved plan: ${task.id}`);
		if (task.version) {
			const { hash, ...version } = task.version;
			if (
				checksum(version) !== hash ||
				!this.taskVersionContractMatches(task) ||
				checksum(task.version.inputs) !==
					checksum(
						task.inputArtifactRefs.map((ref) => {
							const artifact = this.snapshot.canonical[ref];
							const input = this.snapshot.evidence[artifact?.evidenceId ?? ref];
							return {
								ref,
								checksum: input?.versionHash ?? artifact?.checksum ?? input?.checksum ?? "unresolved",
							};
						}),
					)
			)
				throw new StaleResearchInputError(`completion task version or inputs are stale: ${task.id}`);
		}
	}

	private async validatePreparedCompletion(task: TaskPacket, prepared: PreparedEvidenceCompletion): Promise<void> {
		const { preparationHash, ...value } = prepared;
		if (
			prepared.schemaVersion !== "astra.evidence_completion.v1" ||
			checksum(value) !== preparationHash ||
			checksum(prepared.task) !== checksum(this.completionTaskBinding(task))
		)
			throw new Error("prepared completion identity or integrity failure");
		const evidence = prepared.evidence;
		assertAstraId(evidence.id, "prepared evidence id");
		if (
			evidence.taskId !== task.id ||
			evidence.stageId !== task.stageId ||
			evidence.type !== task.requiredOutputType ||
			evidence.status !== "candidate" ||
			evidence.checksum !==
				checksum(
					evidence.incrementalRevision
						? { content: evidence.content, incrementalRevision: evidence.incrementalRevision }
						: evidence.content,
				) ||
			evidence.versionHash !==
				checksum({
					content: evidence.content,
					refs: evidence.refs,
					files: evidence.files,
					taskVersion: evidence.taskVersion,
					...(evidence.incrementalRevision ? { incrementalRevision: evidence.incrementalRevision } : {}),
				}) ||
			checksum(prepared.completion) !== checksum(this.evidenceCompletion(evidence, true))
		)
			throw new Error("prepared completion evidence or graph integrity failure");
		this.assertCompletionTask(task, evidence);
		for (const file of evidence.files ?? []) {
			if (!evidence.refs.includes(file.sourceRef)) throw new Error("prepared file is not declared by evidence");
			await readVersionedFile(task, evidence, file.sourceRef, "", "");
		}
	}

	private async completeWorkerTaskInternal(
		taskId: string,
		output?: {
			artifactType: string;
			content: unknown;
			refs: string[];
			incrementalRevision?: Evidence["incrementalRevision"];
		},
		recordedEvidence?: Evidence | null,
	): Promise<Evidence | undefined> {
		const task = this.snapshot.tasks[taskId];
		if (!task) throw new Error(`unknown completion task ${taskId}`);
		const existing =
			recordedEvidence === undefined
				? Object.values(this.snapshot.evidence).find((evidence) => evidence.taskId === taskId)
				: recordedEvidence;
		if (existing) {
			if (!this.snapshot.evidence[existing.id])
				throw new StaleResearchInputError(`completion evidence identity is stale: ${existing.id}`);
			if (
				existing.status === "rejected" ||
				(task.stageRevision ?? 1) !== (this.snapshot.stages[task.stageId]?.revision ?? 1)
			)
				return structuredClone(existing);
			const completion = this.evidenceCompletion(existing, true);
			if (
				task.status !== "succeeded" ||
				(completion.node && !this.snapshot.graph.nodes[completion.node.id]) ||
				(completion.edge && !this.snapshot.graph.edges[completion.edge.id])
			) {
				this.assertCompletionTask(task, existing);
				await this.appendEvent({ type: "evidence_completion_recovered", evidenceId: existing.id, completion });
			}
			return structuredClone(existing);
		}
		const saved = await this.readTaskCompletionFile(task, "evidence-completion.json");
		let prepared: PreparedEvidenceCompletion;
		if (saved !== undefined) {
			prepared = JSON.parse(saved) as PreparedEvidenceCompletion;
			await this.validatePreparedCompletion(task, prepared);
		} else {
			if (!output) output = await this.recoverWorkerManifest(task);
			if (!output) {
				if (task.status === "succeeded")
					throw new StaleResearchInputError(
						`succeeded worker ${taskId} has a stale completion gap: no recoverable evidence completion or output manifest`,
					);
				return undefined;
			}
			const base = task.repairOfEvidenceId ? this.snapshot.evidence[task.repairOfEvidenceId] : undefined;
			const incrementalBase = output.incrementalRevision
				? this.snapshot.evidence[output.incrementalRevision.baseEvidenceId]
				: undefined;
			const evidence = await this.prepareEvidence(
				{
					taskId,
					stageId: task.stageId,
					type: output.artifactType,
					content: output.content,
					refs: [...new Set([...(incrementalBase?.refs ?? []), ...output.refs])],
					...(output.incrementalRevision ? { incrementalRevision: output.incrementalRevision } : {}),
					currentEvidenceSetId: base?.currentEvidenceSetId,
				},
				true,
			);
			this.assertCompletionTask(task, evidence);
			const value = JSON.parse(
				JSON.stringify({
					schemaVersion: "astra.evidence_completion.v1",
					task: this.completionTaskBinding(task),
					evidence,
					completion: this.evidenceCompletion(evidence, true),
				}),
			) as Omit<PreparedEvidenceCompletion, "preparationHash">;
			prepared = { ...value, preparationHash: checksum(value) };
			await mkdir(taskDir(task.scope.workspaceRoot, task.jobId, task.id), { recursive: true });
			// Validate the parent before atomicWriteJson can follow an unexpected ancestor link.
			await this.readTaskCompletionFile(task, "evidence-completion.json");
			await atomicWriteJson(
				join(taskDir(task.scope.workspaceRoot, task.jobId, task.id), "evidence-completion.json"),
				prepared,
			);
		}
		await this.appendEvent({
			type: "evidence_recorded",
			evidence: prepared.evidence,
			completion: prepared.completion,
		});
		return structuredClone(prepared.evidence);
	}

	async completeWorkerTask(
		taskId: string,
		output: {
			artifactType: string;
			content: unknown;
			refs: string[];
			incrementalRevision?: Evidence["incrementalRevision"];
		},
	): Promise<Evidence> {
		return this.exclusive(async () => (await this.completeWorkerTaskInternal(taskId, output))!);
	}

	async recoverWorkerTaskCompletion(taskId: string): Promise<Evidence | undefined> {
		return this.exclusive(async () => {
			const evidence = await this.completeWorkerTaskInternal(taskId);
			if (evidence) {
				await this.persist();
			}
			return evidence;
		});
	}

	private async recoverWorkerManifest(task: TaskPacket): Promise<
		| {
				artifactType: string;
				content: unknown;
				refs: string[];
				incrementalRevision?: Evidence["incrementalRevision"];
				sourceRefs: OutputRef[];
		  }
		| undefined
	> {
		const bytes = await this.readTaskCompletionFile(task, "output-manifest.json");
		if (bytes === undefined) return undefined;
		const packetBytes = await this.readTaskCompletionFile(task, "task-packet.json");
		if (!packetBytes || !task.version)
			throw new Error("worker completion manifest has no persisted task version binding");
		const packet = JSON.parse(packetBytes) as TaskPacket;
		if (checksum(this.completionTaskBinding(packet)) !== checksum(this.completionTaskBinding(task)))
			throw new Error("worker completion packet identity, attempt, version or contract mismatch");
		const manifest: WorkerOutputManifest = await readWorkerOutputManifest(
			join(taskDir(task.scope.workspaceRoot, task.jobId, task.id), "output-manifest.json"),
		);
		if (
			manifest.jobId !== task.jobId ||
			manifest.taskId !== task.id ||
			manifest.agentId !== task.agentId ||
			manifest.artifactType !== task.requiredOutputType ||
			manifest.validationErrors.length
		)
			throw new Error("worker completion manifest identity or validation mismatch");
		const prefix = `.astra/jobs/${task.jobId}/workspaces/${task.id}/`;
		const refs = manifest.outputRefs.map((ref) => {
			if (ref.kind !== "artifact" && ref.kind !== "log") return ref;
			if (!ref.ref.startsWith(prefix) || !ref.sha256 || !/^[a-f0-9]{64}$/.test(ref.sha256))
				throw new Error("worker completion file path or digest does not match this task");
			return { ...ref, ref: ref.ref.slice(prefix.length) };
		});
		const { resultHash: declaredResultHash, ...revision } = manifest.incrementalRevision ?? { resultHash: undefined };
		const validated = await validateWorkerSubmission(
			task,
			{
				artifactType: manifest.artifactType,
				content: manifest.incrementalRevision ? {} : manifest.content,
				refs,
				...(manifest.incrementalRevision
					? { incrementalRevision: revision as Omit<NonNullable<Evidence["incrementalRevision"]>, "resultHash"> }
					: {}),
			},
			{
				executionRoot: taskWorkspacePath(task.scope.workspaceRoot, task.jobId, task.id),
				sessionRef: manifest.sessionRef,
				minSourceRefs: taskStageContract(this.definitions[task.stageId], task).minSourceRefs,
				job: this,
				sourceRecoveryRefs: manifest.outputRefs.filter((ref) => ref.kind === "source"),
			},
		);
		if (
			checksum(validated.content) !== checksum(manifest.content) ||
			(manifest.incrementalRevision &&
				(validated.incrementalRevision?.resultHash !== declaredResultHash ||
					checksum(validated.incrementalRevision) !== checksum(manifest.incrementalRevision)))
		)
			throw new Error("worker completion merged content or incremental result hash mismatch");
		for (const ref of refs) {
			if (
				(ref.kind === "artifact" || ref.kind === "log") &&
				validated.outputRefs.find((entry) => entry.kind === ref.kind && entry.ref === ref.ref)?.sha256 !==
					ref.sha256
			)
				throw new Error("worker completion file changed after validation");
		}
		return {
			artifactType: manifest.artifactType,
			content: validated.content,
			refs: manifest.outputRefs.map((ref) => ref.ref),
			sourceRefs: manifest.outputRefs.filter((ref) => ref.kind === "source"),
			...(validated.incrementalRevision ? { incrementalRevision: validated.incrementalRevision } : {}),
		};
	}

	private async recoverWorkerCompletionsInternal(): Promise<void> {
		const evidenceByTask = new Map(
			Object.values(this.snapshot.evidence).map((evidence) => [evidence.taskId, evidence]),
		);
		const candidates = Object.values(this.snapshot.tasks).filter((task) => {
			if (task.role !== "worker" || task.status === "blocked") return false;
			const evidence = evidenceByTask.get(task.id);
			if (!evidence) return taskIsCurrentFromSnapshot(this.snapshot, task);
			if (!this.snapshot.evidence[evidence.id])
				throw new StaleResearchInputError(`completion evidence identity is stale: ${evidence.id}`);
			if (
				evidence.status === "rejected" ||
				(task.stageRevision ?? 1) !== (this.snapshot.stages[task.stageId]?.revision ?? 1)
			)
				return false;
			const completion = this.evidenceCompletion(evidence, true);
			return (
				task.status !== "succeeded" ||
				Boolean(
					(completion.node && !this.snapshot.graph.nodes[completion.node.id]) ||
						(completion.edge && !this.snapshot.graph.edges[completion.edge.id]),
				)
			);
		});
		if (!candidates.length) return;
		const superseded = new Set<string>();
		const latestAttempts = new Map<string, number>();
		for (const task of Object.values(this.snapshot.tasks)) {
			if (task.supersedesTaskId) superseded.add(task.supersedesTaskId);
			latestAttempts.set(task.replayKey, Math.max(latestAttempts.get(task.replayKey) ?? 0, task.attempt));
		}
		const archived = new Set(
			[
				...Object.values(this.snapshot.retiredArtifacts),
				...Object.values(this.snapshot.discardedEvidence),
				...Object.values(this.snapshot.discardedCandidates),
			].map((receipt) => receipt.taskId),
		);
		for (const task of candidates) {
			if (superseded.has(task.id) || task.attempt < latestAttempts.get(task.replayKey)! || archived.has(task.id))
				continue;
			await this.completeWorkerTaskInternal(task.id, undefined, evidenceByTask.get(task.id) ?? null);
		}
	}

	historicalRepairArchives(stageId: string): Array<{
		artifactId: string;
		type: string;
		evidenceId: string;
		taskId: string;
		archiveRefs?: string[];
	}> {
		if (this.backtrackChecks(stageId).length === 0) return [];
		return [
			...Object.values(this.snapshot.retiredArtifacts),
			...Object.values(this.snapshot.discardedEvidence).map((receipt) => ({
				...receipt,
				artifactId: receipt.evidenceId,
				type: this.snapshot.tasks[receipt.taskId].requiredOutputType,
			})),
		].filter((receipt) => this.snapshot.tasks[receipt.taskId]?.stageId === stageId);
	}

	backtrackChecks(stageId: string): Array<{ nodeId: string; criterion: string }> {
		return backtrackChecksFromSnapshot(this.snapshot, stageId);
	}

	repairCriterion(criterion: string): string {
		for (const issue of Object.values(this.snapshot.obligations)) {
			const findings = this.snapshot.reviews[issue.sourceReviewId]?.findings ?? [];
			for (const item of issue.items ?? []) {
				if (criterion === `[${item.id}] ${item.criterion}`)
					return `[${item.id}] ${this.repairCriterion(item.criterion)}`;
			}
			if (findings.includes(criterion)) return `Verify that the reported issue is resolved: ${criterion}`;
		}
		return criterion;
	}

	normalizedRepairCriterion(criterion: string): string {
		return repairContext(this.snapshot).normalize(criterion);
	}

	async recordReview(input: Omit<Review, "id" | "createdAt"> & { id?: string }): Promise<Review> {
		return this.exclusive(() => this.recordReviewInternal(input));
	}

	/** Shared async preflight for Pi, Codex and registration. Integrity precedes semantic judgment. */
	async validateReview(input: Omit<Review, "id" | "createdAt">): Promise<void> {
		try {
			const evidence = this.snapshot.evidence[input.evidenceId];
			if (!evidence) throw new Error(`unknown evidence ${input.evidenceId}`);
			this.assertEvidenceCurrent(evidence.id);
			if (input.targetVersionHash && input.targetVersionHash !== evidence.versionHash)
				throw new Error("review target version does not match evidence");
			const refs = await this.reviewReferences(evidence.id, input.reviewerTaskId);
			validateReviewAssessment(input, frozenReviewCriteria(this.snapshot.tasks[evidence.taskId]));
			validateReviewReferences(input, refs);
		} catch (error) {
			if (error instanceof ReviewAssessmentError) throw error;
			throw new ReviewIntegrityError(error instanceof Error ? error.message : String(error), { cause: error });
		}
	}

	async reviewReferences(evidenceId: string, reviewerTaskId?: string): Promise<Set<string>> {
		const evidence = this.snapshot.evidence[evidenceId];
		if (!evidence) throw new Error(`unknown evidence ${evidenceId}`);
		const source = this.snapshot.tasks[evidence.taskId];
		const refs = new Set([
			evidence.id,
			`evidence:${evidence.id}`,
			...evidence.refs,
			...source.inputArtifactRefs,
			...source.inputArtifactRefs.flatMap(
				(ref) => this.snapshot.evidence[this.snapshot.canonical[ref]?.evidenceId ?? ref]?.refs ?? [],
			),
		]);
		if (reviewerTaskId === undefined) return refs;
		const task = this.snapshot.tasks[reviewerTaskId];
		if (this.snapshot.reviewDeliveryRejections?.[reviewerTaskId])
			throw new Error("reviewer delivery was already durably rejected");
		if (
			!task ||
			task.role !== "reviewer" ||
			task.jobId !== this.snapshot.frame.jobId ||
			task.stageId !== evidence.stageId ||
			(task.stageRevision ?? 1) !== (source.stageRevision ?? 1) ||
			task.inputArtifactRefs.length !== 1 ||
			task.inputArtifactRefs[0] !== evidence.id
		)
			throw new Error("reviewer task identity does not match its frozen target");
		this.assertTaskCurrent(task.id);
		const [taskBytes, packetBytes, snapshotBytes] = await Promise.all([
			this.readTaskCompletionFile(task, "task-packet.json"),
			this.readTaskCompletionFile(task, "review-packet.json"),
			this.readTaskCompletionFile(task, "review-target-snapshot.json"),
		]);
		if (!taskBytes || !packetBytes || !snapshotBytes)
			throw new Error("review completion has missing frozen packages");
		const savedTask = JSON.parse(taskBytes) as TaskPacket;
		const packet = JSON.parse(packetBytes) as ReviewPacket;
		const frozen = JSON.parse(snapshotBytes) as {
			evidence: Evidence;
			reviewCriteria?: Array<{ criterion: string; frozenCriteria: string[] }>;
			resolvedEvidenceRefs: ReviewPacket["resolvedEvidenceRefs"];
			resources?: Array<{ artifactId: string; artifactType: string; taskId: string; root: string }>;
		};
		const definition = taskStageContract(this.definitions[evidence.stageId], source);
		if (
			checksum(this.completionTaskBinding(savedTask)) !== checksum(this.completionTaskBinding(task)) ||
			packet.schemaVersion !== "astra.review_packet.v1" ||
			packet.jobId !== task.jobId ||
			packet.taskId !== task.id ||
			packet.evidenceId !== evidence.id ||
			packet.reviewerRole !== "reviewer" ||
			packet.objective !== task.objective ||
			checksum(packet.inputRefs) !== checksum(task.inputArtifactRefs) ||
			packet.targetSnapshotRef !== reviewSnapshotPath(task.scope.workspaceRoot, task.jobId, task.id) ||
			packet.targetSnapshotHash !== evidence.versionHash ||
			!Array.isArray(packet.resolvedEvidenceRefs) ||
			checksum(packet.resolvedEvidenceRefs) !== checksum(frozen.resolvedEvidenceRefs) ||
			checksum(packet.workerContract) !==
				checksum({
					objective: source.objective,
					requiredOutputFields: source.requiredOutputFields,
					acceptanceChecks: source.acceptanceChecks,
					failureSignals: source.failureSignals,
					successCriteria: source.successCriteria,
				}) ||
			checksum(packet.stageContract) !==
				checksum({
					stageId: definition.id,
					label: definition.label,
					outputArtifactType: definition.outputArtifactType,
					requiredOutputFields: definition.requiredOutputFields,
					acceptanceChecks: definition.acceptanceChecks,
					failureSignals: definition.failureSignals,
				})
		)
			throw new Error("review completion identity, frozen target or contract mismatch");
		this.assertEvidenceCurrent(evidence.id, frozen.evidence);
		if (frozen.resources !== undefined) {
			const resources = [...(await taskInputResources(task, this)), ...(await taskInputResources(source, this))].map(
				({ artifactId, artifactType, taskId, root }) => ({ artifactId, artifactType, taskId, root }),
			);
			if (!Array.isArray(frozen.resources) || checksum(frozen.resources) !== checksum(resources))
				throw new Error("review resource identity binding does not match its declared tasks");
		}
		if (
			frozen.reviewCriteria &&
			checksum(frozen.reviewCriteria.flatMap((group) => group.frozenCriteria).sort()) !==
				checksum(frozenReviewCriteria(source).sort())
		)
			throw new Error("review frozen criteria do not match the worker contract");
		await verifyReviewEvidenceBundle(task, evidence, packet, this);
		for (const ref of [
			"review-packet.json",
			"review-target-snapshot.json",
			...(await this.boundReviewAuxiliaryRefs(task, frozen)),
			...(frozen.resources ?? []).map((resource) => resource.artifactId),
			...packet.resolvedEvidenceRefs.flatMap((ref) => [ref.sourceRef, ref.path]),
		])
			refs.add(ref);
		return refs;
	}

	private async recordReviewInternal(input: Omit<Review, "id" | "createdAt"> & { id?: string }): Promise<Review> {
		const history = checkReviewHistory(await this.store.readEvents(this.snapshot.frame.jobId));
		const existing = input.id ? (history.get(input.id) ?? this.snapshot.reviews[input.id]) : undefined;
		if (existing) {
			const { createdAt: _createdAt, ...body } = existing;
			const { createdAt: _providedCreatedAt, ...submitted } = input as Review;
			if (
				reviewBody(body) !==
				reviewBody({
					...submitted,
					id: existing.id,
					targetVersionHash:
						input.targetVersionHash ??
						this.snapshot.evidence[input.evidenceId]?.versionHash ??
						existing.targetVersionHash,
				})
			)
				throw new Error(`review id already has different content: ${existing.id}`);
			await this.recoverReviewsInternal();
			if (existing.reviewerTaskId)
				await this.recoverReviewerCompletionsInternal(existing.evidenceId, existing.reviewerTaskId);
			return structuredClone(existing);
		}
		await this.validateReview(input);
		const evidence = this.snapshot.evidence[input.evidenceId];
		if (
			input.reviewerTaskId &&
			Object.values(this.snapshot.reviews).some(
				(review) => review.evidenceId === evidence.id && review.reviewerTaskId === input.reviewerTaskId,
			)
		)
			throw new Error("reviewer task already reviewed this evidence");
		const review: Review = {
			...input,
			targetVersionHash: evidence.versionHash,
			id: input.id ?? `review_${randomUUID()}`,
			createdAt: new Date().toISOString(),
		};
		await this.appendEvent({ type: "review_recorded", review, consequences: this.reviewConsequences(review) });
		return review;
	}

	private async boundReviewAuxiliaryRefs(
		task: TaskPacket,
		frozen: { evidence: Evidence; reviewCriteria?: Array<{ criterion: string }> },
	): Promise<string[]> {
		const refs: string[] = [];
		for (const [filename, expected] of [
			["review-criteria.json", frozen.reviewCriteria?.map((group) => group.criterion)],
			["review-target-evidence.json", frozen.reviewCriteria ? frozen.evidence : undefined],
		] as const) {
			const bytes = await this.readTaskCompletionFile(task, filename);
			if (!expected) continue;
			if (!bytes || checksum(JSON.parse(bytes)) !== checksum(JSON.parse(JSON.stringify(expected))))
				throw new Error(`review auxiliary file identity mismatch: ${filename}`);
			refs.push(filename);
		}
		return refs;
	}

	/** Finish the registered reviewer's own delivery before any further execution or error classification. */
	async recoverReviewerTaskCompletions(evidenceId: string, taskId?: string): Promise<Review[]> {
		return this.exclusive(() => this.recoverReviewerCompletionsInternal(evidenceId, taskId));
	}

	private async recoverReviewerCompletionsInternal(evidenceId?: string, taskId?: string): Promise<Review[]> {
		const recovered: Review[] = [];
		for (const task of Object.values(this.snapshot.tasks)) {
			if (
				task.role !== "reviewer" ||
				(taskId && task.id !== taskId) ||
				(evidenceId && !taskId && !task.inputArtifactRefs.includes(evidenceId))
			)
				continue;
			const committed = Object.values(this.snapshot.reviews).filter((review) => review.reviewerTaskId === task.id);
			if (committed.length) {
				if (committed.length !== 1 || (evidenceId && committed[0].evidenceId !== evidenceId))
					throw new Error("reviewer completion conflicts with its registered review");
				await this.finishReviewSuccessInternal(task);
				if (taskId) recovered.push(structuredClone(committed[0]));
				continue;
			}
			const rejection = this.snapshot.reviewDeliveryRejections?.[task.id];
			if (rejection) {
				await this.finishReviewRejectionInternal(task, rejection);
				continue;
			}
			const evidence = this.snapshot.evidence[task.inputArtifactRefs[0]];
			if (
				task.status === "blocked" ||
				!evidence ||
				evidence.status === "rejected" ||
				evidence.supersededByTaskId ||
				(task.stageRevision ?? 1) !== (this.snapshot.stages[task.stageId]?.revision ?? 1) ||
				Object.values(this.snapshot.tasks).some(
					(next) =>
						next.supersedesTaskId === task.id ||
						(next.replayKey === task.replayKey && next.attempt > task.attempt),
				) ||
				Object.values(this.snapshot.canonical).some(
					(artifact) => artifact.evidenceId === evidence.id && artifact.status === "stale",
				)
			)
				continue;
			if (evidence.type === "stage-plan") {
				const planId = this.snapshot.tasks[evidence.taskId]?.planId;
				if (!planId || planReviewStatus(this, planId) === "stale") continue;
			} else if (!evidenceHasCurrentPlanApprovalFromSnapshot(this.snapshot, evidence)) continue;
			if (!taskIsCurrentFromSnapshot(this.snapshot, task)) continue;
			const bytes = await this.readTaskCompletionFile(task, "review-manifest.json");
			if (bytes === undefined) {
				if (
					task.status === "succeeded" ||
					Object.values(this.snapshot.sessions).some(
						(session) =>
							session.taskId === task.id &&
							session.role === "reviewer" &&
							session.attempt === task.attempt &&
							session.status === "completed",
					)
				)
					throw new Error(`reviewer ${task.id} has a completion gap: no review manifest`);
				continue;
			}
			for (const peer of Object.values(this.snapshot.tasks).filter(
				(peer) =>
					peer.id !== task.id &&
					peer.role === "reviewer" &&
					peer.replayKey === task.replayKey &&
					peer.attempt === task.attempt &&
					peer.stageId === task.stageId &&
					(peer.stageRevision ?? 1) === (task.stageRevision ?? 1) &&
					peer.status !== "blocked" &&
					!this.snapshot.reviewDeliveryRejections?.[peer.id] &&
					!Object.values(this.snapshot.reviews).some((review) => review.reviewerTaskId === peer.id) &&
					!Object.values(this.snapshot.tasks).some((next) => next.supersedesTaskId === peer.id),
			)) {
				if (
					(await this.readTaskCompletionFile(peer, "review-manifest.json")) !== undefined ||
					peer.status === "succeeded" ||
					Object.values(this.snapshot.sessions).some(
						(session) =>
							session.taskId === peer.id &&
							session.role === "reviewer" &&
							session.attempt === peer.attempt &&
							session.status === "completed",
					)
				)
					throw new Error("reviewer completion is ambiguous for the same replay key and attempt");
			}
			const manifest = JSON.parse(bytes) as ReviewerOutputManifest;
			if (
				manifest.schemaVersion !== "astra.reviewer_output_manifest.v1" ||
				!["pass", "fail", "partial", "blocked"].includes(manifest.verdict) ||
				!Array.isArray(manifest.findings) ||
				manifest.findings.some((finding) => typeof finding !== "string") ||
				manifest.jobId !== task.jobId ||
				manifest.taskId !== task.id ||
				manifest.evidenceId !== evidence.id
			)
				throw new Error("review completion manifest identity mismatch");
			const source = this.snapshot.tasks[evidence.taskId];
			const input = {
				evidenceId: evidence.id,
				reviewerTaskId: task.id,
				targetVersionHash: evidence.versionHash,
				verdict: manifest.verdict,
				findings: manifest.findings,
				score: manifest.score,
				criteria: manifest.criteria,
				verifiedRefs: manifest.verifiedRefs,
				blocking: evidence.type !== "stage-plan" && source.searchBatchId === undefined,
			};
			try {
				await this.validateReview(input);
			} catch (error) {
				if (!(error instanceof ReviewAssessmentError)) throw error;
				const rejection: ReviewDeliveryRejection = {
					taskId: task.id,
					attempt: task.attempt,
					evidenceId: evidence.id,
					targetVersionHash: evidence.versionHash!,
					manifestSha256: sha256(bytes),
					reason: error.message,
					createdAt: new Date().toISOString(),
				};
				await this.appendEvent({ type: "review_delivery_rejected", rejection });
				await this.finishReviewRejectionInternal(task, rejection);
				continue;
			}
			const review = await this.recordReviewInternal(input);
			await this.finishReviewSuccessInternal(task);
			recovered.push(review);
		}
		return recovered;
	}

	private async finishReviewSuccessInternal(task: TaskPacket): Promise<void> {
		if (task.status !== "succeeded")
			await this.appendEvent({ type: "task_status", taskId: task.id, status: "succeeded" });
		for (const session of Object.values(this.snapshot.sessions)) {
			if (
				session.taskId !== task.id ||
				session.role !== "reviewer" ||
				session.attempt !== task.attempt ||
				session.status === "completed"
			)
				continue;
			await this.appendEvent({
				type: "child_session_recorded",
				session: {
					...session,
					status: "completed",
					error: undefined,
					manifestRef: reviewerManifestPath(task.scope.workspaceRoot, task.jobId, task.id),
					updatedAt: new Date().toISOString(),
				},
			});
		}
	}

	private async finishReviewRejectionInternal(task: TaskPacket, rejection: ReviewDeliveryRejection): Promise<void> {
		const bytes = await this.readTaskCompletionFile(task, "review-manifest.json");
		if (
			!bytes ||
			sha256(bytes) !== rejection.manifestSha256 ||
			task.id !== rejection.taskId ||
			task.attempt !== rejection.attempt ||
			task.inputArtifactRefs[0] !== rejection.evidenceId ||
			this.snapshot.evidence[rejection.evidenceId]?.versionHash !== rejection.targetVersionHash
		)
			throw new Error("rejected review manifest digest or task/target identity changed");
		if (task.status !== "failed") await this.appendEvent({ type: "task_status", taskId: task.id, status: "failed" });
		for (const session of Object.values(this.snapshot.sessions)) {
			if (
				session.taskId !== task.id ||
				session.role !== "reviewer" ||
				session.attempt !== task.attempt ||
				session.status === "failed"
			)
				continue;
			await this.appendEvent({
				type: "child_session_recorded",
				session: { ...session, status: "failed", error: rejection.reason, updatedAt: new Date().toISOString() },
			});
		}
	}

	/** Same constructor serves atomic new reviews and narrow historical repair. */
	private reviewConsequences(review: Review): ReviewConsequences {
		if (!["fail", "partial", "blocked"].includes(review.verdict) || review.blocking === false) return {};
		const evidence = this.snapshot.evidence[review.evidenceId];
		if (
			!evidence ||
			this.snapshot.discardedEvidence[evidence.id] ||
			Object.values(this.snapshot.discardedCandidates).some((receipt) => receipt.evidenceId === evidence.id)
		)
			return {};
		const saved = Object.values(this.snapshot.obligations).find((issue) => issue.sourceReviewId === review.id);
		const existingObjection =
			(saved?.graphObjectionId ? this.snapshot.graph.nodes[saved.graphObjectionId] : undefined) ??
			Object.values(this.snapshot.graph.nodes).find(
				(node) => node.kind === "objection" && node.domainRef === review.id,
			);
		const objection =
			existingObjection ??
			createResearchNode({
				id: saved?.graphObjectionId ?? `research_objection_${review.id}`,
				kind: "objection",
				statement: review.findings.join("; ") || "review failed",
				status: saved?.status === "resolved" ? "resolved" : "open",
				stageId: evidence.stageId,
				domainRef: review.id,
				sourceRefs: [review.id, evidence.id],
			});
		const edge = createResearchEdge({
			fromNodeId: objection.id,
			toNodeId: `research_evidence_${evidence.id}`,
			kind: "contradicts",
			sourceRefs: [review.id],
		});
		const consequences: ReviewConsequences = {
			...(!existingObjection ? { objection } : {}),
			...(!this.snapshot.graph.edges[edge.id] ? { edge } : {}),
		};
		if (saved) return consequences;

		const obligation: Obligation = {
			stageId: evidence.stageId,
			evidenceId: evidence.id,
			targetVersionHash: evidence.versionHash,
			id: `obligation_${review.id}`,
			sourceReviewId: review.id,
			description: review.findings.join("; ") || "review failed",
			graphObjectionId: objection.id,
			status: "open",
			createdAt: review.createdAt,
		};
		const existingCriteria = new Set(
			Object.values(this.snapshot.obligations)
				.filter((issue) => {
					const failed = this.snapshot.evidence[this.snapshot.reviews[issue.sourceReviewId]?.evidenceId];
					return issue.status === "open" && failed?.currentEvidenceSetId === evidence.currentEvidenceSetId;
				})
				.flatMap((issue) => (issue.items ?? []).map((item) => this.normalizedRepairCriterion(item.criterion))),
		);
		obligation.items = [
			...new Set([
				...(review.criteria ?? []).filter((criterion) => !criterion.passed).map((criterion) => criterion.criterion),
				...review.findings,
			]),
		]
			.filter((criterion) => {
				const normalized = this.normalizedRepairCriterion(criterion);
				if (existingCriteria.has(normalized)) return false;
				existingCriteria.add(normalized);
				return true;
			})
			.map((criterion, index) => ({ id: `${obligation.id}_${index + 1}`, criterion, status: "open" }));
		consequences.obligation = obligation;
		return consequences;
	}

	private async recoverReviewsInternal(): Promise<void> {
		for (const review of Object.values(this.snapshot.reviews)) {
			const consequences = this.reviewConsequences(review);
			if (Object.keys(consequences).length)
				await this.appendEvent({ type: "review_recorded", review, consequences });
		}
	}

	/** Explicit recovery only; open/reload remain read-only. */
	async recoverPendingOperations(): Promise<void> {
		return this.exclusive(async () => {
			for (const plan of Object.values(this.snapshot.stagePlans)) {
				if (
					plan.mode === "search" &&
					!Object.values(this.snapshot.searchBatches).some((batch) => batch.planId === plan.id)
				)
					await this.appendEvent({
						type: "search_batch_recorded",
						...this.searchRegistration(plan, this.definitions[plan.stageId]),
					});
			}
			for (const task of Object.values(this.snapshot.tasks)) {
				if (
					task.role === "worker" &&
					task.status === "failed" &&
					task.attempt >= MAX_TASK_ATTEMPTS &&
					task.searchBatchId &&
					task.searchCandidateId &&
					this.snapshot.searchBatches[task.searchBatchId]?.candidates[task.searchCandidateId]?.taskId === task.id
				)
					await this.markSearchCandidateFailedInternal(task.searchBatchId, task.searchCandidateId);
			}
			for (const batch of Object.values(this.snapshot.searchBatches))
				if (batch.status === "selected") await this.finishSearchSelectionInternal(batch);
			await this.finishCleanupsInternal();
			await this.recoverReviewsInternal();
			const recoveryHistory = await this.recoverAcceptancesInternal();
			await this.recoverRoutesInternal(recoveryHistory);
			await this.recoverWorkerCompletionsInternal();
			await this.recoverReviewerCompletionsInternal();
			for (const artifact of Object.values(this.snapshot.canonical)) {
				if (["stale", "retired"].includes(artifact.status) || artifact.adoptionCompletedAt) continue;
				await this.adoptEvidenceInternal(artifact.evidenceId, artifact.replacementOf);
			}
		});
	}

	async adoptEvidence(evidenceId: string, replacementOf?: string, decisionRef?: string): Promise<CanonicalArtifact> {
		return this.exclusive(() => {
			if (decisionRef) this.assertMainAgentPermission(decisionRef);
			return this.adoptEvidenceInternal(evidenceId, replacementOf, decisionRef);
		});
	}

	private async adoptEvidenceInternal(
		evidenceId: string,
		replacementOf?: string,
		decisionRef?: string,
	): Promise<CanonicalArtifact> {
		const evidence = this.snapshot.evidence[evidenceId];
		if (!evidence) throw new Error(`unknown evidence ${evidenceId}`);
		const existing = Object.values(this.snapshot.canonical).find((artifact) => artifact.evidenceId === evidenceId);
		if (existing) {
			if (
				["stale", "retired"].includes(existing.status) ||
				(replacementOf !== undefined && replacementOf !== existing.replacementOf && replacementOf !== existing.id)
			)
				throw new Error("unsafe repeated evidence adoption");
			if (existing.adoptionCompletedAt) {
				await this.finishCleanupsInternal();
				return structuredClone(existing);
			}
			if (
				existing.status === "active" &&
				this.snapshot.canonicalRoute.stageArtifactIds[evidence.stageId] !== existing.id
			)
				throw new StaleResearchInputError("historical adoption is no longer the current route");
		}
		const deliveryTask = this.snapshot.tasks[evidence.taskId];
		if (!existing) this.assertEvidenceCurrent(evidence.id);
		if (evidence.type === "stage-plan") throw new Error("a reviewed plan is not a research result");
		if (deliveryTask?.deliveryKind === "local")
			throw new Error("local evidence requires synthesis before canonical adoption");
		this.assertDeliveryInputs(evidence.stageId, deliveryTask?.deliveryKind, deliveryTask?.inputArtifactRefs ?? []);
		if (evidence.status !== "accepted") throw new Error("evidence must be accepted before adoption");
		const resultAssessment = evidence.type === "result-to-claim" ? scientificAssessment(evidence.content) : undefined;
		if (evidence.type === "result-to-claim" && !resultAssessment) {
			throw new Error("result-to-claim requires scientificOutcome and missionCoverage");
		}
		if (evidence.type === "result-to-claim") validateClaimAssessments(evidence.content);
		const sourceTask = this.snapshot.tasks[evidence.taskId];
		if ((sourceTask.stageRevision ?? 1) !== (this.snapshot.stages[evidence.stageId]?.revision ?? 1))
			throw new StaleResearchInputError("adoption stage revision is stale");
		const ownRetirement = existing?.replacementOf
			? this.snapshot.retiredArtifacts[existing.replacementOf]
			: undefined;
		const refs = sourceTask.inputArtifactRefs.filter(
			(ref) =>
				!(
					ownRetirement &&
					existing &&
					ownRetirement.replacementId === existing.id &&
					(ref === ownRetirement.artifactId || ref === ownRetirement.evidenceId)
				),
		);
		if (!existing || !sourceTask.planId)
			this.assertCurrentInputs(refs, sourceTask.repairOfEvidenceId, new Set(), sourceTask);
		if (sourceTask.planId && !existing && !evidenceHasCurrentPlanApprovalFromSnapshot(this.snapshot, evidence))
			throw new Error("adoption requires the frozen approved plan");
		if (sourceTask.planId && existing) {
			let basis = this.snapshot;
			const history =
				ownRetirement?.replacementId === existing.id || existing.status === "active"
					? await this.store.readEvents(this.snapshot.frame.jobId)
					: [];
			if (ownRetirement?.replacementId === existing.id) {
				// Only this adoption's historical replacement may be restored for contract checking.
				const oldEvidence = history.find(
					(saved) =>
						saved.event.type === "evidence_recorded" && saved.event.evidence.id === ownRetirement.evidenceId,
				)?.event;
				const oldArtifact = history.find(
					(saved) =>
						saved.event.type === "evidence_adopted" && saved.event.artifact.id === ownRetirement.artifactId,
				)?.event;
				if (oldEvidence?.type !== "evidence_recorded" || oldArtifact?.type !== "evidence_adopted")
					throw new Error("pending adoption replacement lineage unavailable");
				const materialized = history.find(
					(saved) =>
						saved.event.type === "canonical_artifact_materialized" &&
						saved.event.artifactId === ownRetirement.artifactId,
				)?.event;
				const retiredArtifacts = { ...this.snapshot.retiredArtifacts };
				delete retiredArtifacts[ownRetirement.artifactId];
				basis = {
					...this.snapshot,
					retiredArtifacts,
					evidence: {
						...this.snapshot.evidence,
						[ownRetirement.evidenceId]: { ...oldEvidence.evidence, status: "accepted" },
					},
					canonical: {
						...this.snapshot.canonical,
						[ownRetirement.artifactId]: {
							...oldArtifact.artifact,
							...(materialized?.type === "canonical_artifact_materialized"
								? {
										materializationRef: materialized.materializationRef,
										targetSha256: materialized.targetSha256,
									}
								: {}),
							status: "active",
						},
					},
				};
			}
			if (existing.status === "active") {
				// This committed acceptance changed its own comparison inputs; it did not invalidate its frozen contract.
				const planEvidence = planReviewEvidence({ state: this.snapshot }, sourceTask.planId);
				const frozen = (
					planEvidence?.content as
						| { effectiveContracts?: Array<{ hash: string; contract: EffectiveTaskContract }> }
						| undefined
				)?.effectiveContracts?.find((entry) => entry.hash === sourceTask.effectiveContractHash);
				const activationIndex = history.findIndex(
					(saved) =>
						saved.event.type === "canonical_artifact_status" &&
						saved.event.artifactId === existing.id &&
						saved.event.status === "active",
				);
				const acceptedIndex = history.findIndex(
					(saved) =>
						saved.event.type === "evidence_decided" &&
						saved.event.evidenceId === evidence.id &&
						saved.event.accepted,
				);
				const comparisons: Record<string, Evidence> = {};
				for (const input of frozen?.contract.inputVersions ?? []) {
					if (input.canonical || !input.evidenceId || !sourceTask.inputArtifactRefs.includes(input.inputRef))
						continue;
					let recordedIndex = -1;
					for (let index = 0; index < acceptedIndex; index++) {
						const event = history[index].event;
						if (event.type === "evidence_recorded" && event.evidence.id === input.evidenceId)
							recordedIndex = index;
					}
					const recorded = history[recordedIndex]?.event;
					if (recorded?.type !== "evidence_recorded") continue;
					const ancestor = recorded.evidence;
					const current = this.snapshot.evidence[ancestor.id];
					if (
						history.some(
							(saved, index) =>
								index > acceptedIndex &&
								saved.event.type === "evidence_recorded" &&
								saved.event.evidence.id === ancestor.id,
						)
					)
						throw new StaleResearchInputError(
							`historical adoption comparison is stale after re-recording: ${ancestor.id}`,
						);
					if (
						current &&
						(current.id !== ancestor.id ||
							current.taskId !== ancestor.taskId ||
							current.stageId !== ancestor.stageId ||
							current.type !== ancestor.type ||
							current.versionHash !== ancestor.versionHash ||
							current.versionHash !== input.versionHash ||
							(current.currentEvidenceSetId ?? current.taskId) !==
								(ancestor.currentEvidenceSetId ?? ancestor.taskId) ||
							current.checksum !== ancestor.checksum)
					)
						throw new StaleResearchInputError(
							`historical adoption comparison identity or version is stale: ${ancestor.id}`,
						);
					const discarded = this.snapshot.discardedEvidence[ancestor.id];
					let pruneIndex = -1;
					for (let index = acceptedIndex + 1; index < history.length; index++) {
						const event = history[index].event;
						if (event.type === "evidence_pruned" && event.receipt.evidenceId === ancestor.id) pruneIndex = index;
					}
					if (
						ancestor.id === evidence.id ||
						ancestor.stageId !== evidence.stageId ||
						this.snapshot.tasks[ancestor.taskId]?.jobId !== sourceTask.jobId ||
						(ancestor.currentEvidenceSetId ?? ancestor.taskId) !==
							(evidence.currentEvidenceSetId ?? evidence.taskId) ||
						ancestor.versionHash !== input.versionHash ||
						input.inputRef !== ancestor.id ||
						recordedIndex >= acceptedIndex ||
						acceptedIndex >= activationIndex ||
						(current
							? current.status !== "rejected" ||
								current.supersededByTaskId !== sourceTask.id ||
								current.checksum !== ancestor.checksum
							: discarded?.reason !== "superseded-repair" ||
								discarded.taskId !== ancestor.taskId ||
								discarded.checksum !== ancestor.checksum ||
								pruneIndex <= acceptedIndex) ||
						(input.status !== "candidate" && input.status !== "accepted" && input.status !== "rejected")
					)
						continue;
					comparisons[ancestor.id] = { ...ancestor, status: input.status, supersededByTaskId: sourceTask.id };
				}
				if (Object.keys(comparisons).length) basis = { ...basis, evidence: { ...basis.evidence, ...comparisons } };
			}
			if (!evidenceMatchesHistoricalPlanFromSnapshot(basis, evidence))
				throw new Error("pending adoption plan is stale");
		}
		if (sourceTask?.searchBatchId && sourceTask.searchCandidateId) {
			const batch = this.snapshot.searchBatches[sourceTask.searchBatchId];
			if (batch?.selectedCandidateId !== sourceTask.searchCandidateId) {
				throw new Error("search evidence must be selected before adoption");
			}
		}
		const reviews = Object.values(this.snapshot.reviews).filter((review) => review.evidenceId === evidenceId);
		if (
			reviews.some((review) => review.verdict !== "pass") ||
			!reviews.some((review) => review.verdict === "pass" && review.targetVersionHash === evidence.versionHash)
		)
			throw new Error("evidence requires an independent passing review");
		const evidenceSetId = evidence.currentEvidenceSetId ?? evidence.taskId;
		const acceptedRevisionRefs = Object.values(this.snapshot.evidence)
			.filter(
				(candidate) =>
					candidate.status === "accepted" &&
					(candidate.currentEvidenceSetId ?? candidate.taskId) === evidenceSetId,
			)
			.map((candidate) => candidate.id)
			.sort();
		if (acceptedRevisionRefs.length !== 1 || acceptedRevisionRefs[0] !== evidenceId)
			throw new Error("evidence revision is ambiguous");
		const currentRouteArtifactId = this.snapshot.canonicalRoute.stageArtifactIds[evidence.stageId];
		const replacedArtifactId = existing ? existing.replacementOf : (replacementOf ?? currentRouteArtifactId);
		if (
			existing &&
			currentRouteArtifactId &&
			currentRouteArtifactId !== existing.id &&
			currentRouteArtifactId !== replacedArtifactId
		)
			throw new StaleResearchInputError("pending adoption replacement route changed");
		const ownedReplacement = replacedArtifactId ? this.snapshot.retiredArtifacts[replacedArtifactId] : undefined;
		if (!existing && replacedArtifactId) {
			const replacement = this.snapshot.canonical[replacedArtifactId];
			if (
				!replacement ||
				replacement.status !== "active" ||
				replacedArtifactId !== currentRouteArtifactId ||
				replacement.type !== evidence.type ||
				this.snapshot.evidence[replacement.evidenceId]?.stageId !== evidence.stageId
			)
				throw new Error("replacement requires the current same-stage same-type active canonical artifact");
		}
		if (
			replacedArtifactId &&
			replacedArtifactId !== existing?.id &&
			!this.snapshot.canonical[replacedArtifactId] &&
			!(ownedReplacement && existing && ownedReplacement.replacementId === existing.id)
		)
			throw new Error(`unknown replacement artifact ${replacedArtifactId}`);
		const artifact: CanonicalArtifact = existing
			? structuredClone(existing)
			: {
					id: `artifact_${randomUUID()}`,
					mainAgentDecisionRef: decisionRef,
					type: evidence.type,
					evidenceId,
					content: evidence.content,
					checksum: evidence.checksum,
					status: "adoption_requested",
					replacementOf: replacedArtifactId,
					evidenceSnapshotHash: checksum({ stageId: evidence.stageId, acceptedRevisionRefs }),
					sourceSha256: evidence.checksum,
					adoptedAt: new Date().toISOString(),
				};
		artifact.targetSha256 ??= sha256(`${JSON.stringify(artifact.content, null, 2)}\n`);
		if (!existing) await this.appendEvent({ type: "evidence_adopted", artifact });
		const phases = ["adoption_requested", "materialized", "baseline_visible", "integration_verified", "active"];
		await access(this.snapshot.frame.permissions.workspaceRoot);
		const artifactPath = canonicalArtifactPath(
			this.snapshot.frame.permissions.workspaceRoot,
			this.snapshot.frame.jobId,
			artifact.id,
		);
		const receiptPath = canonicalReceiptPath(
			this.snapshot.frame.permissions.workspaceRoot,
			this.snapshot.frame.jobId,
			artifact.id,
		);
		const receipt = {
			schemaVersion: "astra.materialization_receipt.v1",
			artifactId: artifact.id,
			sourceSha256: artifact.sourceSha256,
			targetSha256: artifact.targetSha256,
			targetPath: artifactPath,
			createdAt: artifact.adoptedAt,
		};
		if (
			(artifact.materializationRef && artifact.materializationRef !== artifactPath) ||
			artifact.targetSha256 !== sha256(`${JSON.stringify(artifact.content, null, 2)}\n`) ||
			artifact.sourceSha256 !== evidence.checksum ||
			checksum(artifact.content) !== checksum(evidence.content)
		)
			throw new Error(`canonical artifact ${artifact.id} has invalid materialization identity`);
		for (const [path, value] of [
			[artifactPath, artifact.content],
			[receiptPath, receipt],
		] as const) {
			let bytes: string;
			try {
				bytes = await readFile(path, "utf8");
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
				await atomicWriteJson(path, value);
				bytes = await readFile(path, "utf8");
			}
			if (
				path === artifactPath
					? sha256(bytes) !== artifact.targetSha256
					: checksum(JSON.parse(bytes)) !== checksum(receipt)
			)
				throw new Error(`canonical artifact ${artifact.id} failed materialization integrity verification`);
		}
		if (!artifact.materializationRef) {
			await this.appendEvent({
				type: "canonical_artifact_materialized",
				artifactId: artifact.id,
				materializationRef: artifactPath,
				targetSha256: artifact.targetSha256,
			});
			artifact.materializationRef = artifactPath;
		}
		for (const status of ["materialized", "baseline_visible", "integration_verified"] as const) {
			if (phases.indexOf(artifact.status) >= phases.indexOf(status)) continue;
			await this.appendEvent({ type: "canonical_artifact_status", artifactId: artifact.id, status });
			artifact.status = status;
		}
		const completion = await this.adoptionCompletion(artifact, evidence, reviews, sourceTask, resultAssessment);
		await this.appendEvent({
			type: "canonical_artifact_status",
			artifactId: artifact.id,
			status: "active",
			completion,
		});
		await this.finishCleanupsInternal();
		return structuredClone(this.snapshot.canonical[artifact.id]);
	}

	/** Compute only this adoption's verified consequences before retirement removes its lineage. */
	private async adoptionCompletion(
		artifact: CanonicalArtifact,
		evidence: Evidence,
		reviews: Review[],
		sourceTask: TaskPacket,
		assessment: ReturnType<typeof scientificAssessment>,
	): Promise<AdoptionCompletion> {
		const evidenceId = evidence.id;
		const completion: AdoptionCompletion = {
			nodes: [],
			edges: [],
			resolvedNodeIds: [],
			repairItems: [],
			resolvedObligations: [],
			assessment,
			cleanups: [],
			completedAt: new Date().toISOString(),
		};
		const expectedPhase = sourceTask?.deliveryKind === "synthesis" ? "synthesis" : "stage";
		for (const binding of sourceTask?.responsibilityBindings ?? []) {
			if (
				binding.stageId === evidence.stageId &&
				binding.phase === expectedPhase &&
				sourceTask?.deliveryKind !== "local" &&
				this.backtrackChecks(binding.stageId).some(
					(check) => check.nodeId === binding.nodeId && sourceTask.acceptanceChecks.includes(check.criterion),
				)
			) {
				completion.resolvedNodeIds.push(binding.nodeId);
			}
		}
		if (sourceTask?.deliveryKind === "synthesis") {
			for (const transfer of sourceTask.responsibilityTransfers ?? []) {
				if (
					!transfer.issueId ||
					transfer.destinationPhase !== "synthesis" ||
					transfer.destinationStageId !== evidence.stageId
				)
					continue;
				const legacyTask = this.snapshot.tasks[transfer.sourceTaskId];
				const localEvidence = sourceTask.inputArtifactRefs
					.map((ref) => this.snapshot.evidence[ref])
					.find(
						(candidate) =>
							candidate?.status === "accepted" &&
							this.snapshot.tasks[candidate.taskId]?.deliveryKind === "local" &&
							this.snapshot.tasks[candidate.taskId]?.responsibilityTransfers?.some(
								(entry) => entry.issueId === transfer.issueId,
							),
					);
				const localTask = localEvidence ? this.snapshot.tasks[localEvidence.taskId] : undefined;
				const planEvidence = localTask?.planId
					? Object.values(this.snapshot.evidence).find(
							(candidate) =>
								candidate.type === "stage-plan" &&
								this.snapshot.tasks[candidate.taskId]?.planId === localTask.planId,
						)
					: undefined;
				const frozenContracts = (
					planEvidence?.content as
						| { effectiveContracts?: Array<{ hash: string; contract: EffectiveTaskContract }> }
						| undefined
				)?.effectiveContracts;
				const frozen = frozenContracts?.find((entry) => entry.hash === localTask?.effectiveContractHash);
				const planReviews = planEvidence
					? Object.values(this.snapshot.reviews).filter(
							(review) =>
								review.evidenceId === planEvidence.id && review.targetVersionHash === planEvidence.versionHash,
						)
					: [];
				const issueBinding = sourceTask.repairChecks?.find(
					(check) => check.issueId === transfer.issueId && check.criterion === transfer.exactCriterion,
				);
				const targetReview = reviews.find(
					(review) =>
						review.verdict === "pass" &&
						review.targetVersionHash === evidence.versionHash &&
						review.criteria?.some(
							(criterion) => criterion.criterion === issueBinding?.criterion && criterion.passed,
						),
				);
				const issueOwner = Object.values(this.snapshot.obligations).find((obligation) =>
					obligation.items?.some(
						(item) =>
							item.id === transfer.issueId &&
							item.status === "open" &&
							item.criterion === transfer.exactCriterion,
					),
				);
				if (
					!legacyTask ||
					sourceTaskContractHash(legacyTask) !== transfer.sourceContractHash ||
					legacyTask.acceptanceChecks[transfer.sourceIndex] !== transfer.exactCriterion ||
					!localTask ||
					!localEvidence ||
					!planEvidence ||
					!frozen ||
					!taskContractMatches(localTask, frozen.contract) ||
					!frozen.contract.responsibilityTransfers.some(
						(entry) => entry.issueId === transfer.issueId && entry.exactCriterion === transfer.exactCriterion,
					) ||
					!planReviews.length ||
					planReviews.some((review) => review.verdict !== "pass" || (review.score ?? 0) < 0.8) ||
					!issueBinding ||
					!targetReview ||
					!issueOwner
				)
					continue;
				completion.repairItems.push({
					obligationId: issueOwner.id,
					itemId: transfer.issueId,
					reviewId: targetReview.id,
					evidenceId,
				});
				if (
					this.snapshot.obligations[issueOwner.id].items?.every(
						(item) =>
							item.status !== "open" || completion.repairItems.some((closed) => closed.itemId === item.id),
					)
				) {
					completion.resolvedObligations.push({
						obligationId: issueOwner.id,
						satisfiedBy: targetReview.id,
					});
					if (issueOwner.graphObjectionId) {
						completion.resolvedNodeIds.push(issueOwner.graphObjectionId);
					}
				}
			}
		}
		const artifactNode =
			this.snapshot.graph.nodes[`research_artifact_${artifact.id}`] ??
			createResearchNode({
				id: `research_artifact_${artifact.id}`,
				kind: "artifact",
				statement: `${artifact.type} canonical artifact`,
				status: "accepted",
				stageId: evidence.stageId,
				domainRef: artifact.id,
				sourceRefs: [evidence.id, ...(artifact.materializationRef ? [artifact.materializationRef] : [])],
			});
		if (!this.snapshot.graph.nodes[artifactNode.id]) completion.nodes.push(artifactNode);
		const artifactEdge = createResearchEdge({
			fromNodeId: `research_evidence_${evidence.id}`,
			toNodeId: artifactNode.id,
			kind: "derives",
			sourceRefs: [artifact.id],
		});
		if (!this.snapshot.graph.edges[artifactEdge.id]) completion.edges.push(artifactEdge);
		if (artifact.type === "result-to-claim") this.claimFacts(artifact, artifactNode.id, completion);
		if (
			artifact.replacementOf &&
			artifact.replacementOf !== artifact.id &&
			this.snapshot.canonical[artifact.replacementOf]
		)
			completion.cleanups.push(await this.retirementIntent(artifact.replacementOf, artifact.id));
		completion.cleanups.push(...(await this.supersededEvidenceIntents(evidence)));
		return completion;
	}

	private async supersededEvidenceIntents(winner: Evidence): Promise<CleanupIntent[]> {
		const evidenceSetId = winner.currentEvidenceSetId ?? winner.taskId;
		const superseded = Object.values(this.snapshot.evidence).filter(
			(evidence) =>
				evidence.id !== winner.id &&
				evidence.status === "rejected" &&
				(evidence.currentEvidenceSetId ?? evidence.taskId) === evidenceSetId &&
				!Object.values(this.snapshot.canonical).some((artifact) => artifact.evidenceId === evidence.id),
		);
		const intents: CleanupIntent[] = [];
		for (const evidence of superseded) {
			const reviews = Object.values(this.snapshot.reviews).filter((review) => review.evidenceId === evidence.id);
			const reviewIds = reviews.map((review) => review.id);
			const receipt: DiscardedEvidenceReceipt = {
				evidenceId: evidence.id,
				taskId: evidence.taskId,
				reviewIds,
				checksum: evidence.checksum,
				reason: "superseded-repair",
				cleanupStatus: "pending",
			};
			intents.push({
				id: `evidence:${evidence.id}`,
				kind: "evidence",
				receipt,
				status: "pending",
				tasks: await cleanupTaskFiles(this.snapshot, [
					evidence.taskId,
					...reviews.flatMap((review) => (review.reviewerTaskId ? [review.reviewerTaskId] : [])),
				]),
			});
		}
		return intents;
	}

	private claimFacts(artifact: CanonicalArtifact, artifactNodeId: string, completion: AdoptionCompletion): void {
		const content = artifact.content;
		if (content === null || typeof content !== "object" || Array.isArray(content)) return;
		const claims = (content as Record<string, unknown>).claims;
		const entries = Array.isArray(claims) ? claims : claims === undefined ? [] : [claims];
		for (const claim of entries) {
			const record = claim !== null && typeof claim === "object" && !Array.isArray(claim) ? claim : undefined;
			const assessment = claimAssessment(record?.assessment);
			const statement =
				typeof claim === "string"
					? claim
					: [record?.supportedStatement, record?.statement, record?.claim, record?.plannedClaim, record?.id].find(
							(value) => typeof value === "string" && value.trim(),
						);
			if (typeof statement !== "string" || !statement.trim()) continue;
			const accepted = assessment === "supported" || assessment === "partially-supported";
			const existing = Object.values(this.snapshot.graph.nodes).find(
				(node) =>
					node.kind === "claim" &&
					node.domainRef === artifact.id &&
					node.statement === statement &&
					node.claimAssessment === assessment,
			);
			const node =
				existing ??
				createResearchNode({
					id: `research_claim_${checksum({ artifactId: artifact.id, statement, assessment }).slice(0, 24)}`,
					kind: "claim",
					statement,
					status: accepted ? "accepted" : assessment === "unresolved" ? "active" : "rejected",
					stageId: "result-to-claim",
					domainRef: artifact.id,
					sourceRefs: [artifact.id],
				});
			node.claimAssessment = assessment;
			node.actor = "worker";
			if (!existing && !completion.nodes.some((saved) => saved.id === node.id)) completion.nodes.push(node);
			const edge = createResearchEdge({
				fromNodeId: artifactNodeId,
				toNodeId: node.id,
				kind: accepted ? "supports" : assessment === "refuted" ? "contradicts" : "derives",
				sourceRefs: [artifact.id],
			});
			if (!this.snapshot.graph.edges[edge.id] && !completion.edges.some((saved) => saved.id === edge.id))
				completion.edges.push(edge);
		}
	}

	private passingAcceptanceReviews(evidence: Evidence): Review[] {
		const policy = this.definitions[evidence.stageId]?.qualityPolicy;
		const reviews = Object.values(this.snapshot.reviews).filter((review) => review.evidenceId === evidence.id);
		if (reviews.some((review) => review.verdict !== "pass"))
			throw new Error("evidence with a non-passing review requires a repaired candidate before acceptance");
		const passing = reviews.filter(
			(review) =>
				review.verdict === "pass" &&
				(review.score ?? 0) >= (policy?.minScore ?? 0.8) &&
				review.targetVersionHash === evidence.versionHash,
		);
		if (passing.length < (policy?.minPassingReviews ?? 1))
			throw new Error("evidence requires the configured passing reviews before acceptance");
		return passing;
	}

	/** Acceptance and its historical tail use the same item-bound closure rules. */
	private acceptanceConsequences(evidence: Evidence, decisionRef: string): EvidenceAcceptanceConsequences {
		const passing = this.passingAcceptanceReviews(evidence);
		const task = this.snapshot.tasks[evidence.taskId];
		const consequences: EvidenceAcceptanceConsequences = {
			repairItems: [],
			resolvedObligations: [],
			resolvedNodeIds: [],
		};
		for (const obligation of Object.values(this.snapshot.obligations)) {
			const failed = this.snapshot.evidence[this.snapshot.reviews[obligation.sourceReviewId]?.evidenceId];
			if (
				!failed ||
				failed.id === evidence.id ||
				failed.stageId !== evidence.stageId ||
				(failed.currentEvidenceSetId ?? failed.taskId) !== (evidence.currentEvidenceSetId ?? evidence.taskId)
			)
				continue;
			for (const item of obligation.items ?? []) {
				if (item.status !== "open") continue;
				if (
					task.deliveryKind === "local" &&
					task.responsibilityTransfers?.some((transfer) => transfer.issueId === item.id)
				)
					continue;
				const binding = task.repairChecks?.find((check) => check.issueId === item.id);
				if (
					!binding ||
					this.normalizedRepairCriterion(binding.criterion) !== this.normalizedRepairCriterion(item.criterion) ||
					!passing.every((review) =>
						review.criteria?.some((criterion) => criterion.criterion === binding.criterion && criterion.passed),
					)
				)
					throw new Error(`repair item requires explicit verified closure: ${item.id}`);
				consequences.repairItems.push({
					obligationId: obligation.id,
					itemId: item.id,
					reviewId: passing[0].id,
					evidenceId: evidence.id,
				});
			}
			if (
				(obligation.items ?? []).every(
					(item) => item.status !== "open" || consequences.repairItems.some((closed) => closed.itemId === item.id),
				)
			) {
				if (obligation.status === "open")
					consequences.resolvedObligations.push({ obligationId: obligation.id, satisfiedBy: decisionRef });
				if (
					obligation.graphObjectionId &&
					this.snapshot.graph.nodes[obligation.graphObjectionId]?.status === "open"
				)
					consequences.resolvedNodeIds.push(obligation.graphObjectionId);
			}
		}
		return consequences;
	}

	async decideEvidence(
		evidenceId: string,
		accepted: boolean,
		decisionRef = `main_agent_worker_artifact_decision::${randomUUID()}`,
	): Promise<void> {
		return this.exclusive(async () => {
			this.assertMainAgentPermission(decisionRef);
			const evidence = this.snapshot.evidence[evidenceId];
			if (!evidence) throw new Error(`unknown evidence ${evidenceId}`);
			if (evidence.mainAgentDecisionRef === decisionRef && evidence.status === (accepted ? "accepted" : "rejected"))
				return;
			this.assertEvidenceCurrent(evidence.id);
			if (accepted)
				this.assertCurrentInputs(
					this.snapshot.tasks[evidence.taskId].inputArtifactRefs,
					this.snapshot.tasks[evidence.taskId].repairOfEvidenceId,
					new Set(),
					this.snapshot.tasks[evidence.taskId],
				);
			const consequences = accepted ? this.acceptanceConsequences(evidence, decisionRef) : undefined;
			await this.appendEvent({ type: "evidence_decided", evidenceId, accepted, decisionRef, consequences });
		});
	}

	private async recoverAcceptancesInternal(): Promise<StoredEvent[] | undefined> {
		const pendingByLineage = new Map<string, Obligation[]>();
		for (const obligation of Object.values(this.snapshot.obligations)) {
			if (
				obligation.status !== "open" &&
				!obligation.items?.some((item) => item.status === "open") &&
				this.snapshot.graph.nodes[obligation.graphObjectionId ?? ""]?.status !== "open"
			)
				continue;
			const failed = this.snapshot.evidence[this.snapshot.reviews[obligation.sourceReviewId]?.evidenceId];
			if (!failed) continue;
			const key = JSON.stringify([failed.stageId, failed.currentEvidenceSetId ?? failed.taskId]);
			const pending = pendingByLineage.get(key) ?? [];
			pending.push(obligation);
			pendingByLineage.set(key, pending);
		}
		if (!pendingByLineage.size) return undefined;
		const candidates = Object.values(this.snapshot.evidence).filter((evidence) => {
			if (
				evidence.status !== "accepted" ||
				evidence.acceptanceAuthority !== "main_agent" ||
				!evidence.mainAgentDecisionRef
			)
				return false;
			const task = this.snapshot.tasks[evidence.taskId];
			const pending = pendingByLineage.get(
				JSON.stringify([evidence.stageId, evidence.currentEvidenceSetId ?? evidence.taskId]),
			);
			return (
				pending?.some((obligation) => {
					if (this.snapshot.reviews[obligation.sourceReviewId]?.evidenceId === evidence.id) return false;
					const openItems = (obligation.items ?? []).filter((item) => item.status === "open");
					return (
						!openItems.length ||
						openItems.some(
							(item) =>
								task?.deliveryKind !== "local" ||
								!task.responsibilityTransfers?.some((transfer) => transfer.issueId === item.id),
						)
					);
				}) ?? false
			);
		});
		if (!candidates.length) return undefined;
		const history = await this.store.readEvents(this.snapshot.frame.jobId);
		for (const evidence of candidates) {
			let recordedIndex = -1;
			let acceptedIndex = -1;
			for (const [index, saved] of history.entries()) {
				if (saved.event.type === "evidence_recorded" && saved.event.evidence.id === evidence.id)
					recordedIndex = index;
				if (
					saved.event.type === "evidence_decided" &&
					saved.event.evidenceId === evidence.id &&
					saved.event.accepted &&
					saved.event.decisionRef === evidence.mainAgentDecisionRef
				)
					acceptedIndex = index;
			}
			const recorded = history[recordedIndex]?.event;
			if (
				recorded?.type !== "evidence_recorded" ||
				recordedIndex >= acceptedIndex ||
				checksum({
					...recorded.evidence,
					status: undefined,
					acceptanceAuthority: undefined,
					mainAgentDecisionRef: undefined,
					supersededByTaskId: undefined,
				}) !==
					checksum({
						...evidence,
						status: undefined,
						acceptanceAuthority: undefined,
						mainAgentDecisionRef: undefined,
						supersededByTaskId: undefined,
					})
			)
				throw new StaleResearchInputError(`historical acceptance identity or version is stale: ${evidence.id}`);
			const task = this.snapshot.tasks[evidence.taskId];
			if (
				!task ||
				task.status !== "succeeded" ||
				!evidenceHasCurrentPlanApprovalFromSnapshot(this.snapshot, evidence)
			)
				throw new StaleResearchInputError(`historical acceptance task or plan is stale: ${evidence.id}`);
			this.assertCompletionTask(task, evidence);
			const consequences = this.acceptanceConsequences(evidence, evidence.mainAgentDecisionRef!);
			if (
				consequences.repairItems.length ||
				consequences.resolvedObligations.length ||
				consequences.resolvedNodeIds.length
			)
				await this.appendEvent({ type: "evidence_acceptance_recovered", evidenceId: evidence.id, consequences });
		}
		return history;
	}

	async recordCandidateEvaluation(
		input: Omit<CandidateEvaluation, "id" | "createdAt"> & { id?: string },
	): Promise<CandidateEvaluation> {
		return this.exclusive(async () => {
			const batch = this.snapshot.searchBatches[input.batchId];
			const candidate = batch?.candidates[input.candidateId];
			if (!batch || !candidate) throw new Error("candidate evaluation references an unknown search candidate");
			if (candidate.evidenceId !== input.evidenceId) throw new Error("candidate evaluation evidence does not match");
			const review = this.snapshot.reviews[input.reviewId];
			if (
				!review ||
				review.evidenceId !== input.evidenceId ||
				review.targetVersionHash !== this.snapshot.evidence[input.evidenceId]?.versionHash
			)
				throw new Error("candidate evaluation review does not match");
			if (!Number.isFinite(input.score) || input.score < 0 || input.score > 1) {
				throw new Error("candidate evaluation score must be between zero and one");
			}
			const missingCriteria = batch.criteria.filter(
				(expected) => !input.criteria.some((criterion) => criterion.criterion === expected),
			);
			if (missingCriteria.length > 0) {
				throw new Error(`candidate evaluation omits frozen criteria: ${missingCriteria.join("; ")}`);
			}
			if (
				input.verdict !== review.verdict ||
				input.score !== (review.score ?? 0) ||
				checksum(input.criteria) !== checksum(review.criteria ?? []) ||
				checksum(input.findings) !== checksum(review.findings)
			)
				throw new Error("candidate evaluation differs from its review");
			const existing = Object.values(this.snapshot.candidateEvaluations).find(
				(evaluation) =>
					evaluation.batchId === input.batchId &&
					evaluation.candidateId === input.candidateId &&
					evaluation.evidenceId === input.evidenceId &&
					evaluation.reviewId === input.reviewId,
			);
			if (existing) return structuredClone(existing);
			const evaluation: CandidateEvaluation = {
				...input,
				id: input.id ?? `candidate_evaluation_${randomUUID()}`,
				createdAt: new Date().toISOString(),
			};
			await this.appendEvent({ type: "candidate_evaluation_recorded", evaluation });
			return evaluation;
		});
	}

	async recordCandidateEvaluationFromReview(reviewId: string): Promise<CandidateEvaluation> {
		const review = this.snapshot.reviews[reviewId];
		const evidence = review && this.snapshot.evidence[review.evidenceId];
		const task = evidence && this.snapshot.tasks[evidence.taskId];
		if (!review || !evidence || !task?.searchBatchId || !task.searchCandidateId)
			throw new Error("review does not belong to a search candidate");
		return this.recordCandidateEvaluation({
			batchId: task.searchBatchId,
			candidateId: task.searchCandidateId,
			evidenceId: evidence.id,
			reviewId,
			verdict: review.verdict,
			score: review.score ?? 0,
			criteria: review.criteria ?? [],
			findings: review.findings,
		});
	}

	/** One qualification rule for automatic review readiness and source selection. */
	searchQualification(batchId: string) {
		const batch = this.snapshot.searchBatches[batchId];
		if (
			!batch ||
			!["planning", "running", "evaluating"].includes(batch.status) ||
			batch.stageId !== this.snapshot.frame.activeStageId ||
			planReviewStatus(this, batch.planId) !== "passed"
		)
			throw new Error("search batch is stale, superseded or not current");
		const stage = this.snapshot.stages[batch.stageId];
		if (
			(batch.stageRevision ?? 1) !== (stage.revision ?? 1) ||
			(batch.stageExecutionId && batch.stageExecutionId !== stage.executionId)
		)
			throw new Error("search execution is stale");
		const policy = this.definitions[batch.stageId].qualityPolicy;
		const required = policy?.minPassingReviews ?? 1;
		const candidates = Object.values(batch.candidates).map((candidate) => {
			const evidence = candidate.evidenceId ? this.snapshot.evidence[candidate.evidenceId] : undefined;
			const task = candidate.taskId ? this.snapshot.tasks[candidate.taskId] : undefined;
			const validIdentity =
				evidence &&
				task &&
				task.planId === batch.planId &&
				task.stageExecutionId === (stage.executionId ?? `stage_exec_${batch.stageId}`) &&
				task.status === "succeeded" &&
				evidence.taskId === task.id &&
				task.searchBatchId === batch.id &&
				task.searchCandidateId === candidate.id &&
				evidenceHasCurrentPlanApprovalFromSnapshot(this.snapshot, evidence);
			const recordedEvaluations = validIdentity
				? Object.values(this.snapshot.candidateEvaluations).filter((evaluation) => {
						const review = this.snapshot.reviews[evaluation.reviewId];
						return (
							evaluation.batchId === batchId &&
							evaluation.candidateId === candidate.id &&
							evaluation.evidenceId === evidence.id &&
							review?.evidenceId === evidence.id &&
							review.targetVersionHash === evidence.versionHash &&
							review.verdict === evaluation.verdict &&
							(review.score ?? 0) === evaluation.score &&
							checksum(review.criteria ?? []) === checksum(evaluation.criteria) &&
							checksum(review.findings) === checksum(evaluation.findings) &&
							batch.criteria.every((criterion) =>
								evaluation.criteria.some((item) => item.criterion === criterion),
							)
						);
					})
				: [];
			const evaluations = [
				...new Map(recordedEvaluations.map((evaluation) => [evaluation.reviewId, evaluation])).values(),
			];
			const veto =
				evidence &&
				Object.values(this.snapshot.reviews).some(
					(review) => review.evidenceId === evidence.id && review.verdict !== "pass",
				);
			const passing = evaluations.filter(
				(evaluation) =>
					evaluation.verdict === "pass" &&
					evaluation.score >= (policy?.minScore ?? 0.8) &&
					batch.criteria.every((criterion) =>
						evaluation.criteria.some((item) => item.criterion === criterion && item.passed),
					),
			);
			return {
				candidateId: candidate.id,
				evaluations,
				ready: candidate.status === "failed" || evaluations.length >= required,
				eligible: !veto && passing.length >= required,
			};
		});
		return {
			ready: candidates.every((candidate) => candidate.ready),
			candidates,
			eligibleIds: candidates.filter((candidate) => candidate.eligible).map((candidate) => candidate.candidateId),
		};
	}

	finalSearchBatch(stageId: string): SearchBatch | undefined {
		return Object.values(this.snapshot.searchBatches)
			.filter(
				(batch) =>
					batch.stageId === stageId &&
					(batch.stageRevision ?? 1) === (this.snapshot.stages[stageId].revision ?? 1) &&
					batch.round >= batch.maxRounds &&
					batch.status === "exhausted",
			)
			.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0];
	}

	async exhaustNegativeSearch(batchId: string): Promise<void> {
		return this.exclusive(async () => {
			const qualification = this.searchQualification(batchId);
			const batch = this.snapshot.searchBatches[batchId];
			if (!qualification.ready || qualification.eligibleIds.length || batch.round < batch.maxRounds)
				throw new Error("search is not a fully evaluated final negative result");
			await this.appendEvent({
				type: "search_batch_exhausted",
				batchId,
				rationale: `No eligible candidate after complete current evaluations: ${qualification.candidates.flatMap((candidate) => candidate.evaluations.map((evaluation) => evaluation.id)).join(", ")}`,
			});
		});
	}

	async selectSearchCandidate(batchId: string, candidateId: string, decisionRef: string): Promise<void> {
		return this.exclusive(async () => {
			this.assertMainAgentPermission(decisionRef);
			const batch = this.snapshot.searchBatches[batchId];
			if (batch?.status === "selected") {
				if (batch.selectedCandidateId !== candidateId || batch.decisionRef !== decisionRef)
					throw new Error("search selection conflicts with original decision");
				await this.finishSearchSelectionInternal(batch);
				return;
			}
			const qualification = this.searchQualification(batchId);
			if (!qualification.ready || !qualification.eligibleIds.includes(candidateId))
				throw new Error("selected search candidate does not satisfy the current quality threshold");
			const selected = batch.candidates[candidateId];
			const evidence = this.snapshot.evidence[selected.evidenceId!];
			this.assertCurrentInputs(
				this.snapshot.tasks[evidence.taskId].inputArtifactRefs,
				this.snapshot.tasks[evidence.taskId].repairOfEvidenceId,
				new Set(),
				this.snapshot.tasks[evidence.taskId],
			);
			const consequences = this.acceptanceConsequences(evidence, decisionRef);
			await this.appendEvent({
				type: "search_batch_decided",
				batchId,
				candidateId,
				decisionRef,
				acceptance: { evidenceId: evidence.id, consequences },
			});
			await this.finishSearchSelectionInternal(this.snapshot.searchBatches[batchId]);
		});
	}

	private async finishSearchSelectionInternal(batch: SearchBatch): Promise<void> {
		const winner = batch.candidates[batch.selectedCandidateId!];
		const evidence = winner?.evidenceId ? this.snapshot.evidence[winner.evidenceId] : undefined;
		if (!winner?.evidenceId || !batch.decisionRef) throw new Error("original selected search evidence missing");
		const retirement = Object.values(this.snapshot.cleanupIntents ?? {}).find(
			(intent) =>
				intent.kind === "retirement" &&
				intent.receipt.evidenceId === winner.evidenceId &&
				intent.receipt.taskId === winner.taskId &&
				this.snapshot.retiredArtifacts[intent.receipt.artifactId]?.evidenceId === winner.evidenceId,
		);
		if (!evidence && !retirement) throw new Error("original selected search evidence missing");
		if (!batch.acceptanceCompleted) {
			const history = await this.store.readEvents(this.snapshot.frame.jobId);
			const recordedIndex = history.findIndex(
				(saved) => saved.event.type === "evidence_recorded" && saved.event.evidence.id === winner.evidenceId,
			);
			const recorded = history[recordedIndex]?.event;
			const selectedIndex = history.findIndex(
				(saved) =>
					saved.event.type === "search_batch_decided" &&
					saved.event.batchId === batch.id &&
					saved.event.candidateId === winner.id &&
					saved.event.decisionRef === batch.decisionRef,
			);
			if (
				recorded?.type !== "evidence_recorded" ||
				recordedIndex >= selectedIndex ||
				recorded.evidence.taskId !== winner.taskId ||
				recorded.evidence.stageId !== batch.stageId
			)
				throw new Error("original search acceptance identity cannot be proved");
			let acceptedIndex = -1;
			let complete = false;
			for (const [index, saved] of history.entries()) {
				if (index < selectedIndex) continue;
				const event = saved.event;
				if (
					event.type === "search_batch_decided" &&
					index === selectedIndex &&
					event.acceptance?.evidenceId === winner.evidenceId
				) {
					acceptedIndex = index;
					complete = Boolean(event.acceptance.consequences);
				}
				if (
					event.type === "evidence_decided" &&
					event.evidenceId === winner.evidenceId &&
					event.accepted &&
					event.decisionRef === batch.decisionRef
				) {
					acceptedIndex = index;
					complete = Boolean(event.consequences);
				}
			}
			if (acceptedIndex < 0) {
				if (!evidence || evidence.status !== "candidate")
					throw new Error("original search acceptance cannot be proved");
				await this.appendEvent({
					type: "evidence_decided",
					evidenceId: evidence.id,
					accepted: true,
					decisionRef: batch.decisionRef,
					consequences: this.acceptanceConsequences(evidence, batch.decisionRef),
				});
			} else {
				const superseded = history.some((saved, index) => {
					if (index <= acceptedIndex) return false;
					const event = saved.event;
					if (
						event.type === "evidence_decided" &&
						event.accepted &&
						event.evidenceId !== winner.evidenceId &&
						evidence?.supersededByTaskId
					) {
						const successor = history
							.slice(0, index)
							.find(
								(entry) =>
									entry.event.type === "evidence_recorded" && entry.event.evidence.id === event.evidenceId,
							)?.event;
						return (
							successor?.type === "evidence_recorded" &&
							successor.evidence.taskId === evidence.supersededByTaskId &&
							successor.evidence.stageId === recorded.evidence.stageId &&
							(successor.evidence.currentEvidenceSetId ?? successor.evidence.taskId) ===
								(recorded.evidence.currentEvidenceSetId ?? recorded.evidence.taskId)
						);
					}
					if (!retirement || retirement.kind !== "retirement") return false;
					const intents =
						event.type === "cleanup_requested"
							? [event.intent]
							: event.type === "canonical_artifact_status"
								? (event.completion?.cleanups ?? [])
								: event.type === "stage_reopened"
									? (event.cleanups ?? [])
									: [];
					return intents.some(
						(intent) =>
							intent.kind === "retirement" &&
							intent.receipt.artifactId === retirement.receipt.artifactId &&
							intent.receipt.evidenceId === winner.evidenceId &&
							intent.receipt.taskId === winner.taskId &&
							history
								.slice(acceptedIndex + 1, index)
								.some(
									(entry) =>
										entry.event.type === "evidence_adopted" &&
										entry.event.artifact.id === intent.receipt.artifactId &&
										entry.event.artifact.evidenceId === winner.evidenceId &&
										entry.event.artifact.checksum === intent.receipt.checksum,
								),
					);
				});
				if (
					(!evidence || evidence.status !== "accepted" || evidence.mainAgentDecisionRef !== batch.decisionRef) &&
					!superseded
				)
					throw new Error("original search acceptance has no legal successor");
				let consequences: EvidenceAcceptanceConsequences = {
					repairItems: [],
					resolvedObligations: [],
					resolvedNodeIds: [],
				};
				if (!complete && !superseded) {
					const initial = history[0]?.event;
					if (initial?.type !== "job_created") throw new Error("original search acceptance prefix unavailable");
					const replay = new ResearchJob(structuredClone(initial.snapshot), this.store, this.definitions);
					for (const saved of history.slice(0, acceptedIndex + 1)) replay.apply(saved.event, saved.timestamp);
					consequences = replay.acceptanceConsequences(
						replay.snapshot.evidence[winner.evidenceId],
						batch.decisionRef,
					);
				}
				await this.appendEvent({
					type: "evidence_acceptance_recovered",
					evidenceId: winner.evidenceId,
					consequences,
					searchSelection: { batchId: batch.id, candidateId: winner.id, decisionRef: batch.decisionRef },
				});
			}
		}
		for (const candidate of Object.values(batch.candidates)) {
			if (candidate.id === winner.id || this.snapshot.discardedCandidates[candidate.id]) continue;
			await this.pruneSearchCandidateInternal(batch.id, candidate.id);
		}
		await this.finishCleanupsInternal();
	}

	async continueSearchBatch(batchId: string, decisionRef: string, rationale: string): Promise<void> {
		return this.exclusive(async () => {
			this.assertMainAgentPermission(decisionRef);
			if (
				this.snapshot.searchBatches[batchId]?.status === "exhausted" &&
				this.snapshot.searchBatches[batchId].decisionRef === decisionRef &&
				this.snapshot.searchBatches[batchId].continuationRationale === rationale
			)
				return;
			const batch = this.snapshot.searchBatches[batchId];
			if (!batch) throw new Error("continued search references an unknown batch");
			if (!["planning", "running", "evaluating"].includes(batch.status)) {
				throw new Error(`search batch ${batchId} cannot continue from status ${batch.status}`);
			}
			if (batch.round >= batch.maxRounds) {
				throw new Error(`search batch ${batchId} is already at final round ${batch.round}/${batch.maxRounds}`);
			}
			if (!this.searchQualification(batchId).ready)
				throw new Error("search candidates have not been completely independently evaluated");
			if (!decisionRef.trim()) throw new Error("continued search requires a decision reference");
			if (!rationale.trim()) throw new Error("continued search requires a rationale");
			await this.appendEvent({ type: "search_batch_continued", batchId, decisionRef, rationale });
		});
	}

	private async pruneSearchCandidateInternal(batchId: string, candidateId: string): Promise<void> {
		if (this.snapshot.discardedCandidates[candidateId]) return;
		const batch = this.snapshot.searchBatches[batchId];
		const candidate = batch?.candidates[candidateId];
		if (!candidate) return;
		const evidence = candidate.evidenceId ? this.snapshot.evidence[candidate.evidenceId] : undefined;
		const reviews = evidence
			? Object.values(this.snapshot.reviews).filter((review) => review.evidenceId === evidence.id)
			: [];
		const reviewIds = new Set(reviews.map((review) => review.id));
		const obligations = Object.values(this.snapshot.obligations).filter((obligation) =>
			reviewIds.has(obligation.sourceReviewId),
		);
		if (
			Object.values(this.snapshot.obligations).some(
				(obligation) =>
					candidate.evidenceId &&
					obligation.evidenceId === candidate.evidenceId &&
					!reviewIds.has(obligation.sourceReviewId),
			) ||
			obligations.some(
				(obligation) => obligation.evidenceId !== undefined && obligation.evidenceId !== candidate.evidenceId,
			)
		)
			throw new Error("cannot safely archive ambiguous candidate obligations");
		const receipt: DiscardedCandidateReceipt = {
			candidateId,
			batchId,
			...(candidate.taskId ? { taskId: candidate.taskId } : {}),
			...(candidate.evidenceId ? { evidenceId: candidate.evidenceId } : {}),
			...(evidence ? { evidenceChecksum: evidence.checksum } : {}),
			archivedReviews: structuredClone(reviews),
			archivedObligations: structuredClone(obligations),
			decisionRef: batch.decisionRef,
			reason: "unselected-search-candidate",
			cleanupStatus: "pending",
		};
		const intent: CleanupIntent = {
			id: `search-candidate:${candidateId}`,
			kind: "search-candidate",
			receipt,
			status: "pending",
			tasks: await cleanupTaskFiles(this.snapshot, [
				...(candidate.taskId ? [candidate.taskId] : []),
				...reviews.flatMap((review) => (review.reviewerTaskId ? [review.reviewerTaskId] : [])),
			]),
		};
		await this.appendEvent({ type: "cleanup_requested", intent });
		await this.finishCleanupsInternal();
	}

	completionBlockers(): string[] {
		const blockers: string[] = [];
		for (const stageId of Object.keys(this.snapshot.stages)) {
			if (this.unsynthesizedLocalEvidence(stageId).length)
				blockers.push(`stage ${stageId} has local evidence awaiting synthesis`);
		}
		for (const [stageId, stage] of Object.entries(this.snapshot.stages)) {
			if (stage.invalidatedBy) blockers.push(`stage ${stageId} requires revalidation after ${stage.invalidatedBy}`);
		}
		if (this.snapshot.frame.openObligationIds.some((id) => this.snapshot.obligations[id]?.status === "open")) {
			blockers.push("blocking review obligations remain");
		}
		if (this.snapshot.graph.unresolvedObjectionIds.length > 0) blockers.push("unresolved research objections remain");
		if (this.snapshot.frame.scientificOutcome === "pending" || this.snapshot.frame.missionCoverage === "pending") {
			blockers.push("scientific outcome has not been recorded");
		}
		if (
			["supported", "partially-supported"].includes(this.snapshot.frame.scientificOutcome) &&
			this.snapshot.graph.acceptedClaimIds.length === 0
		) {
			blockers.push("supported scientific outcome has no accepted research claim");
		}
		const activeArtifacts = Object.values(this.snapshot.canonical).filter((artifact) => artifact.status === "active");
		if (activeArtifacts.length === 0) blockers.push("canonical research route is empty");
		const activeArtifactTypes = new Set(activeArtifacts.map((artifact) => artifact.type));
		for (const requiredType of this.snapshot.frame.requiredArtifactTypes) {
			if (!activeArtifactTypes.has(requiredType)) {
				blockers.push(`required canonical artifact ${requiredType} is missing`);
			}
		}
		for (const artifact of activeArtifacts) {
			const evidence = this.snapshot.evidence[artifact.evidenceId];
			const policy = evidence ? this.definitions[evidence.stageId]?.qualityPolicy : undefined;
			const passingReviews = Object.values(this.snapshot.reviews).filter(
				(review) =>
					review.evidenceId === artifact.evidenceId &&
					review.verdict === "pass" &&
					(review.score ?? 0) >= (policy?.minScore ?? 0.8),
			);
			if (passingReviews.length < (policy?.minPassingReviews ?? 1)) {
				blockers.push(`canonical artifact ${artifact.id} lacks passing independent review`);
			}
		}
		const finalReview = activeArtifacts.find((artifact) => artifact.type === "research-review");
		const reviewContent = finalReview?.content;
		const reviewRecord =
			reviewContent !== null && typeof reviewContent === "object" && !Array.isArray(reviewContent)
				? (reviewContent as Record<string, unknown>)
				: undefined;
		const verdict = typeof reviewRecord?.verdict === "string" ? reviewRecord.verdict.toLowerCase() : "";
		const requiredRepairs = Array.isArray(reviewRecord?.requiredRepairs) ? reviewRecord.requiredRepairs : [];
		const reviewedOutcome = normalizedLabel(reviewRecord?.scientificOutcome);
		const reviewedCoverage = normalizedLabel(reviewRecord?.missionCoverage);
		if (
			!finalReview ||
			!["pass", "pass_with_nonblocking_caveats", "accept", "accepted"].includes(verdict) ||
			requiredRepairs.length > 0
		) {
			blockers.push("no passing whole-research review");
		}
		if (
			finalReview &&
			(reviewedOutcome !== this.snapshot.frame.scientificOutcome ||
				reviewedCoverage !== this.snapshot.frame.missionCoverage)
		) {
			blockers.push("whole-research review does not confirm the scientific outcome and mission coverage");
		}
		return [...new Set(blockers)];
	}

	async applyRouteDecision(manifest: MainAgentDecisionManifest): Promise<void> {
		return this.exclusive(async () => {
			this.assertMainAgentPermission(manifest.decisionRef);
			if (manifest.jobId !== this.snapshot.frame.jobId) throw new Error("route decision job id does not match");
			if (manifest.decisionType !== "route" || !manifest.routeAction) {
				throw new Error("route decision manifest is incomplete");
			}
			const existing = this.snapshot.routeDecisions[manifest.decisionRef];
			if (existing) {
				const proposed = {
					negativeSearchBatchId: existing.negativeSearchBatchId,
					id: manifest.decisionRef,
					stageId: manifest.stageId,
					action: manifest.routeAction,
					targetStageId: manifest.targetStageId,
					evidenceRefs: manifest.evidenceRefs ?? existing.evidenceRefs,
					question: manifest.question,
					newQuestions: manifest.newQuestions ?? [],
					rationale: manifest.rationale,
					sessionRef: manifest.sessionRef,
					createdAt: manifest.createdAt,
				};
				if (
					checksum(JSON.parse(JSON.stringify(proposed))) !==
					checksum(
						JSON.parse(
							JSON.stringify({
								...existing,
								consequencesCompleted: undefined,
								consequencesSupersededBy: undefined,
								continuedPlanId: undefined,
							}),
						),
					)
				)
					throw new Error("route decision id already has different content");
				await this.recoverRoutesInternal();
				await this.finishCleanupsInternal();
				return;
			}
			const call = this.snapshot.mainAgentCalls?.[manifest.decisionRef];
			if (call?.obligationId) {
				const obligation = this.snapshot.obligations[call.obligationId];
				const evidence = this.snapshot.evidence[this.snapshot.reviews[obligation?.sourceReviewId]?.evidenceId];
				if (
					call.type !== "route" ||
					!obligation ||
					obligation.status !== "open" ||
					evidence?.stageId !== call.stageId ||
					(obligation.stageId && obligation.stageId !== call.stageId)
				)
					throw new Error("repair route obligation is no longer current");
				if (!["continue", "backtrack", "ask-user"].includes(manifest.routeAction))
					throw new Error("repair route requires continue, backtrack, or ask-user");
			}
			const unknownEvidenceRefs = (manifest.evidenceRefs ?? []).filter(
				(ref) => !this.snapshot.canonical[ref] && !this.snapshot.evidence[ref] && !this.snapshot.graph.nodes[ref],
			);
			if (unknownEvidenceRefs.length > 0) {
				throw new Error(`route decision references unknown evidence: ${unknownEvidenceRefs.join(", ")}`);
			}
			const stageId = this.snapshot.frame.activeStageId;
			if (manifest.routeAction === "search" && this.finalSearchBatch(stageId))
				throw new Error(
					"search maximum rounds reached; backtrack to a new revision or continue with ordinary planning",
				);
			if (manifest.stageId !== stageId) throw new Error(`route decision must target active stage ${stageId}`);
			if (["advance", "complete"].includes(manifest.routeAction) && this.unsynthesizedLocalEvidence(stageId).length)
				throw new Error("route requires synthesis of accepted local evidence");
			const currentArtifactId = this.snapshot.canonicalRoute.stageArtifactIds[stageId];
			if (["advance", "complete"].includes(manifest.routeAction) && this.backtrackChecks(stageId).length)
				throw new Error("route cannot advance while bound responsibility nodes remain open");
			if (
				["advance", "complete"].includes(manifest.routeAction) &&
				this.snapshot.frame.openObligationIds.some((id) => this.snapshot.obligations[id]?.stageId === stageId)
			)
				throw new Error("route cannot advance while repair obligations remain open");
			if (["advance", "complete"].includes(manifest.routeAction) && !currentArtifactId) {
				throw new Error(`route decision ${manifest.routeAction} requires a canonical artifact for ${stageId}`);
			}
			if (manifest.routeAction === "advance") {
				if (
					!manifest.targetStageId ||
					manifest.targetStageId === stageId ||
					!this.definitions[manifest.targetStageId]
				) {
					throw new Error("advance route decision requires a different valid target stage");
				}
			}
			if (
				manifest.routeAction === "backtrack" &&
				(!manifest.targetStageId || !this.definitions[manifest.targetStageId])
			) {
				throw new Error("backtrack route decision requires a valid target stage");
			}
			if (manifest.routeAction === "ask-user" && !manifest.question?.trim()) {
				throw new Error("ask-user route decision requires a concrete question");
			}
			if (manifest.routeAction === "complete") {
				const blockers = this.completionBlockers();
				if (blockers.length > 0) throw new Error(`research completion blocked: ${blockers.join("; ")}`);
			}
			const decision: StageRouteDecision = {
				...(this.finalSearchBatch(stageId) ? { negativeSearchBatchId: this.finalSearchBatch(stageId)!.id } : {}),
				id: manifest.decisionRef,
				stageId,
				action: manifest.routeAction,
				...(manifest.targetStageId ? { targetStageId: manifest.targetStageId } : {}),
				evidenceRefs: manifest.evidenceRefs ?? (currentArtifactId ? [currentArtifactId] : []),
				...(manifest.question ? { question: manifest.question } : {}),
				newQuestions: manifest.newQuestions ?? [],
				rationale: manifest.rationale,
				sessionRef: manifest.sessionRef,
				createdAt: manifest.createdAt,
			};
			const consequences = await this.routeConsequences(decision);
			await this.appendEvent({ type: "route_decided", decision, consequences });
			await this.finishCleanupsInternal();
		});
	}

	private async routeConsequences(decision: StageRouteDecision): Promise<RouteConsequences> {
		const consequences: RouteConsequences = { nodes: [], edges: [] };
		const matched = new Set<string>();
		for (const [index, question] of decision.newQuestions.entries()) {
			const existing = Object.values(this.snapshot.graph.nodes).find(
				(node) =>
					node.kind === "question" &&
					node.stageId === decision.stageId &&
					node.domainRef === decision.id &&
					node.statement === question &&
					!matched.has(node.id),
			);
			const node =
				existing ??
				createResearchNode({
					id: `research_question_${checksum({ decisionRef: decision.id, stageId: decision.stageId, index, question }).slice(0, 24)}`,
					kind: "question",
					statement: question,
					status: "open",
					stageId: decision.stageId,
					domainRef: decision.id,
					sourceRefs: [decision.id],
				});
			matched.add(node.id);
			if (!existing) consequences.nodes.push(node);
			const edge = createResearchEdge({
				fromNodeId: this.snapshot.graph.rootQuestionId,
				toNodeId: node.id,
				kind: "refines",
				sourceRefs: [decision.id],
			});
			if (!this.snapshot.graph.edges[edge.id]) consequences.edges.push(edge);
		}
		if (
			decision.action === "backtrack" &&
			decision.targetStageId &&
			!Object.values(this.snapshot.graph.nodes).some(
				(node) =>
					node.kind === "objection" && node.stageId === decision.targetStageId && node.domainRef === decision.id,
			)
		)
			consequences.reopening = await this.prepareStageReopening(
				decision.targetStageId,
				decision.id,
				decision.rationale,
			);
		if (
			decision.action === "ask-user" &&
			decision.question &&
			!(
				this.snapshot.frame.userGate?.kind === "research" &&
				this.snapshot.frame.userGate.question === decision.question &&
				this.snapshot.frame.userGate.stageId === decision.stageId
			)
		)
			consequences.gate = {
				kind: "research",
				stageId: decision.stageId,
				question: decision.question,
				reason: decision.rationale,
				requiredAt: decision.createdAt,
			};
		return consequences;
	}

	private async recoverRoutesInternal(recoveryHistory?: StoredEvent[]): Promise<void> {
		const pending = Object.values(this.snapshot.routeDecisions).filter(
			(decision) => !decision.consequencesCompleted && !decision.consequencesSupersededBy,
		);
		if (!pending.length) return;
		const history = recoveryHistory ?? (await this.store.readEvents(this.snapshot.frame.jobId));
		let lastIndex = -1;
		for (const [index, saved] of history.entries()) if (saved.event.type === "route_decided") lastIndex = index;
		const event = history[lastIndex]?.event;
		if (event?.type !== "route_decided") return;
		const pendingIds = new Set(pending.map((decision) => decision.id));
		for (const saved of history.slice(0, lastIndex)) {
			if (
				saved.event.type !== "route_decided" ||
				saved.event.decision.id === event.decision.id ||
				!pendingIds.delete(saved.event.decision.id)
			)
				continue;
			await this.appendEvent({
				type: "route_consequences_superseded",
				decisionRef: saved.event.decision.id,
				supersededBy: `route:${event.decision.id}`,
			});
		}
		const decision = this.snapshot.routeDecisions[event.decision.id];
		if (!decision || decision.consequencesCompleted || decision.consequencesSupersededBy) return;
		const later = history.slice(lastIndex + 1);
		// Journal ordering excludes structural changes, not harmless leases, pauses or budget accounting.
		const replacement = later.find(
			({ event: change }) =>
				[
					"user_guidance_recorded",
					"active_stage_work_superseded",
					"stage_plan_recorded",
					"task_dispatched",
					"evidence_decided",
					"evidence_adopted",
					"artifact_retired",
					"search_batch_recorded",
					"search_batch_decided",
				].includes(change.type) ||
				(change.type === "canonical_artifact_status" && change.status === "active") ||
				(change.type === "stage_reopened" && change.decisionRef !== decision.id),
		);
		if (replacement) {
			await this.appendEvent({
				type: "route_consequences_superseded",
				decisionRef: decision.id,
				supersededBy: `event:${replacement.seq}:${replacement.event.type}`,
			});
			return;
		}
		const stage = this.snapshot.stages[decision.stageId];
		const reopened = Object.values(this.snapshot.graph.nodes).some(
			(node) =>
				node.kind === "objection" && node.domainRef === decision.id && node.stageId === decision.targetStageId,
		);
		if (
			stage.lastRouteDecisionRef !== decision.id ||
			(["backtrack", "ask-user", "continue", "search"].includes(decision.action) &&
				this.snapshot.frame.activeStageId !== (reopened ? decision.targetStageId : decision.stageId)) ||
			(!reopened && stage.lastRoutedArtifactId !== this.snapshot.canonicalRoute.stageArtifactIds[decision.stageId])
		)
			throw new StaleResearchInputError(`pending route consequences cannot safely recover: ${decision.id}`);
		if (
			this.snapshot.paused &&
			((decision.action === "backtrack" && !reopened) ||
				(decision.action === "ask-user" &&
					!(
						this.snapshot.frame.userGate?.kind === "research" &&
						this.snapshot.frame.userGate.stageId === decision.stageId &&
						this.snapshot.frame.userGate.question === decision.question
					)))
		)
			return;
		const consequences = await this.routeConsequences(decision);
		await this.appendEvent({ type: "route_consequences_recovered", decisionRef: decision.id, consequences });
	}

	async pause(reason: string): Promise<void> {
		await this.commit({ type: "job_paused", reason });
	}

	async resume(): Promise<void> {
		await this.recoverLegacyProviderCapacityFailure();
		return this.exclusive(async () => {
			const gate = this.snapshot.frame.userGate;
			if (!gate) {
				await this.appendEvent({ type: "job_resumed" });
				await this.recoverRoutesInternal();
				return;
			}
			if (gate.kind === "budget") {
				const usage = this.snapshot.budgetUsage ?? { turnsUsed: 0, costUsdUsed: 0 };
				const stillBlocked =
					(gate.limit === "maxTasks" &&
						Object.keys(this.snapshot.tasks).length >= this.snapshot.frame.budget.maxTasks) ||
					(gate.limit === "maxTurns" && usage.turnsUsed >= this.snapshot.frame.budget.maxTurns) ||
					(gate.limit === "maxCostUsd" &&
						this.snapshot.frame.budget.maxCostUsd !== undefined &&
						usage.costUsdUsed >= this.snapshot.frame.budget.maxCostUsd);
				if (stillBlocked) throw new Error(`budget gate remains: increase ${gate.limit} before resume`);
			}
			if (gate.kind === "research") {
				throw new Error("research question gate requires user guidance before resume");
			}
			await this.appendEvent({ type: "user_gate_approved", gate, approvedAt: new Date().toISOString() });
			await this.recoverRoutesInternal();
		});
	}

	async resumeWithGuidance(guidance: string): Promise<void> {
		if (this.snapshot.frame.userGate && this.snapshot.frame.userGate.kind !== "research") {
			throw new Error("guided resume cannot bypass a stage or budget gate");
		}
		await this.recoverLegacyProviderCapacityFailure();
		await this.recordUserGuidance(guidance, true);
	}

	private async recoverLegacyProviderCapacityFailure(): Promise<void> {
		if (
			this.snapshot.providerBackoff ||
			classifyProviderErrorMessage(this.snapshot.frame.nextAction) !== "capacity"
		) {
			return;
		}
		const latestByReplayKey = new Map<string, TaskPacket>();
		for (const task of Object.values(this.snapshot.tasks)) {
			const latest = latestByReplayKey.get(task.replayKey);
			if (!latest || task.attempt > latest.attempt) latestByReplayKey.set(task.replayKey, task);
		}
		const recoveredTasks = [...latestByReplayKey.values()].filter((task) => {
			if (task.stageId !== this.snapshot.frame.activeStageId || task.role !== "worker") return false;
			if (task.status === "running") return true;
			if (task.status !== "failed") return false;
			return Object.values(this.snapshot.sessions).some(
				(session) =>
					session.taskId === task.id &&
					typeof session.error === "string" &&
					classifyProviderErrorMessage(session.error) === "capacity",
			);
		});
		for (const task of recoveredTasks) {
			await this.setTaskStatus(task.id, "ready");
			const session = Object.values(this.snapshot.sessions)
				.filter((candidate) => candidate.taskId === task.id)
				.sort((left, right) => right.updatedAt.localeCompare(left.updatedAt))[0];
			if (session && session.status !== "completed") {
				await this.recordChildSession({
					...session,
					status: "interrupted",
					error: session.error ?? "interrupted by legacy provider capacity pause",
					updatedAt: new Date().toISOString(),
				});
			}
		}
		const refundableTurns = Math.min(recoveredTasks.length, this.snapshot.budgetUsage?.turnsUsed ?? 0);
		if (refundableTurns > 0) await this.refundTurns(refundableTurns);
	}

	private enqueue<T>(operation: () => Promise<T>): Promise<T> {
		const run = this.commitChain.then(operation);
		this.commitChain = run.then(
			() => undefined,
			() => undefined,
		);
		return run;
	}

	private exclusive<T>(operation: () => Promise<T>): Promise<T> {
		return this.enqueue(() =>
			this.store.withWriteLock(this.snapshot.frame.jobId, async () => {
				const seq = await this.store.readEventSeq(this.snapshot.frame.jobId);
				if (seq !== this.snapshot.eventSeq)
					throw new StaleResearchJobError("research job is stale; reopen before writing");
				return operation();
			}),
		);
	}

	private commit(event: AstraEvent): Promise<void> {
		return this.exclusive(() => this.appendEvent(event));
	}

	/** Only called inside exclusive: no queue reentry and no second lock. */
	private async appendEvent(event: AstraEvent): Promise<void> {
		let stored: StoredEvent;
		try {
			stored = await this.store.append(this.snapshot.frame.jobId, event);
		} catch (error) {
			// An append may have committed before reporting failure. Reconcile under the held journal lock.
			const history = await this.store.readEvents(this.snapshot.frame.jobId);
			checkReviewHistory(history);
			for (const saved of history.slice(this.snapshot.eventSeq)) {
				this.apply(saved.event, saved.timestamp);
				this.snapshot.eventSeq = saved.seq;
				this.snapshot.updatedAt = saved.timestamp;
			}
			throw error;
		}
		this.apply(stored.event, stored.timestamp);
		this.snapshot.eventSeq = stored.seq;
		this.snapshot.updatedAt = stored.timestamp;
		await this.persist();
	}

	private async persist(): Promise<void> {
		await this.store.writeSnapshot(this.snapshot);
	}

	private applyCleanupIntent(intent: CleanupIntent, timestamp: string): void {
		this.snapshot.cleanupIntents ??= {};
		if (this.snapshot.cleanupIntents[intent.id]) return;
		this.snapshot.cleanupIntents[intent.id] = structuredClone(intent);
		// Original events now mean logical invalidation when cleanupStatus is pending.
		if (intent.kind === "retirement") this.apply({ type: "artifact_retired", receipt: intent.receipt }, timestamp);
		else if (intent.kind === "evidence") this.apply({ type: "evidence_pruned", receipt: intent.receipt }, timestamp);
		else this.apply({ type: "search_candidate_pruned", receipt: intent.receipt }, timestamp);
	}

	private applyEvidenceCompletion(evidenceId: string, completion: EvidenceCompletion): void {
		if (completion.taskSucceeded) this.snapshot.tasks[this.snapshot.evidence[evidenceId].taskId].status = "succeeded";
		if (completion.node && !this.snapshot.graph.nodes[completion.node.id])
			addNodeToGraph(this.snapshot.graph, completion.node);
		if (completion.edge && !this.snapshot.graph.edges[completion.edge.id])
			addEdgeToGraph(this.snapshot.graph, completion.edge);
	}

	private applyAcceptanceConsequences(consequences: EvidenceAcceptanceConsequences, timestamp: string): void {
		for (const closed of consequences.repairItems) this.apply({ type: "repair_item_resolved", ...closed }, timestamp);
		for (const closed of consequences.resolvedObligations)
			this.apply({ type: "obligation_resolved", ...closed }, timestamp);
		for (const nodeId of consequences.resolvedNodeIds)
			if (this.snapshot.graph.nodes[nodeId]?.status !== "resolved")
				updateNodeStatus(this.snapshot.graph, nodeId, "resolved", timestamp);
	}

	private applyRouteConsequences(decisionRef: string, consequences: RouteConsequences, timestamp: string): void {
		for (const node of consequences.nodes)
			if (!this.snapshot.graph.nodes[node.id]) addNodeToGraph(this.snapshot.graph, node);
		for (const edge of consequences.edges)
			if (!this.snapshot.graph.edges[edge.id]) addEdgeToGraph(this.snapshot.graph, edge);
		if (consequences.reopening) this.apply({ type: "stage_reopened", ...consequences.reopening }, timestamp);
		if (consequences.gate) {
			if (this.snapshot.paused) return;
			this.apply({ type: "user_gate_required", gate: consequences.gate }, timestamp);
		}
		this.snapshot.routeDecisions[decisionRef].consequencesCompleted = true;
	}

	private apply(event: AstraEvent, timestamp: string): void {
		const decisionRef =
			event.type === "stage_plan_recorded"
				? event.plan.decisionRef
				: event.type === "evidence_adopted"
					? event.artifact.mainAgentDecisionRef
					: event.type === "route_decided"
						? event.decision.id
						: event.type === "evidence_decided" ||
								event.type === "search_batch_decided" ||
								event.type === "search_batch_continued" ||
								event.type === "job_paused"
							? event.decisionRef
							: undefined;
		if (decisionRef && this.snapshot.mainAgentCalls?.[decisionRef])
			this.snapshot.mainAgentCalls[decisionRef].applied = true;
		switch (event.type) {
			case "main_agent_call_recorded":
				this.snapshot.mainAgentCalls ??= {};
				this.snapshot.mainAgentCalls[event.call.id] = structuredClone(event.call);
				return;
			case "main_agent_delivery_recorded":
				this.snapshot.mainAgentCalls![event.callId].deliveryHash = event.deliveryHash;
				return;
			case "main_agent_call_finished":
				this.snapshot.mainAgentCalls![event.callId].completed = !event.abandoned;
				this.snapshot.mainAgentCalls![event.callId].abandoned = event.abandoned;
				return;
			case "job_created":
				this.snapshot = structuredClone(event.snapshot);
				return;
			case "lease_acquired":
				this.snapshot.lease = event.lease;
				return;
			case "lease_released":
				if (this.snapshot.lease?.owner === event.owner) this.snapshot.lease = undefined;
				return;
			case "stage_plan_recorded":
				this.snapshot.stagePlans[event.plan.id] = structuredClone(event.plan);
				if (event.continueDecisionRef)
					this.snapshot.routeDecisions[event.continueDecisionRef].continuedPlanId = event.plan.id;
				if (event.search) this.apply({ type: "search_batch_recorded", ...event.search }, timestamp);
				this.snapshot.frame.nextAction = `review stage plan ${event.plan.id}`;
				return;
			case "task_dispatched":
				this.snapshot.tasks[event.task.id] = structuredClone(event.task);
				if (event.task.searchBatchId && event.task.searchCandidateId) {
					const candidate =
						this.snapshot.searchBatches[event.task.searchBatchId]?.candidates[event.task.searchCandidateId];
					if (candidate) {
						candidate.taskId = event.task.id;
						candidate.status = "running";
						this.snapshot.searchBatches[event.task.searchBatchId].status = "running";
					}
				}
				return;
			case "task_status":
				this.snapshot.tasks[event.taskId].status = event.status;
				return;
			case "task_version_recorded":
				this.snapshot.tasks[event.taskId].version = structuredClone(event.version);
				return;
			case "child_session_recorded":
				this.snapshot.sessions[event.session.sessionId] = structuredClone(event.session);
				if (event.session.role === "main-agent") this.snapshot.mainAgentSessionId = event.session.sessionId;
				return;
			case "evidence_recorded":
				this.snapshot.evidence[event.evidence.id] = structuredClone(event.evidence);
				if (event.completion) this.applyEvidenceCompletion(event.evidence.id, event.completion);
				{
					const task = this.snapshot.tasks[event.evidence.taskId];
					if (task?.searchBatchId && task.searchCandidateId) {
						const candidate = this.snapshot.searchBatches[task.searchBatchId]?.candidates[task.searchCandidateId];
						if (candidate) {
							candidate.evidenceId = event.evidence.id;
							candidate.status = "evaluating";
							this.snapshot.searchBatches[task.searchBatchId].status = "evaluating";
						}
					}
				}
				return;
			case "evidence_completion_recovered":
				this.applyEvidenceCompletion(event.evidenceId, event.completion);
				return;
			case "evidence_acceptance_recovered":
				this.applyAcceptanceConsequences(event.consequences, timestamp);
				if (event.searchSelection) {
					const selection = event.searchSelection;
					const batch = this.snapshot.searchBatches[selection.batchId];
					if (
						batch?.status !== "selected" ||
						batch.selectedCandidateId !== selection.candidateId ||
						batch.decisionRef !== selection.decisionRef ||
						batch.candidates[selection.candidateId]?.evidenceId !== event.evidenceId
					)
						throw new Error("search acceptance completion identity mismatch");
					batch.acceptanceCompleted = true;
				}
				return;
			case "evidence_decided":
				if (event.accepted) {
					const evidence = this.snapshot.evidence[event.evidenceId];
					const evidenceSetId = evidence.currentEvidenceSetId ?? evidence.taskId;
					for (const candidate of Object.values(this.snapshot.evidence)) {
						if (
							candidate.id !== evidence.id &&
							(candidate.currentEvidenceSetId ?? candidate.taskId) === evidenceSetId
						) {
							candidate.status = "rejected";
							candidate.supersededByTaskId = evidence.taskId;
						}
					}
				}
				this.snapshot.evidence[event.evidenceId].status = event.accepted ? "accepted" : "rejected";
				this.snapshot.evidence[event.evidenceId].acceptanceAuthority = "main_agent";
				this.snapshot.evidence[event.evidenceId].mainAgentDecisionRef = event.decisionRef;
				if (event.consequences) this.applyAcceptanceConsequences(event.consequences, timestamp);
				if (event.accepted && event.consequences)
					for (const batch of Object.values(this.snapshot.searchBatches)) {
						if (
							batch.status === "selected" &&
							batch.decisionRef === event.decisionRef &&
							batch.candidates[batch.selectedCandidateId ?? ""]?.evidenceId === event.evidenceId
						)
							batch.acceptanceCompleted = true;
					}
				return;
			case "review_delivery_rejected": {
				this.snapshot.reviewDeliveryRejections ??= {};
				const prior = this.snapshot.reviewDeliveryRejections[event.rejection.taskId];
				if (prior && checksum(prior) !== checksum(event.rejection))
					throw new Error("review rejection history integrity conflict");
				this.snapshot.reviewDeliveryRejections[event.rejection.taskId] = structuredClone(event.rejection);
				return;
			}
			case "review_recorded":
				if (
					this.snapshot.reviews[event.review.id] &&
					reviewBody(this.snapshot.reviews[event.review.id]) !== reviewBody(event.review)
				)
					throw new Error("review history integrity conflict");
				this.snapshot.reviews[event.review.id] = structuredClone(event.review);
				if (event.review.verdict !== "pass" || (event.review.score ?? 0) < 0.8) {
					const evidence = this.snapshot.evidence[event.review.evidenceId];
					if (evidence?.type === "stage-plan")
						for (const batch of Object.values(this.snapshot.searchBatches)) {
							if (batch.planId === this.snapshot.tasks[evidence.taskId]?.planId) batch.status = "superseded";
						}
				}
				if (event.consequences?.objection) addNodeToGraph(this.snapshot.graph, event.consequences.objection);
				if (event.consequences?.edge) addEdgeToGraph(this.snapshot.graph, event.consequences.edge);
				if (event.consequences?.obligation) {
					const obligation = event.consequences.obligation;
					this.snapshot.obligations[obligation.id] = structuredClone(obligation);
					if (obligation.status === "open" && !this.snapshot.frame.openObligationIds.includes(obligation.id))
						this.snapshot.frame.openObligationIds.push(obligation.id);
				}
				return;
			case "repair_item_resolved": {
				const item = this.snapshot.obligations[event.obligationId].items?.find((item) => item.id === event.itemId);
				if (item)
					Object.assign(item, { status: "resolved", reviewId: event.reviewId, evidenceId: event.evidenceId });
				return;
			}
			case "obligation_created":
				this.snapshot.obligations[event.obligation.id] = structuredClone(event.obligation);
				this.snapshot.frame.openObligationIds.push(event.obligation.id);
				return;
			case "obligation_resolved":
				this.snapshot.obligations[event.obligationId].status = "resolved";
				this.snapshot.frame.openObligationIds = this.snapshot.frame.openObligationIds.filter(
					(id) => id !== event.obligationId,
				);
				return;
			case "evidence_adopted":
				this.snapshot.canonical[event.artifact.id] = structuredClone(event.artifact);
				this.snapshot.evidence[event.artifact.evidenceId].status = "accepted";
				return;
			case "canonical_artifact_materialized": {
				const artifact = this.snapshot.canonical[event.artifactId];
				artifact.materializationRef = event.materializationRef;
				artifact.targetSha256 = event.targetSha256;
				return;
			}
			case "canonical_artifact_status": {
				if (event.completion)
					for (const intent of event.completion.cleanups.filter((intent) => intent.kind === "retirement"))
						this.applyCleanupIntent(intent, timestamp);
				this.snapshot.canonical[event.artifactId].status = event.status;
				if (event.status === "active") {
					const artifact = this.snapshot.canonical[event.artifactId];
					const stageId = this.snapshot.evidence[artifact.evidenceId]?.stageId;
					if (stageId) {
						this.snapshot.stages[stageId].invalidatedBy = undefined;
						if (this.snapshot.canonicalRoute.stageArtifactIds[stageId] !== artifact.id) {
							this.snapshot.canonicalRoute.stageArtifactIds[stageId] = artifact.id;
							this.snapshot.canonicalRoute.revision += 1;
							this.snapshot.canonicalRoute.updatedAt = timestamp;
						}
						if (stageId === this.snapshot.frame.activeStageId && !this.snapshot.paused)
							this.snapshot.frame.nextAction = `decide route from ${stageId}`;
					}
				}
				if (event.completion) {
					const completion = event.completion;
					for (const node of completion.nodes) addNodeToGraph(this.snapshot.graph, node);
					for (const edge of completion.edges) addEdgeToGraph(this.snapshot.graph, edge);
					for (const closed of completion.repairItems)
						this.apply({ type: "repair_item_resolved", ...closed }, timestamp);
					for (const closed of completion.resolvedObligations)
						this.apply({ type: "obligation_resolved", ...closed }, timestamp);
					for (const nodeId of new Set(completion.resolvedNodeIds))
						if (this.snapshot.graph.nodes[nodeId]?.status !== "resolved")
							updateNodeStatus(this.snapshot.graph, nodeId, "resolved", timestamp);
					if (completion.assessment)
						this.apply(
							{
								type: "scientific_outcome_recorded",
								...completion.assessment,
								sourceArtifactId: event.artifactId,
							},
							timestamp,
						);
					for (const intent of completion.cleanups.filter((intent) => intent.kind !== "retirement"))
						this.applyCleanupIntent(intent, timestamp);
					this.snapshot.canonical[event.artifactId].adoptionCompletedAt = completion.completedAt;
				}
				return;
			}
			case "cleanup_requested":
				this.applyCleanupIntent(event.intent, timestamp);
				return;
			case "cleanup_completed": {
				const intent = this.snapshot.cleanupIntents![event.intentId];
				intent.status = "completed";
				intent.completedAt = event.completedAt;
				intent.archiveRefs = event.archiveRefs;
				const receipt =
					intent.kind === "retirement"
						? this.snapshot.retiredArtifacts[intent.receipt.artifactId]
						: intent.kind === "evidence"
							? this.snapshot.discardedEvidence[intent.receipt.evidenceId]
							: this.snapshot.discardedCandidates[intent.receipt.candidateId];
				receipt.cleanupStatus = "completed";
				receipt.archiveRefs = event.archiveRefs;
				if ("workspacePrunedAt" in receipt || intent.kind !== "retirement")
					(receipt as DiscardedEvidenceReceipt | DiscardedCandidateReceipt).workspacePrunedAt = event.completedAt;
				for (const task of intent.tasks) {
					for (const session of Object.values(this.snapshot.sessions)) {
						if (session.taskId !== task.taskId) continue;
						const saved = task.sessions.find((entry) => entry.sessionId === session.sessionId);
						if (saved?.present) session.sessionFile = saved.target;
					}
				}
				return;
			}
			case "artifact_retired": {
				const retiredArtifactId = event.receipt.artifactId;
				const sourceStageId = this.snapshot.evidence[event.receipt.evidenceId]?.stageId;
				const invalidRefs = dependentEvidenceRefs(this.snapshot, [retiredArtifactId, event.receipt.evidenceId]);
				const affectedStageIds = this.dependentStageIds(invalidRefs);
				if (sourceStageId && affectedStageIds.includes(sourceStageId)) {
					const sourceStage = this.snapshot.stages[sourceStageId];
					sourceStage.status = sourceStageId === this.snapshot.frame.activeStageId ? "running" : "pending";
					sourceStage.completedAt = undefined;
					sourceStage.routeApproval = undefined;
					sourceStage.invalidatedBy = retiredArtifactId;
				}
				for (const artifact of Object.values(this.snapshot.canonical)) {
					if (artifact.id === retiredArtifactId || artifact.status !== "active" || !invalidRefs.has(artifact.id))
						continue;
					artifact.status = "stale";
					artifact.invalidatedBy = retiredArtifactId;
				}
				for (const stageId of affectedStageIds) {
					if (stageId === sourceStageId) continue;
					const stage = this.snapshot.stages[stageId];
					stage.status = stageId === this.snapshot.frame.activeStageId ? "running" : "pending";
					stage.completedAt = undefined;
					stage.routeApproval = undefined;
					stage.invalidatedBy = retiredArtifactId;
					stage.revision = (stage.revision ?? 1) + 1;
					stage.executionId = undefined;
					delete this.snapshot.canonicalRoute.stageArtifactIds[stageId];
					delete this.snapshot.canonicalRoute.selectedCandidateIds[stageId];
					for (const batch of Object.values(this.snapshot.searchBatches))
						if (batch.stageId === stageId) batch.status = "superseded";
				}
				for (const node of Object.values(this.snapshot.graph.nodes)) {
					if (
						["claim", "artifact", "evidence"].includes(node.kind) &&
						((node.domainRef && invalidRefs.has(node.domainRef)) ||
							node.sourceRefs.some((ref) => invalidRefs.has(ref)))
					)
						updateNodeStatus(this.snapshot.graph, node.id, "superseded", timestamp);
				}
				if ([...invalidRefs].some((ref) => this.snapshot.canonical[ref]?.type === "result-to-claim")) {
					this.snapshot.frame.scientificOutcome = "pending";
					this.snapshot.frame.missionCoverage = "pending";
					this.snapshot.frame.scientificOutcomeReason = undefined;
				}
				if (this.snapshot.frame.status === "completed") {
					this.snapshot.frame.status = "running";
					this.snapshot.frame.completedAt = undefined;
					this.snapshot.frame.finalDecisionRef = undefined;
					this.snapshot.frame.nextAction = "revalidate research after artifact version change";
				}
				const prunedTaskIds = new Set([
					event.receipt.taskId,
					...event.receipt.reviewIds.flatMap((reviewId) => {
						const reviewerTaskId = this.snapshot.reviews[reviewId]?.reviewerTaskId;
						return reviewerTaskId ? [reviewerTaskId] : [];
					}),
				]);
				this.snapshot.retiredArtifacts[retiredArtifactId] = structuredClone(event.receipt);
				for (const [stageId, artifactId] of Object.entries(this.snapshot.canonicalRoute.stageArtifactIds)) {
					if (artifactId === retiredArtifactId) delete this.snapshot.canonicalRoute.stageArtifactIds[stageId];
				}
				delete this.snapshot.canonical[retiredArtifactId];
				delete this.snapshot.evidence[event.receipt.evidenceId];
				for (const reviewId of event.receipt.reviewIds) delete this.snapshot.reviews[reviewId];
				for (const session of Object.values(this.snapshot.sessions)) {
					if (!prunedTaskIds.has(session.taskId)) continue;
					session.sessionFile = undefined;
					session.manifestRef = undefined;
				}
				this.snapshot.canonicalRoute.revision += 1;
				this.snapshot.canonicalRoute.updatedAt = timestamp;
				if (event.receipt.type === "result-to-claim") {
					this.snapshot.frame.scientificOutcome = "pending";
					this.snapshot.frame.missionCoverage = "pending";
					this.snapshot.frame.scientificOutcomeReason = undefined;
				}
				return;
			}
			case "budget_usage_recorded": {
				const usage = this.snapshot.budgetUsage ?? { turnsUsed: 0, costUsdUsed: 0 };
				usage.turnsUsed += event.turns;
				usage.costUsdUsed += event.costUsd;
				this.snapshot.budgetUsage = usage;
				return;
			}
			case "budget_turns_refunded": {
				const usage = this.snapshot.budgetUsage ?? { turnsUsed: 0, costUsdUsed: 0 };
				usage.turnsUsed = Math.max(0, usage.turnsUsed - event.turns);
				this.snapshot.budgetUsage = usage;
				return;
			}
			case "budget_updated":
				this.snapshot.frame.budget = structuredClone(event.budget);
				return;
			case "provider_backoff_started":
				this.snapshot.providerBackoff = structuredClone(event.backoff);
				this.snapshot.frame.nextAction = `wait for provider until ${event.backoff.retryAt}`;
				return;
			case "provider_backoff_cleared":
				this.snapshot.providerBackoff = undefined;
				this.snapshot.frame.nextAction = `resume ${this.snapshot.frame.activeStageId} from saved progress`;
				return;
			case "automation_updated":
				this.snapshot.frame.automation = event.automation;
				return;
			case "user_gate_required":
				this.snapshot.frame.userGate = structuredClone(event.gate);
				this.snapshot.paused = true;
				this.snapshot.frame.status = "waiting-for-user";
				this.snapshot.frame.nextAction = `waiting for user: ${event.gate.reason}`;
				return;
			case "user_gate_approved": {
				if (event.gate.kind === "stage") {
					const stage = this.snapshot.stages[event.gate.stageId];
					stage.routeApproval = event.approvedAt;
				}
				this.snapshot.frame.userGate = undefined;
				this.snapshot.paused = false;
				this.snapshot.frame.status = "running";
				this.snapshot.frame.nextAction =
					event.gate.kind === "stage"
						? `decide route from ${event.gate.stageId}`
						: event.gate.kind === "research"
							? `main-agent reconsider ${event.gate.stageId} with user guidance`
							: `resume ${this.snapshot.frame.activeStageId} from saved progress`;
				return;
			}
			case "job_paused":
				this.snapshot.paused = true;
				this.snapshot.frame.nextAction = `paused: ${event.reason}`;
				return;
			case "job_resumed":
				this.snapshot.paused = false;
				this.snapshot.frame.status = "running";
				if (!this.snapshot.frame.nextAction.startsWith("replan ")) {
					this.snapshot.frame.nextAction = `resume ${this.snapshot.frame.activeStageId} from saved progress`;
				}
				return;
			case "user_guidance_recorded":
				if (event.supersession) {
					const stage = this.snapshot.stages[event.supersession.stageId];
					if (
						event.supersession.continueDecisionRef &&
						stage.lastRouteDecisionRef === event.supersession.continueDecisionRef
					) {
						stage.lastRouteDecisionRef = undefined;
						stage.lastRouteAction = undefined;
						stage.lastRoutedArtifactId = undefined;
					}
					for (const taskId of event.supersession.taskIds)
						if (this.snapshot.tasks[taskId]?.status !== "succeeded")
							this.snapshot.tasks[taskId].status = "blocked";
					for (const batchId of event.supersession.batchIds) {
						this.snapshot.searchBatches[batchId].status = "superseded";
						this.snapshot.searchBatches[batchId].updatedAt = timestamp;
					}
					if (event.supersession.resume) this.apply({ type: "job_resumed" }, timestamp);
				}
				addNodeToGraph(this.snapshot.graph, event.node);
				if (this.snapshot.frame.userGate?.kind === "research") {
					this.snapshot.frame.userGate = undefined;
					this.snapshot.paused = false;
					this.snapshot.frame.status = "running";
				}
				this.snapshot.frame.nextAction = event.supersession?.taskIds.length
					? `replan ${this.snapshot.frame.activeStageId}: user guidance supersedes previous work`
					: `main-agent reconsider ${this.snapshot.frame.activeStageId} with user guidance`;
				return;
			case "active_stage_work_superseded":
				for (const taskId of event.taskIds) {
					const task = this.snapshot.tasks[taskId];
					if (task && task.status !== "succeeded") task.status = "blocked";
				}
				for (const batch of Object.values(this.snapshot.searchBatches)) {
					if (batch.stageId === event.stageId && ["planning", "running", "evaluating"].includes(batch.status)) {
						batch.status = "superseded";
						batch.updatedAt = timestamp;
					}
				}
				this.snapshot.frame.nextAction = `replan ${event.stageId}: ${event.reason}`;
				return;
			case "scientific_outcome_recorded":
				this.snapshot.frame.scientificOutcome = event.outcome;
				this.snapshot.frame.missionCoverage = event.missionCoverage;
				this.snapshot.frame.scientificOutcomeReason = event.reason;
				return;
			case "research_node_recorded":
				addNodeToGraph(this.snapshot.graph, event.node);
				if (event.edge) addEdgeToGraph(this.snapshot.graph, event.edge);
				return;
			case "research_node_status":
				updateNodeStatus(this.snapshot.graph, event.nodeId, event.status, timestamp);
				return;
			case "search_batch_recorded":
				this.snapshot.searchBatches[event.batch.id] = structuredClone(event.batch);
				for (const node of event.nodes) addNodeToGraph(this.snapshot.graph, node);
				for (const edge of event.edges) addEdgeToGraph(this.snapshot.graph, edge);
				this.snapshot.frame.nextAction = `execute search ${event.batch.id}`;
				return;
			case "search_candidate_updated":
				this.snapshot.searchBatches[event.batchId].candidates[event.candidate.id] = structuredClone(
					event.candidate,
				);
				if (event.candidate.status === "failed") {
					updateNodeStatus(this.snapshot.graph, event.candidate.graphNodeId, "rejected", timestamp);
				}
				return;
			case "search_batch_exhausted": {
				const batch = this.snapshot.searchBatches[event.batchId];
				batch.status = "exhausted";
				batch.exhaustionRationale = event.rationale;
				batch.updatedAt = timestamp;
				this.snapshot.frame.nextAction = `search ${event.batchId} exhausted; decide next route`;
				return;
			}
			case "candidate_evaluation_recorded":
				this.snapshot.candidateEvaluations[event.evaluation.id] = structuredClone(event.evaluation);
				return;
			case "search_batch_continued": {
				const batch = this.snapshot.searchBatches[event.batchId];
				batch.status = "exhausted";
				batch.decisionRef = event.decisionRef;
				batch.continuationRationale = event.rationale;
				batch.updatedAt = timestamp;
				for (const candidate of Object.values(batch.candidates)) {
					updateNodeStatus(this.snapshot.graph, candidate.graphNodeId, "superseded", timestamp);
				}
				this.snapshot.frame.nextAction = `continue search ${event.batchId} with round ${batch.round + 1}/${batch.maxRounds}`;
				return;
			}
			case "search_batch_decided": {
				const batch = this.snapshot.searchBatches[event.batchId];
				batch.acceptanceCompleted = undefined;
				batch.status = "selected";
				batch.selectedCandidateId = event.candidateId;
				batch.decisionRef = event.decisionRef;
				batch.updatedAt = timestamp;
				for (const candidate of Object.values(batch.candidates)) {
					candidate.status = candidate.id === event.candidateId ? "selected" : "rejected";
					updateNodeStatus(
						this.snapshot.graph,
						candidate.graphNodeId,
						candidate.id === event.candidateId ? "accepted" : "rejected",
						timestamp,
					);
				}
				this.snapshot.canonicalRoute.selectedCandidateIds[batch.stageId] = event.candidateId;
				this.snapshot.canonicalRoute.revision += 1;
				this.snapshot.canonicalRoute.updatedAt = timestamp;
				if (event.acceptance)
					this.apply(
						{
							type: "evidence_decided",
							evidenceId: event.acceptance.evidenceId,
							accepted: true,
							decisionRef: event.decisionRef,
							consequences: event.acceptance.consequences,
						},
						timestamp,
					);
				return;
			}
			case "search_candidate_pruned": {
				for (const obligation of event.receipt.archivedObligations ?? []) {
					delete this.snapshot.obligations[obligation.id];
					this.snapshot.frame.openObligationIds = this.snapshot.frame.openObligationIds.filter(
						(id) => id !== obligation.id,
					);
				}
				for (const obligation of event.receipt.archivedObligations ?? []) {
					if (
						obligation.graphObjectionId &&
						!Object.values(this.snapshot.obligations).some(
							(other) => other.status === "open" && other.graphObjectionId === obligation.graphObjectionId,
						)
					)
						updateNodeStatus(this.snapshot.graph, obligation.graphObjectionId, "superseded", timestamp);
				}
				this.snapshot.discardedCandidates[event.receipt.candidateId] = structuredClone(event.receipt);
				const prunedReviewerTaskIds = event.receipt.evidenceId
					? Object.values(this.snapshot.reviews)
							.filter((review) => review.evidenceId === event.receipt.evidenceId)
							.flatMap((review) => (review.reviewerTaskId ? [review.reviewerTaskId] : []))
					: [];
				if (event.receipt.evidenceId) {
					delete this.snapshot.evidence[event.receipt.evidenceId];
					for (const [reviewId, review] of Object.entries(this.snapshot.reviews)) {
						if (review.evidenceId === event.receipt.evidenceId) delete this.snapshot.reviews[reviewId];
					}
				}
				if (event.receipt.taskId) {
					for (const session of Object.values(this.snapshot.sessions)) {
						if (session.taskId !== event.receipt.taskId && !prunedReviewerTaskIds.includes(session.taskId))
							continue;
						session.sessionFile = undefined;
						session.manifestRef = undefined;
					}
				}
				return;
			}
			case "evidence_pruned": {
				this.snapshot.discardedEvidence[event.receipt.evidenceId] = structuredClone(event.receipt);
				const prunedTaskIds = new Set([
					event.receipt.taskId,
					...event.receipt.reviewIds.flatMap((reviewId) => {
						const reviewerTaskId = this.snapshot.reviews[reviewId]?.reviewerTaskId;
						return reviewerTaskId ? [reviewerTaskId] : [];
					}),
				]);
				delete this.snapshot.evidence[event.receipt.evidenceId];
				for (const reviewId of event.receipt.reviewIds) delete this.snapshot.reviews[reviewId];
				for (const session of Object.values(this.snapshot.sessions)) {
					if (!prunedTaskIds.has(session.taskId)) continue;
					session.sessionFile = undefined;
					session.manifestRef = undefined;
				}
				return;
			}
			case "route_consequences_recovered":
				this.applyRouteConsequences(event.decisionRef, event.consequences, timestamp);
				return;
			case "route_consequences_superseded":
				this.snapshot.routeDecisions[event.decisionRef].consequencesSupersededBy = event.supersededBy;
				return;
			case "route_decided": {
				const pausedFrame = this.snapshot.paused ? structuredClone(this.snapshot.frame) : undefined;
				const decision = structuredClone(event.decision);
				this.snapshot.routeDecisions[decision.id] = decision;
				const stage = this.snapshot.stages[decision.stageId];
				stage.lastRouteDecisionRef = decision.id;
				stage.lastRouteAction = decision.action;
				stage.lastRoutedArtifactId = this.snapshot.canonicalRoute.stageArtifactIds[decision.stageId];
				stage.routeApproval = undefined;
				addNodeToGraph(this.snapshot.graph, {
					id: `research_decision_${checksum({ jobId: this.snapshot.frame.jobId, id: decision.id }).slice(0, 24)}`,
					kind: "decision",
					statement: `${decision.action}: ${decision.rationale}`,
					status: "accepted",
					stageId: decision.stageId,
					domainRef: decision.id,
					sourceRefs: decision.evidenceRefs,
					createdAt: decision.createdAt,
					updatedAt: decision.createdAt,
				});
				if (decision.action === "advance" && decision.targetStageId) {
					stage.status = "completed";
					stage.completedAt = decision.createdAt;
					const target = this.snapshot.stages[decision.targetStageId];
					target.status = "running";
					target.completedAt = undefined;
					target.revision = (target.revision ?? 1) + 1;
					this.snapshot.frame.activeStageId = decision.targetStageId;
					this.snapshot.frame.nextAction = `main-agent frame ${decision.targetStageId}`;
				} else if (decision.action === "continue") {
					this.snapshot.frame.nextAction = `continue ${decision.stageId} with another evidence loop`;
				} else if (decision.action === "search") {
					this.snapshot.frame.nextAction = `search alternatives in ${decision.stageId}`;
				} else if (decision.action === "backtrack") {
					this.snapshot.frame.nextAction = `backtrack to ${decision.targetStageId}`;
				} else if (decision.action === "ask-user") {
					this.snapshot.frame.nextAction = `ask user: ${decision.question}`;
				} else if (decision.action === "complete") {
					stage.status = "completed";
					stage.completedAt = decision.createdAt;
					this.snapshot.frame.status = "completed";
					this.snapshot.frame.finalDecisionRef = decision.id;
					this.snapshot.frame.completedAt = decision.createdAt;
					this.snapshot.frame.nextAction = "research complete";
					updateNodeStatus(this.snapshot.graph, this.snapshot.graph.rootQuestionId, "resolved", timestamp);
				}
				if (event.consequences) this.applyRouteConsequences(decision.id, event.consequences, timestamp);
				if (pausedFrame) {
					this.snapshot.frame.nextAction = pausedFrame.nextAction;
					this.snapshot.frame.userGate = pausedFrame.userGate;
					this.snapshot.frame.status = pausedFrame.status;
				}
				return;
			}
			case "stage_reopened":
				for (const intent of event.cleanups ?? []) this.applyCleanupIntent(intent, timestamp);
				for (const stageId of event.affectedStageIds) {
					const stage = this.snapshot.stages[stageId];
					stage.status = stageId === event.targetStageId ? "running" : "pending";
					stage.completedAt = undefined;
					stage.executionId = undefined;
					stage.routeApproval = undefined;
					stage.revision = (stage.revision ?? 1) + 1;
					stage.lastRouteDecisionRef = event.decisionRef;
					delete this.snapshot.canonicalRoute.stageArtifactIds[stageId];
					delete this.snapshot.canonicalRoute.selectedCandidateIds[stageId];
				}
				for (const batch of Object.values(this.snapshot.searchBatches)) {
					if (event.affectedStageIds.includes(batch.stageId)) batch.status = "superseded";
				}
				addNodeToGraph(this.snapshot.graph, event.objection);
				this.snapshot.canonicalRoute.revision += 1;
				this.snapshot.canonicalRoute.updatedAt = timestamp;
				this.snapshot.frame.activeStageId = event.targetStageId;
				if (!this.snapshot.paused) {
					this.snapshot.frame.nextAction = `revisit ${event.targetStageId}: ${event.reason}`;
					this.snapshot.frame.userGate = undefined;
				}
				return;
		}
	}
}
