//! Audio access for the post-recording pipeline.
//!
//! Recordings can run for hours, so nothing here loads a whole file into
//! memory: the energy envelope is computed in one streaming pass and chunks are
//! decoded on demand by seeking to the requested time range.

use crate::transcription::SpeechRegion;
use hound::{SampleFormat, WavReader};
use std::{fs::File, io::BufReader, path::Path};

/// Whisper's required input rate.
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;
/// Frames per second of the energy envelope handed to the chunker.
pub const ENVELOPE_FPS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub bits_per_sample: u16,
    pub sample_format: SampleFormat,
}

impl WavInfo {
    pub fn duration_seconds(&self) -> f32 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames as f32 / self.sample_rate as f32
        }
    }
}

fn open(path: &Path) -> Result<WavReader<BufReader<File>>, String> {
    WavReader::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))
}

pub fn wav_info(path: &Path) -> Result<WavInfo, String> {
    let reader = open(path)?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(format!("{} has an invalid WAV header", path.display()));
    }
    Ok(WavInfo {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        frames: u64::from(reader.duration()),
        bits_per_sample: spec.bits_per_sample,
        sample_format: spec.sample_format,
    })
}

/// Reads up to `max_frames` interleaved frames starting at `start_frame`,
/// downmixed to mono and scaled to [-1, 1].
fn read_mono_frames(
    reader: &mut WavReader<BufReader<File>>,
    start_frame: u64,
    max_frames: u64,
) -> Result<Vec<f32>, String> {
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    let total = u64::from(reader.duration());
    if start_frame >= total {
        return Ok(Vec::new());
    }
    let frames = max_frames.min(total - start_frame);
    reader
        .seek(start_frame as u32)
        .map_err(|error| format!("cannot seek in WAV: {error}"))?;
    let wanted = frames as usize * channels;
    let mut interleaved: Vec<f32> = Vec::with_capacity(wanted);
    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Float, _) => {
            for sample in reader.samples::<f32>().take(wanted) {
                interleaved.push(sample.map_err(|e| e.to_string())?);
            }
        }
        (SampleFormat::Int, bits) if bits <= 16 => {
            for sample in reader.samples::<i16>().take(wanted) {
                interleaved.push(f32::from(sample.map_err(|e| e.to_string())?) / 32_768.0);
            }
        }
        (SampleFormat::Int, bits) => {
            let scale = (1_u64 << (bits - 1)) as f32;
            for sample in reader.samples::<i32>().take(wanted) {
                interleaved.push(sample.map_err(|e| e.to_string())? as f32 / scale);
            }
        }
    }
    if channels == 1 {
        return Ok(interleaved);
    }
    Ok(interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect())
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Windowed-sinc resampler. Down-sampling low-passes just below the new
/// Nyquist first, so music, crowd noise and hiss above 8 kHz cannot fold back
/// into the speech band (a plain average does exactly that). The filter is
/// precomputed per output phase, so cost is a few dozen multiply-adds per
/// output sample even for hours of audio.
pub fn resample_mono(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }
    const ZERO_CROSSINGS: f64 = 10.0;
    let ratio = f64::from(from_rate) / f64::from(to_rate);
    // cutoff in cycles per input sample
    let cutoff = 0.5 * 0.94 / ratio.max(1.0);
    let half = ZERO_CROSSINGS / (2.0 * cutoff);
    let taps = (2.0 * half).ceil() as usize + 2;
    let divisor = gcd(u64::from(from_rate), u64::from(to_rate));
    let period = (u64::from(to_rate) / divisor) as usize;
    let step = (u64::from(from_rate) / divisor) as i64;

    let kernel = |x: f64| -> f64 {
        if x.abs() >= half {
            return 0.0;
        }
        let arg = 2.0 * cutoff * x;
        let sinc = if arg.abs() < 1e-12 {
            1.0
        } else {
            (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
        };
        let window = 0.5 * (1.0 + (std::f64::consts::PI * x / half).cos());
        2.0 * cutoff * sinc * window
    };

    // (first input index relative to the period start, weights) per phase
    let phases: Vec<(i64, Vec<f32>)> = (0..period)
        .map(|phase| {
            let position = (phase as f64 + 0.5) * ratio - 0.5;
            let first = (position - half).floor() as i64 + 1;
            let weights = (0..taps)
                .map(|tap| kernel((first + tap as i64) as f64 - position) as f32)
                .collect();
            (first, weights)
        })
        .collect();

    let output_len = (input.len() as f64 / ratio).floor() as usize;
    let mut output = Vec::with_capacity(output_len);
    for index in 0..output_len {
        let (first, weights) = &phases[index % period];
        let base = (index / period) as i64 * step + first;
        let mut accumulated = 0.0_f32;
        let mut weight_sum = 0.0_f32;
        for (tap, weight) in weights.iter().enumerate() {
            let source = base + tap as i64;
            if source >= 0 && (source as usize) < input.len() {
                accumulated += weight * input[source as usize];
                weight_sum += weight;
            }
        }
        // renormalizing keeps unity gain at the file edges where taps fall off
        output.push(if weight_sum.abs() > 1e-6 {
            accumulated / weight_sum
        } else {
            0.0
        });
    }
    output
}

/// Decodes `[start_seconds, end_seconds)` as 16 kHz mono f32 for Whisper.
pub fn read_range_16k(path: &Path, start_seconds: f32, end_seconds: f32) -> Result<Vec<f32>, String> {
    let mut reader = open(path)?;
    let spec = reader.spec();
    let start_frame = (start_seconds.max(0.0) as f64 * f64::from(spec.sample_rate)) as u64;
    let end_frame = (end_seconds.max(start_seconds) as f64 * f64::from(spec.sample_rate)) as u64;
    let mono = read_mono_frames(&mut reader, start_frame, end_frame.saturating_sub(start_frame))?;
    Ok(resample_mono(&mono, spec.sample_rate, WHISPER_SAMPLE_RATE))
}

/// One streaming pass producing an RMS envelope at [`ENVELOPE_FPS`].
pub fn energy_envelope(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = open(path)?;
    let spec = reader.spec();
    let frame_len = (spec.sample_rate as usize / ENVELOPE_FPS).max(1);
    let total = u64::from(reader.duration());
    let block_frames = (frame_len * ENVELOPE_FPS * 30) as u64; // ~30 s per read
    let mut envelope = Vec::with_capacity((total as usize / frame_len) + 1);
    let mut position = 0_u64;
    let mut carry: Vec<f32> = Vec::new();
    while position < total {
        let block = read_mono_frames(&mut reader, position, block_frames)?;
        if block.is_empty() {
            break;
        }
        position += block.len() as u64;
        carry.extend_from_slice(&block);
        let usable = (carry.len() / frame_len) * frame_len;
        for window in carry[..usable].chunks_exact(frame_len) {
            let energy = window.iter().map(|s| s * s).sum::<f32>() / frame_len as f32;
            envelope.push(energy.sqrt());
        }
        carry.drain(..usable);
    }
    if !carry.is_empty() {
        let energy = carry.iter().map(|s| s * s).sum::<f32>() / carry.len() as f32;
        envelope.push(energy.sqrt());
    }
    Ok(envelope)
}

/// Fallback voice-activity detection used when the Python sidecar (and its
/// pyannote model) is unavailable. It is deliberately conservative: it only
/// has to keep long silences away from Whisper, which hallucinates on them.
pub fn energy_speech_regions(envelope: &[f32], fps: usize) -> Vec<SpeechRegion> {
    if envelope.is_empty() || fps == 0 {
        return Vec::new();
    }
    let mut sorted = envelope.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let percentile = |p: f32| sorted[((sorted.len() - 1) as f32 * p) as usize];
    let noise = percentile(0.10);
    let loud = percentile(0.95);
    if loud < 0.004 {
        return Vec::new(); // effectively silent
    }
    let threshold = (noise * 3.0).max(loud * 0.10).max(0.004);
    let hangover = (fps as f32 * 0.5) as usize; // bridge short pauses
    let min_frames = (fps as f32 * 0.3) as usize;
    let pad = 0.15_f32;
    let duration = envelope.len() as f32 / fps as f32;

    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    let mut last_loud = 0_usize;
    for (index, value) in envelope.iter().enumerate() {
        if *value >= threshold {
            if start.is_none() {
                start = Some(index);
            }
            last_loud = index;
        } else if let Some(begin) = start {
            if index - last_loud > hangover {
                regions.push((begin, last_loud + 1));
                start = None;
            }
        }
    }
    if let Some(begin) = start {
        regions.push((begin, last_loud + 1));
    }
    regions
        .into_iter()
        .filter(|(begin, end)| end - begin >= min_frames)
        .map(|(begin, end)| SpeechRegion {
            start_seconds: (begin as f32 / fps as f32 - pad).max(0.0),
            end_seconds: (end as f32 / fps as f32 + pad).min(duration),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};

    fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
        let mut writer = WavWriter::create(
            path,
            WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
        )
        .unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn tone(rate: u32, seconds: f32, amplitude: f32) -> Vec<i16> {
        (0..(rate as f32 * seconds) as usize)
            .map(|n| ((n as f32 * 0.3).sin() * amplitude * 32_767.0) as i16)
            .collect()
    }

    #[test]
    fn resamples_48k_to_16k_with_expected_length() {
        let input = vec![0.5_f32; 48_000];
        let output = resample_mono(&input, 48_000, 16_000);
        assert_eq!(output.len(), 16_000);
        assert!(output.iter().all(|v| (v - 0.5).abs() < 1e-4));
    }

    fn sine(rate: u32, freq: f32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|n| (2.0 * std::f32::consts::PI * freq * n as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn resampling_keeps_speech_band_energy_and_rejects_what_would_alias() {
        // 44.1 kHz is the awkward real-world case (160 phases)
        for rate in [48_000_u32, 44_100] {
            let seconds = 2.0;
            let trim = |v: Vec<f32>| v[1_000..v.len() - 1_000].to_vec();
            // in-band: 1 kHz and 6 kHz come through at (nearly) full level
            for freq in [1_000.0_f32, 6_000.0] {
                let out = trim(resample_mono(&sine(rate, freq, seconds), rate, 16_000));
                let level = rms(&out) / (1.0 / 2.0_f32.sqrt());
                assert!(level > 0.93 && level < 1.05, "{rate} Hz {freq}: {level}");
            }
            // out-of-band: a 12 kHz / 20 kHz tone would fold into 4 kHz / 4 kHz
            for freq in [12_000.0_f32, 20_000.0] {
                let out = trim(resample_mono(&sine(rate, freq, seconds), rate, 16_000));
                assert!(rms(&out) < 0.02, "{rate} Hz {freq}: aliased at {}", rms(&out));
            }
        }
    }

    #[test]
    fn upsampling_and_odd_ratios_keep_length_and_level() {
        let out = resample_mono(&vec![0.25_f32; 8_000], 8_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-3));
        let odd = resample_mono(&vec![0.5_f32; 22_050], 22_050, 16_000);
        assert_eq!(odd.len(), 16_000);
        assert!(odd.iter().all(|v| (v - 0.5).abs() < 1e-3));
    }

    #[test]
    fn reads_a_time_range_as_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        write_wav(&path, 48_000, 1, &tone(48_000, 4.0, 0.5));
        let info = wav_info(&path).unwrap();
        assert_eq!(info.sample_rate, 48_000);
        assert!((info.duration_seconds() - 4.0).abs() < 0.01);
        let range = read_range_16k(&path, 1.0, 3.0).unwrap();
        assert_eq!(range.len(), 32_000);
        assert!(range.iter().any(|v| v.abs() > 0.1));
    }

    #[test]
    fn stereo_is_downmixed_and_past_end_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.wav");
        let mono = tone(16_000, 1.0, 0.4);
        let stereo: Vec<i16> = mono.iter().flat_map(|s| [*s, *s]).collect();
        write_wav(&path, 16_000, 2, &stereo);
        assert_eq!(read_range_16k(&path, 0.0, 1.0).unwrap().len(), 16_000);
        assert!(read_range_16k(&path, 5.0, 6.0).unwrap().is_empty());
    }

    #[test]
    fn envelope_and_energy_vad_find_the_spoken_middle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.wav");
        let mut samples = vec![0_i16; 16_000 * 3];
        samples.extend(tone(16_000, 3.0, 0.5));
        samples.extend(vec![0_i16; 16_000 * 3]);
        write_wav(&path, 16_000, 1, &samples);
        let envelope = energy_envelope(&path).unwrap();
        assert_eq!(envelope.len(), 9 * ENVELOPE_FPS);
        let regions = energy_speech_regions(&envelope, ENVELOPE_FPS);
        assert_eq!(regions.len(), 1);
        assert!((regions[0].start_seconds - 2.85).abs() < 0.2);
        assert!((regions[0].end_seconds - 6.15).abs() < 0.2);
    }

    #[test]
    fn silence_yields_no_regions() {
        assert!(energy_speech_regions(&vec![0.0; 500], ENVELOPE_FPS).is_empty());
        assert!(energy_speech_regions(&[], ENVELOPE_FPS).is_empty());
    }
}
