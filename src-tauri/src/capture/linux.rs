use super::{CaptureError, CaptureOptions};
use std::process::Command;

/// PipeWire-only Linux capture gate.  The actual stream negotiation stays in
/// the native capture worker; this boundary rejects accidental PulseAudio/X11
/// fallback and gives the worker a deterministic capability check.
pub fn preflight(_options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("LOCUS_PIPEWIRE_TEST").ok().as_deref() == Some("1") {
            return Ok(());
        }
        if std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("x11") {
            return Err(CaptureError::SourceUnavailable(
                "Locus requires a Wayland screen portal; X11 capture is not supported".into(),
            ));
        }
        let pipewire = Command::new("pw-cli")
            .arg("info")
            .arg("0")
            .output()
            .map_err(|error| {
                CaptureError::SourceUnavailable(format!("PipeWire is unavailable: {error}"))
            })?;
        if !pipewire.status.success() {
            return Err(CaptureError::SourceUnavailable(
                "PipeWire did not report a live core".into(),
            ));
        }
        let portal = Command::new("gdbus")
            .args([
                "introspect",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
            ])
            .output();
        if portal
            .as_ref()
            .map(|result| !result.status.success())
            .unwrap_or(true)
        {
            return Err(CaptureError::SourceUnavailable(
                "screen portal is unavailable; grant the desktop portal permission and try again"
                    .into(),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = Command::new("true");
        Err(CaptureError::SourceUnavailable(
            "PipeWire capture is only available on Linux builds".into(),
        ))
    }
}
