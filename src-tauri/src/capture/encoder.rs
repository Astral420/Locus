use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncoderError {
    #[error("encoder I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("ffmpeg exited with status {0}")]
    Process(i32),
    #[error("manifest is invalid: {0}")]
    Manifest(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub stream_id: String,
    pub source: String,
    pub ordinal: u64,
    pub relative_path: String,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub checksum: Option<String>,
}
/// Seconds a source's first sample came after the earliest source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackLead {
    pub source: String,
    pub lead_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub capture_id: String,
    pub meeting_id: String,
    pub generation: u64,
    pub state: String,
    pub segments: Vec<Segment>,
    // The fields below let a crashed capture be rebuilt from what is on disk.
    // They default so manifests written before NC-5 still load.
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub has_video: bool,
    /// Seconds the audio began after the first video frame (negative: before).
    #[serde(default)]
    pub audio_offset_seconds: f64,
    #[serde(default)]
    pub track_leads: Vec<TrackLead>,
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct Encoder {
    manifest_path: PathBuf,
    manifest: Manifest,
}
impl Encoder {
    pub fn create(
        root: impl AsRef<Path>,
        capture_id: impl Into<String>,
        meeting_id: impl Into<String>,
    ) -> Result<Self, EncoderError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let manifest_path = root.join("capture-manifest.json");
        let encoder = Self {
            manifest_path,
            manifest: Manifest {
                capture_id: capture_id.into(),
                meeting_id: meeting_id.into(),
                generation: 0,
                state: "recording".into(),
                segments: vec![],
                ..Manifest::default()
            },
        };
        encoder.persist()?;
        Ok(encoder)
    }
    pub fn add_segment(&mut self, segment: Segment) -> Result<(), EncoderError> {
        if segment.duration_seconds < 0.0 || segment.start_seconds < 0.0 {
            return Err(EncoderError::Manifest("negative segment time".into()));
        }
        self.manifest.segments.push(segment);
        self.persist()
    }
    /// Opens the manifest in `dir` (a meeting's media folder) for recovery.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, EncoderError> {
        let manifest_path = dir.as_ref().join("capture-manifest.json");
        let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)
            .map_err(|e| EncoderError::Manifest(e.to_string()))?;
        Ok(Self {
            manifest_path,
            manifest,
        })
    }

    /// Records how the capture was set up, so recovery can line tracks up.
    pub fn set_session(
        &mut self,
        sources: Vec<String>,
        has_video: bool,
        audio_offset_seconds: f64,
        track_leads: Vec<TrackLead>,
    ) -> Result<(), EncoderError> {
        self.manifest.sources = sources;
        self.manifest.has_video = has_video;
        self.manifest.audio_offset_seconds = audio_offset_seconds;
        self.manifest.track_leads = track_leads;
        self.persist()
    }

    pub fn set_reason(&mut self, reason: Option<String>) -> Result<(), EncoderError> {
        self.manifest.reason = reason;
        self.persist()
    }

    pub fn dir(&self) -> &Path {
        self.manifest_path.parent().unwrap_or(Path::new("."))
    }

    pub fn mark_state(&mut self, state: impl Into<String>) -> Result<(), EncoderError> {
        self.manifest.state = state.into();
        self.persist()
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn finalize(
        &mut self,
        output: impl AsRef<Path>,
        ffmpeg: impl AsRef<Path>,
    ) -> Result<(), EncoderError> {
        let root = self.manifest_path.parent().unwrap_or(Path::new("."));
        let list_path = root.join("segments.txt");
        let mut list = File::create(&list_path)?;
        for segment in &self.manifest.segments {
            let path = root.join(&segment.relative_path);
            if !path.exists() {
                return Err(EncoderError::Manifest(format!(
                    "segment does not exist: {}",
                    path.display()
                )));
            }
            writeln!(
                list,
                "file '{}'",
                path.display().to_string().replace('\'', "'\\''")
            )?;
        }
        list.sync_all()?;
        let status = std::process::Command::new(ffmpeg.as_ref())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
            ])
            .arg(&list_path)
            .args(["-c", "copy", "-y"])
            .arg(output.as_ref())
            .status()?;
        if !status.success() {
            return Err(EncoderError::Process(status.code().unwrap_or(-1)));
        }
        self.manifest.state = "saved".into();
        self.persist()
    }
    pub fn persist(&self) -> Result<(), EncoderError> {
        let temp = self.manifest_path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|e| EncoderError::Manifest(e.to_string()))?;
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(temp, &self.manifest_path)?;
        Ok(())
    }
    pub fn recoverable_manifests(root: impl AsRef<Path>) -> Result<Vec<Manifest>, EncoderError> {
        let mut manifests = vec![];
        if !root.as_ref().exists() {
            return Ok(manifests);
        }
        for entry in fs::read_dir(root)? {
            let path = entry?.path().join("capture-manifest.json");
            if path.exists() {
                // One unreadable manifest must not hide the others.
                let Ok(manifest) = fs::read(&path).map_err(|e| e.to_string()).and_then(|b| {
                    serde_json::from_slice::<Manifest>(&b).map_err(|e| e.to_string())
                }) else {
                    continue;
                };
                if matches!(
                    manifest.state.as_str(),
                    "recording" | "interrupted" | "recoverable"
                ) {
                    manifests.push(manifest);
                }
            }
        }
        Ok(manifests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_is_durable_and_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let mut encoder =
            Encoder::create(dir.path().join("meeting"), "capture", "meeting").unwrap();
        encoder
            .add_segment(Segment {
                stream_id: "mixed".into(),
                source: "mixed".into(),
                ordinal: 0,
                relative_path: "seg-0.mp4".into(),
                start_seconds: 0.0,
                duration_seconds: 2.0,
                checksum: None,
            })
            .unwrap();
        assert_eq!(Encoder::recoverable_manifests(dir.path()).unwrap().len(), 1);
    }
}
