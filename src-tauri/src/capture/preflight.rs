use super::{CaptureError, CaptureOptions};
use std::{fs, path::Path, process::Command};

pub fn validate_storage(options: &CaptureOptions) -> Result<(), CaptureError> {
    options.validate()?;
    fs::create_dir_all(&options.output_root).map_err(|error| {
        CaptureError::SourceUnavailable(format!("capture storage is not writable: {error}"))
    })?;
    let probe = options.output_root.join(".locus-write-test");
    fs::write(&probe, b"locus").map_err(|error| {
        CaptureError::SourceUnavailable(format!("capture storage is not writable: {error}"))
    })?;
    let _ = fs::remove_file(probe);
    Ok(())
}

pub fn validate_encoder(ffmpeg: impl AsRef<Path>) -> Result<(), CaptureError> {
    let status = Command::new(ffmpeg.as_ref())
        .arg("-version")
        .status()
        .map_err(|error| {
            CaptureError::SourceUnavailable(format!("FFmpeg is unavailable: {error}"))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(CaptureError::SourceUnavailable(
            "FFmpeg did not start successfully".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{CaptureSource, MeetingType};
    #[test]
    fn storage_probe_requires_audio_and_writes_in_the_selected_root() {
        let root = tempfile::tempdir().unwrap();
        let options = CaptureOptions {
            sources: vec![CaptureSource::Microphone],
            meeting_type: MeetingType::Auto,
            single_person_mic: false,
            title: "Test capture".into(),
            output_root: root.path().join("media"),
            screen_target: None,
        };
        validate_storage(&options).unwrap();
        assert!(!root.path().join("media/.locus-write-test").exists());
    }
}
