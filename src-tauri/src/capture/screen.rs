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
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub const VIDEO_FILE_NAME: &str = "recording.mp4";
/// How long to wait for the first captured frame. First use on macOS can sit
/// behind the Screen Recording permission prompt, so this is generous.
const READY_TIMEOUT: Duration = Duration::from_secs(8);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STDERR_TAIL_BYTES: usize = 2048;

/// macOS: AVFoundation index of the primary screen. Listing devices spawns
/// FFmpeg (slow), so it is done once, ideally by `prewarm()` before the user
/// presses Record.
#[cfg(target_os = "macos")]
static SCREEN_DEVICE_INDEX: Mutex<Option<u32>> = Mutex::new(None);

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
        let cached = SCREEN_DEVICE_INDEX.lock().ok().and_then(|guard| *guard);
        let index = match cached {
            Some(index) => index,
            None => {
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
                if let Ok(mut guard) = SCREEN_DEVICE_INDEX.lock() {
                    *guard = Some(index);
                }
                index
            }
        };
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

/// Does the slow one-time work (locating FFmpeg, loading/Gatekeeper-checking the
/// binary, listing screen devices) so pressing Record is fast. Safe to call any
/// number of times, from any thread.
pub fn prewarm() {
    if let Some(ffmpeg) = locate_ffmpeg() {
        let _ = input_args(&ffmpeg);
        let _ = Command::new(&ffmpeg)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

struct Running {
    child: Child,
    path: PathBuf,
    stderr_tail: Arc<Mutex<String>>,
}

pub struct ScreenCapture {
    dir: PathBuf,
    ffmpeg: PathBuf,
    input: Vec<String>,
    segments: Vec<PathBuf>,
    /// Segment being recorded.
    running: Option<Running>,
    /// Segment that was told to stop and is flushing to disk.
    stopping: Option<Running>,
    /// When the first video frame of the recording was captured.
    video_started_at: Instant,
}

#[derive(Debug)]
pub struct FinishedScreen {
    pub segments: Vec<PathBuf>,
    pub video_started_at: Instant,
}

/// Reads FFmpeg's `-progress` stream on a thread and reports when the first
/// frame has been encoded: the moment recording has really begun.
fn watch_progress(stdout: impl Read + Send + 'static) -> mpsc::Receiver<Instant> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut announced = false;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if announced {
                continue; // keep draining so FFmpeg never blocks on a full pipe
            }
            let frames = line
                .strip_prefix("frame=")
                .and_then(|n| n.trim().parse::<u64>().ok())
                .unwrap_or(0);
            if frames >= 1 {
                announced = true;
                let _ = tx.send(Instant::now());
            }
        }
    });
    rx
}

/// Keeps only the last few KB of stderr (for error messages) while draining it.
fn drain_stderr(mut stderr: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let tail = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&tail);
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        while let Ok(read) = stderr.read(&mut buffer) {
            if read == 0 {
                break;
            }
            if let Ok(mut text) = shared.lock() {
                text.push_str(&String::from_utf8_lossy(&buffer[..read]));
                if text.len() > STDERR_TAIL_BYTES {
                    let mut cut = text.len() - STDERR_TAIL_BYTES;
                    while !text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    text.drain(..cut);
                }
            }
        }
    });
    tail
}

fn start_failure(
    child: &mut Child,
    tail: &Arc<Mutex<String>>,
    path: &Path,
    headline: &str,
) -> CaptureError {
    let _ = child.kill();
    let status = child.wait().ok();
    thread::sleep(Duration::from_millis(100)); // let the stderr drain thread catch up
    let detail = tail
        .lock()
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    let _ = fs::remove_file(path);
    #[cfg(target_os = "macos")]
    if let Ok(mut guard) = SCREEN_DEVICE_INDEX.lock() {
        *guard = None; // displays may have changed; rediscover next time
    }
    let code = status.map(|s| format!(" ({s})")).unwrap_or_default();
    unavailable(if detail.is_empty() {
        format!("{headline}{code}")
    } else {
        format!(
            "{headline}{code}: {}",
            detail.lines().last().unwrap_or(&detail)
        )
    })
}

impl ScreenCapture {
    /// Starts recording and returns once the first frame has been captured.
    pub fn start(dir: &Path) -> Result<Self, CaptureError> {
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
            stopping: None,
            video_started_at: Instant::now(),
        };
        capture.video_started_at = capture.spawn_segment()?;
        Ok(capture)
    }

    /// When the first video frame was captured (t=0 of the video timeline).
    pub fn started_at(&self) -> Instant {
        self.video_started_at
    }

    /// Spawns a new segment and blocks until its first frame exists. Returns
    /// the instant that frame was captured.
    fn spawn_segment(&mut self) -> Result<Instant, CaptureError> {
        let path = segment_path(&self.dir, self.segments.len());
        let mut child = Command::new(&self.ffmpeg)
            // `-progress` lets us see the first frame the moment it is encoded.
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostats",
                "-progress",
                "pipe:1",
                "-stats_period",
                "0.1",
            ])
            .args(&self.input)
            .args([
                "-an",
                // `setpts` puts the first captured frame at t=0. AVFoundation stamps
                // frames relative to when the device was opened, so without this the
                // first frame sat ~2s into the file and video lagged audio by that much.
                // Retina/odd-sized displays: H.264 4:2:0 needs even dimensions.
                "-vf",
                "setpts=PTS-STARTPTS,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                // No lookahead/B-frame buffering: first frame out immediately.
                "-tune",
                "zerolatency",
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| unavailable(format!("cannot start FFmpeg: {e}")))?;
        let ready = watch_progress(child.stdout.take().expect("piped stdout"));
        let stderr_tail = drain_stderr(child.stderr.take().expect("piped stderr"));

        let deadline = Instant::now() + READY_TIMEOUT;
        let started_at = loop {
            match ready.recv_timeout(Duration::from_millis(25)) {
                Ok(at) => break at,
                Err(RecvTimeoutError::Timeout) => {
                    if let Ok(Some(_)) = child.try_wait() {
                        return Err(start_failure(
                            &mut child,
                            &stderr_tail,
                            &path,
                            "FFmpeg stopped before capturing a frame",
                        ));
                    }
                    if Instant::now() >= deadline {
                        return Err(start_failure(
                            &mut child,
                            &stderr_tail,
                            &path,
                            "no frame was captured within 8 seconds (is Screen Recording \
                             permission granted?)",
                        ));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(start_failure(
                        &mut child,
                        &stderr_tail,
                        &path,
                        "FFmpeg exited before capturing a frame",
                    ));
                }
            }
        };
        self.segments.push(path.clone());
        self.running = Some(Running {
            child,
            path,
            stderr_tail,
        });
        Ok(started_at)
    }

    /// Tells the current segment to stop *now* without waiting for it to flush,
    /// so the caller can pause/stop audio at the same instant.
    pub fn request_stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            if let Some(mut stdin) = running.child.stdin.take() {
                let _ = stdin.write_all(b"q\n");
                let _ = stdin.flush();
            }
            self.stopping = Some(running);
        }
    }

    /// Waits for a segment told to stop to finalise its MP4.
    pub fn complete_stop(&mut self) {
        let Some(mut running) = self.stopping.take() else {
            return;
        };
        let deadline = Instant::now() + STOP_TIMEOUT;
        loop {
            match running.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
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
        let _ = &running.stderr_tail;
    }

    pub fn pause(&mut self) {
        self.request_stop();
        self.complete_stop();
    }

    /// Opens a new segment and blocks until it is really recording, so the
    /// caller can un-pause audio at that moment and no drift builds up.
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
        self.request_stop();
        self.complete_stop();
        FinishedScreen {
            segments: std::mem::take(&mut self.segments),
            video_started_at: self.video_started_at,
        }
    }

    pub fn abort(mut self) {
        self.request_stop();
        self.complete_stop();
        for segment in self.segments.drain(..) {
            let _ = fs::remove_file(segment);
        }
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        for mut running in [self.running.take(), self.stopping.take()]
            .into_iter()
            .flatten()
        {
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

/// Lossless re-time of one segment so its first frame is at t=0, whatever clock
/// the capture device used. Returns false (caller keeps the original) on failure.
fn normalize_segment(ffmpeg: &Path, segment: &Path, output: &Path) -> bool {
    let ok = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(segment)
        .args([
            "-map",
            "0:v:0",
            "-c",
            "copy",
            "-bsf:v",
            "setts=ts=TS-STARTDTS",
            "-an",
            "-f",
            "mp4",
            "-y",
        ])
        .arg(output)
        .stdin(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !ok {
        let _ = fs::remove_file(output);
    }
    ok
}

/// The "Duration"/"Stream" lines FFmpeg prints for a file (includes each stream's
/// `start`), for the sync report next to the recording.
pub fn describe_streams(ffmpeg: &Path, file: &Path) -> String {
    Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(file)
        .stdin(Stdio::null())
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .filter(|line| line.contains("Duration:") || line.contains("Stream #"))
                .map(|line| line.trim().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Muxes the video segments (and, if any, the mixed audio) into one H.264/AAC
/// MP4 with the index at the front so webview playback and seeking are immediate.
///
/// `audio` is `(wav, offset_seconds)` where the offset is how much later the
/// audio began than the first video frame: positive delays the audio, negative
/// trims the audio's head (it was recording before the first frame existed).
/// Video is never shifted, so the file never opens on a black lead-in.
pub fn mux_recording(
    ffmpeg: &Path,
    segments: &[PathBuf],
    audio: Option<(&Path, f64)>,
    output: &Path,
) -> Result<(), String> {
    if segments.is_empty() {
        return Err("no video segments were captured".into());
    }
    let dir = output.parent().unwrap_or(Path::new("."));
    let list_path = dir.join("screen-segments.txt");
    // Re-time every segment to start at 0 before concatenating, so the video
    // timeline is exactly [0, duration) and audio offsets mean what they say.
    let mut timed: Vec<PathBuf> = Vec::with_capacity(segments.len());
    let mut normalized: Vec<PathBuf> = vec![];
    for (index, segment) in segments.iter().enumerate() {
        let candidate = dir.join(format!("screen-norm-{index:03}.mp4"));
        if normalize_segment(ffmpeg, segment, &candidate) {
            normalized.push(candidate.clone());
            timed.push(candidate);
        } else {
            timed.push(segment.clone());
        }
    }
    let written = write_concat_list(&list_path, &timed).map_err(|e| e.to_string());
    if let Err(error) = written {
        for file in &normalized {
            let _ = fs::remove_file(file);
        }
        return Err(error);
    }
    let temp = output.with_extension("mp4.part");
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-loglevel", "error"])
        .args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path);
    match audio {
        Some((wav, offset)) => {
            if offset > 0.02 {
                command.args(["-itsoffset", &format!("{offset:.3}")]);
            } else if offset < -0.02 {
                command.args(["-ss", &format!("{:.3}", -offset)]);
            }
            command.arg("-i").arg(wav).args([
                "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "160k",
            ]);
        }
        None => {
            command.args(["-map", "0:v:0", "-c:v", "copy", "-an"]);
        }
    }
    let status = command
        .args(["-movflags", "+faststart", "-f", "mp4", "-y"])
        .arg(&temp)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run FFmpeg: {e}"))?;
    let _ = fs::remove_file(&list_path);
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
    // LOCUS_KEEP_SEGMENTS=1 keeps the raw video pieces for debugging sync.
    if std::env::var_os("LOCUS_KEEP_SEGMENTS").is_none() {
        for segment in segments {
            let _ = fs::remove_file(segment);
        }
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
        mux_recording(&ffmpeg, std::slice::from_ref(&seg), Some((&wav, 0.3)), &out).unwrap();
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
        let requested = Instant::now();
        let mut capture = match ScreenCapture::start(dir.path()) {
            Ok(capture) => capture,
            Err(error) => {
                skip_or_fail(&format!("screen capture could not start: {error}"));
                return; // FFmpeg without libx264/lavfi: nothing to verify
            }
        };
        // start() returns as soon as the first frame exists, not after a fixed wait.
        assert!(
            requested.elapsed() < Duration::from_millis(2500),
            "start took {:?}",
            requested.elapsed()
        );
        assert!(capture.started_at() >= requested);
        std::thread::sleep(Duration::from_millis(1200));
        assert!(capture.is_alive());
        // Pause is request + complete, split so audio can pause in between.
        capture.request_stop();
        capture.complete_stop();
        assert!(capture.is_alive(), "paused is not a failure");
        let resumed = Instant::now();
        capture.resume().unwrap();
        assert!(
            resumed.elapsed() < Duration::from_millis(2500),
            "resume waits for the first frame, not a fixed delay: {:?}",
            resumed.elapsed()
        );
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
        mux_recording(&ffmpeg, &finished.segments, Some((&wav, -0.5)), &out).unwrap();
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

    fn ffprobe_for_test(ffmpeg: &Path) -> Option<PathBuf> {
        let probe = ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        let found = probe.is_file().then_some(probe).or_else(|| {
            [
                "/usr/bin/ffprobe",
                "/opt/homebrew/bin/ffprobe",
                "/usr/local/bin/ffprobe",
            ]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
        });
        if found.is_none() {
            skip_or_fail("ffprobe not available");
        }
        found
    }

    /// (start_time, duration) in seconds of the first stream of `kind` ("v"/"a").
    fn stream_times(ffprobe: &Path, file: &Path, kind: &str) -> (f64, f64) {
        let out = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                &format!("{kind}:0"),
                "-show_entries",
                "stream=start_time,duration",
                "-of",
                "csv=p=0",
            ])
            .arg(file)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let mut parts = text.split(',');
        let start = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(f64::NAN);
        let duration = parts
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(f64::NAN);
        (start, duration)
    }

    fn make_segment_and_wav(ffmpeg: &Path, dir: &Path) -> Option<(PathBuf, PathBuf)> {
        let seg = dir.join("screen-000.mp4");
        let ok = Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=3",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
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
            return None;
        }
        let wav = dir.join("audio.wav");
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
        for n in 0..(48_000 * 3) {
            writer
                .write_sample(((n as f32 * 0.05).sin() * 8000.0) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
        Some((seg, wav))
    }

    /// Regression: the old mux shifted the VIDEO by the start-up delay, so the
    /// file opened on a black screen while audio was already playing.
    #[test]
    fn video_always_starts_at_zero_and_audio_is_aligned_to_it() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let Some(ffprobe) = ffprobe_for_test(&ffmpeg) else {
            return;
        };

        // Audio began 0.8s BEFORE the first video frame -> its head is trimmed.
        let dir = tempfile::tempdir().unwrap();
        let Some((seg, wav)) = make_segment_and_wav(&ffmpeg, dir.path()) else {
            return;
        };
        let out = dir.path().join("early_audio.mp4");
        mux_recording(&ffmpeg, &[seg], Some((&wav, -0.8)), &out).unwrap();
        let (v_start, _) = stream_times(&ffprobe, &out, "v");
        let (a_start, a_dur) = stream_times(&ffprobe, &out, "a");
        assert!(v_start.abs() < 0.05, "video must start at 0, got {v_start}");
        assert!(
            a_start.abs() < 0.08,
            "trimmed audio starts with the video, got {a_start}"
        );
        assert!(
            (a_dur - 2.2).abs() < 0.15,
            "0.8s trimmed from 3s audio, got {a_dur}"
        );

        // Audio began 0.5s AFTER the first video frame -> audio is delayed, video untouched.
        let dir = tempfile::tempdir().unwrap();
        let Some((seg, wav)) = make_segment_and_wav(&ffmpeg, dir.path()) else {
            return;
        };
        let out = dir.path().join("late_audio.mp4");
        mux_recording(&ffmpeg, &[seg], Some((&wav, 0.5)), &out).unwrap();
        let (v_start, _) = stream_times(&ffprobe, &out, "v");
        let (a_start, _) = stream_times(&ffprobe, &out, "a");
        assert!(v_start.abs() < 0.05, "video must start at 0, got {v_start}");
        assert!(
            (a_start - 0.5).abs() < 0.08,
            "audio delayed by 0.5s, got {a_start}"
        );
    }

    #[test]
    fn normalizing_a_segment_keeps_it_playable_and_starting_at_zero() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let Some(ffprobe) = ffprobe_for_test(&ffmpeg) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let Some((seg, _wav)) = make_segment_and_wav(&ffmpeg, dir.path()) else {
            return;
        };
        let out = dir.path().join("norm.mp4");
        assert!(
            normalize_segment(&ffmpeg, &seg, &out),
            "setts remux must work"
        );
        let (start, duration) = stream_times(&ffprobe, &out, "v");
        assert!(start.abs() < 0.05, "start {start}");
        assert!((duration - 3.0).abs() < 0.2, "duration {duration}");
        let report = describe_streams(&ffmpeg, &out);
        assert!(report.contains("Video: h264"), "{report}");
    }

    #[test]
    fn screen_only_recordings_mux_without_an_audio_stream() {
        let Some(ffmpeg) = ffmpeg_for_test() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let Some((seg, _wav)) = make_segment_and_wav(&ffmpeg, dir.path()) else {
            return;
        };
        let out = dir.path().join("video_only.mp4");
        mux_recording(&ffmpeg, &[seg], None, &out).unwrap();
        let probe = Command::new(&ffmpeg)
            .args(["-hide_banner", "-i"])
            .arg(&out)
            .output()
            .unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Video: h264"), "{info}");
        assert!(!info.contains("Audio:"), "{info}");
    }

    #[test]
    fn empty_segment_list_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = mux_recording(
            Path::new("ffmpeg"),
            &[],
            Some((&dir.path().join("a.wav"), 0.0)),
            &dir.path().join("o.mp4"),
        )
        .unwrap_err();
        assert!(err.contains("no video"));
    }
}
