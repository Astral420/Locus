use super::{CaptureBackend, CaptureError, CaptureOptions};

/// System-audio capture uses a CoreAudio process tap (via cpal's loopback
/// support), which Apple added in macOS 14.2 and which cpal documents as
/// requiring 14.6 or later.
pub const MIN_LOOPBACK_VERSION: (u32, u32) = (14, 6);

/// Pure version check so it can be unit-tested on any platform.
pub fn version_supports_loopback(version: &str) -> bool {
    let mut parts = version
        .trim()
        .split('.')
        .map(|part| part.parse::<u32>().ok());
    let major = parts.next().flatten();
    let minor = parts.next().flatten().unwrap_or(0);
    match major {
        Some(major) => (major, minor) >= MIN_LOOPBACK_VERSION,
        None => false,
    }
}

/// ScreenCaptureKit video and system audio need macOS 13.0 (NFR10); the
/// CoreAudio-tap fallback needs 14.6 (`MIN_LOOPBACK_VERSION`).
pub const MIN_SCREENCAPTUREKIT_VERSION: (u32, u32) = (13, 0);

pub fn version_supports_screencapturekit(version: &str) -> bool {
    let mut parts = version
        .trim()
        .split('.')
        .map(|part| part.parse::<u32>().ok());
    match parts.next().flatten() {
        Some(major) => (major, parts.next().flatten().unwrap_or(0)) >= MIN_SCREENCAPTUREKIT_VERSION,
        None => false,
    }
}

/// Largest picture the native backend records by default (plan D4 proposal:
/// 1080p), configurable later. Retina displays are scaled down by ScreenCaptureKit
/// itself, which is far cheaper than piping full-resolution frames.
pub const DEFAULT_MAX_WIDTH: u32 = 1920;
pub const DEFAULT_MAX_HEIGHT: u32 = 1080;

/// Output size for a source of `width` x `height` points: never upscaled,
/// aspect ratio kept, fits inside `max_width` x `max_height`, both sides even
/// (H.264 4:2:0) and at least 2.
pub fn cap_output_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (2, 2);
    }
    let scale = f64::min(
        1.0,
        f64::min(
            f64::from(max_width) / f64::from(width),
            f64::from(max_height) / f64::from(height),
        ),
    );
    let even = |v: f64| ((v.floor() as u32) & !1).max(2);
    (
        even(f64::from(width) * scale),
        even(f64::from(height) * scale),
    )
}

/// Copies the two planes of a bi-planar 4:2:0 buffer (Y, then interleaved UV)
/// into one tightly packed NV12 frame. The planes may have row padding and
/// different strides; the output has none.
pub fn pack_nv12(
    luma: &[u8],
    luma_stride: usize,
    chroma: &[u8],
    chroma_stride: usize,
    width: usize,
    height: usize,
) -> Option<Vec<u8>> {
    let chroma_rows = height / 2;
    let luma_needed = luma_stride
        .checked_mul(height.checked_sub(1)?)?
        .checked_add(width)?;
    let chroma_needed = chroma_stride
        .checked_mul(chroma_rows.checked_sub(1)?)?
        .checked_add(width)?;
    if width == 0 || luma.len() < luma_needed || chroma.len() < chroma_needed {
        return None;
    }
    let mut out = Vec::with_capacity(width * height + width * chroma_rows);
    for row in 0..height {
        out.extend_from_slice(&luma[row * luma_stride..row * luma_stride + width]);
    }
    for row in 0..chroma_rows {
        out.extend_from_slice(&chroma[row * chroma_stride..row * chroma_stride + width]);
    }
    Some(out)
}

/// True when `LOCUS_CAPTURE_BACKEND=native`: system audio then comes from
/// ScreenCaptureKit (macOS 13+) instead of the CoreAudio tap (macOS 14.6+).
pub fn native_audio_selected() -> bool {
    matches!(CaptureBackend::from_env(), Ok(CaptureBackend::Native))
}

/// Little-endian f32 samples from raw audio bytes (a trailing partial sample
/// is ignored).
pub fn f32_samples_from_le_bytes(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// Mono samples from ScreenCaptureKit audio buffers. Each entry is
/// `(channels in that buffer, samples)`. SCK normally delivers one planar
/// buffer per channel (`channels == 1` each) but an interleaved buffer
/// (`channels > 1`) is handled too. Channels are averaged.
pub fn downmix_to_mono(buffers: &[(u32, Vec<f32>)]) -> Vec<f32> {
    match buffers {
        [] => Vec::new(),
        [(channels, samples)] if *channels > 1 => {
            let n = *channels as usize;
            samples
                .chunks_exact(n)
                .map(|frame| frame.iter().sum::<f32>() / n as f32)
                .collect()
        }
        [(_, samples)] => samples.clone(),
        many => {
            let len = many.iter().map(|(_, s)| s.len()).min().unwrap_or(0);
            (0..len)
                .map(|i| many.iter().map(|(_, s)| s[i]).sum::<f32>() / many.len() as f32)
                .collect()
        }
    }
}

/// Turns a ScreenCaptureKit failure into an actionable message. Denied
/// permission is the common case and macOS only applies a new grant after the
/// app restarts, so the message says so.
pub fn describe_capture_failure(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let denied = lower.contains("declined")
        || lower.contains("permission")
        || lower.contains("not authorized")
        || lower.contains("not permitted")
        || lower.contains("-3801");
    if denied {
        "Screen recording is not allowed for Locus. Open System Settings > Privacy & Security > \
         Screen & System Audio Recording, turn Locus on, then quit and reopen Locus."
            .to_string()
    } else {
        format!("Screen capture failed: {raw}")
    }
}

pub fn preflight(options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "macos")]
    {
        use crate::contracts::CaptureSource;
        let native = matches!(CaptureBackend::from_env(), Ok(CaptureBackend::Native));
        let needs_screencapturekit = native
            && options
                .sources
                .iter()
                .any(|s| matches!(s, CaptureSource::Screen | CaptureSource::SystemAudio));
        if !needs_screencapturekit && !options.sources.contains(&CaptureSource::SystemAudio) {
            return Ok(());
        }
        let version = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .unwrap_or_default();
        if native {
            // ScreenCaptureKit path: video and system audio from macOS 13.
            if !version_supports_screencapturekit(&version) {
                return Err(CaptureError::SourceUnavailable(format!(
                    "Screen and system audio capture need macOS 13 or later (this Mac reports {}).",
                    version.trim()
                )));
            }
            return Ok(());
        }
        if !version_supports_loopback(&version) {
            return Err(CaptureError::SourceUnavailable(format!(
                "System audio capture needs macOS 14.6 or later (this Mac reports {}). \
                 Update macOS, or record with the microphone only.",
                version.trim()
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (options, CaptureBackend::Ffmpeg);
        Err(CaptureError::SourceUnavailable(
            "macOS system audio capture is only available on macOS 14.6+".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_requires_macos_14_6() {
        assert!(!version_supports_loopback("13.6.1"));
        assert!(!version_supports_loopback("14.5"));
        assert!(version_supports_loopback("14.6"));
        assert!(version_supports_loopback("14.6.1"));
        assert!(version_supports_loopback("15.0"));
        assert!(version_supports_loopback("26.1"));
        assert!(!version_supports_loopback(""));
        assert!(!version_supports_loopback("garbage"));
    }

    #[test]
    fn screencapturekit_requires_macos_13() {
        assert!(!version_supports_screencapturekit("12.7"));
        assert!(version_supports_screencapturekit("13.0"));
        assert!(version_supports_screencapturekit("13.6.1"));
        assert!(version_supports_screencapturekit("14.2"));
        assert!(version_supports_screencapturekit("26.0"));
        assert!(!version_supports_screencapturekit(""));
        assert!(!version_supports_screencapturekit("garbage"));
    }

    #[test]
    fn output_size_is_capped_even_and_never_upscaled() {
        // Typical Retina point sizes against the 1080p cap.
        assert_eq!(cap_output_size(2560, 1440, 1920, 1080), (1920, 1080));
        assert_eq!(cap_output_size(1440, 900, 1920, 1080), (1440, 900));
        assert_eq!(cap_output_size(3840, 2160, 1920, 1080), (1920, 1080));
        // Portrait and ultrawide keep their shape.
        assert_eq!(cap_output_size(1080, 2400, 1920, 1080), (486, 1080));
        assert_eq!(cap_output_size(5120, 1440, 1920, 1080), (1920, 540));
        // Small windows are not enlarged; odd sizes become even.
        assert_eq!(cap_output_size(801, 601, 1920, 1080), (800, 600));
        assert_eq!(cap_output_size(0, 0, 1920, 1080), (2, 2));
        let (w, h) = cap_output_size(1921, 1081, 1920, 1080);
        assert!(w % 2 == 0 && h % 2 == 0 && w <= 1920 && h <= 1080);
    }

    #[test]
    fn nv12_planes_with_padding_are_packed_tightly() {
        // 4x4 picture: luma stride 6 (2 bytes padding), chroma stride 8.
        let mut luma = Vec::new();
        for row in 0..4u8 {
            luma.extend_from_slice(&[row; 4]);
            luma.extend_from_slice(&[0xEE; 2]);
        }
        let mut chroma = Vec::new();
        for row in 0..2u8 {
            chroma.extend_from_slice(&[10 + row; 4]);
            chroma.extend_from_slice(&[0xEE; 4]);
        }
        let packed = pack_nv12(&luma, 6, &chroma, 8, 4, 4).unwrap();
        assert_eq!(packed.len(), 4 * 4 + 4 * 2);
        assert_eq!(
            &packed[..16],
            &[0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3]
        );
        assert_eq!(&packed[16..], &[10, 10, 10, 10, 11, 11, 11, 11]);
    }

    #[test]
    fn short_planes_are_rejected_not_read_out_of_bounds() {
        assert!(pack_nv12(&[0; 10], 4, &[0; 8], 4, 4, 4).is_none());
        assert!(pack_nv12(&[0; 16], 4, &[0; 3], 4, 4, 4).is_none());
        assert!(pack_nv12(&[], 0, &[], 0, 0, 0).is_none());
    }

    #[test]
    fn audio_buffers_are_downmixed_to_mono() {
        // Planar stereo: one buffer per channel.
        let planar = vec![(1, vec![1.0, 0.0, -1.0]), (1, vec![0.0, 0.0, 1.0])];
        assert_eq!(downmix_to_mono(&planar), vec![0.5, 0.0, 0.0]);
        // Interleaved stereo in one buffer.
        assert_eq!(
            downmix_to_mono(&[(2, vec![1.0, 0.0, 0.5, 0.5])]),
            vec![0.5, 0.5]
        );
        // Mono passes through; nothing in, nothing out.
        assert_eq!(downmix_to_mono(&[(1, vec![0.25, 0.5])]), vec![0.25, 0.5]);
        assert!(downmix_to_mono(&[]).is_empty());
        // Unequal planar lengths use the shorter one.
        assert_eq!(
            downmix_to_mono(&[(1, vec![1.0, 1.0, 1.0]), (1, vec![1.0])]),
            vec![1.0]
        );
    }

    #[test]
    fn little_endian_bytes_become_samples() {
        let bytes: Vec<u8> = [0.5f32, -0.25]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert_eq!(f32_samples_from_le_bytes(&bytes), vec![0.5, -0.25]);
        assert_eq!(f32_samples_from_le_bytes(&[1, 2, 3]), Vec::<f32>::new());
    }

    #[test]
    fn permission_failures_get_an_actionable_message() {
        for raw in [
            "The user declined TCCs for application, window, display capture",
            "SCStreamErrorDomain code -3801",
            "permission denied",
        ] {
            let message = describe_capture_failure(raw);
            assert!(message.contains("System Settings"), "{message}");
            assert!(message.contains("reopen"), "{message}");
        }
        assert!(describe_capture_failure("display not found").starts_with("Screen capture failed"));
    }
}
