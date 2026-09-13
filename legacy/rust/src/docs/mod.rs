use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocFrame {
    pub doc_id: String,
    pub schema_version: String,
    pub source_path: String,
    pub title: String,
    pub doc_type: String,
    pub lifecycle: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub milestone: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_claims: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_goals: Vec<String>,
    pub generated_by: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidDocRecord {
    pub source_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDocFrameRecord {
    pub source_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionFrameCandidate {
    pub candidate_id: String,
    pub source_doc_ids: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocIndex {
    pub index_id: String,
    pub project_id: String,
    pub generated_at: String,
    pub doc_frames: Vec<DocFrame>,
    pub invalid_docs: Vec<InvalidDocRecord>,
    pub missing_required_frames: Vec<String>,
    pub stale_frames: Vec<StaleDocFrameRecord>,
    pub mission_frame_candidates: Vec<MissionFrameCandidate>,
    pub total_count: usize,
    pub invalid_count: usize,
    pub missing_required_frames_count: usize,
    pub stale_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocContextProjection {
    pub schema_version: String,
    pub conformance_line: String,
    pub projection_policy: String,
    pub project_id: String,
    pub generated_at: String,
    pub doc_frames: Vec<DocFrame>,
    pub active_count: usize,
    pub injected_count: usize,
    pub omitted_active_count: usize,
    pub stale_count: usize,
    pub omitted_stale_count: usize,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocInspectionResult {
    pub source_path: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_frame: Option<DocFrame>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocFrameRefreshResult {
    pub source_path: String,
    pub dry_run: bool,
    pub write_applied: bool,
    pub candidate: DocFrame,
}

#[derive(Debug)]
pub enum DocFrameError {
    ScopeViolation(String),
    NotFound(String),
    InvalidFrame(String),
    WriteNotGraduated,
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl std::fmt::Display for DocFrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScopeViolation(message) => write!(f, "{message}"),
            Self::NotFound(path) => write!(f, "document does not exist: {path}"),
            Self::InvalidFrame(message) => write!(f, "{message}"),
            Self::WriteNotGraduated => write!(f, "docs frame refresh write mode is not graduated"),
            Self::Io(err) => write!(f, "doc frame IO failed: {err}"),
            Self::Serde(err) => write!(f, "doc frame serialization failed: {err}"),
        }
    }
}

impl std::error::Error for DocFrameError {}

impl From<std::io::Error> for DocFrameError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for DocFrameError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

pub fn index(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
) -> Result<DocIndex, DocFrameError> {
    let mut doc_frames = Vec::new();
    let mut invalid_docs = Vec::new();
    let mut missing_required_frames = Vec::new();

    for path in markdown_files(workspace_root)? {
        let source_path = relative_path(workspace_root, &path);
        let contents = fs::read_to_string(&path)?;
        match parse_doc_frame_from_contents(&contents, &source_path) {
            Ok(Some(frame)) => doc_frames.push(frame),
            Ok(None) => missing_required_frames.push(source_path),
            Err(err) => invalid_docs.push(InvalidDocRecord {
                source_path,
                reason: err.to_string(),
            }),
        }
    }

    doc_frames.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    let stale_frames = stale_doc_frames(workspace_root, &doc_frames);
    invalid_docs.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    missing_required_frames.sort();

    let index = DocIndex {
        index_id: format!("doc_index_{}", timestamp_string()),
        project_id: project_id.to_string(),
        generated_at: timestamp_string(),
        total_count: doc_frames.len(),
        invalid_count: invalid_docs.len(),
        missing_required_frames_count: missing_required_frames.len(),
        stale_count: stale_frames.len(),
        doc_frames,
        invalid_docs,
        missing_required_frames,
        stale_frames,
        mission_frame_candidates: Vec::new(),
    };

    let index_path = index_path(data_dir);
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(&index_path, &serde_json::to_string_pretty(&index)?)?;
    Ok(index)
}

pub fn context_projection(
    data_dir: &Path,
    workspace_root: &Path,
    project_id: &str,
) -> Result<DocContextProjection, DocFrameError> {
    let index = index(data_dir, workspace_root, project_id)?;
    let stale_sources = index
        .stale_frames
        .iter()
        .map(|record| record.source_path.clone())
        .collect::<BTreeSet<_>>();
    let mut active_doc_frames = index
        .doc_frames
        .into_iter()
        .filter(|frame| frame.lifecycle == "active")
        .filter(|frame| !stale_sources.contains(&frame.source_path))
        .collect::<Vec<_>>();
    active_doc_frames.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    let active_count = active_doc_frames.len();
    let omitted_active_count = active_count.saturating_sub(8);
    let mut doc_frames = active_doc_frames;
    if doc_frames.len() > 8 {
        doc_frames.truncate(8);
    }
    let injected_count = doc_frames.len();
    Ok(DocContextProjection {
        schema_version: "doc_context_projection.v1".to_string(),
        conformance_line: "M7.docframe_context".to_string(),
        projection_policy: "active_non_stale_docframes_after_mission_frame".to_string(),
        project_id: project_id.to_string(),
        generated_at: timestamp_string(),
        active_count,
        injected_count,
        omitted_active_count,
        stale_count: stale_sources.len(),
        omitted_stale_count: stale_sources.len(),
        evidence_refs: vec![index_path(data_dir).display().to_string()],
        doc_frames,
    })
}

pub fn inspect(workspace_root: &Path, target: &str) -> Result<DocInspectionResult, DocFrameError> {
    let path = resolve_workspace_doc_path(workspace_root, target)?;
    let source_path = relative_path(workspace_root, &path);
    let contents = fs::read_to_string(&path)?;
    match parse_doc_frame_from_contents(&contents, &source_path) {
        Ok(Some(frame)) => Ok(DocInspectionResult {
            source_path,
            status: "valid".to_string(),
            doc_frame: Some(frame),
            issues: Vec::new(),
        }),
        Ok(None) => Ok(DocInspectionResult {
            source_path,
            status: "missing".to_string(),
            doc_frame: None,
            issues: vec!["document has no DocFrame block".to_string()],
        }),
        Err(err) => Ok(DocInspectionResult {
            source_path,
            status: "invalid".to_string(),
            doc_frame: None,
            issues: vec![err.to_string()],
        }),
    }
}

pub fn frame_refresh(
    workspace_root: &Path,
    target: &str,
    dry_run: bool,
) -> Result<DocFrameRefreshResult, DocFrameError> {
    if !dry_run {
        return Err(DocFrameError::WriteNotGraduated);
    }
    let path = resolve_workspace_doc_path(workspace_root, target)?;
    let source_path = relative_path(workspace_root, &path);
    let contents = fs::read_to_string(&path)?;
    let candidate = candidate_frame(&source_path, &contents);
    Ok(DocFrameRefreshResult {
        source_path,
        dry_run,
        write_applied: false,
        candidate,
    })
}

fn markdown_files(workspace_root: &Path) -> Result<Vec<PathBuf>, DocFrameError> {
    let mut files = Vec::new();
    collect_markdown_files(workspace_root, workspace_root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_markdown_files(
    workspace_root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), DocFrameError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == ".pmcli" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_markdown_files(workspace_root, &path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("md")
            && path.starts_with(workspace_root)
        {
            files.push(path);
        }
    }
    Ok(())
}

fn stale_doc_frames(workspace_root: &Path, frames: &[DocFrame]) -> Vec<StaleDocFrameRecord> {
    let mut stale = Vec::new();
    for frame in frames {
        let Some(updated_at_ms) = parse_updated_at_ms(&frame.updated_at) else {
            continue;
        };
        let path = workspace_root.join(&frame.source_path);
        let Ok(metadata) = fs::metadata(&path) else {
            stale.push(StaleDocFrameRecord {
                source_path: frame.source_path.clone(),
                reason: "source document no longer exists".to_string(),
            });
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(modified_ms) = modified.duration_since(UNIX_EPOCH) else {
            continue;
        };
        if modified_ms.as_millis() > updated_at_ms {
            stale.push(StaleDocFrameRecord {
                source_path: frame.source_path.clone(),
                reason: "source document modified after DocFrame updated_at".to_string(),
            });
        }
    }
    stale.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    stale
}

fn parse_updated_at_ms(value: &str) -> Option<u128> {
    if value.chars().all(|character| character.is_ascii_digit()) {
        return value.parse::<u128>().ok();
    }
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let end_of_day_ms = (days as i128 + 1) * 86_400_000 - 1;
    u128::try_from(end_of_day_ms).ok()
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_i = i32::try_from(month).ok()?;
    let day_i = i32::try_from(day).ok()?;
    let doy = (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5 + day_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
}

fn parse_doc_frame_from_contents(
    contents: &str,
    source_path: &str,
) -> Result<Option<DocFrame>, DocFrameError> {
    if let Some(block) = front_matter_block(contents) {
        if let Some(doc_frame_block) = yaml_child_block(block, "doc_frame") {
            return Ok(Some(parse_yaml_doc_frame(doc_frame_block, source_path)?));
        }
    }

    if let Some(block) = fenced_doc_frame_block(contents) {
        return Ok(Some(parse_json_doc_frame(block, source_path)?));
    }

    Ok(None)
}

fn front_matter_block(contents: &str) -> Option<&str> {
    let stripped = contents.strip_prefix("---\n")?;
    let end = stripped.find("\n---")?;
    Some(&stripped[..end])
}

fn yaml_child_block<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let mut start = None;
    let mut end = block.len();
    for (offset, _) in block.match_indices('\n') {
        let next_line_start = offset + 1;
        let current = &block[next_line_start..].lines().next().unwrap_or_default();
        if current.trim_end() == format!("{key}:") {
            start = Some(next_line_start + current.len() + 1);
            continue;
        }
        if start.is_some() && !current.starts_with(' ') && !current.trim().is_empty() {
            end = next_line_start;
            break;
        }
    }
    if block.lines().next().unwrap_or_default().trim_end() == format!("{key}:") {
        start = Some(block.lines().next().unwrap_or_default().len() + 1);
    }
    start.map(|start| &block[start.min(block.len())..end])
}

fn fenced_doc_frame_block(contents: &str) -> Option<&str> {
    let marker = "```research-cli-doc-frame";
    let start = contents.find(marker)?;
    let after_marker = &contents[start + marker.len()..];
    let after_newline = after_marker.strip_prefix('\n').unwrap_or(after_marker);
    let end = after_newline.find("\n```")?;
    Some(&after_newline[..end])
}

fn parse_json_doc_frame(block: &str, source_path: &str) -> Result<DocFrame, DocFrameError> {
    let value: Value = serde_json::from_str(block)?;
    doc_frame_from_json_value(value, source_path)
}

fn parse_yaml_doc_frame(block: &str, source_path: &str) -> Result<DocFrame, DocFrameError> {
    let mut map = std::collections::BTreeMap::<String, Value>::new();
    let mut current_array_key: Option<String> = None;

    for raw_line in block.lines() {
        if raw_line.trim().is_empty() {
            continue;
        }
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if let Some(item) = trimmed.strip_prefix("- ") {
            if let Some(key) = current_array_key.as_ref() {
                map.entry(key.clone())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(Value::Array(items)) = map.get_mut(key) {
                    items.push(Value::String(unquote_yaml_scalar(item.trim()).to_string()));
                }
            }
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();
        if value.is_empty() {
            current_array_key = Some(key.clone());
            map.insert(key, Value::Array(Vec::new()));
        } else {
            current_array_key = None;
            map.insert(key, Value::String(unquote_yaml_scalar(value).to_string()));
        }
    }

    doc_frame_from_json_value(Value::Object(map.into_iter().collect()), source_path)
}

fn doc_frame_from_json_value(value: Value, source_path: &str) -> Result<DocFrame, DocFrameError> {
    let object = value.as_object().ok_or_else(|| {
        DocFrameError::InvalidFrame("DocFrame block must be an object".to_string())
    })?;
    let doc_id = string_field(object, &["doc_id", "id"])
        .ok_or_else(|| DocFrameError::InvalidFrame("DocFrame missing doc_id or id".to_string()))?;
    let title = string_field(object, &["title"]).unwrap_or_else(|| title_from_path(source_path));
    let summary = string_field(object, &["summary"]).unwrap_or_default();
    if summary.trim().is_empty() {
        return Err(DocFrameError::InvalidFrame(
            "DocFrame summary must be non-empty".to_string(),
        ));
    }

    Ok(DocFrame {
        doc_id,
        schema_version: string_field(object, &["schema_version"])
            .unwrap_or_else(|| "v1alpha1".to_string()),
        source_path: string_field(object, &["source_path"])
            .unwrap_or_else(|| source_path.to_string()),
        title,
        doc_type: string_field(object, &["doc_type"])
            .unwrap_or_else(|| "implementation_note".to_string()),
        lifecycle: string_field(object, &["lifecycle"]).unwrap_or_else(|| "draft".to_string()),
        scope: string_field(object, &["scope"]).unwrap_or_else(|| "project".to_string()),
        milestone: string_field(object, &["milestone"]).unwrap_or_default(),
        summary,
        key_claims: string_array_field(object, "key_claims"),
        decisions: string_array_field(object, "decisions"),
        interfaces: string_array_field(object, "interfaces"),
        evidence_refs: string_array_field(object, "evidence_refs"),
        next_actions: string_array_field(object, "next_actions"),
        non_goals: string_array_field(object, "non_goals"),
        generated_by: string_field(object, &["generated_by"])
            .unwrap_or_else(|| "unknown".to_string()),
        updated_at: string_field(object, &["updated_at"]).unwrap_or_else(timestamp_string),
    })
}

fn string_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

fn string_array_field(object: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn candidate_frame(source_path: &str, contents: &str) -> DocFrame {
    let title = contents
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| title_from_path(source_path));
    DocFrame {
        doc_id: doc_id_from_path(source_path),
        schema_version: "v1alpha1".to_string(),
        source_path: source_path.to_string(),
        title: title.clone(),
        doc_type: "implementation_note".to_string(),
        lifecycle: "draft".to_string(),
        scope: "project".to_string(),
        milestone: String::new(),
        summary: format!("Generated DocFrame candidate for {title}."),
        key_claims: Vec::new(),
        decisions: Vec::new(),
        interfaces: Vec::new(),
        evidence_refs: vec![source_path.to_string()],
        next_actions: vec![
            "Review and commit this DocFrame before using it as context.".to_string(),
        ],
        non_goals: vec!["Replace the document body or canonical MissionFrame.".to_string()],
        generated_by: "agent".to_string(),
        updated_at: timestamp_string(),
    }
}

fn resolve_workspace_doc_path(
    workspace_root: &Path,
    target: &str,
) -> Result<PathBuf, DocFrameError> {
    crate::workspace::path::resolve_existing_workspace_path(workspace_root, target).map_err(|err| {
        match err {
            crate::workspace::path::WorkspacePathError::NotFound(_) => {
                DocFrameError::NotFound(target.to_string())
            }
            crate::workspace::path::WorkspacePathError::AbsolutePath(_)
            | crate::workspace::path::WorkspacePathError::ScopeViolation { .. } => {
                DocFrameError::ScopeViolation(err.to_string())
            }
            crate::workspace::path::WorkspacePathError::Io(io_err) => DocFrameError::Io(io_err),
        }
    })
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn title_from_path(source_path: &str) -> String {
    Path::new(source_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document")
        .replace(['_', '-'], " ")
}

fn doc_id_from_path(source_path: &str) -> String {
    source_path
        .trim_end_matches(".md")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '.' })
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}

fn unquote_yaml_scalar(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("docs").join("index.json")
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), DocFrameError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&temp_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    fs::rename(temp_path, path)?;
    Ok(())
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
