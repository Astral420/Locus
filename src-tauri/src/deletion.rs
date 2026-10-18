//! Visibility tombstones and durable owned-file cleanup for meetings.

use crate::db::Database;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeletionError {
    #[error("deletion database operation failed: {0}")]
    Database(String),
    #[error("deletion path is outside app-owned media: {0}")]
    UnsafePath(PathBuf),
    #[error("deletion cleanup failed: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CleanupManifest {
    meeting_id: String,
    generation: i64,
    paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionReport {
    pub meeting_id: String,
    pub generation: i64,
    pub physical_paths_removed: usize,
    pub cleanup_retry_required: bool,
}

pub fn tombstone_and_cleanup(
    database: &Database,
    media_root: impl AsRef<Path>,
    meeting_id: impl Into<String>,
) -> Result<DeletionReport, DeletionError> {
    let media_root = media_root.as_ref().to_path_buf();
    let meeting_id = meeting_id.into();
    let (generation, paths) = database
        .run({
            let meeting_id = meeting_id.clone();
            move |connection| {
                let transaction = connection.transaction().map_err(|e| e.to_string())?;
                let generation: i64 = transaction
                    .query_row(
                        "SELECT deletion_generation + 1 FROM meetings WHERE id=?1 AND deleted_at IS NULL",
                        [&meeting_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let mut paths = Vec::new();
                collect_optional_paths(&transaction, "SELECT manifest_path FROM capture_manifests WHERE meeting_id=?1", &meeting_id, &mut paths)?;
                collect_optional_paths(&transaction, "SELECT relative_path FROM media_segments WHERE manifest_id IN (SELECT id FROM capture_manifests WHERE meeting_id=?1)", &meeting_id, &mut paths)?;
                collect_optional_paths(&transaction, "SELECT relative_path FROM artifact_revisions WHERE meeting_id=?1", &meeting_id, &mut paths)?;
                collect_optional_paths(&transaction, "SELECT relative_path FROM slides WHERE meeting_id=?1", &meeting_id, &mut paths)?;
                // The meeting's media directory is the owned unit: recordings,
                // raw streams, slides and intermediates that are not tracked
                // row-by-row (audio.wav, system_audio.wav, sync-report.txt,
                // audio-streams.json, ...) all live under it. Remove it last so
                // its children are gone before the directory itself.
                paths.push(meeting_id.clone());
                let manifest = CleanupManifest { meeting_id: meeting_id.clone(), generation, paths: paths.clone() };
                transaction
                    .execute(
                        "UPDATE meetings SET deleted_at=?1, deletion_generation=?2, lifecycle='deleted' WHERE id=?3 AND deleted_at IS NULL",
                        (Utc::now().to_rfc3339(), generation, &meeting_id),
                    )
                    .map_err(|e| e.to_string())?;
                transaction
                    .execute(
                        "INSERT INTO cleanup_jobs(id, owner_kind, owner_id, generation, state, manifest_json, updated_at) VALUES (?1, 'meeting', ?2, ?3, 'pending', ?4, ?5)",
                        (format!("cleanup:{meeting_id}:{generation}"), &meeting_id, generation, serde_json::to_string(&manifest).map_err(|e| e.to_string())?, Utc::now().to_rfc3339()),
                    )
                    .map_err(|e| e.to_string())?;
                // Remove app-owned relational data immediately. Independent
                // documents are preserved; only their link rows are removed.
                for sql in [
                    "DELETE FROM chat_threads WHERE meeting_id=?1",
                    "DELETE FROM document_meetings WHERE meeting_id=?1",
                    "DELETE FROM sources WHERE meeting_id=?1",
                    "DELETE FROM jobs WHERE meeting_id=?1",
                    "DELETE FROM capture_manifests WHERE meeting_id=?1",
                    "DELETE FROM artifact_revisions WHERE meeting_id=?1",
                    "DELETE FROM summary_revisions WHERE meeting_id=?1",
                    "DELETE FROM slides WHERE meeting_id=?1",
                ] {
                    transaction.execute(sql, [&meeting_id]).map_err(|e| e.to_string())?;
                }
                transaction.commit().map_err(|e| e.to_string())?;
                Ok((generation, paths))
            }
        })
        .map_err(|error| DeletionError::Database(error.to_string()))?;

    let manifest = CleanupManifest {
        meeting_id: meeting_id.clone(),
        generation,
        paths: paths.clone(),
    };
    let mut removed = 0;
    let mut errors = Vec::new();
    for path in manifest.paths {
        let resolved = owned_path(&media_root, Path::new(&path))?;
        if !resolved.exists() {
            continue;
        }
        let result = if resolved.is_dir() {
            fs::remove_dir_all(&resolved)
        } else {
            fs::remove_file(&resolved)
        };
        match result {
            Ok(()) => removed += 1,
            Err(error) => errors.push(format!("{}: {error}", resolved.display())),
        }
    }
    let cleanup_id = format!("cleanup:{meeting_id}:{generation}");
    let final_state = if errors.is_empty() { "done" } else { "pending" };
    let error_text = (!errors.is_empty()).then(|| errors.join("; "));
    database
        .run(move |connection| {
            connection
                .execute(
                    "UPDATE cleanup_jobs SET state=?1, error=?2, updated_at=?3 WHERE id=?4",
                    (
                        final_state,
                        error_text,
                        Utc::now().to_rfc3339(),
                        &cleanup_id,
                    ),
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .map_err(|error| DeletionError::Database(error.to_string()))?;
    Ok(DeletionReport {
        meeting_id,
        generation,
        physical_paths_removed: removed,
        cleanup_retry_required: !errors.is_empty(),
    })
}

pub fn retry_pending_cleanup(
    database: &Database,
    media_root: impl AsRef<Path>,
) -> Result<usize, DeletionError> {
    let pending = database
        .run(|connection| {
            let mut statement = connection
                .prepare("SELECT id, manifest_json FROM cleanup_jobs WHERE state='pending'")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let json: String = row.get(1)?;
                    Ok((id, json))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            rows
        })
        .map_err(|error| DeletionError::Database(error.to_string()))?;
    let mut completed = 0;
    for (id, json) in pending {
        let manifest: CleanupManifest = serde_json::from_str(&json)
            .map_err(|error| DeletionError::Database(error.to_string()))?;
        let mut failed = false;
        for path in manifest.paths {
            let resolved = owned_path(media_root.as_ref(), Path::new(&path))?;
            if !resolved.exists() {
                continue;
            }
            let result = if resolved.is_dir() {
                fs::remove_dir_all(&resolved)
            } else {
                fs::remove_file(&resolved)
            };
            if result.is_err() {
                failed = true;
            }
        }
        if !failed {
            database
                .run(move |connection| {
                    connection
                        .execute("UPDATE cleanup_jobs SET state='done', error=NULL, updated_at=?1 WHERE id=?2", (Utc::now().to_rfc3339(), &id))
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
                .map_err(|error| DeletionError::Database(error.to_string()))?;
            completed += 1;
        }
    }
    // Reconcile tombstones whose cleanup manifest predates recording the whole
    // meeting directory: the tombstone is the durable signal that the meeting's
    // owned media must go, so any directory still on disk is removed. Already
    // gone is a cheap no-op, keeping restarts idempotent.
    let deleted = database
        .run(|connection| {
            let mut statement = connection
                .prepare("SELECT id FROM meetings WHERE deleted_at IS NOT NULL")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            rows
        })
        .map_err(|error| DeletionError::Database(error.to_string()))?;
    for meeting_id in deleted {
        let directory = owned_path(media_root.as_ref(), Path::new(&meeting_id))?;
        if directory.exists() {
            fs::remove_dir_all(&directory).map_err(|error| DeletionError::Io(error.to_string()))?;
            completed += 1;
        }
    }
    Ok(completed)
}

fn collect_optional_paths(
    connection: &rusqlite::Connection,
    sql: &str,
    meeting_id: &str,
    paths: &mut Vec<String>,
) -> Result<(), String> {
    let mut statement = connection.prepare(sql).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([meeting_id], |row| row.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?;
    for row in rows {
        if let Some(path) = row.map_err(|e| e.to_string())? {
            paths.push(path);
        }
    }
    Ok(())
}

fn owned_path(root: &Path, path: &Path) -> Result<PathBuf, DeletionError> {
    let root = root
        .canonicalize()
        .map_err(|error| DeletionError::Io(error.to_string()))?;
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    // Resolve symlinks when the target still exists; when an earlier attempt
    // already removed it (or its parent), fall back to a lexical check so a
    // retry reconciles the rest instead of failing forever.
    let checked = match resolved.canonicalize() {
        Ok(canonical) => canonical,
        Err(_) => lexical_normalize(&resolved),
    };
    if checked == root || !checked.starts_with(&root) {
        return Err(DeletionError::UnsafePath(resolved));
    }
    Ok(checked)
}

/// Collapse `.` and `..` segments without touching the filesystem, so a path
/// whose parents no longer exist can still be containment-checked.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_meeting(db: &Database, meeting_id: &str) {
        let meeting_id = meeting_id.to_owned();
        db.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO meetings(id,title,title_origin,recorded_at,timezone) VALUES (?1,'Meeting','placeholder','now','+00:00')",
                    [&meeting_id],
                )
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();
    }

    #[test]
    fn tombstone_excludes_meeting_and_removes_all_owned_media() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        fs::create_dir_all(media.join("m1/slides")).unwrap();
        // The mixed recording is tracked; the raw streams and sidecars are not.
        fs::write(media.join("m1/recording.mp4"), b"video").unwrap();
        fs::write(media.join("m1/audio.wav"), b"audio").unwrap();
        fs::write(media.join("m1/system_audio.wav"), b"sys").unwrap();
        fs::write(media.join("m1/sync-report.txt"), b"sync").unwrap();
        fs::write(media.join("m1/slides/slide-0000.png"), b"png").unwrap();
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone) VALUES ('m1','Meeting','placeholder','now','+00:00')", []).map_err(|e| e.to_string())?;
            connection.execute("INSERT INTO documents(id,content_hash,relative_path,media_type,size_bytes,extraction_state,extraction_version) VALUES ('d1','hash','d1.md','text/markdown',3,'ready','1')", []).map_err(|e| e.to_string())?;
            connection.execute("INSERT INTO document_meetings(document_id,meeting_id) VALUES ('d1','m1')", []).map_err(|e| e.to_string())?;
            connection.execute("INSERT INTO capture_manifests(id,meeting_id,capture_generation,state,manifest_path) VALUES ('cm1','m1',0,'saved','m1/manifest.json')", []).map_err(|e| e.to_string())?;
            connection.execute("INSERT INTO media_segments(id,manifest_id,stream_id,source,ordinal,relative_path,start_seconds,duration_seconds,committed_at) VALUES ('seg','cm1','mixed','mixed',0,'m1/recording.mp4',0,1,'now')", []).map_err(|e| e.to_string())?;
            connection.execute("INSERT INTO slides(id,meeting_id,image_hash,relative_path) VALUES ('s1','m1','hash','m1/slides/slide-0000.png')", []).map_err(|e| e.to_string())?;
            Ok(())
        }).unwrap();
        let report = tombstone_and_cleanup(&db, &media, "m1").unwrap();
        assert!(report.physical_paths_removed >= 1);
        assert!(
            !media.join("m1").exists(),
            "the whole media directory is removed"
        );
        assert!(db
            .run(|connection| connection
                .query_row(
                    "SELECT deleted_at IS NOT NULL FROM meetings WHERE id='m1'",
                    [],
                    |row| row.get::<_, bool>(0)
                )
                .map_err(|e| e.to_string()))
            .unwrap());
        assert_eq!(
            db.run(|connection| connection
                .query_row("SELECT count(*) FROM documents WHERE id='d1'", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|e| e.to_string()))
                .unwrap(),
            1
        );
    }

    #[test]
    fn retry_reconciles_media_for_already_tombstoned_meetings() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        fs::create_dir_all(media.join("m1")).unwrap();
        fs::write(media.join("m1/audio.wav"), b"audio").unwrap();
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection.execute("INSERT INTO meetings(id,title,title_origin,recorded_at,timezone,deleted_at,lifecycle) VALUES ('m1','Meeting','placeholder','now','+00:00','now','deleted')", []).map_err(|e| e.to_string())?;
            Ok(())
        }).unwrap();
        assert_eq!(retry_pending_cleanup(&db, &media).unwrap(), 1);
        assert!(!media.join("m1").exists());
        // A second pass is a no-op and does not error on the now-missing paths.
        assert_eq!(retry_pending_cleanup(&db, &media).unwrap(), 0);
    }

    #[test]
    fn tombstone_rejects_a_meeting_id_that_escapes_the_media_root() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        fs::create_dir_all(&media).unwrap();
        let db = Database::in_memory().unwrap();
        seed_meeting(&db, "../escape");
        let result = tombstone_and_cleanup(&db, &media, "../escape");
        assert!(matches!(result, Err(DeletionError::UnsafePath(_))));
        assert!(!root.path().join("escape").exists());
    }
}
