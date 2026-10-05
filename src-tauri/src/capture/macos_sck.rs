//! macOS ScreenCaptureKit adapters (NATIVE_CAPTURE_PLAN NC-2 video, NC-3 system
//! audio). Compiled only on macOS. The decisions that need no OS (output
//! sizing, plane packing, error wording, audio downmix) live in `macos.rs`
//! and are unit-tested on every platform; this file is the thin layer over the
//! `screencapturekit` crate.
//!
//! STATUS: written without a Mac to compile or run it on. The first step on a
//! Mac is `cargo check`; see docs/adr/0004-native-capture-backends.md for the
//! hardware checklist.

use super::{
    clock::MediaClock,
    frame::{PixelFormat as FramePixelFormat, VideoFrame},
    macos::{
        cap_output_size, describe_capture_failure, downmix_to_mono, f32_samples_from_le_bytes,
        pack_nv12, DEFAULT_MAX_HEIGHT, DEFAULT_MAX_WIDTH,
    },
    source::{ScreenSource, ScreenSourceInfo, ScreenTarget},
    CaptureError,
};
use screencapturekit::{cm::SCFrameStatus, prelude::*, stream::delegate_trait::StreamCallbacks};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// How long to wait for the first picture / audio buffer after starting.
const FIRST_SAMPLE_TIMEOUT: Duration = Duration::from_secs(5);
const FIRST_AUDIO_TIMEOUT: Duration = Duration::from_secs(1);
const CAPTURE_FPS: u32 = 30;
const SYSTEM_AUDIO_RATE: u32 = 48_000;

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(format!("Screen: {}", message.into()))
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGMainDisplayID() -> u32;
}

/// Lists what can be captured. The first call on a Mac that has never been
/// asked shows the system's screen-recording prompt (M2.02). If access is
/// denied, the error says exactly what to switch on.
fn shareable_content() -> Result<SCShareableContent, CaptureError> {
    // SAFETY: plain CoreGraphics queries with no arguments.
    let granted = unsafe { CGPreflightScreenCaptureAccess() };
    if !granted {
        // Shows the prompt the first time; returns immediately either way.
        let _ = unsafe { CGRequestScreenCaptureAccess() };
    }
    // The preflight call is known to lag behind a fresh grant, so the real
    // test is whether ScreenCaptureKit will list content.
    SCShareableContent::get().map_err(|e| unavailable(describe_capture_failure(&e.to_string())))
}

pub fn list_sources() -> Result<Vec<ScreenSourceInfo>, CaptureError> {
    let content = shareable_content()?;
    let mut sources = Vec::new();
    for (index, display) in content.displays().iter().enumerate() {
        sources.push(ScreenSourceInfo {
            target: ScreenTarget::Display(display.display_id()),
            title: format!("Display {}", index + 1),
            width: display.width(),
            height: display.height(),
        });
    }
    for window in content.windows() {
        // Layer 0 and on screen: skip menu bar items, overlays and hidden windows.
        if !window.is_on_screen() || window.window_layer() != 0 {
            continue;
        }
        let title = window.title().unwrap_or_default();
        let app = window
            .owning_application()
            .map(|a| a.application_name())
            .unwrap_or_default();
        if title.trim().is_empty() && app.trim().is_empty() {
            continue;
        }
        let frame = window.frame();
        let (width, height) = (frame.size.width as u32, frame.size.height as u32);
        if width < 2 || height < 2 {
            continue;
        }
        let label = match (app.trim().is_empty(), title.trim().is_empty()) {
            (false, false) => format!("{app} - {title}"),
            (false, true) => app,
            _ => title,
        };
        sources.push(ScreenSourceInfo {
            target: ScreenTarget::Window(u64::from(window.window_id())),
            title: label,
            width,
            height,
        });
    }
    Ok(sources)
}

fn build_filter(
    content: &SCShareableContent,
    target: &ScreenTarget,
) -> Result<(SCContentFilter, (u32, u32)), CaptureError> {
    match target {
        ScreenTarget::PrimaryDisplay | ScreenTarget::Display(_) => {
            let displays = content.displays();
            let wanted = match target {
                ScreenTarget::Display(id) => Some(*id),
                // SAFETY: no arguments.
                _ => Some(unsafe { CGMainDisplayID() }),
            };
            let display = displays
                .iter()
                .find(|d| Some(d.display_id()) == wanted)
                .or_else(|| displays.first())
                .ok_or_else(|| unavailable("no display is available to record"))?;
            let filter = SCContentFilter::create()
                .with_display(display)
                .with_excluding_windows(&[])
                .build();
            Ok((filter, (display.width(), display.height())))
        }
        ScreenTarget::Window(id) => {
            let window = content
                .windows()
                .into_iter()
                .find(|w| u64::from(w.window_id()) == *id)
                .ok_or_else(|| unavailable("the selected window is no longer available"))?;
            let frame = window.frame();
            let size = (frame.size.width as u32, frame.size.height as u32);
            Ok((SCContentFilter::create().with_window(&window).build(), size))
        }
    }
}

struct Shared {
    paused: AtomicBool,
    failure: Mutex<Option<String>>,
}

/// Records one display or window through ScreenCaptureKit as NV12 frames.
pub struct MacScreenSource {
    stream: Option<SCStream>,
    shared: Arc<Shared>,
}

impl MacScreenSource {
    pub fn new() -> Self {
        Self {
            stream: None,
            shared: Arc::new(Shared {
                paused: AtomicBool::new(false),
                failure: Mutex::new(None),
            }),
        }
    }
}

impl Default for MacScreenSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenSource for MacScreenSource {
    fn start(
        &mut self,
        target: &ScreenTarget,
        clock: Arc<MediaClock>,
        frames: SyncSender<VideoFrame>,
    ) -> Result<Instant, CaptureError> {
        let content = shareable_content()?;
        let (filter, (source_w, source_h)) = build_filter(&content, target)?;
        // The size is fixed for the whole recording: ScreenCaptureKit scales the
        // display or window to it, so a resolution switch or a resized window
        // never changes the encoded picture size mid-segment.
        let (width, height) =
            cap_output_size(source_w, source_h, DEFAULT_MAX_WIDTH, DEFAULT_MAX_HEIGHT);
        let config = SCStreamConfiguration::new()
            .with_width(width)
            .with_height(height)
            .with_pixel_format(PixelFormat::YCbCr_420v)
            .with_fps(CAPTURE_FPS)
            .with_queue_depth(5)
            .with_shows_cursor(true)
            .with_scales_to_fit(true);

        let shared = self.shared.clone();
        let delegate = StreamCallbacks::new().on_stop(move |error| {
            // Fires when the OS ends the stream: permission revoked, display
            // unplugged, window closed. Never an ordinary `stop_capture`.
            if let Some(raw) = error {
                if let Ok(mut slot) = shared.failure.lock() {
                    *slot = Some(describe_capture_failure(&raw));
                }
            }
        });
        let mut stream = SCStream::new_with_delegate(&filter, &config, delegate);

        let (first_tx, first_rx) = sync_channel::<Instant>(1);
        let first_tx = Mutex::new(first_tx);
        let frames = Mutex::new(frames);
        let shared = self.shared.clone();
        let first_sent = AtomicBool::new(false);
        let handler = move |sample: CMSampleBuffer, kind: SCStreamOutputType| {
            if !matches!(kind, SCStreamOutputType::Screen) {
                return;
            }
            if shared.paused.load(Ordering::Relaxed) {
                return;
            }
            // Idle / blank / suspended notices carry no picture; the pacer
            // repeats the previous frame over an unchanging screen.
            match sample.frame_status() {
                Some(SCFrameStatus::Complete) | None => {}
                Some(_) => return,
            }
            let captured_at = Instant::now();
            let Some(pixel_buffer) = sample.pixel_buffer() else {
                return;
            };
            let Ok(guard) = pixel_buffer.lock_read_only() else {
                return;
            };
            if guard.plane_count() < 2 {
                return; // not the bi-planar NV12 layout we asked for
            }
            let (width, height) = (guard.width() & !1, guard.height() & !1);
            let (Some(luma_ptr), Some(chroma_ptr)) = (
                guard.base_address_of_plane(0),
                guard.base_address_of_plane(1),
            ) else {
                return;
            };
            let luma_stride = guard.bytes_per_row_of_plane(0);
            let chroma_stride = guard.bytes_per_row_of_plane(1);
            if width == 0 || height == 0 {
                return;
            }
            // SAFETY: the buffer is locked for reading for as long as `guard`
            // lives, and each slice stops at the last byte of its last row.
            let (luma, chroma) = unsafe {
                (
                    std::slice::from_raw_parts(luma_ptr, luma_stride * (height - 1) + width),
                    std::slice::from_raw_parts(
                        chroma_ptr,
                        chroma_stride * (height / 2 - 1) + width,
                    ),
                )
            };
            let Some(packed) = pack_nv12(luma, luma_stride, chroma, chroma_stride, width, height)
            else {
                return;
            };
            drop(guard);
            let pts = if first_sent.swap(true, Ordering::SeqCst) {
                clock.stamp(captured_at)
            } else {
                let pts = clock.register_source("video", captured_at);
                if let Ok(tx) = first_tx.lock() {
                    let _ = tx.try_send(captured_at);
                }
                pts
            };
            let frame = VideoFrame::packed(
                packed,
                width as u32,
                height as u32,
                FramePixelFormat::Nv12,
                pts,
            );
            // Never block the capture queue: a full queue drops the frame.
            if let Ok(sender) = frames.lock() {
                let _ = sender.try_send(frame);
            }
        };
        stream.add_output_handler(handler, SCStreamOutputType::Screen);
        stream
            .start_capture()
            .map_err(|e| unavailable(describe_capture_failure(&e.to_string())))?;

        match first_rx.recv_timeout(FIRST_SAMPLE_TIMEOUT) {
            Ok(first_at) => {
                self.stream = Some(stream);
                Ok(first_at)
            }
            Err(_) => {
                let _ = stream.stop_capture();
                Err(unavailable(
                    "no picture arrived from the screen. If macOS asked for screen recording \
                     access, allow Locus in System Settings > Privacy & Security > Screen & \
                     System Audio Recording, then quit and reopen Locus.",
                ))
            }
        }
    }

    /// Pausing only stops forwarding frames. The stream keeps running so that
    /// resume is instant (no 2 s device start-up like the FFmpeg path).
    fn pause(&mut self) {
        self.shared.paused.store(true, Ordering::SeqCst);
    }

    fn resume(&mut self) -> Result<(), CaptureError> {
        self.shared.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.stop_capture();
        }
    }

    fn failure(&self) -> Option<String> {
        self.shared.failure.lock().ok().and_then(|f| f.clone())
    }
}

/// System audio from ScreenCaptureKit (NC-3): works from macOS 13, where the
/// CoreAudio tap needs 14.6. It rides its own stream with a 2x2 picture that
/// is never read, so audio-only sessions work and the audio does not depend
/// on a screen being recorded.
pub struct SystemAudioStream {
    stream: Option<SCStream>,
}

impl SystemAudioStream {
    /// Starts capture. `on_samples` gets mono f32 samples at 48 kHz from
    /// ScreenCaptureKit's callback thread. Returns the stream and the instant
    /// its first sample began (arrival of the first buffer minus its length;
    /// the start call's time if the system is silent and nothing arrives).
    pub fn start(
        on_samples: impl Fn(&[f32]) + Send + Sync + 'static,
    ) -> Result<(Self, Instant), String> {
        let content = shareable_content().map_err(|e| e.to_string())?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| "no display is available for system audio".to_string())?;
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();
        let config = SCStreamConfiguration::new()
            .with_width(2)
            .with_height(2)
            .with_fps(1)
            .with_captures_audio(true)
            .with_sample_rate(SYSTEM_AUDIO_RATE as i32)
            .with_channel_count(1)
            .with_excludes_current_process_audio(true);
        let mut stream = SCStream::new(&filter, &config);

        let (first_tx, first_rx) = sync_channel::<Instant>(1);
        let first_tx = Mutex::new(first_tx);
        let first_sent = AtomicBool::new(false);
        stream.add_output_handler(
            move |sample: CMSampleBuffer, kind: SCStreamOutputType| {
                if !matches!(kind, SCStreamOutputType::Audio) {
                    return;
                }
                let arrived = Instant::now();
                let Some(list) = sample.audio_buffer_list() else {
                    return;
                };
                let buffers: Vec<(u32, Vec<f32>)> = (0..list.num_buffers())
                    .filter_map(|i| list.get(i))
                    .map(|b| (b.number_channels(), f32_samples_from_le_bytes(b.data())))
                    .collect();
                let mono = downmix_to_mono(&buffers);
                if mono.is_empty() {
                    return;
                }
                if !first_sent.swap(true, Ordering::SeqCst) {
                    let length =
                        Duration::from_secs_f64(mono.len() as f64 / f64::from(SYSTEM_AUDIO_RATE));
                    if let Ok(tx) = first_tx.lock() {
                        let _ = tx.try_send(arrived.checked_sub(length).unwrap_or(arrived));
                    }
                }
                on_samples(&mono);
            },
            SCStreamOutputType::Audio,
        );
        let requested_at = Instant::now();
        stream
            .start_capture()
            .map_err(|e| describe_capture_failure(&e.to_string()))?;
        let started_at = first_rx
            .recv_timeout(FIRST_AUDIO_TIMEOUT)
            .unwrap_or(requested_at);
        Ok((
            Self {
                stream: Some(stream),
            },
            started_at,
        ))
    }

    pub fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.stop_capture();
        }
    }
}

impl Drop for SystemAudioStream {
    fn drop(&mut self) {
        self.stop();
    }
}
