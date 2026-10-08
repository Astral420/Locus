//! Linux screen capture: an xdg-desktop-portal ScreenCast session plus a
//! PipeWire video stream (NATIVE_CAPTURE_PLAN NC-7, FR1.14, NFR12a).
//!
//! `LinuxScreenSource` implements the shared `ScreenSource` trait. The portal
//! shows the compositor's own screen/window chooser; the stream is then read
//! from the PipeWire remote the portal hands back. Pause only stops forwarding
//! frames, so resume is instant. If the share ends on its own (the user stops
//! it from the desktop, the window closes) the stream leaves the streaming
//! state and `failure()` reports it so the manager preserves the take.
//!
//! Compiled on Linux only; the logic that needs no OS lives in `linux.rs`.

use super::{
    clock::MediaClock,
    frame::{PixelFormat, VideoFrame},
    linux::{
        describe_pipewire_failure, describe_portal_failure, output_size_for, portal_sources,
        to_output_bgra, PortalSources, RawLayout,
    },
    source::{ScreenSource, ScreenSourceInfo, ScreenTarget},
    CaptureError,
};
use ashpd::{
    desktop::{
        screencast::{
            CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
            StartCastOptions,
        },
        CreateSessionOptions, PersistMode, Session,
    },
    enumflags2::BitFlags,
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::{
    param::{
        format::{FormatProperties, MediaSubtype, MediaType},
        format_utils,
        video::{VideoFormat, VideoInfoRaw},
    },
    pod::Pod,
    utils::{Fraction, Rectangle},
};
use std::{
    os::fd::OwnedFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long `start` waits for the portal dialog to be answered and the first
/// picture to arrive. The dialog needs a person, so this is generous.
const START_TIMEOUT: Duration = Duration::from_secs(300);
/// Once the stream is connected, a picture must arrive this quickly. A source
/// that sends nothing (some compositors only send on change) is reported
/// instead of recording a phantom video.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(10);

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(message.into())
}

/// Checks that a PipeWire daemon is reachable (preflight, NFR12a).
pub fn probe_pipewire() -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|e| describe_pipewire_failure(&e.to_string()))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|e| describe_pipewire_failure(&e.to_string()))?;
    let _core = context
        .connect_rc(None)
        .map_err(|e| describe_pipewire_failure(&e.to_string()))?;
    Ok(())
}

/// Checks that the desktop offers the ScreenCast portal (preflight).
pub fn probe_portal() -> Result<(), String> {
    async_io::block_on(async {
        let screencast = Screencast::new()
            .await
            .map_err(|e| describe_portal_failure(&e.to_string()))?;
        let types = screencast
            .available_source_types()
            .await
            .map_err(|e| describe_portal_failure(&e.to_string()))?;
        if types.is_empty() {
            return Err(describe_portal_failure("ServiceUnknown: no source types"));
        }
        Ok(())
    })
}

/// What the picker shows. Wayland does not let an app enumerate screens or
/// windows, so there is one entry; choosing happens in the system dialog when
/// recording starts (the target only narrows what that dialog offers).
pub fn list_sources() -> Result<Vec<ScreenSourceInfo>, CaptureError> {
    probe_portal().map_err(unavailable)?;
    Ok(vec![ScreenSourceInfo {
        target: ScreenTarget::PrimaryDisplay,
        title: "Screen or window (choose in the system dialog)".into(),
        width: 0,
        height: 0,
    }])
}

/// State shared between the capture thread and the manager's calls.
#[derive(Default)]
struct Shared {
    paused: AtomicBool,
    /// Set by `stop` before the loop is told to quit, so tearing the stream
    /// down is not mistaken for the source ending.
    stopping: AtomicBool,
    /// Set once the first picture was handed to the pipeline.
    first_frame: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl Shared {
    fn fail(&self, message: String) {
        if let Ok(mut slot) = self.failure.lock() {
            slot.get_or_insert(message);
        }
    }

    fn failure(&self) -> Option<String> {
        self.failure.lock().ok().and_then(|f| f.clone())
    }
}

enum Control {
    Stop,
}

/// Where the stream comes from: the portal's chooser, or (tests only) a node
/// that already exists on the local PipeWire daemon.
#[derive(Clone, Copy)]
enum Origin {
    Portal(PortalSources),
    #[cfg(test)]
    Node(u32),
}

pub struct LinuxScreenSource {
    shared: Arc<Shared>,
    stop: Option<pw::channel::Sender<Control>>,
    handle: Option<JoinHandle<()>>,
}

impl LinuxScreenSource {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared::default()),
            stop: None,
            handle: None,
        }
    }

    fn start_from(
        &mut self,
        origin: Origin,
        clock: Arc<MediaClock>,
        frames: SyncSender<VideoFrame>,
    ) -> Result<Instant, CaptureError> {
        if self.handle.is_some() {
            return Err(unavailable("screen capture is already running"));
        }
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<Instant, String>>(1);
        let (stop_tx, stop_rx) = pw::channel::channel::<Control>();
        let shared = Arc::clone(&self.shared);
        let handle = thread::Builder::new()
            .name("locus-pipewire-video".into())
            .spawn(move || capture_thread(origin, clock, frames, shared, stop_rx, ready_tx))
            .map_err(|e| unavailable(format!("cannot start the screen capture thread: {e}")))?;
        self.stop = Some(stop_tx);
        self.handle = Some(handle);
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(first_frame_at)) => Ok(first_frame_at),
            Ok(Err(message)) => {
                self.stop();
                Err(unavailable(message))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.stop();
                Err(unavailable(
                    "Timed out waiting for the screen-sharing dialog or the first picture.",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.stop();
                Err(unavailable(self.shared.failure().unwrap_or_else(|| {
                    "The screen capture thread ended unexpectedly.".into()
                })))
            }
        }
    }
}

impl Default for LinuxScreenSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LinuxScreenSource {
    fn drop(&mut self) {
        self.stop();
    }
}

impl ScreenSource for LinuxScreenSource {
    fn start(
        &mut self,
        target: &ScreenTarget,
        clock: Arc<MediaClock>,
        frames: SyncSender<VideoFrame>,
    ) -> Result<Instant, CaptureError> {
        self.start_from(Origin::Portal(portal_sources(target)), clock, frames)
    }
    fn pause(&mut self) {
        self.shared.paused.store(true, Ordering::SeqCst);
    }

    fn resume(&mut self) -> Result<(), CaptureError> {
        self.shared.paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(Control::Stop);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    fn failure(&self) -> Option<String> {
        self.shared.failure()
    }
}

/// What the portal returned for the chosen source.
struct PortalStream {
    node_id: u32,
    fd: OwnedFd,
    session: Session<Screencast>,
}

async fn open_portal(sources: PortalSources) -> Result<PortalStream, String> {
    let fail = |e: ashpd::Error| describe_portal_failure(&e.to_string());
    let screencast = Screencast::new().await.map_err(fail)?;
    let session = screencast
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(fail)?;
    let types: BitFlags<SourceType> = match sources {
        PortalSources::Monitor => SourceType::Monitor.into(),
        PortalSources::Window => SourceType::Window.into(),
        PortalSources::MonitorOrWindow => SourceType::Monitor | SourceType::Window,
    };
    // Record the pointer when the compositor can draw it into the picture;
    // otherwise accept its default.
    let embed_cursor = screencast
        .available_cursor_modes()
        .await
        .map(|modes| modes.contains(CursorMode::Embedded))
        .unwrap_or(false);
    let mut options = SelectSourcesOptions::default()
        .set_sources(types)
        .set_multiple(false)
        // No restore token: the dialog is shown on every recording, so a
        // recording never starts on a screen the person did not just choose.
        .set_persist_mode(PersistMode::DoNot);
    if embed_cursor {
        options = options.set_cursor_mode(CursorMode::Embedded);
    }
    screencast
        .select_sources(&session, options)
        .await
        .map_err(fail)?
        .response()
        .map_err(fail)?;
    // `start` is where the compositor shows its chooser and waits for a person.
    let streams = screencast
        .start(&session, None, StartCastOptions::default())
        .await
        .map_err(fail)?
        .response()
        .map_err(fail)?;
    let node_id = streams
        .streams()
        .first()
        .map(|stream| stream.pipe_wire_node_id())
        .ok_or_else(|| "The desktop did not share any screen or window.".to_string())?;
    let fd = screencast
        .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
        .await
        .map_err(fail)?;
    Ok(PortalStream {
        node_id,
        fd,
        session,
    })
}

fn capture_thread(
    origin: Origin,
    clock: Arc<MediaClock>,
    frames: SyncSender<VideoFrame>,
    shared: Arc<Shared>,
    stop_rx: pw::channel::Receiver<Control>,
    ready: SyncSender<Result<Instant, String>>,
) {
    let (node_id, fd, session) = match origin {
        Origin::Portal(sources) => match async_io::block_on(open_portal(sources)) {
            Ok(PortalStream {
                node_id,
                fd,
                session,
            }) => (node_id, Some(fd), Some(session)),
            Err(message) => {
                let _ = ready.send(Err(message));
                return;
            }
        },
        #[cfg(test)]
        Origin::Node(node_id) => (node_id, None, None),
    };
    if let Err(message) = run_stream(node_id, fd, clock, frames, &shared, stop_rx, &ready) {
        // Before the first picture the start call reports it; afterwards the
        // watchdog reads it through `failure()`.
        let _ = ready.try_send(Err(message.clone()));
        shared.fail(message);
    }
    if let Some(session) = session {
        let _ = async_io::block_on(session.close());
    }
}

/// Per-stream state touched only on the PipeWire loop thread.
struct VideoState {
    layout: Option<RawLayout>,
    source_size: (u32, u32),
    /// Fixed for the whole recording from the first negotiated size.
    output_size: Option<(u32, u32)>,
    first_frame_sent: bool,
    clock: Arc<MediaClock>,
    frames: SyncSender<VideoFrame>,
    shared: Arc<Shared>,
    ready: SyncSender<Result<Instant, String>>,
    mainloop: pw::main_loop::MainLoopRc,
}

impl VideoState {
    fn fail_and_quit(&self, message: String) {
        let _ = self.ready.try_send(Err(message.clone()));
        self.shared.fail(message);
        self.mainloop.quit();
    }
}

fn layout_of(format: VideoFormat) -> Option<RawLayout> {
    if format == VideoFormat::BGRx || format == VideoFormat::BGRA {
        Some(RawLayout::Bgr)
    } else if format == VideoFormat::RGBx || format == VideoFormat::RGBA {
        Some(RawLayout::Rgb)
    } else {
        None
    }
}

fn enum_format_pod() -> Result<Vec<u8>, String> {
    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        // Shared-memory 32-bit RGB only: no DMA-BUF modifiers are offered, so
        // the compositor falls back to memory buffers Locus can read directly.
        spa::pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        spa::pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle {
                width: 1920,
                height: 1080
            },
            Rectangle {
                width: 1,
                height: 1
            },
            Rectangle {
                width: 16384,
                height: 16384
            }
        ),
        spa::pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 30, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: 144, denom: 1 }
        ),
    );
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|e| format!("cannot describe the video format: {e:?}"))
}

fn run_stream(
    node_id: u32,
    fd: Option<OwnedFd>,
    clock: Arc<MediaClock>,
    frames: SyncSender<VideoFrame>,
    shared: &Arc<Shared>,
    stop_rx: pw::channel::Receiver<Control>,
    ready: &SyncSender<Result<Instant, String>>,
) -> Result<(), String> {
    let pw_err = |e: pw::Error| describe_pipewire_failure(&e.to_string());
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pw_err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pw_err)?;
    // The portal's remote: only the shared screen is visible on it. Tests
    // connect to the local daemon instead.
    let core = match fd {
        Some(fd) => context.connect_fd_rc(fd, None),
        None => context.connect_rc(None),
    }
    .map_err(pw_err)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "locus-screen",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(pw_err)?;

    let _stop = stop_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let shared = Arc::clone(shared);
        move |Control::Stop| {
            shared.stopping.store(true, Ordering::SeqCst);
            mainloop.quit();
        }
    });

    let state = VideoState {
        layout: None,
        source_size: (0, 0),
        output_size: None,
        first_frame_sent: false,
        clock,
        frames,
        shared: Arc::clone(shared),
        ready: ready.clone(),
        mainloop: mainloop.clone(),
    };

    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(|_, state, old, new| {
            use pw::stream::StreamState;
            if state.shared.stopping.load(Ordering::SeqCst) {
                return;
            }
            match new {
                StreamState::Error(message) => {
                    state.fail_and_quit(describe_pipewire_failure(&message));
                }
                StreamState::Unconnected
                    if matches!(old, StreamState::Streaming | StreamState::Paused) =>
                {
                    state.fail_and_quit(
                        "The screen share ended (sharing was stopped or the window closed).".into(),
                    );
                }
                _ => {}
            }
        })
        .param_changed(|_, state, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                return;
            };
            if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
                return;
            }
            let mut info = VideoInfoRaw::new();
            if info.parse(param).is_err() {
                return;
            }
            let Some(layout) = layout_of(info.format()) else {
                state.fail_and_quit(format!(
                    "The desktop sent an unsupported picture format ({:?}).",
                    info.format()
                ));
                return;
            };
            let size = info.size();
            state.layout = Some(layout);
            state.source_size = (size.width, size.height);
            if state.output_size.is_none() {
                state.output_size = Some(output_size_for(size.width, size.height));
            }
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let chunk = data.chunk();
            let (size, offset, stride, flags) = (
                chunk.size() as usize,
                chunk.offset() as usize,
                chunk.stride(),
                chunk.flags(),
            );
            // An empty or corrupted chunk is a cursor-only or failed update.
            if size == 0 || flags.contains(spa::buffer::ChunkFlags::CORRUPTED) {
                return;
            }
            let captured_at = Instant::now();
            let paused = state.shared.paused.load(Ordering::Relaxed);
            if paused && state.first_frame_sent {
                return;
            }
            let (Some(layout), Some((out_w, out_h))) = (state.layout, state.output_size) else {
                return;
            };
            let (src_w, src_h) = state.source_size;
            let Some(bytes) = data.data() else { return };
            let end = offset.saturating_add(size).min(bytes.len());
            let Some(pixels) = bytes.get(offset..end) else {
                return;
            };
            let stride = if stride > 0 {
                stride as usize
            } else {
                src_w as usize * 4
            };
            let Some(packed) = to_output_bgra(
                pixels,
                src_w as usize,
                src_h as usize,
                stride,
                layout,
                out_w as usize,
                out_h as usize,
            ) else {
                return;
            };
            let pts = if state.first_frame_sent {
                state.clock.stamp(captured_at)
            } else {
                state.clock.register_source("video", captured_at)
            };
            let frame = VideoFrame::packed(packed, out_w, out_h, PixelFormat::Bgra, pts);
            match state.frames.try_send(frame) {
                Ok(()) => {}
                // A full queue drops the picture: the capture callback never blocks.
                Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    state.shared.stopping.store(true, Ordering::SeqCst);
                    state.mainloop.quit();
                    return;
                }
            }
            if !state.first_frame_sent {
                state.first_frame_sent = true;
                state.shared.first_frame.store(true, Ordering::SeqCst);
                let _ = state.ready.try_send(Ok(captured_at));
            }
        })
        .register()
        .map_err(pw_err)?;

    let values = enum_format_pod()?;
    let mut params = [Pod::from_bytes(&values).ok_or("cannot build the video format")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            // DONT_RECONNECT: when the shared source goes away, end the stream
            // (reported as a failure) instead of letting the session manager
            // re-target it somewhere else.
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::DONT_RECONNECT,
            &mut params,
        )
        .map_err(pw_err)?;

    // No picture within the deadline: report it rather than record nothing.
    let timer = mainloop.loop_().add_timer({
        let shared = Arc::clone(shared);
        let ready = ready.clone();
        let mainloop = mainloop.clone();
        move |_| {
            if shared.stopping.load(Ordering::SeqCst) || shared.first_frame.load(Ordering::SeqCst) {
                return;
            }
            let message = "No picture arrived from the shared screen. Some desktops only send a \
                           picture when something changes: move a window and try again.";
            let _ = ready.try_send(Err(message.into()));
            shared.fail(message.into());
            mainloop.quit();
        }
    });
    timer
        .update_timer(Some(FIRST_FRAME_TIMEOUT), None)
        .into_result()
        .map_err(|e| format!("cannot arm the first-frame timer: {e}"))?;

    mainloop.run();
    let _ = stream.disconnect();
    Ok(())
}

#[cfg(test)]
mod tests {
    //! These run the real stream code against a real PipeWire daemon, with
    //! GStreamer's `pipewiresink` standing in for the compositor (everything
    //! except the portal dialog). They need a running PipeWire with a session
    //! manager, `gst-launch-1.0` with `pipewiresink`, and `pw-dump`, and are
    //! skipped unless `LOCUS_PIPEWIRE_DAEMON_TEST=1`.
    use super::*;
    use std::process::{Child, Command, Stdio};

    fn enabled() -> bool {
        let on = std::env::var("LOCUS_PIPEWIRE_DAEMON_TEST").ok().as_deref() == Some("1");
        if !on {
            eprintln!("skipped: set LOCUS_PIPEWIRE_DAEMON_TEST=1 with a running PipeWire");
        }
        on
    }

    struct Producer(Child);

    impl Drop for Producer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn node_id_of(name: &str) -> Option<u32> {
        let out = Command::new("pw-dump").output().ok()?;
        let dump: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
        dump.as_array()?
            .iter()
            .find_map(|object| {
                let props = object.get("info")?.get("props")?;
                (props.get("node.name")?.as_str()? == name).then(|| object.get("id")?.as_u64())?
            })
            .map(|id| id as u32)
    }

    /// A solid red picture of `format` at `width` x `height`, 30 fps.
    fn producer(name: &str, format: &str, width: u32, height: u32) -> (Producer, u32) {
        let child = Command::new("gst-launch-1.0")
            .args([
                "-q",
                "videotestsrc",
                "is-live=true",
                "pattern=solid-color",
                "foreground-color=0xffff0000",
                "!",
                &format!(
                    "video/x-raw,format={format},width={width},height={height},framerate=30/1"
                ),
                "!",
                "pipewiresink",
                &format!("stream-properties=props,node.name={name},media.class=Video/Source"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("gst-launch-1.0 with pipewiresink is installed");
        let producer = Producer(child);
        for _ in 0..50 {
            if let Some(id) = node_id_of(name) {
                return (producer, id);
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("producer node {name} never appeared");
    }

    fn start(
        node: u32,
    ) -> (
        LinuxScreenSource,
        mpsc::Receiver<VideoFrame>,
        Result<Instant, CaptureError>,
    ) {
        let (tx, rx) = mpsc::sync_channel(64);
        let mut source = LinuxScreenSource::new();
        let result = source.start_from(Origin::Node(node), Arc::new(MediaClock::new()), tx);
        (source, rx, result)
    }

    #[test]
    fn red_arrives_as_bgra_for_both_byte_orders() {
        if !enabled() {
            return;
        }
        for (n, format) in ["BGRx", "RGBx"].into_iter().enumerate() {
            let (_producer, node) = producer(&format!("locus-test-red-{n}"), format, 1280, 720);
            let (mut source, rx, started) = start(node);
            started.unwrap_or_else(|e| panic!("{format}: {e}"));
            let frame = rx.recv_timeout(Duration::from_secs(3)).expect("a frame");
            assert_eq!((frame.width, frame.height), (1280, 720), "{format}");
            assert_eq!(frame.pixel_format, PixelFormat::Bgra);
            let pixel = &frame.packed_bytes()[..3];
            // B, G, R of a red picture; a couple of levels of conversion slack.
            assert!(
                pixel[0] <= 3 && pixel[1] <= 3 && pixel[2] >= 252,
                "{format}: {pixel:?}"
            );
            source.stop();
        }
    }

    #[test]
    fn a_source_above_1080p_is_scaled_to_the_fixed_output_size() {
        if !enabled() {
            return;
        }
        let (_producer, node) = producer("locus-test-big", "BGRx", 2560, 1440);
        let (mut source, rx, started) = start(node);
        started.unwrap();
        let frame = rx.recv_timeout(Duration::from_secs(3)).expect("a frame");
        assert_eq!((frame.width, frame.height), (1920, 1080));
        assert_eq!(frame.packed_bytes().len(), 1920 * 1080 * 4);
        source.stop();
    }

    #[test]
    fn pause_resume_and_losing_the_source() {
        if !enabled() {
            return;
        }
        let (producer, node) = producer("locus-test-life", "BGRx", 640, 360);
        let (mut source, rx, started) = start(node);
        started.unwrap();
        // A steady stream with non-decreasing timestamps.
        let mut last = Duration::ZERO;
        for _ in 0..5 {
            let frame = rx
                .recv_timeout(Duration::from_secs(3))
                .expect("frames flow");
            assert!(frame.pts >= last);
            last = frame.pts;
        }
        source.pause();
        thread::sleep(Duration::from_millis(300));
        while rx.try_recv().is_ok() {}
        thread::sleep(Duration::from_millis(500));
        assert!(rx.try_recv().is_err(), "frames were forwarded while paused");
        source.resume().unwrap();
        rx.recv_timeout(Duration::from_secs(3))
            .expect("frames resume");
        assert!(source.failure().is_none());
        // The share ending is reported so the manager can keep the take.
        drop(producer);
        let mut reported = None;
        for _ in 0..50 {
            reported = source.failure();
            if reported.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let message = reported.expect("losing the source is reported");
        assert!(
            message.contains("ended") || message.contains("PipeWire"),
            "{message}"
        );
        source.stop();
    }
}
