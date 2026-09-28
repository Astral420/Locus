use crate::{
    sidecar::client::{SidecarClient, SidecarError},
    transcription::{
        chunk_speech_regions, merge_transcript_segments, SpeechRegion, TranscriptSegment,
        TranscriptionChunk, WhisperEngine,
    },
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VadResponse {
    regions: Vec<VadRegion>,
}
#[derive(Debug, Deserialize)]
struct VadRegion {
    start: f32,
    end: f32,
}

/// Ask the Python sidecar for speech regions, then keep chunk construction in Rust
/// so timeline offsets and the five-minute bound are owned by the transcription engine.
pub fn vad_chunks(
    client: &mut SidecarClient,
    audio_path: &str,
    energy: &[f32],
    sample_rate: usize,
) -> Result<Vec<TranscriptionChunk>, SidecarError> {
    let response: VadResponse =
        client.call("vad", serde_json::json!({ "audio_path": audio_path }))?;
    let regions = response
        .regions
        .into_iter()
        .map(|region| SpeechRegion {
            start_seconds: region.start,
            end_seconds: region.end,
        })
        .collect::<Vec<_>>();
    Ok(chunk_speech_regions(&regions, energy, sample_rate))
}

pub fn transcribe_chunks(
    engine: &mut WhisperEngine,
    chunks: &[TranscriptionChunk],
    audio_chunks: &[Vec<f32>],
    language: Option<&str>,
) -> Result<Vec<TranscriptSegment>, crate::transcription::TranscriptionError> {
    let mut segments = Vec::new();
    for (chunk, audio) in chunks.iter().zip(audio_chunks) {
        segments.extend(engine.transcribe_chunk(audio, chunk.start_seconds, language)?);
    }
    Ok(merge_transcript_segments(segments))
}
