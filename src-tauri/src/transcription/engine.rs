use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command};
use thiserror::Error;
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    Metal,
    Cuda,
    Rocm,
    Vulkan,
    Cpu,
}

pub fn detect_backend() -> BackendKind {
    if let Ok(forced) = std::env::var("LOCUS_FORCE_GPU_BACKEND") {
        if let Some(backend) = parse_backend(&forced) {
            return backend;
        }
    }
    #[cfg(target_os = "macos")]
    {
        let profile = system_profiler_displays().unwrap_or_default();
        let intel = std::env::consts::ARCH == "x86_64";
        let amd_discrete = parse_macos_amd_discrete(&profile);
        if !intel {
            return BackendKind::Metal;
        }
        if amd_discrete && vulkan_runtime_ready() {
            return BackendKind::Vulkan;
        }
        BackendKind::Cpu
    }
    #[cfg(not(target_os = "macos"))]
    {
        if std::env::var_os("LOCUS_CUDA_DEVICE").is_some() {
            return BackendKind::Cuda;
        }
        if std::env::var_os("LOCUS_ROCM_DEVICE").is_some() {
            return BackendKind::Rocm;
        }
        if std::env::var_os("LOCUS_VULKAN_DEVICE").is_some() {
            return BackendKind::Vulkan;
        }
        BackendKind::Cpu
    }
}

pub fn parse_backend(value: &str) -> Option<BackendKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "metal" => Some(BackendKind::Metal),
        "cuda" => Some(BackendKind::Cuda),
        "rocm" => Some(BackendKind::Rocm),
        "vulkan" => Some(BackendKind::Vulkan),
        "cpu" => Some(BackendKind::Cpu),
        _ => None,
    }
}

pub fn vulkan_runtime_ready() -> bool {
    std::env::var("LOCUS_VULKAN_ENABLED")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
        || std::env::var("LOCUS_VULKAN_RUNTIME")
            .map(|path| Path::new(&path).is_file())
            .unwrap_or(false)
}

pub fn vulkan_environment() -> [(&'static str, String); 2] {
    [
        ("GGML_VK_DISABLE_F16", "1".into()),
        (
            "GGML_VK_VISIBLE_DEVICES",
            std::env::var("LOCUS_VULKAN_DEVICE").unwrap_or_else(|_| "0".into()),
        ),
    ]
}

pub fn parse_macos_amd_discrete(system_profiler_output: &str) -> bool {
    let output = system_profiler_output.to_ascii_lowercase();
    (output.contains("amd") || output.contains("radeon"))
        && !output.contains("integrated")
        && (output.contains("vram") || output.contains("discrete") || output.contains("radeon pro"))
}

fn system_profiler_displays() -> Option<String> {
    Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn apply_vulkan_environment() {
    for (key, value) in vulkan_environment() {
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_seconds: f32,
    pub end_seconds: f32,
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Error)]
pub enum TranscriptionError {
    #[error("unable to load Whisper model: {0}")]
    Model(String),
    #[error("Whisper inference failed: {0}")]
    Inference(String),
    #[error("audio chunk is empty")]
    EmptyAudio,
}

pub struct WhisperEngine {
    context: WhisperContext,
    pub backend: BackendKind,
}

impl WhisperEngine {
    pub fn load(model_path: impl AsRef<Path>) -> Result<Self, TranscriptionError> {
        let backend = detect_backend();
        if backend == BackendKind::Vulkan {
            apply_vulkan_environment();
        }
        WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .map(|context| Self { context, backend })
            .map_err(|error| TranscriptionError::Model(error.to_string()))
    }

    pub fn transcribe_chunk(
        &mut self,
        audio: &[f32],
        offset_seconds: f32,
        language: Option<&str>,
    ) -> Result<Vec<TranscriptSegment>, TranscriptionError> {
        if audio.is_empty() {
            return Err(TranscriptionError::EmptyAudio);
        }
        let mut state = self
            .context
            .create_state()
            .map_err(|e| TranscriptionError::Inference(e.to_string()))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(language);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        state
            .full(params, audio)
            .map_err(|e| TranscriptionError::Inference(e.to_string()))?;
        let detected_language = language
            .map(str::to_owned)
            .or_else(|| get_lang_str(state.full_lang_id_from_state()).map(str::to_owned));
        let mut segments = Vec::new();
        for index in 0..state.full_n_segments() {
            if let Some(segment) = state.get_segment(index) {
                segments.push(TranscriptSegment {
                    start_seconds: offset_seconds + segment.start_timestamp() as f32 / 100.0,
                    end_seconds: offset_seconds + segment.end_timestamp() as f32 / 100.0,
                    text: segment
                        .to_str()
                        .map_err(|e| TranscriptionError::Inference(e.to_string()))?
                        .trim()
                        .to_owned(),
                    language: detected_language.clone(),
                    confidence: Some(1.0 - segment.no_speech_probability()),
                });
            }
        }
        Ok(segments)
    }
}

/// Merge independently decoded chunks back onto the recording timeline and
/// remove repeated boundary text emitted by overlapping Whisper windows.
pub fn merge_transcript_segments(mut segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    segments.sort_by(|left, right| left.start_seconds.total_cmp(&right.start_seconds));
    let mut merged: Vec<TranscriptSegment> = Vec::with_capacity(segments.len());
    for segment in segments {
        let normalized = normalize_text(&segment.text);
        if let Some(previous) = merged.last_mut() {
            let previous_normalized = normalize_text(&previous.text);
            let overlaps = segment.start_seconds <= previous.end_seconds + 0.25;
            if overlaps && (normalized == previous_normalized || normalized.is_empty()) {
                previous.end_seconds = previous.end_seconds.max(segment.end_seconds);
                continue;
            }
        }
        merged.push(segment);
    }
    merged
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_boundary_segments_are_collapsed() {
        let merged = merge_transcript_segments(vec![
            TranscriptSegment {
                start_seconds: 0.0,
                end_seconds: 2.0,
                text: " Hello world ".into(),
                language: Some("en".into()),
                confidence: Some(0.9),
            },
            TranscriptSegment {
                start_seconds: 1.95,
                end_seconds: 3.0,
                text: "hello   world".into(),
                language: Some("en".into()),
                confidence: Some(0.9),
            },
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].end_seconds, 3.0);
    }

    #[test]
    fn amd_macos_profile_requires_a_discrete_gpu_signal() {
        assert!(parse_macos_amd_discrete(
            "AMD Radeon Pro 5500M: VRAM (Total): 4 GB"
        ));
        assert!(!parse_macos_amd_discrete(
            "Intel Iris Plus Graphics: Integrated"
        ));
    }

    #[test]
    fn backend_parser_and_vulkan_workarounds_are_stable() {
        assert_eq!(parse_backend("vulkan"), Some(BackendKind::Vulkan));
        assert_eq!(
            vulkan_environment()[0],
            ("GGML_VK_DISABLE_F16", "1".to_string())
        );
    }
}
