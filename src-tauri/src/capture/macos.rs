use super::{CaptureError, CaptureOptions};

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

pub fn preflight(options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "macos")]
    {
        use crate::contracts::CaptureSource;
        if !options.sources.contains(&CaptureSource::SystemAudio) {
            return Ok(());
        }
        let version = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .unwrap_or_default();
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
        let _ = options;
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
}
