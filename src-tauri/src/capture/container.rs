//! Final MP4 with separate audio streams (NATIVE_CAPTURE_PLAN NC-4, FR1.13).
//!
//! Layout (assumes the plan's recommendation for D2, "extra MP4 streams"):
//! - video stream first when there is video; audio-only sessions have **no**
//!   video stream at all (no dummy picture);
//! - when two sources were captured: stream 0 = `mixed` (the default, plays in
//!   any player), then `system` and `microphone`, both non-default;
//! - when only one source was captured the mix would be a copy of it, so the
//!   file carries that single stream, marked default, named by its role.
//!
//! Every stream is named by role (`handler_name`) so a consumer can pick one by name:
//! diarization (FR12.2) reads the `microphone` stream when it exists.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Lists the audio streams of `recording.mp4` next to it (the manifest data
/// for FR1.13; NC-5 folds it into the capture manifest).
pub const STREAMS_FILE_NAME: &str = "audio-streams.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRole {
    Mixed,
    System,
    Microphone,
}

impl AudioRole {
    /// Stream name written into the MP4. The MP4 muxer persists only the track
    /// `handler_name` (a `title` tag is silently dropped), so that is the field
    /// consumers read.
    pub fn title(self) -> &'static str {
        match self {
            Self::Mixed => "mixed",
            Self::System => "system",
            Self::Microphone => "microphone",
        }
    }
}

/// One audio input for the container: a mono WAV already aligned to the media
/// clock (lead silence applied), so every stream shares t = 0.
#[derive(Debug, Clone)]
pub struct AudioInput {
    pub role: AudioRole,
    pub wav: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub role: AudioRole,
    /// Index among the file's audio streams (`0:a:<index>`).
    pub index: usize,
    pub default: bool,
}

/// Which audio stream a transcription/diarization consumer should read:
/// the microphone when present (FR12.2), otherwise the default stream.
pub fn transcription_stream(streams: &[AudioStreamInfo]) -> Option<&AudioStreamInfo> {
    streams
        .iter()
        .find(|s| s.role == AudioRole::Microphone)
        .or_else(|| streams.iter().find(|s| s.default))
        .or_else(|| streams.first())
}

/// The streams a capture with these sources produces, in file order.
/// `system` / `microphone` are the aligned per-source WAVs; `mixed` is the
/// aligned mix of all of them.
pub fn plan_streams(
    mixed: &Path,
    system: Option<&Path>,
    microphone: Option<&Path>,
) -> Vec<AudioInput> {
    let mut sources = Vec::new();
    if let Some(wav) = system {
        sources.push(AudioInput {
            role: AudioRole::System,
            wav: wav.to_path_buf(),
        });
    }
    if let Some(wav) = microphone {
        sources.push(AudioInput {
            role: AudioRole::Microphone,
            wav: wav.to_path_buf(),
        });
    }
    if sources.len() <= 1 {
        return sources; // a lone source is its own mix
    }
    let mut streams = vec![AudioInput {
        role: AudioRole::Mixed,
        wav: mixed.to_path_buf(),
    }];
    streams.extend(sources);
    streams
}

/// Writes `output` from optional video segments plus the given audio inputs.
/// `audio_offset` is seconds the audio began after the first video frame
/// (negative: audio was already recording before frame 0), as in
/// `screen::mux_recording`; it is ignored for audio-only output.
pub fn mux_container(
    ffmpeg: &Path,
    segments: &[PathBuf],
    audio: &[AudioInput],
    audio_offset: f64,
    output: &Path,
) -> Result<Vec<AudioStreamInfo>, String> {
    if segments.is_empty() && audio.is_empty() {
        return Err("nothing to write: no video segments and no audio".into());
    }
    let dir = output.parent().unwrap_or(Path::new("."));
    let temp = output.with_extension("mp4.part");
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error"]);

    let mut input_index = 0usize;
    let mut list_path = None;
    let mut normalized: Vec<PathBuf> = Vec::new();
    if !segments.is_empty() {
        let list = dir.join("screen-segments.txt");
        let mut timed = Vec::with_capacity(segments.len());
        for (index, segment) in segments.iter().enumerate() {
            let candidate = dir.join(format!("screen-norm-{index:03}.mp4"));
            if super::screen::normalize_segment(ffmpeg, segment, &candidate) {
                normalized.push(candidate.clone());
                timed.push(candidate);
            } else {
                timed.push(segment.clone());
            }
        }
        super::screen::write_concat_list(&list, &timed).map_err(|e| e.to_string())?;
        command
            .args(["-f", "concat", "-safe", "0", "-i"])
            .arg(&list);
        list_path = Some(list);
        input_index += 1;
    }
    for input in audio {
        if audio_offset > 0.02 && !segments.is_empty() {
            command.args(["-itsoffset", &format!("{audio_offset:.3}")]);
        } else if audio_offset < -0.02 && !segments.is_empty() {
            command.args(["-ss", &format!("{:.3}", -audio_offset)]);
        }
        command.arg("-i").arg(&input.wav);
        input_index += 1;
    }
    debug_assert_eq!(input_index, usize::from(!segments.is_empty()) + audio.len());

    let video_inputs = usize::from(!segments.is_empty());
    if !segments.is_empty() {
        command.args(["-map", "0:v:0", "-c:v", "copy"]);
    }
    for (n, input) in audio.iter().enumerate() {
        command.args(["-map", &format!("{}:a:0", n + video_inputs)]);
        let bitrate = if input.role == AudioRole::Mixed {
            "160k"
        } else {
            "96k"
        };
        command.args([&format!("-b:a:{n}"), bitrate]);
        command.args([
            &format!("-metadata:s:a:{n}"),
            &format!("handler_name={}", input.role.title()),
        ]);
        let disposition = if n == 0 { "default" } else { "0" };
        command.args([&format!("-disposition:a:{n}"), disposition]);
    }
    if audio.is_empty() {
        command.arg("-an");
    } else {
        command.args(["-c:a", "aac"]);
    }
    if segments.is_empty() {
        command.arg("-vn");
    }
    let status = command
        .args(["-movflags", "+faststart", "-f", "mp4", "-y"])
        .arg(&temp)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run FFmpeg: {e}"))?;
    if let Some(list) = list_path {
        let _ = fs::remove_file(list);
    }
    for file in &normalized {
        let _ = fs::remove_file(file);
    }
    if !status.success() {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "FFmpeg mux exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    fs::rename(&temp, output).map_err(|e| e.to_string())?;
    Ok(audio
        .iter()
        .enumerate()
        .map(|(index, input)| AudioStreamInfo {
            role: input.role,
            index,
            default: index == 0,
        })
        .collect())
}

/// What a finished `recording.mp4` contains.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedRecording {
    pub duration_seconds: f64,
    pub has_video: bool,
    pub audio_streams: usize,
}

/// Checks the final file before the raw material it was built from is deleted:
/// every packet reads back cleanly, and the streams and a non-zero duration
/// are what the capture should have produced.
pub fn verify_recording(
    ffmpeg: &Path,
    output: &Path,
    expect_video: bool,
    expect_audio_streams: usize,
) -> Result<VerifiedRecording, String> {
    let read = Command::new(ffmpeg)
        .args(["-hide_banner", "-v", "error", "-i"])
        .arg(output)
        .args(["-map", "0", "-c", "copy", "-f", "null", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run FFmpeg: {e}"))?;
    if !read.status.success() || !read.stderr.is_empty() {
        return Err(format!(
            "the recording does not read back cleanly: {}",
            String::from_utf8_lossy(&read.stderr).trim()
        ));
    }
    let info = Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(output)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run FFmpeg: {e}"))?;
    let text = String::from_utf8_lossy(&info.stderr);
    let verified = parse_stream_info(&text);
    if verified.duration_seconds <= 0.0 {
        return Err("the recording has no duration".into());
    }
    if expect_video && !verified.has_video {
        return Err("the recording is missing its video stream".into());
    }
    if verified.audio_streams < expect_audio_streams {
        return Err(format!(
            "the recording has {} audio stream(s), expected {expect_audio_streams}",
            verified.audio_streams
        ));
    }
    Ok(verified)
}

fn parse_stream_info(text: &str) -> VerifiedRecording {
    let duration_seconds = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Duration: "))
        .and_then(|rest| rest.split(',').next())
        .map(|clock| {
            clock
                .trim()
                .split(':')
                .filter_map(|part| part.parse::<f64>().ok())
                .fold(0.0, |total, part| total * 60.0 + part)
        })
        .unwrap_or(0.0);
    VerifiedRecording {
        duration_seconds,
        has_video: text.contains(" Video: "),
        audio_streams: text.matches(" Audio: ").count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::screen;

    #[test]
    fn stream_info_is_parsed_from_ffmpeg_output() {
        let text = "  Duration: 00:01:02.50, start: 0.000000, bitrate: 100 kb/s\n  \
            Stream #0:0[0x1]: Video: h264 (High), yuv420p, 320x240\n  \
            Stream #0:1[0x2]: Audio: aac (LC), 48000 Hz, mono\n  \
            Stream #0:2[0x3]: Audio: aac (LC), 48000 Hz, mono\n";
        let info = parse_stream_info(text);
        assert!((info.duration_seconds - 62.5).abs() < 1e-9);
        assert!(info.has_video);
        assert_eq!(info.audio_streams, 2);
        assert_eq!(parse_stream_info("nothing here").duration_seconds, 0.0);
    }

    #[test]
    fn verification_accepts_a_good_file_and_rejects_a_broken_one() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let f = fixture();
        let out = f.dir.path().join("recording.mp4");
        let audio = plan_streams(&f.mixed, Some(&f.system), Some(&f.mic));
        mux_container(&ffmpeg, &[], &audio, 0.0, &out).unwrap();
        let ok = verify_recording(&ffmpeg, &out, false, 3).unwrap();
        assert!(!ok.has_video && ok.audio_streams == 3);
        assert!((ok.duration_seconds - 2.0).abs() < 0.2);
        assert!(
            verify_recording(&ffmpeg, &out, true, 3).is_err(),
            "no video stream"
        );
        assert!(
            verify_recording(&ffmpeg, &out, false, 4).is_err(),
            "too few audio streams"
        );
        let bytes = std::fs::read(&out).unwrap();
        let cut = f.dir.path().join("cut.mp4");
        std::fs::write(&cut, &bytes[..bytes.len() / 3]).unwrap();
        assert!(
            verify_recording(&ffmpeg, &cut, false, 1).is_err(),
            "truncated file"
        );
    }

    use hound::{SampleFormat, WavSpec, WavWriter};

    const RATE: u32 = 48_000;

    fn ffmpeg_for_test() -> Option<PathBuf> {
        let found = screen::locate_ffmpeg();
        if found.is_none() && std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
            panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
        }
        found
    }

    fn write_tone(path: &Path, hz: f32, seconds: f32) {
        let mut writer = WavWriter::create(
            path,
            WavSpec {
                channels: 1,
                sample_rate: RATE,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
        )
        .unwrap();
        for n in 0..(RATE as f32 * seconds) as usize {
            let v = (n as f32 / RATE as f32 * hz * std::f32::consts::TAU).sin() * 0.4;
            writer.write_sample((v * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    /// Power of `hz` in decoded 16-bit mono samples (Goertzel).
    fn power_at(samples: &[i16], hz: f32) -> f64 {
        let w = std::f64::consts::TAU * f64::from(hz) / f64::from(RATE);
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in samples {
            let s = f64::from(x) / 32768.0 + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2) / samples.len().max(1) as f64
    }

    /// Decodes the `n`th audio stream of `file` to mono 48 kHz samples.
    fn decode_stream(ffmpeg: &Path, file: &Path, n: usize) -> Vec<i16> {
        let out = Command::new(ffmpeg)
            .args(["-hide_banner", "-v", "error", "-i"])
            .arg(file)
            .args(["-map", &format!("0:a:{n}"), "-ac", "1", "-ar", "48000"])
            .args(["-f", "s16le", "-"])
            .output()
            .unwrap();
        assert!(out.status.success(), "decode of stream {n} failed");
        out.stdout
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect()
    }

    /// (codec_type, handler_name, default) per stream, from ffprobe.
    fn probe(file: &Path) -> Vec<(String, String, bool)> {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries"])
            .arg("stream=codec_type:stream_tags=handler_name:stream_disposition=default")
            .args(["-of", "json"])
            .arg(file)
            .output()
            .expect("ffprobe");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        json["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["codec_type"].as_str().unwrap_or("").to_string(),
                    s["tags"]["handler_name"].as_str().unwrap_or("").to_string(),
                    s["disposition"]["default"].as_i64() == Some(1),
                )
            })
            .collect()
    }

    fn make_video_segment(ffmpeg: &Path, path: &Path) {
        let ok = Command::new(ffmpeg)
            .args(["-hide_banner", "-v", "error", "-f", "lavfi", "-i"])
            .arg("testsrc=size=320x240:rate=30:duration=2")
            .args(["-vf", "format=yuv420p", "-c:v", "libx264", "-y"])
            .arg(path)
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }

    struct Fixture {
        dir: tempfile::TempDir,
        mic: PathBuf,
        system: PathBuf,
        mixed: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let (mic, system, mixed) = (
            dir.path().join("mic.wav"),
            dir.path().join("sys.wav"),
            dir.path().join("mix.wav"),
        );
        write_tone(&mic, 440.0, 2.0);
        write_tone(&system, 1500.0, 2.0);
        // The mix is just both tones; production uses audio::mix_tracks.
        crate::capture::audio::mix_tracks(&[mic.clone(), system.clone()], &mixed).unwrap();
        Fixture {
            dir,
            mic,
            system,
            mixed,
        }
    }

    #[test]
    fn stream_plan_follows_the_sources_captured() {
        let p = Path::new("x.wav");
        let both = plan_streams(p, Some(p), Some(p));
        let roles: Vec<_> = both.iter().map(|s| s.role).collect();
        assert_eq!(
            roles,
            vec![AudioRole::Mixed, AudioRole::System, AudioRole::Microphone]
        );
        let mic_only = plan_streams(p, None, Some(p));
        assert_eq!(mic_only.len(), 1);
        assert_eq!(mic_only[0].role, AudioRole::Microphone);
        assert_eq!(plan_streams(p, Some(p), None)[0].role, AudioRole::System);
        assert!(plan_streams(p, None, None).is_empty());
    }

    #[test]
    fn transcription_reads_the_microphone_stream_when_present() {
        let both = [
            AudioStreamInfo {
                role: AudioRole::Mixed,
                index: 0,
                default: true,
            },
            AudioStreamInfo {
                role: AudioRole::System,
                index: 1,
                default: false,
            },
            AudioStreamInfo {
                role: AudioRole::Microphone,
                index: 2,
                default: false,
            },
        ];
        assert_eq!(transcription_stream(&both).unwrap().index, 2);
        let system_only = [AudioStreamInfo {
            role: AudioRole::System,
            index: 0,
            default: true,
        }];
        assert_eq!(transcription_stream(&system_only).unwrap().index, 0);
        assert!(transcription_stream(&[]).is_none());
    }

    #[test]
    fn video_plus_two_sources_has_mixed_default_then_system_then_microphone() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let f = fixture();
        let seg = f.dir.path().join("screen-000.mp4");
        make_video_segment(&ffmpeg, &seg);
        let out = f.dir.path().join("recording.mp4");
        let audio = plan_streams(&f.mixed, Some(&f.system), Some(&f.mic));
        let info = mux_container(&ffmpeg, &[seg], &audio, 0.0, &out).unwrap();
        assert_eq!(info.len(), 3);
        let streams = probe(&out);
        assert_eq!(streams[0].0, "video");
        assert_eq!(
            streams[1..].to_vec(),
            vec![
                ("audio".to_string(), "mixed".to_string(), true),
                ("audio".to_string(), "system".to_string(), false),
                ("audio".to_string(), "microphone".to_string(), false),
            ]
        );
        // Each stream decodes independently and carries its own signal.
        let mixed = decode_stream(&ffmpeg, &out, 0);
        let system = decode_stream(&ffmpeg, &out, 1);
        let mic = decode_stream(&ffmpeg, &out, 2);
        let (m440, m1500) = (power_at(&mic, 440.0), power_at(&mic, 1500.0));
        assert!(m440 > 100.0 * m1500, "mic stream: 440={m440} 1500={m1500}");
        let (s440, s1500) = (power_at(&system, 440.0), power_at(&system, 1500.0));
        assert!(
            s1500 > 100.0 * s440,
            "system stream: 440={s440} 1500={s1500}"
        );
        assert!(power_at(&mixed, 440.0) > 0.005 && power_at(&mixed, 1500.0) > 0.005);
        assert_eq!(
            transcription_stream(&info).unwrap().role,
            AudioRole::Microphone
        );
    }

    #[test]
    fn audio_only_session_has_no_video_stream() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let f = fixture();
        let out = f.dir.path().join("recording.mp4");
        let audio = plan_streams(&f.mixed, Some(&f.system), Some(&f.mic));
        mux_container(&ffmpeg, &[], &audio, 0.0, &out).unwrap();
        let streams = probe(&out);
        assert!(
            streams.iter().all(|(kind, _, _)| kind == "audio"),
            "{streams:?}"
        );
        assert_eq!(streams.len(), 3);
        assert_eq!(streams[0], ("audio".into(), "mixed".into(), true));
    }

    #[test]
    fn mic_only_and_system_only_each_decode_independently() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let f = fixture();
        for (role_wav, hz_on, hz_off, title, name) in [
            (&f.mic, 440.0, 1500.0, "microphone", "mic.mp4"),
            (&f.system, 1500.0, 440.0, "system", "sys.mp4"),
        ] {
            let out = f.dir.path().join(name);
            let audio = if title == "microphone" {
                plan_streams(&f.mixed, None, Some(role_wav))
            } else {
                plan_streams(&f.mixed, Some(role_wav), None)
            };
            let info = mux_container(&ffmpeg, &[], &audio, 0.0, &out).unwrap();
            assert_eq!(info.len(), 1);
            assert_eq!(probe(&out), vec![("audio".into(), title.into(), true)]);
            let samples = decode_stream(&ffmpeg, &out, 0);
            assert!(power_at(&samples, hz_on) > 100.0 * power_at(&samples, hz_off));
        }
    }

    #[test]
    fn nothing_to_write_is_an_error() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        assert!(mux_container(&ffmpeg, &[], &[], 0.0, Path::new("/tmp/x.mp4")).is_err());
    }
}
