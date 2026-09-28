//! Stable-channel update manifest and package verification.
//!
//! The packaged Tauri updater supplies signed installation on release builds;
//! these functions keep the offline/manual path explicit and digest-checked.

use crate::contracts::UpdateCheckDto;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("update manifest request failed: {0}")]
    Request(String),
    #[error("update manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("update package checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("update package is not readable: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StableUpdateManifest {
    pub channel: String,
    pub version: String,
    pub package_url: String,
    pub sha256: String,
    /// Populated by the release pipeline. The platform updater verifies the
    /// detached signature before installation; manual packages require a
    /// matching digest and platform installer verification.
    pub signature: Option<String>,
}

pub fn check_stable_channel(
    client: &Client,
    endpoint: &str,
    current_version: &str,
) -> Result<UpdateCheckDto, UpdateError> {
    if endpoint.trim().is_empty() {
        return Ok(UpdateCheckDto {
            channel: "stable".into(),
            current_version: current_version.into(),
            available_version: None,
            package_url: None,
            sha256: None,
            verified: false,
        });
    }
    let response = client
        .get(endpoint)
        .send()
        .map_err(|error| UpdateError::Request(error.to_string()))?;
    if !response.status().is_success() {
        return Err(UpdateError::Request(response.status().to_string()));
    }
    let manifest: StableUpdateManifest = response
        .json()
        .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?;
    if manifest.channel != "stable"
        || manifest.version.trim().is_empty()
        || manifest.package_url.trim().is_empty()
        || manifest.sha256.len() != 64
    {
        return Err(UpdateError::InvalidManifest(
            "only stable manifests with a package URL and SHA-256 digest are accepted".into(),
        ));
    }
    Ok(UpdateCheckDto {
        channel: "stable".into(),
        current_version: current_version.into(),
        available_version: (manifest.version != current_version).then_some(manifest.version),
        package_url: Some(manifest.package_url),
        sha256: Some(manifest.sha256),
        verified: false,
    })
}

pub fn verify_package(path: impl AsRef<Path>, expected_sha256: &str) -> Result<(), UpdateError> {
    let bytes = fs::read(path).map_err(|error| UpdateError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(UpdateError::Checksum {
            expected: expected_sha256.into(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_package_verification_is_digest_checked() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("Locus.dmg");
        fs::write(&package, b"signed-package").unwrap();
        let mut hasher = Sha256::new();
        hasher.update(b"signed-package");
        let digest = format!("{:x}", hasher.finalize());
        verify_package(&package, &digest).unwrap();
        assert!(matches!(
            verify_package(&package, &"0".repeat(64)),
            Err(UpdateError::Checksum { .. })
        ));
    }
}
