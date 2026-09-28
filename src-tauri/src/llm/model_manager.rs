use crate::db::Database;
use chrono::Utc;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerationModelCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerationModelAsset {
    pub id: String,
    pub catalog_id: String,
    pub artifact_revision: String,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadProgress {
    pub download_id: String,
    pub bytes_received: u64,
    pub bytes_total: Option<u64>,
    pub fraction: f32,
    pub state: String,
}

#[derive(Debug, Error)]
pub enum GenerationModelError {
    #[error("generation model file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("generation model download failed: {0}")]
    Download(String),
    #[error("generation model checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("generation model download canceled")]
    Canceled,
    #[error("generation model is unavailable: {0}")]
    Unavailable(String),
}

pub struct GenerationModelManager {
    managed_dir: PathBuf,
    client: Client,
    database: Option<Database>,
}

/// Managed GGUF embedding assets. Embedding weights intentionally have their
/// own manager and identity namespace: a generation model cannot be reused as
/// an embedding model without an explicit, verified asset.
pub struct EmbeddingModelManager {
    managed_dir: PathBuf,
    client: Client,
    database: Option<Database>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddingModelCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub dimension: usize,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddingModelAsset {
    pub id: String,
    pub catalog_id: String,
    pub artifact_revision: String,
    pub path: PathBuf,
    pub sha256: String,
    pub dimension: usize,
}

impl EmbeddingModelManager {
    pub fn new(managed_dir: impl Into<PathBuf>) -> Self {
        Self {
            managed_dir: managed_dir.into(),
            client: Client::new(),
            database: None,
        }
    }

    pub fn with_database(mut self, database: Database) -> Self {
        self.database = Some(database);
        self
    }

    pub fn catalog() -> Vec<EmbeddingModelCatalogEntry> {
        vec![EmbeddingModelCatalogEntry {
            id: "bge-small-en-v1.5".into(),
            display_name: "BGE Small English v1.5".into(),
            filename: "bge-small-en-v1.5.Q8_0.gguf".into(),
            size_bytes: 133 * 1024 * 1024,
            ram_bytes: 500 * 1024 * 1024,
            dimension: 384,
            sha256: None,
        }]
    }

    pub fn import(
        &self,
        source: &Path,
        catalog_id: &str,
        artifact_revision: &str,
        dimension: usize,
        expected_sha256: Option<&str>,
    ) -> Result<EmbeddingModelAsset, GenerationModelError> {
        if dimension == 0 {
            return Err(GenerationModelError::Unavailable(
                "embedding dimension must be positive".into(),
            ));
        }
        fs::create_dir_all(&self.managed_dir)?;
        let filename = source
            .file_name()
            .ok_or_else(|| GenerationModelError::Unavailable("source has no filename".into()))?;
        let destination = self.managed_dir.join(filename);
        let partial = destination.with_extension("part");
        fs::copy(source, &partial)?;
        fs::rename(&partial, &destination)?;
        self.finish_asset(
            destination,
            catalog_id,
            artifact_revision,
            dimension,
            "imported",
            expected_sha256,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn download<F>(
        &self,
        url: &str,
        filename: &str,
        catalog_id: &str,
        artifact_revision: &str,
        dimension: usize,
        expected_sha256: Option<&str>,
        cancel: &AtomicBool,
        mut on_progress: F,
    ) -> Result<EmbeddingModelAsset, GenerationModelError>
    where
        F: FnMut(DownloadProgress),
    {
        if dimension == 0 {
            return Err(GenerationModelError::Unavailable(
                "embedding dimension must be positive".into(),
            ));
        }
        fs::create_dir_all(&self.managed_dir)?;
        let destination = self.managed_dir.join(filename);
        let partial = destination.with_extension("part");
        let download_id = Uuid::new_v4().to_string();
        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|error| GenerationModelError::Download(error.to_string()))?;
        if !response.status().is_success() {
            return Err(GenerationModelError::Download(
                response.status().to_string(),
            ));
        }
        let total = response.content_length();
        let mut file = fs::File::create(&partial)?;
        let mut received = 0_u64;
        let mut buffer = [0_u8; 1024 * 1024];
        on_progress(DownloadProgress {
            download_id: download_id.clone(),
            bytes_received: 0,
            bytes_total: total,
            fraction: 0.0,
            state: "running".into(),
        });
        loop {
            if cancel.load(Ordering::Relaxed) {
                file.flush()?;
                on_progress(DownloadProgress {
                    download_id,
                    bytes_received: received,
                    bytes_total: total,
                    fraction: fraction(received, total),
                    state: "canceled".into(),
                });
                return Err(GenerationModelError::Canceled);
            }
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            received += read as u64;
            on_progress(DownloadProgress {
                download_id: download_id.clone(),
                bytes_received: received,
                bytes_total: total,
                fraction: fraction(received, total),
                state: "running".into(),
            });
        }
        file.flush()?;
        fs::rename(&partial, &destination)?;
        let asset = self.finish_asset(
            destination,
            catalog_id,
            artifact_revision,
            dimension,
            "downloaded",
            expected_sha256,
        )?;
        on_progress(DownloadProgress {
            download_id,
            bytes_received: received,
            bytes_total: total,
            fraction: 1.0,
            state: "done".into(),
        });
        Ok(asset)
    }

    pub fn select(&self, asset: &EmbeddingModelAsset) -> Result<PathBuf, GenerationModelError> {
        if !asset.path.is_file() {
            return Err(GenerationModelError::Unavailable(
                "selected embedding model is missing".into(),
            ));
        }
        if let Some(database) = &self.database {
            let model_id = asset.id.clone();
            database
                .run(move |connection| {
                    connection
                        .execute(
                            "INSERT INTO settings(key, value_json, updated_at) VALUES ('llm.embedding_selection', ?1, ?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                            (&model_id, Utc::now().to_rfc3339()),
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .map_err(|error| GenerationModelError::Unavailable(error.to_string()))?;
        }
        Ok(asset.path.clone())
    }

    pub fn select_installed(
        &self,
        catalog_id: &str,
        artifact_revision: &str,
    ) -> Result<EmbeddingModelAsset, GenerationModelError> {
        let database = self.database.as_ref().ok_or_else(|| {
            GenerationModelError::Unavailable("embedding database is not configured".into())
        })?;
        let managed_dir = self.managed_dir.clone();
        let catalog_id = catalog_id.to_owned();
        let artifact_revision = artifact_revision.to_owned();
        let asset = database
            .run(move |connection| {
                let (id, path, sha256, dimension): (String, String, String, i64) = connection
                    .query_row(
                        "SELECT id, relative_path, sha256, COALESCE((SELECT json_extract(value_json, '$.dimension') FROM settings WHERE key='llm.embedding.dimension'), 0) FROM model_assets WHERE catalog_id=?1 AND artifact_revision=?2 AND role='embedding' AND state='ready'",
                        (&catalog_id, &artifact_revision),
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .map_err(|error| error.to_string())?;
                if dimension <= 0 {
                    return Err("embedding model dimension is not recorded".into());
                }
                Ok(EmbeddingModelAsset {
                    id,
                    catalog_id,
                    artifact_revision,
                    path: managed_dir.join(path),
                    sha256,
                    dimension: dimension as usize,
                })
            })
            .map_err(|error| GenerationModelError::Unavailable(error.to_string()))?;
        self.select(&asset)?;
        Ok(asset)
    }

    pub fn mark_setup_skipped(&self) -> Result<(), String> {
        let Some(database) = &self.database else {
            return Ok(());
        };
        database
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO settings(key, value_json, updated_at) VALUES ('llm.embedding_setup_skipped', 'true', ?1) ON CONFLICT(key) DO UPDATE SET value_json='true', updated_at=excluded.updated_at",
                        [Utc::now().to_rfc3339()],
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
    }

    pub fn selected(&self) -> Result<EmbeddingModelAsset, String> {
        let managed_dir = self.managed_dir.clone();
        let database = self
            .database
            .as_ref()
            .ok_or_else(|| "embedding database is not configured".to_string())?;
        database
            .run(move |connection| {
                let id: String = connection
                    .query_row(
                        "SELECT value_json FROM settings WHERE key='llm.embedding_selection'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let (catalog_id, artifact_revision, path, sha256, dimension): (String, String, String, String, i64) = connection
                    .query_row(
                        "SELECT catalog_id, artifact_revision, relative_path, sha256, COALESCE((SELECT json_extract(value_json, '$.dimension') FROM settings WHERE key = 'llm.embedding.dimension'), 0) FROM model_assets WHERE id=?1 AND role='embedding' AND state='ready'",
                        [&id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                    )
                    .map_err(|error| error.to_string())?;
                if dimension <= 0 {
                    return Err("embedding model dimension is not recorded".into());
                }
                Ok(EmbeddingModelAsset {
                    id,
                    catalog_id,
                    artifact_revision,
                    path: managed_dir.join(path),
                    sha256,
                    dimension: dimension as usize,
                })
            })
            .map_err(|error| error.to_string())
    }

    fn finish_asset(
        &self,
        path: PathBuf,
        catalog_id: &str,
        artifact_revision: &str,
        dimension: usize,
        origin: &str,
        expected_sha256: Option<&str>,
    ) -> Result<EmbeddingModelAsset, GenerationModelError> {
        let actual = sha256(&path)?;
        if let Some(expected) = expected_sha256 {
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(GenerationModelError::Checksum {
                    expected: expected.into(),
                    actual,
                });
            }
        }
        let asset = EmbeddingModelAsset {
            id: format!("embedding:{catalog_id}:{artifact_revision}"),
            catalog_id: catalog_id.into(),
            artifact_revision: artifact_revision.into(),
            path: path.clone(),
            sha256: actual,
            dimension,
        };
        if let Some(database) = &self.database {
            let id = asset.id.clone();
            let relative_path = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let catalog_id = asset.catalog_id.clone();
            let artifact_revision = asset.artifact_revision.clone();
            let sha256 = asset.sha256.clone();
            let origin = origin.to_owned();
            let dimension_value = serde_json::json!({"dimension": dimension});
            database
                .run(move |connection| {
                    connection
                        .execute(
                            "INSERT INTO model_assets(id, catalog_id, artifact_revision, sha256, format, role, origin, relative_path, state) VALUES (?1, ?2, ?3, ?4, 'gguf', 'embedding', ?5, ?6, 'ready') ON CONFLICT(catalog_id, artifact_revision) DO UPDATE SET sha256=excluded.sha256, relative_path=excluded.relative_path, origin=excluded.origin, state='ready'",
                            (&id, &catalog_id, &artifact_revision, &sha256, &origin, &relative_path),
                        )
                        .map_err(|error| error.to_string())?;
                    connection
                        .execute(
                            "INSERT INTO settings(key, value_json, updated_at) VALUES ('llm.embedding.dimension', ?1, ?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                            (dimension_value.to_string(), Utc::now().to_rfc3339()),
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .map_err(|error| GenerationModelError::Unavailable(error.to_string()))?;
        }
        Ok(asset)
    }
}

impl GenerationModelManager {
    pub fn new(managed_dir: impl Into<PathBuf>) -> Self {
        Self {
            managed_dir: managed_dir.into(),
            client: Client::new(),
            database: None,
        }
    }

    pub fn with_database(mut self, database: Database) -> Self {
        self.database = Some(database);
        self
    }

    pub fn catalog() -> Vec<GenerationModelCatalogEntry> {
        vec![
            GenerationModelCatalogEntry {
                id: "generation-3b-instruct".into(),
                display_name: "Local 3B Instruct".into(),
                filename: "generation-3b-instruct.Q4_K_M.gguf".into(),
                size_bytes: 2_200_000_000,
                ram_bytes: 4_000_000_000,
                sha256: None,
            },
            GenerationModelCatalogEntry {
                id: "generation-7b-instruct".into(),
                display_name: "Local 7B Instruct".into(),
                filename: "generation-7b-instruct.Q4_K_M.gguf".into(),
                size_bytes: 4_500_000_000,
                ram_bytes: 7_000_000_000,
                sha256: None,
            },
        ]
    }

    pub fn import(
        &self,
        source: &Path,
        catalog_id: &str,
        artifact_revision: &str,
        expected_sha256: Option<&str>,
    ) -> Result<GenerationModelAsset, GenerationModelError> {
        fs::create_dir_all(&self.managed_dir)?;
        let filename = source
            .file_name()
            .ok_or_else(|| GenerationModelError::Unavailable("source has no filename".into()))?;
        let destination = self.managed_dir.join(filename);
        let partial = destination.with_extension("part");
        fs::copy(source, &partial)?;
        fs::rename(&partial, &destination)?;
        self.finish_asset(
            destination,
            catalog_id,
            artifact_revision,
            "imported",
            expected_sha256,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn download<F>(
        &self,
        url: &str,
        filename: &str,
        catalog_id: &str,
        artifact_revision: &str,
        expected_sha256: Option<&str>,
        cancel: &AtomicBool,
        mut on_progress: F,
    ) -> Result<GenerationModelAsset, GenerationModelError>
    where
        F: FnMut(DownloadProgress),
    {
        fs::create_dir_all(&self.managed_dir)?;
        let destination = self.managed_dir.join(filename);
        let partial = destination.with_extension("part");
        let download_id = Uuid::new_v4().to_string();
        let mut response = self
            .client
            .get(url)
            .send()
            .map_err(|error| GenerationModelError::Download(error.to_string()))?;
        if !response.status().is_success() {
            return Err(GenerationModelError::Download(
                response.status().to_string(),
            ));
        }
        let total = response.content_length();
        let mut file = fs::File::create(&partial)?;
        let mut received = 0_u64;
        let mut buffer = [0_u8; 1024 * 1024];
        on_progress(DownloadProgress {
            download_id: download_id.clone(),
            bytes_received: 0,
            bytes_total: total,
            fraction: 0.0,
            state: "running".into(),
        });
        loop {
            if cancel.load(Ordering::Relaxed) {
                file.flush()?;
                on_progress(DownloadProgress {
                    download_id,
                    bytes_received: received,
                    bytes_total: total,
                    fraction: fraction(received, total),
                    state: "canceled".into(),
                });
                return Err(GenerationModelError::Canceled);
            }
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            received += read as u64;
            on_progress(DownloadProgress {
                download_id: download_id.clone(),
                bytes_received: received,
                bytes_total: total,
                fraction: fraction(received, total),
                state: "running".into(),
            });
        }
        file.flush()?;
        fs::rename(&partial, &destination)?;
        let asset = self.finish_asset(
            destination,
            catalog_id,
            artifact_revision,
            "downloaded",
            expected_sha256,
        )?;
        on_progress(DownloadProgress {
            download_id,
            bytes_received: received,
            bytes_total: total,
            fraction: 1.0,
            state: "done".into(),
        });
        Ok(asset)
    }

    pub fn select(&self, asset: &GenerationModelAsset) -> Result<PathBuf, GenerationModelError> {
        if !asset.path.is_file() {
            return Err(GenerationModelError::Unavailable(
                "selected generation model is missing".into(),
            ));
        }
        Ok(asset.path.clone())
    }

    pub fn mark_setup_skipped(&self) -> Result<(), String> {
        let Some(database) = &self.database else {
            return Ok(());
        };
        database
            .run(|connection| {
                connection
                    .execute(
                        "INSERT INTO settings(key, value_json, updated_at) VALUES ('llm.generation_setup_skipped', 'true', ?1) ON CONFLICT(key) DO UPDATE SET value_json='true', updated_at=excluded.updated_at",
                        [Utc::now().to_rfc3339()],
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
    }

    fn finish_asset(
        &self,
        path: PathBuf,
        catalog_id: &str,
        artifact_revision: &str,
        origin: &str,
        expected_sha256: Option<&str>,
    ) -> Result<GenerationModelAsset, GenerationModelError> {
        let actual = sha256(&path)?;
        if let Some(expected) = expected_sha256 {
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(GenerationModelError::Checksum {
                    expected: expected.into(),
                    actual,
                });
            }
        }
        let asset = GenerationModelAsset {
            id: format!("generation:{catalog_id}:{artifact_revision}"),
            catalog_id: catalog_id.into(),
            artifact_revision: artifact_revision.into(),
            path: path.clone(),
            sha256: actual,
        };
        if let Some(database) = &self.database {
            let id = asset.id.clone();
            let relative_path = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let catalog_id = asset.catalog_id.clone();
            let artifact_revision = asset.artifact_revision.clone();
            let sha256 = asset.sha256.clone();
            let origin = origin.to_owned();
            let _ = database.run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO model_assets(id, catalog_id, artifact_revision, sha256, format, role, origin, relative_path, state) VALUES (?1, ?2, ?3, ?4, 'gguf', 'generation', ?5, ?6, 'ready') ON CONFLICT(catalog_id, artifact_revision) DO UPDATE SET sha256=excluded.sha256, relative_path=excluded.relative_path, origin=excluded.origin, state='ready'",
                        (&id, &catalog_id, &artifact_revision, &sha256, &origin, &relative_path),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        }
        Ok(asset)
    }
}

fn fraction(received: u64, total: Option<u64>) -> f32 {
    total
        .filter(|total| *total > 0)
        .map(|total| (received as f64 / total as f64).min(1.0) as f32)
        .unwrap_or(0.0)
}

fn sha256(path: &Path) -> Result<String, GenerationModelError> {
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
    use std::sync::atomic::AtomicBool;

    #[test]
    fn imported_generation_model_is_verified_and_setup_can_be_skipped() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("model.gguf");
        fs::write(&source, b"generation-model").unwrap();
        let manager = GenerationModelManager::new(root.path().join("managed"));
        let asset = manager
            .import(&source, "generation-test", "r1", None)
            .unwrap();
        assert_eq!(manager.select(&asset).unwrap(), asset.path);
        assert!(manager.mark_setup_skipped().is_ok());
        assert!(!AtomicBool::new(false).load(Ordering::Relaxed));
    }

    #[test]
    fn imported_embedding_model_can_be_selected_without_network_setup() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("embedding.gguf");
        fs::write(&source, b"embedding-model").unwrap();
        let db = Database::in_memory().unwrap();
        let manager =
            EmbeddingModelManager::new(root.path().join("managed")).with_database(db.clone());
        let asset = manager
            .import(&source, "embedding-test", "r1", 4, None)
            .unwrap();
        manager.select(&asset).unwrap();
        assert_eq!(manager.selected().unwrap().dimension, 4);
        manager.mark_setup_skipped().unwrap();
        let skipped: String = db
            .run(|connection| {
                connection
                    .query_row(
                        "SELECT value_json FROM settings WHERE key='llm.embedding_setup_skipped'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())
            })
            .unwrap();
        assert_eq!(skipped, "true");
    }
}
