import type { CandidateEvaluation, JobSnapshot, ResearchNode, SearchBatch, StageRouteDecision } from "./types.ts";

export interface ResearchBoard {
	jobId: string;
	objective: string;
	status: JobSnapshot["frame"]["status"];
	activeStageId: string;
	questions: ResearchNode[];
	hypotheses: ResearchNode[];
	claims: ResearchNode[];
	objections: ResearchNode[];
	decisions: ResearchNode[];
	userGuidance: ResearchNode[];
	searchBatches: SearchBatch[];
	candidateEvaluations: CandidateEvaluation[];
	routeDecisions: StageRouteDecision[];
	canonicalArtifactIds: string[];
	budget: { tasks: string; turns: string; costUsd: string };
	scientificOutcome: JobSnapshot["frame"]["scientificOutcome"];
	missionCoverage: JobSnapshot["frame"]["missionCoverage"];
	requiredArtifactTypes: string[];
	missingRequiredArtifactTypes: string[];
	nextAction: string;
}

export function buildResearchBoard(snapshot: JobSnapshot): ResearchBoard {
	const nodes = Object.values(snapshot.graph.nodes);
	return {
		jobId: snapshot.frame.jobId,
		objective: snapshot.frame.objective,
		status: snapshot.frame.status,
		activeStageId: snapshot.frame.activeStageId,
		questions: nodes.filter((node) => node.kind === "question" && node.status === "open"),
		hypotheses: nodes.filter((node) => node.kind === "hypothesis" && node.status === "active"),
		claims: nodes.filter((node) => node.kind === "claim" && node.status !== "superseded"),
		objections: nodes.filter((node) => node.kind === "objection" && node.status === "open"),
		decisions: nodes.filter((node) => node.kind === "decision" && node.status !== "superseded"),
		userGuidance: nodes.filter((node) => node.actor === "user"),
		searchBatches: Object.values(snapshot.searchBatches).filter((batch) => batch.status !== "superseded"),
		candidateEvaluations: Object.values(snapshot.candidateEvaluations),
		routeDecisions: Object.values(snapshot.routeDecisions).sort((left, right) =>
			left.createdAt.localeCompare(right.createdAt),
		),
		canonicalArtifactIds: Object.values(snapshot.canonicalRoute.stageArtifactIds),
		budget: {
			tasks: `${Object.keys(snapshot.tasks).length}/${snapshot.frame.budget.maxTasks}`,
			turns: `${snapshot.budgetUsage?.turnsUsed ?? 0}/${snapshot.frame.budget.maxTurns}`,
			costUsd: `${(snapshot.budgetUsage?.costUsdUsed ?? 0).toFixed(4)}${snapshot.frame.budget.maxCostUsd === undefined ? "" : `/${snapshot.frame.budget.maxCostUsd.toFixed(4)}`}`,
		},
		scientificOutcome: snapshot.frame.scientificOutcome,
		missionCoverage: snapshot.frame.missionCoverage,
		requiredArtifactTypes: [...snapshot.frame.requiredArtifactTypes],
		missingRequiredArtifactTypes: snapshot.frame.requiredArtifactTypes.filter(
			(type) =>
				!Object.values(snapshot.canonical).some(
					(artifact) => artifact.status === "active" && artifact.type === type,
				),
		),
		nextAction: snapshot.frame.nextAction,
	};
}

function section(label: string, nodes: ResearchNode[]): string[] {
	return [label, ...(nodes.length > 0 ? nodes.map((node) => `  ${node.id}: ${node.statement}`) : ["  none"])];
}

export function formatResearchBoard(board: ResearchBoard): string {
	const searches = board.searchBatches.flatMap((batch) => [
		`  ${batch.id} [${batch.status} round ${batch.round}/${batch.maxRounds}] ${batch.objective}`,
		...Object.values(batch.candidates).map((candidate) => {
			const evaluation = board.candidateEvaluations.find((value) => value.candidateId === candidate.id);
			return `    ${candidate.id} [${candidate.status}]${evaluation ? ` score=${evaluation.score.toFixed(2)}` : ""} ${candidate.hypothesis}`;
		}),
	]);
	return [
		`Astra Research Board: ${board.jobId}`,
		`Objective: ${board.objective}`,
		`State: ${board.status}; active capability: ${board.activeStageId}`,
		`Scientific result: ${board.scientificOutcome}; mission coverage: ${board.missionCoverage}`,
		`Budget: ${board.budget.tasks} tasks; ${board.budget.turns} turns; $${board.budget.costUsd}`,
		`Required artifacts: ${board.requiredArtifactTypes.length > 0 ? board.requiredArtifactTypes.join(", ") : "none"}; missing: ${board.missingRequiredArtifactTypes.length > 0 ? board.missingRequiredArtifactTypes.join(", ") : "none"}`,
		`Canonical route: ${board.canonicalArtifactIds.length} artifacts; ${board.routeDecisions.length} decisions`,
		...section("Open questions", board.questions),
		...section("Active hypotheses", board.hypotheses),
		"Research claims",
		...(board.claims.length > 0
			? board.claims.map((node) => `  ${node.id} [${node.claimAssessment ?? node.status}] ${node.statement}`)
			: ["  none"]),
		...section("Blocking objections", board.objections),
		...section("User guidance", board.userGuidance),
		"Search candidates",
		...(searches.length > 0 ? searches : ["  none"]),
		`Next: ${board.nextAction}`,
	].join("\n");
}
