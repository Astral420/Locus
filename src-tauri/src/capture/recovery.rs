//! Crash recovery (NATIVE_CAPTURE_PLAN NC-5, FR12.3, M2.05).
//!
//! While recording, everything needed to rebuild the take is already on disk:
//! the manifest (how the capture was set up), the per-source WAVs (header
//! refreshed every second) and the fragmented-MP4 video segments (a fragment
//! per second). After a crash, power loss or forced quit this module finds
//! those captures, offers them to the user, and on request rebuilds the final
//! `recording.mp4` from whatever survived. It never restarts a capture by
//! itself.

use super::{
    audio::{self, MIXED_FILE_NAME},
    container::{self, AudioInput},
    encoder::Encoder,
    screen::VIDEO_FILE_NAME,
};
use crate::{contracts::CaptureSource, db::Database};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Size of the canonical 44-byte header `hound` writes for a PCM WAV.
const WAV_HEADER_LEN: u64 = 44;

/// Rewrites a possibly unfinished WAV's header from the file's real length, so
/// everything that reached the disk can be read. Returns the audio duration.
pub fn repair_wav(path: &Path) -> Result<f64, String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length < WAV_HEADER_LEN {
        return Err("audio file has no header".into());
    }
    let mut header = [0u8; WAV_HEADER_LEN as usize];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" || &header[36..40] != b"data" {
        return Err("audio file is not a recognised WAV".into());
    }
    let channels = u64::from(u16::from_le_bytes([header[22], header[23]]));
    let rate = u64::from(u32::from_le_bytes([
        header[24], header[25], header[26], header[27],
    ]));
    let bits = u64::from(u16::from_le_bytes([header[34], header[35]]));
    let block = (channels * bits / 8).max(1);
    if rate == 0 || channels == 0 {
        return Err("audio file header is damaged".into());
    }
    let data_len = (length - WAV_HEADER_LEN) / block * block; // whole frames only
    let riff_len = (WAV_HEADER_LEN - 8 + data_len) as u32;
    file.seek(SeekFrom::Start(4)).map_err(|e| e.to_string())?;
    file.write_all(&riff_len.to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(40)).map_err(|e| e.to_string())?;
    file.write_all(&(data_len as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(data_len as f64 / block as f64 / rate as f64)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableCapture {
    pub meeting_id: String,
    pub capture_id: String,
    pub sources: Vec<String>,
    pub has_video: bool,
    pub video_segments: usize,
    pub audio_files: Vec<String>,
    /// Best estimate of how much can be recovered (the longest audio track,
    /// or 0 when only video survived).
    pub estimated_seconds: f64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryOutcome {
    pub meeting_id: String,
    /// Relative to the media root.
    pub relative_path: String,
    pub duration_seconds: f64,
    pub has_video: bool,
    pub audio_streams: usize,
}

fn source_from_name(name: &str) -> Option<CaptureSource> {
    serde_json::from_str(&format!("\"{name}\"")).ok()
}

fn video_segments_on_disk(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("screen-") && n.ends_with(".mp4") && !n.contains("norm")
            })
        })
        .collect();
    found.sort();
    found
}

/// Audio duration from a WAV's size on disk, without trusting its header.
fn wav_seconds_on_disk(path: &Path) -> f64 {
    let Ok(mut file) = fs::File::open(path) else {
        return 0.0;
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut header = [0u8; WAV_HEADER_LEN as usize];
    if length <= WAV_HEADER_LEN || file.read_exact(&mut header).is_err() {
        return 0.0;
    }
    let rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]).max(1);
    let block = u64::from(u16::from_le_bytes([header[32], header[33]])).max(1);
    (length - WAV_HEADER_LEN) as f64 / block as f64 / f64::from(rate)
}

/// Captures that were interrupted and have something on disk to recover.
/// `active_meeting` (the capture running right now, if any) is excluded.
/// Orphans still marked "recording" are flipped to "interrupted" in the
/// manifest and the database so the UI stops showing them as live.
pub fn list_recoverable(
    media_root: &Path,
    active_meeting: Option<&str>,
    db: Option<&Database>,
) -> Vec<RecoverableCapture> {
    let manifests = Encoder::recoverable_manifests(media_root).unwrap_or_default();
    let mut found = Vec::new();
    for manifest in manifests {
        if Some(manifest.meeting_id.as_str()) == active_meeting {
            continue;
        }
        let dir = media_root.join(&manifest.meeting_id);
        let audio_files: Vec<String> = manifest
            .sources
            .iter()
            .filter_map(|name| source_from_name(name))
            .map(|source| audio::file_name(&source).to_string())
            .filter(|file| wav_seconds_on_disk(&dir.join(file)) > 0.0)
            .collect();
        let video_segments = video_segments_on_disk(&dir).len();
        if audio_files.is_empty() && video_segments == 0 {
            continue; // nothing survived; nothing to offer
        }
        let estimated_seconds = audio_files
            .iter()
            .map(|file| wav_seconds_on_disk(&dir.join(file)))
            .fold(0.0, f64::max);
        if manifest.state == "recording" {
            if let Ok(mut encoder) = Encoder::open(&dir) {
                let _ = encoder.mark_state("interrupted");
            }
            if let Some(db) = db {
                let meeting_id = manifest.meeting_id.clone();
                let _ = db.run(move |connection| {
                    connection
                        .execute(
                            "UPDATE meetings SET lifecycle='interrupted' WHERE id=?1 AND lifecycle IN ('recording','paused')",
                            [&meeting_id],
                        )
                        .map_err(|e| e.to_string())?;
                    connection
                        .execute(
                            "UPDATE capture_manifests SET state='interrupted' WHERE meeting_id=?1 AND state='recording'",
                            [&meeting_id],
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                });
            }
        }
        found.push(RecoverableCapture {
            meeting_id: manifest.meeting_id,
            capture_id: manifest.capture_id,
            has_video: manifest.has_video,
            sources: manifest.sources,
            video_segments,
            audio_files,
            estimated_seconds,
            reason: manifest.reason,
        });
    }
    found.sort_by(|a, b| a.meeting_id.cmp(&b.meeting_id));
    found
}

/// At least one picture decodes: a segment cut off mid-write is still usable
/// up to its last complete fragment, but a zero-frame file is not.
fn segment_is_usable(ffmpeg: &Path, segment: &Path) -> bool {
    Command::new(ffmpeg)
        .args(["-hide_banner", "-v", "error", "-i"])
        .arg(segment)
        .args(["-map", "0:v:0", "-frames:v", "1", "-f", "null", "-"])
        .stdin(Stdio::null())
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn keep_raw_files() -> bool {
    std::env::var("LOCUS_KEEP_SEGMENTS").ok().as_deref() == Some("1")
}

/// Rebuilds `recording.mp4` for an interrupted capture. Raw material stays on
/// disk until the new file has been read back and verified; on any failure it
/// is all still there to retry.
pub fn recover(
    media_root: &Path,
    meeting_id: &str,
    ffmpeg: &Path,
) -> Result<RecoveryOutcome, String> {
    let dir = media_root.join(meeting_id);
    let mut manifest =
        Encoder::open(&dir).map_err(|e| format!("cannot read the capture manifest: {e}"))?;
    let info = manifest.manifest().clone();
    if info.state == "saved" {
        return Err("this capture was already saved".into());
    }
    // A crash can land after the final file was built and verified but before
    // the manifest said so; the raw video is gone then, and rebuilding from the
    // audio alone would overwrite a complete recording with a worse one.
    let existing = dir.join(VIDEO_FILE_NAME);
    if existing.exists() {
        if let Ok(done) = container::verify_recording(ffmpeg, &existing, info.has_video, 1) {
            let _ = manifest.mark_state("saved");
            return Ok(RecoveryOutcome {
                meeting_id: meeting_id.to_string(),
                relative_path: format!("{meeting_id}/{VIDEO_FILE_NAME}"),
                duration_seconds: done.duration_seconds,
                has_video: done.has_video,
                audio_streams: done.audio_streams,
            });
        }
    }

    // Audio: repair headers, line each track up, mix.
    let mut aligned: Vec<(CaptureSource, PathBuf)> = Vec::new();
    let mut mix_inputs: Vec<(PathBuf, f64)> = Vec::new();
    for name in &info.sources {
        let Some(source) = source_from_name(name) else {
            continue;
        };
        if !matches!(
            source,
            CaptureSource::SystemAudio | CaptureSource::Microphone
        ) {
            continue;
        }
        let wav = dir.join(audio::file_name(&source));
        if !wav.exists() || repair_wav(&wav).unwrap_or(0.0) <= 0.0 {
            continue;
        }
        let lead = info
            .track_leads
            .iter()
            .find(|l| &l.source == name)
            .map_or(0.0, |l| l.lead_seconds);
        let aligned_path = dir.join(audio::file_name(&source).replace(".wav", ".aligned.wav"));
        if audio::mix_tracks_with_leads(&[(wav.clone(), lead)], &aligned_path).is_ok() {
            aligned.push((source, aligned_path));
        }
        mix_inputs.push((wav, lead));
    }
    let mixed_path = dir.join(MIXED_FILE_NAME);
    let has_audio = !mix_inputs.is_empty();
    if has_audio {
        audio::mix_tracks_with_leads(&mix_inputs, &mixed_path)
            .map_err(|e| format!("cannot mix the recovered audio: {e}"))?;
    }

    // Video: keep the segments that still decode.
    let segments: Vec<PathBuf> = video_segments_on_disk(&dir)
        .into_iter()
        .filter(|segment| segment_is_usable(ffmpeg, segment))
        .collect();
    if !has_audio && segments.is_empty() {
        return Err("nothing could be recovered from this capture".into());
    }

    let wav_for = |source: &CaptureSource| {
        aligned
            .iter()
            .find(|(s, _)| s == source)
            .map(|(_, path)| path.as_path())
    };
    let inputs: Vec<AudioInput> = if has_audio {
        container::plan_streams(
            &mixed_path,
            wav_for(&CaptureSource::SystemAudio),
            wav_for(&CaptureSource::Microphone),
        )
    } else {
        Vec::new()
    };
    let output = dir.join(VIDEO_FILE_NAME);
    let streams = container::mux_container(
        ffmpeg,
        &segments,
        &inputs,
        if segments.is_empty() {
            0.0
        } else {
            info.audio_offset_seconds
        },
        &output,
    )?;
    let verified =
        container::verify_recording(ffmpeg, &output, !segments.is_empty(), inputs.len())?;

    if !keep_raw_files() {
        for segment in video_segments_on_disk(&dir) {
            let _ = fs::remove_file(segment);
        }
    }
    for (_, path) in &aligned {
        let _ = fs::remove_file(path);
    }
    if let Ok(json) = serde_json::to_string_pretty(&streams) {
        let _ = fs::write(dir.join(container::STREAMS_FILE_NAME), json);
    }
    let _ = manifest.set_reason(Some(
        "Recovered after the app stopped unexpectedly; the last few seconds may be missing.".into(),
    ));
    manifest
        .mark_state("saved")
        .map_err(|e| format!("cannot update the capture manifest: {e}"))?;
    Ok(RecoveryOutcome {
        meeting_id: meeting_id.to_string(),
        relative_path: format!("{meeting_id}/{VIDEO_FILE_NAME}"),
        duration_seconds: verified.duration_seconds,
        has_video: verified.has_video,
        audio_streams: verified.audio_streams,
    })
}

/// `recover`, then record the result the way a normal stop does: the meeting
/// becomes saved, with its recording registered.
pub fn recover_and_commit(
    db: &Database,
    media_root: &Path,
    meeting_id: &str,
    ffmpeg: &Path,
) -> Result<RecoveryOutcome, String> {
    let outcome = recover(media_root, meeting_id, ffmpeg)?;
    let id = meeting_id.to_string();
    let (path, duration) = (outcome.relative_path.clone(), outcome.duration_seconds);
    db.run(move |connection| {
        let now = Utc::now().to_rfc3339();
        let manifest_id = format!("manifest:{id}");
        // The raw video segments are gone; the final file replaces them.
        connection
            .execute(
                "DELETE FROM media_segments WHERE manifest_id=?1 AND source='screen'",
                [&manifest_id],
            )
            .map_err(|e| e.to_string())?;
        connection
            .execute(
                "INSERT OR REPLACE INTO media_segments(id, manifest_id, stream_id, source, ordinal, relative_path, start_seconds, duration_seconds, committed_at) VALUES (?1, ?2, 'mixed', 'mixed', 0, ?3, 0, ?4, ?5)",
                (format!("segment:{id}:0"), &manifest_id, &path, duration, &now),
            )
            .map_err(|e| e.to_string())?;
        connection
            .execute(
                "UPDATE capture_manifests SET state='saved', recovered_at=?2 WHERE id=?1",
                (&manifest_id, &now),
            )
            .map_err(|e| e.to_string())?;
        connection
            .execute(
                "UPDATE meetings SET lifecycle='saved', duration_seconds=?2 WHERE id=?1",
                (&id, duration),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .map_err(|e| e.to_string())?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{
        clock::MediaClock,
        encoder::TrackLead,
        screen,
        source::{synthetic::SyntheticSource, ScreenTarget},
        video_encoder::EncoderConfig,
        video_pipeline::VideoPipeline,
    };
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    fn ffmpeg_for_test() -> Option<PathBuf> {
        let found = screen::locate_ffmpeg();
        if found.is_none() && std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
            panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
        }
        found
    }

    fn tone_chunk(hz: f32, from: usize, len: usize) -> Vec<f32> {
        (from..from + len)
            .map(|n| (n as f32 / 48_000.0 * hz * std::f32::consts::TAU).sin() * 0.4)
            .collect()
    }

    fn manifest_for(dir: &Path, sources: &[&str], has_video: bool) -> Encoder {
        let mut encoder = Encoder::create(dir, "capture-1", "meeting-1").unwrap();
        encoder
            .set_session(
                sources.iter().map(|s| s.to_string()).collect(),
                has_video,
                0.0,
                sources
                    .iter()
                    .map(|s| TrackLead {
                        source: s.to_string(),
                        lead_seconds: 0.0,
                    })
                    .collect(),
            )
            .unwrap();
        encoder
    }

    #[test]
    fn unfinished_wav_is_repaired_from_its_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("microphone.wav");
        let mut writer = WavWriter::create(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
        )
        .unwrap();
        for n in 0..96_000 {
            writer.write_sample((n % 1000) as i16).unwrap();
        }
        writer.flush().unwrap(); // 2 s on disk
        for n in 0..4_000 {
            writer.write_sample((n % 1000) as i16).unwrap();
        }
        std::mem::forget(writer); // "crash": no finalize, buffered tail lost
        let seconds = repair_wav(&path).unwrap();
        assert!((2.0..2.1).contains(&seconds), "got {seconds}");
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.duration() as f64 / 48_000.0, seconds);
    }

    #[test]
    fn a_header_with_nothing_written_still_repairs_to_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("microphone.wav");
        let writer = WavWriter::create(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
        )
        .unwrap();
        std::mem::forget(writer);
        // Header sat in the buffer: the file is empty and cannot be repaired.
        assert!(repair_wav(&path).is_err());
        fs::write(&path, b"RIFF....WAVEjunk").unwrap();
        assert!(repair_wav(&path).is_err());
    }

    #[test]
    fn real_sink_keeps_the_file_readable_while_recording() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("microphone.wav");
        let (push, health) = audio::open_test_sink(&path).unwrap();
        let started = Instant::now();
        let mut sent = 0usize;
        // 3.3 s of audio delivered in real time, then the process "dies".
        while started.elapsed() < Duration::from_millis(3300) {
            push(&tone_chunk(440.0, sent, 480));
            sent += 480;
            thread::sleep(Duration::from_millis(10));
        }
        assert!(health.failure().is_none());
        let seconds = repair_wav(&path).unwrap();
        let recorded = sent as f64 / 48_000.0;
        assert!(
            seconds >= recorded - 1.5,
            "{seconds}s survived of {recorded}s"
        );
    }

    #[test]
    fn a_full_disk_is_reported_not_swallowed() {
        // /dev/full accepts the open and fails every write with ENOSPC.
        if !Path::new("/dev/full").exists() {
            return;
        }
        let (push, health) = audio::open_test_sink(Path::new("/dev/full")).unwrap();
        for n in 0..40 {
            push(&tone_chunk(440.0, n * 480, 480));
        }
        let message = health
            .failure()
            .expect("the write failure must be recorded");
        assert!(message.contains("disk"), "{message}");
    }

    #[test]
    fn interrupted_captures_are_listed_and_the_active_one_is_not() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting-1");
        let _ = manifest_for(&dir, &["microphone"], false);
        // Nothing on disk yet: nothing to offer.
        assert!(list_recoverable(root.path(), None, None).is_empty());
        let (push, _health) = audio::open_test_sink(&dir.join("microphone.wav")).unwrap();
        for n in 0..300 {
            push(&tone_chunk(440.0, n * 480, 480));
            if n % 100 == 99 {
                thread::sleep(Duration::from_millis(1100)); // let a checkpoint land
            }
        }
        let listed = list_recoverable(root.path(), None, None);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].meeting_id, "meeting-1");
        assert!(listed[0].estimated_seconds > 0.5);
        assert_eq!(listed[0].audio_files, vec!["microphone.wav"]);
        // The orphan was flipped from "recording" to "interrupted".
        assert_eq!(Encoder::open(&dir).unwrap().manifest().state, "interrupted");
        // The capture running right now is never offered.
        assert!(list_recoverable(root.path(), Some("meeting-1"), None).is_empty());
    }

    #[test]
    fn a_finished_recording_is_never_rebuilt_over() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting-1");
        let _ = manifest_for(&dir, &["microphone"], false);
        let (push, _) = audio::open_test_sink(&dir.join("microphone.wav")).unwrap();
        for n in 0..200 {
            push(&tone_chunk(440.0, n * 480, 480));
        }
        thread::sleep(Duration::from_millis(1100));
        push(&tone_chunk(440.0, 96_000, 480));
        let first = recover(root.path(), "meeting-1", &ffmpeg).unwrap();
        // Simulate the crash window: file complete, manifest not yet "saved".
        Encoder::open(&dir)
            .unwrap()
            .mark_state("recording")
            .unwrap();
        let before = fs::metadata(dir.join(VIDEO_FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        let again = recover(root.path(), "meeting-1", &ffmpeg).unwrap();
        assert_eq!(again.relative_path, first.relative_path);
        let after = fs::metadata(dir.join(VIDEO_FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after, "the finished file must not be rewritten");
        assert_eq!(Encoder::open(&dir).unwrap().manifest().state, "saved");
    }

    #[test]
    fn audio_only_capture_recovers_to_an_audio_only_mp4() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting-1");
        let _ = manifest_for(&dir, &["microphone", "system_audio"], false);
        for (file, hz) in [("microphone.wav", 440.0), ("system_audio.wav", 1500.0)] {
            let (push, _) = audio::open_test_sink(&dir.join(file)).unwrap();
            for n in 0..200 {
                push(&tone_chunk(hz, n * 480, 480)); // 2 s
            }
            thread::sleep(Duration::from_millis(1100));
            push(&tone_chunk(hz, 96_000, 480)); // a checkpoint flushes the 2 s
        }
        let outcome = recover(root.path(), "meeting-1", &ffmpeg).unwrap();
        assert!(!outcome.has_video);
        assert_eq!(outcome.audio_streams, 3, "mixed + system + microphone");
        assert!(
            outcome.duration_seconds > 1.5,
            "{}",
            outcome.duration_seconds
        );
        assert!(root.path().join(&outcome.relative_path).exists());
        assert_eq!(Encoder::open(&dir).unwrap().manifest().state, "saved");
        // Raw per-source WAVs stay (the pipeline reads them); derived copies go.
        assert!(dir.join("microphone.wav").exists());
        assert!(!dir.join("microphone.aligned.wav").exists());
        assert!(
            recover(root.path(), "meeting-1", &ffmpeg).is_err(),
            "already saved"
        );
    }

    #[test]
    fn a_torn_final_video_segment_is_recovered_up_to_the_last_fragment() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting-1");
        let _ = manifest_for(&dir, &["microphone"], true);
        let clock = Arc::new(MediaClock::new());
        let pipeline = VideoPipeline::start(
            Box::new(SyntheticSource::new(320, 240, 30)),
            &ScreenTarget::PrimaryDisplay,
            clock,
            EncoderConfig::new(ffmpeg.clone()),
            dir.clone(),
        )
        .unwrap();
        let (push, _) = audio::open_test_sink(&dir.join("microphone.wav")).unwrap();
        let started = Instant::now();
        let mut sent = 0;
        while started.elapsed() < Duration::from_secs(4) {
            push(&tone_chunk(440.0, sent, 480));
            sent += 480;
            thread::sleep(Duration::from_millis(10));
        }
        let report = pipeline.stop().unwrap();
        // Tear the segment: a crash can cut it anywhere.
        let segment = &report.segments[0].path;
        let bytes = fs::read(segment).unwrap();
        fs::write(segment, &bytes[..bytes.len() * 7 / 10]).unwrap();
        let outcome = recover(root.path(), "meeting-1", &ffmpeg).unwrap();
        assert!(outcome.has_video);
        assert!(
            outcome.duration_seconds > 2.0,
            "{}",
            outcome.duration_seconds
        );
        assert!(
            video_segments_on_disk(&dir).is_empty(),
            "raw segments removed after verification"
        );
    }

    /// Body of the process that gets killed. Does nothing unless the parent
    /// test started it with the directory in the environment.
    #[test]
    fn crash_child() {
        let Ok(dir) = std::env::var("LOCUS_CRASH_CHILD_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let ffmpeg = ffmpeg_for_test().expect("ffmpeg for the crash child");
        let _manifest = manifest_for(&dir, &["microphone", "system_audio"], true);
        let clock = Arc::new(MediaClock::new());
        let _pipeline = VideoPipeline::start(
            Box::new(SyntheticSource::new(320, 240, 30)),
            &ScreenTarget::PrimaryDisplay,
            clock,
            EncoderConfig::new(ffmpeg),
            dir.clone(),
        )
        .unwrap();
        let (mic, _) = audio::open_test_sink(&dir.join("microphone.wav")).unwrap();
        let (sys, _) = audio::open_test_sink(&dir.join("system_audio.wav")).unwrap();
        let started = Instant::now();
        let mut sent = 0;
        let mut ready = false;
        loop {
            mic(&tone_chunk(440.0, sent, 480));
            sys(&tone_chunk(1500.0, sent, 480));
            sent += 480;
            if !ready && started.elapsed() >= Duration::from_secs(7) {
                fs::write(
                    dir.join("ready"),
                    format!("{}", started.elapsed().as_secs_f64()),
                )
                .unwrap();
                ready = true;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn killing_the_process_mid_recording_loses_at_most_five_seconds() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("meeting-1");
        fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture::recovery::tests::crash_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("LOCUS_CRASH_CHILD_DIR", &dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let ready = dir.join("ready");
        let deadline = Instant::now() + Duration::from_secs(90);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "the child never reached 7 s of recording"
            );
            thread::sleep(Duration::from_millis(50));
        }
        let at_marker: f64 = fs::read_to_string(&ready).unwrap().trim().parse().unwrap();
        thread::sleep(Duration::from_millis(300));
        child.kill().unwrap(); // SIGKILL: no destructors, no finalize
        child.wait().unwrap();
        let recorded = at_marker + 0.3;

        let listed = list_recoverable(root.path(), None, None);
        assert_eq!(listed.len(), 1, "the interrupted capture must be offered");
        assert!(listed[0].has_video && listed[0].video_segments >= 1);
        let outcome = recover(root.path(), "meeting-1", &ffmpeg).unwrap();
        assert!(outcome.has_video);
        assert_eq!(outcome.audio_streams, 3);
        let lost = recorded - outcome.duration_seconds;
        assert!(
            lost <= 5.0,
            "recorded {recorded:.1} s but recovered {:.1} s",
            outcome.duration_seconds
        );
        assert!(outcome.duration_seconds <= recorded + 1.0);
        eprintln!(
            "NC-5 kill test: recorded {recorded:.2} s, recovered {:.2} s, lost {lost:.2} s",
            outcome.duration_seconds
        );
    }
}
