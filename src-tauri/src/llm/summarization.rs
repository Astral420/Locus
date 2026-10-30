use super::{
    prompts::{PromptKind, PromptRouter},
    providers::ProviderSnapshot,
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, CompletionProvider, LlmError,
    ResponseFormat,
};
use crate::{contracts::MeetingType, db::Database};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

/// Prompt sizes are counted in characters because that is how the transcript is
/// measured; the engine's window is counted in tokens. English runs at roughly
/// three characters per token, so this limit keeps
/// `MAX_SINGLE_CONTEXT_CHARS / 3 + max_tokens` (9_000 / 3 + 4_096 = 7_096) below
/// `DEFAULT_GENERATION_CONTEXT` (8_192), leaving about 1_000 tokens for the
/// prompt's own instructions and the model's chat template.
const MAX_SINGLE_CONTEXT_CHARS: usize = 9_000;
/// One hierarchical passage, small enough that a passage plus its folded note
/// still fits the same window.
const HIERARCHICAL_CHUNK_CHARS: usize = 4_000;
/// Notes are short on purpose: a full-size completion per passage would push the
/// folded second pass out of the window on long recordings.
const INTERMEDIATE_MAX_TOKENS: u32 = 700;
/// Folding halves the text roughly fivefold per pass, so this bound is only ever
/// reached by pathological input; running out means the engine reports the real
/// context error rather than silently dropping transcript content.
const MAX_FOLD_PASSES: usize = 4;
/// Tokens reserved for a summary completion.
const SUMMARY_MAX_TOKENS: u32 = 4_096;
/// How many raw-output diagnostic files are kept in the summary-failures folder.
const SUMMARY_FAILURE_LIMIT: usize = 5;
/// Upper bound on a diagnostic file so a runaway model cannot fill the disk.
const SUMMARY_FAILURE_MAX_BYTES: usize = 64 * 1024;
/// Opt-in capture of raw output for summaries that parsed successfully.
const ENV_SUMMARY_DEBUG: &str = "LOCUS_SUMMARY_DEBUG";

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

/// JSON Schema for [`GeneratedSummary`], sent with the final summary request so
/// a local `llama-server` compiles it into a grammar and cannot emit a summary
/// of the wrong shape. It mirrors the struct exactly: the five list fields are
/// arrays of **strings**, the two structured lists are arrays of objects with
/// every key required, and no object accepts extra keys.
///
/// Kept deliberately plain (no `$ref`, no `not`, no `if/then`, no pattern
/// properties) because it has to convert cleanly through llama.cpp's
/// `json-schema-to-grammar`. Every property is listed in `required` so the
/// generated grammar has one fixed shape; an empty array or `null` is still a
/// valid value for every field.
pub fn summary_json_schema() -> Value {
    let nullable_string = serde_json::json!({ "type": ["string", "null"] });
    let string_list = serde_json::json!({
        "type": "array",
        "items": { "type": "string" },
    });
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": nullable_string,
            "overview": { "type": "string" },
            "action_items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "assignee_speaker_id": nullable_string,
                        "assignee": nullable_string,
                        "deadline_phrase": nullable_string,
                        "normalized_deadline": nullable_string,
                        "evidence_refs": { "type": "array", "items": { "type": "string" } },
                    },
                    "required": [
                        "text",
                        "assignee_speaker_id",
                        "assignee",
                        "deadline_phrase",
                        "normalized_deadline",
                        "evidence_refs",
                    ],
                    "additionalProperties": false,
                },
            },
            "decisions": string_list,
            "key_concepts": string_list,
            "assignments": string_list,
            "important_dates": string_list,
            "citations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string" },
                        "ref_id": { "type": "string" },
                        "start_seconds": { "type": ["number", "null"] },
                        "end_seconds": { "type": ["number", "null"] },
                        "page_number": { "type": ["integer", "null"] },
                    },
                    "required": ["kind", "ref_id", "start_seconds", "end_seconds", "page_number"],
                    "additionalProperties": false,
                },
            },
            "disclosures": string_list,
        },
        "required": [
            "title",
            "overview",
            "action_items",
            "decisions",
            "key_concepts",
            "assignments",
            "important_dates",
            "citations",
            "disclosures",
        ],
        "additionalProperties": false,
    })
}

/// Everything a diagnostic file needs to explain a failed summary. The raw
/// model output is stored locally only; nothing here crosses a network boundary.
#[derive(Debug, Clone)]
pub struct FailureRecord {
    pub timestamp: String,
    pub meeting_id: String,
    pub provider: String,
    pub model: String,
    pub prompt_identity: String,
    pub max_tokens: u32,
    pub completion_tokens: Option<u64>,
    pub message: String,
    pub raw: String,
}

impl FailureRecord {
    fn truncated_at_max_tokens(&self) -> bool {
        self.completion_tokens
            .is_some_and(|tokens| tokens >= u64::from(self.max_tokens))
    }
}

/// Injected sink for local diagnostic files. Implementations must never panic
/// and must ignore their own I/O errors so the original summary error survives.
pub trait SummaryFailureSink: Send + Sync {
    fn record(&self, record: &FailureRecord) -> Option<PathBuf>;
}

pub struct NoopSummaryFailureSink;

impl SummaryFailureSink for NoopSummaryFailureSink {
    fn record(&self, _record: &FailureRecord) -> Option<PathBuf> {
        None
    }
}

/// Writes one plain-text file per record into a dedicated folder, keeping only
/// the newest [`SUMMARY_FAILURE_LIMIT`] files.
pub struct FileSummaryFailureSink {
    directory: PathBuf,
}

impl FileSummaryFailureSink {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
}

impl SummaryFailureSink for FileSummaryFailureSink {
    fn record(&self, record: &FailureRecord) -> Option<PathBuf> {
        if fs::create_dir_all(&self.directory).is_err() {
            return None;
        }
        let stem = format!(
            "{}-{}",
            sanitize_file_component(&record.timestamp),
            sanitize_file_component(&record.meeting_id)
        );
        let path = unique_path(&self.directory, &stem);
        let mut content = render_failure_record(record);
        if content.len() > SUMMARY_FAILURE_MAX_BYTES {
            let end = floor_char_boundary(&content, SUMMARY_FAILURE_MAX_BYTES);
            content.truncate(end);
        }
        fs::write(&path, content).ok()?;
        prune_old_failures(&self.directory, SUMMARY_FAILURE_LIMIT);
        Some(path)
    }
}

fn render_failure_record(record: &FailureRecord) -> String {
    let mut text = format!(
        "timestamp: {}\nmeeting id: {}\nprovider: {}\nmodel: {}\nprompt: {}\nmax_tokens: {}\n",
        record.timestamp,
        record.meeting_id,
        record.provider,
        record.model,
        record.prompt_identity,
        record.max_tokens,
    );
    if let Some(tokens) = record.completion_tokens {
        text.push_str(&format!("completion_tokens: {tokens}\n"));
    }
    if record.truncated_at_max_tokens() {
        text.push_str("WARNING: output may have been truncated at max_tokens\n");
    }
    text.push_str(&format!(
        "error: {}\n\n----- raw model output -----\n{}",
        record.message, record.raw
    ));
    text
}

fn sanitize_file_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "unknown".into()
    } else {
        sanitized
    }
}

fn unique_path(directory: &Path, stem: &str) -> PathBuf {
    let mut candidate = directory.join(format!("{stem}.txt"));
    let mut index = 1;
    while candidate.exists() {
        candidate = directory.join(format!("{stem}-{index}.txt"));
        index += 1;
    }
    candidate
}

fn prune_old_failures(directory: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
        .collect();
    files.sort();
    if files.len() > keep {
        for path in &files[..files.len() - keep] {
            let _ = fs::remove_file(path);
        }
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn debug_capture_enabled() -> bool {
    std::env::var(ENV_SUMMARY_DEBUG).as_deref() == Ok("1")
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
    failure_sink: Arc<dyn SummaryFailureSink>,
}

impl SummarizationService {
    pub fn new(database: Database) -> Self {
        Self {
            database,
            failure_sink: Arc::new(NoopSummaryFailureSink),
        }
    }

    pub fn with_failure_sink(mut self, sink: Arc<dyn SummaryFailureSink>) -> Self {
        self.failure_sink = sink;
        self
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
        let result = self.generate_payload(meeting_id, provider, snapshot, &route, &context);
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
        meeting_id: &str,
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
        let fold = |passage: &str| -> Result<String, LlmError> {
            let request = ChatCompletionRequest {
                model: snapshot.model.clone(),
                messages: vec![
                    ChatMessage::system(PromptRouter::folding_system_prompt()),
                    ChatMessage::user(format!(
                        "Summarize this sequential transcript passage into concise evidence-preserving notes. Keep action items, decisions, people, dates, and source IDs. Do not omit content because later passages may depend on it.\n\n{passage}"
                    )),
                ],
                temperature: 0.1,
                max_tokens: Some(INTERMEDIATE_MAX_TOKENS),
                response_format: None,
            };
            provider.complete(&request).map(|response| response.content)
        };
        let response = if transcript.chars().count() + slide_text.chars().count()
            <= MAX_SINGLE_CONTEXT_CHARS
        {
            self.complete(provider, snapshot, route.kind, &transcript, &slide_text)?
        } else {
            let mut notes = char_chunks(&transcript, HIERARCHICAL_CHUNK_CHARS)
                .into_iter()
                .map(fold)
                .collect::<Result<Vec<_>, LlmError>>()?
                .join("\n\n--- NEXT PASS ---\n\n");
            // The folded notes are the prompt of the final pass, so they have to
            // respect the same window the transcript did.
            let mut passes = 0;
            while notes.chars().count() + slide_text.chars().count() > MAX_SINGLE_CONTEXT_CHARS
                && passes < MAX_FOLD_PASSES
            {
                notes = char_chunks(&notes, HIERARCHICAL_CHUNK_CHARS)
                    .into_iter()
                    .map(fold)
                    .collect::<Result<Vec<_>, LlmError>>()?
                    .join("\n\n--- NEXT PASS ---\n\n");
                passes += 1;
            }
            self.complete(provider, snapshot, route.kind, &notes, &slide_text)?
        };
        match parse_generated_summary(&response.content) {
            Ok((mut payload, report)) => {
                if !report.is_empty() {
                    payload
                        .disclosures
                        .push(normalization_disclosure(&report).to_owned());
                    eprintln!("locus: summary normalized: {}", report.description());
                }
                if debug_capture_enabled() {
                    let message = if report.is_empty() {
                        "summary parsed successfully".to_owned()
                    } else {
                        format!(
                            "summary parsed successfully; normalized: {}",
                            report.description()
                        )
                    };
                    self.record_summary_output(
                        meeting_id,
                        snapshot,
                        route,
                        &response,
                        &response.content,
                        &message,
                    );
                }
                Ok(payload)
            }
            Err(error) => {
                let saved = self.record_summary_output(
                    meeting_id,
                    snapshot,
                    route,
                    &response,
                    &error.raw,
                    &error.message,
                );
                match &saved {
                    Some(path) => eprintln!("locus: summary parse failed ({})", path.display()),
                    None => eprintln!(
                        "locus: summary parse failed ({})",
                        error.path.as_deref().unwrap_or("unknown field")
                    ),
                }
                let message = match saved {
                    Some(path) => format!(
                        "{} Raw model output saved to {}.",
                        error.message,
                        path.display()
                    ),
                    None => error.message,
                };
                Err(LlmError::InvalidResponse(message))
            }
        }
    }

    fn record_summary_output(
        &self,
        meeting_id: &str,
        snapshot: &ProviderSnapshot,
        route: &super::prompts::PromptRoute,
        response: &ChatCompletionResponse,
        raw: &str,
        message: &str,
    ) -> Option<PathBuf> {
        let record = FailureRecord {
            timestamp: Utc::now().to_rfc3339(),
            meeting_id: meeting_id.to_owned(),
            provider: snapshot.provider.as_str().to_owned(),
            model: snapshot.model.clone(),
            prompt_identity: route.kind.identity().to_owned(),
            max_tokens: SUMMARY_MAX_TOKENS,
            completion_tokens: response.completion_tokens,
            message: message.to_owned(),
            raw: raw.to_owned(),
        };
        self.failure_sink.record(&record)
    }

    fn complete(
        &self,
        provider: &dyn CompletionProvider,
        snapshot: &ProviderSnapshot,
        kind: PromptKind,
        transcript: &str,
        slide_text: &str,
    ) -> Result<ChatCompletionResponse, LlmError> {
        provider.complete(&ChatCompletionRequest {
            model: snapshot.model.clone(),
            messages: vec![
                ChatMessage::system(PromptRouter::system_prompt(kind)),
                ChatMessage::user(PromptRouter::user_prompt(kind, transcript, slide_text)),
            ],
            temperature: 0.1,
            max_tokens: Some(SUMMARY_MAX_TOKENS),
            // The final summary is the one call whose shape the app depends on.
            // Folding passes stay free text.
            response_format: Some(ResponseFormat::JsonSchema {
                name: "meeting_summary".into(),
                schema: summary_json_schema(),
            }),
        })
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
        let prompt_identity = kind.identity().to_owned();
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
        // One row per meeting, shared with the pipeline queue, so the UI shows a
        // single "summarization" step instead of an extra entry per attempt.
        let job_id = format!("pipeline:{meeting_id}:summarization");
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
                        "INSERT INTO jobs(id, meeting_id, kind, state, source_revision, config_snapshot, resource_class, created_at, updated_at) VALUES (?1, ?2, 'summarization', 'running', ?3, ?4, 'inference', ?5, ?5) ON CONFLICT(id) DO UPDATE SET state='running', reason=NULL, progress=0, source_revision=excluded.source_revision, updated_at=excluded.updated_at",
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

/// Split at character boundaries, never inside a multi-byte character. Slicing
/// the byte vector directly, as this used to, replaced whatever character
/// straddled a boundary with U+FFFD and sent that to the model.
fn char_chunks(text: &str, limit: usize) -> Vec<&str> {
    let limit = limit.max(1);
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut chars_in_chunk = 0;
    for (index, _) in text.char_indices() {
        if chars_in_chunk == limit {
            chunks.push(&text[start..index]);
            start = index;
            chars_in_chunk = 0;
        }
        chars_in_chunk += 1;
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NormalizationReport {
    /// JSON paths whose value was rewritten into the expected shape.
    pub flattened: Vec<String>,
    /// Number of list entries that could not be recovered and were removed.
    pub dropped: usize,
}

impl NormalizationReport {
    fn is_empty(&self) -> bool {
        self.flattened.is_empty() && self.dropped == 0
    }

    fn description(&self) -> String {
        format!(
            "{}, dropped {}",
            if self.flattened.is_empty() {
                "nothing".to_owned()
            } else {
                self.flattened.join(", ")
            },
            self.dropped
        )
    }
}

const REFORMATTED_DISCLOSURE: &str =
    "Some summary items were reformatted because the model returned structured entries.";
const REFORMATTED_AND_DROPPED_DISCLOSURE: &str = "Some summary items were reformatted because the model returned structured entries and some invalid entries were dropped.";
const PRIMARY_OBJECT_KEYS: &[&str] = &[
    "name", "term", "concept", "title", "date", "decision", "task",
];
const SECONDARY_OBJECT_KEYS: &[&str] = &[
    "definition",
    "description",
    "explanation",
    "event",
    "details",
    "rationale",
];

fn normalization_disclosure(report: &NormalizationReport) -> &'static str {
    if report.dropped > 0 {
        REFORMATTED_AND_DROPPED_DISCLOSURE
    } else {
        REFORMATTED_DISCLOSURE
    }
}

#[derive(Debug)]
struct SummaryParseError {
    message: String,
    path: Option<String>,
    raw: String,
}

fn parse_generated_summary(
    content: &str,
) -> Result<(GeneratedSummary, NormalizationReport), SummaryParseError> {
    let raw = content.to_owned();
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
    let not_json = || SummaryParseError {
        message: "summary is not JSON".into(),
        path: None,
        raw: raw.clone(),
    };
    let start = json.find('{').ok_or_else(not_json)?;
    let end = json.rfind('}').ok_or_else(not_json)?;
    let mut value: Value =
        serde_json::from_str(&json[start..=end]).map_err(|detail| SummaryParseError {
            message: format!("summary is not valid JSON: {detail}"),
            path: None,
            raw: raw.clone(),
        })?;
    let mut report = NormalizationReport::default();
    normalize_summary_value(&mut value, &mut report);
    match serde_path_to_error::deserialize::<_, GeneratedSummary>(value) {
        Ok(summary) => Ok((summary, report)),
        Err(error) => {
            let path = error.path().to_string();
            let path = (!path.is_empty() && path != ".").then_some(path);
            let detail = error.into_inner();
            let message = match &path {
                Some(path) => format!("summary schema validation failed at {path}: {detail}"),
                None => format!("summary schema validation failed: {detail}"),
            };
            Err(SummaryParseError { message, path, raw })
        }
    }
}

fn normalize_summary_value(value: &mut Value, report: &mut NormalizationReport) {
    let Value::Object(object) = value else {
        return;
    };
    normalize_overview(object.get_mut("overview"), report);
    for field in [
        "decisions",
        "key_concepts",
        "assignments",
        "important_dates",
        "disclosures",
    ] {
        normalize_string_list(object.get_mut(field), field, report);
    }
    normalize_action_items(object.get_mut("action_items"), report);
    normalize_citations(object.get_mut("citations"), report);
    normalize_optional_string(object.get_mut("title"), "title", report);
}

/// Render a JSON value as text without losing the order of object keys. The
/// crate is built with `serde_json`'s `preserve_order`, so an object keeps the
/// model's original key order instead of sorting alphabetically.
fn flatten_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(flatten_value).collect();
            (!parts.is_empty()).then(|| parts.join("; "))
        }
        Value::Object(object) => flatten_object(object),
    }
}

fn flatten_object(object: &serde_json::Map<String, Value>) -> Option<String> {
    let find = |keys: &[&str]| {
        object.iter().find(|(key, _)| {
            let key = key.to_ascii_lowercase();
            keys.contains(&key.as_str())
        })
    };
    if let (Some((_, primary)), Some((_, secondary))) =
        (find(PRIMARY_OBJECT_KEYS), find(SECONDARY_OBJECT_KEYS))
    {
        if let (Some(primary), Some(secondary)) = (flatten_value(primary), flatten_value(secondary))
        {
            return Some(format!("{primary}: {secondary}"));
        }
    }
    let parts: Vec<String> = object.values().filter_map(flatten_value).collect();
    (!parts.is_empty()).then(|| parts.join(" - "))
}

fn normalize_string_list(slot: Option<&mut Value>, path: &str, report: &mut NormalizationReport) {
    let Some(value) = slot else {
        return;
    };
    match value {
        Value::Null => {
            *value = Value::Array(Vec::new());
            report.flattened.push(path.to_owned());
        }
        Value::String(_) => {
            let item = value.take();
            *value = Value::Array(vec![item]);
            report.flattened.push(path.to_owned());
        }
        Value::Array(items) => {
            let mut normalized = Vec::with_capacity(items.len());
            for (index, item) in items.drain(..).enumerate() {
                let item_path = format!("{path}[{index}]");
                match item {
                    Value::Null => report.dropped += 1,
                    Value::String(text) if text.trim().is_empty() => report.dropped += 1,
                    Value::String(text) => normalized.push(Value::String(text)),
                    other => match flatten_value(&other) {
                        Some(text) => {
                            normalized.push(Value::String(text));
                            report.flattened.push(item_path);
                        }
                        None => report.dropped += 1,
                    },
                }
            }
            *value = Value::Array(normalized);
        }
        ref other => match flatten_value(other) {
            Some(text) => {
                *value = Value::Array(vec![Value::String(text)]);
                report.flattened.push(path.to_owned());
            }
            None => {
                *value = Value::Array(Vec::new());
                report.dropped += 1;
            }
        },
    }
}

fn normalize_optional_string(
    slot: Option<&mut Value>,
    path: &str,
    report: &mut NormalizationReport,
) {
    let Some(value) = slot else {
        return;
    };
    match value {
        Value::Null => {}
        Value::String(text) => {
            if text.trim().is_empty() {
                *value = Value::Null;
                report.flattened.push(path.to_owned());
            }
        }
        ref other => {
            let flattened = flatten_value(other);
            *value = flattened.map_or(Value::Null, Value::String);
            report.flattened.push(path.to_owned());
        }
    }
}

fn normalize_overview(slot: Option<&mut Value>, report: &mut NormalizationReport) {
    let Some(value) = slot else {
        return;
    };
    match value {
        Value::String(_) | Value::Null => {}
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(flatten_value).collect();
            let text = parts.join(" ");
            if text.trim().is_empty() {
                *value = Value::Null;
                report.dropped += 1;
            } else {
                *value = Value::String(text);
                report.flattened.push("overview".to_owned());
            }
        }
        ref other => {
            let flattened = flatten_value(other);
            *value = flattened.map_or(Value::Null, Value::String);
            report.flattened.push("overview".to_owned());
        }
    }
}

fn normalize_action_items(slot: Option<&mut Value>, report: &mut NormalizationReport) {
    let Some(value) = slot else {
        return;
    };
    match value {
        Value::Null => {
            *value = Value::Array(Vec::new());
            report.flattened.push("action_items".to_owned());
        }
        Value::Array(items) => {
            let mut normalized = Vec::with_capacity(items.len());
            for (index, item) in items.drain(..).enumerate() {
                let path = format!("action_items[{index}]");
                match item {
                    Value::String(text) if !text.trim().is_empty() => {
                        normalized.push(serde_json::json!({ "text": text }));
                        report.flattened.push(path);
                    }
                    Value::Object(mut object) => {
                        let text = ["text", "task", "description", "action"]
                            .iter()
                            .find_map(|key| object.get(*key).and_then(flatten_value))
                            .filter(|text| !text.trim().is_empty());
                        let Some(text) = text else {
                            report.dropped += 1;
                            continue;
                        };
                        let has_text = object
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.trim().is_empty());
                        if !has_text {
                            object.insert("text".to_owned(), Value::String(text));
                            report.flattened.push(format!("{path}.text"));
                        }
                        for field in [
                            "assignee_speaker_id",
                            "assignee",
                            "deadline_phrase",
                            "normalized_deadline",
                        ] {
                            normalize_optional_string(
                                object.get_mut(field),
                                &format!("{path}.{field}"),
                                report,
                            );
                        }
                        normalized.push(Value::Object(object));
                    }
                    _ => report.dropped += 1,
                }
            }
            *value = Value::Array(normalized);
        }
        _ => {
            *value = Value::Array(Vec::new());
            report.dropped += 1;
        }
    }
}

fn normalize_citations(slot: Option<&mut Value>, report: &mut NormalizationReport) {
    let Some(value) = slot else {
        return;
    };
    match value {
        Value::Null => {
            *value = Value::Array(Vec::new());
            report.flattened.push("citations".to_owned());
        }
        Value::Array(items) => {
            let mut normalized = Vec::with_capacity(items.len());
            for (index, mut item) in items.drain(..).enumerate() {
                let path = format!("citations[{index}]");
                if let Value::Object(object) = &mut item {
                    for field in ["start_seconds", "end_seconds"] {
                        normalize_numeric_string(object, field, &path, report);
                    }
                    normalize_integer_string(object, "page_number", &path, report);
                }
                match serde_json::from_value::<Citation>(item.clone()) {
                    Ok(_) => normalized.push(item),
                    Err(_) => report.dropped += 1,
                }
            }
            *value = Value::Array(normalized);
        }
        _ => {
            *value = Value::Array(Vec::new());
            report.dropped += 1;
        }
    }
}

fn normalize_numeric_string(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    path: &str,
    report: &mut NormalizationReport,
) {
    let Some(Value::String(text)) = object.get(field) else {
        return;
    };
    let Ok(number) = text.trim().parse::<f64>() else {
        return;
    };
    let Some(number) = serde_json::Number::from_f64(number) else {
        return;
    };
    object.insert(field.to_owned(), Value::Number(number));
    report.flattened.push(format!("{path}.{field}"));
}

fn normalize_integer_string(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    path: &str,
    report: &mut NormalizationReport,
) {
    let Some(Value::String(text)) = object.get(field) else {
        return;
    };
    let Ok(number) = text.trim().parse::<i64>() else {
        return;
    };
    object.insert(field.to_owned(), Value::Number(number.into()));
    report.flattened.push(format!("{path}.{field}"));
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
    fn passages_split_on_character_boundaries_without_replacement_characters() {
        // Two-byte accented letters and a four-byte emoji, both of which used to
        // be corrupted when a byte chunk cut through them.
        for text in ["é".repeat(10), "👩‍💻 meeting notes ".repeat(4)] {
            let chunks = char_chunks(&text, 4);
            assert_eq!(chunks.concat(), text);
            for chunk in &chunks {
                assert!(!chunk.contains('\u{FFFD}'), "chunk lost a character: {chunk:?}");
                assert!(chunk.chars().count() <= 4, "chunk too long: {chunk:?}");
            }
            assert!(chunks.len() > 1);
        }
        assert!(char_chunks("", 4).is_empty());
    }

    #[test]
    fn the_single_pass_limit_leaves_room_for_the_summary_completion() {
        let prompt_tokens = MAX_SINGLE_CONTEXT_CHARS / 3;
        let context = crate::llm::llama_cpp::DEFAULT_GENERATION_CONTEXT as usize;
        let reserved = prompt_tokens + SUMMARY_MAX_TOKENS as usize;
        assert!(
            reserved < context,
            "{reserved} tokens do not fit a {context}-token window"
        );
        // The instructions around the transcript need their own headroom.
        assert!(context - reserved >= 500);
        // A folded passage plus its note must fit the same window.
        let passage = HIERARCHICAL_CHUNK_CHARS / 3 + INTERMEDIATE_MAX_TOKENS as usize;
        assert!(
            passage < context,
            "a hierarchical passage ({passage} tokens) does not fit the window"
        );
    }

    /// Records what each pass actually sent, so the window arithmetic can be
    /// checked against the requests instead of the constants.
    struct RecordingProvider {
        response: String,
        requests: std::sync::Mutex<Vec<(usize, u32)>>,
    }

    impl CompletionProvider for RecordingProvider {
        fn complete(
            &self,
            request: &ChatCompletionRequest,
        ) -> Result<super::super::ChatCompletionResponse, LlmError> {
            let chars = request
                .messages
                .iter()
                .map(|message| message.content.chars().count())
                .sum::<usize>();
            self.requests
                .lock()
                .unwrap()
                .push((chars, request.max_tokens.unwrap_or(0)));
            let is_fold = request.messages.iter().any(|message| {
                message
                    .content
                    .starts_with("Summarize this sequential transcript passage")
            });
            Ok(super::super::ChatCompletionResponse {
                content: if is_fold {
                    "note: the team agreed on a sprint date. ".repeat(4)
                } else {
                    self.response.clone()
                },
                model: Some("fake".into()),
                prompt_tokens: None,
                completion_tokens: None,
            })
        }
    }

    #[test]
    fn long_transcripts_are_folded_inside_the_generation_window() {
        let db = fixture_db();
        // About 48k characters of speech, far beyond one pass.
        let long = "The team agreed to ship the sprint by Friday. ".repeat(1_100);
        db.run({
            let long = long.clone();
            move |connection| {
                connection
                    .execute(
                        "UPDATE transcript_segments SET text=?1 WHERE id='seg1'",
                        [&long],
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        })
        .unwrap();
        let service = SummarizationService::new(db.clone());
        let fake = RecordingProvider {
            response: serde_json::json!({
                "title": "Sprint delivery",
                "overview": "The team agreed on a sprint delivery date.",
                "decisions": ["Ship by Friday"],
                "action_items": [],
                "citations": [],
                "key_concepts": [], "assignments": [], "important_dates": [], "disclosures": []
            })
            .to_string(),
            requests: std::sync::Mutex::new(Vec::new()),
        };

        let revision = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap();
        assert_eq!(revision.revision, 1);

        let requests = fake.requests.lock().unwrap();
        assert!(
            requests.len() > 1,
            "a long transcript must be folded in several passes"
        );
        let context = crate::llm::llama_cpp::DEFAULT_GENERATION_CONTEXT as usize;
        for (chars, max_tokens) in requests.iter() {
            let tokens = chars / 3 + *max_tokens as usize;
            assert!(
                tokens < context,
                "a {chars}-character request reserving {max_tokens} tokens needs {tokens} of {context}"
            );
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

    fn invalid_summary_output() -> String {
        serde_json::json!({
            "title": "Sprint delivery",
            "overview": null,
            "decisions": ["Ship by Friday"],
            "action_items": [],
            "citations": [],
            "key_concepts": [{"term": "sprint", "definition": "a fixed delivery window"}],
            "assignments": [],
            "important_dates": [],
            "disclosures": []
        })
        .to_string()
    }

    #[derive(Default)]
    struct CapturingSink {
        records: std::sync::Mutex<Vec<FailureRecord>>,
    }

    impl SummaryFailureSink for CapturingSink {
        fn record(&self, record: &FailureRecord) -> Option<PathBuf> {
            self.records.lock().unwrap().push(record.clone());
            Some(PathBuf::from("/tmp/locus-summary-diagnostic.txt"))
        }
    }

    #[test]
    fn a_parse_failure_names_the_field_and_records_the_raw_output() {
        let db = fixture_db();
        let sink = Arc::new(CapturingSink::default());
        let service = SummarizationService::new(db).with_failure_sink(sink.clone());
        let output = invalid_summary_output();
        let fake = FakeProvider {
            response: output.clone(),
        };

        let error = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap_err();

        assert!(
            error.contains("overview"),
            "the error must name the failing field: {error}"
        );
        assert!(error.contains("Raw model output saved to"));
        let records = sink.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].raw, output);
        assert_eq!(records[0].meeting_id, "m");
        assert!(records[0].completion_tokens.is_none());
    }

    fn record_with(timestamp: &str, raw: &str) -> FailureRecord {
        FailureRecord {
            timestamp: timestamp.into(),
            meeting_id: "m".into(),
            provider: "llama_server".into(),
            model: "local-test".into(),
            prompt_identity: "generic".into(),
            max_tokens: SUMMARY_MAX_TOKENS,
            completion_tokens: None,
            message: "summary schema validation failed".into(),
            raw: raw.into(),
        }
    }

    fn failure_files(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn only_the_newest_five_failure_files_are_kept() {
        let root = tempfile::tempdir().unwrap();
        let sink = FileSummaryFailureSink::new(root.path().to_path_buf());
        for index in 0..7 {
            let timestamp = format!("20260101T00000{index}Z");
            sink.record(&record_with(&timestamp, "raw output"));
        }

        let names = failure_files(root.path());
        assert_eq!(names.len(), 5);
        for index in 2..7 {
            assert!(
                names.contains(&format!("20260101T00000{index}Z-m.txt")),
                "missing newest file {index}: {names:?}"
            );
        }
    }

    #[test]
    fn diagnostic_content_is_truncated_at_64_kib() {
        let root = tempfile::tempdir().unwrap();
        let sink = FileSummaryFailureSink::new(root.path().to_path_buf());
        let path = sink
            .record(&record_with("20260101T000000Z", &"a".repeat(70_000)))
            .unwrap();

        let bytes = fs::read(&path).unwrap();
        assert!(bytes.len() <= SUMMARY_FAILURE_MAX_BYTES);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("----- raw model output -----"));
    }

    #[test]
    fn an_unwritable_failure_directory_leaves_the_original_error_intact() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("not-a-directory");
        fs::write(&blocker, b"x").unwrap();
        let sink = Arc::new(FileSummaryFailureSink::new(
            blocker.join("summary-failures"),
        ));
        let db = fixture_db();
        let service = SummarizationService::new(db).with_failure_sink(sink);
        let fake = FakeProvider {
            response: invalid_summary_output(),
        };

        let error = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap_err();

        assert!(error.contains("summary schema validation failed at overview"));
        assert!(!error.contains("Raw model output saved"));
    }

    struct TokenProvider {
        response: String,
        completion_tokens: u64,
    }

    impl CompletionProvider for TokenProvider {
        fn complete(
            &self,
            _request: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, LlmError> {
            Ok(ChatCompletionResponse {
                content: self.response.clone(),
                model: Some("fake".into()),
                prompt_tokens: None,
                completion_tokens: Some(self.completion_tokens),
            })
        }
    }

    #[test]
    fn a_completion_at_the_token_limit_is_flagged_in_the_diagnostic_file() {
        let root = tempfile::tempdir().unwrap();
        let sink = Arc::new(FileSummaryFailureSink::new(root.path().to_path_buf()));
        let db = fixture_db();
        let service = SummarizationService::new(db).with_failure_sink(sink);
        let fake = TokenProvider {
            response: invalid_summary_output(),
            completion_tokens: u64::from(SUMMARY_MAX_TOKENS),
        };

        service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap_err();

        let names = failure_files(root.path());
        assert_eq!(names.len(), 1);
        let text = fs::read_to_string(root.path().join(&names[0])).unwrap();
        assert!(text.contains("completion_tokens: 4096"));
        assert!(text.contains("WARNING: output may have been truncated at max_tokens"));
    }

    #[test]
    fn a_successful_summary_writes_nothing_unless_debug_capture_is_enabled() {
        std::env::remove_var(ENV_SUMMARY_DEBUG);
        let root = tempfile::tempdir().unwrap();
        let sink = Arc::new(FileSummaryFailureSink::new(root.path().to_path_buf()));
        let db = fixture_db();
        let service = SummarizationService::new(db).with_failure_sink(sink);
        let fake = FakeProvider {
            response: serde_json::json!({
                "title": "Sprint delivery",
                "overview": "The team agreed on a sprint delivery date.",
                "decisions": ["Ship by Friday"],
                "action_items": [],
                "citations": [],
                "key_concepts": [], "assignments": [], "important_dates": [], "disclosures": []
            })
            .to_string(),
        };

        let revision = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap();

        assert_eq!(revision.revision, 1);
        assert!(!root.path().exists() || failure_files(root.path()).is_empty());
    }

    fn parse_ok(value: Value) -> (GeneratedSummary, NormalizationReport) {
        parse_generated_summary(&value.to_string()).expect("payload should parse")
    }

    fn disclosures(revision: &SummaryRevisionDto) -> Vec<String> {
        revision
            .structured_json
            .get("disclosures")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn structured_list_entries_are_flattened_and_rendered_without_artifacts() {
        let (summary, report) = parse_ok(serde_json::json!({
            "overview": "The team covered delivery and scheduling.",
            "key_concepts": [{"term": "Idempotency", "definition": "same effect twice"}],
            "important_dates": [{"date": "2026-11-03", "event": "project demo"}],
            "assignments": [{"task": "Send the report", "assignee": "Ana"}]
        }));

        assert_eq!(summary.key_concepts, ["Idempotency: same effect twice"]);
        assert_eq!(summary.important_dates, ["2026-11-03: project demo"]);
        assert_eq!(summary.assignments, ["Send the report - Ana"]);
        assert_eq!(report.dropped, 0);
        assert!(report.flattened.contains(&"key_concepts[0]".to_owned()));
        let markdown = render_markdown(PromptKind::Generic, &summary);
        assert!(markdown.contains("- Idempotency: same effect twice"));
        assert!(markdown.contains("- 2026-11-03: project demo"));
        assert!(!markdown.contains('{') && !markdown.contains('}'));
    }

    #[test]
    fn object_key_order_is_preserved_when_flattening() {
        let (summary, _) = parse_ok(serde_json::json!({
            "overview": "Ordering check.",
            "decisions": [{"zeta": "last", "alpha": "first"}]
        }));

        assert_eq!(summary.decisions, ["last - first"]);
    }

    #[test]
    fn scalar_nested_and_single_string_lists_are_tolerated() {
        let (summary, report) = parse_ok(serde_json::json!({
            "overview": "Tolerant shapes.",
            "decisions": [1, true, ["a", "b"], {"definition": "nested"}],
            "key_concepts": "Single concept",
            "assignments": null
        }));

        assert_eq!(summary.decisions, ["1", "true", "a; b", "nested"]);
        assert_eq!(summary.key_concepts, ["Single concept"]);
        assert!(summary.assignments.is_empty());
        assert_eq!(report.dropped, 0);
        assert!(report.flattened.contains(&"decisions[0]".to_owned()));
        assert!(report.flattened.contains(&"decisions[2]".to_owned()));
        assert!(report.flattened.contains(&"key_concepts".to_owned()));
        assert!(report.flattened.contains(&"assignments".to_owned()));
    }

    #[test]
    fn optional_strings_accept_objects_numbers_and_blank_values() {
        let (summary, report) = parse_ok(serde_json::json!({
            "overview": "Assignments.",
            "action_items": [
                {
                    "text": "Ship",
                    "assignee": {"name": "Ana"},
                    "deadline_phrase": "   ",
                    "normalized_deadline": 20261103
                },
                {"task": "Send the report", "assignee_speaker_id": 7}
            ]
        }));

        assert_eq!(summary.action_items.len(), 2);
        let first = &summary.action_items[0];
        assert_eq!(first.assignee.as_deref(), Some("Ana"));
        assert_eq!(first.deadline_phrase, None);
        assert_eq!(first.normalized_deadline.as_deref(), Some("20261103"));
        let second = &summary.action_items[1];
        assert_eq!(second.text, "Send the report");
        assert_eq!(second.assignee_speaker_id.as_deref(), Some("7"));
        assert!(report
            .flattened
            .contains(&"action_items[0].deadline_phrase".to_owned()));
    }

    #[test]
    fn unusable_action_items_and_citations_are_counted_and_dropped() {
        let (summary, report) = parse_ok(serde_json::json!({
            "overview": "Drops.",
            "action_items": [{"assignee": "Ana"}, {"task": "Do it"}],
            "citations": [
                {"kind": "transcript"},
                {"kind": "transcript", "ref_id": "seg1"}
            ]
        }));

        assert_eq!(summary.action_items.len(), 1);
        assert_eq!(summary.action_items[0].text, "Do it");
        assert_eq!(summary.citations.len(), 1);
        assert_eq!(summary.citations[0].ref_id, "seg1");
        assert_eq!(report.dropped, 2);
        assert_eq!(
            normalization_disclosure(&report),
            REFORMATTED_AND_DROPPED_DISCLOSURE
        );
    }

    #[test]
    fn overview_arrays_and_objects_are_flattened() {
        let (joined, _) = parse_ok(serde_json::json!({
            "overview": ["The team shipped.", "Delivery is Friday."]
        }));
        assert_eq!(joined.overview, "The team shipped. Delivery is Friday.");

        let (object, report) = parse_ok(serde_json::json!({
            "overview": {"title": "Delivery", "description": "The team shipped."}
        }));
        assert_eq!(object.overview, "Delivery: The team shipped.");
        assert!(report.flattened.contains(&"overview".to_owned()));
    }

    #[test]
    fn numeric_string_citation_fields_are_accepted() {
        let (summary, report) = parse_ok(serde_json::json!({
            "overview": "Citations.",
            "citations": [{
                "kind": "transcript",
                "ref_id": "seg1",
                "start_seconds": "0.5",
                "end_seconds": "2",
                "page_number": "3"
            }]
        }));

        let citation = &summary.citations[0];
        assert_eq!(citation.start_seconds, Some(0.5));
        assert_eq!(citation.end_seconds, Some(2.0));
        assert_eq!(citation.page_number, Some(3));
        assert!(report
            .flattened
            .contains(&"citations[0].start_seconds".to_owned()));
        assert!(report
            .flattened
            .contains(&"citations[0].page_number".to_owned()));
    }

    #[test]
    fn a_correct_payload_produces_no_report_and_no_extra_disclosure() {
        let db = fixture_db();
        let service = SummarizationService::new(db);
        let fake = FakeProvider {
            response: serde_json::json!({
                "title": "Sprint delivery",
                "overview": "The team agreed on a sprint delivery date.",
                "decisions": ["Ship by Friday"],
                "action_items": [],
                "citations": [],
                "key_concepts": [], "assignments": [], "important_dates": [], "disclosures": []
            })
            .to_string(),
        };

        let revision = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap();

        assert!(disclosures(&revision).is_empty());
        assert!(!revision.markdown.contains(REFORMATTED_DISCLOSURE));
    }

    #[test]
    fn a_normalized_payload_adds_exactly_one_disclosure() {
        let db = fixture_db();
        let service = SummarizationService::new(db);
        let fake = FakeProvider {
            response: serde_json::json!({
                "title": "Sprint delivery",
                "overview": "The team agreed on a sprint delivery date.",
                "decisions": ["Ship by Friday"],
                "action_items": [],
                "citations": [{"kind": "transcript", "ref_id": "seg1"}],
                "key_concepts": [{"term": "Sprint", "definition": "a fixed delivery window"}],
                "assignments": [], "important_dates": [], "disclosures": []
            })
            .to_string(),
        };

        let revision = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap();

        assert_eq!(disclosures(&revision), [REFORMATTED_DISCLOSURE]);
        assert!(!revision.markdown.contains('{'));
        assert!(revision
            .markdown
            .contains("- Sprint: a fixed delivery window"));
    }

    #[test]
    fn non_json_output_is_still_an_error() {
        let error = parse_generated_summary("the model rambled without JSON").unwrap_err();
        assert!(error.message.contains("not JSON"));
    }

    #[test]
    fn a_missing_overview_is_still_an_error() {
        let error =
            parse_generated_summary(&serde_json::json!({"decisions": []}).to_string()).unwrap_err();
        assert!(error.message.contains("overview"), "{}", error.message);
    }

    #[test]
    fn an_unknown_evidence_reference_is_still_an_error() {
        let db = fixture_db();
        let service = SummarizationService::new(db);
        let fake = FakeProvider {
            response: serde_json::json!({
                "title": "Sprint delivery",
                "overview": "The team agreed on a sprint delivery date.",
                "action_items": [{
                    "text": "Ship the sprint",
                    "evidence_refs": ["ghost-segment"]
                }],
                "citations": [],
                "key_concepts": [], "assignments": [], "important_dates": [], "disclosures": []
            })
            .to_string(),
        };

        let error = service
            .summarize_with_provider("m", &snapshot(), &fake, false)
            .unwrap_err();

        assert!(error.contains("unknown source"), "{error}");
    }

    /// Records the response format of every pass, so the constrained final call
    /// can be told apart from the free-text folding calls.
    struct FormatCaptureProvider {
        summary: String,
        formats: std::sync::Mutex<Vec<Option<ResponseFormat>>>,
    }

    impl FormatCaptureProvider {
        fn new(summary: &str) -> Self {
            Self {
                summary: summary.to_owned(),
                formats: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn formats(&self) -> Vec<Option<ResponseFormat>> {
            self.formats.lock().unwrap().clone()
        }
    }

    impl CompletionProvider for FormatCaptureProvider {
        fn complete(
            &self,
            request: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, LlmError> {
            self.formats
                .lock()
                .unwrap()
                .push(request.response_format.clone());
            let is_fold = request.messages.iter().any(|message| {
                message
                    .content
                    .starts_with("Summarize this sequential transcript passage")
            });
            Ok(ChatCompletionResponse {
                content: if is_fold {
                    "note: the team agreed on a sprint date. ".repeat(4)
                } else {
                    self.summary.clone()
                },
                model: Some("fake".into()),
                prompt_tokens: None,
                completion_tokens: None,
            })
        }
    }

    fn plain_summary_json() -> String {
        serde_json::json!({
            "title": "Sprint delivery",
            "overview": "The team agreed on a sprint delivery date.",
            "decisions": ["Ship by Friday"],
            "action_items": [],
            "citations": [],
            "key_concepts": [], "assignments": [], "important_dates": [], "disclosures": []
        })
        .to_string()
    }

    #[test]
    fn the_summary_request_carries_the_schema() {
        let db = fixture_db();
        let service = SummarizationService::new(db);
        let provider = FormatCaptureProvider::new(&plain_summary_json());

        service
            .summarize_with_provider("m", &snapshot(), &provider, false)
            .unwrap();

        let formats = provider.formats();
        assert_eq!(formats.len(), 1, "a short transcript needs one pass");
        match &formats[0] {
            Some(ResponseFormat::JsonSchema { name, schema }) => {
                assert_eq!(name, "meeting_summary");
                assert_eq!(schema, &summary_json_schema());
            }
            other => panic!("the summary request must carry the schema, got {other:?}"),
        }
    }

    #[test]
    fn folding_passes_stay_free_text_and_only_the_final_pass_is_constrained() {
        let db = fixture_db();
        let long = "The team agreed to ship the sprint by Friday. ".repeat(1_100);
        db.run(move |connection| {
            connection
                .execute(
                    "UPDATE transcript_segments SET text=?1 WHERE id='seg1'",
                    [&long],
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap();
        let service = SummarizationService::new(db);
        let provider = FormatCaptureProvider::new(&plain_summary_json());

        service
            .summarize_with_provider("m", &snapshot(), &provider, false)
            .unwrap();

        let formats = provider.formats();
        assert!(formats.len() > 1, "a long transcript must be folded first");
        let (last, folds) = formats.split_last().unwrap();
        assert!(
            folds.iter().all(Option::is_none),
            "folding notes are free text and must not be constrained"
        );
        assert!(matches!(last, Some(ResponseFormat::JsonSchema { .. })));
    }

    fn complete_summary_sample() -> GeneratedSummary {
        GeneratedSummary {
            title: Some("Sprint delivery".into()),
            overview: "The team agreed the sprint scope and the delivery date.".into(),
            action_items: vec![GeneratedActionItem {
                text: "Send the report".into(),
                assignee_speaker_id: Some("sp1".into()),
                assignee: Some("Ana".into()),
                deadline_phrase: Some("Friday".into()),
                normalized_deadline: None,
                evidence_refs: vec![Value::String("seg1".into())],
            }],
            decisions: vec!["Ship the report by Friday".into()],
            key_concepts: vec!["Idempotency: repeating a call has the same effect".into()],
            assignments: vec!["Ana owns the report".into()],
            important_dates: vec!["2026-11-03: project demo".into()],
            citations: vec![Citation {
                kind: "transcript".into(),
                ref_id: "seg1".into(),
                start_seconds: Some(0.0),
                end_seconds: Some(2.5),
                page_number: None,
            }],
            disclosures: vec!["Some slides were unavailable.".into()],
        }
    }

    /// Manual helper: prints the schema the summary request sends, so it can be
    /// fed to the pinned engine's `json-schema-to-grammar` when checking that a
    /// schema change still converts.
    ///   cargo test print_summary_schema -- --ignored --nocapture
    #[test]
    #[ignore]
    fn print_summary_schema() {
        println!(
            "SUMMARY_SCHEMA:{}",
            serde_json::to_string(&summary_json_schema()).unwrap()
        );
    }

    fn summary_validator() -> jsonschema::Validator {
        jsonschema::validator_for(&summary_json_schema())
            .expect("the summary schema must be a valid JSON Schema")
    }

    #[test]
    fn the_schema_accepts_a_full_and_a_quiet_summary() {
        let validator = summary_validator();
        let full = serde_json::to_value(complete_summary_sample()).unwrap();
        let errors = validator
            .iter_errors(&full)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "the schema must accept a real summary: {errors:?}"
        );

        // A meeting with no decisions still has every key, with empty lists.
        let quiet = serde_json::to_value(GeneratedSummary {
            title: None,
            overview: "No speech was detected in this recording.".into(),
            action_items: Vec::new(),
            decisions: Vec::new(),
            key_concepts: Vec::new(),
            assignments: Vec::new(),
            important_dates: Vec::new(),
            citations: Vec::new(),
            disclosures: vec!["The transcript contains no speech segments.".into()],
        })
        .unwrap();
        assert!(validator.is_valid(&quiet));
    }

    #[test]
    fn the_schema_rejects_the_structured_lists_from_the_bug_report() {
        let validator = summary_validator();
        let sample = serde_json::to_value(complete_summary_sample()).unwrap();

        // The reported failure: an object where a plain string belongs.
        let mut value = sample.clone();
        value["key_concepts"] = serde_json::json!([
            {"term": "Idempotency", "definition": "repeating a call has the same effect"}
        ]);
        assert!(
            !validator.is_valid(&value),
            "an object inside key_concepts must be rejected"
        );

        // Every string list has the same rule, and so does a nested list.
        for field in ["decisions", "assignments", "important_dates", "disclosures"] {
            let mut value = sample.clone();
            value[field] = serde_json::json!([{"date": "2026-11-03", "event": "demo"}]);
            assert!(
                !validator.is_valid(&value),
                "an object inside {field} must be rejected"
            );
        }
        let mut value = sample.clone();
        value["action_items"][0]["evidence_refs"] = serde_json::json!([{"id": "seg1"}]);
        assert!(!validator.is_valid(&value));

        // The grammar always emits every key, so a missing key is invalid too.
        let mut value = sample;
        value.as_object_mut().unwrap().remove("disclosures");
        assert!(!validator.is_valid(&value));
    }
}
