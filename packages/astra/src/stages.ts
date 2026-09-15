import type { StageDefinition } from "./types.ts";

const READ_TOOLS = ["read", "grep", "find", "ls"];
const LITERATURE_TOOLS = [...READ_TOOLS, "astra_search_papers"];
const RESEARCH_REVIEW_TOOLS = [...LITERATURE_TOOLS, "bash"];
const WORKSPACE_TOOLS = [...READ_TOOLS, "write", "edit", "bash"];
const DEFAULT_WORKER_BUDGET = { maxTurns: 12, maxToolCalls: 24, maxRuntimeMs: 600_000 };

interface StageTemplate {
	id: string;
	label: string;
	suggestedInputs?: string[];
	fields: string[];
	checks: string[];
	failures: string[];
	tools: string[];
	workspaceWrite?: boolean;
	workerBudget?: StageDefinition["workerBudget"];
	reviewerBudget?: StageDefinition["reviewerBudget"];
	minSourceRefs?: number;
	gate?: StageDefinition["gate"];
	searchPolicy?: StageDefinition["searchPolicy"];
}

const STAGE_TEMPLATES: StageTemplate[] = [
	{
		id: "validation",
		label: "Validate research question",
		fields: ["researchQuestion", "scope", "nonGoals", "acceptanceCriteria", "falsifiableNextStep"],
		checks: ["research question is bounded and falsifiable", "acceptance criteria are measurable"],
		failures: ["objective remains ambiguous", "acceptance criteria are not measurable"],
		tools: READ_TOOLS,
	},
	{
		id: "literature",
		label: "Survey relevant literature",
		suggestedInputs: ["validation"],
		fields: ["queryStrategy", "sources", "closestWork", "gaps", "synthesis"],
		checks: ["primary sources have stable identifiers", "closest work and unresolved gaps are explicit"],
		failures: ["source identifiers are missing", "claims are not traceable to retrieved records"],
		tools: LITERATURE_TOOLS,
		minSourceRefs: 3,
	},
	{
		id: "idea",
		label: "Generate candidate ideas",
		suggestedInputs: ["validation", "literature"],
		fields: ["candidates", "mechanisms", "expectedContributions", "risks", "selection"],
		checks: ["candidate ideas address the validated gap", "selection names a testable mechanism"],
		failures: ["ideas are generic restatements", "selected idea has no falsifiable mechanism"],
		tools: READ_TOOLS,
		searchPolicy: {
			strategy: "diverse-candidates",
			minCandidates: 3,
			maxCandidates: 4,
			criteria: ["gap fit", "novel mechanism", "falsifiability", "implementation cost"],
		},
	},
	{
		id: "novelty",
		label: "Check novelty",
		suggestedInputs: ["literature", "idea"],
		fields: ["queries", "closestPriorWork", "overlap", "differentiators", "verdict"],
		checks: ["novelty is compared against primary sources", "differentiators are concrete and falsifiable"],
		failures: ["closest prior work is missing", "novelty relies on unsupported assertions"],
		tools: LITERATURE_TOOLS,
		minSourceRefs: 3,
	},
	{
		id: "refine",
		label: "Refine the method",
		suggestedInputs: ["validation", "idea", "novelty"],
		fields: ["problem", "method", "components", "assumptions", "pseudocode", "failureModes"],
		checks: ["method directly addresses the selected problem", "components and assumptions are implementation-ready"],
		failures: ["method is overbuilt or underspecified", "critical assumptions are hidden"],
		tools: READ_TOOLS,
		searchPolicy: {
			strategy: "best-first",
			minCandidates: 2,
			maxCandidates: 4,
			criteria: ["problem alignment", "simplicity", "technical feasibility", "failure robustness"],
		},
	},
	{
		id: "experiment-plan",
		label: "Plan experiments",
		suggestedInputs: ["refine", "novelty"],
		fields: [
			"claims",
			"datasets",
			"baselines",
			"metrics",
			"ablations",
			"runOrder",
			"computeBudget",
			"stoppingCriteria",
		],
		checks: ["every intended claim maps to evidence", "run order has cheap failure gates"],
		failures: ["claims lack supporting experiments", "baselines or stopping criteria are missing"],
		tools: READ_TOOLS,
		searchPolicy: {
			strategy: "diverse-candidates",
			minCandidates: 2,
			maxCandidates: 4,
			criteria: ["claim coverage", "baseline strength", "failure detection", "compute efficiency"],
		},
	},
	{
		id: "implement-solution",
		label: "Implement the solution",
		suggestedInputs: ["refine", "experiment-plan"],
		fields: ["implementation", "files", "tests", "commands", "limitations"],
		checks: [
			"implementation and required runtime resources are reusable from the task resource root",
			"behavioral tests cover the core method",
		],
		failures: ["implementation is prose-only", "runtime resource or test evidence is missing"],
		tools: WORKSPACE_TOOLS,
		workspaceWrite: true,
		workerBudget: { maxTurns: 40, maxToolCalls: 96, maxRuntimeMs: 3_600_000 },
	},
	{
		id: "run",
		label: "Run experiments",
		suggestedInputs: ["implement-solution", "experiment-plan"],
		fields: ["commands", "runs", "metrics", "logs", "failures"],
		checks: ["reported metrics are backed by run logs", "failed runs remain visible"],
		failures: ["metrics are invented", "commands or logs are missing"],
		tools: WORKSPACE_TOOLS,
		workspaceWrite: true,
		workerBudget: { maxTurns: 24, maxToolCalls: 64, maxRuntimeMs: 3_600_000 },
	},
	{
		id: "monitor",
		label: "Monitor experiments",
		suggestedInputs: ["run", "experiment-plan"],
		fields: ["runStatus", "metrics", "anomalies", "integrityChecks", "decisions"],
		checks: ["health conclusions cite current logs", "anomalies have explicit actions"],
		failures: ["run status is stale", "integrity failures are ignored"],
		tools: WORKSPACE_TOOLS,
		workspaceWrite: true,
		workerBudget: { maxTurns: 16, maxToolCalls: 48, maxRuntimeMs: 1_800_000 },
	},
	{
		id: "result-to-claim",
		label: "Map results to claims",
		suggestedInputs: ["run", "monitor", "experiment-plan"],
		fields: [
			"scientificOutcome",
			"missionCoverage",
			"claims",
			"supportingResults",
			"unsupportedClaims",
			"missingEvidence",
			"conclusion",
		],
		checks: [
			"each claim names supporting result evidence",
			"unsupported claims are explicitly withheld",
			"scientific outcome and mission coverage match the primary research objective",
		],
		failures: ["claims exceed evidence", "negative or inconclusive results are omitted"],
		tools: READ_TOOLS,
	},
	{
		id: "paper-plan",
		label: "Plan the paper",
		suggestedInputs: ["result-to-claim"],
		fields: ["narrative", "outline", "claimMap", "figurePlan", "citationPlan"],
		checks: ["paper structure follows supported claims", "figures and citations map to evidence"],
		failures: ["outline includes unsupported claims", "evidence-to-section mapping is missing"],
		tools: READ_TOOLS,
	},
	{
		id: "paper-write",
		label: "Write the paper",
		suggestedInputs: ["paper-plan", "result-to-claim", "literature"],
		fields: ["manuscript", "sections", "citations", "claimBindings", "limitations"],
		checks: ["manuscript claims remain evidence-bound", "citations have stable source refs"],
		failures: ["citations are fabricated", "limitations or negative results are hidden"],
		tools: [...LITERATURE_TOOLS, "write", "edit"],
		workspaceWrite: true,
		workerBudget: { maxTurns: 20, maxToolCalls: 48, maxRuntimeMs: 900_000 },
		minSourceRefs: 3,
	},
	{
		id: "paper-compile",
		label: "Compile the paper",
		suggestedInputs: ["paper-write"],
		fields: ["artifact", "command", "buildLog", "source", "buildInputs", "validation", "remainingWarnings"],
		checks: [
			"compiled artifact exists",
			"build log and validation are retained",
			"editable compilation sources and required local inputs are retained",
			"page layout is checked and no text or tables are clipped",
		],
		failures: [
			"compiled artifact is missing",
			"build errors are suppressed",
			"editable sources are missing",
			"page content is clipped",
		],
		tools: WORKSPACE_TOOLS,
		workspaceWrite: true,
		workerBudget: { maxTurns: 16, maxToolCalls: 48, maxRuntimeMs: 900_000 },
	},
	{
		id: "research-review",
		label: "Review the research",
		suggestedInputs: ["result-to-claim", "literature", "novelty", "experiment-plan"],
		fields: [
			"verdict",
			"scientificOutcome",
			"missionCoverage",
			"strengths",
			"weaknesses",
			"claimAudit",
			"requiredRepairs",
		],
		checks: [
			"review checks the full claim-evidence chain",
			"blocking weaknesses become explicit repairs",
			"review independently confirms the scientific outcome and mission coverage",
		],
		failures: ["review only summarizes", "claim or citation audit is missing"],
		tools: RESEARCH_REVIEW_TOOLS,
		workerBudget: { maxTurns: 16, maxToolCalls: 48, maxRuntimeMs: 900_000 },
		minSourceRefs: 3,
		gate: "user",
	},
];

export const DEFAULT_STAGES: StageDefinition[] = STAGE_TEMPLATES.map((stage) => ({
	id: stage.id,
	label: stage.label,
	suggestedInputArtifactTypes: stage.suggestedInputs ?? [],
	outputArtifactType: stage.id,
	requiredOutputFields: stage.fields,
	acceptanceChecks: stage.checks,
	failureSignals: stage.failures,
	workerTaskFamily: stage.id,
	workerTools: stage.tools,
	workspaceWrite: stage.workspaceWrite ?? false,
	workerBudget: stage.workerBudget ?? DEFAULT_WORKER_BUDGET,
	reviewerBudget: stage.reviewerBudget ?? { maxTurns: 10, maxToolCalls: 32, maxRuntimeMs: 600_000 },
	minSourceRefs: stage.minSourceRefs ?? 0,
	gate: stage.gate ?? "main-agent",
	searchPolicy: stage.searchPolicy,
	qualityPolicy: {
		minPassingReviews: ["result-to-claim", "research-review"].includes(stage.id) ? 2 : 1,
		minScore: 0.8,
		requireResolvableArtifacts: true,
	},
}));

export function stageMap(definitions: StageDefinition[] = DEFAULT_STAGES): Record<string, StageDefinition> {
	return Object.fromEntries(definitions.map((definition) => [definition.id, definition]));
}
