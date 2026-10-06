//! Windows Graphics Capture adapter (NATIVE_CAPTURE_PLAN NC-6). Compiled only
//! on Windows. Output sizing, scaling and error wording live in `windows.rs`
//! and `macos.rs` (pure, tested on every platform); this file is the thin layer
//! over the `windows-capture` crate.
//!
//! STATUS: written without a Windows machine to compile or run it on. The
//! first step on Windows is `cargo check`; see docs/adr/0004-native-capture-backends.md
//! for the hardware checklist.

use super::{
    clock::MediaClock,
    frame::{PixelFormat, VideoFrame},
    macos::{cap_output_size, DEFAULT_MAX_HEIGHT, DEFAULT_MAX_WIDTH},
    source::{ScreenSource, ScreenSourceInfo, ScreenTarget},
    windows::{describe_capture_failure, scale_bgra},
    CaptureError,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(format!("Screen: {}", message.into()))
}

type HandlerError = Box<dyn std::error::Error + Send + Sync>;

/// Displays (one-based index, as `Monitor::from_index` expects) and windows
/// (their `HWND`) that can be recorded, for the frontend picker (FR1.3).
pub fn list_sources() -> Result<Vec<ScreenSourceInfo>, CaptureError> {
    let mut sources = Vec::new();
    let monitors =
        Monitor::enumerate().map_err(|e| unavailable(describe_capture_failure(&e.to_string())))?;
    for (index, monitor) in monitors.iter().enumerate() {
        let name = monitor.name().unwrap_or_default();
        sources.push(ScreenSourceInfo {
            target: ScreenTarget::Display((index + 1) as u32),
            title: if name.trim().is_empty() {
                format!("Display {}", index + 1)
            } else {
                format!("Display {} - {}", index + 1, name.trim())
            },
            width: monitor.width().unwrap_or(0),
            height: monitor.height().unwrap_or(0),
        });
    }
    for window in Window::enumerate().unwrap_or_default() {
        if !window.is_valid() {
            continue;
        }
        let title = window.title().unwrap_or_default();
        if title.trim().is_empty() {
            continue;
        }
        let (width, height) = (
            window.width().unwrap_or(0).max(0) as u32,
            window.height().unwrap_or(0).max(0) as u32,
        );
        if width < 2 || height < 2 {
            continue;
        }
        let app = window.process_name().unwrap_or_default();
        sources.push(ScreenSourceInfo {
            target: ScreenTarget::Window(window.as_raw_hwnd() as usize as u64),
            title: if app.trim().is_empty() {
                title
            } else {
                format!("{} - {title}", app.trim())
            },
            width,
            height,
        });
    }
    Ok(sources)
}

struct Shared {
    paused: AtomicBool,
    stop: AtomicBool,
    failure: Mutex<Option<String>>,
}

struct HandlerFlags {
    frames: SyncSender<VideoFrame>,
    clock: Arc<MediaClock>,
    shared: Arc<Shared>,
    first_tx: SyncSender<Instant>,
    /// Fixed output size; every frame is scaled to it.
    out: (u32, u32),
}

struct Handler {
    flags: HandlerFlags,
    first_sent: bool,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = HandlerFlags;
    type Error = HandlerError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            flags: ctx.flags,
            first_sent: false,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let shared = &self.flags.shared;
        if shared.stop.load(Ordering::SeqCst) {
            control.stop();
            return Ok(());
        }
        // Pausing keeps the capture session open (instant resume) and only
        // stops forwarding pictures.
        if shared.paused.load(Ordering::Relaxed) {
            return Ok(());
        }
        let captured_at = Instant::now();
        let (width, height) = (frame.width() as usize, frame.height() as usize);
        let mut buffer = frame.buffer()?;
        let pitch = buffer.row_pitch() as usize;
        let (out_w, out_h) = self.flags.out;
        let Some(packed) = scale_bgra(
            buffer.as_raw_buffer(),
            width,
            height,
            pitch,
            out_w as usize,
            out_h as usize,
        ) else {
            return Ok(()); // short or odd buffer: skip this picture
        };
        let pts = if self.first_sent {
            self.flags.clock.stamp(captured_at)
        } else {
            self.first_sent = true;
            let pts = self.flags.clock.register_source("video", captured_at);
            let _ = self.flags.first_tx.try_send(captured_at);
            pts
        };
        // Never block the capture callback: a full queue drops the frame.
        let _ = self.flags.frames.try_send(VideoFrame::packed(
            packed,
            out_w,
            out_h,
            PixelFormat::Bgra,
            pts,
        ));
        Ok(())
    }

    /// The captured window closed or the display went away.
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        if let Ok(mut slot) = self.flags.shared.failure.lock() {
            slot.get_or_insert_with(|| {
                "The captured window or display is no longer available.".into()
            });
        }
        Ok(())
    }
}

fn launch<T>(
    item: T,
    flags: HandlerFlags,
) -> Result<CaptureControl<Handler, HandlerError>, CaptureError>
where
    T: TryInto<GraphicsCaptureItemType> + Send + 'static,
{
    // Default cursor and border: the only choice that works on every build
    // from Windows 10 1903. The cursor is recorded; the OS draws its yellow
    // capture border (removing it needs Windows 11 and a permission prompt).
    let settings = Settings::new(
        item,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    );
    Handler::start_free_threaded(settings)
        .map_err(|e| unavailable(describe_capture_failure(&e.to_string())))
}

/// Records one display or window through Windows Graphics Capture as BGRA frames.
pub struct WinScreenSource {
    control: Option<CaptureControl<Handler, HandlerError>>,
    shared: Arc<Shared>,
}

impl WinScreenSource {
    pub fn new() -> Self {
        Self {
            control: None,
            shared: Arc::new(Shared {
                paused: AtomicBool::new(false),
                stop: AtomicBool::new(false),
                failure: Mutex::new(None),
            }),
        }
    }
}

impl Default for WinScreenSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenSource for WinScreenSource {
    fn start(
        &mut self,
        target: &ScreenTarget,
        clock: Arc<MediaClock>,
        frames: SyncSender<VideoFrame>,
    ) -> Result<Instant, CaptureError> {
        let describe =
            |e: &dyn std::fmt::Display| unavailable(describe_capture_failure(&e.to_string()));
        let (first_tx, first_rx) = sync_channel::<Instant>(1);
        let make_flags = |source_w: u32, source_h: u32| HandlerFlags {
            frames: frames.clone(),
            clock: clock.clone(),
            shared: self.shared.clone(),
            first_tx: first_tx.clone(),
            // Fixed for the whole recording, so a resolution switch or a resized
            // window never changes the encoded picture size mid-segment.
            out: cap_output_size(source_w, source_h, DEFAULT_MAX_WIDTH, DEFAULT_MAX_HEIGHT),
        };
        let control = match target {
            ScreenTarget::PrimaryDisplay | ScreenTarget::Display(_) => {
                let monitor = match target {
                    ScreenTarget::Display(index) => Monitor::from_index(*index as usize),
                    _ => Monitor::primary(),
                }
                .map_err(|e| describe(&e))?;
                let flags = make_flags(
                    monitor.width().map_err(|e| describe(&e))?,
                    monitor.height().map_err(|e| describe(&e))?,
                );
                launch(monitor, flags)?
            }
            ScreenTarget::Window(hwnd) => {
                let window = Window::from_raw_hwnd(*hwnd as usize as *mut std::ffi::c_void);
                if !window.is_valid() {
                    return Err(unavailable("the selected window is no longer available"));
                }
                let flags = make_flags(
                    window.width().map_err(|e| describe(&e))?.max(2) as u32,
                    window.height().map_err(|e| describe(&e))?.max(2) as u32,
                );
                launch(window, flags)?
            }
        };
        match first_rx.recv_timeout(FIRST_FRAME_TIMEOUT) {
            Ok(first_at) => {
                self.control = Some(control);
                Ok(first_at)
            }
            Err(_) => {
                let _ = control.stop();
                Err(unavailable(
                    "no picture arrived from the screen. If Windows asked for permission, allow \
                     it and try again.",
                ))
            }
        }
    }

    fn pause(&mut self) {
        self.shared.paused.store(true, Ordering::SeqCst);
    }

    fn resume(&mut self) -> Result<(), CaptureError> {
        self.shared.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }

    fn failure(&self) -> Option<String> {
        self.shared.failure.lock().ok().and_then(|f| f.clone())
    }
}
