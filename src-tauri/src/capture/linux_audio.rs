//! Linux audio capture through PipeWire streams (NATIVE_CAPTURE_PLAN NC-7,
//! NFR12a, decision D5 recommendation).
//!
//! System audio is the monitor of the default output (`stream.capture.sink`);
//! the microphone is the default input. Both ask PipeWire's stream adapter for
//! mono 32-bit float at 48 kHz, so Locus does no resampling, and feed the same
//! per-source WAV writer, level meter and pause handling as the other
//! platforms. Compiled on Linux only.

use super::{
    linux::{describe_pipewire_failure, AUDIO_RATE},
    macos::{downmix_to_mono, f32_samples_from_le_bytes},
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::{
    param::{
        audio::{AudioFormat, AudioInfoRaw},
        format::{MediaSubtype, MediaType},
        format_utils,
    },
    pod::Pod,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// How long to wait for the first audio buffer before judging the stream.
const START_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioKind {
    /// What the computer is playing: the monitor of the default output.
    System,
    /// The default input device.
    Microphone,
}

impl AudioKind {
    fn label(self) -> &'static str {
        match self {
            Self::System => "system audio",
            Self::Microphone => "microphone",
        }
    }
}

struct Control;

/// A running capture; dropping it stops the stream.
pub struct PipeWireAudioStream {
    stop: Option<pw::channel::Sender<Control>>,
    handle: Option<JoinHandle<()>>,
}

impl PipeWireAudioStream {
    /// Starts capturing and returns when audio is flowing, with the instant the
    /// first sample was recorded (first buffer's arrival minus its length).
    /// `on_samples` receives mono f32 at 48 kHz. `on_failure` is called once if
    /// the stream breaks later (device removed, PipeWire restarted).
    pub fn start(
        kind: AudioKind,
        on_samples: impl FnMut(&[f32]) + Send + 'static,
        on_failure: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<(Self, Instant), String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<Instant, String>>(1);
        let (stop_tx, stop_rx) = pw::channel::channel::<Control>();
        let streaming_since = Arc::new(Mutex::new(None::<Instant>));
        let thread_streaming = Arc::clone(&streaming_since);
        let handle = thread::Builder::new()
            .name(format!("locus-pipewire-{}", kind.label().replace(' ', "-")))
            .spawn(move || {
                let tx = ready_tx.clone();
                if let Err(message) = run(
                    kind,
                    on_samples,
                    Arc::new(on_failure),
                    stop_rx,
                    ready_tx,
                    thread_streaming,
                ) {
                    let _ = tx.try_send(Err(message));
                }
            })
            .map_err(|e| format!("cannot start the audio thread: {e}"))?;
        let mut stream = Self {
            stop: Some(stop_tx),
            handle: Some(handle),
        };
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(started_at)) => Ok((stream, started_at)),
            Ok(Err(message)) => {
                stream.shutdown();
                Err(message)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // A silent graph may deliver no buffers yet; a stream that
                // reached "streaming" is still a live source.
                let since = streaming_since.lock().ok().and_then(|s| *s);
                match since {
                    Some(at) => Ok((stream, at)),
                    None => {
                        stream.shutdown();
                        Err(format!(
                            "PipeWire did not start the {} stream. Check that a default {} \
                             device exists and that PipeWire's session manager is running.",
                            kind.label(),
                            if kind == AudioKind::System {
                                "output"
                            } else {
                                "input"
                            }
                        ))
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stream.shutdown();
                Err(format!("the {} thread ended unexpectedly", kind.label()))
            }
        }
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(Control);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PipeWireAudioStream {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct AudioState<F: FnMut(&[f32])> {
    channels: u32,
    negotiated: bool,
    first_buffer_seen: bool,
    on_samples: F,
    ready: mpsc::SyncSender<Result<Instant, String>>,
    streaming_since: Arc<Mutex<Option<Instant>>>,
    on_failure: Arc<dyn Fn(String) + Send + Sync>,
    stopping: Arc<AtomicBool>,
    reported: bool,
    mainloop: pw::main_loop::MainLoopRc,
}

impl<F: FnMut(&[f32])> AudioState<F> {
    fn fail(&mut self, message: String) {
        // Before the first buffer `start` reports it; afterwards the manager's
        // watchdog reads it through the track's health.
        let _ = self.ready.try_send(Err(message.clone()));
        if !self.reported {
            self.reported = true;
            (self.on_failure)(message);
        }
        self.mainloop.quit();
    }
}

fn run<F: FnMut(&[f32]) + 'static>(
    kind: AudioKind,
    on_samples: F,
    on_failure: Arc<dyn Fn(String) + Send + Sync>,
    stop_rx: pw::channel::Receiver<Control>,
    ready: mpsc::SyncSender<Result<Instant, String>>,
    streaming_since: Arc<Mutex<Option<Instant>>>,
) -> Result<(), String> {
    let pw_err = |e: pw::Error| describe_pipewire_failure(&e.to_string());
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pw_err)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pw_err)?;
    let core = context.connect_rc(None).map_err(pw_err)?;

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Production",
        *pw::keys::APP_NAME => "Locus",
    };
    if kind == AudioKind::System {
        // Capture what the default output plays (its monitor ports).
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }
    let stream = pw::stream::StreamBox::new(
        &core,
        match kind {
            AudioKind::System => "locus-system-audio",
            AudioKind::Microphone => "locus-microphone",
        },
        props,
    )
    .map_err(pw_err)?;

    let stopping = Arc::new(AtomicBool::new(false));
    let _stop = stop_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let stopping = Arc::clone(&stopping);
        move |_: Control| {
            stopping.store(true, Ordering::SeqCst);
            mainloop.quit();
        }
    });

    let state = AudioState {
        channels: 1,
        negotiated: false,
        first_buffer_seen: false,
        on_samples,
        ready,
        streaming_since,
        on_failure,
        stopping: Arc::clone(&stopping),
        reported: false,
        mainloop: mainloop.clone(),
    };

    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(|_, state, old, new| {
            use pw::stream::StreamState;
            if state.stopping.load(Ordering::SeqCst) {
                return;
            }
            match new {
                StreamState::Streaming => {
                    if let Ok(mut since) = state.streaming_since.lock() {
                        since.get_or_insert_with(Instant::now);
                    }
                }
                StreamState::Error(message) => {
                    state.fail(describe_pipewire_failure(&message));
                }
                StreamState::Unconnected
                    if matches!(old, StreamState::Streaming | StreamState::Paused) =>
                {
                    state.fail(
                        "The audio device went away (PipeWire disconnected the stream).".into(),
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
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            let mut info = AudioInfoRaw::default();
            if info.parse(param).is_err() {
                return;
            }
            if info.rate() != AUDIO_RATE || info.format() != AudioFormat::F32LE {
                state.fail(format!(
                    "PipeWire negotiated {} Hz {:?} audio instead of {AUDIO_RATE} Hz F32.",
                    info.rate(),
                    info.format()
                ));
                return;
            }
            state.channels = info.channels().max(1);
            state.negotiated = true;
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let (size, offset) = (data.chunk().size() as usize, data.chunk().offset() as usize);
            if !state.negotiated || size == 0 {
                return;
            }
            let arrived = Instant::now();
            let Some(bytes) = data.data() else { return };
            let end = offset.saturating_add(size).min(bytes.len());
            let Some(slice) = bytes.get(offset..end) else {
                return;
            };
            let samples = f32_samples_from_le_bytes(slice);
            let mono = if state.channels > 1 {
                downmix_to_mono(&[(state.channels, samples)])
            } else {
                samples
            };
            if mono.is_empty() {
                return;
            }
            if !state.first_buffer_seen {
                state.first_buffer_seen = true;
                let length = Duration::from_secs_f64(mono.len() as f64 / f64::from(AUDIO_RATE));
                let started_at = arrived.checked_sub(length).unwrap_or(arrived);
                let _ = state.ready.try_send(Ok(started_at));
            }
            (state.on_samples)(&mono);
        })
        .register()
        .map_err(pw_err)?;

    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(AUDIO_RATE);
    info.set_channels(1);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map_err(|e| format!("cannot describe the audio format: {e:?}"))?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or("cannot build the audio format")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            // No RT_PROCESS: the callback writes to disk, so it runs on this
            // thread's loop, not on PipeWire's realtime data thread.
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(pw_err)?;

    mainloop.run();
    let _ = stream.disconnect();
    Ok(())
}

#[cfg(test)]
mod tests {
    //! These need a running PipeWire with a session manager and a default
    //! output and input (a desktop session, or a headless daemon with null
    //! devices). They are skipped unless `LOCUS_PIPEWIRE_DAEMON_TEST=1`.
    use super::*;
    use std::process::{Command, Stdio};

    fn enabled() -> bool {
        let on = std::env::var("LOCUS_PIPEWIRE_DAEMON_TEST").ok().as_deref() == Some("1");
        if !on {
            eprintln!("skipped: set LOCUS_PIPEWIRE_DAEMON_TEST=1 with a running PipeWire");
        }
        on
    }

    fn collector() -> (Arc<Mutex<Vec<f32>>>, impl FnMut(&[f32]) + Send + 'static) {
        let store = Arc::new(Mutex::new(Vec::<f32>::new()));
        let sink = Arc::clone(&store);
        (store, move |samples: &[f32]| {
            sink.lock().unwrap().extend_from_slice(samples)
        })
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn system_audio_hears_what_the_default_output_plays() {
        if !enabled() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tone = dir.path().join("tone.wav");
        let mut writer = hound::WavWriter::create(
            &tone,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for n in 0..96_000 {
            let v = (n as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
            writer
                .write_sample((v * f32::from(i16::MAX)) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();

        let (store, on_samples) = collector();
        let failed = Arc::new(Mutex::new(None::<String>));
        let failed_in = Arc::clone(&failed);
        let (stream, started_at) =
            PipeWireAudioStream::start(AudioKind::System, on_samples, move |m| {
                *failed_in.lock().unwrap() = Some(m)
            })
            .expect("system audio stream starts");
        assert!(started_at <= Instant::now());

        let mut player = Command::new("pw-cat")
            .args(["--playback", tone.to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("pw-cat is installed");
        thread::sleep(Duration::from_millis(2500));
        let _ = player.kill();
        let _ = player.wait();
        drop(stream);

        assert!(
            failed.lock().unwrap().is_none(),
            "{:?}",
            failed.lock().unwrap()
        );
        let samples = store.lock().unwrap().clone();
        assert!(samples.len() > 48_000, "only {} samples", samples.len());
        let loudest = samples.chunks(4_800).map(rms).fold(0.0_f32, f32::max);
        // A 0.5-amplitude sine has RMS about 0.35.
        assert!(
            loudest > 0.2,
            "tone not heard, loudest window RMS {loudest}"
        );
    }

    #[test]
    fn microphone_stream_starts_delivers_buffers_and_stops_cleanly() {
        if !enabled() {
            return;
        }
        let (store, on_samples) = collector();
        let (stream, _) = PipeWireAudioStream::start(AudioKind::Microphone, on_samples, |_| {})
            .expect("microphone stream starts");
        thread::sleep(Duration::from_millis(1500));
        drop(stream);
        assert!(store.lock().unwrap().len() > 24_000);
    }
}
