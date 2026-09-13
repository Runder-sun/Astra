use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewArtifactKind {
    Packet,
    Trace,
}

impl ReviewArtifactKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Packet => "review_packet",
            Self::Trace => "review_trace",
        }
    }

    fn default_file_name(self) -> &'static str {
        match self {
            Self::Packet => "packet.json",
            Self::Trace => "trace.latest.json",
        }
    }
}

pub fn parse_review_artifact_ref(reference: &str) -> Option<(ReviewArtifactKind, String)> {
    let trimmed = reference.trim().trim_matches('`');
    let (prefix, suffix) = trimmed.split_once(':')?;
    let kind = match prefix.trim() {
        "review_packet" => ReviewArtifactKind::Packet,
        "review_trace" => ReviewArtifactKind::Trace,
        _ => return None,
    };
    let suffix = suffix.trim().trim_matches('`');
    if suffix.is_empty() {
        return None;
    }
    if review_id_token(suffix) {
        return Some((kind, suffix.to_string()));
    }
    review_id_from_path_like(suffix).map(|review_id| (kind, review_id))
}

pub fn canonical_review_artifact_ref(reference: &str) -> Option<String> {
    parse_review_artifact_ref(reference)
        .map(|(kind, review_id)| format!("{}:{review_id}", kind.prefix()))
}

pub fn canonical_review_artifact_ref_in_data_dir(
    data_dir: &Path,
    reference: &str,
) -> Option<String> {
    if let Some(canonical) = canonical_review_artifact_ref(reference) {
        return Some(canonical);
    }
    let trimmed = reference.trim().trim_matches('`');
    let (prefix, suffix) = trimmed.split_once(':')?;
    let kind = match prefix.trim() {
        "review_trace" => ReviewArtifactKind::Trace,
        _ => return None,
    };
    let trace_id = suffix.trim().trim_matches('`');
    if !trace_id_token(trace_id) {
        return None;
    }
    review_id_for_trace_id(data_dir, trace_id)
        .map(|review_id| format!("{}:{review_id}", kind.prefix()))
}

pub fn review_artifact_ref_workspace_path(reference: &str) -> Option<PathBuf> {
    parse_review_artifact_ref(reference).map(|(kind, review_id)| {
        PathBuf::from(".pmcli")
            .join("reviews")
            .join(review_id)
            .join(kind.default_file_name())
    })
}

pub fn review_artifact_ref_workspace_path_in_data_dir(
    data_dir: &Path,
    reference: &str,
) -> Option<PathBuf> {
    let canonical = canonical_review_artifact_ref_in_data_dir(data_dir, reference)
        .or_else(|| canonical_review_artifact_ref(reference))?;
    review_artifact_ref_workspace_path(&canonical)
}

pub fn review_artifact_path_in_data_dir(data_dir: &Path, reference: &str) -> Option<PathBuf> {
    let canonical = canonical_review_artifact_ref_in_data_dir(data_dir, reference)
        .or_else(|| canonical_review_artifact_ref(reference))?;
    let (kind, review_id) = parse_review_artifact_ref(&canonical)?;
    Some(
        data_dir
            .join("reviews")
            .join(review_id)
            .join(kind.default_file_name()),
    )
}

pub fn starts_with_review_artifact_prefix(reference: &str) -> bool {
    let trimmed = reference.trim().trim_matches('`');
    trimmed.starts_with("review_packet:") || trimmed.starts_with("review_trace:")
}

pub fn review_ids_in_text(value: &str) -> BTreeSet<String> {
    value
        .trim()
        .trim_matches('`')
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
        .filter(|token| token.len() > 4 && review_id_token(token))
        .map(str::to_string)
        .collect()
}

fn review_id_token(value: &str) -> bool {
    value.starts_with("rev_") && !value.contains('/') && !value.contains('\\')
}

fn trace_id_token(value: &str) -> bool {
    value.starts_with("trc_") && !value.contains('/') && !value.contains('\\')
}

fn review_id_from_path_like(value: &str) -> Option<String> {
    Path::new(value)
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .find(|component| review_id_token(component))
        .map(str::to_string)
}

fn review_id_for_trace_id(data_dir: &Path, trace_id: &str) -> Option<String> {
    let reviews_dir = data_dir.join("reviews");
    let mut review_ids = fs::read_dir(reviews_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| review_id_token(name))
        .collect::<Vec<_>>();
    review_ids.sort();
    for review_id in review_ids {
        let review_dir = data_dir.join("reviews").join(&review_id);
        if review_dir
            .join("traces")
            .join(format!("{trace_id}.json"))
            .exists()
        {
            return Some(review_id);
        }
        let latest = review_dir.join("trace.latest.json");
        let Ok(content) = fs::read_to_string(latest) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if value.get("trace_id").and_then(|value| value.as_str()) == Some(trace_id) {
            return Some(review_id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_review_packet_id_and_path_forms() {
        assert_eq!(
            canonical_review_artifact_ref("review_packet:rev_123"),
            Some("review_packet:rev_123".to_string())
        );
        assert_eq!(
            canonical_review_artifact_ref("review_packet:.pmcli/reviews/rev_123/packet.json"),
            Some("review_packet:rev_123".to_string())
        );
        assert_eq!(
            canonical_review_artifact_ref(
                "review_packet:/workspace/.pmcli/reviews/rev_123/packet.json"
            ),
            Some("review_packet:rev_123".to_string())
        );
    }

    #[test]
    fn resolves_review_packet_and_trace_workspace_paths() {
        assert_eq!(
            review_artifact_ref_workspace_path("review_packet:rev_123"),
            Some(PathBuf::from(".pmcli/reviews/rev_123/packet.json"))
        );
        assert_eq!(
            review_artifact_ref_workspace_path(
                "review_trace:/tmp/.pmcli/reviews/rev_123/traces/trc_123.json"
            ),
            Some(PathBuf::from(".pmcli/reviews/rev_123/trace.latest.json"))
        );
    }

    #[test]
    fn canonicalizes_review_trace_ids_with_data_dir_lookup() {
        let root = std::env::temp_dir().join(format!(
            "research_cli_review_refs_{}_{}",
            std::process::id(),
            "trace_lookup"
        ));
        let data_dir = root.join(".pmcli");
        let review_dir = data_dir.join("reviews").join("rev_123");
        fs::create_dir_all(review_dir.join("traces")).expect("review trace dir should exist");
        fs::write(
            review_dir.join("trace.latest.json"),
            r#"{"review_id":"rev_123","trace_id":"trc_456"}"#,
        )
        .expect("latest trace should write");

        assert_eq!(
            canonical_review_artifact_ref_in_data_dir(&data_dir, "review_trace:trc_456"),
            Some("review_trace:rev_123".to_string())
        );
        assert_eq!(
            review_artifact_ref_workspace_path_in_data_dir(&data_dir, "review_trace:trc_456"),
            Some(PathBuf::from(".pmcli/reviews/rev_123/trace.latest.json"))
        );
        assert_eq!(
            review_artifact_path_in_data_dir(&data_dir, "review_trace:trc_456"),
            Some(
                data_dir
                    .join("reviews")
                    .join("rev_123")
                    .join("trace.latest.json")
            )
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_review_ids_from_route_path_and_legacy_refs() {
        let ids = review_ids_in_text(
            "worker_review_failure_route::rev_123 review_packet:.pmcli/reviews/rev_456/packet.json review:rev_789#finding",
        );

        assert!(ids.contains("rev_123"));
        assert!(ids.contains("rev_456"));
        assert!(ids.contains("rev_789"));
        assert_eq!(ids.len(), 3);
    }
}
