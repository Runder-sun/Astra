import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fauxAssistantMessage, fauxProvider, fauxToolCall } from "@earendil-works/pi-ai/providers/faux";
import type { ExtensionFactory } from "@earendil-works/pi-coding-agent";

export async function fetchAstraFixtureOpenAlex(_input: string | URL, _init?: RequestInit): Promise<Response> {
	return new Response(
		JSON.stringify({
			meta: { count: 3 },
			results: [1, 2, 3].map((index) => ({
				id: `https://openalex.org/W${index}`,
				doi: `https://doi.org/10.0000/astra-fixture-${index}`,
				title: `Astra fixture paper ${index}`,
				publication_year: 2026,
				type: "article",
				cited_by_count: index,
				authorships: [{ author: { display_name: `Fixture Author ${index}` } }],
				primary_location: { landing_page_url: `https://openalex.org/W${index}` },
			})),
		}),
		{ status: 200, headers: { "Content-Type": "application/json" } },
	);
}

/** Deterministic provider used only for offline, auditable child-session tests. */
export function createAstraFixtureProvider(): ExtensionFactory {
	return (pi): void => {
		const fixture = fauxProvider({
			provider: "astra-fixture",
			models: [{ id: "astra-fixture-1", name: "Astra Fixture", reasoning: false }],
		});
		fixture.setResponses([
			(context) => {
				const lastMessage = context.messages.at(-1);
				if (lastMessage?.role !== "user")
					return fauxAssistantMessage("Astra fixture completed the requested operation.");
				const role = process.env.ASTRA_ROLE;
				const tools = new Set((context.tools ?? []).map((tool) => tool.name));
				if (role === "worker" && tools.has("astra_submit_worker_output")) {
					const requiredFields = JSON.parse(process.env.ASTRA_REQUIRED_OUTPUT_FIELDS ?? '["content"]') as string[];
					const content: Record<string, unknown> = Object.fromEntries(
						requiredFields.map((field) => [
							field,
							{ fixture: true, taskId: process.env.ASTRA_TASK_ID ?? "unknown" },
						]),
					);
					if (process.env.ASTRA_STAGE_ID === "result-to-claim") {
						content.scientificOutcome = "supported";
						content.missionCoverage = "sufficient";
						content.claims = [
							{
								statement: "The fixture method is supported by the audited experiment result",
								assessment: "supported",
							},
						];
						content.supportingResults = ["fixture deterministic metric"];
						content.unsupportedClaims = [];
						content.missingEvidence = [];
						content.conclusion = "supported";
					}
					if (process.env.ASTRA_STAGE_ID === "research-review") {
						content.verdict = "pass";
						content.scientificOutcome = "supported";
						content.missionCoverage = "sufficient";
						content.strengths = ["claim and evidence are traceable"];
						content.weaknesses = [];
						content.claimAudit = ["fixture claim is supported"];
						content.requiredRepairs = [];
					}
					if (process.env.ASTRA_STAGE_ID === "paper-write") {
						writeFileSync(
							join(process.cwd(), "paper-manuscript.md"),
							"# Astra Fixture Paper\n\nThis deterministic manuscript verifies the paper delivery contract.\n",
							"utf8",
						);
						content.manuscript = "paper-manuscript.md";
					}
					if (process.env.ASTRA_STAGE_ID === "paper-compile") {
						writeFileSync(join(process.cwd(), "paper.pdf"), "%PDF-1.4\n% Astra fixture\n", "utf8");
						writeFileSync(join(process.cwd(), "paper-build.log"), "fixture paper build passed\n", "utf8");
						content.artifact = "paper.pdf";
						content.buildLog = "paper-build.log";
					}
					const sourceStage = ["literature", "novelty", "paper-write", "research-review"].includes(
						process.env.ASTRA_STAGE_ID ?? "",
					);
					const sourceRefs = [1, 2, 3].map((index) => ({
						kind: "source",
						ref: `openalex:W${index}`,
						summary: `deterministic fixture source ${index}`,
					}));
					const outputRefs =
						process.env.ASTRA_STAGE_ID === "paper-write"
							? [
									...sourceRefs,
									{
										kind: "artifact",
										ref: "paper-manuscript.md",
										summary: "deterministic fixture manuscript",
									},
								]
							: process.env.ASTRA_STAGE_ID === "paper-compile"
								? [
										{ kind: "artifact", ref: "paper.pdf", summary: "compiled fixture paper" },
										{ kind: "log", ref: "paper-build.log", summary: "fixture paper build log" },
									]
								: sourceStage
									? sourceRefs
									: [
											{
												kind: "session",
												ref: `pi-session:${process.env.ASTRA_SESSION_ID ?? "fixture"}`,
												summary: "deterministic fixture session",
											},
										];
					const outputCall = fauxToolCall(
						"astra_submit_worker_output",
						{
							artifactType: process.env.ASTRA_REQUIRED_OUTPUT_TYPE ?? "research_output",
							content,
							refs: outputRefs,
						},
						{ id: "fixture-worker-output" },
					);
					return fauxAssistantMessage(
						sourceStage && tools.has("astra_search_papers")
							? [
									fauxToolCall(
										"astra_search_papers",
										{ query: `Astra fixture ${process.env.ASTRA_STAGE_ID}`, limit: 3 },
										{ id: "fixture-paper-search" },
									),
									outputCall,
								]
							: outputCall,
					);
				}
				if (role === "reviewer" && tools.has("astra_submit_review")) {
					const jobId = process.env.ASTRA_JOB_ID ?? "unknown-job";
					const failureMarker = join(
						process.env.ASTRA_PROJECT_ROOT ?? process.cwd(),
						".astra",
						"jobs",
						jobId,
						"fixture-review-failure-used",
					);
					const firstReviewFailure = !existsSync(failureMarker);
					if (firstReviewFailure) writeFileSync(failureMarker, `${new Date().toISOString()}\n`, "utf8");
					return fauxAssistantMessage(
						fauxToolCall(
							"astra_submit_review",
							{
								evidenceId: process.env.ASTRA_EVIDENCE_ID ?? "unknown-evidence",
								verdict: firstReviewFailure ? "fail" : "pass",
								findings: firstReviewFailure
									? ["fixture review failure requiring repair"]
									: ["fixture review passed"],
								score: firstReviewFailure ? 0.2 : 1,
								criteria: (
									JSON.parse(
										process.env.ASTRA_REVIEW_CRITERIA ?? '["fixture contract is satisfied"]',
									) as string[]
								).map((criterion) => ({
									criterion,
									passed: !firstReviewFailure,
									score: firstReviewFailure ? 0.2 : 1,
									evidenceRefs: ["review-target-snapshot.json"],
									rationale: firstReviewFailure ? "fixture repair required" : "fixture evidence verified",
								})),
								verifiedRefs: ["review-target-snapshot.json"],
							},
							{ id: "fixture-review" },
						),
					);
				}
				if (role === "main-agent" && process.env.ASTRA_STAGE_PLAN_ID && tools.has("astra_submit_stage_plan")) {
					const requiredOutputFields = JSON.parse(
						process.env.ASTRA_STAGE_REQUIRED_FIELDS ?? '["content"]',
					) as string[];
					const inputArtifactRefs = JSON.parse(process.env.ASTRA_AVAILABLE_INPUT_REFS ?? "[]") as string[];
					const mode = process.env.ASTRA_PLAN_MODE ?? (process.env.ASTRA_OBLIGATION_ID ? "repair" : "decompose");
					const taskCount = mode === "search" ? Number(process.env.ASTRA_SEARCH_MIN_CANDIDATES ?? "2") : 1;
					const searchRound = process.env.ASTRA_SEARCH_PREVIOUS_BATCH_ID ? 2 : 1;
					return fauxAssistantMessage(
						fauxToolCall(
							"astra_submit_stage_plan",
							{
								mode,
								tasks: Array.from({ length: taskCount }, (_, index) => ({
									key: process.env.ASTRA_OBLIGATION_ID
										? "repair"
										: `round-${searchRound}-primary-${index + 1}`,
									objective: process.env.ASTRA_OBLIGATION_ID
										? `Repair ${process.env.ASTRA_STAGE_ID} evidence for ${process.env.ASTRA_OBLIGATION_ID}`
										: `Produce traceable ${process.env.ASTRA_STAGE_ID} evidence round ${searchRound} pass ${index + 1}`,
									hypothesis:
										mode === "search"
											? `${process.env.ASTRA_STAGE_ID} round ${searchRound} candidate ${index + 1} optimizes discriminator ${searchRound}-${index + 1}`
											: undefined,
									inputArtifactRefs,
									requiredOutputFields,
									acceptanceChecks: [`${process.env.ASTRA_STAGE_ID} output is structured and traceable`],
									failureSignals: ["missing required output field or evidence ref"],
									successCriteria: [`${process.env.ASTRA_STAGE_ID} contract is satisfied`],
								})),
								rationale: process.env.ASTRA_OBLIGATION_ID
									? "Fixture repair plan addresses the open review obligation"
									: "Fixture plan creates a bounded parallel evidence wave",
							},
							{ id: "fixture-stage-plan" },
						),
					);
				}
				if (role === "main-agent" && tools.has("astra_submit_main_decision")) {
					const decisionType = process.env.ASTRA_DECISION_TYPE ?? "evidence";
					const routeTargets: Record<string, string> = {
						validation: "literature",
						literature: "idea",
						idea: "novelty",
						novelty: "refine",
						refine: "experiment-plan",
						"experiment-plan": "implement-solution",
						"implement-solution": "run",
						run: "result-to-claim",
						"result-to-claim": "research-review",
					};
					const stageId = process.env.ASTRA_STAGE_ID ?? "validation";
					const completionBlockers = JSON.parse(process.env.ASTRA_COMPLETION_BLOCKERS ?? "[]") as string[];
					const paperRequired = completionBlockers.some((blocker) =>
						/^required canonical artifact paper-(?:write|compile) is missing$/.test(blocker),
					);
					const targetStageId =
						stageId === "result-to-claim" && paperRequired
							? "paper-plan"
							: stageId === "paper-plan"
								? "paper-write"
								: stageId === "paper-write"
									? "paper-compile"
									: stageId === "paper-compile"
										? "research-review"
										: routeTargets[stageId];
					const searchContinuationMarker = join(
						process.env.ASTRA_PROJECT_ROOT ?? process.cwd(),
						".astra",
						"jobs",
						process.env.ASTRA_JOB_ID ?? "unknown-job",
						"fixture-search-continuation-used",
					);
					const continueFixtureSearch =
						decisionType === "search-selection" &&
						process.env.ASTRA_FIXTURE_CONTINUE_SEARCH_ONCE === "1" &&
						process.env.ASTRA_SEARCH_ALLOW_CONTINUE === "1" &&
						!existsSync(searchContinuationMarker);
					if (continueFixtureSearch)
						writeFileSync(searchContinuationMarker, `${new Date().toISOString()}\n`, "utf8");
					const routeAction =
						stageId === "research-review"
							? completionBlockers.length === 0
								? "complete"
								: "backtrack"
							: "advance";
					return fauxAssistantMessage(
						fauxToolCall(
							"astra_submit_main_decision",
							decisionType === "evidence"
								? {
										decisionType,
										decisionRef: process.env.ASTRA_DECISION_REF ?? `fixture-${Date.now()}`,
										evidenceId: process.env.ASTRA_EVIDENCE_ID,
										decision: "accept",
										rationale: "fixture acceptance",
									}
								: decisionType === "adoption"
									? {
											decisionType,
											decisionRef: process.env.ASTRA_DECISION_REF ?? `fixture-${Date.now()}`,
											evidenceId: process.env.ASTRA_EVIDENCE_ID,
											adopt: true,
											rationale: "fixture adoption",
										}
									: decisionType === "search-selection"
										? continueFixtureSearch
											? {
													decisionType,
													decisionRef: process.env.ASTRA_DECISION_REF ?? `fixture-${Date.now()}`,
													searchBatchId: process.env.ASTRA_SEARCH_BATCH_ID,
													continueSearch: true,
													rationale: "fixture requested one orthogonal search round",
												}
											: {
													decisionType,
													decisionRef: process.env.ASTRA_DECISION_REF ?? `fixture-${Date.now()}`,
													searchBatchId: process.env.ASTRA_SEARCH_BATCH_ID,
													selectedCandidateId: (
														JSON.parse(process.env.ASTRA_CANDIDATE_IDS ?? "[]") as string[]
													)[0],
													rationale: "fixture selected the highest-scoring reviewed candidate",
												}
										: {
												decisionType,
												decisionRef: process.env.ASTRA_DECISION_REF ?? `fixture-${Date.now()}`,
												stageId,
												routeAction,
												targetStageId: routeAction === "backtrack" ? "result-to-claim" : targetStageId,
												evidenceRefs: JSON.parse(process.env.ASTRA_ROUTE_EVIDENCE_REFS ?? "[]"),
												newQuestions: [],
												rationale:
													routeAction === "complete"
														? "fixture quality gates are clear"
														: routeAction === "backtrack"
															? `fixture completion is blocked: ${completionBlockers.join("; ")}`
															: `fixture selected ${targetStageId} as the next capability`,
											},
							{ id: `fixture-${decisionType}` },
						),
					);
				}
				return fauxAssistantMessage("Astra fixture has no operation to perform.");
			},
			...Array.from({ length: 8 }, () => fauxAssistantMessage("Astra fixture completed the requested operation.")),
		]);
		pi.registerProvider(fixture.provider);
	};
}
