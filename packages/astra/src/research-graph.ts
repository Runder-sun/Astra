import { createHash, randomUUID } from "node:crypto";
import type { ResearchEdge, ResearchEdgeKind, ResearchGraph, ResearchNode, ResearchNodeKind } from "./types.ts";

function stableId(prefix: string, value: unknown): string {
	return `${prefix}_${createHash("sha256").update(JSON.stringify(value)).digest("hex").slice(0, 24)}`;
}

export function createResearchNode(input: {
	kind: ResearchNodeKind;
	statement: string;
	status?: ResearchNode["status"];
	stageId?: string;
	domainRef?: string;
	sourceRefs?: string[];
	id?: string;
}): ResearchNode {
	const now = new Date().toISOString();
	return {
		id: input.id ?? `research_${input.kind}_${randomUUID()}`,
		kind: input.kind,
		statement: input.statement,
		status: input.status ?? (input.kind === "question" || input.kind === "objection" ? "open" : "active"),
		sourceRefs: input.sourceRefs ?? [],
		createdAt: now,
		updatedAt: now,
		...(input.stageId ? { stageId: input.stageId } : {}),
		...(input.domainRef ? { domainRef: input.domainRef } : {}),
	};
}

export function createResearchEdge(input: {
	fromNodeId: string;
	toNodeId: string;
	kind: ResearchEdgeKind;
	sourceRefs?: string[];
}): ResearchEdge {
	return {
		id: stableId("research_edge", input),
		fromNodeId: input.fromNodeId,
		toNodeId: input.toNodeId,
		kind: input.kind,
		sourceRefs: input.sourceRefs ?? [],
		createdAt: new Date().toISOString(),
	};
}

export function createResearchGraph(jobId: string, objective: string): ResearchGraph {
	const root = createResearchNode({
		id: stableId("research_question", { jobId, objective }),
		kind: "question",
		statement: objective,
		status: "open",
		sourceRefs: [`job:${jobId}`],
	});
	return {
		version: 1,
		nodes: { [root.id]: root },
		edges: {},
		rootQuestionId: root.id,
		openQuestionIds: [root.id],
		activeHypothesisIds: [],
		acceptedClaimIds: [],
		unresolvedObjectionIds: [],
		revision: 1,
	};
}

export function addNodeToGraph(graph: ResearchGraph, node: ResearchNode): void {
	graph.nodes[node.id] = structuredClone(node);
	if (node.kind === "question" && node.status === "open" && !graph.openQuestionIds.includes(node.id)) {
		graph.openQuestionIds.push(node.id);
	}
	if (node.kind === "hypothesis" && node.status === "active" && !graph.activeHypothesisIds.includes(node.id)) {
		graph.activeHypothesisIds.push(node.id);
	}
	if (node.kind === "claim" && node.status === "accepted" && !graph.acceptedClaimIds.includes(node.id)) {
		graph.acceptedClaimIds.push(node.id);
	}
	if (node.kind === "objection" && node.status === "open" && !graph.unresolvedObjectionIds.includes(node.id)) {
		graph.unresolvedObjectionIds.push(node.id);
	}
	graph.revision += 1;
}

export function addEdgeToGraph(graph: ResearchGraph, edge: ResearchEdge): void {
	graph.edges[edge.id] = structuredClone(edge);
	graph.revision += 1;
}

export function updateNodeStatus(
	graph: ResearchGraph,
	nodeId: string,
	status: ResearchNode["status"],
	timestamp = new Date().toISOString(),
): void {
	const node = graph.nodes[nodeId];
	if (!node) throw new Error(`unknown research node ${nodeId}`);
	node.status = status;
	node.updatedAt = timestamp;
	graph.openQuestionIds = graph.openQuestionIds.filter((id) => id !== nodeId || status === "open");
	graph.activeHypothesisIds = graph.activeHypothesisIds.filter((id) => id !== nodeId || status === "active");
	graph.acceptedClaimIds = graph.acceptedClaimIds.filter((id) => id !== nodeId || status === "accepted");
	graph.unresolvedObjectionIds = graph.unresolvedObjectionIds.filter((id) => id !== nodeId || status === "open");
	graph.revision += 1;
}
