export type AutomationLevel = "collaborative" | "autonomous" | "full";
export type StageStatus = "pending" | "running" | "completed" | "blocked";
export type TaskStatus = "ready" | "running" | "succeeded" | "failed" | "blocked";
export type ReviewVerdict = "pass" | "fail" | "partial" | "blocked";
export type Role = "main-agent" | "worker" | "reviewer" | "supervisor";
export type BudgetLimit = "maxTasks" | "maxTurns" | "maxCostUsd";
export type ResearchNodeKind = "question" | "hypothesis" | "claim" | "evidence" | "objection" | "decision" | "artifact";
export type ResearchNodeStatus = "open" | "active" | "accepted" | "rejected" | "resolved" | "superseded";
export type ResearchEdgeKind = "supports" | "contradicts" | "tests" | "derives" | "refines" | "resolves" | "supersedes";
export type ResearchRouteAction = "continue" | "search" | "advance" | "backtrack" | "ask-user" | "complete";
export type ResearchRunStatus = "running" | "waiting-for-user" | "completed";
export type ScientificOutcome =
	| "pending"
	| "supported"
	| "partially-supported"
	| "refuted"
	| "inconclusive"
	| "insufficient-evidence";
export type MissionCoverage = "pending" | "sufficient" | "insufficient";
export type ClaimAssessment = "supported" | "partially-supported" | "refuted" | "unsupported" | "unresolved";

export interface TaskBudget {
	maxTurns: number;
	maxToolCalls: number;
	maxRuntimeMs: number;
}

export type UserGate =
	| {
			kind: "stage";
			stageId: string;
			phase: "route";
			reason: string;
			requiredAt: string;
	  }
	| {
			kind: "budget";
			stageId: string;
			limit: BudgetLimit;
			reason: string;
			requiredAt: string;
	  }
	| {
			kind: "research";
			stageId: string;
			question: string;
			reason: string;
			requiredAt: string;
	  };

export type UserGateRequest =
	| Omit<Extract<UserGate, { kind: "stage" }>, "requiredAt">
	| Omit<Extract<UserGate, { kind: "budget" }>, "requiredAt">
	| Omit<Extract<UserGate, { kind: "research" }>, "requiredAt">;

export interface MissionFrame {
	goalId: string;
	jobId: string;
	objective: string;
	boundaries: string[];
	automation: AutomationLevel;
	budget: { maxTasks: number; maxTurns: number; maxCostUsd?: number };
	permissions: { allowedTools: string[]; workspaceRoot: string; allowDestructive: boolean };
	acceptance: string[];
	requiredArtifactTypes: string[];
	status: ResearchRunStatus;
	scientificOutcome: ScientificOutcome;
	missionCoverage: MissionCoverage;
	scientificOutcomeReason?: string;
	activeStageId: string;
	nextAction: string;
	openObligationIds: string[];
	finalDecisionRef?: string;
	completedAt?: string;
	userGate?: UserGate;
}

export interface StageDefinition {
	id: string;
	label: string;
	suggestedInputArtifactTypes: string[];
	outputArtifactType: string;
	requiredOutputFields: string[];
	acceptanceChecks: string[];
	failureSignals: string[];
	workerTaskFamily: string;
	workerTools: string[];
	workspaceWrite: boolean;
	workerBudget?: TaskBudget;
	reviewerBudget?: TaskBudget;
	minSourceRefs: number;
	gate: "none" | "user" | "main-agent";
	searchPolicy?: {
		strategy: "diverse-candidates" | "best-first";
		minCandidates: number;
		maxCandidates: number;
		maxRounds?: number;
		criteria: string[];
	};
	qualityPolicy?: {
		minPassingReviews: number;
		minScore: number;
		requireResolvableArtifacts: boolean;
	};
}

export interface StageState {
	invalidatedBy?: string;
	definitionId: string;
	status: StageStatus;
	executionId?: string;
	completedAt?: string;
	routeApproval?: string;
	revision?: number;
	lastRouteDecisionRef?: string;
	lastRouteAction?: ResearchRouteAction;
	lastRoutedArtifactId?: string;
}

export interface TaskPacket {
	planId?: string;
	effectiveContractHash?: string;
	repairChecks?: Array<{ issueId: string; criterion: string }>;
	responsibilityBindings?: Array<{ nodeId: string; stageId: string; phase: "stage" | "synthesis" }>;
	responsibilityTransfers?: ResponsibilityTransfer[];
	deliveryKind?: "stage" | "local" | "synthesis";
	stageRevision?: number;
	repairOfEvidenceId?: string;
	version?: TaskVersion;
	schemaVersion: "astra.task_packet.v1";
	id: string;
	jobId: string;
	agentId: string;
	stageId: string;
	stageExecutionId: string;
	role: Exclude<Role, "supervisor">;
	runnerKind: "pi-session";
	objective: string;
	inputArtifactRefs: string[];
	requiredCanonicalArtifacts: string[];
	requiredOutputType: string;
	requiredOutputFields: string[];
	acceptanceChecks: string[];
	failureSignals: string[];
	dependencies: string[];
	scope: { workspaceRoot: string; allowedPaths: string[] };
	allowedTools: string[];
	writeAuthority: "workspace-write" | "none";
	budget: TaskBudget;
	outputManifestRequired: true;
	reviewGateRequired: boolean;
	resumePolicy: "resume-session" | "restart-attempt";
	successCriteria: string[];
	replayKey: string;
	attempt: number;
	status: TaskStatus;
	supersedesTaskId?: string;
	searchBatchId?: string;
	searchCandidateId?: string;
	createdAt: string;
}

export interface OutputRef {
	kind: "artifact" | "source" | "log" | "session";
	ref: string;
	summary: string;
	sha256?: string;
}

export interface WorkerOutputManifest {
	schemaVersion: "astra.worker_output_manifest.v1";
	manifestId: string;
	jobId: string;
	taskId: string;
	agentId: string;
	status: "completed" | "failed";
	artifactType: string;
	content: unknown;
	incrementalRevision?: IncrementalRevision;
	outputRefs: OutputRef[];
	validationStatus: "passed" | "failed";
	validationErrors: string[];
	sessionRef: string;
	createdAt: string;
}

export interface IncrementalRevision {
	baseEvidenceId: string;
	baseHash: string;
	operations: Array<{
		op: "set" | "delete";
		path: string[];
		value?: unknown;
		issueId?: string;
		sourceRefs: string[];
		reason: string;
	}>;
	resultHash: string;
	affectedCriteria: string[];
	rationale: string;
}

export interface ReviewerOutputManifest {
	schemaVersion: "astra.reviewer_output_manifest.v1";
	manifestId: string;
	jobId: string;
	taskId: string;
	evidenceId: string;
	verdict: ReviewVerdict;
	findings: string[];
	score: number;
	criteria: CriterionAssessment[];
	verifiedRefs: string[];
	sessionRef: string;
	createdAt: string;
}

export interface CriterionAssessment {
	criterion: string;
	passed: boolean;
	score: number;
	evidenceRefs: string[];
	rationale: string;
}

export interface ReviewPacket {
	schemaVersion: "astra.review_packet.v1";
	id: string;
	jobId: string;
	taskId: string;
	evidenceId: string;
	targetSnapshotHash: string;
	targetSnapshotRef: string;
	inputRefs: string[];
	resolvedEvidenceRefs: Array<{
		sourceRef: string;
		path: string;
		sha256: string;
	}>;
	objective: string;
	stageContract: {
		stageId: string;
		label: string;
		outputArtifactType: string;
		requiredOutputFields: string[];
		acceptanceChecks: string[];
		failureSignals: string[];
	};
	workerContract: {
		objective: string;
		requiredOutputFields: string[];
		acceptanceChecks: string[];
		failureSignals: string[];
		successCriteria: string[];
	};
	reviewerRole: "reviewer";
	freshThread: true;
	blinded: true;
	bannedContext: string[];
	createdAt: string;
}

export interface ReviewTrace {
	schemaVersion: "astra.review_trace.v1";
	id: string;
	packetId: string;
	jobId: string;
	taskId: string;
	evidenceId: string;
	sessionId: string;
	verdict: ReviewVerdict;
	findings: string[];
	createdAt: string;
}

export interface MainAgentDecisionManifest {
	schemaVersion: "astra.main_agent_decision_manifest.v1";
	manifestId: string;
	jobId: string;
	decisionType: "evidence" | "adoption" | "search-selection" | "route";
	decisionRef: string;
	evidenceId?: string;
	decision?: "accept" | "reject" | "defer";
	adopt?: boolean;
	replacementOf?: string;
	stageId?: string;
	searchBatchId?: string;
	selectedCandidateId?: string;
	continueSearch?: boolean;
	routeAction?: ResearchRouteAction;
	targetStageId?: string;
	evidenceRefs?: string[];
	question?: string;
	newQuestions?: string[];
	rationale: string;
	sessionRef: string;
	createdAt: string;
}

export interface ResponsibilityTransfer {
	id: string;
	planId: string;
	sourceTaskId: string;
	sourceContractHash: string;
	sourceField: "acceptanceChecks";
	sourceIndex: number;
	exactCriterion: string;
	nodeId?: string;
	issueId?: string;
	destinationStageId: string;
	destinationPhase: "synthesis";
	rationale: string;
}

export type ResponsibilityTransferProposal = Omit<ResponsibilityTransfer, "id" | "planId">;

export interface PlannedTask {
	responsibilityTransfers?: ResponsibilityTransferProposal[];
	deliveryKind?: "stage" | "local" | "synthesis";
	responsibilityBindings?: Array<{ nodeId: string; stageId: string; phase: "stage" | "synthesis" }>;
	key: string;
	objective: string;
	inputArtifactRefs: string[];
	requiredOutputFields: string[];
	acceptanceChecks: string[];
	failureSignals: string[];
	successCriteria: string[];
	hypothesis?: string;
}

export interface StagePlanManifest {
	/** Host-only fingerprint of the snapshot read before plan generation. */
	generationBasisHash?: string;
	schemaVersion: "astra.stage_plan_manifest.v1";
	id: string;
	jobId: string;
	stageId: string;
	decisionRef: string;
	mode?: "decompose" | "search" | "repair";
	tasks: PlannedTask[];
	rationale: string;
	sessionRef: string;
	obligationId?: string;
	createdAt: string;
}

export interface TaskRecoveryMaterials {
	previousTaskId: string;
	workspaceRoot?: string;
	sessionFile?: string;
	error?: string;
	readRoots: string[];
}

export interface ChildSessionRecord {
	sessionId: string;
	role: "main-agent" | "worker" | "reviewer";
	taskId: string;
	status: "starting" | "running" | "completed" | "interrupted" | "failed";
	attempt: number;
	sessionFile?: string;
	manifestRef?: string;
	error?: string;
	updatedAt: string;
}

export interface ProviderBackoffState {
	attempt: number;
	reason: string;
	startedAt: string;
	retryAt: string;
}

export interface Evidence {
	incrementalRevision?: IncrementalRevision;
	taskVersion?: TaskVersion;
	files?: EvidenceFileVersion[];
	versionHash?: string;
	id: string;
	taskId: string;
	stageId: string;
	type: string;
	content: unknown;
	refs: string[];
	checksum: string;
	createdAt: string;
	status: "candidate" | "accepted" | "rejected";
	acceptanceAuthority?: "main_agent";
	mainAgentDecisionRef?: string;
	currentEvidenceSetId?: string;
	supersededByTaskId?: string;
}

export interface Review {
	targetVersionHash?: string;
	id: string;
	evidenceId: string;
	reviewerTaskId?: string;
	verdict: ReviewVerdict;
	findings: string[];
	blocking?: boolean;
	score?: number;
	criteria?: CriterionAssessment[];
	verifiedRefs?: string[];
	repairTaskId?: string;
	createdAt: string;
}

export interface Obligation {
	stageId?: string;
	evidenceId?: string;
	targetVersionHash?: string;
	items?: Array<{
		id: string;
		criterion: string;
		status: "open" | "resolved";
		reviewId?: string;
		evidenceId?: string;
	}>;
	id: string;
	sourceReviewId: string;
	description: string;
	graphObjectionId?: string;
	status: "open" | "resolved";
	createdAt: string;
}

export interface CanonicalArtifact {
	adoptionCompletedAt?: string;
	id: string;
	type: string;
	evidenceId: string;
	content: unknown;
	checksum: string;
	status:
		| "candidate_only"
		| "accepted_evidence"
		| "adoption_requested"
		| "materialized"
		| "baseline_visible"
		| "integration_verified"
		| "active"
		| "stale"
		| "retired";
	invalidatedBy?: string;
	replacementOf?: string;
	evidenceSnapshotHash?: string;
	sourceSha256?: string;
	targetSha256?: string;
	materializationRef?: string;
	adoptedAt: string;
}

export interface Lease {
	owner: string;
	expiresAt: string;
	heartbeatAt: string;
}

export interface ResearchNode {
	id: string;
	kind: ResearchNodeKind;
	statement: string;
	status: ResearchNodeStatus;
	stageId?: string;
	domainRef?: string;
	claimAssessment?: ClaimAssessment;
	actor?: "user" | "main-agent" | "worker" | "reviewer" | "system";
	sourceRefs: string[];
	createdAt: string;
	updatedAt: string;
}

export interface ResearchEdge {
	id: string;
	fromNodeId: string;
	toNodeId: string;
	kind: ResearchEdgeKind;
	sourceRefs: string[];
	createdAt: string;
}

export interface ResearchGraph {
	version: 1;
	nodes: Record<string, ResearchNode>;
	edges: Record<string, ResearchEdge>;
	rootQuestionId: string;
	openQuestionIds: string[];
	activeHypothesisIds: string[];
	acceptedClaimIds: string[];
	unresolvedObjectionIds: string[];
	revision: number;
}

export interface SearchCandidate {
	id: string;
	key: string;
	hypothesis: string;
	status: "planned" | "running" | "evaluating" | "selected" | "rejected" | "failed";
	graphNodeId: string;
	taskId?: string;
	evidenceId?: string;
	artifactId?: string;
}

export interface SearchBatch {
	id: string;
	stageId: string;
	planId: string;
	round: number;
	maxRounds: number;
	previousBatchId?: string;
	objective: string;
	strategy: "diverse-candidates" | "best-first";
	status: "planning" | "running" | "evaluating" | "selected" | "exhausted" | "superseded";
	minCandidates: number;
	maxCandidates: number;
	criteria: string[];
	candidates: Record<string, SearchCandidate>;
	selectedCandidateId?: string;
	decisionRef?: string;
	continuationRationale?: string;
	exhaustionRationale?: string;
	createdAt: string;
	updatedAt: string;
}

export interface CandidateEvaluation {
	id: string;
	batchId: string;
	candidateId: string;
	evidenceId: string;
	reviewId: string;
	verdict: ReviewVerdict;
	score: number;
	criteria: CriterionAssessment[];
	findings: string[];
	createdAt: string;
}

export interface StageRouteDecision {
	id: string;
	stageId: string;
	action: ResearchRouteAction;
	targetStageId?: string;
	evidenceRefs: string[];
	question?: string;
	newQuestions: string[];
	rationale: string;
	sessionRef: string;
	createdAt: string;
}

export interface RetiredArtifactReceipt {
	cleanupStatus?: "pending" | "completed";
	artifactId: string;
	type: string;
	evidenceId: string;
	taskId: string;
	reviewIds: string[];
	checksum: string;
	replacementId?: string;
	archiveRefs?: string[];
	materializationReceiptRef?: string;
	retiredAt: string;
}

export interface DiscardedCandidateReceipt {
	cleanupStatus?: "pending" | "completed";
	archivedObligations?: Obligation[];
	archivedReviews?: Review[];
	decisionRef?: string;
	reason?: "unselected-search-candidate";
	candidateId: string;
	batchId: string;
	taskId?: string;
	evidenceId?: string;
	evidenceChecksum?: string;
	archiveRefs?: string[];
	workspacePrunedAt?: string;
}

export interface DiscardedEvidenceReceipt {
	cleanupStatus?: "pending" | "completed";
	evidenceId: string;
	taskId: string;
	reviewIds: string[];
	checksum: string;
	reason: "superseded-repair";
	archiveRefs?: string[];
	workspacePrunedAt?: string;
}

export interface CleanupTaskFiles {
	taskId: string;
	workspace: boolean;
	task: boolean;
	resources: boolean;
	sessions: Array<{ sessionId: string; source: string; target: string; present: boolean }>;
}

export type CleanupIntent = {
	id: string;
	tasks: CleanupTaskFiles[];
	status: "pending" | "completed";
	archiveRefs?: string[];
	completedAt?: string;
} & (
	| { kind: "retirement"; receipt: RetiredArtifactReceipt; materializationRef?: string }
	| { kind: "evidence"; receipt: DiscardedEvidenceReceipt }
	| { kind: "search-candidate"; receipt: DiscardedCandidateReceipt }
);

export interface ReviewConsequences {
	objection?: ResearchNode;
	edge?: ResearchEdge;
	obligation?: Obligation;
}

export interface AdoptionCompletion {
	nodes: ResearchNode[];
	edges: ResearchEdge[];
	resolvedNodeIds: string[];
	repairItems: Array<{ obligationId: string; itemId: string; reviewId: string; evidenceId: string }>;
	resolvedObligations: Array<{ obligationId: string; satisfiedBy: string }>;
	assessment?: { outcome: ScientificOutcome; missionCoverage: MissionCoverage; reason: string };
	cleanups: CleanupIntent[];
	completedAt: string;
}

export interface CanonicalResearchRoute {
	id: "canonical";
	revision: number;
	stageArtifactIds: Record<string, string>;
	selectedCandidateIds: Record<string, string>;
	updatedAt: string;
}

export interface JobSnapshot {
	cleanupIntents?: Record<string, CleanupIntent>;
	stageDefinitions?: Record<string, StageDefinition>;
	version: 1;
	frame: MissionFrame;
	stages: Record<string, StageState>;
	stagePlans: Record<string, StagePlanManifest>;
	tasks: Record<string, TaskPacket>;
	evidence: Record<string, Evidence>;
	reviews: Record<string, Review>;
	obligations: Record<string, Obligation>;
	canonical: Record<string, CanonicalArtifact>;
	retiredArtifacts: Record<string, RetiredArtifactReceipt>;
	discardedCandidates: Record<string, DiscardedCandidateReceipt>;
	discardedEvidence: Record<string, DiscardedEvidenceReceipt>;
	sessions: Record<string, ChildSessionRecord>;
	graph: ResearchGraph;
	searchBatches: Record<string, SearchBatch>;
	candidateEvaluations: Record<string, CandidateEvaluation>;
	routeDecisions: Record<string, StageRouteDecision>;
	mainAgentSessionId: string;
	canonicalRoute: CanonicalResearchRoute;
	budgetUsage?: { turnsUsed: number; costUsdUsed: number };
	providerBackoff?: ProviderBackoffState;
	lease?: Lease;
	paused: boolean;
	eventSeq: number;
	updatedAt: string;
}

export type AstraEvent =
	| { type: "job_created"; snapshot: JobSnapshot }
	| { type: "task_version_recorded"; taskId: string; version: TaskVersion }
	| { type: "lease_acquired"; lease: Lease }
	| { type: "lease_released"; owner: string }
	| { type: "stage_plan_recorded"; plan: StagePlanManifest }
	| { type: "task_dispatched"; task: TaskPacket }
	| { type: "task_status"; taskId: string; status: TaskStatus }
	| { type: "child_session_recorded"; session: ChildSessionRecord }
	| { type: "evidence_recorded"; evidence: Evidence }
	| { type: "evidence_decided"; evidenceId: string; accepted: boolean; decisionRef: string }
	| { type: "review_recorded"; review: Review; consequences?: ReviewConsequences }
	| { type: "obligation_created"; obligation: Obligation }
	| { type: "obligation_resolved"; obligationId: string; satisfiedBy: string }
	| { type: "repair_item_resolved"; obligationId: string; itemId: string; reviewId: string; evidenceId: string }
	| { type: "evidence_adopted"; artifact: CanonicalArtifact }
	| { type: "canonical_artifact_materialized"; artifactId: string; materializationRef: string; targetSha256: string }
	| {
			type: "canonical_artifact_status";
			artifactId: string;
			status: CanonicalArtifact["status"];
			completion?: AdoptionCompletion;
	  }
	| { type: "cleanup_requested"; intent: CleanupIntent }
	| { type: "cleanup_completed"; intentId: string; archiveRefs: string[]; completedAt: string }
	| { type: "artifact_retired"; receipt: RetiredArtifactReceipt }
	| { type: "budget_usage_recorded"; turns: number; costUsd: number }
	| { type: "budget_turns_refunded"; turns: number }
	| { type: "budget_updated"; budget: MissionFrame["budget"] }
	| { type: "provider_backoff_started"; backoff: ProviderBackoffState }
	| { type: "provider_backoff_cleared" }
	| { type: "automation_updated"; automation: AutomationLevel }
	| { type: "user_gate_required"; gate: UserGate }
	| { type: "user_gate_approved"; gate: UserGate; approvedAt: string }
	| { type: "job_paused"; reason: string }
	| { type: "job_resumed" }
	| { type: "user_guidance_recorded"; node: ResearchNode }
	| { type: "active_stage_work_superseded"; stageId: string; taskIds: string[]; reason: string }
	| {
			type: "scientific_outcome_recorded";
			outcome: ScientificOutcome;
			missionCoverage: MissionCoverage;
			reason: string;
			sourceArtifactId: string;
	  }
	| { type: "research_node_recorded"; node: ResearchNode; edge?: ResearchEdge }
	| { type: "research_node_status"; nodeId: string; status: ResearchNodeStatus }
	| { type: "search_batch_recorded"; batch: SearchBatch; nodes: ResearchNode[]; edges: ResearchEdge[] }
	| { type: "search_candidate_updated"; batchId: string; candidate: SearchCandidate }
	| { type: "candidate_evaluation_recorded"; evaluation: CandidateEvaluation }
	| { type: "search_batch_exhausted"; batchId: string; rationale: string }
	| { type: "search_batch_continued"; batchId: string; decisionRef: string; rationale: string }
	| { type: "search_batch_decided"; batchId: string; candidateId: string; decisionRef: string }
	| { type: "search_candidate_pruned"; receipt: DiscardedCandidateReceipt }
	| { type: "evidence_pruned"; receipt: DiscardedEvidenceReceipt }
	| { type: "route_decided"; decision: StageRouteDecision }
	| {
			type: "stage_reopened";
			targetStageId: string;
			affectedStageIds: string[];
			decisionRef: string;
			reason: string;
			objection: ResearchNode;
			cleanups?: CleanupIntent[];
	  };

export interface StoredEvent {
	seq: number;
	timestamp: string;
	jobId: string;
	event: AstraEvent;
}

export type GitVersion =
	| { status: "unavailable"; reason: string }
	| {
			status: "captured";
			root: string;
			head: string;
			branch: string;
			dirty: boolean;
			patchSha256: string;
			indexPatchSha256: string;
			untracked: Array<{ path: string; sha256: string; mode: number }>;
	  };

export interface TaskVersion {
	hash: string;
	contractHash: string;
	inputs: Array<{ ref: string; checksum: string }>;
	git: GitVersion;
	runtime: { node: string; platform: string; arch: string };
}

export interface EvidenceFileVersion {
	sourceRef: string;
	sha256: string;
}
