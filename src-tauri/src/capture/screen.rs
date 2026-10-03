//! Screen capture: drives an FFmpeg child process that records the desktop to
//! fragmented H.264 MP4 segments (decodable even if the app dies mid-record,
//! per PRD FR12.3), then muxes them with the mixed audio into one H.264/AAC
//! `recording.mp4` (PRD FR1.11 / NFR12).
//!
//! Pause/resume is implemented by closing the current segment and opening a
//! new one, so paused time never reaches the file (PRD FR12.5).

use super::CaptureError;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub const VIDEO_FILE_NAME: &str = "recording.mp4";
const STARTUP_PROBE: Duration = Duration::from_millis(1500);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(format!("Screen: {}", message.into()))
}

const FFMPEG_NAME: &str = if cfg!(windows) {
    "ffmpeg.exe"
} else {
    "ffmpeg"
};

/// Where the installer places the bundled FFmpeg (`src-tauri/ffmpeg/` is
/// listed in `bundle.resources`), relative to the running executable:
/// - macOS: `Locus.app/Contents/MacOS/Locus` -> `../Resources/ffmpeg/`
/// - Windows (NSIS): next to `Locus.exe` -> `ffmpeg/`
/// - Linux (deb/AppImage): `usr/bin/locus` -> `../lib/Locus/ffmpeg/`
pub fn bundled_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    ["../Resources/ffmpeg", "ffmpeg", "../lib/Locus/ffmpeg"]
        .iter()
        .map(|relative| exe_dir.join(relative).join(FFMPEG_NAME))
        .collect()
}

/// Finds the FFmpeg binary. `LOCUS_FFMPEG` (an explicit developer/QA override)
/// wins, then the bundled copy. Release builds never fall back to a system
/// FFmpeg, so a packaging mistake is caught in QA instead of working only on
/// machines that happen to have Homebrew FFmpeg. Debug builds also probe
/// common install locations and PATH for convenience.
pub fn locate_ffmpeg() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("LOCUS_FFMPEG") {
        let path = PathBuf::from(custom);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut candidates: Vec<PathBuf> = vec![];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        candidates.extend(bundled_candidates(&dir));
    }
    // Running from a source checkout (`tauri dev`): the resource folder.
    candidates.push(PathBuf::from("ffmpeg").join(FFMPEG_NAME));
    candidates.push(PathBuf::from("src-tauri/ffmpeg").join(FFMPEG_NAME));
    #[cfg(debug_assertions)]
    {
        for fixed in [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ] {
            candidates.push(PathBuf::from(fixed));
        }
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                candidates.push(dir.join(FFMPEG_NAME));
            }
        }
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Parses `ffmpeg -f avfoundation -list_devices true -i ""` output and returns
/// the index of the first "Capture screen" video device.
pub fn parse_avfoundation_screen_index(listing: &str) -> Option<u32> {
    for line in listing.lines() {
        if !line.contains("Capture screen") {
            continue;
        }
        // "[AVFoundation indev @ 0x...] [1] Capture screen 0"
        let mut rest = line;
        while let Some(open) = rest.find('[') {
            let after = &rest[open + 1..];
            let Some(close) = after.find(']') else { break };
            if let Ok(index) = after[..close].trim().parse::<u32>() {
                return Some(index);
            }
            rest = &after[close + 1..];
        }
    }
    None
}

/// Platform-specific FFmpeg input arguments for grabbing the primary display.
fn input_args(ffmpeg: &Path) -> Result<Vec<String>, CaptureError> {
    // Deterministic synthetic display for automated lifecycle tests (same
    // pattern as LOCUS_PIPEWIRE_TEST / LOCUS_WINDOWS_CAPTURE_TEST).
    if std::env::var("LOCUS_SCREEN_TEST_INPUT").ok().as_deref() == Some("lavfi") {
        return Ok(["-re", "-f", "lavfi", "-i", "testsrc=size=321x241:rate=30"]
            .iter()
            .map(|s| s.to_string())
            .collect());
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-f",
                "avfoundation",
                "-list_devices",
                "true",
                "-i",
                "",
            ])
            .output()
            .map_err(|e| unavailable(format!("cannot run FFmpeg: {e}")))?;
        let listing = String::from_utf8_lossy(&output.stderr);
        let index = parse_avfoundation_screen_index(&listing).ok_or_else(|| {
            unavailable(
                "no screen device was found. Grant Screen Recording permission to Locus in \
                 System Settings → Privacy & Security, then restart the app.",
            )
        })?;
        Ok(vec![
            "-f".into(),
            "avfoundation".into(),
            "-framerate".into(),
            "30".into(),
            "-capture_cursor".into(),
            "1".into(),
            "-pixel_format".into(),
            "uyvy422".into(),
            "-i".into(),
            format!("{index}:none"),
        ])
    }
    #[cfg(target_os = "windows")]
    {
        let _ = ffmpeg;
        Ok(vec![
            "-f".into(),
            "gdigrab".into(),
            "-framerate".into(),
            "30".into(),
            "-draw_mouse".into(),
            "1".into(),
            "-i".into(),
            "desktop".into(),
        ])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = ffmpeg;
        Err(unavailable(
            "screen recording is not available on this platform yet (the Linux PipeWire \
             portal adapter is not implemented). Turn off Screen Video to record audio only.",
        ))
    }
}

fn segment_path(dir: &Path, ordinal: usize) -> PathBuf {
    dir.join(format!("screen-{ordinal:03}.mp4"))
}

struct Running {
    child: Child,
    path: PathBuf,
}

pub struct ScreenCapture {
    dir: PathBuf,
    ffmpeg: PathBuf,
    input: Vec<String>,
    segments: Vec<PathBuf>,
    running: Option<Running>,
    /// How long after `epoch` the first frame segment started; used to align
    /// video with audio, which starts first.
    first_segment_delay: Duration,
    epoch: Instant,
}

#[derive(Debug)]
pub struct FinishedScreen {
    pub segments: Vec<PathBuf>,
    pub video_delay: Duration,
}

impl ScreenCapture {
    /// `epoch` is the moment audio capture began.
    pub fn start(dir: &Path, epoch: Instant) -> Result<Self, CaptureError> {
        let ffmpeg = locate_ffmpeg().ok_or_else(|| {
            unavailable(
                "the bundled FFmpeg is missing from this installation. Reinstall Locus, or \
                 turn off Screen Video to record audio only.",
            )
        })?;
        let input = input_args(&ffmpeg)?;
        let mut capture = ScreenCapture {
            dir: dir.to_path_buf(),
            ffmpeg,
            input,
            segments: vec![],
            running: None,
            first_segment_delay: Duration::ZERO,
            epoch,
        };
        capture.spawn_segment()?;
        capture.first_segment_delay = capture.epoch.elapsed();
        Ok(capture)
    }

    fn spawn_segment(&mut self) -> Result<(), CaptureError> {
        let path = segment_path(&self.dir, self.segments.len());
        let mut command = Command::new(&self.ffmpeg);
        command
            .args(["-hide_banner", "-loglevel", "error"])
            .args(&self.input)
            .args([
                "-an",
                // Retina/odd-sized displays: H.264 4:2:0 needs even dimensions.
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "26",
                "-r",
                "30",
                "-g",
                "30",
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof",
                "-y",
            ])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| unavailable(format!("cannot start FFmpeg: {e}")))?;
        // Fail fast (bad permission, missing encoder, no display) instead of
        // showing "recording" while nothing is captured.
        let probe_until = Instant::now() + STARTUP_PROBE;
        while Instant::now() < probe_until {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut detail = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = stderr.read_to_string(&mut detail);
                    }
                    let detail = detail.trim();
                    let _ = fs::remove_file(&path);
                    return Err(unavailable(format!(
                        "FFmpeg stopped immediately ({status}){}",
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", detail.lines().last().unwrap_or(detail))
                        }
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(unavailable(format!("FFmpeg status check failed: {e}"))),
            }
        }
        self.segments.push(path.clone());
        self.running = Some(Running { child, path });
        Ok(())
    }

    /// Gracefully ends the current segment so its MP4 is finalised.
    fn stop_segment(&mut self) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        if let Some(mut stdin) = running.child.stdin.take() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        loop {
            match running.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                _ => {
                    let _ = running.child.kill();
                    let _ = running.child.wait();
                    break;
                }
            }
        }
        // Fragmented MP4 stays readable even after a kill; drop empty files only.
        if fs::metadata(&running.path)
            .map(|m| m.len() == 0)
            .unwrap_or(true)
        {
            let _ = fs::remove_file(&running.path);
            self.segments.retain(|p| p != &running.path);
        }
    }

    pub fn pause(&mut self) {
        self.stop_segment();
    }

    pub fn resume(&mut self) -> Result<(), CaptureError> {
        if self.running.is_none() {
            self.spawn_segment()?;
        }
        Ok(())
    }

    /// True while FFmpeg is still alive (false => the capture died mid-session).
    pub fn is_alive(&mut self) -> bool {
        match &mut self.running {
            Some(running) => matches!(running.child.try_wait(), Ok(None)),
            None => true, // paused
        }
    }

    pub fn finish(mut self) -> FinishedScreen {
        self.stop_segment();
        FinishedScreen {
            segments: std::mem::take(&mut self.segments),
            video_delay: self.first_segment_delay,
        }
    }

    pub fn abort(mut self) {
        self.stop_segment();
        for segment in self.segments.drain(..) {
            let _ = fs::remove_file(segment);
        }
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }
}

/// Builds the concat list for the video segments.
fn write_concat_list(list_path: &Path, segments: &[PathBuf]) -> std::io::Result<()> {
    let mut list = fs::File::create(list_path)?;
    for segment in segments {
        writeln!(
            list,
            "file '{}'",
            segment.display().to_string().replace('\'', "'\\''")
        )?;
    }
    list.sync_all()
}

/// Muxes video segments + mixed audio into one H.264/AAC MP4 with the index at
/// the front so webview playback and seeking are immediate.
pub fn mux_recording(
    ffmpeg: &Path,
    finished: &FinishedScreen,
    audio: &Path,
    output: &Path,
) -> Result<(), String> {
    if finished.segments.is_empty() {
        return Err("no video segments were captured".into());
    }
    let dir = output.parent().unwrap_or(Path::new("."));
    let list_path = dir.join("screen-segments.txt");
    write_concat_list(&list_path, &finished.segments).map_err(|e| e.to_string())?;
    let temp = output.with_extension("mp4.part");
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error"]);
    if finished.video_delay > Duration::from_millis(50) {
        command.args([
            "-itsoffset",
            &format!("{:.3}", finished.video_delay.as_secs_f64()),
        ]);
    }
    let status = command
        .args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .arg("-i")
        .arg(audio)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "-y",
        ])
        .arg(&temp)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run FFmpeg: {e}"))?;
    let _ = fs::remove_file(&list_path);
    if !status.success() {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "FFmpeg mux exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    fs::rename(&temp, output).map_err(|e| e.to_string())?;
    for segment in &finished.segments {
        let _ = fs::remove_file(segment);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FFmpeg for tests. Locally a missing FFmpeg skips FFmpeg-backed tests;
    /// CI sets `LOCUS_REQUIRE_FFMPEG=1` so a missing/broken FFmpeg is a failure
    /// instead of a silent pass.
    fn ffmpeg_for_test() -> Option<PathBuf> {
        let found = locate_ffmpeg();
        if found.is_none() && std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
            panic!("LOCUS_REQUIRE_FFMPEG=1 but no FFmpeg was found");
        }
        found
    }

    fn skip_or_fail(reason: &str) {
        if std::env::var("LOCUS_REQUIRE_FFMPEG").ok().as_deref() == Some("1") {
            panic!("LOCUS_REQUIRE_FFMPEG=1 but FFmpeg cannot run this test: {reason}");
        }
    }

    #[test]
    fn parses_avfoundation_screen_device() {
        let listing = "[AVFoundation indev @ 0x1] AVFoundation video devices:\n\
            [AVFoundation indev @ 0x1] [0] FaceTime HD Camera\n\
            [AVFoundation indev @ 0x1] [1] Capture screen 0\n\
            [AVFoundation indev @ 0x1] AVFoundation audio devices:\n\
            [AVFoundation indev @ 0x1] [0] MacBook Microphone\n";
        assert_eq!(parse_avfoundation_screen_index(listing), Some(1));
        assert_eq!(
            parse_avfoundation_screen_index("[0] FaceTime HD Camera"),
            None
        );
    }

    /// End-to-end mux with synthetic media; skipped when FFmpeg is absent.
    #[test]
    fn muxes_segments_and_audio_into_playable_mp4() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let seg = dir.path().join("screen-000.mp4");
        let ok = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=321x241:rate=30:duration=2",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
                "-c:v",
                "libx264",
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof",
                "-y",
            ])
            .arg(&seg)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            skip_or_fail("no libx264/lavfi in this FFmpeg build");
            return;
        }
        let wav = dir.path().join("audio.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for n in 0..96_000 {
            writer
                .write_sample(((n as f32 * 0.05).sin() * 8000.0) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
        let out = dir.path().join(VIDEO_FILE_NAME);
        mux_recording(
            &ffmpeg,
            &FinishedScreen {
                segments: vec![seg.clone()],
                video_delay: Duration::from_millis(300),
            },
            &wav,
            &out,
        )
        .unwrap();
        assert!(out.is_file());
        assert!(!seg.exists(), "segments are removed after a successful mux");
        let probe = Command::new(&ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(&out)
            .output()
            .unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Video: h264"), "{info}");
        assert!(info.contains("Audio: aac"), "{info}");
    }

    /// Real FFmpeg child: start, pause (segment closed), resume (new segment),
    /// stop, mux with audio, and verify the result decodes with both streams.
    #[test]
    fn pause_resume_produces_two_segments_and_a_playable_mp4() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        std::env::set_var("LOCUS_SCREEN_TEST_INPUT", "lavfi");
        let dir = tempfile::tempdir().unwrap();
        let epoch = Instant::now();
        let mut capture = match ScreenCapture::start(dir.path(), epoch) {
            Ok(capture) => capture,
            Err(error) => {
                skip_or_fail(&format!("screen capture could not start: {error}"));
                return; // FFmpeg without libx264/lavfi: nothing to verify
            }
        };
        std::thread::sleep(Duration::from_millis(1200));
        assert!(capture.is_alive());
        capture.pause();
        assert!(capture.is_alive(), "paused is not a failure");
        capture.resume().unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        let finished = capture.finish();
        assert_eq!(
            finished.segments.len(),
            2,
            "pause closes a segment, resume opens one"
        );
        for segment in &finished.segments {
            assert!(fs::metadata(segment).unwrap().len() > 0);
        }
        let wav = dir.path().join("audio.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for n in 0..96_000 {
            writer
                .write_sample(((n as f32 * 0.05).sin() * 8000.0) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
        let out = dir.path().join(VIDEO_FILE_NAME);
        mux_recording(&ffmpeg, &finished, &wav, &out).unwrap();
        let probe = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(&out)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        assert!(
            probe.status.success(),
            "must fully decode: {}",
            String::from_utf8_lossy(&probe.stderr)
        );
        std::env::remove_var("LOCUS_SCREEN_TEST_INPUT");
    }

    #[test]
    fn bundled_locations_cover_every_platform_layout() {
        let dir = Path::new("/app/bin");
        let found: Vec<String> = bundled_candidates(dir)
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(
            found.iter().any(|p| p.contains("../Resources/ffmpeg/")),
            "macOS bundle"
        );
        assert!(
            found.iter().any(|p| p.contains("/app/bin/ffmpeg/")),
            "Windows install dir"
        );
        assert!(
            found.iter().any(|p| p.contains("../lib/Locus/ffmpeg/")),
            "Linux deb/AppImage"
        );
    }

    #[test]
    fn empty_segment_list_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = mux_recording(
            Path::new("ffmpeg"),
            &FinishedScreen {
                segments: vec![],
                video_delay: Duration::ZERO,
            },
            &dir.path().join("a.wav"),
            &dir.path().join("o.mp4"),
        )
        .unwrap_err();
        assert!(err.contains("no video"));
    }
}
