//! What each pipeline step actually does.
//!
//! The orchestrator decides *when* a step may run; this module runs it and
//! reports a [`StepOutcome`]. The rules every step follows:
//!
//! * Required steps (`encoding`, `transcription`, `summarization`) either
//!   finish, fail loudly, or block with an instruction the user can act on.
//! * Optional steps (`diarization`, `slide_extraction`, `ocr`, `indexing`) are
//!   skipped, with a reason, when the capability they need is missing. They
//!   never invent output (no guessed speakers, no empty OCR pretending to be
//!   text).
//! * Long loops report progress and stop when the job is no longer current
//!   (meeting deleted, attempt superseded).

use super::{
    audio_io::{self, ENVELOPE_FPS},
    diarization::{self, DiarizationOutcome},
    orchestrator::PipelineJob,
    slides, transcription as vad_step,
};
use crate::{
    contracts::CaptureLifecycle,
    db::Database,
    llm::generation_server::build_runtime_provider,
    sidecar::{
        client::{SidecarClient, SidecarError},
        protocol::ProtocolError,
    },
    transcription::{
        chunk_speech_regions, merge_transcript_segments, TranscriptSegment, TranscriptionError,
        WhisperEngine,
    },
    AppState,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Heavy sidecar calls (VAD, diarization, slide detection) scale with
/// recording length; the client default of 10 s is only for handshakes.
pub const LONG_SIDECAR_TIMEOUT: Duration = Duration::from_secs(3 * 60 * 60);
const DEFAULT_SIDECAR_TIMEOUT: Duration = Duration::from_secs(10);
/// Whisper needs at least a second of audio; shorter slivers are skipped.
const MIN_CHUNK_SAMPLES: usize = audio_io::WHISPER_SAMPLE_RATE as usize / 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Finished. The optional note is kept as the job's reason for transparency.
    Done(Option<String>),
    /// Optional capability unavailable or nothing to do; dependents continue.
    Skipped(String),
    /// Needs the user (missing model, no provider selected). Retryable.
    Blocked(String),
    /// Not ready yet; stays pending and is looked at again later.
    Deferred,
    Failed(String),
    Canceled,
}

pub fn run_step(state: &AppState, job: &PipelineJob) -> StepOutcome {
    match job.kind.as_str() {
        "encoding" => encoding(state, job),
        "transcription" => transcription(state, job),
        "diarization" => diarization_step(state, job),
        "slide_extraction" => slide_extraction(state, job),
        "ocr" => ocr(state, job),
        "summarization" => summarization(state, job),
        "indexing" => indexing(state, job),
        other => StepOutcome::Failed(format!("unknown pipeline step: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingFacts {
    pub lifecycle: String,
    pub language_selected: Option<String>,
    pub single_person_mic: bool,
    pub has_screen: bool,
}

pub fn meeting_facts(db: &Database, meeting_id: &str) -> Result<MeetingFacts, String> {
    let meeting_id = meeting_id.to_owned();
    db.run(move |connection| {
        connection
            .query_row(
                "SELECT lifecycle, language_selected, single_person_mic, capture_sources FROM meetings WHERE id = ?1",
                [&meeting_id],
                |row| {
                    let sources: String = row.get(3)?;
                    Ok(MeetingFacts {
                        lifecycle: row.get(0)?,
                        language_selected: row
                            .get::<_, Option<String>>(1)?
                            .map(|value| value.trim().to_owned())
                            .filter(|value| !value.is_empty() && value != "auto"),
                        single_person_mic: row.get::<_, i64>(2)? != 0,
                        has_screen: sources.to_ascii_lowercase().contains("screen"),
                    })
                },
            )
            .map_err(|e| format!("meeting not found: {e}"))
    })
    .map_err(|e| e.to_string())
}

fn meeting_dir(state: &AppState, meeting_id: &str) -> PathBuf {
    state.paths.media.join(meeting_id)
}

fn audio_path(state: &AppState, meeting_id: &str) -> PathBuf {
    meeting_dir(state, meeting_id).join(crate::capture::audio::MIXED_FILE_NAME)
}

fn video_path(state: &AppState, meeting_id: &str) -> PathBuf {
    meeting_dir(state, meeting_id).join(crate::capture::screen::VIDEO_FILE_NAME)
}

fn capture_active(state: &AppState) -> bool {
    !matches!(state.capture.state().state, CaptureLifecycle::Idle)
}

/// Capture has priority (SPEC / PRD). Between work units, wait while a new
/// recording is active. Returns false if the job stopped being current.
fn wait_for_idle_capture(state: &AppState, job: &PipelineJob) -> bool {
    while capture_active(state) {
        if !state.pipeline.is_current(job) {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    state.pipeline.is_current(job)
}

fn with_sidecar<T>(
    state: &AppState,
    timeout: Duration,
    call: impl FnOnce(&mut SidecarClient) -> Result<T, SidecarError>,
) -> Result<T, SidecarError> {
    let mut manager = state
        .sidecar
        .lock()
        .map_err(|_| SidecarError::Process("sidecar lock is poisoned".into()))?;
    let client = manager.ensure_started()?;
    client.set_timeout(timeout);
    let result = call(client);
    client.set_timeout(DEFAULT_SIDECAR_TIMEOUT);
    result
}

/// True when the sidecar cannot do the work in this install (not startable,
/// dependency or model missing) as opposed to failing on this recording.
pub fn sidecar_capability_missing(error: &SidecarError) -> bool {
    match error {
        SidecarError::NotRunning | SidecarError::Process(_) => true,
        SidecarError::Protocol(ProtocolError::Remote(code, _))
            if *code == crate::sidecar::protocol::CAPABILITY_UNAVAILABLE_CODE =>
        {
            true
        }
        // Older sidecars reported missing packages as a generic error.
        SidecarError::Protocol(ProtocolError::Remote(_, message)) => {
            let message = message.to_ascii_lowercase();
            message.contains("required for")
                || message.contains("not installed")
                || message.contains("not found")
                || message.contains("no module named")
        }
        _ => false,
    }
}

/// What the user reads when an optional add-on is missing. The technical reason (and how to fix it)
/// goes to the log, not the interface.
pub fn unavailable_message(step: &str) -> String {
    match step {
        "diarization" => "Speaker labels aren't available because speaker detection isn't installed on this computer.".into(),
        "slides" => "Slides weren't detected because slide detection isn't installed on this computer.".into(),
        "ocr" => "Slide text wasn't read because text recognition isn't installed on this computer.".into(),
        other => format!("{other} isn't available on this computer."),
    }
}

fn optional_sidecar_failure(step: &str, error: SidecarError) -> StepOutcome {
    if sidecar_capability_missing(&error) {
        eprintln!("locus: {step} is unavailable: {error}");
        StepOutcome::Skipped(unavailable_message(step))
    } else {
        StepOutcome::Failed(error.to_string())
    }
}

// ---------------------------------------------------------------------------
// encoding: the recording must be finalized and on disk
// ---------------------------------------------------------------------------

fn encoding(state: &AppState, job: &PipelineJob) -> StepOutcome {
    let facts = match meeting_facts(&state.database, &job.meeting_id) {
        Ok(facts) => facts,
        Err(error) => return StepOutcome::Failed(error),
    };
    let audio = audio_path(state, &job.meeting_id);
    let video = video_path(state, &job.meeting_id);
    judge_encoding(&facts.lifecycle, audio.is_file(), video.is_file())
}

pub fn judge_encoding(lifecycle: &str, has_audio: bool, has_video: bool) -> StepOutcome {
    match lifecycle {
        "saved" if has_audio || has_video => StepOutcome::Done(None),
        "saved" => StepOutcome::Failed("The recording's media files are missing.".into()),
        "deleted" => StepOutcome::Canceled,
        "interrupted" => StepOutcome::Blocked(
            "The recording was interrupted. Recover it from the recovery prompt first.".into(),
        ),
        _ => StepOutcome::Deferred, // still recording / finalizing
    }
}

// ---------------------------------------------------------------------------
// transcription
// ---------------------------------------------------------------------------

fn whisper_model(state: &AppState) -> Option<PathBuf> {
    let selection: Option<String> = state
        .database
        .run(|connection| {
            Ok(connection
                .query_row(
                    "SELECT value_json FROM settings WHERE key='transcription.whisper_selection'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .ok())
        })
        .ok()
        .flatten();
    crate::paths::find_whisper_model(
        &state.paths,
        selection.as_deref(),
        std::env::var_os(crate::paths::ENV_WHISPER_MODEL).map(PathBuf::from),
    )
}

/// VAD regions from the sidecar's model when available, otherwise the Rust
/// energy detector, so transcription never depends on Python being installed.
fn plan_chunks(
    state: &AppState,
    audio: &Path,
    envelope: &[f32],
) -> (Vec<crate::transcription::TranscriptionChunk>, &'static str) {
    let audio_str = audio.to_string_lossy().to_string();
    let sidecar = with_sidecar(state, LONG_SIDECAR_TIMEOUT, |client| {
        vad_step::vad_chunks(client, &audio_str, envelope, ENVELOPE_FPS)
    });
    match sidecar {
        Ok(chunks) => (chunks, "sidecar VAD"),
        Err(_) => {
            let regions = audio_io::energy_speech_regions(envelope, ENVELOPE_FPS);
            (chunk_speech_regions(&regions, envelope, ENVELOPE_FPS), "energy VAD")
        }
    }
}

/// Whisper emits bracketed placeholders for silence; they are not speech.
pub fn is_non_speech(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let inner = lowered
        .trim_matches(|c| matches!(c, '[' | ']' | '(' | ')' | '*' | ' '))
        .replace('_', " ");
    trimmed.len() < 40
        && (lowered.starts_with('[') || lowered.starts_with('('))
        && matches!(
            inner.as_str(),
            "blank audio" | "silence" | "no audio" | "no speech" | "inaudible"
        )
}

pub fn clean_segments(segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    // Drop silence placeholders first: a "[BLANK_AUDIO]" sitting between two
    // copies of a boundary phrase would otherwise stop them from merging.
    merge_transcript_segments(
        segments
            .into_iter()
            .filter(|segment| !is_non_speech(&segment.text))
            .collect(),
    )
}

/// Up to `max` indices spread evenly over `len` items (always includes the first).
pub fn pick_indices(len: usize, max: usize) -> Vec<usize> {
    if len == 0 || max == 0 {
        return Vec::new();
    }
    if len <= max {
        return (0..len).collect();
    }
    (0..max).map(|slot| slot * (len - 1) / (max - 1).max(1)).collect()
}

/// Whisper emits very little text when it is decoding noise or the wrong
/// language. Speech this long with this few words is worth telling the user.
pub fn transcript_density_warning(speech_seconds: f32, words: usize) -> bool {
    speech_seconds >= 30.0 && (words as f32 / speech_seconds) < 0.4
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LanguageBasis {
    /// The user picked the language.
    Chosen,
    /// Detected from the audio, with this probability.
    Detected(f32),
    Unknown,
}

pub fn language_name(code: &str) -> String {
    let name = match code {
        "en" => "English", "es" => "Spanish", "fr" => "French", "de" => "German", "it" => "Italian",
        "pt" => "Portuguese", "nl" => "Dutch", "ca" => "Catalan", "ru" => "Russian", "uk" => "Ukrainian",
        "pl" => "Polish", "cs" => "Czech", "ro" => "Romanian", "hu" => "Hungarian", "el" => "Greek",
        "sv" => "Swedish", "da" => "Danish", "no" => "Norwegian", "fi" => "Finnish", "tr" => "Turkish",
        "ar" => "Arabic", "he" => "Hebrew", "fa" => "Persian", "hi" => "Hindi", "zh" => "Chinese",
        "ja" => "Japanese", "ko" => "Korean", "th" => "Thai", "vi" => "Vietnamese", "id" => "Indonesian",
        "tl" => "Tagalog",
        _ => return code.to_uppercase(),
    };
    name.to_owned()
}

/// The one-line result a person sees for the transcription step. Technical detail
/// (voice-activity source, device, model) is logged instead.
pub fn describe_transcription(
    passages: usize,
    speech_seconds: f32,
    words: usize,
    language: Option<&str>,
    basis: LanguageBasis,
) -> String {
    if passages == 0 {
        return if speech_seconds <= 0.0 {
            "No speech was found in this recording.".into()
        } else {
            "No words could be recognised in this recording.".into()
        };
    }
    let noun = if passages == 1 { "passage" } else { "passages" };
    let mut text = format!("Transcribed {passages} {noun}");
    match (language, basis) {
        (Some(code), LanguageBasis::Detected(probability)) if probability < 0.5 => text.push_str(&format!(
            ". The language looks like {} but isn't certain; set it yourself and re-run if that's wrong.",
            language_name(code)
        )),
        (Some(code), _) => text.push_str(&format!(" in {}.", language_name(code))),
        (None, _) => text.push('.'),
    }
    if transcript_density_warning(speech_seconds, words) {
        text.push_str(" Very little text was recognised for this much speech, so the language or the audio source may be wrong.");
    }
    text
}

/// Clips for language detection: a handful of the longer chunks spread over the
/// recording, each at most 30 s, rather than the first chunk alone.
fn language_clips(
    audio: &Path,
    chunks: &[crate::transcription::TranscriptionChunk],
) -> Vec<Vec<f32>> {
    let long: Vec<_> = chunks
        .iter()
        .filter(|chunk| chunk.end_seconds - chunk.start_seconds >= 3.0)
        .collect();
    let pool: Vec<_> = if long.is_empty() { chunks.iter().collect() } else { long };
    pick_indices(pool.len(), 4)
        .into_iter()
        .filter_map(|index| {
            let chunk = pool[index];
            let end = chunk.end_seconds.min(chunk.start_seconds + 30.0);
            audio_io::read_range_16k(audio, chunk.start_seconds, end).ok()
        })
        .collect()
}

fn transcription(state: &AppState, job: &PipelineJob) -> StepOutcome {
    let meeting_id = job.meeting_id.as_str();
    let facts = match meeting_facts(&state.database, meeting_id) {
        Ok(facts) => facts,
        Err(error) => return StepOutcome::Failed(error),
    };
    let audio = audio_path(state, meeting_id);
    if !audio.is_file() {
        return StepOutcome::Failed(format!("Recording audio not found at {}", audio.display()));
    }
    let Some(model) = whisper_model(state) else {
        return StepOutcome::Blocked(format!(
            "No Whisper model found. Add a ggml Whisper model (for example ggml-small.bin) to {} and retry.",
            state.paths.whisper_models().display()
        ));
    };

    let _ = state.pipeline.set_progress(&job.id, 0.02);
    let envelope = match audio_io::energy_envelope(&audio) {
        Ok(envelope) => envelope,
        Err(error) => return StepOutcome::Failed(error),
    };
    let (chunks, vad_source) = plan_chunks(state, &audio, &envelope);
    let speech_seconds: f32 = chunks.iter().map(|c| c.end_seconds - c.start_seconds).sum();
    let _ = state.pipeline.set_progress(&job.id, 0.06);

    let mut collected: Vec<TranscriptSegment> = Vec::new();
    let mut device = String::from("no speech");
    let mut language: Option<String> = facts.language_selected.clone();
    let mut language_basis = if language.is_some() { LanguageBasis::Chosen } else { LanguageBasis::Unknown };
    if !chunks.is_empty() {
        let mut engine = match WhisperEngine::load(&model) {
            Ok(engine) => engine,
            Err(error) => return StepOutcome::Failed(error.to_string()),
        };
        device = engine.device_label();

        // One language for the whole meeting. Detecting per chunk let short,
        // noisy chunks flip between languages (English commentary came out as
        // Catalan and Dutch) and each wrong guess decodes to gibberish.
        if language.is_none() {
            let clips = language_clips(&audio, &chunks);
            match engine.detect_language(&clips) {
                Ok(Some(guess)) => {
                    language_basis = LanguageBasis::Detected(guess.probability);
                    language = Some(guess.language);
                }
                Ok(None) => {}
                Err(error) => return StepOutcome::Failed(error.to_string()),
            }
        }
        let _ = state.pipeline.set_progress(&job.id, 0.08);

        let total = chunks.len();
        for (index, chunk) in chunks.iter().enumerate() {
            if !wait_for_idle_capture(state, job) {
                return StepOutcome::Canceled;
            }
            let samples = match audio_io::read_range_16k(&audio, chunk.start_seconds, chunk.end_seconds)
            {
                Ok(samples) => samples,
                Err(error) => return StepOutcome::Failed(error),
            };
            if samples.len() >= MIN_CHUNK_SAMPLES {
                match engine.transcribe_chunk(&samples, chunk.start_seconds, language.as_deref()) {
                    Ok(mut segments) => collected.append(&mut segments),
                    Err(TranscriptionError::EmptyAudio) => {}
                    Err(error) => return StepOutcome::Failed(error.to_string()),
                }
            }
            let _ = state
                .pipeline
                .set_progress(&job.id, 0.08 + 0.90 * (index + 1) as f32 / total as f32);
        }
    }

    let segments = clean_segments(collected);
    if !state.pipeline.is_current(job) {
        return StepOutcome::Canceled;
    }
    let words: usize = segments.iter().map(|s| s.text.split_whitespace().count()).sum();
    eprintln!(
        "locus: transcription {meeting_id}: {} segments, {words} words, {vad_source}, {device}, language {:?}",
        segments.len(),
        language
    );
    let model_name = model
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "whisper".into());
    match state.transcription.persist_segments(
        meeting_id,
        &format!("whisper.cpp:{model_name}:{device}"),
        language.as_deref(),
        &segments,
    ) {
        Ok(_) => StepOutcome::Done(Some(describe_transcription(
            segments.len(),
            speech_seconds,
            words,
            language.as_deref(),
            language_basis,
        ))),
        Err(error) => StepOutcome::Failed(error),
    }
}

// ---------------------------------------------------------------------------
// diarization
// ---------------------------------------------------------------------------

fn diarization_step(state: &AppState, job: &PipelineJob) -> StepOutcome {
    let meeting_id = job.meeting_id.as_str();
    let facts = match meeting_facts(&state.database, meeting_id) {
        Ok(facts) => facts,
        Err(error) => return StepOutcome::Failed(error),
    };
    let audio = audio_path(state, meeting_id);
    if !audio.is_file() {
        return StepOutcome::Failed("Recording audio is missing.".into());
    }
    let audio_str = audio.to_string_lossy().to_string();
    let outcome = with_sidecar(state, LONG_SIDECAR_TIMEOUT, |client| {
        diarization::diarize(client, &audio_str, facts.single_person_mic)
    });
    match outcome {
        Err(error) => optional_sidecar_failure("diarization", error),
        Ok(DiarizationOutcome::Unavailable(reason)) => {
        {
            eprintln!("locus: diarization is unavailable: {reason}");
            StepOutcome::Skipped(unavailable_message("diarization"))
        }
        }
        Ok(DiarizationOutcome::Segments(segments)) if segments.is_empty() => {
            StepOutcome::Skipped("No distinct speakers were detected.".into())
        }
        Ok(DiarizationOutcome::Segments(segments)) => {
            if !state.pipeline.is_current(job) {
                return StepOutcome::Canceled;
            }
            let speakers = segments
                .iter()
                .map(|segment| segment.speaker.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            match diarization::persist_alignments(&state.database, meeting_id, &segments) {
                Ok(()) => StepOutcome::Done(Some(if speakers == 1 {
                    "1 speaker detected.".to_owned()
                } else {
                    format!("{speakers} speakers detected.")
                })),
                Err(error) => StepOutcome::Failed(error),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// slides and OCR
// ---------------------------------------------------------------------------

fn slide_extraction(state: &AppState, job: &PipelineJob) -> StepOutcome {
    let meeting_id = job.meeting_id.as_str();
    let facts = match meeting_facts(&state.database, meeting_id) {
        Ok(facts) => facts,
        Err(error) => return StepOutcome::Failed(error),
    };
    if !facts.has_screen {
        return StepOutcome::Skipped("No screen was recorded for this meeting.".into());
    }
    let video = video_path(state, meeting_id);
    if !video.is_file() {
        return StepOutcome::Skipped("This recording has no video file.".into());
    }
    let output_dir = meeting_dir(state, meeting_id).join("slides");
    if let Err(error) = std::fs::create_dir_all(&output_dir) {
        return StepOutcome::Failed(format!("cannot create slides folder: {error}"));
    }
    let (video_str, output_str) = (
        video.to_string_lossy().to_string(),
        output_dir.to_string_lossy().to_string(),
    );
    let detected = with_sidecar(state, LONG_SIDECAR_TIMEOUT, |client| {
        slides::detect(client, &video_str, Some(&output_str))
    });
    let detected = match detected {
        Ok(detected) => detected,
        Err(error) => return optional_sidecar_failure("slides", error),
    };
    if !state.pipeline.is_current(job) {
        return StepOutcome::Canceled;
    }
    match slides::persist_detected(&state.database, &state.paths.media, meeting_id, &detected) {
        Ok(unique) => StepOutcome::Done(Some(match unique {
            0 => "No slides were found.".to_owned(),
            1 => "1 slide found.".to_owned(),
            n => format!("{n} slides found."),
        })),
        Err(error) => StepOutcome::Failed(error),
    }
}

fn ocr(state: &AppState, job: &PipelineJob) -> StepOutcome {
    let meeting_id = job.meeting_id.as_str();
    let pending = match slides::pending_ocr(&state.database, &state.paths.media, meeting_id) {
        Ok(pending) => pending,
        Err(error) => return StepOutcome::Failed(error),
    };
    if pending.is_empty() {
        return StepOutcome::Skipped("There are no slides to read.".into());
    }
    let total = pending.len();
    let mut failed = 0_usize;
    for (index, (slide_id, path)) in pending.iter().enumerate() {
        if !wait_for_idle_capture(state, job) {
            return StepOutcome::Canceled;
        }
        let path_str = path.to_string_lossy().to_string();
        let text = with_sidecar(state, Duration::from_secs(5 * 60), |client| {
            slides::ocr_image(client, &path_str)
        });
        match text {
            Ok(text) => {
                if let Err(error) = slides::store_ocr(&state.database, slide_id, &text) {
                    return StepOutcome::Failed(error);
                }
            }
            // Nothing read yet: treat as "OCR is not available here", not as
            // many failed slides.
            Err(error) if index == 0 => return optional_sidecar_failure("ocr", error),
            Err(_) => failed += 1,
        }
        let _ = state
            .pipeline
            .set_progress(&job.id, (index + 1) as f32 / total as f32);
    }
    StepOutcome::Done(Some(if failed == 0 {
        format!("Text read from {total} {}.", if total == 1 { "slide" } else { "slides" })
    } else {
        format!("Text read from {} of {total} slides.", total - failed)
    }))
}

// ---------------------------------------------------------------------------
// summarization
// ---------------------------------------------------------------------------

fn summarization(state: &AppState, job: &PipelineJob) -> StepOutcome {
    // Never choose a destination for the user: without an explicit selection
    // the step waits rather than falling back to any provider.
    let snapshot = match state.providers.snapshot() {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return StepOutcome::Blocked(
                "Select an AI provider in Settings to generate the summary, then retry.".into(),
            )
        }
    };
    let keychain = crate::keychain::Keychain::new("com.locus.app");
    let provider = match build_runtime_provider(state, &snapshot, &keychain) {
        Ok(provider) => provider,
        Err(error) => return StepOutcome::Blocked(error.to_string()),
    };
    match state.summarization.summarize_with_provider(
        &job.meeting_id,
        &snapshot,
        provider.as_ref(),
        // An existing summary is only reused when nothing upstream changed. The
        // step is only pending on first run or after a re-run of earlier steps,
        // so any summary that already exists is stale.
        true,
    ) {
        Ok(_) => StepOutcome::Done(None),
        Err(error) => StepOutcome::Failed(error),
    }
}

// ---------------------------------------------------------------------------
// indexing (knowledge base)
// ---------------------------------------------------------------------------

fn indexing(state: &AppState, job: &PipelineJob) -> StepOutcome {
    let meeting_id = job.meeting_id.as_str();
    let provider = match crate::commands::knowledge::embedding_provider(state) {
        Ok(provider) => provider,
        // Semantic search is opt-in; the meeting is fully usable without it.
        Err(reason) => return StepOutcome::Skipped(reason),
    };
    if let Err(error) = state.knowledge.ensure_meeting_sources(meeting_id) {
        return StepOutcome::Failed(error);
    }
    let owned = meeting_id.to_owned();
    let sources: Result<Vec<String>, String> = state
        .database
        .run(move |connection| {
            let mut statement = connection
                .prepare("SELECT id FROM sources WHERE meeting_id = ?1 AND visibility = 'visible' ORDER BY id")
                .map_err(|e| e.to_string())?;
            let ids = statement
                .query_map([&owned], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            ids
        })
        .map_err(|e| e.to_string());
    let sources = match sources {
        Ok(sources) => sources,
        Err(error) => return StepOutcome::Failed(error),
    };
    for (index, source_id) in sources.iter().enumerate() {
        if !state.pipeline.is_current(job) {
            return StepOutcome::Canceled;
        }
        if let Err(error) = state.knowledge.index_source(source_id, &provider) {
            return StepOutcome::Failed(error);
        }
        let _ = state
            .pipeline
            .set_progress(&job.id, (index + 1) as f32 / sources.len().max(1) as f32);
    }
    StepOutcome::Done(Some(
        if sources.is_empty() { "Nothing to add to search yet." } else { "Added to search." }.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(text: &str, start: f32, end: f32) -> TranscriptSegment {
        TranscriptSegment {
            start_seconds: start,
            end_seconds: end,
            text: text.into(),
            language: Some("en".into()),
            confidence: Some(0.9),
        }
    }

    #[test]
    fn transcription_result_reads_like_a_sentence() {
        let d = |n, secs, words, lang: Option<&str>, basis| describe_transcription(n, secs, words, lang, basis);
        assert_eq!(d(11, 80.0, 220, Some("en"), LanguageBasis::Detected(0.98)), "Transcribed 11 passages in English.");
        assert_eq!(d(1, 12.0, 30, Some("es"), LanguageBasis::Chosen), "Transcribed 1 passage in Spanish.");
        assert_eq!(d(4, 40.0, 100, None, LanguageBasis::Unknown), "Transcribed 4 passages.");
        assert_eq!(d(0, 0.0, 0, None, LanguageBasis::Unknown), "No speech was found in this recording.");
        assert_eq!(d(0, 25.0, 0, Some("en"), LanguageBasis::Chosen), "No words could be recognised in this recording.");
        let unsure = d(5, 60.0, 150, Some("ca"), LanguageBasis::Detected(0.3));
        assert!(unsure.contains("looks like Catalan") && unsure.contains("re-run"), "{unsure}");
        let sparse = d(3, 120.0, 6, Some("en"), LanguageBasis::Detected(0.9));
        assert!(sparse.starts_with("Transcribed 3 passages in English.") && sparse.contains("Very little text"));
        // none of the engine jargon reaches the user
        for text in [&unsure, &sparse] {
            for jargon in ["VAD", "CPU", "GPU", "sidecar", "segments (", "%"] {
                assert!(!text.contains(jargon), "{jargon} in {text}");
            }
        }
    }

    #[test]
    fn language_codes_become_names() {
        assert_eq!(language_name("en"), "English");
        assert_eq!(language_name("nl"), "Dutch");
        assert_eq!(language_name("xx"), "XX");
    }

    #[test]
    fn missing_add_ons_are_explained_without_technical_detail() {
        for step in ["diarization", "slides", "ocr"] {
            let message = unavailable_message(step);
            for jargon in ["remote error", "-32001", "pip", "rapidocr", "pyannote", "sherpa", "Python"] {
                assert!(!message.contains(jargon), "{jargon} in {message}");
            }
            assert!(message.ends_with("on this computer."), "{message}");
        }
        assert!(unavailable_message("ocr").starts_with("Slide text wasn't read"));
    }

    #[test]
    fn language_clips_are_spread_over_the_recording() {
        assert_eq!(pick_indices(0, 4), Vec::<usize>::new());
        assert_eq!(pick_indices(3, 4), vec![0, 1, 2]);
        assert_eq!(pick_indices(10, 4), vec![0, 3, 6, 9]);
        assert_eq!(pick_indices(100, 1), vec![0]);
    }

    #[test]
    fn sparse_output_for_long_speech_is_flagged() {
        assert!(transcript_density_warning(120.0, 6));
        assert!(!transcript_density_warning(120.0, 240));
        // too little audio to judge
        assert!(!transcript_density_warning(8.0, 0));
    }

    #[test]
    fn encoding_waits_for_a_saved_recording() {
        assert_eq!(judge_encoding("saved", true, false), StepOutcome::Done(None));
        assert_eq!(judge_encoding("saved", false, true), StepOutcome::Done(None));
        assert!(matches!(
            judge_encoding("saved", false, false),
            StepOutcome::Failed(_)
        ));
        assert_eq!(judge_encoding("recording", true, true), StepOutcome::Deferred);
        assert_eq!(judge_encoding("finalizing", true, true), StepOutcome::Deferred);
        assert!(matches!(
            judge_encoding("interrupted", true, true),
            StepOutcome::Blocked(_)
        ));
        assert_eq!(judge_encoding("deleted", false, false), StepOutcome::Canceled);
    }

    #[test]
    fn silence_placeholders_are_not_speech() {
        for text in ["[BLANK_AUDIO]", " [ Silence ] ", "(silence)", "[no audio]", ""] {
            assert!(is_non_speech(text), "{text:?}");
        }
        assert!(!is_non_speech("The silence was deafening."));
        assert!(!is_non_speech("[Applause] thanks everyone for coming to the review"));
    }

    #[test]
    fn cleaning_merges_repeats_and_drops_placeholders() {
        let cleaned = clean_segments(vec![
            segment("Hello team", 0.0, 1.0),
            segment("[BLANK_AUDIO]", 1.0, 3.0),
            segment("Hello team", 1.1, 1.6),
            segment("Let's begin", 4.0, 5.0),
        ]);
        let texts: Vec<_> = cleaned.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Hello team", "Let's begin"]);
    }

    #[test]
    fn missing_sidecar_capabilities_skip_but_real_failures_do_not() {
        assert!(sidecar_capability_missing(&SidecarError::NotRunning));
        assert!(sidecar_capability_missing(&SidecarError::Process("spawn failed".into())));
        assert!(sidecar_capability_missing(&SidecarError::Protocol(
            ProtocolError::Remote(-32001, "text recognition is not installed".into())
        )));
        assert!(sidecar_capability_missing(&SidecarError::Protocol(
            ProtocolError::Remote(-32000, "rapidocr is required for OCR".into())
        )));
        assert!(!sidecar_capability_missing(&SidecarError::Protocol(
            ProtocolError::Remote(-32000, "unable to open video: x.mp4".into())
        )));
        assert!(!sidecar_capability_missing(&SidecarError::Protocol(
            ProtocolError::Timeout(Duration::from_secs(1))
        )));
        assert_eq!(
            optional_sidecar_failure("ocr", SidecarError::NotRunning),
            StepOutcome::Skipped(unavailable_message("ocr"))
        );
        assert!(matches!(
            optional_sidecar_failure(
                "ocr",
                SidecarError::Protocol(ProtocolError::Timeout(Duration::from_secs(1)))
            ),
            StepOutcome::Failed(_)
        ));
    }

    #[test]
    fn meeting_facts_read_language_screen_and_mic_flags() {
        let db = Database::in_memory().unwrap();
        db.run(|c| {
            c.execute(
                "INSERT INTO meetings(id,title,title_origin,recorded_at,timezone,capture_sources,single_person_mic,lifecycle,language_selected) VALUES ('m','T','placeholder','now','UTC','[\"microphone\",\"screen\"]',1,'saved','auto')",
                [],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
        .unwrap();
        let facts = meeting_facts(&db, "m").unwrap();
        assert_eq!(
            facts,
            MeetingFacts {
                lifecycle: "saved".into(),
                language_selected: None, // "auto" means detect
                single_person_mic: true,
                has_screen: true,
            }
        );
        assert!(meeting_facts(&db, "nope").is_err());
    }
}
