import { createHash, randomUUID } from "node:crypto";
import { access, cp, mkdir, readFile, rename, rm } from "node:fs/promises";
import { join } from "node:path";
import { assertAstraId, atomicWriteJson, canonicalArtifactPath, canonicalReceiptPath, sha256 } from "./contracts.ts";
import { planReviewStatus } from "./plan-review.ts";
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
import { validateReviewAssessment } from "./review-validation.ts";
import { DEFAULT_STAGES, stageMap } from "./stages.ts";
import type { AstraStore } from "./store.ts";
import { freezeEvidenceFiles } from "./task-workspace.ts";
import type {
	AstraEvent,
	AutomationLevel,
	BudgetLimit,
	CandidateEvaluation,
	CanonicalArtifact,
	ClaimAssessment,
	DiscardedCandidateReceipt,
	DiscardedEvidenceReceipt,
	Evidence,
	JobSnapshot,
	Lease,
	MainAgentDecisionManifest,
	MissionCoverage,
	MissionFrame,
	Obligation,
	ProviderBackoffState,
	ResearchEdge,
	ResearchNode,
	RetiredArtifactReceipt,
	Review,
	ScientificOutcome,
	SearchBatch,
	SearchCandidate,
	StageDefinition,
	StagePlanManifest,
	StageRouteDecision,
	StageState,
	TaskPacket,
	TaskStatus,
	TaskVersion,
	UserGateRequest,
} from "./types.ts";

const DEFAULT_SEARCH_MAX_ROUNDS = 2;
export const MAX_TASK_ATTEMPTS = 3;
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
			(artifact) => artifact.status === "active" && artifact.type === "result-to-claim",
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

	async reload(): Promise<void> {
		await this.commitChain;
		const latest = await ResearchJob.open(this.store, this.snapshot.frame.jobId);
		if (!latest) throw new Error(`research job not found: ${this.snapshot.frame.jobId}`);
		this.snapshot = latest.state;
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
		if (!Number.isInteger(turns) || turns <= 0) throw new Error("turn usage must be a positive integer");
		const block = this.budgetBlock({ turns });
		if (block) throw new Error(block.reason);
		await this.commit({ type: "budget_usage_recorded", turns, costUsd: 0 });
	}

	async recordCost(costUsd: number): Promise<void> {
		if (!Number.isFinite(costUsd) || costUsd < 0) throw new Error("cost usage must be a non-negative number");
		if (costUsd === 0) return;
		await this.commit({ type: "budget_usage_recorded", turns: 0, costUsd });
	}

	async refundTurns(turns: number): Promise<void> {
		if (!Number.isInteger(turns) || turns <= 0) throw new Error("turn refund must be a positive integer");
		const used = this.snapshot.budgetUsage?.turnsUsed ?? 0;
		if (turns > used) throw new Error("turn refund cannot exceed turns used");
		await this.commit({ type: "budget_turns_refunded", turns });
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
		await this.commit({ type: "budget_updated", budget });
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
					: input.kind === "research" && current.question === input.question)
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
		await this.commit({ type: "lease_acquired", lease });
		return lease;
	}

	async releaseLease(owner: string): Promise<void> {
		if (this.snapshot.lease?.owner !== owner) return;
		await this.commit({ type: "lease_released", owner });
	}

	async recordStagePlan(plan: StagePlanManifest): Promise<StagePlanManifest> {
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
		}
		if (plan.obligationId) {
			const obligation = this.snapshot.obligations[plan.obligationId];
			if (!obligation || obligation.status !== "open")
				throw new Error("repair stage plan requires an open obligation");
			const failed = this.snapshot.evidence[this.snapshot.reviews[obligation.sourceReviewId]?.evidenceId];
			const failedTask = failed ? this.snapshot.tasks[failed.taskId] : undefined;
			if (failedTask && (plan.tasks[0].deliveryKind ?? "stage") !== (failedTask.deliveryKind ?? "stage"))
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
				Object.values(previousBatch.candidates).map((candidate) => this.normalizeHypothesis(candidate.hypothesis)),
			);
			const repeated = plan.tasks.find((task) =>
				previousHypotheses.has(this.normalizeHypothesis(task.hypothesis ?? task.objective)),
			);
			if (repeated)
				throw new Error(`continued search repeats a hypothesis from ${previousBatch.id}: ${repeated.key}`);
		}
		const existing = this.snapshot.stagePlans[plan.id];
		if (existing) {
			if (checksum(existing) !== checksum(plan))
				throw new Error(`stage plan id ${plan.id} already has different content`);
			if (
				plan.mode === "search" &&
				!Object.values(this.snapshot.searchBatches).some((batch) => batch.planId === plan.id)
			) {
				await this.recordSearchBatch(plan, definition);
			}
			return structuredClone(existing);
		}
		await this.commit({ type: "stage_plan_recorded", plan });
		if (plan.mode === "search") await this.recordSearchBatch(plan, definition);
		return structuredClone(plan);
	}

	private async recordSearchBatch(plan: StagePlanManifest, definition: StageDefinition): Promise<void> {
		const now = new Date().toISOString();
		const policy = definition.searchPolicy ?? {
			strategy: "diverse-candidates" as const,
			minCandidates: 2,
			maxCandidates: 4,
			criteria: definition.acceptanceChecks,
		};
		const batchId = `search_${checksum({ jobId: plan.jobId, planId: plan.id }).slice(0, 24)}`;
		if (this.snapshot.searchBatches[batchId]) return;
		const previousBatch = this.pendingSearchContinuation(plan.stageId);
		const maxRounds = previousBatch?.maxRounds ?? policy.maxRounds ?? DEFAULT_SEARCH_MAX_ROUNDS;
		const round = previousBatch ? previousBatch.round + 1 : 1;
		if (!Number.isInteger(maxRounds) || maxRounds <= 0)
			throw new Error("search maxRounds must be a positive integer");
		if (round > maxRounds) throw new Error(`search round ${round} exceeds the maximum of ${maxRounds}`);
		const nodes: ResearchNode[] = [];
		const edges: ResearchEdge[] = [];
		const candidates: Record<string, SearchCandidate> = {};
		for (const task of plan.tasks) {
			const candidateId = `candidate_${checksum({ batchId, key: task.key }).slice(0, 24)}`;
			const node = createResearchNode({
				id: `hypothesis_${checksum({ batchId, key: task.key }).slice(0, 24)}`,
				kind: "hypothesis",
				statement: task.hypothesis ?? task.objective,
				stageId: plan.stageId,
				domainRef: candidateId,
				sourceRefs: [`stage-plan:${plan.id}`, plan.sessionRef],
			});
			nodes.push(node);
			edges.push(
				createResearchEdge({
					fromNodeId: this.snapshot.graph.rootQuestionId,
					toNodeId: node.id,
					kind: "refines",
					sourceRefs: [`stage-plan:${plan.id}`],
				}),
			);
			candidates[candidateId] = {
				id: candidateId,
				key: task.key,
				hypothesis: node.statement,
				status: "planned",
				graphNodeId: node.id,
			};
		}
		const batch: SearchBatch = {
			id: batchId,
			stageId: plan.stageId,
			planId: plan.id,
			round,
			maxRounds,
			...(previousBatch ? { previousBatchId: previousBatch.id } : {}),
			objective: `Search alternatives for ${definition.label}`,
			strategy: policy.strategy,
			status: "planning",
			minCandidates: policy.minCandidates,
			maxCandidates: policy.maxCandidates,
			criteria: policy.criteria,
			candidates,
			createdAt: now,
			updatedAt: now,
		};
		await this.commit({ type: "search_batch_recorded", batch, nodes, edges });
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

	async recordUserGuidance(guidance: string): Promise<ResearchNode> {
		const statement = guidance.trim();
		if (!statement) throw new Error("research guidance cannot be empty");
		const node = createResearchNode({
			kind: "decision",
			statement: `User guidance: ${statement}`,
			status: "accepted",
			stageId: this.snapshot.frame.activeStageId,
			sourceRefs: [`user-guidance:${this.snapshot.eventSeq + 1}`],
		});
		node.actor = "user";
		await this.commit({ type: "user_guidance_recorded", node });
		return node;
	}

	async reopenStage(targetStageId: string, decisionRef: string, reason: string): Promise<void> {
		if (!this.definitions[targetStageId]) throw new Error(`unknown stage ${targetStageId}`);
		const invalidArtifactIds = new Set(
			Object.values(this.snapshot.canonical)
				.filter(
					(artifact) =>
						artifact.status === "active" &&
						this.snapshot.evidence[artifact.evidenceId]?.stageId === targetStageId,
				)
				.map((artifact) => artifact.id),
		);
		let changed = true;
		while (changed) {
			changed = false;
			for (const artifact of Object.values(this.snapshot.canonical)) {
				if (artifact.status !== "active" || invalidArtifactIds.has(artifact.id)) continue;
				const evidence = this.snapshot.evidence[artifact.evidenceId];
				const task = evidence ? this.snapshot.tasks[evidence.taskId] : undefined;
				if (task?.inputArtifactRefs.some((ref) => invalidArtifactIds.has(ref))) {
					invalidArtifactIds.add(artifact.id);
					changed = true;
				}
			}
		}
		const affectedStageIds = [
			...new Set([
				targetStageId,
				...[...invalidArtifactIds].flatMap((artifactId) => {
					const artifact = this.snapshot.canonical[artifactId];
					const stageId = artifact ? this.snapshot.evidence[artifact.evidenceId]?.stageId : undefined;
					return stageId ? [stageId] : [];
				}),
			]),
		];
		for (const artifactId of invalidArtifactIds) await this.retireArtifact(artifactId);
		const objection = createResearchNode({
			kind: "objection",
			statement: reason,
			status: "open",
			stageId: targetStageId,
			domainRef: decisionRef,
			sourceRefs: [decisionRef],
		});
		await this.commit({
			type: "stage_reopened",
			targetStageId,
			affectedStageIds,
			decisionRef,
			reason,
			objection,
		});
	}

	private async retireArtifact(artifactId: string, replacementId?: string): Promise<void> {
		const artifact = this.snapshot.canonical[artifactId];
		if (!artifact || artifact.status === "retired") return;
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
		if (artifact.materializationRef) await rm(artifact.materializationRef, { force: true });
		const archiveRefs = await this.pruneTaskFiles([evidence.taskId, ...reviewerTaskIds]);
		if (archiveRefs.length > 0) receipt.archiveRefs = archiveRefs;
		if (receipt.materializationReceiptRef) {
			try {
				await atomicWriteJson(receipt.materializationReceiptRef, {
					schemaVersion: "astra.retired_artifact_receipt.v1",
					...receipt,
				});
			} catch (error) {
				if (
					(error as NodeJS.ErrnoException).code !== "ENOENT" &&
					(error as NodeJS.ErrnoException).code !== "EACCES"
				)
					throw error;
			}
		}
		await this.commit({ type: "artifact_retired", receipt });
	}

	private async pruneTaskFiles(taskIds: string[]): Promise<string[]> {
		const uniqueTaskIds = new Set(taskIds);
		const jobRoot = join(this.snapshot.frame.permissions.workspaceRoot, ".astra", "jobs", this.snapshot.frame.jobId);
		const archiveRefs: string[] = [];
		for (const taskId of uniqueTaskIds) {
			const archiveRoot = join(jobRoot, "archive", "tasks", taskId);
			let archived = false;
			try {
				await cp(join(jobRoot, "workspaces", taskId), join(archiveRoot, "workspace"), {
					recursive: true,
					force: true,
				});
				archived = true;
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			}
			try {
				await cp(join(jobRoot, "tasks", taskId), join(archiveRoot, "task"), {
					recursive: true,
					force: true,
				});
				archived = true;
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			}
			if (archived) archiveRefs.push(archiveRoot);
			try {
				await access(join(jobRoot, "resources", taskId));
				await mkdir(archiveRoot, { recursive: true });
				await rm(join(archiveRoot, "resources"), { recursive: true, force: true });
				await rename(join(jobRoot, "resources", taskId), join(archiveRoot, "resources"));
				if (!archived) archiveRefs.push(archiveRoot);
			} catch (error) {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			}
			await rm(join(jobRoot, "workspaces", taskId), { recursive: true, force: true });
			await rm(join(jobRoot, "tasks", taskId), { recursive: true, force: true });
		}
		for (const session of Object.values(this.snapshot.sessions)) {
			if (uniqueTaskIds.has(session.taskId) && session.sessionFile) await rm(session.sessionFile, { force: true });
		}
		return archiveRefs;
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
		if (this.snapshot.paused) throw new Error("research job is paused");
		const replayKey = input.replayKey ?? `${this.snapshot.frame.jobId}:${input.stageId}:${input.objective}`;
		const duplicate = Object.values(this.snapshot.tasks).find(
			(task) => task.replayKey === replayKey && task.status !== "failed",
		);
		if (duplicate) return structuredClone(duplicate);
		if (Object.keys(this.snapshot.tasks).length >= this.snapshot.frame.budget.maxTasks) {
			throw new ResearchTaskBudgetError();
		}
		this.assertCurrentInputs(input.inputArtifactRefs, input.repairOfEvidenceId);
		if (input.role === "worker" && input.planId) {
			if (planReviewStatus(this, input.planId) !== "passed")
				throw new Error("worker dispatch requires an independently approved plan");
			const planned = this.snapshot.stagePlans[input.planId]?.tasks.find(
				(task) => input.replayKey === `stage-plan:${input.planId}:${task.key}`,
			);
			if (
				!planned ||
				planned.objective !== input.objective ||
				(planned.deliveryKind ?? "stage") !== (input.deliveryKind ?? "stage") ||
				planned.requiredOutputFields.some((field) => !input.requiredOutputFields.includes(field)) ||
				planned.acceptanceChecks.some((check) => !input.acceptanceChecks.includes(check)) ||
				planned.inputArtifactRefs.some((ref) => !input.inputArtifactRefs.includes(ref))
			)
				throw new Error("worker contract differs from the reviewed plan");
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
		await this.commit({ type: "task_dispatched", task });
		return task;
	}

	unsynthesizedLocalEvidence(stageId: string): Evidence[] {
		const artifact = this.snapshot.canonical[this.snapshot.canonicalRoute.stageArtifactIds[stageId]];
		const sourceTask = artifact
			? this.snapshot.tasks[this.snapshot.evidence[artifact.evidenceId]?.taskId]
			: undefined;
		const consumed = new Set(sourceTask?.inputArtifactRefs ?? []);
		return Object.values(this.snapshot.evidence).filter((evidence) => {
			const task = this.snapshot.tasks[evidence.taskId];
			return (
				evidence.stageId === stageId &&
				evidence.status === "accepted" &&
				task?.deliveryKind === "local" &&
				(task.stageRevision ?? 1) === (this.snapshot.stages[stageId]?.revision ?? 1) &&
				!consumed.has(evidence.id)
			);
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
					(["ready", "running"].includes(task.status) ||
						(task.status === "succeeded" &&
							!Object.values(this.snapshot.evidence).some(
								(evidence) => evidence.taskId === task.id && evidence.status !== "candidate",
							))),
			)
		)
			throw new Error("synthesis must wait for unfinished local tasks and reviews");
		if (
			this.snapshot.frame.openObligationIds.some((id) => {
				const obligation = this.snapshot.obligations[id];
				const review = this.snapshot.reviews[obligation?.sourceReviewId];
				return (
					id !== repairObligationId &&
					obligation?.status === "open" &&
					this.snapshot.evidence[review?.evidenceId]?.stageId === stageId
				);
			})
		)
			throw new Error("synthesis must wait for local review repairs");
	}

	resolveRepairInput(ref: string): string {
		const retired =
			this.snapshot.retiredArtifacts[ref] ??
			Object.values(this.snapshot.retiredArtifacts).find((receipt) => receipt.evidenceId === ref);
		const artifact =
			this.snapshot.canonical[ref] ??
			Object.values(this.snapshot.canonical).find((entry) => entry.evidenceId === ref);
		if (!retired && artifact?.status !== "stale") return ref;
		const taskId = retired?.taskId ?? (artifact ? this.snapshot.evidence[artifact.evidenceId]?.taskId : undefined);
		const stageId = taskId ? this.snapshot.tasks[taskId]?.stageId : undefined;
		const replacement = stageId ? this.snapshot.canonicalRoute.stageArtifactIds[stageId] : undefined;
		if (!replacement || this.snapshot.canonical[replacement]?.status !== "active")
			throw new Error(`Repair input ${ref} requires a reviewed active replacement before retry`);
		return replacement;
	}

	private assertCurrentInputs(refs: string[], repairOfEvidenceId?: string, checked = new Set<string>()): void {
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
						if (!refs.includes(currentRef)) throw new Error(`Repair omits current input ${currentRef}`);
					}
					// The failed snapshot is a comparison target, not a current scientific input.
					continue;
				}
			}
			if (checked.has(ref)) continue;
			checked.add(ref);
			if (this.snapshot.retiredArtifacts[ref] || this.snapshot.canonical[ref]?.status === "stale")
				throw new Error(`task input version is stale: ${ref}`);
			const retiredEvidence = Object.values(this.snapshot.retiredArtifacts).some(
				(receipt) => receipt.evidenceId === ref,
			);
			const staleEvidence = Object.values(this.snapshot.canonical).some(
				(artifact) => artifact.evidenceId === ref && artifact.status === "stale",
			);
			if (retiredEvidence || staleEvidence) throw new Error(`task input version is stale: ${ref}`);
			const evidence = this.snapshot.evidence[ref];
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
				);
		}
	}

	async captureTaskVersion(taskId: string): Promise<TaskVersion> {
		const task = this.snapshot.tasks[taskId];
		if (!task) throw new Error(`unknown task ${taskId}`);
		this.assertCurrentInputs(task.inputArtifactRefs, task.repairOfEvidenceId);
		const git = await captureGitVersion(
			task.scope.workspaceRoot,
			join(task.scope.workspaceRoot, ".astra", "jobs", task.jobId, "versions", "git"),
		);
		const contractHash = checksum({
			planId: task.planId,
			repairChecks: task.repairChecks,
			deliveryKind: task.deliveryKind,
			stageRevision: task.stageRevision,
			repairOfEvidenceId: task.repairOfEvidenceId,
			stage: this.definitions[task.stageId],
			objective: task.objective,
			requiredOutputFields: task.requiredOutputFields,
			acceptanceChecks: task.acceptanceChecks,
			failureSignals: task.failureSignals,
			successCriteria: task.successCriteria,
		});
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
		const task = this.snapshot.tasks[taskId];
		if (!task) throw new Error(`unknown task ${taskId}`);
		if (task.status === "succeeded" && status !== "succeeded")
			throw new Error("succeeded task cannot move backwards");
		await this.commit({ type: "task_status", taskId, status });
		if (
			status === "failed" &&
			task.role === "worker" &&
			task.searchBatchId &&
			task.searchCandidateId &&
			task.attempt >= MAX_TASK_ATTEMPTS
		) {
			await this.markSearchCandidateFailed(task.searchBatchId, task.searchCandidateId);
		}
	}

	async markSearchCandidateFailed(batchId: string, candidateId: string): Promise<void> {
		const batch = this.snapshot.searchBatches[batchId];
		const candidate = batch?.candidates[candidateId];
		if (!batch || !candidate || candidate.status === "failed") return;
		await this.commit({
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
			await this.commit({
				type: "search_batch_exhausted",
				batchId,
				rationale: `All search candidates failed after ${MAX_TASK_ATTEMPTS} worker attempts`,
			});
		}
	}

	async failUncommittedReviewerTask(taskId: string): Promise<void> {
		const task = this.snapshot.tasks[taskId];
		if (!task) throw new Error(`unknown task ${taskId}`);
		if (task.role !== "reviewer") throw new Error("only reviewer tasks may use uncommitted review recovery");
		if (Object.values(this.snapshot.reviews).some((review) => review.reviewerTaskId === taskId)) {
			throw new Error("committed reviewer task cannot move backwards");
		}
		if (task.status === "failed") return;
		await this.commit({ type: "task_status", taskId, status: "failed" });
	}

	async recordChildSession(session: import("./types.ts").ChildSessionRecord): Promise<void> {
		await this.commit({ type: "child_session_recorded", session });
	}

	async recordEvidence(
		input: Omit<Evidence, "id" | "checksum" | "createdAt" | "status"> & { id?: string },
	): Promise<Evidence> {
		const task = this.snapshot.tasks[input.taskId];
		if (!task) throw new Error(`unknown evidence task ${input.taskId}`);
		if (task.status !== "succeeded") throw new Error("evidence requires a succeeded task");
		if (task.deliveryKind && (input.type !== task.requiredOutputType || input.stageId !== task.stageId))
			throw new Error("evidence does not match task delivery contract");
		this.assertCurrentInputs(task.inputArtifactRefs, task.repairOfEvidenceId);
		const files = await freezeEvidenceFiles(task, input.refs);
		const evidence: Evidence = {
			...input,
			files,
			taskVersion: task.version,
			versionHash: checksum({ content: input.content, refs: input.refs, files, taskVersion: task.version }),
			id: input.id ?? `evidence_${randomUUID()}`,
			checksum: checksum(input.content),
			createdAt: new Date().toISOString(),
			status: "candidate",
			currentEvidenceSetId: input.currentEvidenceSetId ?? `${input.stageId}::${input.taskId}`,
		};
		await this.commit({ type: "evidence_recorded", evidence });
		if (evidence.type === "stage-plan") return evidence;
		const taskCandidate = task.searchBatchId
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
		await this.commit({
			type: "research_node_recorded",
			node,
			edge: createResearchEdge({
				fromNodeId: node.id,
				toNodeId: taskCandidate?.graphNodeId ?? this.snapshot.graph.rootQuestionId,
				kind: "tests",
				sourceRefs: [evidence.id],
			}),
		});
		return evidence;
	}

	async recordReview(input: Omit<Review, "id" | "createdAt"> & { id?: string }): Promise<Review> {
		const evidence = this.snapshot.evidence[input.evidenceId];
		if (!evidence) throw new Error(`unknown evidence ${input.evidenceId}`);
		if (input.targetVersionHash && input.targetVersionHash !== evidence.versionHash)
			throw new Error("review target version does not match evidence");
		const definition = this.definitions[evidence.stageId];
		const sourceTask = this.snapshot.tasks[evidence.taskId];
		const expectedCriteria = [
			...new Set([
				...(sourceTask?.acceptanceChecks ?? definition?.acceptanceChecks ?? []),
				...(sourceTask?.successCriteria ?? []),
				...(sourceTask?.repairChecks ?? []).map((check) => check.criterion),
			]),
		];
		validateReviewAssessment(input, expectedCriteria);
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
		await this.commit({ type: "review_recorded", review });
		if (["fail", "partial", "blocked"].includes(review.verdict) && review.blocking !== false) {
			const objection = createResearchNode({
				kind: "objection",
				statement: review.findings.join("; ") || "review failed",
				status: "open",
				stageId: evidence.stageId,
				domainRef: review.id,
				sourceRefs: [review.id, evidence.id],
			});
			await this.commit({
				type: "research_node_recorded",
				node: objection,
				edge: createResearchEdge({
					fromNodeId: objection.id,
					toNodeId: `research_evidence_${evidence.id}`,
					kind: "contradicts",
					sourceRefs: [review.id],
				}),
			});
			const obligation: Obligation = {
				stageId: evidence.stageId,
				evidenceId: evidence.id,
				targetVersionHash: evidence.versionHash,
				id: `obligation_${randomUUID()}`,
				sourceReviewId: review.id,
				description: review.findings.join("; ") || "review failed",
				graphObjectionId: objection.id,
				status: "open",
				createdAt: new Date().toISOString(),
			};
			obligation.items = [
				...new Set([
					...(review.criteria ?? [])
						.filter((criterion) => !criterion.passed)
						.map((criterion) => criterion.criterion),
					...review.findings,
				]),
			].map((criterion, index) => ({ id: `${obligation.id}_${index + 1}`, criterion, status: "open" }));
			await this.commit({ type: "obligation_created", obligation });
		}
		return review;
	}

	async adoptEvidence(evidenceId: string, replacementOf?: string): Promise<CanonicalArtifact> {
		const evidence = this.snapshot.evidence[evidenceId];
		if (!evidence) throw new Error(`unknown evidence ${evidenceId}`);
		const deliveryTask = this.snapshot.tasks[evidence.taskId];
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
		this.assertCurrentInputs(sourceTask.inputArtifactRefs, sourceTask.repairOfEvidenceId);
		if (sourceTask?.searchBatchId && sourceTask.searchCandidateId) {
			const batch = this.snapshot.searchBatches[sourceTask.searchBatchId];
			if (batch?.selectedCandidateId !== sourceTask.searchCandidateId) {
				throw new Error("search evidence must be selected before adoption");
			}
		}
		const reviews = Object.values(this.snapshot.reviews).filter((review) => review.evidenceId === evidenceId);
		if (!reviews.some((review) => review.verdict === "pass"))
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
		const replacedArtifactId = replacementOf ?? currentRouteArtifactId;
		if (replacedArtifactId && !this.snapshot.canonical[replacedArtifactId])
			throw new Error(`unknown replacement artifact ${replacementOf}`);
		const artifact: CanonicalArtifact = {
			id: `artifact_${randomUUID()}`,
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
		const materialized = `${JSON.stringify(artifact.content, null, 2)}\n`;
		artifact.targetSha256 = sha256(materialized);
		if (replacedArtifactId) await this.retireArtifact(replacedArtifactId, artifact.id);
		await this.commit({ type: "evidence_adopted", artifact });
		try {
			await access(this.snapshot.frame.permissions.workspaceRoot);
			const artifactPath = canonicalArtifactPath(
				this.snapshot.frame.permissions.workspaceRoot,
				this.snapshot.frame.jobId,
				artifact.id,
			);
			artifact.materializationRef = artifactPath;
			await atomicWriteJson(artifactPath, artifact.content);
			await atomicWriteJson(
				canonicalReceiptPath(this.snapshot.frame.permissions.workspaceRoot, this.snapshot.frame.jobId, artifact.id),
				{
					schemaVersion: "astra.materialization_receipt.v1",
					artifactId: artifact.id,
					sourceSha256: artifact.sourceSha256,
					targetSha256: artifact.targetSha256,
					targetPath: artifactPath,
					createdAt: artifact.adoptedAt,
				},
			);
			await this.commit({
				type: "canonical_artifact_materialized",
				artifactId: artifact.id,
				materializationRef: artifactPath,
				targetSha256: artifact.targetSha256,
			});
			artifact.status = "materialized";
			await this.commit({ type: "canonical_artifact_status", artifactId: artifact.id, status: "materialized" });
			artifact.status = "baseline_visible";
			await this.commit({ type: "canonical_artifact_status", artifactId: artifact.id, status: "baseline_visible" });
			const materializedContent = await readFile(artifactPath, "utf8");
			if (sha256(materializedContent) !== artifact.targetSha256) {
				throw new Error(`canonical artifact ${artifact.id} failed materialization integrity verification`);
			}
			artifact.status = "integration_verified";
			await this.commit({
				type: "canonical_artifact_status",
				artifactId: artifact.id,
				status: "integration_verified",
			});
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT" && (error as NodeJS.ErrnoException).code !== "EACCES")
				throw error;
		}
		artifact.status = "active";
		await this.commit({ type: "canonical_artifact_status", artifactId: artifact.id, status: "active" });
		for (const nodeId of this.snapshot.graph.unresolvedObjectionIds) {
			const objection = this.snapshot.graph.nodes[nodeId];
			if (
				objection.stageId === evidence.stageId &&
				objection.domainRef === this.snapshot.stages[evidence.stageId].lastRouteDecisionRef &&
				sourceTask?.acceptanceChecks.includes(objection.statement)
			) {
				await this.commit({ type: "research_node_status", nodeId, status: "resolved" });
			}
		}
		const artifactNode = createResearchNode({
			id: `research_artifact_${artifact.id}`,
			kind: "artifact",
			statement: `${artifact.type} canonical artifact`,
			status: "accepted",
			stageId: evidence.stageId,
			domainRef: artifact.id,
			sourceRefs: [evidence.id, ...(artifact.materializationRef ? [artifact.materializationRef] : [])],
		});
		await this.commit({
			type: "research_node_recorded",
			node: artifactNode,
			edge: createResearchEdge({
				fromNodeId: `research_evidence_${evidence.id}`,
				toNodeId: artifactNode.id,
				kind: "derives",
				sourceRefs: [artifact.id],
			}),
		});
		if (artifact.type === "result-to-claim") {
			await this.recordClaimsFromArtifact(artifact, artifactNode.id);
			await this.commit({
				type: "scientific_outcome_recorded",
				outcome: resultAssessment?.outcome ?? "inconclusive",
				missionCoverage: resultAssessment?.missionCoverage ?? "insufficient",
				reason: resultAssessment?.reason ?? "result assessment unavailable",
				sourceArtifactId: artifact.id,
			});
		}
		await this.pruneSupersededEvidence(evidence);
		return artifact;
	}

	private async pruneSupersededEvidence(winner: Evidence): Promise<void> {
		const evidenceSetId = winner.currentEvidenceSetId ?? winner.taskId;
		const superseded = Object.values(this.snapshot.evidence).filter(
			(evidence) =>
				evidence.id !== winner.id &&
				evidence.status === "rejected" &&
				(evidence.currentEvidenceSetId ?? evidence.taskId) === evidenceSetId &&
				!Object.values(this.snapshot.canonical).some((artifact) => artifact.evidenceId === evidence.id),
		);
		for (const evidence of superseded) {
			const reviews = Object.values(this.snapshot.reviews).filter((review) => review.evidenceId === evidence.id);
			const reviewIds = reviews.map((review) => review.id);
			const archiveRefs = await this.pruneTaskFiles([
				evidence.taskId,
				...reviews.flatMap((review) => (review.reviewerTaskId ? [review.reviewerTaskId] : [])),
			]);
			const receipt: DiscardedEvidenceReceipt = {
				evidenceId: evidence.id,
				taskId: evidence.taskId,
				reviewIds,
				checksum: evidence.checksum,
				reason: "superseded-repair",
				...(archiveRefs.length > 0 ? { archiveRefs } : {}),
				workspacePrunedAt: new Date().toISOString(),
			};
			await this.commit({ type: "evidence_pruned", receipt });
		}
	}

	private async recordClaimsFromArtifact(artifact: CanonicalArtifact, artifactNodeId: string): Promise<void> {
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
			const node = createResearchNode({
				kind: "claim",
				statement,
				status: accepted ? "accepted" : assessment === "unresolved" ? "active" : "rejected",
				stageId: "result-to-claim",
				domainRef: artifact.id,
				sourceRefs: [artifact.id],
			});
			node.claimAssessment = assessment;
			node.actor = "worker";
			await this.commit({
				type: "research_node_recorded",
				node,
				edge: createResearchEdge({
					fromNodeId: artifactNodeId,
					toNodeId: node.id,
					kind: accepted ? "supports" : assessment === "refuted" ? "contradicts" : "derives",
					sourceRefs: [artifact.id],
				}),
			});
		}
	}

	async decideEvidence(
		evidenceId: string,
		accepted: boolean,
		decisionRef = `main_agent_worker_artifact_decision::${randomUUID()}`,
	): Promise<void> {
		const evidence = this.snapshot.evidence[evidenceId];
		if (!evidence) throw new Error(`unknown evidence ${evidenceId}`);
		if (accepted) {
			this.assertCurrentInputs(
				this.snapshot.tasks[evidence.taskId].inputArtifactRefs,
				this.snapshot.tasks[evidence.taskId].repairOfEvidenceId,
			);
			const policy = this.definitions[evidence.stageId]?.qualityPolicy;
			if (
				Object.values(this.snapshot.reviews).some(
					(review) => review.evidenceId === evidenceId && review.verdict !== "pass",
				)
			) {
				throw new Error("evidence with a non-passing review requires a repaired candidate before acceptance");
			}
			const passingReviews = Object.values(this.snapshot.reviews).filter(
				(review) =>
					review.evidenceId === evidenceId &&
					review.verdict === "pass" &&
					(review.score ?? 0) >= (policy?.minScore ?? 0.8) &&
					review.targetVersionHash === evidence.versionHash,
			);
			if (passingReviews.length < (policy?.minPassingReviews ?? 1)) {
				throw new Error("evidence requires the configured passing reviews before acceptance");
			}
			for (const issue of Object.values(this.snapshot.obligations)) {
				const failed = this.snapshot.evidence[this.snapshot.reviews[issue.sourceReviewId]?.evidenceId];
				if (issue.status !== "open" || !failed || failed.currentEvidenceSetId !== evidence.currentEvidenceSetId)
					continue;
				for (const item of issue.items ?? []) {
					const binding = this.snapshot.tasks[evidence.taskId].repairChecks?.find(
						(check) => check.issueId === item.id,
					);
					if (
						!binding ||
						binding.criterion !== `[${item.id}] ${item.criterion}` ||
						!passingReviews.every((review) =>
							review.criteria?.some(
								(criterion) => criterion.criterion === binding.criterion && criterion.passed,
							),
						)
					)
						throw new Error(`repair item requires explicit verified closure: ${item.id}`);
				}
			}
		}
		await this.commit({ type: "evidence_decided", evidenceId, accepted, decisionRef });
		if (accepted) {
			for (const obligation of Object.values(this.snapshot.obligations)) {
				if (obligation.status !== "open") continue;
				const failedReview = this.snapshot.reviews[obligation.sourceReviewId];
				const failedEvidence = failedReview ? this.snapshot.evidence[failedReview.evidenceId] : undefined;
				if (
					failedEvidence &&
					failedEvidence.id !== evidence.id &&
					failedEvidence.stageId === evidence.stageId &&
					failedEvidence.currentEvidenceSetId === evidence.currentEvidenceSetId
				) {
					for (const item of obligation.items ?? []) {
						const binding = this.snapshot.tasks[evidence.taskId].repairChecks?.find(
							(check) => check.issueId === item.id,
						);
						const review = binding
							? Object.values(this.snapshot.reviews).find(
									(review) =>
										review.evidenceId === evidence.id &&
										review.verdict === "pass" &&
										review.targetVersionHash === evidence.versionHash &&
										review.criteria?.some(
											(criterion) => criterion.criterion === binding.criterion && criterion.passed,
										),
								)
							: undefined;
						if (review)
							await this.commit({
								type: "repair_item_resolved",
								obligationId: obligation.id,
								itemId: item.id,
								reviewId: review.id,
								evidenceId: evidence.id,
							});
					}
					if (this.snapshot.obligations[obligation.id].items?.some((item) => item.status === "open")) continue;
					await this.commit({
						type: "obligation_resolved",
						obligationId: obligation.id,
						satisfiedBy: decisionRef,
					});
					if (obligation.graphObjectionId) {
						await this.commit({
							type: "research_node_status",
							nodeId: obligation.graphObjectionId,
							status: "resolved",
						});
					}
				}
			}
		}
	}

	async recordCandidateEvaluation(
		input: Omit<CandidateEvaluation, "id" | "createdAt"> & { id?: string },
	): Promise<CandidateEvaluation> {
		const batch = this.snapshot.searchBatches[input.batchId];
		const candidate = batch?.candidates[input.candidateId];
		if (!batch || !candidate) throw new Error("candidate evaluation references an unknown search candidate");
		if (candidate.evidenceId !== input.evidenceId) throw new Error("candidate evaluation evidence does not match");
		const review = this.snapshot.reviews[input.reviewId];
		if (!review || review.evidenceId !== input.evidenceId)
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
		const evaluation: CandidateEvaluation = {
			...input,
			id: input.id ?? `candidate_evaluation_${randomUUID()}`,
			createdAt: new Date().toISOString(),
		};
		await this.commit({ type: "candidate_evaluation_recorded", evaluation });
		return evaluation;
	}

	async selectSearchCandidate(batchId: string, candidateId: string, decisionRef: string): Promise<void> {
		const batch = this.snapshot.searchBatches[batchId];
		const selected = batch?.candidates[candidateId];
		if (!batch || !selected) throw new Error("search decision references an unknown candidate");
		const evaluations = Object.values(this.snapshot.candidateEvaluations).filter(
			(evaluation) => evaluation.batchId === batchId,
		);
		for (const candidate of Object.values(batch.candidates)) {
			if (
				candidate.status !== "failed" &&
				!evaluations.some((evaluation) => evaluation.candidateId === candidate.id)
			) {
				throw new Error(`search candidate ${candidate.id} has not been independently evaluated`);
			}
		}
		const selectedEvaluation = evaluations.find((evaluation) => evaluation.candidateId === candidateId);
		const minScore = this.definitions[batch.stageId]?.qualityPolicy?.minScore ?? 0.8;
		if (!selectedEvaluation || selectedEvaluation.verdict !== "pass" || selectedEvaluation.score < minScore) {
			throw new Error("selected search candidate does not satisfy the quality threshold");
		}
		await this.commit({ type: "search_batch_decided", batchId, candidateId, decisionRef });
		for (const candidate of Object.values(batch.candidates)) {
			if (!candidate.evidenceId) continue;
			await this.decideEvidence(candidate.evidenceId, candidate.id === candidateId, decisionRef);
			if (candidate.id !== candidateId) await this.pruneSearchCandidate(batchId, candidate.id);
		}
	}

	async continueSearchBatch(batchId: string, decisionRef: string, rationale: string): Promise<void> {
		const batch = this.snapshot.searchBatches[batchId];
		if (!batch) throw new Error("continued search references an unknown batch");
		if (!["planning", "running", "evaluating"].includes(batch.status)) {
			throw new Error(`search batch ${batchId} cannot continue from status ${batch.status}`);
		}
		if (batch.round >= batch.maxRounds) {
			throw new Error(`search batch ${batchId} is already at final round ${batch.round}/${batch.maxRounds}`);
		}
		const evaluations = Object.values(this.snapshot.candidateEvaluations).filter(
			(evaluation) => evaluation.batchId === batchId,
		);
		for (const candidate of Object.values(batch.candidates)) {
			if (
				candidate.status !== "failed" &&
				!evaluations.some((evaluation) => evaluation.candidateId === candidate.id)
			) {
				throw new Error(`search candidate ${candidate.id} has not been independently evaluated`);
			}
		}
		if (!decisionRef.trim()) throw new Error("continued search requires a decision reference");
		if (!rationale.trim()) throw new Error("continued search requires a rationale");
		await this.commit({ type: "search_batch_continued", batchId, decisionRef, rationale });
	}

	private async pruneSearchCandidate(batchId: string, candidateId: string): Promise<void> {
		const candidate = this.snapshot.searchBatches[batchId]?.candidates[candidateId];
		if (!candidate) return;
		const evidence = candidate.evidenceId ? this.snapshot.evidence[candidate.evidenceId] : undefined;
		if (candidate.taskId) {
			const reviewerTaskIds = candidate.evidenceId
				? Object.values(this.snapshot.reviews)
						.filter((review) => review.evidenceId === candidate.evidenceId)
						.flatMap((review) => (review.reviewerTaskId ? [review.reviewerTaskId] : []))
				: [];
			const archiveRefs = await this.pruneTaskFiles([candidate.taskId, ...reviewerTaskIds]);
			const receipt: DiscardedCandidateReceipt = {
				candidateId,
				batchId,
				...(candidate.taskId ? { taskId: candidate.taskId } : {}),
				...(candidate.evidenceId ? { evidenceId: candidate.evidenceId } : {}),
				...(evidence ? { evidenceChecksum: evidence.checksum } : {}),
				...(archiveRefs.length > 0 ? { archiveRefs } : {}),
				workspacePrunedAt: new Date().toISOString(),
			};
			await this.commit({ type: "search_candidate_pruned", receipt });
			return;
		}
		const receipt: DiscardedCandidateReceipt = {
			candidateId,
			batchId,
			...(candidate.taskId ? { taskId: candidate.taskId } : {}),
			...(candidate.evidenceId ? { evidenceId: candidate.evidenceId } : {}),
			...(evidence ? { evidenceChecksum: evidence.checksum } : {}),
			workspacePrunedAt: new Date().toISOString(),
		};
		await this.commit({ type: "search_candidate_pruned", receipt });
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
		if (manifest.decisionType !== "route" || !manifest.routeAction) {
			throw new Error("route decision manifest is incomplete");
		}
		const unknownEvidenceRefs = (manifest.evidenceRefs ?? []).filter(
			(ref) => !this.snapshot.canonical[ref] && !this.snapshot.evidence[ref] && !this.snapshot.graph.nodes[ref],
		);
		if (unknownEvidenceRefs.length > 0) {
			throw new Error(`route decision references unknown evidence: ${unknownEvidenceRefs.join(", ")}`);
		}
		const stageId = this.snapshot.frame.activeStageId;
		if (manifest.stageId !== stageId) throw new Error(`route decision must target active stage ${stageId}`);
		if (["advance", "complete"].includes(manifest.routeAction) && this.unsynthesizedLocalEvidence(stageId).length)
			throw new Error("route requires synthesis of accepted local evidence");
		const currentArtifactId = this.snapshot.canonicalRoute.stageArtifactIds[stageId];
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
		await this.commit({ type: "route_decided", decision });
		for (const question of decision.newQuestions) {
			const node = createResearchNode({
				kind: "question",
				statement: question,
				status: "open",
				stageId,
				domainRef: decision.id,
				sourceRefs: [decision.id],
			});
			await this.commit({
				type: "research_node_recorded",
				node,
				edge: createResearchEdge({
					fromNodeId: this.snapshot.graph.rootQuestionId,
					toNodeId: node.id,
					kind: "refines",
					sourceRefs: [decision.id],
				}),
			});
		}
		if (decision.action === "backtrack" && decision.targetStageId) {
			await this.reopenStage(decision.targetStageId, decision.id, decision.rationale);
		}
		if (decision.action === "ask-user" && decision.question) {
			await this.requireUserGate({
				kind: "research",
				stageId,
				question: decision.question,
				reason: decision.rationale,
			});
		}
	}

	async pause(reason: string): Promise<void> {
		await this.commit({ type: "job_paused", reason });
	}

	async resume(): Promise<void> {
		await this.recoverLegacyProviderCapacityFailure();
		const gate = this.snapshot.frame.userGate;
		if (!gate) {
			await this.commit({ type: "job_resumed" });
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
		await this.commit({ type: "user_gate_approved", gate, approvedAt: new Date().toISOString() });
	}

	async resumeWithGuidance(guidance: string): Promise<void> {
		if (this.snapshot.frame.userGate && this.snapshot.frame.userGate.kind !== "research") {
			throw new Error("guided resume cannot bypass a stage or budget gate");
		}
		await this.recoverLegacyProviderCapacityFailure();
		const node = await this.recordUserGuidance(guidance);
		const stageId = this.snapshot.frame.activeStageId;
		const activeBatchIds = new Set(
			Object.values(this.snapshot.searchBatches)
				.filter(
					(batch) => batch.stageId === stageId && ["planning", "running", "evaluating"].includes(batch.status),
				)
				.map((batch) => batch.id),
		);
		const taskIds = Object.values(this.snapshot.tasks)
			.filter(
				(task) =>
					task.stageId === stageId &&
					task.role === "worker" &&
					["ready", "running", "failed"].includes(task.status) &&
					(activeBatchIds.size === 0
						? ["ready", "running"].includes(task.status)
						: activeBatchIds.has(task.searchBatchId ?? "")),
			)
			.map((task) => task.id);
		await this.commit({
			type: "active_stage_work_superseded",
			stageId,
			taskIds,
			reason: node.statement,
		});
		await this.commit({ type: "job_resumed" });
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

	private commit(event: AstraEvent): Promise<void> {
		const run = this.commitChain.then(async () => {
			const stored = await this.store.append(this.snapshot.frame.jobId, event);
			this.apply(stored.event, stored.timestamp);
			this.snapshot.eventSeq = stored.seq;
			this.snapshot.updatedAt = stored.timestamp;
			await this.persist();
		});
		this.commitChain = run.catch(() => undefined);
		return run;
	}

	private async persist(): Promise<void> {
		await this.store.writeSnapshot(this.snapshot);
	}

	private apply(event: AstraEvent, timestamp: string): void {
		switch (event.type) {
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
				return;
			case "review_recorded":
				this.snapshot.reviews[event.review.id] = structuredClone(event.review);
				if (event.review.verdict !== "pass" || (event.review.score ?? 0) < 0.8) {
					const evidence = this.snapshot.evidence[event.review.evidenceId];
					if (evidence?.type === "stage-plan")
						for (const batch of Object.values(this.snapshot.searchBatches)) {
							if (batch.planId === this.snapshot.tasks[evidence.taskId]?.planId) batch.status = "superseded";
						}
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
				this.snapshot.canonical[event.artifactId].status = event.status;
				if (event.status === "active") {
					const artifact = this.snapshot.canonical[event.artifactId];
					const stageId = this.snapshot.evidence[artifact.evidenceId]?.stageId;
					if (stageId) {
						this.snapshot.stages[stageId].invalidatedBy = undefined;
						this.snapshot.canonicalRoute.stageArtifactIds[stageId] = artifact.id;
						this.snapshot.canonicalRoute.revision += 1;
						this.snapshot.canonicalRoute.updatedAt = timestamp;
					}
				}
				return;
			}
			case "artifact_retired": {
				const retiredArtifactId = event.receipt.artifactId;
				const sourceStageId = this.snapshot.evidence[event.receipt.evidenceId]?.stageId;
				if (sourceStageId) {
					const sourceStage = this.snapshot.stages[sourceStageId];
					sourceStage.status = sourceStageId === this.snapshot.frame.activeStageId ? "running" : "pending";
					sourceStage.completedAt = undefined;
					sourceStage.routeApproval = undefined;
					sourceStage.invalidatedBy = retiredArtifactId;
				}
				const invalidRefs = new Set([retiredArtifactId, event.receipt.evidenceId]);
				let changed = true;
				while (changed) {
					changed = false;
					for (const artifact of Object.values(this.snapshot.canonical)) {
						if (invalidRefs.has(artifact.id) || artifact.status !== "active") continue;
						const evidence = this.snapshot.evidence[artifact.evidenceId];
						const task = this.snapshot.tasks[evidence.taskId];
						if (!task.inputArtifactRefs.some((ref) => invalidRefs.has(ref))) continue;
						invalidRefs.add(artifact.id);
						invalidRefs.add(evidence.id);
						artifact.status = "stale";
						artifact.invalidatedBy = retiredArtifactId;
						const stage = this.snapshot.stages[evidence.stageId];
						stage.status = evidence.stageId === this.snapshot.frame.activeStageId ? "running" : "pending";
						stage.completedAt = undefined;
						stage.routeApproval = undefined;
						stage.invalidatedBy = retiredArtifactId;
						stage.revision = (stage.revision ?? 1) + 1;
						stage.executionId = undefined;
						delete this.snapshot.canonicalRoute.stageArtifactIds[evidence.stageId];
						delete this.snapshot.canonicalRoute.selectedCandidateIds[evidence.stageId];
						for (const batch of Object.values(this.snapshot.searchBatches)) {
							if (batch.stageId === evidence.stageId) batch.status = "superseded";
						}
						changed = true;
					}
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
				this.snapshot.frame.nextAction = `dispatch ${this.snapshot.frame.activeStageId}`;
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
							: `dispatch ${this.snapshot.frame.activeStageId}`;
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
					this.snapshot.frame.nextAction = `dispatch ${this.snapshot.frame.activeStageId}`;
				}
				return;
			case "user_guidance_recorded":
				addNodeToGraph(this.snapshot.graph, event.node);
				if (this.snapshot.frame.userGate?.kind === "research") {
					this.snapshot.frame.userGate = undefined;
					this.snapshot.paused = false;
					this.snapshot.frame.status = "running";
				}
				this.snapshot.frame.nextAction = `main-agent reconsider ${this.snapshot.frame.activeStageId} with user guidance`;
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
				return;
			}
			case "search_candidate_pruned": {
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
			case "route_decided": {
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
				return;
			}
			case "stage_reopened":
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
				this.snapshot.frame.nextAction = `revisit ${event.targetStageId}: ${event.reason}`;
				this.snapshot.frame.userGate = undefined;
				this.snapshot.paused = false;
				return;
		}
	}
}
