use crate::memory::MemoryRecord;
use crate::skills::{SkillOutputArtifactInput, SkillOutputSubmitRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static EVOLUTION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeClusterRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub cluster_id: String,
    pub project_id: String,
    pub cluster_topic: String,
    pub centroid_method: String,
    pub similarity_threshold: f64,
    pub crystallization_threshold: usize,
    pub memory_ids: Vec<String>,
    pub memory_count: usize,
    pub support_refs: Vec<String>,
    pub cluster_status: String,
    pub supersedes: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillEvolutionCandidate {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub candidate_id: String,
    pub cluster_id: String,
    pub project_id: String,
    pub skill_id: String,
    pub title: String,
    pub category: String,
    pub trigger: String,
    pub skill_artifact_path: String,
    pub source_memory_ids: Vec<String>,
    pub support_refs: Vec<String>,
    pub verification_status: String,
    pub publication_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillProvenanceVerification {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub verification_id: String,
    pub candidate_id: String,
    pub grounded_claim_count: usize,
    pub hallucinated_claim_count: usize,
    pub contradicted_claim_count: usize,
    pub grounded_evidence_ratio: f64,
    pub decision: String,
    pub evidence_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillStructureScore {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub score_id: String,
    pub candidate_id: String,
    pub workflow_clarity: f64,
    pub specificity_and_reusability: f64,
    pub preserves_existing_value: f64,
    pub overall_score: f64,
    pub upgrade_decision: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillClusterResult {
    pub schema_version: String,
    pub conformance_line: String,
    pub clusters: Vec<KnowledgeClusterRecord>,
    pub total_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillCrystallizeResult {
    pub candidate: SkillEvolutionCandidate,
    pub evolution_record: SkillEvolutionEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillEvolutionEvent {
    pub schema_version: String,
    pub conformance_line: String,
    pub event_id: String,
    pub candidate_id: String,
    pub cluster_id: String,
    pub status: String,
    pub support_refs: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillVerifyResult {
    pub verification: SkillProvenanceVerification,
    pub structure_score: SkillStructureScore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillInstallRecord {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub install_id: String,
    pub candidate_id: String,
    pub skill_id: String,
    pub install_state: String,
    pub install_path: String,
    pub manifest_path: String,
    pub human_gate_approved: bool,
    pub support_refs: Vec<String>,
    pub created_at: String,
}

pub fn cluster(
    data_dir: &Path,
    project_id: &str,
    min_members: usize,
) -> Result<SkillClusterResult, EvolutionError> {
    let threshold = min_members.max(2);
    let memories = latest_active_memories(data_dir)?;
    let mut clusters = Vec::new();
    for cluster_memories in discover_memory_clusters(&memories, threshold) {
        let memory_ids = cluster_memories
            .iter()
            .map(|memory| memory.record_id.clone())
            .collect::<Vec<_>>();
        let support_refs = cluster_memories
            .iter()
            .flat_map(|memory| memory.support_refs.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let topic = cluster_topic(&cluster_memories);
        let cluster_id = format!("skill_cluster_{}", digest16(&memory_ids.join("|")));
        let now = timestamp_string();
        let path = cluster_path(data_dir, &cluster_id);
        let record = KnowledgeClusterRecord {
            schema_version: "skill.knowledge_cluster.v1".to_string(),
            conformance_line: "M16.knowledge_cluster".to_string(),
            canonical_path: path.display().to_string(),
            retention_policy: "project_local_until_cluster_superseded".to_string(),
            atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
            cluster_id: cluster_id.clone(),
            project_id: project_id.to_string(),
            cluster_topic: topic,
            centroid_method: "deterministic_token_overlap".to_string(),
            similarity_threshold: CLUSTER_SIMILARITY_THRESHOLD,
            crystallization_threshold: threshold,
            memory_ids,
            memory_count: cluster_memories.len(),
            support_refs,
            cluster_status: "ready_for_crystallization".to_string(),
            supersedes: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        };
        write_json_atomic(&path, &record)?;
        append_json_line(&clusters_log_path(data_dir), &record)?;
        clusters.push(record);
    }
    clusters.sort_by(|left, right| {
        left.cluster_topic
            .cmp(&right.cluster_topic)
            .then_with(|| left.cluster_id.cmp(&right.cluster_id))
    });
    Ok(SkillClusterResult {
        schema_version: "skill.cluster_result.v1".to_string(),
        conformance_line: "M16.skill_evolution_cluster".to_string(),
        total_count: clusters.len(),
        clusters,
    })
}

pub fn crystallize(
    data_dir: &Path,
    workspace_root: &Path,
    cluster_id: &str,
) -> Result<SkillCrystallizeResult, EvolutionError> {
    let cluster = load_cluster(data_dir, cluster_id)?;
    if cluster.cluster_status != "ready_for_crystallization" {
        return Err(EvolutionError::InvalidInput(format!(
            "cluster is not ready for crystallization: {cluster_id}"
        )));
    }
    let memories = memories_by_id(data_dir, &cluster.memory_ids)?;
    let now = timestamp_string();
    let candidate_id = format!(
        "skill_candidate_{}_{}",
        digest16(cluster_id),
        next_sequence()
    );
    let skill_id = format!("evolved.{}", slug(&cluster.cluster_topic));
    let relative_artifact = PathBuf::from("docs")
        .join("skill_candidates")
        .join(&candidate_id)
        .join("SKILL.md");
    let artifact_path = workspace_root.join(&relative_artifact);
    let title = format!("{} Workflow", title_case(&cluster.cluster_topic));
    let trigger = format!(
        "Use when a task matches the '{}' memory cluster and needs a repeatable workflow.",
        cluster.cluster_topic
    );
    let skill_markdown = render_skill_markdown(&title, &trigger, &cluster, &memories);
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&artifact_path, skill_markdown)?;

    let candidate_path = candidate_path(data_dir, &candidate_id);
    let candidate = SkillEvolutionCandidate {
        schema_version: "skill.evolution_candidate.v1".to_string(),
        conformance_line: "M16.skill_evolution_candidate".to_string(),
        canonical_path: candidate_path.display().to_string(),
        retention_policy: "project_local_until_candidate_superseded_or_rejected".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        candidate_id: candidate_id.clone(),
        cluster_id: cluster_id.to_string(),
        project_id: cluster.project_id.clone(),
        skill_id,
        title,
        category: "research_workflow".to_string(),
        trigger,
        skill_artifact_path: relative_artifact.to_string_lossy().to_string(),
        source_memory_ids: cluster.memory_ids.clone(),
        support_refs: cluster.support_refs.clone(),
        verification_status: "pending".to_string(),
        publication_status: "not_submitted".to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    write_json_atomic(&candidate_path, &candidate)?;
    append_json_line(&candidates_log_path(data_dir), &candidate)?;
    let event = SkillEvolutionEvent {
        schema_version: "skill.evolution_event.v1".to_string(),
        conformance_line: "M16.skill_evolution_event".to_string(),
        event_id: format!("skill_evolution_{}_{}", now, next_sequence()),
        candidate_id: candidate_id.clone(),
        cluster_id: cluster_id.to_string(),
        status: "crystallized".to_string(),
        support_refs: candidate.support_refs.clone(),
        created_at: now,
    };
    append_json_line(&events_log_path(data_dir), &event)?;
    Ok(SkillCrystallizeResult {
        candidate,
        evolution_record: event,
    })
}

pub fn verify(data_dir: &Path, candidate_id: &str) -> Result<SkillVerifyResult, EvolutionError> {
    let mut candidate = load_candidate(data_dir, candidate_id)?;
    let memories = memories_by_id(data_dir, &candidate.source_memory_ids)?;
    let workspace_root = data_dir.parent().unwrap_or(data_dir);
    let artifact_path =
        workspace_relative_existing_file(workspace_root, &candidate.skill_artifact_path)?;
    let artifact_text = fs::read_to_string(&artifact_path).unwrap_or_default();
    let grounded_claim_count = memories
        .iter()
        .filter(|memory| {
            artifact_text.contains(&memory.record_id)
                || memory
                    .support_refs
                    .iter()
                    .any(|support| !support.is_empty() && artifact_text.contains(support))
        })
        .count();
    let hallucinated_claim_count = candidate
        .source_memory_ids
        .len()
        .saturating_sub(grounded_claim_count);
    let contradicted_claim_count = memories
        .iter()
        .filter(|memory| memory.status == "invalidated" || memory.status == "superseded")
        .count();
    let total_claims = candidate.source_memory_ids.len().max(1);
    let grounded_evidence_ratio = round_score(grounded_claim_count as f64 / total_claims as f64);
    let decision = if grounded_evidence_ratio >= 0.8 && contradicted_claim_count == 0 {
        "verified"
    } else {
        "rejected"
    };
    let now = timestamp_string();
    let verification_id = format!("skill_verification_{}_{}", now, next_sequence());
    let verification_path = verification_path(data_dir, &verification_id);
    let verification = SkillProvenanceVerification {
        schema_version: "skill.provenance_verification.v1".to_string(),
        conformance_line: "M16.skill_provenance_verification".to_string(),
        canonical_path: verification_path.display().to_string(),
        retention_policy: "append_only_skill_evolution_evidence".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        verification_id: verification_id.clone(),
        candidate_id: candidate_id.to_string(),
        grounded_claim_count,
        hallucinated_claim_count,
        contradicted_claim_count,
        grounded_evidence_ratio,
        decision: decision.to_string(),
        evidence_refs: candidate.support_refs.clone(),
        created_at: now.clone(),
    };
    write_json_atomic(&verification_path, &verification)?;
    append_json_line(&verifications_log_path(data_dir), &verification)?;

    let workflow_clarity = if artifact_text.contains("## Steps") {
        1.0
    } else {
        0.0
    };
    let specificity_and_reusability = (candidate.support_refs.len() as f64 / 3.0).min(1.0);
    let preserves_existing_value = (grounded_claim_count as f64 / 2.0).min(1.0);
    let overall_score = round_score(
        0.35 * workflow_clarity
            + 0.35 * specificity_and_reusability
            + 0.30 * preserves_existing_value,
    );
    let score_id = format!("skill_structure_score_{}_{}", now, next_sequence());
    let score_path = structure_score_path(data_dir, &score_id);
    let structure_score = SkillStructureScore {
        schema_version: "skill.structure_score.v1".to_string(),
        conformance_line: "M16.skill_structure_score".to_string(),
        canonical_path: score_path.display().to_string(),
        retention_policy: "append_only_skill_evolution_evidence".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        score_id,
        candidate_id: candidate_id.to_string(),
        workflow_clarity: round_score(workflow_clarity),
        specificity_and_reusability: round_score(specificity_and_reusability),
        preserves_existing_value: round_score(preserves_existing_value),
        overall_score,
        upgrade_decision: if decision == "verified" && overall_score >= 0.6 {
            "upgrade_allowed"
        } else {
            "upgrade_blocked"
        }
        .to_string(),
        created_at: now,
    };
    write_json_atomic(&score_path, &structure_score)?;
    append_json_line(&structure_scores_log_path(data_dir), &structure_score)?;

    candidate.verification_status = decision.to_string();
    candidate.updated_at = timestamp_string();
    write_json_atomic(&candidate_path(data_dir, candidate_id), &candidate)?;
    Ok(SkillVerifyResult {
        verification,
        structure_score,
    })
}

pub fn submit_request(
    data_dir: &Path,
    candidate_id: &str,
) -> Result<SkillOutputSubmitRequest, EvolutionError> {
    let mut candidate = load_candidate(data_dir, candidate_id)?;
    if candidate.verification_status != "verified" {
        return Err(EvolutionError::InvalidInput(format!(
            "skill evolution candidate is not verified: {candidate_id}"
        )));
    }
    candidate.publication_status = "submitted_to_m11_governance".to_string();
    candidate.updated_at = timestamp_string();
    write_json_atomic(&candidate_path(data_dir, candidate_id), &candidate)?;
    Ok(SkillOutputSubmitRequest {
        skill_id: "skills.evolve.crystallized".to_string(),
        output_kind: "skill".to_string(),
        artifacts: vec![SkillOutputArtifactInput {
            artifact_path: candidate.skill_artifact_path,
            artifact_kind: "skill_markdown".to_string(),
        }],
        artifact_family: format!("skill_catalog/{}", slug(&candidate.title)),
        doc_frame_path: None,
        canonicality_policy: "requires_review".to_string(),
        human_gate_required: true,
    })
}

pub fn install(
    data_dir: &Path,
    workspace_root: &Path,
    candidate_id: &str,
    human_gate_approved: bool,
) -> Result<SkillInstallRecord, EvolutionError> {
    let candidate = load_candidate(data_dir, candidate_id)?;
    if candidate.verification_status != "verified" {
        return Err(EvolutionError::PublicationGateRequired(format!(
            "skill evolution candidate is not verified: {candidate_id}"
        )));
    }
    if !human_gate_approved {
        return Err(EvolutionError::PublicationGateRequired(format!(
            "skill evolution install requires explicit human approval: {candidate_id}"
        )));
    }
    validate_install_skill_id(&candidate.skill_id)?;
    let source_path =
        workspace_relative_existing_file(workspace_root, &candidate.skill_artifact_path)?;
    let install_dir = workspace_root
        .join(".codex")
        .join("skills")
        .join(&candidate.skill_id);
    let manifest_path = install_dir.join("SKILL.md");
    if manifest_path.exists() {
        return Err(EvolutionError::InvalidInput(format!(
            "local skill already exists and will not be overwritten: {}",
            relative_workspace_path(workspace_root, &manifest_path)
        )));
    }
    fs::create_dir_all(&install_dir)?;
    fs::copy(&source_path, &manifest_path)?;

    let now = timestamp_string();
    let install_id = format!("skill_install_{}_{}", now, next_sequence());
    let record_path = install_path(data_dir, &install_id);
    let record = SkillInstallRecord {
        schema_version: "skill.install_record.v1".to_string(),
        conformance_line: "M16.skill_install".to_string(),
        canonical_path: record_path.display().to_string(),
        retention_policy: "project_local_until_skill_uninstalled_or_superseded".to_string(),
        atomic_write_policy: "append_log_plus_atomic_record_snapshot".to_string(),
        install_id,
        candidate_id: candidate_id.to_string(),
        skill_id: candidate.skill_id,
        install_state: "installed".to_string(),
        install_path: relative_workspace_path(workspace_root, &install_dir),
        manifest_path: relative_workspace_path(workspace_root, &manifest_path),
        human_gate_approved,
        support_refs: candidate.support_refs,
        created_at: now,
    };
    write_json_atomic(&record_path, &record)?;
    append_json_line(&installs_log_path(data_dir), &record)?;
    Ok(record)
}

pub fn load_candidate(
    data_dir: &Path,
    candidate_id: &str,
) -> Result<SkillEvolutionCandidate, EvolutionError> {
    let path = candidate_path(data_dir, candidate_id);
    if !path.exists() {
        return Err(EvolutionError::InvalidInput(format!(
            "unknown skill evolution candidate: {candidate_id}"
        )));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn render_skill_markdown(
    title: &str,
    trigger: &str,
    cluster: &KnowledgeClusterRecord,
    memories: &[MemoryRecord],
) -> String {
    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&format!("name: {}\n", slug(title)));
    body.push_str(&format!("description: {trigger}\n"));
    body.push_str("---\n\n");
    body.push_str(&format!("# {title}\n\n"));
    body.push_str("## Trigger\n\n");
    body.push_str(trigger);
    body.push_str("\n\n## Steps\n\n");
    body.push_str(&format!(
        "1. Inspect the source memories in cluster `{}` before acting.\n",
        cluster.cluster_id
    ));
    body.push_str("2. Check every support reference before publishing docs, code, or claims.\n");
    body.push_str("3. Route any durable output through the existing SkillOutputEnvelope publication gate.\n\n");
    body.push_str("## Provenance\n\n");
    for memory in memories {
        body.push_str(&format!(
            "- memory `{}`: {}; support: {}\n",
            memory.record_id,
            memory.summary,
            memory.support_refs.join(", ")
        ));
    }
    body
}

fn discover_memory_clusters(memories: &[MemoryRecord], threshold: usize) -> Vec<Vec<MemoryRecord>> {
    if memories.len() < threshold {
        return Vec::new();
    }
    let profiles = memories
        .iter()
        .map(memory_cluster_profile)
        .collect::<Vec<_>>();
    let mut visited = vec![false; memories.len()];
    let mut clusters = Vec::new();
    for seed in 0..memories.len() {
        if visited[seed] {
            continue;
        }
        visited[seed] = true;
        let mut stack = vec![seed];
        let mut component = Vec::new();
        while let Some(current) = stack.pop() {
            component.push(memories[current].clone());
            for candidate in 0..memories.len() {
                if visited[candidate] {
                    continue;
                }
                if token_profiles_related(&profiles[current], &profiles[candidate]) {
                    visited[candidate] = true;
                    stack.push(candidate);
                }
            }
        }
        if component.len() >= threshold {
            component.sort_by(|left, right| left.record_id.cmp(&right.record_id));
            clusters.push(component);
        }
    }
    clusters.sort_by(|left, right| {
        cluster_topic(left)
            .cmp(&cluster_topic(right))
            .then_with(|| left.len().cmp(&right.len()))
    });
    clusters
}

fn memory_cluster_profile(memory: &MemoryRecord) -> BTreeMap<String, f64> {
    let mut profile = BTreeMap::<String, f64>::new();
    add_weighted_tokens(&mut profile, &memory.summary, 2.0);
    add_weighted_tokens(&mut profile, &memory.title, 1.0);
    add_weighted_tokens(&mut profile, &memory.body, 0.5);
    profile
}

fn add_weighted_tokens(profile: &mut BTreeMap<String, f64>, text: &str, weight: f64) {
    for token in tokenize(text)
        .into_iter()
        .filter(|token| !STOPWORDS.contains(&token.as_str()))
        .collect::<BTreeSet<_>>()
    {
        *profile.entry(token).or_insert(0.0) += weight;
    }
}

fn token_profiles_related(left: &BTreeMap<String, f64>, right: &BTreeMap<String, f64>) -> bool {
    let shared_token_count = left
        .keys()
        .filter(|token| right.contains_key(*token))
        .count();
    if shared_token_count < CLUSTER_MIN_SHARED_TOKENS {
        return false;
    }
    weighted_jaccard(left, right) >= CLUSTER_SIMILARITY_THRESHOLD
}

fn weighted_jaccard(left: &BTreeMap<String, f64>, right: &BTreeMap<String, f64>) -> f64 {
    let mut intersection = 0.0;
    let mut union = 0.0;
    for (token, left_weight) in left {
        if let Some(right_weight) = right.get(token) {
            intersection += left_weight.min(*right_weight);
            union += left_weight.max(*right_weight);
        } else {
            union += left_weight;
        }
    }
    for (token, right_weight) in right {
        if !left.contains_key(token) {
            union += right_weight;
        }
    }
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn latest_active_memories(data_dir: &Path) -> Result<Vec<MemoryRecord>, EvolutionError> {
    let mut records_by_id = HashMap::<String, MemoryRecord>::new();
    for record in read_jsonl_records::<MemoryRecord>(&durable_records_log_path(data_dir))? {
        records_by_id.insert(record.record_id.clone(), record);
    }
    let mut records = records_by_id
        .into_values()
        .filter(|record| {
            record.injectable
                && matches!(record.status.as_str(), "supported" | "trusted")
                && record.superseded_by.is_empty()
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    Ok(records)
}

fn memories_by_id(
    data_dir: &Path,
    memory_ids: &[String],
) -> Result<Vec<MemoryRecord>, EvolutionError> {
    let records = latest_active_memories(data_dir)?;
    let by_id = records
        .into_iter()
        .map(|record| (record.record_id.clone(), record))
        .collect::<HashMap<_, _>>();
    let mut selected = Vec::new();
    for memory_id in memory_ids {
        let record = by_id.get(memory_id).ok_or_else(|| {
            EvolutionError::InvalidInput(format!(
                "skill evolution source memory is not active or injectable: {memory_id}"
            ))
        })?;
        selected.push(record.clone());
    }
    Ok(selected)
}

fn load_cluster(
    data_dir: &Path,
    cluster_id: &str,
) -> Result<KnowledgeClusterRecord, EvolutionError> {
    let path = cluster_path(data_dir, cluster_id);
    if !path.exists() {
        return Err(EvolutionError::InvalidInput(format!(
            "unknown skill evolution cluster: {cluster_id}"
        )));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn cluster_topic(memories: &[MemoryRecord]) -> String {
    let mut counts = BTreeMap::<String, (usize, usize)>::new();
    let mut position = 0;
    for memory in memories {
        for token in tokenize(&format!("{} {}", memory.summary, memory.body)) {
            if STOPWORDS.contains(&token.as_str()) {
                continue;
            }
            let entry = counts.entry(token).or_insert((0, position));
            entry.0 += 1;
            entry.1 = entry.1.min(position);
            position += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|left, right| {
            let (left_count, left_position) = left.1;
            let (right_count, right_position) = right.1;
            left_count
                .cmp(&right_count)
                .then_with(|| right_position.cmp(&left_position))
                .then_with(|| right.0.cmp(&left.0))
        })
        .map(|(token, _)| token)
        .unwrap_or_else(|| "evolved skill".to_string())
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_lowercase())
        .filter(|token| token.len() >= 4)
        .collect()
}

const STOPWORDS: &[&str] = &[
    "this", "that", "with", "from", "before", "after", "into", "memory", "supports", "requires",
    "workflow",
];

const CLUSTER_SIMILARITY_THRESHOLD: f64 = 0.34;
const CLUSTER_MIN_SHARED_TOKENS: usize = 2;

fn durable_records_log_path(data_dir: &Path) -> PathBuf {
    data_dir
        .join("memory")
        .join("durable")
        .join("records.jsonl")
}

fn evolution_root(data_dir: &Path) -> PathBuf {
    data_dir.join("skills").join("evolution")
}

fn clusters_dir(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("clusters")
}

fn cluster_path(data_dir: &Path, cluster_id: &str) -> PathBuf {
    clusters_dir(data_dir).join(format!("{cluster_id}.json"))
}

fn clusters_log_path(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("clusters.jsonl")
}

fn candidates_dir(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("candidates")
}

fn candidate_path(data_dir: &Path, candidate_id: &str) -> PathBuf {
    candidates_dir(data_dir).join(format!("{candidate_id}.json"))
}

fn candidates_log_path(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("candidates.jsonl")
}

fn events_log_path(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("events.jsonl")
}

fn verifications_dir(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("verifications")
}

fn verification_path(data_dir: &Path, verification_id: &str) -> PathBuf {
    verifications_dir(data_dir).join(format!("{verification_id}.json"))
}

fn verifications_log_path(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("verifications.jsonl")
}

fn structure_scores_dir(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("structure_scores")
}

fn structure_score_path(data_dir: &Path, score_id: &str) -> PathBuf {
    structure_scores_dir(data_dir).join(format!("{score_id}.json"))
}

fn structure_scores_log_path(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("structure_scores.jsonl")
}

fn installs_dir(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("installs")
}

fn install_path(data_dir: &Path, install_id: &str) -> PathBuf {
    installs_dir(data_dir).join(format!("{install_id}.json"))
}

fn installs_log_path(data_dir: &Path) -> PathBuf {
    evolution_root(data_dir).join("installs.jsonl")
}

fn read_jsonl_records<T>(path: &Path) -> Result<Vec<T>, EvolutionError>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line)?);
    }
    Ok(records)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), EvolutionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<(), EvolutionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(&line)?;
    Ok(())
}

fn digest16(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

fn slug(value: &str) -> String {
    let mut output = value
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    output.trim_matches('-').to_string()
}

fn title_case(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_install_skill_id(skill_id: &str) -> Result<(), EvolutionError> {
    if skill_id.is_empty()
        || skill_id.contains('/')
        || skill_id.contains('\\')
        || skill_id.contains("..")
    {
        return Err(EvolutionError::InvalidInput(format!(
            "invalid skill id for local install: {skill_id}"
        )));
    }
    Ok(())
}

fn workspace_relative_existing_file(
    workspace_root: &Path,
    candidate_path: &str,
) -> Result<PathBuf, EvolutionError> {
    let relative_path = Path::new(candidate_path);
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(EvolutionError::InvalidInput(format!(
            "skill artifact path must be workspace-relative and stay inside workspace: {candidate_path}"
        )));
    }
    let workspace_root = workspace_root.canonicalize()?;
    let source_path = workspace_root.join(relative_path);
    if !source_path.is_file() {
        return Err(EvolutionError::InvalidInput(format!(
            "skill artifact does not exist: {candidate_path}"
        )));
    }
    let canonical_source = source_path.canonicalize()?;
    if !canonical_source.starts_with(&workspace_root) {
        return Err(EvolutionError::InvalidInput(format!(
            "skill artifact path must stay inside workspace: {candidate_path}"
        )));
    }
    Ok(canonical_source)
}

fn relative_workspace_path(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn round_score(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn next_sequence() -> u64 {
    EVOLUTION_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub enum EvolutionError {
    InvalidInput(String),
    PublicationGateRequired(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for EvolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "{message}"),
            Self::PublicationGateRequired(message) => write!(formatter, "{message}"),
            Self::Io(err) => write!(formatter, "{err}"),
            Self::Json(err) => write!(formatter, "{err}"),
        }
    }
}

impl std::error::Error for EvolutionError {}

impl From<std::io::Error> for EvolutionError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for EvolutionError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
