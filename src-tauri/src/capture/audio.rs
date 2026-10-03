//! Native audio capture: records each selected source (system audio loopback,
//! microphone) to its own WAV file while publishing live dBFS levels, then
//! mixes the tracks into a single mono `audio.wav` that the UI can play back.

use super::CaptureError;
use crate::contracts::CaptureSource;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, SupportedStreamConfig};
use hound::{SampleFormat as WavFormat, WavReader, WavSpec, WavWriter};
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
};

/// Level reported for silence / no signal / paused.
pub const SILENCE_DBFS: f32 = -90.0;
pub const MIXED_FILE_NAME: &str = "audio.wav";

/// RMS level of a block of normalised (-1.0..1.0) samples, in dBFS.
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return SILENCE_DBFS;
    }
    let mean_square = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    let rms = mean_square.sqrt();
    if rms <= 1e-5 {
        SILENCE_DBFS
    } else {
        (20.0 * rms.log10()).clamp(SILENCE_DBFS, 0.0)
    }
}

type SharedWriter = Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>;

/// Audio-callback side of a track: writes samples and publishes the level.
#[derive(Clone)]
struct Sink {
    writer: SharedWriter,
    level: Arc<AtomicU32>,
    paused: Arc<AtomicBool>,
}

impl Sink {
    fn push(&self, samples: &[f32]) {
        if self.paused.load(Ordering::Relaxed) {
            self.level.store(SILENCE_DBFS.to_bits(), Ordering::Relaxed);
            return;
        }
        self.level
            .store(rms_dbfs(samples).to_bits(), Ordering::Relaxed);
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(writer) = guard.as_mut() {
                for sample in samples {
                    let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    if writer.write_sample(value).is_err() {
                        break;
                    }
                }
            }
        }
    }
}

struct Track {
    source: CaptureSource,
    path: PathBuf,
    level: Arc<AtomicU32>,
    stop: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

pub struct AudioCapture {
    dir: PathBuf,
    tracks: Vec<Track>,
    paused: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct FinishedAudio {
    pub mixed_path: PathBuf,
    pub duration_seconds: f64,
}

fn unavailable(message: impl Into<String>) -> CaptureError {
    CaptureError::SourceUnavailable(message.into())
}

fn pick_device(source: &CaptureSource) -> Result<(cpal::Device, SupportedStreamConfig), String> {
    let host = cpal::default_host();
    match source {
        CaptureSource::Microphone => {
            let device = host
                .default_input_device()
                .ok_or("no microphone input device was found")?;
            let config = device
                .default_input_config()
                .map_err(|e| format!("microphone configuration failed: {e}"))?;
            Ok((device, config))
        }
        CaptureSource::SystemAudio => {
            #[cfg(target_os = "linux")]
            {
                // PulseAudio/PipeWire expose "what you hear" as a *monitor* input.
                let devices = host
                    .input_devices()
                    .map_err(|e| format!("cannot list audio devices: {e}"))?;
                for device in devices {
                    let is_monitor = device
                        .description()
                        .map(|d| d.name().to_lowercase().contains("monitor"))
                        .unwrap_or(false);
                    if is_monitor {
                        let config = device
                            .default_input_config()
                            .map_err(|e| format!("system audio configuration failed: {e}"))?;
                        return Ok((device, config));
                    }
                }
                Err("no system-audio monitor source was found (PipeWire/PulseAudio)".into())
            }
            #[cfg(not(target_os = "linux"))]
            {
                // WASAPI loopback: an input stream opened on the default *output* device.
                let device = host
                    .default_output_device()
                    .ok_or("no default output device to capture system audio from")?;
                let config = device
                    .default_output_config()
                    .map_err(|e| format!("system audio configuration failed: {e}"))?;
                Ok((device, config))
            }
        }
        CaptureSource::Screen => Err("screen is not an audio source".into()),
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &SupportedStreamConfig,
    sink: Sink,
) -> Result<Stream, String> {
    let stream_config = config.config();
    let on_error = |error| eprintln!("locus: audio stream error: {error}");
    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            stream_config,
            move |data: &[f32], _| sink.push(data),
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            stream_config,
            move |data: &[i16], _| {
                let converted: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                sink.push(&converted);
            },
            on_error,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            stream_config,
            move |data: &[u16], _| {
                let converted: Vec<f32> = data
                    .iter()
                    .map(|s| (*s as f32 - 32768.0) / 32768.0)
                    .collect();
                sink.push(&converted);
            },
            on_error,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            stream_config,
            move |data: &[i32], _| {
                let converted: Vec<f32> =
                    data.iter().map(|s| *s as f32 / 2_147_483_648.0).collect();
                sink.push(&converted);
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported audio sample format: {other:?}")),
    };
    stream.map_err(|e| format!("could not open audio stream: {e}"))
}

fn file_name(source: &CaptureSource) -> &'static str {
    match source {
        CaptureSource::SystemAudio => "system_audio.wav",
        CaptureSource::Microphone => "microphone.wav",
        CaptureSource::Screen => "screen.wav",
    }
}

impl AudioCapture {
    /// Opens every selected audio source. Fails (and tears down anything
    /// already opened) if any selected source cannot be captured, so the UI
    /// never shows "recording" for a source that captures nothing.
    pub fn start(dir: &Path, sources: &[CaptureSource]) -> Result<Self, CaptureError> {
        fs::create_dir_all(dir)
            .map_err(|e| unavailable(format!("cannot create media folder: {e}")))?;
        let paused = Arc::new(AtomicBool::new(false));
        let mut capture = AudioCapture {
            dir: dir.to_path_buf(),
            tracks: vec![],
            paused: Arc::clone(&paused),
        };
        for source in sources {
            if !matches!(
                source,
                CaptureSource::SystemAudio | CaptureSource::Microphone
            ) {
                continue;
            }
            match Self::start_track(dir, source, Arc::clone(&paused)) {
                Ok(track) => capture.tracks.push(track),
                Err(message) => {
                    let label = if *source == CaptureSource::Microphone {
                        "Microphone"
                    } else {
                        "System audio"
                    };
                    capture.abort();
                    return Err(unavailable(format!("{label}: {message}")));
                }
            }
        }
        Ok(capture)
    }

    fn start_track(
        dir: &Path,
        source: &CaptureSource,
        paused: Arc<AtomicBool>,
    ) -> Result<Track, String> {
        let path = dir.join(file_name(source));
        let level = Arc::new(AtomicU32::new(SILENCE_DBFS.to_bits()));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let thread_path = path.clone();
        let thread_level = Arc::clone(&level);
        let thread_source = source.clone();

        // cpal streams are not `Send`, so each one lives on its own thread.
        let handle = std::thread::Builder::new()
            .name(format!("locus-capture-{}", file_name(source)))
            .spawn(move || {
                let setup = (|| -> Result<(Stream, SharedWriter), String> {
                    let (device, config) = pick_device(&thread_source)?;
                    let spec = WavSpec {
                        channels: config.channels(),
                        sample_rate: config.sample_rate(),
                        bits_per_sample: 16,
                        sample_format: WavFormat::Int,
                    };
                    let writer = WavWriter::create(&thread_path, spec)
                        .map_err(|e| format!("cannot create recording file: {e}"))?;
                    let writer: SharedWriter = Arc::new(Mutex::new(Some(writer)));
                    let sink = Sink {
                        writer: Arc::clone(&writer),
                        level: thread_level,
                        paused,
                    };
                    let stream = build_stream(&device, &config, sink)?;
                    stream
                        .play()
                        .map_err(|e| format!("could not start audio stream: {e}"))?;
                    Ok((stream, writer))
                })();
                match setup {
                    Err(message) => {
                        let _ = ready_tx.send(Err(message));
                    }
                    Ok((stream, writer)) => {
                        let _ = ready_tx.send(Ok(()));
                        let _ = stop_rx.recv(); // block until stop (or sender dropped)
                        drop(stream);
                        if let Ok(mut guard) = writer.lock() {
                            if let Some(writer) = guard.take() {
                                let _ = writer.finalize();
                            }
                        }
                    }
                }
            })
            .map_err(|e| format!("cannot start capture thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Track {
                source: source.clone(),
                path,
                level,
                stop: stop_tx,
                handle: Some(handle),
            }),
            Ok(Err(message)) => {
                let _ = handle.join();
                let _ = fs::remove_file(&path);
                Err(message)
            }
            Err(_) => Err("capture thread exited unexpectedly".into()),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Latest dBFS reading for a source, if that source is being captured.
    pub fn level(&self, source: &CaptureSource) -> Option<f32> {
        self.tracks
            .iter()
            .find(|track| &track.source == source)
            .map(|track| f32::from_bits(track.level.load(Ordering::Relaxed)))
    }

    /// Stops and deletes everything recorded so far (failed start).
    pub fn discard(mut self) {
        self.abort();
    }

    fn abort(&mut self) {
        for track in self.tracks.drain(..) {
            let _ = track.stop.send(());
            if let Some(handle) = track.handle {
                let _ = handle.join();
            }
            let _ = fs::remove_file(track.path);
        }
    }

    /// Stops all tracks, flushes their files and mixes them into `audio.wav`.
    pub fn finish(mut self) -> Result<FinishedAudio, CaptureError> {
        let mut paths = vec![];
        for track in self.tracks.drain(..) {
            let _ = track.stop.send(());
            if let Some(handle) = track.handle {
                let _ = handle.join();
            }
            paths.push(track.path);
        }
        let mixed_path = self.dir.join(MIXED_FILE_NAME);
        let duration_seconds = mix_tracks(&paths, &mixed_path)
            .map_err(|e| unavailable(format!("could not finalise recording: {e}")))?;
        Ok(FinishedAudio {
            mixed_path,
            duration_seconds,
        })
    }
}

/// Streams a WAV file as mono f32 frames resampled to a target rate.
struct MonoResampler {
    reader: WavReader<BufReader<File>>,
    channels: usize,
    step: f64,
    position: f64,
    index: u64,
    prev: f32,
    next: f32,
    exhausted: bool,
    started: bool,
}

impl MonoResampler {
    fn open(path: &Path, target_rate: u32) -> Result<Self, String> {
        let reader = WavReader::open(path).map_err(|e| e.to_string())?;
        let spec = reader.spec();
        Ok(Self {
            channels: spec.channels.max(1) as usize,
            step: spec.sample_rate as f64 / target_rate as f64,
            reader,
            position: 0.0,
            index: 0,
            prev: 0.0,
            next: 0.0,
            exhausted: false,
            started: false,
        })
    }

    fn read_frame(&mut self) -> Option<f32> {
        let mut sum = 0.0;
        let mut samples = self.reader.samples::<i16>();
        for _ in 0..self.channels {
            sum += samples.next()?.ok()? as f32 / 32768.0;
        }
        Some(sum / self.channels as f32)
    }

    fn next_sample(&mut self) -> Option<f32> {
        if !self.started {
            self.started = true;
            self.prev = self.read_frame()?;
            match self.read_frame() {
                Some(frame) => self.next = frame,
                None => {
                    self.next = self.prev;
                    self.exhausted = true;
                }
            }
        }
        while self.position >= (self.index + 1) as f64 {
            if self.exhausted {
                return None;
            }
            self.prev = self.next;
            self.index += 1;
            match self.read_frame() {
                Some(frame) => self.next = frame,
                None => self.exhausted = true,
            }
        }
        let fraction = (self.position - self.index as f64) as f32;
        self.position += self.step;
        Some(self.prev + (self.next - self.prev) * fraction)
    }
}

/// Mixes the given WAV tracks into one mono 16-bit file; returns its duration.
pub fn mix_tracks(inputs: &[PathBuf], output: &Path) -> Result<f64, String> {
    let mut target_rate = 0;
    for path in inputs {
        let spec = WavReader::open(path).map_err(|e| e.to_string())?.spec();
        target_rate = target_rate.max(spec.sample_rate);
    }
    if target_rate == 0 {
        target_rate = 48_000;
    }
    let mut sources = inputs
        .iter()
        .map(|path| MonoResampler::open(path, target_rate))
        .collect::<Result<Vec<_>, _>>()?;
    let mut writer = WavWriter::create(
        output,
        WavSpec {
            channels: 1,
            sample_rate: target_rate,
            bits_per_sample: 16,
            sample_format: WavFormat::Int,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut frames: u64 = 0;
    loop {
        let mut mixed = 0.0;
        let mut any = false;
        for source in sources.iter_mut() {
            if let Some(sample) = source.next_sample() {
                mixed += sample;
                any = true;
            }
        }
        if !any {
            break;
        }
        let value = (mixed.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(value).map_err(|e| e.to_string())?;
        frames += 1;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(frames as f64 / target_rate as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tone(path: &Path, rate: u32, channels: u16, seconds: f32, amplitude: f32) {
        let mut writer = WavWriter::create(
            path,
            WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: WavFormat::Int,
            },
        )
        .unwrap();
        for n in 0..(rate as f32 * seconds) as usize {
            let value = ((n as f32 * 0.05).sin() * amplitude * i16::MAX as f32) as i16;
            for _ in 0..channels {
                writer.write_sample(value).unwrap();
            }
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn dbfs_levels_track_signal_strength() {
        assert_eq!(rms_dbfs(&[]), SILENCE_DBFS);
        assert_eq!(rms_dbfs(&[0.0; 64]), SILENCE_DBFS);
        let full_scale = rms_dbfs(&[1.0; 64]);
        assert!(
            full_scale.abs() < 0.01,
            "full scale is 0 dBFS, got {full_scale}"
        );
        let half = rms_dbfs(&[0.5; 64]);
        assert!(
            (half + 6.02).abs() < 0.05,
            "0.5 amplitude is about -6 dBFS, got {half}"
        );
        assert!(rms_dbfs(&[0.01; 64]) < half);
    }

    #[test]
    fn mixes_tracks_with_different_rates_and_channels() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, out) = (
            dir.path().join("a.wav"),
            dir.path().join("b.wav"),
            dir.path().join("audio.wav"),
        );
        write_tone(&a, 48_000, 2, 1.0, 0.3);
        write_tone(&b, 16_000, 1, 0.5, 0.3);
        let duration = mix_tracks(&[a, b], &out).unwrap();
        assert!(
            (duration - 1.0).abs() < 0.01,
            "longest track sets duration: {duration}"
        );
        let reader = WavReader::open(&out).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert!(reader.duration() > 47_000);
    }

    #[test]
    fn mixing_a_single_track_preserves_length() {
        let dir = tempfile::tempdir().unwrap();
        let (a, out) = (dir.path().join("mic.wav"), dir.path().join("audio.wav"));
        write_tone(&a, 44_100, 1, 0.25, 0.5);
        let duration = mix_tracks(&[a], &out).unwrap();
        assert!((duration - 0.25).abs() < 0.01);
    }
}
