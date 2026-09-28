use rusqlite::{Connection, Result as SqlResult};
use std::{path::Path, sync::mpsc, thread};
use thiserror::Error;

const MIGRATION: &str = include_str!("../../migrations/0001_initial.sql");
const KNOWLEDGE_MIGRATION: &str =
    "ALTER TABLE documents ADD COLUMN created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'));";

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database worker stopped")]
    WorkerStopped,
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("database callback failed: {0}")]
    Callback(String),
    #[error("database path is not usable: {0}")]
    Path(String),
}

type Job = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

#[derive(Clone)]
pub struct Database {
    jobs: mpsc::Sender<Job>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|e| DbError::Path(e.to_string()))?;
        }
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("locus-db".into())
            .spawn(move || {
                let Ok(mut connection) = Connection::open(path) else {
                    return;
                };
                if configure(&connection)
                    .and_then(|_| migrate(&mut connection))
                    .is_err()
                {
                    return;
                }
                while let Ok(job) = rx.recv() {
                    job(&mut connection);
                }
            })
            .map_err(|e| DbError::Path(e.to_string()))?;
        Ok(Self { jobs: tx })
    }

    pub fn in_memory() -> Result<Self, DbError> {
        Self::open(":memory:")
    }

    pub fn run<F, R>(&self, callback: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut Connection) -> Result<R, String> + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(1);
        self.jobs
            .send(Box::new(move |connection| {
                let _ = tx.send(callback(connection));
            }))
            .map_err(|_| DbError::WorkerStopped)?;
        rx.recv()
            .map_err(|_| DbError::WorkerStopped)?
            .map_err(DbError::Callback)
    }

    pub fn execute(&self, sql: impl Into<String>) -> Result<usize, DbError> {
        let sql = sql.into();
        self.run(move |connection| {
            connection
                .execute_batch(&sql)
                .map(|_| 0)
                .map_err(|e| e.to_string())
        })
    }
}

fn configure(connection: &Connection) -> SqlResult<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", 5000i64)?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> SqlResult<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);")?;
    let applied: i64 = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if applied < 1 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION)?;
        transaction.execute("INSERT INTO schema_migrations(version, applied_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))", [])?;
        transaction.commit()?;
    }
    let applied: i64 = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if applied < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(KNOWLEDGE_MIGRATION)?;
        transaction.execute("INSERT INTO schema_migrations(version, applied_at) VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ','now'))", [])?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent_and_enforce_foreign_keys() {
        let db = Database::in_memory().unwrap();
        let tables: i64 = db
            .run(|connection| {
                connection
                    .query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meetings'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(tables, 1);
        db.execute(MIGRATION).unwrap();
        let foreign_keys: i64 = db
            .run(|connection| {
                connection
                    .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(foreign_keys, 1);
        let result = db.run(|connection| connection.execute("INSERT INTO media_segments(id, manifest_id, stream_id, source, ordinal, relative_path, start_seconds, duration_seconds, committed_at) VALUES ('x','missing','s','mixed',0,'x',0,1,'now')", []).map(|_| ()).map_err(|e| e.to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn worker_serializes_writes() {
        let db = Database::in_memory().unwrap();
        for index in 0..8 {
            let value = format!("k{index}");
            db.run(move |connection| connection.execute("INSERT INTO settings(key, value_json, updated_at) VALUES (?1, '{}', 'now')", [&value]).map(|_| ()).map_err(|e| e.to_string())).unwrap();
        }
        let count: i64 = db
            .run(|connection| {
                connection
                    .query_row("SELECT count(*) FROM settings", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(count, 8);
    }
}
