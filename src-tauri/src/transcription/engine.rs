use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;
use thiserror::Error;
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    WhisperState,
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

#[cfg(target_os = "macos")]
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

/// Environment override for GPU use: `1`/`true` forces it, `0`/`false` forbids it.
pub const ENV_WHISPER_GPU: &str = "LOCUS_WHISPER_GPU";

/// Whether whisper.cpp may use a GPU. `detect_backend()` is the product's
/// decision (Intel Macs run on CPU); the old code ignored it and let whisper.cpp
/// pick Metal on an AMD GPU by default, where ggml's Metal backend is known to
/// produce garbage output. The override exists for experiments only.
pub fn decide_gpu(backend: BackendKind, env_override: Option<&str>) -> bool {
    match env_override.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
        _ => backend != BackendKind::Cpu,
    }
}

fn worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .clamp(1, 8)
}

#[derive(Debug, Clone, PartialEq)]
pub struct LanguageGuess {
    pub language: String,
    pub probability: f32,
    pub runner_up: Option<(String, f32)>,
}

/// Averages per-clip language probability vectors (clips of different length
/// are weighted equally; one noisy clip must not decide for the whole meeting).
pub fn average_language_probs(all: &[Vec<f32>]) -> Vec<f32> {
    let width = all.iter().map(Vec::len).max().unwrap_or(0);
    let mut average = vec![0.0_f32; width];
    for probs in all {
        for (slot, value) in average.iter_mut().zip(probs) {
            *slot += value;
        }
    }
    let count = all.len().max(1) as f32;
    average.iter_mut().for_each(|value| *value /= count);
    average
}

pub fn top_languages(average: &[f32]) -> Option<LanguageGuess> {
    let mut ranked: Vec<(usize, f32)> = average.iter().copied().enumerate().collect();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
    let name = |index: usize| get_lang_str(index as i32).map(str::to_owned);
    let (best, probability) = *ranked.first()?;
    Some(LanguageGuess {
        language: name(best)?,
        probability,
        runner_up: ranked
            .get(1)
            .and_then(|(index, value)| name(*index).map(|language| (language, *value))),
    })
}

pub struct WhisperEngine {
    #[allow(dead_code)]
    context: WhisperContext,
    state: WhisperState,
    pub backend: BackendKind,
    pub gpu: bool,
}

impl WhisperEngine {
    pub fn load(model_path: impl AsRef<Path>) -> Result<Self, TranscriptionError> {
        let backend = detect_backend();
        if backend == BackendKind::Vulkan {
            apply_vulkan_environment();
        }
        let gpu = decide_gpu(backend, std::env::var(ENV_WHISPER_GPU).ok().as_deref());
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(gpu);
        let context = WhisperContext::new_with_params(model_path, parameters)
            .map_err(|error| TranscriptionError::Model(error.to_string()))?;
        // One state for the whole job: re-creating it per chunk re-allocated
        // every compute buffer (and recompiled GPU kernels) each time.
        let state = context
            .create_state()
            .map_err(|error| TranscriptionError::Model(error.to_string()))?;
        Ok(Self {
            context,
            state,
            backend,
            gpu,
        })
    }

    /// What actually runs the model, for job notes and logs.
    pub fn device_label(&self) -> String {
        if self.gpu {
            format!("{:?} GPU", self.backend)
        } else {
            "CPU".into()
        }
    }

    /// Detects the language once from several clips (each up to 30 s of 16 kHz
    /// mono audio), so a short noisy chunk cannot flip the language.
    pub fn detect_language(
        &mut self,
        clips: &[Vec<f32>],
    ) -> Result<Option<LanguageGuess>, TranscriptionError> {
        let threads = worker_threads();
        let mut all = Vec::new();
        for clip in clips {
            if clip.len() < 16_000 {
                continue;
            }
            let clip = &clip[..clip.len().min(30 * 16_000)];
            self.state
                .pcm_to_mel(clip, threads)
                .map_err(|error| TranscriptionError::Inference(error.to_string()))?;
            let (_, probabilities) = self
                .state
                .lang_detect(0, threads)
                .map_err(|error| TranscriptionError::Inference(error.to_string()))?;
            all.push(probabilities);
        }
        Ok(top_languages(&average_language_probs(&all)))
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
        let state = &mut self.state;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(worker_threads() as i32);
        params.set_suppress_nst(true);
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
    fn intel_mac_style_cpu_decision_is_respected_unless_overridden() {
        assert!(!decide_gpu(BackendKind::Cpu, None));
        assert!(decide_gpu(BackendKind::Metal, None));
        assert!(decide_gpu(BackendKind::Cuda, None));
        assert!(decide_gpu(BackendKind::Cpu, Some("1")));
        assert!(decide_gpu(BackendKind::Cpu, Some(" TRUE ")));
        assert!(!decide_gpu(BackendKind::Metal, Some("0")));
        assert!(!decide_gpu(BackendKind::Metal, Some("off")));
        assert!(!decide_gpu(BackendKind::Cpu, Some("maybe")));
        assert!(decide_gpu(BackendKind::Metal, Some("maybe")));
    }

    #[test]
    fn language_is_decided_across_clips_not_by_one_noisy_clip() {
        // id 0 = en, id 1 = zh in whisper's table; two clips agree, one is noise.
        let mut english = vec![0.0; 99];
        english[0] = 0.8;
        english[1] = 0.1;
        let mut noisy = vec![0.0; 99];
        noisy[1] = 0.4;
        noisy[0] = 0.3;
        let average = average_language_probs(&[english.clone(), english, noisy]);
        let guess = top_languages(&average).unwrap();
        assert_eq!(guess.language, "en");
        assert!(guess.probability > 0.6);
        assert_eq!(guess.runner_up.unwrap().0, "zh");
        assert!(top_languages(&[]).is_none());
    }

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
