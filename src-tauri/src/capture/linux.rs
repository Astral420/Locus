//! Linux capture: preflight and the pure logic behind the PipeWire adapters
//! (NATIVE_CAPTURE_PLAN NC-7). The OS-facing adapters are `linux_pipewire.rs`
//! (xdg-desktop-portal ScreenCast + PipeWire video) and `linux_audio.rs`
//! (PipeWire audio); both are Linux only. Everything here compiles and is
//! tested on every platform.

use super::{
    macos::cap_output_size, source::ScreenTarget, windows::scale_bgra, CaptureBackend,
    CaptureError, CaptureOptions,
};

/// Sample rate of every PipeWire audio track. The stream asks PipeWire's
/// adapter for mono F32 at this rate, so no resampling happens in Locus.
pub const AUDIO_RATE: u32 = 48_000;

/// True when `LOCUS_CAPTURE_BACKEND=native`: system audio and the microphone
/// then come from PipeWire streams instead of `cpal`'s ALSA host.
pub fn native_audio_selected() -> bool {
    matches!(CaptureBackend::from_env(), Ok(CaptureBackend::Native))
}

/// SPEC l.140: Wayland only, no X11 fallback.
pub fn session_is_x11(session_type: Option<&str>) -> bool {
    session_type.is_some_and(|value| value.trim().eq_ignore_ascii_case("x11"))
}

/// What the portal's own picker may offer. On Linux the compositor shows the
/// screen/window chooser (the app cannot enumerate sources on Wayland), so a
/// `ScreenTarget` only narrows what the dialog lists; its id is not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalSources {
    Monitor,
    Window,
    MonitorOrWindow,
}

pub fn portal_sources(target: &ScreenTarget) -> PortalSources {
    match target {
        ScreenTarget::PrimaryDisplay => PortalSources::MonitorOrWindow,
        ScreenTarget::Display(_) => PortalSources::Monitor,
        ScreenTarget::Window(_) => PortalSources::Window,
    }
}

/// Byte order of the 32-bit RGB formats compositors offer for screen casts
/// (the fourth byte is alpha or padding and is ignored by the encoder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawLayout {
    /// B, G, R, x — the usual format (SPA `BGRx`, `BGRA`).
    Bgr,
    /// R, G, B, x (SPA `RGBx`, `RGBA`).
    Rgb,
}

/// Converts one PipeWire frame to the tightly packed BGRA at the recording's
/// fixed output size that the encoder reads, scaling when the source differs
/// (a window resized mid-recording is scaled, never re-sized: SPEC says never
/// silently downgrade a selected source). `None` if the buffer is too short.
pub fn to_output_bgra(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    stride: usize,
    layout: RawLayout,
    dst_w: usize,
    dst_h: usize,
) -> Option<Vec<u8>> {
    match layout {
        RawLayout::Bgr => scale_bgra(src, src_w, src_h, stride, dst_w, dst_h),
        RawLayout::Rgb => {
            if src_w == 0 || src_h == 0 || stride < src_w * 4 {
                return None;
            }
            let needed = stride.checked_mul(src_h - 1)?.checked_add(src_w * 4)?;
            if src.len() < needed {
                return None;
            }
            let mut tight = Vec::with_capacity(src_w * src_h * 4);
            for row in 0..src_h {
                let line = &src[row * stride..row * stride + src_w * 4];
                for pixel in line.chunks_exact(4) {
                    tight.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
            }
            scale_bgra(&tight, src_w, src_h, src_w * 4, dst_w, dst_h)
        }
    }
}

/// The fixed output size for a source of `width` x `height`: fits 1080p, even
/// sides (plan D4 proposal), same rule as macOS and Windows.
pub fn output_size_for(width: u32, height: u32) -> (u32, u32) {
    cap_output_size(
        width,
        height,
        super::macos::DEFAULT_MAX_WIDTH,
        super::macos::DEFAULT_MAX_HEIGHT,
    )
}

/// Turns a portal or D-Bus failure into an actionable message.
pub fn describe_portal_failure(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("cancel") {
        return "Screen sharing was cancelled in the system dialog. Start the recording again \
                and choose a screen or window to share."
            .into();
    }
    if lower.contains("serviceunknown")
        || lower.contains("name has no owner")
        || lower.contains("not provided by any .service")
        || lower.contains("no such interface")
        || lower.contains("unknownmethod")
    {
        return "The desktop's screen-sharing portal is not available. Install \
                xdg-desktop-portal and the backend for your desktop (for example \
                xdg-desktop-portal-gnome, -kde or -wlr), then log in again."
            .into();
    }
    if lower.contains("permission") || lower.contains("not allowed") || lower.contains("denied") {
        return "The desktop denied screen sharing for Locus. Allow it in the system dialog or \
                your desktop's privacy settings, then try again."
            .into();
    }
    format!("Screen capture failed: {raw}")
}

/// Turns a PipeWire failure into an actionable message.
pub fn describe_pipewire_failure(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("no such file")
        || lower.contains("connection refused")
        || lower.contains("creation failed")
        || lower.contains("host is down")
    {
        return "PipeWire is not running. Start your desktop's PipeWire service \
                (for example `systemctl --user start pipewire`) and try again."
            .into();
    }
    format!("PipeWire error: {raw}")
}

pub fn preflight(options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "linux")]
    {
        use crate::contracts::CaptureSource;
        if std::env::var("LOCUS_PIPEWIRE_TEST").ok().as_deref() == Some("1") {
            return Ok(());
        }
        if session_is_x11(std::env::var("XDG_SESSION_TYPE").ok().as_deref()) {
            return Err(CaptureError::SourceUnavailable(
                "Locus requires a Wayland session; X11 capture is not supported".into(),
            ));
        }
        super::linux_pipewire::probe_pipewire().map_err(CaptureError::SourceUnavailable)?;
        if options.sources.contains(&CaptureSource::Screen) {
            super::linux_pipewire::probe_portal().map_err(CaptureError::SourceUnavailable)?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = options;
        Err(CaptureError::SourceUnavailable(
            "PipeWire capture is only available on Linux builds".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_x11_sessions_are_rejected() {
        assert!(session_is_x11(Some("x11")));
        assert!(session_is_x11(Some(" X11\n")));
        assert!(!session_is_x11(Some("wayland")));
        assert!(!session_is_x11(Some("tty")));
        assert!(!session_is_x11(None));
    }

    #[test]
    fn the_target_only_narrows_what_the_portal_dialog_lists() {
        assert_eq!(
            portal_sources(&ScreenTarget::PrimaryDisplay),
            PortalSources::MonitorOrWindow
        );
        assert_eq!(
            portal_sources(&ScreenTarget::Display(2)),
            PortalSources::Monitor
        );
        assert_eq!(
            portal_sources(&ScreenTarget::Window(7)),
            PortalSources::Window
        );
    }

    #[test]
    fn bgr_frames_pass_through_unchanged_at_the_same_size() {
        let src: Vec<u8> = (0..4 * 2 * 4).map(|v| v as u8).collect();
        let out = to_output_bgra(&src, 4, 2, 16, RawLayout::Bgr, 4, 2).unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn rgb_frames_are_swapped_to_bgr_and_row_padding_is_dropped() {
        // Two 2x1 rows with 4 bytes of padding each (stride 12).
        let src = vec![
            1, 2, 3, 255, 4, 5, 6, 255, 0, 0, 0, 0, //
            7, 8, 9, 255, 10, 11, 12, 255, 0, 0, 0, 0,
        ];
        let out = to_output_bgra(&src, 2, 2, 12, RawLayout::Rgb, 2, 2).unwrap();
        assert_eq!(
            out,
            vec![3, 2, 1, 255, 6, 5, 4, 255, 9, 8, 7, 255, 12, 11, 10, 255]
        );
    }

    #[test]
    fn a_source_larger_than_the_output_is_averaged_down() {
        let (w, h) = (8usize, 4usize);
        let src = vec![100u8; w * h * 4];
        let out = to_output_bgra(&src, w, h, w * 4, RawLayout::Bgr, 4, 2).unwrap();
        assert_eq!(out.len(), 4 * 2 * 4);
        assert!(out.iter().all(|&b| b == 100));
    }

    #[test]
    fn short_buffers_are_rejected_not_read_out_of_bounds() {
        let src = vec![0u8; 10];
        assert!(to_output_bgra(&src, 4, 2, 16, RawLayout::Bgr, 4, 2).is_none());
        assert!(to_output_bgra(&src, 4, 2, 16, RawLayout::Rgb, 4, 2).is_none());
        assert!(to_output_bgra(&src, 0, 2, 16, RawLayout::Rgb, 4, 2).is_none());
    }

    #[test]
    fn output_size_is_capped_to_1080p_and_even() {
        assert_eq!(output_size_for(3840, 2160), (1920, 1080));
        assert_eq!(output_size_for(1366, 768), (1366, 768));
        assert_eq!(output_size_for(1281, 721), (1280, 720));
    }

    #[test]
    fn portal_failures_get_actionable_wording() {
        assert!(describe_portal_failure("Response: Cancelled").contains("cancelled"));
        assert!(describe_portal_failure(
            "org.freedesktop.DBus.Error.ServiceUnknown: The name is not activatable"
        )
        .contains("xdg-desktop-portal"));
        assert!(describe_portal_failure("Permission denied").contains("denied"));
        assert!(describe_portal_failure("weird").starts_with("Screen capture failed"));
    }

    #[test]
    fn pipewire_failures_get_actionable_wording() {
        assert!(describe_pipewire_failure("No such file or directory").contains("not running"));
        assert!(describe_pipewire_failure("boom").starts_with("PipeWire error"));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn preflight_refuses_outside_linux() {
        use crate::contracts::{CaptureSource, MeetingType};
        let options = CaptureOptions {
            sources: vec![CaptureSource::Microphone],
            meeting_type: MeetingType::Auto,
            single_person_mic: false,
            title: String::new(),
            output_root: std::path::PathBuf::from("media"),
            screen_target: None,
        };
        assert!(preflight(&options).is_err());
    }
}
