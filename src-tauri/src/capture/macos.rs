use super::{CaptureError, CaptureOptions};

pub fn preflight(_options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "macos")]
    {
        Err(CaptureError::SourceUnavailable(
            "ScreenCaptureKit native capture bridge is not linked in this foundation build".into(),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(CaptureError::SourceUnavailable(
            "macOS ScreenCaptureKit is only available on macOS 13+".into(),
        ))
    }
}
