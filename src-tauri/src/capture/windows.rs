use super::{CaptureError, CaptureOptions};
use std::process::Command;

pub fn preflight(_options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "windows")]
    {
        if std::env::var("LOCUS_WINDOWS_CAPTURE_TEST").ok().as_deref() == Some("1") {
            return Ok(());
        }
        // The native worker supplies the WGC and WASAPI handles.  This probe
        // makes a missing Windows runtime visible instead of silently falling
        // back to an unselected capture source.
        if std::env::var("WINDIR").is_err() {
            return Err(CaptureError::SourceUnavailable(
                "Windows Graphics Capture requires a Windows session".into(),
            ));
        }
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("true");
        Err(CaptureError::SourceUnavailable(
            "Windows Graphics Capture is only available on Windows builds".into(),
        ))
    }
}
