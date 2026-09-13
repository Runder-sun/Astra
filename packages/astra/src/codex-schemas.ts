import { Type } from "typebox";

const text = Type.String({ minLength: 1 });
const texts = Type.Array(text);
const score = Type.Number({ minimum: 0, maximum: 1 });

export const codexWorkerSchema = (minSourceRefs = 0) =>
	Type.Object(
		{
			artifactType: text,
			contentJson: Type.String({ description: "JSON-encoded object containing every required artifact field" }),
			refs: Type.Array(
				Type.Object(
					{
						kind: Type.Union([Type.Literal("artifact"), Type.Literal("log"), Type.Literal("source")]),
						ref: text,
						summary: text,
					},
					{ additionalProperties: false },
				),
				{ minItems: minSourceRefs },
			),
		},
		{ additionalProperties: false },
	);

export const codexReviewSchema = (criteria: readonly string[], evidenceRefs: readonly string[]) =>
	Type.Object(
		{
			verdict: Type.Union([
				Type.Literal("pass"),
				Type.Literal("fail"),
				Type.Literal("partial"),
				Type.Literal("blocked"),
			]),
			findings: texts,
			score,
			criteria: Type.Array(
				Type.Object(
					{
						criterion: Type.Union(criteria.map((criterion) => Type.Literal(criterion))),
						passed: Type.Boolean(),
						score,
						evidenceRefs: Type.Array(Type.Union(evidenceRefs.map((ref) => Type.Literal(ref))), { minItems: 1 }),
						rationale: text,
					},
					{ additionalProperties: false },
				),
				{ minItems: criteria.length, maxItems: criteria.length },
			),
			verifiedRefs: Type.Array(Type.Union(evidenceRefs.map((ref) => Type.Literal(ref))), { minItems: 1 }),
		},
		{ additionalProperties: false },
	);

export const codexPlanSchema = Type.Object(
	{
		tasks: Type.Array(
			Type.Object(
				{
					key: Type.String({ pattern: "^[A-Za-z0-9][A-Za-z0-9_-]*$" }),
					objective: text,
					inputArtifactRefs: texts,
					requiredOutputFields: Type.Array(text, { minItems: 1 }),
					acceptanceChecks: Type.Array(text, { minItems: 1 }),
					failureSignals: Type.Array(text, { minItems: 1 }),
					successCriteria: Type.Array(text, { minItems: 1 }),
					hypothesis: text,
				},
				{ additionalProperties: false },
			),
			{ minItems: 1, maxItems: 4 },
		),
		rationale: text,
	},
	{ additionalProperties: false },
);

export const codexEvidenceDecisionSchema = Type.Object(
	{
		decision: Type.Union([Type.Literal("accept"), Type.Literal("reject"), Type.Literal("defer")]),
		rationale: text,
	},
	{ additionalProperties: false },
);

export const codexAdoptionSchema = Type.Object(
	{
		adopt: Type.Boolean(),
		rationale: text,
	},
	{ additionalProperties: false },
);

export const codexSearchSchema = Type.Object(
	{
		selectedCandidateId: Type.Union([text, Type.Null()]),
		continueSearch: Type.Boolean(),
		rationale: text,
	},
	{ additionalProperties: false },
);

export const codexRouteSchema = Type.Object(
	{
		routeAction: Type.Union(
			(["continue", "search", "advance", "backtrack", "ask-user", "complete"] as const).map((value) =>
				Type.Literal(value),
			),
		),
		targetStageId: Type.Union([text, Type.Null()]),
		evidenceRefs: texts,
		question: Type.Union([text, Type.Null()]),
		newQuestions: texts,
		rationale: text,
	},
	{ additionalProperties: false },
);
