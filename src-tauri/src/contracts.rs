use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    SystemAudio,
    Microphone,
    Screen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeetingType {
    Auto,
    Meeting,
    Lecture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureLifecycle {
    Idle,
    Preparing,
    Recording,
    Paused,
    Finalizing,
    Saved,
    Interrupted,
    Recoverable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingStateDto {
    pub capture_id: Option<String>,
    pub meeting_id: Option<String>,
    pub state: CaptureLifecycle,
    pub generation: u64,
    pub elapsed_seconds: f64,
    pub selected_sources: Vec<CaptureSource>,
    pub recoverable: bool,
    pub reason: Option<String>,
    pub warning: bool,
    #[serde(default)]
    pub system_audio_level: Option<f32>,
    #[serde(default)]
    pub mic_level: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingDto {
    pub id: String,
    pub title: String,
    pub title_origin: String,
    pub title_revision: i64,
    pub recorded_at: String,
    pub timezone: String,
    pub duration_seconds: f64,
    pub requested_type: MeetingType,
    pub detected_type: Option<String>,
    pub lifecycle: String,
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub speaker_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatusDto {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub progress: f64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideDto {
    pub id: String,
    pub ordinal: i64,
    pub timestamp: f64,
    pub image_url: String,
    pub ocr_text: String,
    #[serde(default)]
    pub repeated_timestamps: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelAssetDto {
    pub id: String,
    pub name: String,
    pub role: String,
    pub parameter_size: String,
    pub quantization: Option<String>,
    pub memory_ram: String,
    pub memory_vram: String,
    pub disk_size: String,
    pub status: String,
    pub download_progress: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageInfoDto {
    pub data_root: String,
    pub models_root: String,
    pub free_bytes: Option<u64>,
    pub capture_active: bool,
    pub migration_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateCheckDto {
    pub channel: String,
    pub current_version: String,
    pub available_version: Option<String>,
    pub package_url: Option<String>,
    pub sha256: Option<String>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBackendDto {
    pub backend: String,
    pub architecture: String,
    pub device: Option<String>,
    pub reason: String,
    pub vulkan_ready: bool,
}
