//! App-owned storage migration primitives.
//!
//! Migrations are copy-first and journaled.  The source is never removed by a
//! migration, which keeps capture and bundled assets recoverable if a volume
//! disappears or the copy is interrupted.

use crate::{contracts::StorageInfoDto, db::Database};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage path is invalid: {0}")]
    InvalidPath(String),
    #[error("storage migration is unavailable while capture or data-changing jobs are active")]
    Busy,
    #[error("storage I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("storage database operation failed: {0}")]
    Database(String),
    #[error("storage verification failed for {0}")]
    Verification(PathBuf),
}

#[derive(Debug, Clone)]
pub struct StorageMigrationResult {
    pub migration_id: String,
    pub source_root: PathBuf,
    pub target_root: PathBuf,
    pub state: String,
    pub old_copy_cleanup_offered: bool,
}

#[derive(Clone)]
pub struct StorageService {
    database: Database,
    data_root: PathBuf,
    models_root: Arc<Mutex<PathBuf>>,
}

impl StorageService {
    pub fn new(
        database: Database,
        data_root: impl Into<PathBuf>,
        models_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            database,
            data_root: data_root.into(),
            models_root: Arc::new(Mutex::new(models_root.into())),
        }
    }

    pub fn info(&self, capture_active: bool) -> StorageInfoDto {
        let models_root = self
            .models_root
            .lock()
            .map(|path| path.clone())
            .unwrap_or_else(|_| PathBuf::from("models"));
        StorageInfoDto {
            data_root: self.data_root.display().to_string(),
            models_root: models_root.display().to_string(),
            free_bytes: None,
            capture_active,
            migration_state: None,
        }
    }

    pub fn migrate_models(
        &self,
        target_root: impl AsRef<Path>,
        capture_active: bool,
        workers_idle: bool,
    ) -> Result<StorageMigrationResult, StorageError> {
        let source_root = self
            .models_root
            .lock()
            .map_err(|_| StorageError::InvalidPath("models root lock poisoned".into()))?
            .clone();
        self.migrate_tree(
            source_root,
            target_root.as_ref().to_path_buf(),
            capture_active,
            workers_idle,
            "models",
            true,
        )
    }

    pub fn relocate_data(
        &self,
        target_root: impl AsRef<Path>,
        capture_active: bool,
        workers_idle: bool,
    ) -> Result<StorageMigrationResult, StorageError> {
        self.migrate_tree(
            self.data_root.clone(),
            target_root.as_ref().to_path_buf(),
            capture_active,
            workers_idle,
            "data",
            false,
        )
    }

    pub fn recover_incomplete(&self) -> Result<usize, StorageError> {
        self.database
            .run(|connection| {
                connection
                    .execute(
                        "UPDATE storage_migrations SET state='recovery_required', error='application restart interrupted migration', updated_at=?1 WHERE state IN ('copying','verifying')",
                        [Utc::now().to_rfc3339()],
                    )
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| StorageError::Database(error.to_string()))
    }

    fn migrate_tree(
        &self,
        source_root: PathBuf,
        target_root: PathBuf,
        capture_active: bool,
        workers_idle: bool,
        kind: &str,
        update_models_root: bool,
    ) -> Result<StorageMigrationResult, StorageError> {
        if capture_active || !workers_idle {
            return Err(StorageError::Busy);
        }
        validate_roots(&source_root, &target_root)?;
        if target_root.exists() && fs::read_dir(&target_root)?.next().is_some() {
            return Err(StorageError::InvalidPath(format!(
                "target directory is not empty: {}",
                target_root.display()
            )));
        }

        let migration_id = Uuid::new_v4().to_string();
        let source_for_db = source_root.display().to_string();
        let target_for_db = target_root.display().to_string();
        self.database
            .run({
                let migration_id = migration_id.clone();
                let kind = kind.to_owned();
                move |connection| {
                    connection
                        .execute(
                            "INSERT INTO storage_migrations(id, source_root, target_root, manifest_json, state, progress, updated_at) VALUES (?1, ?2, ?3, ?4, 'copying', 0, ?5)",
                            (
                                &migration_id,
                                &source_for_db,
                                &target_for_db,
                                serde_json::json!({"kind": kind}).to_string(),
                                Utc::now().to_rfc3339(),
                            ),
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            })
            .map_err(|error| StorageError::Database(error.to_string()))?;

        let operation = copy_and_verify(&source_root, &target_root);
        if let Err(error) = operation {
            let message = error.to_string();
            let _ = self.update_migration(&migration_id, "error", 0.0, Some(&message));
            return Err(error);
        }
        self.update_migration(&migration_id, "verifying", 1.0, None)?;
        if update_models_root {
            let root = target_root.clone();
            let migration_id_for_db = migration_id.clone();
            self.database
                .run(move |connection| {
                    let value = serde_json::to_string(&root.display().to_string())
                        .map_err(|error| error.to_string())?;
                    connection
                        .execute(
                            "INSERT INTO settings(key, value_json, updated_at) VALUES ('storage.models_root', ?1, ?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                            (&value, Utc::now().to_rfc3339()),
                        )
                        .map_err(|error| error.to_string())?;
                    connection
                        .execute(
                            "UPDATE storage_migrations SET state='complete', updated_at=?1 WHERE id=?2",
                            (Utc::now().to_rfc3339(), &migration_id_for_db),
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .map_err(|error| StorageError::Database(error.to_string()))?;
            *self
                .models_root
                .lock()
                .map_err(|_| StorageError::InvalidPath("models root lock poisoned".into()))? =
                target_root.clone();
        } else {
            self.update_migration(&migration_id, "complete_restart_required", 1.0, None)?;
        }

        Ok(StorageMigrationResult {
            migration_id,
            source_root,
            target_root,
            state: if update_models_root {
                "complete".into()
            } else {
                "complete_restart_required".into()
            },
            old_copy_cleanup_offered: true,
        })
    }

    fn update_migration(
        &self,
        migration_id: &str,
        state: &str,
        progress: f64,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        let migration_id = migration_id.to_owned();
        let state = state.to_owned();
        let error = error.map(str::to_owned);
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "UPDATE storage_migrations SET state=?1, progress=?2, error=?3, updated_at=?4 WHERE id=?5",
                        (&state, progress, error, Utc::now().to_rfc3339(), &migration_id),
                    )
                    .map(|_| ())
                    .map_err(|operation| operation.to_string())
            })
            .map_err(|operation| StorageError::Database(operation.to_string()))
    }
}

fn validate_roots(source: &Path, target: &Path) -> Result<(), StorageError> {
    if !source.exists() {
        return Err(StorageError::InvalidPath(format!(
            "source directory does not exist: {}",
            source.display()
        )));
    }
    if !source.is_dir() || target == source {
        return Err(StorageError::InvalidPath(
            "source and target must be different directories".into(),
        ));
    }
    if target.starts_with(source) {
        return Err(StorageError::InvalidPath(
            "target cannot be inside the source directory".into(),
        ));
    }
    Ok(())
}

fn copy_and_verify(source: &Path, target: &Path) -> Result<(), StorageError> {
    fs::create_dir_all(target)?;
    let mut files = Vec::new();
    collect_files(source, source, target, &mut files)?;
    for (source_file, target_file) in files {
        if let Some(parent) = target_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source_file, &target_file)?;
        if sha256(&source_file)? != sha256(&target_file)? {
            return Err(StorageError::Verification(source_file));
        }
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    current: &Path,
    target_root: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), StorageError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| StorageError::InvalidPath(path.display().to_string()))?;
        let destination = target_root.join(relative);
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(StorageError::InvalidPath(format!(
                "symlinks are not migratable: {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            fs::create_dir_all(&destination)?;
            collect_files(root, &path, target_root, files)?;
        } else if file_type.is_file() {
            files.push((path, destination));
        }
    }
    Ok(())
}

fn sha256(path: &Path) -> Result<String, StorageError> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_migration_is_copy_first_and_verified() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("models");
        let target = root.path().join("external-models");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested/model.gguf"), b"model").unwrap();
        let db = Database::in_memory().unwrap();
        let service = StorageService::new(db, root.path().join("data"), &source);
        let result = service.migrate_models(&target, false, true).unwrap();
        assert_eq!(result.state, "complete");
        assert_eq!(
            fs::read(target.join("nested/model.gguf")).unwrap(),
            b"model"
        );
        assert!(source.join("nested/model.gguf").exists());
    }

    #[test]
    fn migration_is_rejected_while_busy_or_into_source() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("models");
        fs::create_dir_all(&source).unwrap();
        let service = StorageService::new(Database::in_memory().unwrap(), root.path(), &source);
        assert!(matches!(
            service.migrate_models(&source, false, true),
            Err(StorageError::InvalidPath(_))
        ));
        assert!(matches!(
            service.migrate_models(root.path().join("other"), true, true),
            Err(StorageError::Busy)
        ));
    }
}
