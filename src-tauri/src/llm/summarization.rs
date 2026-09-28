use super::{
    prompts::{PromptKind, PromptRouter},
    providers::ProviderSnapshot,
    ChatCompletionRequest, ChatMessage, CompletionProvider, LlmError,
};
use crate::{contracts::MeetingType, db::Database};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const MAX_SINGLE_CONTEXT_CHARS: usize = 24_000;
const HIERARCHICAL_CHUNK_CHARS: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Citation {
    pub kind: String,
    pub ref_id: String,
    pub start_seconds: Option<f32>,
    pub end_seconds: Option<f32>,
    pub page_number: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedActionItem {
    pub text: String,
    pub assignee_speaker_id: Option<String>,
    pub assignee: Option<String>,
    pub deadline_phrase: Option<String>,
    pub normalized_deadline: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedSummary {
    pub title: Option<String>,
    pub overview: String,
    #[serde(default)]
    pub action_items: Vec<GeneratedActionItem>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub key_concepts: Vec<String>,
    #[serde(default)]
    pub assignments: Vec<String>,
    #[serde(default)]
    pub important_dates: Vec<String>,
    #[serde(default)]
    pub citations: Vec<Citation>,
    #[serde(default)]
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionItemDto {
    pub id: String,
    pub summary_revision_id: String,
    pub text: String,
    pub assignee_speaker_id: Option<String>,
    pub deadline_phrase: Option<String>,
    pub normalized_deadline: Option<String>,
    pub evidence_refs: Vec<Value>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryRevisionDto {
    pub id: String,
    pub meeting_id: String,
    pub revision: i64,
    pub meeting_type: String,
    pub structured_json: Value,
    pub markdown: String,
    pub input_revision_set: Vec<Value>,
    pub model_identity: Option<String>,
    pub prompt_identity: Option<String>,
    pub freshness: String,
    pub published_at: Option<String>,
    pub action_items: Vec<ActionItemDto>,
}

#[derive(Debug, Clone)]
struct TranscriptRow {
    id: String,
    start_seconds: f32,
    end_seconds: f32,
    text: String,
}

#[derive(Debug, Clone)]
struct SlideRow {
    id: String,
    timestamp_seconds: f32,
    ocr_text: String,
}

#[derive(Debug, Clone)]
struct OptionalState {
    kind: String,
    state: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
struct SummaryContext {
    title_origin: String,
    title_revision: i64,
    current_summary_revision_id: Option<String>,
    requested_type: MeetingType,
    transcript_revision_id: String,
    transcripts: Vec<TranscriptRow>,
    slides: Vec<SlideRow>,
    optional_states: Vec<OptionalState>,
    speakers: HashMap<String, String>,
}

#[derive(Clone)]
pub struct SummarizationService {
    database: Database,
}

impl SummarizationService {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Generate and publish a summary using an explicitly selected provider.
    /// The completion provider is injected so the pipeline is deterministic in
    /// tests and cannot silently choose a fallback destination.
    pub fn summarize_with_provider(
        &self,
        meeting_id: &str,
        snapshot: &ProviderSnapshot,
        provider: &dyn CompletionProvider,
        regenerate: bool,
    ) -> Result<SummaryRevisionDto, String> {
        let context = self.load_context(meeting_id)?;
        if let Some(revision_id) = context.current_summary_revision_id.as_deref() {
            if !regenerate {
                return self
                    .list_revisions(meeting_id)
                    .map(|revisions| {
                        revisions
                            .into_iter()
                            .find(|revision| revision.id == revision_id)
                            .ok_or_else(|| "current summary revision is unavailable".to_string())
                    })
                    .and_then(|revision| revision);
            }
        }
        for state in &context.optional_states {
            if matches!(state.state.as_str(), "pending" | "running") {
                return Err(format!(
                    "summary is waiting for {} to reach a terminal state",
                    state.kind
                ));
            }
        }
        if context.transcripts.is_empty() {
            return self.publish_empty(meeting_id, snapshot, context, regenerate);
        }
        let route = PromptRouter::route(
            &context
                .transcripts
                .iter()
                .map(|row| row.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            &context.requested_type,
        );
        let job_id = self.start_job(meeting_id, snapshot, &route, &context)?;
        let result = self.generate_payload(provider, snapshot, &route, &context);
        let payload = match result {
            Ok(payload) => payload,
            Err(error) => {
                let message = error.to_string();
                self.finish_job(&job_id, "error", Some(&message));
                return Err(error.to_string());
            }
        };
        let publish = self.publish(
            meeting_id, snapshot, route.kind, context, payload, regenerate,
        );
        match publish {
            Ok(value) => {
                self.finish_job(&job_id, "done", None);
                Ok(value)
            }
            Err(error) => {
                self.finish_job(&job_id, "error", Some(&error));
                Err(error)
            }
        }
    }

    /// The command-facing entry point. A provider must be supplied by the
    /// runtime after explicit selection; this method keeps that boundary clear.
    pub fn selected_snapshot(
        &self,
        snapshot: Option<ProviderSnapshot>,
    ) -> Result<ProviderSnapshot, String> {
        snapshot.ok_or_else(|| "select an LLM provider before generating a summary".into())
    }

    pub fn list_revisions(&self, meeting_id: &str) -> Result<Vec<SummaryRevisionDto>, String> {
        let meeting_id = meeting_id.to_owned();
        self.database
            .run(move |connection| {
                let mut statement = connection
                    .prepare("SELECT id, meeting_id, revision, meeting_type, structured_json, markdown, input_revision_set, model_identity, prompt_identity, freshness, published_at FROM summary_revisions WHERE meeting_id = ?1 ORDER BY revision DESC")
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([&meeting_id], |row| {
                        let id: String = row.get(0)?;
                        let structured_json: String = row.get(4)?;
                        let input_revision_set: String = row.get(6)?;
                        Ok(SummaryRevisionDto {
                            id: id.clone(),
                            meeting_id: row.get(1)?,
                            revision: row.get(2)?,
                            meeting_type: row.get(3)?,
                            structured_json: serde_json::from_str(&structured_json).unwrap_or(Value::Null),
                            markdown: row.get(5)?,
                            input_revision_set: serde_json::from_str(&input_revision_set).unwrap_or_default(),
                            model_identity: row.get(7)?,
                            prompt_identity: row.get(8)?,
                            freshness: row.get(9)?,
                            published_at: row.get(10)?,
                            action_items: Vec::new(),
                        })
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                let mut result = rows;
                for revision in &mut result {
                    revision.action_items = action_items_for(connection, &revision.id)?;
                }
                Ok(result)
            })
            .map_err(|error| error.to_string())
    }

    pub fn action_items(&self, revision_id: &str) -> Result<Vec<ActionItemDto>, String> {
        self.database
            .run({
                let revision_id = revision_id.to_owned();
                move |connection| action_items_for(connection, &revision_id)
            })
            .map_err(|error| error.to_string())
    }

    pub fn toggle_action_item(&self, action_item_id: &str, completed: bool) -> Result<(), String> {
        let action_item_id = action_item_id.to_owned();
        self.database
            .run(move |connection| {
                let exists: bool = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM action_items WHERE id = ?1)",
                        [&action_item_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                if !exists {
                    return Err("action item not found".into());
                }
                connection
                    .execute(
                        "INSERT INTO action_state(action_item_id, completed, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(action_item_id) DO UPDATE SET completed=excluded.completed, updated_at=excluded.updated_at",
                        (&action_item_id, completed as i64, Utc::now().to_rfc3339()),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
    }

    pub fn set_meeting_type(
        &self,
        meeting_id: &str,
        meeting_type: MeetingType,
    ) -> Result<(), String> {
        let value = match meeting_type {
            MeetingType::Auto => "auto",
            MeetingType::Meeting => "meeting",
            MeetingType::Lecture => "lecture",
        };
        let meeting_id = meeting_id.to_owned();
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE meetings SET requested_type = ?1 WHERE id = ?2",
                        (value, &meeting_id),
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|changed| {
                        if changed == 0 {
                            Err("meeting not found".into())
                        } else {
                            Ok(())
                        }
                    })
            })
            .map_err(|error| error.to_string())
    }

    pub fn set_user_title(&self, meeting_id: &str, title: &str) -> Result<(), String> {
        let title = title.trim().to_owned();
        if title.is_empty() {
            return Err("title cannot be empty".into());
        }
        let meeting_id = meeting_id.to_owned();
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE meetings SET title = ?1, title_origin = 'user', title_revision = title_revision + 1 WHERE id = ?2",
                        (&title, &meeting_id),
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|changed| {
                        if changed == 0 {
                            Err("meeting not found".into())
                        } else {
                            Ok(())
                        }
                    })
            })
            .map_err(|error| error.to_string())
    }

    fn load_context(&self, meeting_id: &str) -> Result<SummaryContext, String> {
        let meeting_id = meeting_id.to_owned();
        self.database
            .run(move |connection| {
                let (
                    title_origin,
                    title_revision,
                    requested_type,
                    transcript_revision_id,
                    current_summary_revision_id,
                ): (
                    String,
                    i64,
                    String,
                    Option<String>,
                    Option<String>,
                ) = connection
                    .query_row(
                        "SELECT title_origin, title_revision, requested_type, current_transcript_revision_id, current_summary_revision_id FROM meetings WHERE id = ?1 AND deleted_at IS NULL",
                        [&meeting_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                    )
                    .map_err(|error| error.to_string())?;
                let transcript_revision_id = transcript_revision_id
                    .ok_or_else(|| "meeting has no published transcript".to_string())?;
                let requested_type = match requested_type.as_str() {
                    "meeting" => MeetingType::Meeting,
                    "lecture" => MeetingType::Lecture,
                    _ => MeetingType::Auto,
                };
                let mut transcript_statement = connection
                    .prepare("SELECT id, start_seconds, end_seconds, text FROM transcript_segments WHERE transcript_revision_id = ?1 ORDER BY ordinal")
                    .map_err(|error| error.to_string())?;
                let transcripts = transcript_statement
                    .query_map([&transcript_revision_id], |row| {
                        Ok(TranscriptRow {
                            id: row.get(0)?,
                            start_seconds: row.get(1)?,
                            end_seconds: row.get(2)?,
                            text: row.get(3)?,
                        })
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                let mut slide_statement = connection
                    .prepare("SELECT id, COALESCE((SELECT timestamp_seconds FROM slide_occurrences WHERE slide_id = slides.id ORDER BY ordinal LIMIT 1), 0), COALESCE(ocr_text, '') FROM slides WHERE meeting_id = ?1 ORDER BY id")
                    .map_err(|error| error.to_string())?;
                let slides = slide_statement
                    .query_map([&meeting_id], |row| {
                        Ok(SlideRow {
                            id: row.get(0)?,
                            timestamp_seconds: row.get(1)?,
                            ocr_text: row.get(2)?,
                        })
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                let mut job_statement = connection
                    .prepare("SELECT kind, state, reason FROM jobs WHERE meeting_id = ?1 AND (kind LIKE 'diariz%' OR kind LIKE 'slide%' OR kind LIKE 'ocr%') ORDER BY updated_at")
                    .map_err(|error| error.to_string())?;
                let optional_states = job_statement
                    .query_map([&meeting_id], |row| {
                        Ok(OptionalState {
                            kind: row.get(0)?,
                            state: row.get(1)?,
                            reason: row.get(2)?,
                        })
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                let mut speaker_statement = connection
                    .prepare("SELECT id, COALESCE(display_name, anonymous_label) FROM speakers WHERE meeting_id = ?1")
                    .map_err(|error| error.to_string())?;
                let speakers = speaker_statement
                    .query_map([&meeting_id], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<(String, String)>, _>>()
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .collect();
                Ok(SummaryContext {
                    title_origin,
                    title_revision,
                    current_summary_revision_id,
                    requested_type,
                    transcript_revision_id,
                    transcripts,
                    slides,
                    optional_states,
                    speakers,
                })
            })
            .map_err(|error| error.to_string())
    }

    fn generate_payload(
        &self,
        provider: &dyn CompletionProvider,
        snapshot: &ProviderSnapshot,
        route: &super::prompts::PromptRoute,
        context: &SummaryContext,
    ) -> Result<GeneratedSummary, LlmError> {
        let transcript = context
            .transcripts
            .iter()
            .map(|row| {
                format!(
                    "[{} @ {:.2}-{:.2}] {}",
                    row.id, row.start_seconds, row.end_seconds, row.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let slide_text = context
            .slides
            .iter()
            .map(|slide| {
                format!(
                    "[{} @ {:.2}] {}",
                    slide.id, slide.timestamp_seconds, slide.ocr_text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let response = if transcript.len() + slide_text.len() <= MAX_SINGLE_CONTEXT_CHARS {
            self.complete(provider, snapshot, route.kind, &transcript, &slide_text)?
        } else {
            let mut intermediate = Vec::new();
            for chunk in transcript.as_bytes().chunks(HIERARCHICAL_CHUNK_CHARS) {
                let chunk = String::from_utf8_lossy(chunk);
                let prompt = format!(
                    "Summarize this sequential transcript passage into concise evidence-preserving notes. Keep action items, decisions, people, dates, and source IDs. Do not omit content because later passages may depend on it.\n\n{chunk}"
                );
                let request = ChatCompletionRequest {
                    model: snapshot.model.clone(),
                    messages: vec![
                        ChatMessage::system(PromptRouter::system_prompt(route.kind)),
                        ChatMessage::user(prompt),
                    ],
                    temperature: 0.1,
                    max_tokens: Some(4096),
                };
                intermediate.push(provider.complete(&request)?.content);
            }
            self.complete(
                provider,
                snapshot,
                route.kind,
                &intermediate.join("\n\n--- NEXT PASS ---\n\n"),
                &slide_text,
            )?
        };
        parse_generated_summary(&response)
    }

    fn complete(
        &self,
        provider: &dyn CompletionProvider,
        snapshot: &ProviderSnapshot,
        kind: PromptKind,
        transcript: &str,
        slide_text: &str,
    ) -> Result<String, LlmError> {
        provider
            .complete(&ChatCompletionRequest {
                model: snapshot.model.clone(),
                messages: vec![
                    ChatMessage::system(PromptRouter::system_prompt(kind)),
                    ChatMessage::user(PromptRouter::user_prompt(kind, transcript, slide_text)),
                ],
                temperature: 0.1,
                max_tokens: Some(4096),
            })
            .map(|response| response.content)
    }

    fn publish_empty(
        &self,
        meeting_id: &str,
        snapshot: &ProviderSnapshot,
        context: SummaryContext,
        regenerate: bool,
    ) -> Result<SummaryRevisionDto, String> {
        let mut payload = GeneratedSummary {
            title: None,
            overview: "No speech was detected in this recording.".into(),
            action_items: Vec::new(),
            decisions: Vec::new(),
            key_concepts: Vec::new(),
            assignments: Vec::new(),
            important_dates: Vec::new(),
            citations: Vec::new(),
            disclosures: vec!["The transcript contains no speech segments.".into()],
        };
        if let Some(state) = context
            .optional_states
            .iter()
            .find(|state| state.state == "error")
        {
            payload.disclosures.push(format!(
                "{} was unavailable: {}",
                state.kind,
                state
                    .reason
                    .clone()
                    .unwrap_or_else(|| "unknown error".into())
            ));
        }
        self.publish(
            meeting_id,
            snapshot,
            PromptKind::Generic,
            context,
            payload,
            regenerate,
        )
    }

    fn publish(
        &self,
        meeting_id: &str,
        snapshot: &ProviderSnapshot,
        kind: PromptKind,
        context: SummaryContext,
        mut payload: GeneratedSummary,
        _regenerate: bool,
    ) -> Result<SummaryRevisionDto, String> {
        validate_payload(&payload, &context)?;
        for state in &context.optional_states {
            if state.state == "error" {
                payload.disclosures.push(format!(
                    "{} output was unavailable: {}",
                    state.kind,
                    state
                        .reason
                        .clone()
                        .unwrap_or_else(|| "unknown error".into())
                ));
            }
        }
        let meeting_id = meeting_id.to_owned();
        let structured_json = serde_json::to_value(&payload).map_err(|error| error.to_string())?;
        let markdown = render_markdown(kind, &payload);
        let input_revision_set = serde_json::json!([{
            "kind": "transcript",
            "revision_id": context.transcript_revision_id,
        }]);
        let model_identity = format!("{}:{}", snapshot.provider.as_str(), snapshot.model);
        let prompt_identity = kind.as_str().to_owned();
        self.database
            .run(move |connection| {
                let current_revision: Option<String> = connection
                    .query_row(
                        "SELECT current_transcript_revision_id FROM meetings WHERE id = ?1 AND deleted_at IS NULL",
                        [&meeting_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                if current_revision.as_deref() != Some(
                    input_revision_set[0]
                        .get("revision_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                ) {
                    return Err("transcript changed while summary was generating".into());
                }
                let revision: i64 = connection
                    .query_row(
                        "SELECT COALESCE(MAX(revision), 0) + 1 FROM summary_revisions WHERE meeting_id = ?1",
                        [&meeting_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let summary_id = Uuid::new_v4().to_string();
                let now = Utc::now().to_rfc3339();
                let tx = connection.transaction().map_err(|error| error.to_string())?;
                tx.execute(
                    "UPDATE summary_revisions SET freshness = 'outdated' WHERE meeting_id = ?1 AND freshness = 'current'",
                    [&meeting_id],
                )
                .map_err(|error| error.to_string())?;
                tx.execute(
                    "INSERT INTO summary_revisions(id, meeting_id, revision, meeting_type, structured_json, markdown, input_revision_set, model_identity, prompt_identity, freshness, published_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'current', ?10)",
                    (&summary_id, &meeting_id, revision, kind.meeting_type(), structured_json.to_string(), &markdown, input_revision_set.to_string(), &model_identity, &prompt_identity, &now),
                )
                .map_err(|error| error.to_string())?;
                for action in &payload.action_items {
                    let action_id = Uuid::new_v4().to_string();
                    let evidence_refs = serde_json::to_string(&action.evidence_refs).map_err(|error| error.to_string())?;
                    let assignee = action
                        .assignee_speaker_id
                        .clone()
                        .or_else(|| action.assignee.as_ref().and_then(|name| context.speakers.iter().find(|(_, value)| value.eq_ignore_ascii_case(name)).map(|(id, _)| id.clone())));
                    tx.execute(
                        "INSERT INTO action_items(id, summary_revision_id, text, assignee_speaker_id, deadline_phrase, normalized_deadline, evidence_refs) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        (&action_id, &summary_id, &action.text, &assignee, &action.deadline_phrase, &action.normalized_deadline, &evidence_refs),
                    )
                    .map_err(|error| error.to_string())?;
                    tx.execute(
                        "INSERT INTO action_state(action_item_id, completed, updated_at) VALUES (?1, 0, ?2)",
                        (&action_id, &now),
                    )
                    .map_err(|error| error.to_string())?;
                }
                tx.execute(
                    "UPDATE meetings SET current_summary_revision_id = ?1, detected_type = ?2 WHERE id = ?3",
                    (&summary_id, kind.meeting_type(), &meeting_id),
                )
                .map_err(|error| error.to_string())?;
                if context.title_origin == "placeholder" {
                    if let Some(title) = payload
                        .title
                        .as_deref()
                        .and_then(clean_title)
                        .or_else(|| clean_title(&payload.overview))
                    {
                        tx.execute(
                            "UPDATE meetings SET title = ?1, title_origin = 'automatic', title_revision = title_revision + 1 WHERE id = ?2 AND title_origin = 'placeholder' AND title_revision = ?3",
                            (&title, &meeting_id, context.title_revision),
                        )
                        .map_err(|error| error.to_string())?;
                    }
                }
                tx.commit().map_err(|error| error.to_string())?;
                read_revision(connection, &summary_id)
            })
            .map_err(|error| error.to_string())
    }

    fn start_job(
        &self,
        meeting_id: &str,
        snapshot: &ProviderSnapshot,
        route: &super::prompts::PromptRoute,
        context: &SummaryContext,
    ) -> Result<String, String> {
        let job_id = Uuid::new_v4().to_string();
        let meeting_id = meeting_id.to_owned();
        let config_snapshot = serde_json::json!({
            "provider": snapshot,
            "prompt_kind": route.kind,
            "transcript_revision_id": context.transcript_revision_id,
        })
        .to_string();
        let transcript_revision_id = context.transcript_revision_id.clone();
        self.database
            .run(move |connection| {
                let now = Utc::now().to_rfc3339();
                connection
                    .execute(
                        "INSERT INTO jobs(id, meeting_id, kind, state, source_revision, config_snapshot, resource_class, created_at, updated_at) VALUES (?1, ?2, 'summarization', 'running', ?3, ?4, 'background', ?5, ?5)",
                        (&job_id, &meeting_id, &transcript_revision_id, &config_snapshot, &now),
                    )
                    .map(|_| job_id)
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
    }

    fn finish_job(&self, job_id: &str, state: &str, reason: Option<&str>) {
        let job_id = job_id.to_owned();
        let state = state.to_owned();
        let reason = reason.map(str::to_owned);
        let _ = self.database.run(move |connection| {
            connection
                .execute(
                    "UPDATE jobs SET state = ?1, reason = ?2, progress = ?3, updated_at = ?4 WHERE id = ?5",
                    (&state, &reason, if state == "done" { 1.0 } else { 0.0 }, Utc::now().to_rfc3339(), &job_id),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }
}

fn validate_payload(payload: &GeneratedSummary, context: &SummaryContext) -> Result<(), String> {
    if payload.overview.trim().is_empty() {
        return Err("summary overview cannot be empty".into());
    }
    let transcript_ids = context
        .transcripts
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    let slide_ids = context
        .slides
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    for citation in &payload.citations {
        let valid = match citation.kind.as_str() {
            "transcript" => transcript_ids.contains(citation.ref_id.as_str()),
            "slide" => slide_ids.contains(citation.ref_id.as_str()),
            _ => false,
        };
        if !valid {
            return Err(format!(
                "summary citation references unavailable {}:{}",
                citation.kind, citation.ref_id
            ));
        }
    }
    for action in &payload.action_items {
        if action.text.trim().is_empty() {
            return Err("action item text cannot be empty".into());
        }
        if let Some(speaker_id) = action.assignee_speaker_id.as_deref() {
            if !context.speakers.contains_key(speaker_id) {
                return Err(format!(
                    "action item references unknown speaker {speaker_id}"
                ));
            }
        }
        for evidence in &action.evidence_refs {
            if let Some(reference) = evidence.as_str() {
                if !transcript_ids.contains(reference) && !slide_ids.contains(reference) {
                    return Err(format!(
                        "action item evidence references unknown source {reference}"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn parse_generated_summary(content: &str) -> Result<GeneratedSummary, LlmError> {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .and_then(|value| value.strip_suffix("```"))
        .or_else(|| {
            trimmed
                .strip_prefix("```")
                .and_then(|value| value.strip_suffix("```"))
        })
        .unwrap_or(trimmed)
        .trim();
    let start = json
        .find('{')
        .ok_or_else(|| LlmError::InvalidResponse("summary is not JSON".into()))?;
    let end = json
        .rfind('}')
        .ok_or_else(|| LlmError::InvalidResponse("summary is not JSON".into()))?;
    serde_json::from_str(&json[start..=end]).map_err(|error| {
        LlmError::InvalidResponse(format!("summary schema validation failed: {error}"))
    })
}

fn clean_title(title: &str) -> Option<String> {
    let title = title
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('#')
        .trim();
    if title.is_empty() {
        None
    } else {
        Some(title.chars().take(100).collect())
    }
}

fn render_markdown(kind: PromptKind, payload: &GeneratedSummary) -> String {
    let mut markdown = format!(
        "# {} Summary\n\n{}\n",
        kind.as_str(),
        payload.overview.trim()
    );
    if !payload.decisions.is_empty() {
        markdown.push_str("\n## Decisions\n");
        for decision in &payload.decisions {
            markdown.push_str(&format!("- {}\n", decision));
        }
    }
    if !payload.action_items.is_empty() {
        markdown.push_str("\n## Action items\n");
        for action in &payload.action_items {
            let assignee = action
                .assignee
                .as_deref()
                .map(|value| format!(" — {value}"))
                .unwrap_or_default();
            let deadline = action
                .deadline_phrase
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            markdown.push_str(&format!("- [ ] {}{}{}\n", action.text, assignee, deadline));
        }
    }
    if !payload.key_concepts.is_empty() {
        markdown.push_str("\n## Key concepts\n");
        for concept in &payload.key_concepts {
            markdown.push_str(&format!("- {}\n", concept));
        }
    }
    if !payload.assignments.is_empty() {
        markdown.push_str("\n## Assignments\n");
        for assignment in &payload.assignments {
            markdown.push_str(&format!("- {}\n", assignment));
        }
    }
    if !payload.important_dates.is_empty() {
        markdown.push_str("\n## Important dates\n");
        for date in &payload.important_dates {
            markdown.push_str(&format!("- {}\n", date));
        }
    }
    if !payload.disclosures.is_empty() {
        markdown.push_str("\n> Disclosure: ");
        markdown.push_str(&payload.disclosures.join(" "));
        markdown.push('\n');
    }
    markdown
}

fn action_items_for(
    connection: &rusqlite::Connection,
    revision_id: &str,
) -> Result<Vec<ActionItemDto>, String> {
    let mut statement = connection
        .prepare("SELECT action_items.id, action_items.summary_revision_id, action_items.text, action_items.assignee_speaker_id, action_items.deadline_phrase, action_items.normalized_deadline, action_items.evidence_refs, COALESCE(action_state.completed, 0) FROM action_items LEFT JOIN action_state ON action_state.action_item_id = action_items.id WHERE action_items.summary_revision_id = ?1 ORDER BY action_items.rowid")
        .map_err(|error| error.to_string())?;
    let result = statement
        .query_map([revision_id], |row| {
            let evidence_refs: String = row.get(6)?;
            Ok(ActionItemDto {
                id: row.get(0)?,
                summary_revision_id: row.get(1)?,
                text: row.get(2)?,
                assignee_speaker_id: row.get(3)?,
                deadline_phrase: row.get(4)?,
                normalized_deadline: row.get(5)?,
                evidence_refs: serde_json::from_str(&evidence_refs).unwrap_or_default(),
                completed: row.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string());
    result
}

#[allow(clippy::type_complexity)]
fn read_revision(
    connection: &rusqlite::Connection,
    revision_id: &str,
) -> Result<SummaryRevisionDto, String> {
    let (id, meeting_id, revision, meeting_type, structured_json, markdown, input_revision_set, model_identity, prompt_identity, freshness, published_at): (String, String, i64, String, String, String, String, Option<String>, Option<String>, String, Option<String>) = connection
        .query_row(
            "SELECT id, meeting_id, revision, meeting_type, structured_json, markdown, input_revision_set, model_identity, prompt_identity, freshness, published_at FROM summary_revisions WHERE id = ?1",
            [revision_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?)),
        )
        .map_err(|error| error.to_string())?;
    Ok(SummaryRevisionDto {
        id: id.clone(),
        meeting_id,
        revision,
        meeting_type,
        structured_json: serde_json::from_str(&structured_json).unwrap_or(Value::Null),
        markdown,
        input_revision_set: serde_json::from_str(&input_revision_set).unwrap_or_default(),
        model_identity,
        prompt_identity,
        freshness,
        published_at,
        action_items: action_items_for(connection, &id)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    struct FakeProvider {
        response: String,
    }
    impl CompletionProvider for FakeProvider {
        fn complete(
            &self,
            _request: &ChatCompletionRequest,
        ) -> Result<super::super::ChatCompletionResponse, LlmError> {
            Ok(super::super::ChatCompletionResponse {
                content: self.response.clone(),
                model: Some("fake".into()),
                prompt_tokens: None,
                completion_tokens: None,
            })
        }
    }

    fn fixture_db() -> Database {
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection
                .execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone,requested_type) VALUES ('m','2026-09-26 09:00','placeholder','now','UTC','auto')", [])
                .map_err(|error| error.to_string())?;
            connection
                .execute("INSERT INTO artifact_revisions(id,meeting_id,kind,revision,model_identity,freshness) VALUES ('t1','m','transcript',1,'whisper','current')", [])
                .map_err(|error| error.to_string())?;
            connection
                .execute("UPDATE meetings SET current_transcript_revision_id = 't1' WHERE id='m'", [])
                .map_err(|error| error.to_string())?;
            connection
                .execute("INSERT INTO transcript_segments(id,transcript_revision_id,meeting_id,ordinal,start_seconds,end_seconds,text) VALUES ('seg1','t1','m',0,0,2,'We agreed to ship the sprint by Friday.')", [])
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn snapshot() -> ProviderSnapshot {
        ProviderSnapshot {
            provider: super::super::ProviderKind::LlamaServer,
            model: "local-test".into(),
            destination: "127.0.0.1".into(),
            selected_at: "now".into(),
            disclosure: "local".into(),
        }
    }

    #[test]
    fn summary_revision_and_completion_are_revision_owned() {
        let db = fixture_db();
        let service = SummarizationService::new(db.clone());
        let fake = FakeProvider {
            response: serde_json::json!({
                "title": "Sprint delivery",
                "overview": "The team agreed on a sprint delivery date.",
                "decisions": ["Ship by Friday"],
                "action_items": [{"text":"Ship the sprint","assignee_speaker_id":null,"assignee":null,"deadline_phrase":"Friday","normalized_deadline":null,"evidence_refs":["seg1"]}],
                "citations": [{"kind":"transcript","ref_id":"seg1","start_seconds":0.0,"end_seconds":2.0,"page_number":null}],
                "key_concepts":[], "assignments":[], "important_dates":[], "disclosures":[]
            })
            .to_string(),
        };
        let first = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(first.action_items.len(), 1);
        let unchanged = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap();
        assert_eq!(unchanged.id, first.id);
        service.set_user_title("m", "Chosen by user").unwrap();
        service
            .toggle_action_item(&first.action_items[0].id, true)
            .unwrap();
        assert!(service.action_items(&first.id).unwrap()[0].completed);
        let second = service
            .summarize_with_provider("m", &snapshot(), &fake, true)
            .unwrap();
        assert_eq!(second.revision, 2);
        assert!(!second.action_items[0].completed);
        assert!(service
            .list_revisions("m")
            .unwrap()
            .iter()
            .any(|revision| revision.revision == 1 && revision.freshness == "outdated"));
        let title: String = db
            .run(|connection| {
                connection
                    .query_row("SELECT title FROM meetings WHERE id='m'", [], |row| {
                        row.get(0)
                    })
                    .map_err(|error| error.to_string())
            })
            .unwrap();
        assert_eq!(title, "Chosen by user");
    }
}
