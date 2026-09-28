use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelOrigin {
    Bundled,
    Imported,
    Downloaded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVariant {
    pub id: String,
    pub filename: String,
    pub quantization: Option<String>,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub catalog_id: String,
    pub display_name: String,
    pub role: String,
    pub variants: Vec<ModelVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAsset {
    pub id: String,
    pub catalog_id: String,
    pub artifact_revision: String,
    pub path: PathBuf,
    pub origin: ModelOrigin,
    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("model file is unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error("download failed: {0}")]
    Download(String),
    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("bundled models cannot be deleted")]
    BundledDelete,
}

pub struct ModelManager {
    bundled_dir: PathBuf,
    managed_dir: PathBuf,
    client: Client,
}

impl ModelManager {
    pub fn new(bundled_dir: impl Into<PathBuf>, managed_dir: impl Into<PathBuf>) -> Self {
        Self {
            bundled_dir: bundled_dir.into(),
            managed_dir: managed_dir.into(),
            client: Client::new(),
        }
    }

    pub fn catalog() -> Vec<ModelCatalogEntry> {
        vec![ModelCatalogEntry {
            catalog_id: "whisper-small".into(),
            display_name: "Whisper Small".into(),
            role: "transcription".into(),
            variants: [
                (
                    "standard",
                    "ggml-small.bin",
                    None,
                    466 * 1024 * 1024,
                    900 * 1024 * 1024,
                ),
                (
                    "q5_0",
                    "ggml-small-q5_0.bin",
                    Some("q5_0"),
                    181 * 1024 * 1024,
                    500 * 1024 * 1024,
                ),
                (
                    "q5_1",
                    "ggml-small-q5_1.bin",
                    Some("q5_1"),
                    190 * 1024 * 1024,
                    520 * 1024 * 1024,
                ),
                (
                    "q8_0",
                    "ggml-small-q8_0.bin",
                    Some("q8_0"),
                    261 * 1024 * 1024,
                    650 * 1024 * 1024,
                ),
            ]
            .into_iter()
            .map(
                |(id, filename, quantization, size_bytes, ram_bytes)| ModelVariant {
                    id: id.into(),
                    filename: filename.into(),
                    quantization: quantization.map(str::to_owned),
                    size_bytes,
                    ram_bytes,
                    sha256: None,
                },
            )
            .collect(),
        }]
    }

    pub fn import(
        &self,
        source: &Path,
        catalog_id: &str,
        revision: &str,
        expected_sha256: Option<&str>,
    ) -> Result<ModelAsset, ModelError> {
        fs::create_dir_all(&self.managed_dir)?;
        let destination = self
            .managed_dir
            .join(source.file_name().unwrap_or_default());
        fs::copy(source, &destination)?;
        self.finish_asset(
            destination,
            catalog_id,
            revision,
            ModelOrigin::Imported,
            expected_sha256,
        )
    }

    pub fn bundled(
        &self,
        filename: &str,
        catalog_id: &str,
        revision: &str,
    ) -> Result<ModelAsset, ModelError> {
        let path = self.bundled_dir.join(filename);
        self.finish_asset(path, catalog_id, revision, ModelOrigin::Bundled, None)
    }

    pub fn download(
        &self,
        url: &str,
        filename: &str,
        catalog_id: &str,
        revision: &str,
        expected_sha256: &str,
    ) -> Result<ModelAsset, ModelError> {
        fs::create_dir_all(&self.managed_dir)?;
        let destination = self.managed_dir.join(filename);
        let partial = destination.with_extension("part");
        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|e| ModelError::Download(e.to_string()))?;
        if !response.status().is_success() {
            return Err(ModelError::Download(response.status().to_string()));
        }
        let mut file = fs::File::create(&partial)?;
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = response.read(&mut buffer).map_err(ModelError::Io)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
        }
        file.flush()?;
        fs::rename(partial, &destination)?;
        self.finish_asset(
            destination,
            catalog_id,
            revision,
            ModelOrigin::Downloaded,
            Some(expected_sha256),
        )
    }

    pub fn select(&self, asset: &ModelAsset) -> Result<PathBuf, ModelError> {
        if asset.path.exists() {
            Ok(asset.path.clone())
        } else {
            Err(ModelError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "selected model is missing",
            )))
        }
    }
    pub fn delete(&self, asset: &ModelAsset) -> Result<(), ModelError> {
        if asset.origin == ModelOrigin::Bundled {
            return Err(ModelError::BundledDelete);
        }
        fs::remove_file(&asset.path).map_err(ModelError::Io)
    }

    fn finish_asset(
        &self,
        path: PathBuf,
        catalog_id: &str,
        revision: &str,
        origin: ModelOrigin,
        expected: Option<&str>,
    ) -> Result<ModelAsset, ModelError> {
        let actual = sha256(&path)?;
        if let Some(expected) = expected {
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(ModelError::Checksum {
                    expected: expected.into(),
                    actual,
                });
            }
        }
        Ok(ModelAsset {
            id: format!("{catalog_id}:{revision}"),
            catalog_id: catalog_id.into(),
            artifact_revision: revision.into(),
            path,
            origin,
            sha256: actual,
        })
    }
}

fn sha256(path: &Path) -> Result<String, ModelError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_verifies_integrity_and_catalog_lists_quantized_variants() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("model.bin");
        fs::write(&source, b"test-model").unwrap();
        let manager = ModelManager::new(root.path().join("bundled"), root.path().join("managed"));
        let asset = manager
            .import(&source, "whisper-small", "q5_0", None)
            .unwrap();
        assert_eq!(asset.origin, ModelOrigin::Imported);
        assert!(manager.select(&asset).unwrap().exists());
        assert!(ModelManager::catalog()[0]
            .variants
            .iter()
            .any(|variant| variant.quantization.as_deref() == Some("q8_0")));
    }
}
