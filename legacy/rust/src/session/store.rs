use super::identity::SessionIdentity;
use super::index::ResumeSelector;
use super::lineage::SessionLineageRecord;
use super::transcript::TranscriptLine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SessionStore {
    data_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub identity: SessionIdentity,
    pub lineage: SessionLineageRecord,
    pub transcript_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionStats {
    pub session_count: usize,
    pub active_count: usize,
    pub titled_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionPruneResult {
    pub dry_run: bool,
    pub candidate_session_ids: Vec<String>,
    pub pruned_session_ids: Vec<String>,
    pub blocking_conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionSearchHit {
    pub session_id: String,
    pub title: Option<String>,
    pub score: usize,
    pub source_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionOperatorLogManifest {
    pub session_id: String,
    pub project_id: String,
    pub request_seq: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_stream_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results_log_path: Option<String>,
    pub transcript_source: String,
    pub consistency_state: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DerivedSessionReadModel {
    pub model_id: String,
    pub project_id: String,
    pub model_kind: String,
    pub storage_path: String,
    pub authoritative: bool,
    pub rebuildable: bool,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionLogsResult {
    pub project_id: String,
    pub session_id: String,
    pub operator_logs: Vec<SessionOperatorLogManifest>,
    pub derived_read_models: Vec<DerivedSessionReadModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumeRecap {
    pub session_id: String,
    pub message_count: usize,
    pub shown_exchange_count: usize,
    pub truncated: bool,
    pub user_preview: Vec<String>,
    pub assistant_preview: Vec<String>,
    pub tool_summary: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderSessionSource {
    pub source_id: String,
    pub session_id: String,
    pub provider_id: String,
    pub source_family: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_locator: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_affinity: String,
    pub vendor_resume_supported: bool,
    pub happy_attach_supported: bool,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptSourceEnvelope {
    pub envelope_id: String,
    pub session_id: String,
    pub transcript_family: String,
    pub transcript_locator: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub projection_line: String,
    pub low_watermark_seq: u64,
    pub high_watermark_seq: u64,
    pub consistency_state: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInspection {
    pub session_id: String,
    pub session: SessionIdentity,
    pub recap: ResumeRecap,
    pub lineage: Vec<SessionLineageRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_source: Option<ProviderSessionSource>,
    pub transcript_source: TranscriptSourceEnvelope,
    pub operator_logs: Vec<SessionOperatorLogManifest>,
    pub section_status: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectInspection {
    pub project_id: String,
    pub registry_entry: Value,
    pub init_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_continuity: Option<serde_json::Value>,
    pub degraded_features: Vec<String>,
    pub section_status: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompactResult {
    pub session_id: String,
    pub compaction_record_id: String,
    pub summary_ref: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mission_frame_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mission_frame: Option<crate::goals::MissionFrameProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_context: Option<crate::docs::DocContextProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research_context: Option<crate::research::ResearchContextProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_outputs: Option<crate::skills::SkillOutputContextProjection>,
    pub promotion_candidate_id: String,
    pub promotion_queue_status: String,
    pub digest_candidate_id: String,
    pub digest_candidate_status: String,
    pub projectops_tick_id: String,
    pub silent_summary_staged: bool,
    pub durable_memory_promoted: bool,
    pub resume_recap_ref: String,
    pub compacted_turn_count: usize,
    pub raw_log_retained: bool,
    pub lineage_ok: bool,
    pub derived_views_updated: Vec<String>,
    pub deferred_repairs: Vec<String>,
}

impl SessionStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn create_session(
        &self,
        title: Option<String>,
    ) -> Result<SessionIdentity, SessionStoreError> {
        self.create_session_with_kind(title, "interactive")
    }

    pub fn create_session_with_kind(
        &self,
        title: Option<String>,
        session_kind: &str,
    ) -> Result<SessionIdentity, SessionStoreError> {
        self.create_session_with_id_and_kind(generate_session_id(), title, session_kind)
    }

    pub fn create_session_with_id(
        &self,
        session_id: String,
        title: Option<String>,
    ) -> Result<SessionIdentity, SessionStoreError> {
        self.create_session_with_id_and_kind(session_id, title, "interactive")
    }

    pub fn create_session_with_id_and_kind(
        &self,
        session_id: String,
        title: Option<String>,
        session_kind: &str,
    ) -> Result<SessionIdentity, SessionStoreError> {
        let now = timestamp_string();
        let identity = SessionIdentity {
            session_id: session_id.clone(),
            title,
            created_at: now.clone(),
            updated_at: now.clone(),
            status: "active".to_string(),
            session_kind: session_kind.to_string(),
        };
        let lineage = SessionLineageRecord {
            session_id: session_id.clone(),
            parent_session_id: None,
            resumed_from_session_id: None,
            child_session_id: None,
            trigger: None,
            summary_ref: None,
            created_at: now.clone(),
            updated_at: now,
        };

        let session_dir = self.session_dir(&session_id);
        fs::create_dir_all(&session_dir)?;
        self.write_json(self.identity_path(&session_id), &identity)?;
        self.write_json(self.lineage_path(&session_id), &lineage)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.transcript_path(&session_id))?;

        Ok(identity)
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionIdentity>, SessionStoreError> {
        if !self.sessions_root().exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in fs::read_dir(self.sessions_root())? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let contents = fs::read_to_string(entry.path().join("session.json"))?;
                sessions.push(serde_json::from_str::<SessionIdentity>(&contents)?);
            }
        }
        sessions.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| right.session_id.cmp(&left.session_id))
        });
        Ok(sessions)
    }

    pub fn load_session(&self, session_id: &str) -> Result<StoredSession, SessionStoreError> {
        let identity = self.read_json(self.identity_path(session_id))?;
        let lineage = self.read_json(self.lineage_path(session_id))?;
        Ok(StoredSession {
            identity,
            lineage,
            transcript_path: self.transcript_path(session_id),
        })
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), SessionStoreError> {
        self.load_session(session_id)?;
        fs::remove_dir_all(self.session_dir(session_id))?;
        Ok(())
    }

    pub fn rename_session(
        &self,
        session_id: &str,
        title: Option<String>,
    ) -> Result<SessionIdentity, SessionStoreError> {
        let mut stored = self.load_session(session_id)?.identity;
        stored.title = title;
        stored.updated_at = timestamp_string();
        self.write_json(self.identity_path(session_id), &stored)?;
        Ok(stored)
    }

    pub fn append_line(
        &self,
        session_id: &str,
        line: TranscriptLine,
    ) -> Result<(), SessionStoreError> {
        let transcript_path = self.transcript_path(session_id);
        if let Some(parent) = transcript_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)?;
        serde_json::to_writer(&mut file, &line)?;
        file.write_all(b"\n")?;

        let mut identity = self.load_session(session_id)?.identity;
        identity.updated_at = timestamp_string();
        self.write_json(self.identity_path(session_id), &identity)?;
        Ok(())
    }

    pub fn read_transcript(
        &self,
        session_id: &str,
    ) -> Result<Vec<TranscriptLine>, SessionStoreError> {
        let path = self.transcript_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(path)?;
        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Ok(serde_json::from_str::<TranscriptLine>(line)?))
            .collect()
    }

    pub fn resolve_resume_target(
        &self,
        selector: ResumeSelector,
    ) -> Result<SessionIdentity, SessionStoreError> {
        let sessions = self.list_sessions()?;
        match selector {
            ResumeSelector::Latest => sessions
                .into_iter()
                .next()
                .ok_or(SessionStoreError::NoSessions),
            ResumeSelector::Exact(session_id) => sessions
                .into_iter()
                .find(|identity| identity.session_id == session_id)
                .ok_or(SessionStoreError::UnknownSession(session_id)),
            ResumeSelector::Prefix(prefix) => {
                let matches: Vec<_> = sessions
                    .into_iter()
                    .filter(|identity| identity.session_id.starts_with(&prefix))
                    .collect();
                match matches.len() {
                    0 => Err(SessionStoreError::UnknownSession(prefix)),
                    1 => Ok(matches.into_iter().next().expect("exactly one match")),
                    _ => Err(SessionStoreError::AmbiguousSessionSelector(prefix)),
                }
            }
        }
    }

    pub fn browse_sessions(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<SessionIdentity>, SessionStoreError> {
        let mut sessions = self.list_sessions()?;
        if let Some(limit) = limit {
            sessions.truncate(limit);
        }
        Ok(sessions)
    }

    pub fn export_session(&self, session_id: &str) -> Result<ExportedSession, SessionStoreError> {
        let stored = self.load_session(session_id)?;
        let transcript = self.read_transcript(session_id)?;
        Ok(ExportedSession {
            session: stored.identity,
            lineage: stored.lineage,
            transcript,
            transcript_path: stored.transcript_path,
            operator_logs: self.session_logs(session_id, "")?.operator_logs,
        })
    }

    pub fn stats(&self) -> Result<SessionStats, SessionStoreError> {
        let sessions = self.list_sessions()?;
        Ok(SessionStats {
            session_count: sessions.len(),
            active_count: sessions
                .iter()
                .filter(|session| session.status == "active")
                .count(),
            titled_count: sessions
                .iter()
                .filter(|session| session.title.is_some())
                .count(),
        })
    }

    pub fn prune_sessions(&self, apply: bool) -> Result<SessionPruneResult, SessionStoreError> {
        let sessions = self.list_sessions()?;
        let candidate_session_ids = sessions
            .iter()
            .filter(|session| session.status != "active")
            .map(|session| session.session_id.clone())
            .collect::<Vec<_>>();
        let mut pruned_session_ids = Vec::new();
        if apply {
            for session_id in &candidate_session_ids {
                self.delete_session(session_id)?;
                pruned_session_ids.push(session_id.clone());
            }
        }
        Ok(SessionPruneResult {
            dry_run: !apply,
            candidate_session_ids,
            pruned_session_ids,
            blocking_conflicts: Vec::new(),
        })
    }

    pub fn search_sessions(&self, query: &str) -> Result<Vec<SessionSearchHit>, SessionStoreError> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let mut hits = Vec::new();
        for session in self.list_sessions()? {
            let mut score = 0usize;
            let mut source_tags = Vec::new();

            if session.session_id.to_lowercase().contains(&query) {
                score += 3;
                source_tags.push("session_id".to_string());
            }

            if session
                .title
                .as_ref()
                .map(|title| title.to_lowercase().contains(&query))
                .unwrap_or(false)
            {
                score += 5;
                source_tags.push("title".to_string());
            }

            let transcript = self.read_transcript(&session.session_id)?;
            if transcript
                .iter()
                .any(|line| transcript_line_text(line).contains(&query))
            {
                score += 2;
                source_tags.push("transcript".to_string());
            }

            if score > 0 {
                hits.push(SessionSearchHit {
                    session_id: session.session_id,
                    title: session.title,
                    score,
                    source_tags,
                });
            }
        }

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(hits)
    }

    pub fn session_logs(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<SessionLogsResult, SessionStoreError> {
        self.session_logs_filtered(session_id, project_id, None, None)
    }

    pub fn session_logs_filtered(
        &self,
        session_id: &str,
        project_id: &str,
        request_seq: Option<usize>,
        kind: Option<&str>,
    ) -> Result<SessionLogsResult, SessionStoreError> {
        let session = self.load_session(session_id)?;
        let transcript = self.read_transcript(session_id)?;
        let project_id = project_id.to_string();
        let logs_root = self.session_dir(session_id).join("logs");
        fs::create_dir_all(&logs_root)?;

        let requests = build_request_logs(&transcript);
        let mut manifests = Vec::new();

        for request in requests {
            if request_seq
                .map(|value| value != request.request_seq)
                .unwrap_or(false)
            {
                continue;
            }

            let request_log_path = if matches!(kind, None | Some("request")) {
                Some(write_log_lines(
                    logs_root.join(format!("request_{:04}.jsonl", request.request_seq)),
                    &request.request_lines,
                )?)
            } else {
                None
            };
            let response_stream_path = if matches!(kind, None | Some("response_stream")) {
                Some(write_log_lines(
                    logs_root.join(format!("response_stream_{:04}.jsonl", request.request_seq)),
                    &request.response_stream_lines,
                )?)
            } else {
                None
            };
            let response_log_path = if matches!(kind, None | Some("response")) {
                Some(write_log_lines(
                    logs_root.join(format!("response_{:04}.jsonl", request.request_seq)),
                    &request.response_lines,
                )?)
            } else {
                None
            };
            let tool_results_log_path = if matches!(kind, None | Some("tool_results")) {
                Some(write_log_lines(
                    logs_root.join(format!("tool_results_{:04}.jsonl", request.request_seq)),
                    &request.tool_result_lines,
                )?)
            } else {
                None
            };

            manifests.push(SessionOperatorLogManifest {
                session_id: session.identity.session_id.clone(),
                project_id: project_id.clone(),
                request_seq: request.request_seq,
                request_log_path,
                response_stream_path,
                response_log_path,
                tool_results_log_path,
                transcript_source: session.transcript_path.display().to_string(),
                consistency_state: "derived_from_transcript".to_string(),
                recorded_at: timestamp_string(),
            });
        }

        Ok(SessionLogsResult {
            project_id: project_id.clone(),
            session_id: session.identity.session_id,
            operator_logs: manifests,
            derived_read_models: vec![DerivedSessionReadModel {
                model_id: format!("session_logs_manifest:{session_id}"),
                project_id,
                model_kind: "session_operator_logs".to_string(),
                storage_path: logs_root.display().to_string(),
                authoritative: false,
                rebuildable: true,
                refreshed_at: timestamp_string(),
            }],
        })
    }

    pub fn inspect_session(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<SessionInspection, SessionStoreError> {
        let stored = self.load_session(session_id)?;
        let transcript = self.read_transcript(session_id)?;
        let recap = build_resume_recap(&stored.identity.session_id, &transcript);
        let logs = self.session_logs(session_id, project_id)?;
        let transcript_source = TranscriptSourceEnvelope {
            envelope_id: format!("transcript_source:{session_id}"),
            session_id: session_id.to_string(),
            transcript_family: "jsonl".to_string(),
            transcript_locator: stored.transcript_path.display().to_string(),
            projection_line: "canonical_transcript".to_string(),
            low_watermark_seq: 0,
            high_watermark_seq: transcript.len() as u64,
            consistency_state: "authoritative".to_string(),
            updated_at: stored.identity.updated_at.clone(),
        };
        Ok(SessionInspection {
            session_id: stored.identity.session_id.clone(),
            session: stored.identity,
            recap,
            lineage: vec![stored.lineage],
            provider_source: None,
            transcript_source,
            operator_logs: logs.operator_logs,
            section_status: serde_json::json!({
                "provider": "not_graduated",
                "permission": "not_graduated"
            }),
        })
    }

    pub fn build_resume_recap_for_session(
        &self,
        session_id: &str,
    ) -> Result<ResumeRecap, SessionStoreError> {
        Ok(build_resume_recap(
            session_id,
            &self.read_transcript(session_id)?,
        ))
    }

    pub fn compact_session(
        &self,
        session_id: &str,
        project_id: &str,
        mission_frame: Option<crate::goals::MissionFrameProjection>,
        doc_context: Option<crate::docs::DocContextProjection>,
        research_context: Option<crate::research::ResearchContextProjection>,
        skill_outputs: Option<crate::skills::SkillOutputContextProjection>,
    ) -> Result<CompactResult, SessionStoreError> {
        let stored = self.load_session(session_id)?;
        let transcript = self.read_transcript(session_id)?;
        let compacted_turn_count = transcript
            .iter()
            .filter(|line| matches!(line, TranscriptLine::Message { role, .. } if role == "user"))
            .count();
        let summary_ref = format!("summary_{session_id}_{}", timestamp_string());
        let resume_recap_ref = format!("resume_recap_{session_id}_{}", timestamp_string());
        let compaction_record_id = format!("cmp_{}_{}", session_id, timestamp_string());
        let summary_markdown = build_summary_markdown(
            &self.data_dir,
            session_id,
            &transcript,
            mission_frame.as_ref(),
            doc_context.as_ref(),
            research_context.as_ref(),
            skill_outputs.as_ref(),
        );

        self.append_line(
            session_id,
            TranscriptLine::SummaryReference {
                summary_ref: summary_ref.clone(),
            },
        )?;

        let mut lineage = stored.lineage;
        lineage.trigger = Some("compaction".to_string());
        lineage.summary_ref = Some(summary_ref.clone());
        lineage.updated_at = timestamp_string();
        self.write_json(self.lineage_path(session_id), &lineage)?;
        self.write_summary_markdown(session_id, &summary_ref, &summary_markdown)?;
        let promotion_candidate_id = crate::artifacts::stage_summary_candidate(
            &self.data_dir,
            session_id,
            &summary_ref,
            &self.summary_markdown_path(session_id, &summary_ref),
        )
        .map_err(|err| SessionStoreError::Io(std::io::Error::other(err.to_string())))?;
        let digest_staging = crate::projectops::stage_session_compaction_digest(
            &self.data_dir,
            project_id,
            session_id,
            &summary_ref,
            &self.summary_markdown_path(session_id, &summary_ref),
        )
        .map_err(|err| SessionStoreError::Io(std::io::Error::other(err.to_string())))?;

        let recap = build_resume_recap(session_id, &self.read_transcript(session_id)?);
        self.write_json(
            self.resume_recap_path(session_id, &resume_recap_ref),
            &recap,
        )?;

        Ok(CompactResult {
            session_id: session_id.to_string(),
            compaction_record_id,
            summary_ref,
            mission_frame_ref: mission_frame
                .as_ref()
                .map(|frame| frame.mission_frame_ref.clone())
                .unwrap_or_default(),
            mission_frame,
            doc_context,
            research_context,
            skill_outputs,
            promotion_candidate_id,
            promotion_queue_status: "pending_review".to_string(),
            digest_candidate_id: digest_staging.digest_candidate.candidate_id,
            digest_candidate_status: digest_staging.digest_candidate.status,
            projectops_tick_id: digest_staging.projectops_tick.tick_id,
            silent_summary_staged: true,
            durable_memory_promoted: false,
            resume_recap_ref,
            compacted_turn_count,
            raw_log_retained: true,
            lineage_ok: true,
            derived_views_updated: vec![
                "resume_recap".to_string(),
                "session_search_index".to_string(),
                "doc_context_projection".to_string(),
                "research_context_projection".to_string(),
                "skill_output_context".to_string(),
            ],
            deferred_repairs: Vec::new(),
        })
    }

    /// LLM-driven compaction: generates a structured summary using the configured provider.
    ///
    /// Head (first 2 turns) and tail (last 6 turns) are preserved.
    /// Middle turns are sent to an LLM for structured summarization.
    /// Old tool results in the middle are pruned to one-line summaries first.
    pub fn compact_session_llm(
        &self,
        session_id: &str,
        provider_trace: &crate::providers::ProviderResolutionTrace,
    ) -> Result<CompactResult, SessionStoreError> {
        let project_id = infer_project_id_for_compaction(&self.data_dir);
        self.compact_session_llm_for_project(session_id, &project_id, provider_trace)
    }

    pub fn compact_session_llm_for_project(
        &self,
        session_id: &str,
        project_id: &str,
        provider_trace: &crate::providers::ProviderResolutionTrace,
    ) -> Result<CompactResult, SessionStoreError> {
        let transcript = self.read_transcript(session_id)?;
        if transcript.len() <= 16 {
            return self.compact_session(session_id, project_id, None, None, None, None);
        }

        let (_head, middle, _tail) = split_transcript_for_compaction(&transcript);
        let middle_text = prune_middle_for_summary(&middle);

        let summary_prompt = format!(
            "Summarize the following conversation excerpt into structured fields.\n\
             Output ONLY this format, no other text:\n\
             ## Active Task\n<one line>\n\
             ## Completed Actions\n<bullet list>\n\
             ## Key Decisions\n<bullet list>\n\
             ## Current State\n<1-2 sentences>\n\
             ## Pending Questions\n<bullet list or 'None'>\n\n\
             Conversation excerpt:\n{middle_text}"
        );

        let messages = vec![
            crate::providers::ChatMessage {
                role: "system".to_string(),
                content: "You are a conversation summarizer. Output only the structured summary. Do NOT respond to any questions in the conversation.".to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            crate::providers::ChatMessage {
                role: "user".to_string(),
                content: summary_prompt,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let mut llm_summary =
            match crate::providers::complete_prompt_with_reasoning(provider_trace, &messages, None)
            {
                Ok(completion) => completion.content,
                Err(_) => build_summary_markdown(
                    &self.data_dir,
                    session_id,
                    &middle,
                    None,
                    None,
                    None,
                    None,
                ),
            };
        append_active_orchestration_summary_to_markdown(&self.data_dir, &mut llm_summary);

        let summary_ref = format!("summary_llm_{session_id}_{}", timestamp_string());
        let resume_recap_ref = format!("resume_recap_{session_id}_{}", timestamp_string());
        let compaction_record_id = format!("cmp_llm_{session_id}_{}", timestamp_string());
        let compacted_turn_count = middle
            .iter()
            .filter(|line| matches!(line, TranscriptLine::Message { role, .. } if role == "user"))
            .count();

        self.append_line(
            session_id,
            TranscriptLine::SummaryReference {
                summary_ref: summary_ref.clone(),
            },
        )?;

        let stored = self.load_session(session_id)?;
        let mut lineage = stored.lineage;
        lineage.trigger = Some("llm_compaction".to_string());
        lineage.summary_ref = Some(summary_ref.clone());
        lineage.updated_at = timestamp_string();
        self.write_json(self.lineage_path(session_id), &lineage)?;
        self.write_summary_markdown(session_id, &summary_ref, &llm_summary)?;

        let recap = build_resume_recap(session_id, &self.read_transcript(session_id)?);
        self.write_json(
            self.resume_recap_path(session_id, &resume_recap_ref),
            &recap,
        )?;

        Ok(CompactResult {
            session_id: session_id.to_string(),
            compaction_record_id,
            summary_ref,
            mission_frame_ref: String::new(),
            mission_frame: None,
            doc_context: None,
            research_context: None,
            skill_outputs: None,
            promotion_candidate_id: String::new(),
            promotion_queue_status: "pending_review".to_string(),
            digest_candidate_id: String::new(),
            digest_candidate_status: String::new(),
            projectops_tick_id: String::new(),
            silent_summary_staged: true,
            durable_memory_promoted: false,
            resume_recap_ref,
            compacted_turn_count,
            raw_log_retained: true,
            lineage_ok: true,
            derived_views_updated: vec![
                "resume_recap".to_string(),
                "session_search_index".to_string(),
            ],
            deferred_repairs: Vec::new(),
        })
    }

    fn sessions_root(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_root().join(session_id)
    }

    fn identity_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("session.json")
    }

    fn lineage_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("lineage.json")
    }

    fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("transcript.jsonl")
    }

    fn summaries_root(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("summaries")
    }

    fn summary_markdown_path(&self, session_id: &str, summary_ref: &str) -> PathBuf {
        self.summaries_root(session_id)
            .join(format!("{summary_ref}.md"))
    }

    fn resume_recap_path(&self, session_id: &str, resume_recap_ref: &str) -> PathBuf {
        self.session_dir(session_id)
            .join(format!("{resume_recap_ref}.json"))
    }

    fn write_json<T>(&self, path: PathBuf, value: &T) -> Result<(), SessionStoreError>
    where
        T: Serialize,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(value)?)?;
        Ok(())
    }

    fn read_json<T>(&self, path: PathBuf) -> Result<T, SessionStoreError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let contents = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&contents)?)
    }

    fn write_summary_markdown(
        &self,
        session_id: &str,
        summary_ref: &str,
        summary: &str,
    ) -> Result<(), SessionStoreError> {
        let path = self.summary_markdown_path(session_id, summary_ref);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, summary)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportedSession {
    pub session: SessionIdentity,
    pub lineage: SessionLineageRecord,
    pub transcript: Vec<TranscriptLine>,
    pub transcript_path: PathBuf,
    pub operator_logs: Vec<SessionOperatorLogManifest>,
}

#[derive(Debug, Clone)]
struct SessionRequestLogs {
    request_seq: usize,
    request_lines: Vec<TranscriptLine>,
    response_stream_lines: Vec<TranscriptLine>,
    response_lines: Vec<TranscriptLine>,
    tool_result_lines: Vec<TranscriptLine>,
}

fn transcript_line_text(line: &TranscriptLine) -> String {
    match line {
        TranscriptLine::Control { event } => event.to_lowercase(),
        TranscriptLine::Message { role, content } => {
            format!("{} {}", role.to_lowercase(), content.to_lowercase())
        }
        TranscriptLine::ToolCall {
            tool_name,
            arguments,
            ..
        } => {
            format!("{} {}", tool_name.to_lowercase(), arguments.to_lowercase())
        }
        TranscriptLine::ToolResult {
            tool_name, output, ..
        } => {
            format!("{} {}", tool_name.to_lowercase(), output.to_lowercase())
        }
        TranscriptLine::SummaryReference { summary_ref } => summary_ref.to_lowercase(),
    }
}

fn infer_project_id_for_compaction(data_dir: &Path) -> String {
    let registry_entry = data_dir.join("registry_entry.json");
    if let Ok(content) = fs::read_to_string(&registry_entry) {
        if let Ok(value) = serde_json::from_str::<Value>(&content) {
            if let Some(project_id) = value.get("project_id").and_then(Value::as_str) {
                if !project_id.trim().is_empty() {
                    return project_id.to_string();
                }
            }
        }
    }
    data_dir
        .parent()
        .and_then(|workspace| crate::projects::registry::stable_project_id(workspace).ok())
        .unwrap_or_else(|| "unknown_project".to_string())
}

fn build_resume_recap(session_id: &str, transcript: &[TranscriptLine]) -> ResumeRecap {
    let mut user_preview = Vec::new();
    let mut assistant_preview = Vec::new();
    let mut tool_summary = Vec::new();

    for line in transcript.iter().rev() {
        match line {
            TranscriptLine::Message { role, content }
                if role == "user" && user_preview.len() < 3 =>
            {
                user_preview.push(content.clone());
            }
            TranscriptLine::Message { role, content }
                if role == "assistant" && assistant_preview.len() < 3 =>
            {
                assistant_preview.push(content.clone());
            }
            TranscriptLine::ToolResult {
                tool_name, output, ..
            } if tool_summary.len() < 3 => {
                tool_summary.push(format!("{tool_name}: {output}"));
            }
            _ => {}
        }
    }

    user_preview.reverse();
    assistant_preview.reverse();
    tool_summary.reverse();

    ResumeRecap {
        session_id: session_id.to_string(),
        message_count: transcript.len(),
        shown_exchange_count: user_preview.len().min(assistant_preview.len()),
        truncated: transcript.len()
            > (user_preview.len() + assistant_preview.len() + tool_summary.len()),
        user_preview,
        assistant_preview,
        tool_summary,
    }
}

/// Split transcript into head/middle/tail for LLM compaction.
/// Head: first 2 user turns (preserved). Tail: last 6 user turns (preserved).
/// Middle: everything else (summarized by LLM).
fn split_transcript_for_compaction(
    transcript: &[TranscriptLine],
) -> (
    Vec<TranscriptLine>,
    Vec<TranscriptLine>,
    Vec<TranscriptLine>,
) {
    let user_turn_indices: Vec<usize> = transcript
        .iter()
        .enumerate()
        .filter(|(_, line)| matches!(line, TranscriptLine::Message { role, .. } if role == "user"))
        .map(|(i, _)| i)
        .collect();

    if user_turn_indices.len() <= 8 {
        // Not enough turns to split meaningfully
        return (Vec::new(), transcript.to_vec(), Vec::new());
    }

    let head_end = user_turn_indices.get(2).copied().unwrap_or(0);
    let tail_start = user_turn_indices
        .get(user_turn_indices.len().saturating_sub(6))
        .copied()
        .unwrap_or(0);

    let head = transcript[..head_end].to_vec();
    let middle = transcript[head_end..tail_start].to_vec();
    let tail = transcript[tail_start..].to_vec();

    (head, middle, tail)
}

/// Prune tool results in middle section to one-line summaries for compaction.
fn prune_middle_for_summary(middle: &[TranscriptLine]) -> String {
    let mut parts = Vec::new();
    for line in middle {
        match line {
            TranscriptLine::Control { .. } => {}
            TranscriptLine::Message { role, content } => {
                let truncated = if content.len() > 500 {
                    format!("{}...", &content[..500])
                } else {
                    content.clone()
                };
                parts.push(format!("[{role}] {truncated}"));
            }
            TranscriptLine::ToolCall { tool_name, .. } => {
                parts.push(format!("[tool_call] {tool_name}"));
            }
            TranscriptLine::ToolResult {
                tool_name, output, ..
            } => {
                let first_line = output.lines().next().unwrap_or("");
                let summary = if first_line.len() > 100 {
                    format!("{}...", &first_line[..100])
                } else {
                    first_line.to_string()
                };
                parts.push(format!("[{tool_name}] {summary}"));
            }
            TranscriptLine::SummaryReference { summary_ref } => {
                parts.push(format!("[summary] {summary_ref}"));
            }
        }
    }
    let text = parts.join("\n");
    // Cap at ~20K chars to avoid overflowing the summarizer's context window
    if text.len() > 20_000 {
        format!(
            "{}...\n[truncated, {}/middle lines shown]",
            &text[..20_000],
            middle.len()
        )
    } else {
        text
    }
}

fn build_summary_markdown(
    data_dir: &Path,
    session_id: &str,
    transcript: &[TranscriptLine],
    mission_frame: Option<&crate::goals::MissionFrameProjection>,
    doc_context: Option<&crate::docs::DocContextProjection>,
    research_context: Option<&crate::research::ResearchContextProjection>,
    skill_outputs: Option<&crate::skills::SkillOutputContextProjection>,
) -> String {
    let mut lines = vec![
        format!("# Session Summary: {session_id}"),
        String::new(),
        "This summary is a derived compaction artifact. Raw transcript remains authoritative."
            .to_string(),
        String::new(),
    ];

    if let Some(frame) = mission_frame {
        lines.extend([
            "## MissionFrame Projection".to_string(),
            String::new(),
            format!("- mission_frame_ref: {}", frame.mission_frame_ref),
            format!("- project_max_goal: {}", frame.project_max_goal),
            format!("- milestone_goal: {}", frame.milestone_goal),
            format!(
                "- current_implementation_goal: {}",
                frame.current_implementation_goal
            ),
            format!("- priority_rule: {}", frame.priority_rule),
            String::new(),
        ]);
    }

    if let Some(context) = doc_context {
        lines.extend([
            "## Active DocFrames".to_string(),
            String::new(),
            format!("- active_count: {}", context.active_count),
            format!("- stale_count: {}", context.stale_count),
        ]);
        for frame in context.doc_frames.iter().take(5) {
            lines.push(format!(
                "- {}: {} ({})\n",
                frame.doc_id, frame.summary, frame.source_path
            ));
        }
        lines.push(String::new());
    }

    if let Some(context) = research_context {
        lines.extend([
            "## ResearchContext Projection".to_string(),
            String::new(),
            format!("- active_thread_id: {}", context.active_thread_id),
            format!("- active_thread_title: {}", context.active_thread_title),
            format!(
                "- active_stage_execution_id: {}",
                context.active_stage_execution_id
            ),
            format!("- active_stage_id: {}", context.active_stage_id),
            format!(
                "- active_deliberation_span_id: {}",
                context.active_deliberation_span_id
            ),
            format!(
                "- active_deliberation_mode: {}",
                context.active_deliberation_mode
            ),
            format!(
                "- next_recommended_action: {}",
                context.next_recommended_action
            ),
            format!("- projection_policy: {}", context.projection_policy),
            String::new(),
        ]);
    }

    if let Some(outputs) = skill_outputs {
        lines.extend([
            "## SkillOutput Context".to_string(),
            String::new(),
            format!("- projection_policy: {}", outputs.projection_policy),
            format!("- public_latest_count: {}", outputs.public_latest.len()),
            format!(
                "- omitted_candidate_count: {}",
                outputs.omitted_candidate_count
            ),
            String::new(),
        ]);
    }

    append_active_orchestration_summary(data_dir, &mut lines);

    lines.push("## Recent Highlights".to_string());

    let mut recent_items = Vec::new();
    for line in transcript
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        match line {
            TranscriptLine::Message { role, content } => {
                recent_items.push(format!("- {role}: {content}"));
            }
            TranscriptLine::ToolCall {
                tool_name,
                arguments,
                ..
            } => {
                recent_items.push(format!("- tool_call {tool_name}: {arguments}"));
            }
            TranscriptLine::ToolResult {
                tool_name, output, ..
            } => {
                recent_items.push(format!("- tool_result {tool_name}: {output}"));
            }
            TranscriptLine::Control { event } => {
                recent_items.push(format!("- control: {event}"));
            }
            TranscriptLine::SummaryReference { summary_ref } => {
                recent_items.push(format!("- summary_reference: {summary_ref}"));
            }
        }
    }

    if recent_items.is_empty() {
        lines.push("- no transcript lines recorded".to_string());
    } else {
        lines.extend(recent_items);
    }

    lines.join("\n")
}

fn append_active_orchestration_summary(data_dir: &Path, lines: &mut Vec<String>) {
    if let Some(orchestration) =
        crate::orchestration::active_orchestration_summary_markdown(data_dir)
    {
        lines.extend([orchestration, String::new()]);
    }
}

fn append_active_orchestration_summary_to_markdown(data_dir: &Path, markdown: &mut String) {
    if let Some(orchestration) =
        crate::orchestration::active_orchestration_summary_markdown(data_dir)
    {
        if !markdown.ends_with('\n') {
            markdown.push('\n');
        }
        markdown.push('\n');
        markdown.push_str(&orchestration);
        markdown.push('\n');
    }
}

fn build_request_logs(transcript: &[TranscriptLine]) -> Vec<SessionRequestLogs> {
    let mut logs = Vec::new();
    let mut current: Option<SessionRequestLogs> = None;

    for line in transcript {
        match line {
            TranscriptLine::Message { role, .. } if role == "user" => {
                if let Some(existing) = current.take() {
                    logs.push(existing);
                }
                let request_seq = logs.len() + 1;
                current = Some(SessionRequestLogs {
                    request_seq,
                    request_lines: vec![line.clone()],
                    response_stream_lines: Vec::new(),
                    response_lines: Vec::new(),
                    tool_result_lines: Vec::new(),
                });
            }
            TranscriptLine::ToolResult { .. } => {
                if current.is_none() {
                    current = Some(SessionRequestLogs {
                        request_seq: logs.len() + 1,
                        request_lines: Vec::new(),
                        response_stream_lines: Vec::new(),
                        response_lines: Vec::new(),
                        tool_result_lines: Vec::new(),
                    });
                }
                if let Some(active) = current.as_mut() {
                    active.tool_result_lines.push(line.clone());
                }
            }
            TranscriptLine::Message { role, .. } if role == "assistant" => {
                if current.is_none() {
                    current = Some(SessionRequestLogs {
                        request_seq: logs.len() + 1,
                        request_lines: Vec::new(),
                        response_stream_lines: Vec::new(),
                        response_lines: Vec::new(),
                        tool_result_lines: Vec::new(),
                    });
                }
                if let Some(active) = current.as_mut() {
                    active.response_lines.push(line.clone());
                }
            }
            _ => {
                if current.is_none() {
                    current = Some(SessionRequestLogs {
                        request_seq: logs.len() + 1,
                        request_lines: Vec::new(),
                        response_stream_lines: Vec::new(),
                        response_lines: Vec::new(),
                        tool_result_lines: Vec::new(),
                    });
                }
                if let Some(active) = current.as_mut() {
                    active.response_stream_lines.push(line.clone());
                }
            }
        }
    }

    if let Some(existing) = current.take() {
        logs.push(existing);
    }

    logs
}

fn write_log_lines(path: PathBuf, lines: &[TranscriptLine]) -> Result<String, SessionStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let payload = lines
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<Value>, _>>()?;
    let encoded = payload
        .into_iter()
        .map(|value| serde_json::to_string(&value))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    fs::write(
        &path,
        if encoded.is_empty() {
            String::new()
        } else {
            format!("{encoded}\n")
        },
    )?;
    Ok(path.display().to_string())
}

#[derive(Debug)]
pub enum SessionStoreError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    NoSessions,
    UnknownSession(String),
    AmbiguousSessionSelector(String),
}

impl fmt::Display for SessionStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "session store io failed: {err}"),
            Self::Serde(err) => write!(f, "session store serialization failed: {err}"),
            Self::NoSessions => write!(f, "no sessions exist in this project"),
            Self::UnknownSession(session_id) => write!(f, "unknown session: {session_id}"),
            Self::AmbiguousSessionSelector(prefix) => {
                write!(f, "ambiguous session selector: {prefix}")
            }
        }
    }
}

impl std::error::Error for SessionStoreError {}

impl From<std::io::Error> for SessionStoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for SessionStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

fn generate_session_id() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    format!("sess_{nonce}")
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_millis()
        .to_string()
}
