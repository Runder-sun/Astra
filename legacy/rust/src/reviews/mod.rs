use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const REVIEW_MATERIAL_MAX_CHARS_PER_TARGET: usize = 64_000;
const REVIEW_MATERIAL_MAX_TOTAL_CHARS: usize = 160_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPacket {
    pub schema_version: String,
    pub conformance_line: String,
    pub canonical_path: String,
    pub retention_policy: String,
    pub atomic_write_policy: String,
    pub review_id: String,
    pub target_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_materials: Vec<ReviewTargetMaterial>,
    pub objective: String,
    pub reviewer_role: String,
    pub review_model: String,
    pub fresh_thread: bool,
    #[serde(default = "default_blinded")]
    pub blinded: bool,
    #[serde(default = "default_blinding_policy")]
    pub blinding_policy: String,
    pub blind_context: Vec<String>,
    pub banned_context: Vec<String>,
    #[serde(default = "default_redaction")]
    pub redaction: ReviewInputRedaction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare_against: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retry_attempt: u64,
    pub evidence_required: Vec<String>,
    pub output_schema: String,
    pub trace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewTargetMaterial {
    pub source_ref: String,
    pub content_sha256: String,
    pub original_bytes: usize,
    pub included_bytes: usize,
    pub truncated: bool,
    pub content: String,
}

impl ReviewTargetMaterial {
    pub fn from_text(source_ref: impl Into<String>, content: &str, max_chars: usize) -> Self {
        let original_bytes = content.as_bytes().len();
        let truncated = max_chars > 0 && content.chars().count() > max_chars;
        let material = if truncated {
            let mut value = content.chars().take(max_chars).collect::<String>();
            value.push_str("\n...[truncated]");
            value
        } else {
            content.to_string()
        };
        Self {
            source_ref: source_ref.into(),
            content_sha256: sha256_bytes(content.as_bytes()),
            original_bytes,
            included_bytes: material.as_bytes().len(),
            truncated,
            content: material,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewInputRedaction {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default = "default_blinding_policy")]
    pub policy: String,
    #[serde(default)]
    pub allowed_count: usize,
    #[serde(default)]
    pub redacted_count: usize,
    #[serde(default)]
    pub redacted_sources: Vec<ReviewRedactedSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRedactedSource {
    pub source_kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTrace {
    pub review_id: String,
    pub trace_id: String,
    pub thread_id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt_snapshot: String,
    pub file_list: Vec<String>,
    pub response_text: String,
    pub verdict: String,
    pub timestamp: String,
    pub trace_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewOpenResult {
    pub status: String,
    pub packet_path: String,
    pub latest_trace_path: String,
    pub packet: ReviewPacket,
    pub trace: ReviewTrace,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewListEntry {
    pub review_id: String,
    pub objective: String,
    pub reviewer_role: String,
    pub review_model: String,
    pub target_count: usize,
    pub status: String,
    pub verdict: String,
    pub packet_path: String,
    pub latest_trace_path: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewListResult {
    pub reviews: Vec<ReviewListEntry>,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewInspectionResult {
    pub status: String,
    pub packet_path: String,
    pub history_paths: Vec<String>,
    pub packet: ReviewPacket,
    pub latest_trace: ReviewTrace,
}

#[derive(Debug, Clone)]
pub struct ReviewOpenRequest {
    pub target_paths: Vec<String>,
    pub objective: String,
    pub reviewer_role: String,
    pub review_model: String,
    pub blind_context: Vec<String>,
    pub review_materials: Vec<ReviewTargetMaterial>,
    pub executor_summary: Option<String>,
    pub evidence_required: Vec<String>,
    pub compare_against: Option<String>,
    pub retry_of: Option<String>,
    pub retry_attempt: u64,
    pub verdict: Option<String>,
    pub response_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReviewResolveRequest {
    pub verdict: String,
    pub response_text: String,
}

#[derive(Debug)]
pub enum ReviewError {
    UnknownReview(String),
    InvalidCompareTarget(String),
    RetryNotAllowed { review_id: String, verdict: String },
    ResolveNotAllowed { review_id: String, verdict: String },
    InvalidVerdict(String),
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownReview(review_id) => write!(f, "unknown review: {review_id}"),
            Self::InvalidCompareTarget(review_id) => {
                write!(f, "compare_against review does not exist: {review_id}")
            }
            Self::RetryNotAllowed { review_id, verdict } => write!(
                f,
                "review retry is not allowed for {review_id} with verdict {verdict}"
            ),
            Self::ResolveNotAllowed { review_id, verdict } => write!(
                f,
                "review resolve is not allowed for {review_id} with current verdict {verdict}"
            ),
            Self::InvalidVerdict(verdict) => write!(f, "invalid review verdict: {verdict}"),
            Self::Io(err) => write!(f, "review IO failed: {err}"),
            Self::Serde(err) => write!(f, "review serialization failed: {err}"),
        }
    }
}

impl std::error::Error for ReviewError {}

impl From<std::io::Error> for ReviewError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ReviewError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn open(
    data_dir: &Path,
    mut request: ReviewOpenRequest,
) -> Result<ReviewOpenResult, ReviewError> {
    if let Some(compare_against) = request.compare_against.as_deref() {
        if !packet_path(data_dir, compare_against).exists() {
            return Err(ReviewError::InvalidCompareTarget(
                compare_against.to_string(),
            ));
        }
    }

    let review_id = format!("rev_{}", timestamp_string());
    let trace_id = format!("trc_{}", timestamp_string());
    let packet_path = packet_path(data_dir, &review_id);
    let history_path = trace_history_path(data_dir, &review_id, &trace_id);
    let latest_trace_path = latest_trace_path(data_dir, &review_id);

    if let Some(parent) = packet_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = history_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let requested_review_materials = std::mem::take(&mut request.review_materials);
    let review_materials = if requested_review_materials.is_empty() {
        build_review_materials_from_targets(data_dir, &request.target_paths)
    } else {
        limit_review_materials(requested_review_materials)
    };
    let blinding = build_blinded_input(&request, review_materials.len());
    let packet = ReviewPacket {
        schema_version: "v1alpha1".to_string(),
        conformance_line: "M3.review_packet.v1".to_string(),
        canonical_path: packet_path.display().to_string(),
        retention_policy: "retain_history".to_string(),
        atomic_write_policy: "write_temp_then_rename".to_string(),
        review_id: review_id.clone(),
        target_paths: request.target_paths,
        review_materials,
        objective: request.objective,
        reviewer_role: request.reviewer_role,
        review_model: request.review_model.clone(),
        fresh_thread: true,
        blinded: true,
        blinding_policy: blinding.redaction.policy.clone(),
        blind_context: blinding.blind_context,
        banned_context: default_banned_context(),
        redaction: blinding.redaction,
        compare_against: request.compare_against,
        retry_of: request.retry_of,
        retry_attempt: request.retry_attempt,
        evidence_required: request.evidence_required,
        output_schema: "review_trace.schema.json".to_string(),
        trace_id: trace_id.clone(),
    };
    let trace = ReviewTrace {
        review_id: review_id.clone(),
        trace_id: trace_id.clone(),
        thread_id: format!("fresh::{review_id}"),
        model: request.review_model,
        reasoning_effort: "medium".to_string(),
        prompt_snapshot: build_prompt_snapshot(&packet),
        file_list: packet.target_paths.clone(),
        response_text: request.response_text.unwrap_or_default(),
        verdict: request.verdict.unwrap_or_else(|| "pending".to_string()),
        timestamp: timestamp_string(),
        trace_path: history_path.display().to_string(),
    };

    atomic_write(&packet_path, &serde_json::to_string_pretty(&packet)?)?;
    atomic_write(&history_path, &serde_json::to_string_pretty(&trace)?)?;
    atomic_write(&latest_trace_path, &serde_json::to_string_pretty(&trace)?)?;

    Ok(ReviewOpenResult {
        status: review_status(&trace.verdict),
        packet_path: packet_path.display().to_string(),
        latest_trace_path: latest_trace_path.display().to_string(),
        packet,
        trace,
    })
}

pub fn retry(
    data_dir: &Path,
    review_id: &str,
    force: bool,
) -> Result<ReviewOpenResult, ReviewError> {
    let packet = load_packet(data_dir, review_id)?;
    let latest_trace = load_latest_trace(data_dir, review_id)?;
    if !force && !is_retryable_verdict(&latest_trace.verdict) {
        return Err(ReviewError::RetryNotAllowed {
            review_id: review_id.to_string(),
            verdict: latest_trace.verdict,
        });
    }
    open(
        data_dir,
        ReviewOpenRequest {
            target_paths: packet.target_paths,
            objective: packet.objective,
            reviewer_role: packet.reviewer_role,
            review_model: packet.review_model,
            blind_context: packet.blind_context,
            review_materials: packet.review_materials,
            executor_summary: None,
            evidence_required: packet.evidence_required,
            compare_against: packet.compare_against,
            retry_of: Some(review_id.to_string()),
            retry_attempt: packet.retry_attempt + 1,
            verdict: None,
            response_text: None,
        },
    )
}

pub fn resolve(
    data_dir: &Path,
    review_id: &str,
    request: ReviewResolveRequest,
) -> Result<ReviewOpenResult, ReviewError> {
    let verdict = normalize_verdict(&request.verdict)?;
    let packet = load_packet(data_dir, review_id)?;
    let latest_trace = load_latest_trace(data_dir, review_id)?;
    if latest_trace.verdict != "pending" {
        return Err(ReviewError::ResolveNotAllowed {
            review_id: review_id.to_string(),
            verdict: latest_trace.verdict,
        });
    }

    let trace_id = format!("trc_{}", timestamp_string());
    let history_path = trace_history_path(data_dir, review_id, &trace_id);
    let latest_trace_path = latest_trace_path(data_dir, review_id);
    let trace = ReviewTrace {
        review_id: review_id.to_string(),
        trace_id,
        thread_id: latest_trace.thread_id,
        model: latest_trace.model,
        reasoning_effort: latest_trace.reasoning_effort,
        prompt_snapshot: latest_trace.prompt_snapshot,
        file_list: latest_trace.file_list,
        response_text: request.response_text,
        verdict,
        timestamp: timestamp_string(),
        trace_path: history_path.display().to_string(),
    };

    atomic_write(&history_path, &serde_json::to_string_pretty(&trace)?)?;
    atomic_write(&latest_trace_path, &serde_json::to_string_pretty(&trace)?)?;

    Ok(ReviewOpenResult {
        status: review_status(&trace.verdict),
        packet_path: packet_path(data_dir, review_id).display().to_string(),
        latest_trace_path: latest_trace_path.display().to_string(),
        packet,
        trace,
    })
}

pub fn list(
    data_dir: &Path,
    status_filter: Option<&str>,
    verdict_filter: Option<&str>,
) -> Result<ReviewListResult, ReviewError> {
    let mut reviews = Vec::new();
    for review_id in review_ids(data_dir)? {
        let packet = load_packet(data_dir, &review_id)?;
        let trace = load_latest_trace(data_dir, &review_id)?;
        let status = review_status(&trace.verdict);
        if let Some(filter) = status_filter {
            if status != filter {
                continue;
            }
        }
        if let Some(filter) = verdict_filter {
            if trace.verdict != filter {
                continue;
            }
        }
        reviews.push(ReviewListEntry {
            review_id: packet.review_id.clone(),
            objective: packet.objective.clone(),
            reviewer_role: packet.reviewer_role.clone(),
            review_model: packet.review_model.clone(),
            target_count: packet.target_paths.len(),
            status: status.to_string(),
            verdict: trace.verdict.clone(),
            packet_path: packet_path(data_dir, &review_id).display().to_string(),
            latest_trace_path: latest_trace_path(data_dir, &review_id)
                .display()
                .to_string(),
            updated_at: trace.timestamp,
        });
    }
    reviews.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(ReviewListResult {
        total_count: reviews.len(),
        reviews,
    })
}

pub fn inspect(data_dir: &Path, review_id: &str) -> Result<ReviewInspectionResult, ReviewError> {
    let packet = load_packet(data_dir, review_id)?;
    let latest_trace = load_latest_trace(data_dir, review_id)?;
    let mut history_paths = trace_history_paths(data_dir, review_id)?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    history_paths.sort();
    Ok(ReviewInspectionResult {
        status: review_status(&latest_trace.verdict),
        packet_path: packet_path(data_dir, review_id).display().to_string(),
        history_paths,
        packet,
        latest_trace,
    })
}

pub fn review_links_for_target(
    data_dir: &Path,
    target: &str,
    canonical_path: &Path,
) -> Vec<String> {
    let canonical = canonical_path.display().to_string();
    let mut matches = Vec::new();
    let Ok(review_ids) = review_ids(data_dir) else {
        return matches;
    };

    for review_id in review_ids {
        let Ok(packet) = load_packet(data_dir, &review_id) else {
            continue;
        };
        if packet
            .target_paths
            .iter()
            .any(|entry| entry == target || entry == &canonical || canonical.ends_with(entry))
        {
            matches.push(review_id);
        }
    }
    matches.sort();
    matches
}

fn load_packet(data_dir: &Path, review_id: &str) -> Result<ReviewPacket, ReviewError> {
    let path = packet_path(data_dir, review_id);
    if !path.exists() {
        return Err(ReviewError::UnknownReview(review_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn load_latest_trace(data_dir: &Path, review_id: &str) -> Result<ReviewTrace, ReviewError> {
    let path = latest_trace_path(data_dir, review_id);
    if !path.exists() {
        return Err(ReviewError::UnknownReview(review_id.to_string()));
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn review_ids(data_dir: &Path) -> Result<Vec<String>, ReviewError> {
    let root = reviews_root(data_dir);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut ids = fs::read_dir(root)?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

fn trace_history_paths(data_dir: &Path, review_id: &str) -> Result<Vec<PathBuf>, ReviewError> {
    let root = review_dir(data_dir, review_id).join("traces");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(root)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

struct BlindedReviewInput {
    blind_context: Vec<String>,
    redaction: ReviewInputRedaction,
}

fn build_blinded_input(
    request: &ReviewOpenRequest,
    review_material_count: usize,
) -> BlindedReviewInput {
    let mut redacted_sources = Vec::new();
    if request.executor_summary.is_some() {
        redacted_sources.push(ReviewRedactedSource {
            source_kind: "executor_summary".to_string(),
            reason: "executor interpretations are banned from default reviewer packets".to_string(),
        });
    }

    BlindedReviewInput {
        blind_context: request.blind_context.clone(),
        redaction: ReviewInputRedaction {
            schema_version: "v1alpha1".to_string(),
            policy: "default_reviewer_blinding".to_string(),
            allowed_count: request.blind_context.len() + review_material_count,
            redacted_count: redacted_sources.len(),
            redacted_sources,
        },
    }
}

fn default_blinded() -> bool {
    true
}

fn default_schema_version() -> String {
    "v1alpha1".to_string()
}

fn default_blinding_policy() -> String {
    "default_reviewer_blinding".to_string()
}

fn default_redaction() -> ReviewInputRedaction {
    ReviewInputRedaction {
        schema_version: default_schema_version(),
        policy: default_blinding_policy(),
        allowed_count: 0,
        redacted_count: 0,
        redacted_sources: Vec::new(),
    }
}

fn build_prompt_snapshot(packet: &ReviewPacket) -> String {
    let mut snapshot = format!(
        "objective: {}\ntargets: {}\nreviewer_role: {}\nreview_model: {}\nblinding_policy: {}\nblind_context: {}\ncompare_against: {}\nretry_of: {}\nretry_attempt: {}",
        packet.objective,
        packet.target_paths.join(", "),
        packet.reviewer_role,
        packet.review_model,
        packet.blinding_policy,
        packet.blind_context.join(" | "),
        packet.compare_against.clone().unwrap_or_default(),
        packet.retry_of.clone().unwrap_or_default(),
        packet.retry_attempt
    );
    if !packet.review_materials.is_empty() {
        snapshot.push_str("\nreview_materials:");
        for material in &packet.review_materials {
            snapshot.push_str(&format!(
                "\n\n--- review material: {} ---\nsha256: {}\noriginal_bytes: {}\nincluded_bytes: {}\ntruncated: {}\n\n{}\n--- end review material ---",
                material.source_ref,
                material.content_sha256,
                material.original_bytes,
                material.included_bytes,
                material.truncated,
                material.content
            ));
        }
    }
    snapshot
}

fn build_review_materials_from_targets(
    data_dir: &Path,
    target_paths: &[String],
) -> Vec<ReviewTargetMaterial> {
    let mut materials = Vec::new();
    let mut remaining = REVIEW_MATERIAL_MAX_TOTAL_CHARS;
    for target in target_paths {
        if remaining == 0 {
            break;
        }
        let Some(path) = resolve_review_target_path(data_dir, target) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let max_chars = REVIEW_MATERIAL_MAX_CHARS_PER_TARGET.min(remaining);
        let material = ReviewTargetMaterial::from_text(target.clone(), &content, max_chars);
        remaining = remaining.saturating_sub(material.content.chars().count());
        materials.push(material);
    }
    materials
}

fn limit_review_materials(materials: Vec<ReviewTargetMaterial>) -> Vec<ReviewTargetMaterial> {
    let mut limited = Vec::new();
    let mut remaining = REVIEW_MATERIAL_MAX_TOTAL_CHARS;
    for mut material in materials {
        if remaining == 0 {
            break;
        }
        let max_chars = REVIEW_MATERIAL_MAX_CHARS_PER_TARGET.min(remaining);
        if max_chars > 0 && material.content.chars().count() > max_chars {
            let mut content = material.content.chars().take(max_chars).collect::<String>();
            content.push_str("\n...[truncated]");
            material.content = content;
            material.included_bytes = material.content.as_bytes().len();
            material.truncated = true;
        }
        remaining = remaining.saturating_sub(material.content.chars().count());
        limited.push(material);
    }
    limited
}

fn resolve_review_target_path(data_dir: &Path, target: &str) -> Option<PathBuf> {
    let path = Path::new(target);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    data_dir
        .parent()
        .map(|workspace_root| workspace_root.join(path))
        .or_else(|| Some(path.to_path_buf()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn review_status(verdict: &str) -> String {
    if verdict == "pending" {
        "open".to_string()
    } else {
        "resolved".to_string()
    }
}

fn normalize_verdict(verdict: &str) -> Result<String, ReviewError> {
    let normalized = verdict.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "pending" {
        return Err(ReviewError::InvalidVerdict(verdict.to_string()));
    }
    if normalized
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        Ok(normalized)
    } else {
        Err(ReviewError::InvalidVerdict(verdict.to_string()))
    }
}

fn default_banned_context() -> Vec<String> {
    vec![
        "executor_interpretations".to_string(),
        "subjective_conclusions".to_string(),
        "pre_ranked_findings".to_string(),
        "leading_confirmation_requests".to_string(),
    ]
}

fn is_retryable_verdict(verdict: &str) -> bool {
    matches!(verdict, "timeout" | "transport_failure")
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn reviews_root(data_dir: &Path) -> PathBuf {
    data_dir.join("reviews")
}

fn review_dir(data_dir: &Path, review_id: &str) -> PathBuf {
    reviews_root(data_dir).join(review_id)
}

fn packet_path(data_dir: &Path, review_id: &str) -> PathBuf {
    review_dir(data_dir, review_id).join("packet.json")
}

fn latest_trace_path(data_dir: &Path, review_id: &str) -> PathBuf {
    review_dir(data_dir, review_id).join("trace.latest.json")
}

fn trace_history_path(data_dir: &Path, review_id: &str, trace_id: &str) -> PathBuf {
    review_dir(data_dir, review_id)
        .join("traces")
        .join(format!("{trace_id}.json"))
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), ReviewError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("{}.tmp", timestamp_string()));
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_data_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "research_cli_reviews_{label}_{}",
            timestamp_string()
        ));
        fs::create_dir_all(&root).expect("test data dir should be created");
        root
    }

    #[test]
    fn resolve_pending_review_updates_latest_trace_and_preserves_history() {
        let data_dir = temp_data_dir("resolve_pending");
        let opened = open(
            &data_dir,
            ReviewOpenRequest {
                target_paths: vec!["README.md".to_string()],
                objective: "Review a target".to_string(),
                reviewer_role: "external_reviewer".to_string(),
                review_model: "gpt-5.4".to_string(),
                blind_context: vec!["target: README.md".to_string()],
                review_materials: Vec::new(),
                executor_summary: None,
                evidence_required: vec!["finding".to_string()],
                compare_against: None,
                retry_of: None,
                retry_attempt: 0,
                verdict: None,
                response_text: None,
            },
        )
        .expect("review should open");
        assert_eq!(opened.status, "open");
        assert_eq!(opened.trace.verdict, "pending");

        let resolved = resolve(
            &data_dir,
            &opened.packet.review_id,
            ReviewResolveRequest {
                verdict: "PASS".to_string(),
                response_text: "approved after independent review".to_string(),
            },
        )
        .expect("review should resolve");

        assert_eq!(resolved.status, "resolved");
        assert_eq!(resolved.packet.review_id, opened.packet.review_id);
        assert_eq!(resolved.trace.verdict, "pass");
        assert_eq!(
            resolved.trace.response_text,
            "approved after independent review"
        );

        let inspection =
            inspect(&data_dir, &opened.packet.review_id).expect("review should inspect");
        assert_eq!(inspection.status, "resolved");
        assert_eq!(inspection.latest_trace.verdict, "pass");
        assert_eq!(inspection.history_paths.len(), 2);

        let list = list(&data_dir, Some("resolved"), Some("pass")).expect("reviews should list");
        assert_eq!(list.total_count, 1);
        assert_eq!(list.reviews[0].review_id, opened.packet.review_id);

        let second_resolve = resolve(
            &data_dir,
            &opened.packet.review_id,
            ReviewResolveRequest {
                verdict: "fail".to_string(),
                response_text: "second resolution should be blocked".to_string(),
            },
        );
        assert!(matches!(
            second_resolve,
            Err(ReviewError::ResolveNotAllowed { .. })
        ));

        let invalid = resolve(
            &data_dir,
            "missing",
            ReviewResolveRequest {
                verdict: "pending".to_string(),
                response_text: String::new(),
            },
        );
        assert!(matches!(invalid, Err(ReviewError::InvalidVerdict(_))));
    }

    #[test]
    fn open_review_snapshots_target_materials_in_packet_and_prompt() {
        let data_dir = temp_data_dir("target_materials");
        let evidence_path = data_dir.join("evidence.md");
        fs::write(
            &evidence_path,
            "# Evidence\n\nsource_id: S1\ncanonical verified title: Fixture Paper\n",
        )
        .expect("evidence should write");

        let opened = open(
            &data_dir,
            ReviewOpenRequest {
                target_paths: vec![evidence_path.display().to_string()],
                objective: "Review visible evidence".to_string(),
                reviewer_role: "external_reviewer".to_string(),
                review_model: "gpt-5.4".to_string(),
                blind_context: Vec::new(),
                review_materials: Vec::new(),
                executor_summary: None,
                evidence_required: vec!["source ledger".to_string()],
                compare_against: None,
                retry_of: None,
                retry_attempt: 0,
                verdict: None,
                response_text: None,
            },
        )
        .expect("review should open");

        assert_eq!(opened.packet.review_materials.len(), 1);
        assert!(opened.packet.review_materials[0]
            .content
            .contains("canonical verified title: Fixture Paper"));
        assert!(opened
            .trace
            .prompt_snapshot
            .contains("canonical verified title: Fixture Paper"));
        assert!(opened.trace.prompt_snapshot.contains("review_materials"));
    }
}
