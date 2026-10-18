//! Uninstalling installed quantizations and whole models from the Model Manager.
//!
//! Only files inside the three managed folders (`whisper_models()`,
//! `embedding_models()`, `generation_models()`) are ever deleted (D3). The
//! planner and the selection-fallback decision are pure and unit-tested; the
//! executor deletes each file before its database row, so a crash in between is
//! healed by the truthful-install rule in [`crate::commands::model_state`].

use crate::{
    catalog::{download::DownloadRegistry, model::ModelRole},
    commands::{
        model_state::{variant_id_from_revision, whisper_file_identity, LocalModels},
        models::{clear_selection, store_selection, whisper_selector},
    },
    contracts::UninstallResultDto,
    db::Database,
    paths::AppPaths,
    AppState,
};
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use thiserror::Error;

/// One file to delete, resolved from local truth (never from the catalog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub role: ModelRole,
    pub catalog_id: String,
    pub variant_id: String,
    pub path: PathBuf,
    /// The `model_assets` id, absent for whisper files (they have no row).
    pub asset_id: Option<String>,
    pub size: u64,
}

/// A still-installed sibling of the same model, offered as the fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantRef {
    pub variant_id: String,
    pub selection_value: String,
    pub size: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UninstallError {
    #[error("{0}")]
    NotInstalled(String),
    #[error("{0}")]
    Busy(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Db(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionChange {
    Keep,
    Set(String),
    Clear,
}

/// Pure: resolves the files a request would delete, without touching anything.
/// `None` variant means every removable installed variant of the model.
pub fn plan(
    paths: &AppPaths,
    local: &LocalModels,
    catalog_id: &str,
    variant: Option<&str>,
) -> Result<Vec<Target>, UninstallError> {
    let mut targets = Vec::new();

    // Whisper has no rows: the managed folder's own files are the local truth.
    if let Ok(entries) = std::fs::read_dir(paths.whisper_models()) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some((found_catalog, found_variant)) = whisper_file_identity(&name) else {
                continue;
            };
            if found_catalog != catalog_id {
                continue;
            }
            if variant.is_some_and(|wanted| wanted != found_variant.as_str()) {
                continue;
            }
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            targets.push(Target {
                role: ModelRole::Whisper,
                catalog_id: catalog_id.to_string(),
                variant_id: found_variant,
                path,
                asset_id: None,
                size,
            });
        }
    }

    // LLM / embedding: ready rows whose file lives inside the managed folder.
    for asset in local.assets {
        if asset.catalog_id != catalog_id || asset.state != "ready" {
            continue;
        }
        let role = match asset.role.as_str() {
            "generation" => ModelRole::Llm,
            "embedding" => ModelRole::Embedding,
            _ => continue,
        };
        let Some(dir) = local.managed.dir(role) else {
            continue;
        };
        let path = dir.join(&asset.relative_path);
        if !path.is_file() {
            continue;
        }
        let found_variant = variant_id_from_revision(&asset.artifact_revision);
        if variant.is_some_and(|wanted| found_variant != Some(wanted)) {
            continue;
        }
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        targets.push(Target {
            role,
            catalog_id: catalog_id.to_string(),
            variant_id: found_variant.unwrap_or_default().to_string(),
            path,
            asset_id: Some(asset.id.clone()),
            size,
        });
    }

    if targets.is_empty() {
        let label = match variant {
            Some(variant) => format!("{catalog_id} · {variant} is not installed"),
            None => format!("{catalog_id} is not installed"),
        };
        return Err(UninstallError::NotInstalled(label));
    }
    Ok(targets)
}

/// Pure: what to do with the role's selection after the removals.
pub fn selection_change(
    _role: ModelRole,
    _removed: &[Target],
    remaining: &[VariantRef],
    active_was_removed: bool,
    sibling_rule: bool,
) -> SelectionChange {
    if !active_was_removed {
        return SelectionChange::Keep;
    }
    if !sibling_rule || remaining.is_empty() {
        return SelectionChange::Clear;
    }
    // Prefer the standard file, then the smallest quantization.
    remaining
        .iter()
        .find(|variant| variant.variant_id == "standard")
        .or_else(|| remaining.iter().min_by_key(|variant| variant.size))
        .map(|variant| SelectionChange::Set(variant.selection_value.clone()))
        .unwrap_or(SelectionChange::Clear)
}

fn role_str(role: ModelRole) -> &'static str {
    match role {
        ModelRole::Whisper => "whisper",
        ModelRole::Llm => "llm",
        ModelRole::Embedding => "embedding",
    }
}

fn db_role(role: ModelRole) -> &'static str {
    match role {
        ModelRole::Whisper => "whisper",
        ModelRole::Llm => "generation",
        ModelRole::Embedding => "embedding",
    }
}

fn selection_key(role: ModelRole) -> &'static str {
    match role {
        ModelRole::Whisper => "transcription.whisper_selection",
        ModelRole::Llm => "llm.generation_selection",
        ModelRole::Embedding => "llm.embedding_selection",
    }
}

fn setting(database: &Database, key: &'static str) -> Option<String> {
    database
        .run(move |connection| {
            connection
                .query_row(
                    "SELECT value_json FROM settings WHERE key=?1",
                    [key],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|error| error.to_string())
        })
        .ok()
        .flatten()
}

fn count(
    database: &Database,
    sql: &'static str,
    param: Option<String>,
) -> Result<i64, UninstallError> {
    database
        .run(move |connection| {
            let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
            let value = match &param {
                Some(value) => statement.query_row([value.as_str()], |row| row.get::<_, i64>(0)),
                None => statement.query_row([], |row| row.get::<_, i64>(0)),
            }
            .map_err(|error| error.to_string())?;
            Ok(value)
        })
        .map_err(|error| UninstallError::Db(error.to_string()))
}

/// Refuse while the model is downloading or a job that owns it is running.
fn guard(
    database: &Database,
    downloads: &DownloadRegistry,
    role: ModelRole,
    catalog_id: &str,
) -> Result<(), UninstallError> {
    if downloads.is_running(catalog_id) {
        return Err(UninstallError::Busy(format!(
            "{catalog_id} is still downloading; cancel it before removing files"
        )));
    }
    let kind = match role {
        ModelRole::Whisper => "transcription",
        ModelRole::Llm => "summarization",
        ModelRole::Embedding => "indexing",
    };
    if count(
        database,
        "SELECT COUNT(*) FROM jobs WHERE kind=?1 AND state='running'",
        Some(kind.to_string()),
    )? > 0
    {
        return Err(UninstallError::Busy(format!(
            "a {kind} job is running; wait for it to finish"
        )));
    }
    if role == ModelRole::Embedding
        && count(
            database,
            "SELECT COUNT(*) FROM index_generations WHERE state='building'",
            None,
        )? > 0
    {
        return Err(UninstallError::Busy(
            "the knowledge index is still building; wait for it to finish".into(),
        ));
    }
    Ok(())
}

fn delete_asset(database: &Database, asset_id: &str) -> Result<(), UninstallError> {
    let asset_id = asset_id.to_string();
    database
        .run(move |connection| {
            connection
                .execute("DELETE FROM model_assets WHERE id=?1", [&asset_id])
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .map_err(|error| UninstallError::Db(error.to_string()))
}

fn remove_partial(path: &Path) {
    let partial = PathBuf::from(format!("{}.part", path.display()));
    match std::fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("locus: could not delete {}: {error}", partial.display()),
    }
}

fn remaining_variants(
    database: &Database,
    paths: &AppPaths,
    role: ModelRole,
    catalog_id: &str,
) -> Vec<VariantRef> {
    match role {
        ModelRole::Whisper => {
            let selector = whisper_selector(catalog_id);
            let mut remaining = Vec::new();
            if let Ok(entries) = std::fs::read_dir(paths.whisper_models()) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let Some((found_catalog, variant_id)) = whisper_file_identity(&name) else {
                        continue;
                    };
                    if found_catalog != catalog_id {
                        continue;
                    }
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    remaining.push(VariantRef {
                        selection_value: format!("whisper:{selector}:{variant_id}"),
                        variant_id,
                        size,
                    });
                }
            }
            remaining
        }
        ModelRole::Llm | ModelRole::Embedding => {
            let dir = match role {
                ModelRole::Embedding => paths.embedding_models(),
                _ => paths.generation_models(),
            };
            let db_role = db_role(role);
            let catalog = catalog_id.to_string();
            let rows: Vec<(String, String, String)> = database
                .run(move |connection| {
                    let mut statement = connection
                        .prepare("SELECT id, artifact_revision, relative_path FROM model_assets WHERE catalog_id=?1 AND role=?2 AND state='ready'")
                        .map_err(|error| error.to_string())?;
                    let rows = statement
                        .query_map((catalog.as_str(), db_role), |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })
                        .map_err(|error| error.to_string())?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| error.to_string());
                    rows
                })
                .unwrap_or_default();
            rows.into_iter()
                .filter_map(|(id, revision, relative_path)| {
                    let path = dir.join(&relative_path);
                    if !path.is_file() {
                        return None;
                    }
                    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    Some(VariantRef {
                        variant_id: variant_id_from_revision(&revision)
                            .unwrap_or_default()
                            .to_string(),
                        selection_value: id,
                        size,
                    })
                })
                .collect()
        }
    }
}

/// Delete the files, drop their rows, then repair the selection. Crash-safe:
/// file first, row second. `stop_embedding_server` runs for embedding removals
/// before any deletion so Windows can unlink the file.
pub fn execute(
    database: &Database,
    paths: &AppPaths,
    targets: &[Target],
    stop_embedding_server: impl FnOnce(),
) -> Result<UninstallResultDto, UninstallError> {
    let Some(first) = targets.first() else {
        return Err(UninstallError::NotInstalled("nothing to remove".into()));
    };
    let role = first.role;
    let catalog_id = first.catalog_id.clone();

    if role == ModelRole::Embedding {
        stop_embedding_server();
    }

    // Read the resolved active path before deletion to know if it disappears.
    let active_before = if role == ModelRole::Whisper {
        let selection = setting(database, selection_key(role));
        crate::paths::find_whisper_model(
            paths,
            selection.as_deref(),
            std::env::var_os(crate::paths::ENV_WHISPER_MODEL).map(PathBuf::from),
        )
    } else {
        None
    };

    let mut removed = Vec::with_capacity(targets.len());
    let mut freed_bytes = 0u64;
    for target in targets {
        match std::fs::remove_file(&target.path) {
            Ok(()) => freed_bytes += target.size,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(UninstallError::Io(format!(
                    "could not delete {}: {error}",
                    target.path.display()
                )))
            }
        }
        remove_partial(&target.path);
        if let Some(asset_id) = &target.asset_id {
            delete_asset(database, asset_id)?;
        }
        removed.push(target.variant_id.clone());
    }

    let active_was_removed = match role {
        ModelRole::Whisper => active_before
            .as_deref()
            .is_some_and(|active| targets.iter().any(|target| target.path.as_path() == active)),
        _ => {
            let selection = setting(database, selection_key(role))
                .map(|value| value.trim_matches('"').to_string());
            selection.as_deref().is_some_and(|active| {
                targets
                    .iter()
                    .any(|target| target.asset_id.as_deref() == Some(active))
            })
        }
    };

    let remaining = remaining_variants(database, paths, role, &catalog_id);
    let new_selection = match selection_change(
        role,
        targets,
        &remaining,
        active_was_removed,
        role != ModelRole::Embedding,
    ) {
        SelectionChange::Keep => None,
        SelectionChange::Set(value) => {
            store_selection(database, role_str(role), &value).map_err(UninstallError::Db)?;
            Some(value)
        }
        SelectionChange::Clear => {
            clear_selection(database, role_str(role)).map_err(UninstallError::Db)?;
            None
        }
    };

    Ok(UninstallResultDto {
        removed,
        freed_bytes,
        new_selection,
    })
}

fn load_assets(
    database: &Database,
) -> Result<Vec<crate::commands::model_state::InstalledAsset>, UninstallError> {
    use crate::commands::model_state::InstalledAsset;
    database
        .run(|connection| {
            let mut statement = connection
                .prepare("SELECT id, catalog_id, role, state, artifact_revision, relative_path FROM model_assets")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok(InstalledAsset {
                        id: row.get(0)?,
                        catalog_id: row.get(1)?,
                        role: row.get(2)?,
                        state: row.get(3)?,
                        artifact_revision: row.get(4)?,
                        relative_path: row.get(5)?,
                    })
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string());
            rows
        })
        .map_err(|error| UninstallError::Db(error.to_string()))
}

fn run(
    state: &AppState,
    catalog_id: &str,
    variant: Option<&str>,
) -> Result<UninstallResultDto, UninstallError> {
    let assets = load_assets(&state.database)?;
    let whisper_files: Vec<String> = state
        .paths
        .whisper_search_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let whisper_selection = setting(&state.database, selection_key(ModelRole::Whisper));
    let active_whisper_file = crate::paths::find_whisper_model(
        &state.paths,
        whisper_selection.as_deref(),
        std::env::var_os(crate::paths::ENV_WHISPER_MODEL).map(PathBuf::from),
    )
    .and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    });
    let active_generation = setting(&state.database, selection_key(ModelRole::Llm));
    let active_embedding = setting(&state.database, selection_key(ModelRole::Embedding));

    let local = LocalModels {
        assets: &assets,
        whisper_files: &whisper_files,
        active_whisper_file: active_whisper_file.as_deref(),
        active_generation: active_generation.as_deref(),
        active_embedding: active_embedding.as_deref(),
        managed: crate::commands::model_state::ManagedFolders::for_paths(&state.paths),
    };

    let targets = plan(&state.paths, &local, catalog_id, variant)?;
    guard(
        &state.database,
        &state.downloads,
        targets[0].role,
        catalog_id,
    )?;
    execute(&state.database, &state.paths, &targets, || {
        if let Ok(mut slot) = state.embedding_server.lock() {
            if let Some(server) = slot.take() {
                let _ = server.shutdown();
            }
        }
    })
}

#[tauri::command]
pub async fn uninstall_model(
    app: AppHandle,
    catalog_id: String,
    variant_id: Option<String>,
) -> Result<UninstallResultDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        run(&state, &catalog_id, variant_id.as_deref()).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::model_state::{InstalledAsset, ManagedFolders};

    fn local<'a>(paths: &AppPaths, assets: &'a [InstalledAsset]) -> LocalModels<'a> {
        LocalModels {
            assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
            managed: ManagedFolders::for_paths(paths),
        }
    }

    fn asset(id: &str, role: &str, revision: &str, relative_path: &str) -> InstalledAsset {
        InstalledAsset {
            id: id.into(),
            catalog_id: "a/one".into(),
            role: role.into(),
            state: "ready".into(),
            artifact_revision: revision.into(),
            relative_path: relative_path.into(),
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    fn stored(db: &Database, key: &'static str) -> Option<String> {
        setting(db, key).map(|value| value.trim_matches('"').to_string())
    }

    fn whisper_fixture(files: &[&str]) -> (tempfile::TempDir, AppPaths) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        for file in files {
            write(&paths.whisper_models().join(file), b"m");
        }
        (root, paths)
    }

    #[test]
    fn uninstalling_a_whisper_variant_deletes_only_that_file() {
        let (_root, paths) = whisper_fixture(&["ggml-small.bin", "ggml-small-q5_1.bin"]);
        let db = Database::in_memory().unwrap();
        let targets = plan(&paths, &local(&paths, &[]), "whisper-small", Some("q5_1")).unwrap();
        assert_eq!(targets.len(), 1);

        let result = execute(&db, &paths, &targets, || {}).unwrap();
        assert_eq!(result.removed, ["q5_1"]);
        assert_eq!(names(&paths.whisper_models()), ["ggml-small.bin"]);
        assert!(result.new_selection.is_none());
    }

    #[test]
    fn uninstalling_the_active_whisper_variant_switches_to_a_sibling_preferring_standard() {
        let (_root, paths) = whisper_fixture(&["ggml-small-q5_1.bin", "ggml-small.bin"]);
        let db = Database::in_memory().unwrap();
        store_selection(&db, "whisper", "whisper:small:q5_1").unwrap();

        let targets = plan(&paths, &local(&paths, &[]), "whisper-small", Some("q5_1")).unwrap();
        let result = execute(&db, &paths, &targets, || {}).unwrap();

        assert_eq!(
            stored(&db, "transcription.whisper_selection").as_deref(),
            Some("whisper:small:standard")
        );
        assert_eq!(
            result.new_selection.as_deref(),
            Some("whisper:small:standard")
        );
    }

    #[test]
    fn uninstalling_the_last_whisper_variant_clears_the_selection() {
        let (_root, paths) = whisper_fixture(&["ggml-small-q5_1.bin"]);
        let db = Database::in_memory().unwrap();
        store_selection(&db, "whisper", "whisper:small:q5_1").unwrap();

        let targets = plan(&paths, &local(&paths, &[]), "whisper-small", None).unwrap();
        execute(&db, &paths, &targets, || {}).unwrap();

        assert!(stored(&db, "transcription.whisper_selection").is_none());
    }

    #[test]
    fn files_outside_the_managed_folder_are_never_deleted() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let models_root = paths.models.join("ggml-small.bin");
        let bundled = paths.bundled_models[0].join("ggml-small.bin");
        write(&models_root, b"m");
        write(&bundled, b"m");

        let error = plan(
            &paths,
            &local(&paths, &[]),
            "whisper-small",
            Some("standard"),
        )
        .unwrap_err();
        assert!(matches!(error, UninstallError::NotInstalled(_)));
        assert!(models_root.is_file() && bundled.is_file());
    }

    #[test]
    fn whisper_uninstall_is_refused_while_a_transcription_job_is_running() {
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection.execute("INSERT INTO jobs(id,document_id,kind,state,created_at,updated_at) VALUES ('j1','d1','transcription','running','now','now')", []).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap();
        let error = guard(
            &db,
            &DownloadRegistry::default(),
            ModelRole::Whisper,
            "whisper-small",
        )
        .unwrap_err();
        assert!(matches!(error, UninstallError::Busy(_)), "{error}");
    }

    #[test]
    fn an_llm_uninstall_removes_the_file_and_the_row_and_cascades_downloads() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let file = paths.generation_models().join("gemma-Q4_K_M.gguf");
        write(&file, b"weights");
        let db = Database::in_memory().unwrap();
        let id = "generation:a/one:q4_k_m-01234567";
        db.run(move |connection| {
            connection.execute("INSERT INTO model_assets(id,catalog_id,artifact_revision,sha256,format,role,origin,relative_path,state) VALUES (?1,'a/one','q4_k_m-01234567','sha','gguf','generation','downloaded','gemma-Q4_K_M.gguf','ready')", [id]).map_err(|e| e.to_string())?;
            connection.execute("INSERT INTO downloads(id,model_asset_id,url,state,updated_at) VALUES ('d1',?1,'u','ready','now')", [id]).map_err(|e| e.to_string())?;
            Ok(())
        })
        .unwrap();
        let assets = [asset(
            id,
            "generation",
            "q4_k_m-01234567",
            "gemma-Q4_K_M.gguf",
        )];

        let targets = plan(&paths, &local(&paths, &assets), "a/one", Some("q4_k_m")).unwrap();
        let result = execute(&db, &paths, &targets, || {}).unwrap();

        assert_eq!(result.removed, ["q4_k_m"]);
        assert!(result.freed_bytes > 0);
        assert!(!file.exists());
        let remains: i64 = db
            .run(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM model_assets", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        let downloads: i64 = db
            .run(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM downloads", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!((remains, downloads), (0, 0));
    }

    #[test]
    fn a_missing_file_is_treated_as_removed_and_the_row_is_still_deleted() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let db = Database::in_memory().unwrap();
        let id = "generation:a/one:q4_k_m-01234567";
        db.run(move |connection| {
            connection.execute("INSERT INTO model_assets(id,catalog_id,artifact_revision,sha256,format,role,origin,relative_path,state) VALUES (?1,'a/one','q4_k_m-01234567','sha','gguf','generation','downloaded','gone.gguf','ready')", [id]).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap();
        let targets = [Target {
            role: ModelRole::Llm,
            catalog_id: "a/one".into(),
            variant_id: "q4_k_m".into(),
            path: paths.generation_models().join("gone.gguf"),
            asset_id: Some(id.into()),
            size: 10,
        }];

        let result = execute(&db, &paths, &targets, || {}).unwrap();
        assert_eq!(result.freed_bytes, 0);
        let remains: i64 = db
            .run(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM model_assets", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(remains, 0);
    }

    #[test]
    fn a_delete_failure_leaves_the_database_row_untouched() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        // A non-empty directory cannot be removed with remove_file.
        let stubborn = paths.generation_models().join("stubborn");
        write(&stubborn.join("child"), b"x");
        let db = Database::in_memory().unwrap();
        let id = "generation:a/one:q4_k_m-01234567";
        db.run(move |connection| {
            connection.execute("INSERT INTO model_assets(id,catalog_id,artifact_revision,sha256,format,role,origin,relative_path,state) VALUES (?1,'a/one','q4_k_m-01234567','sha','gguf','generation','downloaded','stubborn','ready')", [id]).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap();
        let targets = [Target {
            role: ModelRole::Llm,
            catalog_id: "a/one".into(),
            variant_id: "q4_k_m".into(),
            path: stubborn.clone(),
            asset_id: Some(id.into()),
            size: 1,
        }];

        let error = execute(&db, &paths, &targets, || {}).unwrap_err();
        assert!(matches!(error, UninstallError::Io(_)), "{error}");
        let remains: i64 = db
            .run(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM model_assets", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(remains, 1);
        assert!(stubborn.exists());
    }

    #[test]
    fn partial_files_for_the_variant_are_removed_too() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let file = paths.generation_models().join("m.gguf");
        let partial = PathBuf::from(format!("{}.part", file.display()));
        write(&file, b"weights");
        write(&partial, b"half");
        let db = Database::in_memory().unwrap();

        let targets = [Target {
            role: ModelRole::Llm,
            catalog_id: "a/one".into(),
            variant_id: "q4_k_m".into(),
            path: file.clone(),
            asset_id: None,
            size: 7,
        }];
        execute(&db, &paths, &targets, || {}).unwrap();
        assert!(!file.exists() && !partial.exists());
    }

    #[test]
    fn embedding_uninstall_empties_the_server_slot_and_keeps_index_generations() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let file = paths.embedding_models().join("minilm.gguf");
        write(&file, b"weights");
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection.execute("INSERT INTO index_generations(id,embedding_model_id,embedding_model_revision,dimension,chunker_version,state,created_at) VALUES ('g1','a/one','q8_0-01234567',384,'v1','ready','now')", []).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap();
        let targets = [Target {
            role: ModelRole::Embedding,
            catalog_id: "a/one".into(),
            variant_id: "q8_0".into(),
            path: file.clone(),
            asset_id: None,
            size: 7,
        }];

        let mut stopped = false;
        execute(&db, &paths, &targets, || stopped = true).unwrap();

        assert!(stopped, "the embedding server is shut down before deletion");
        let generations: i64 = db
            .run(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM index_generations", [], |row| {
                        row.get(0)
                    })
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(generations, 1);
        assert!(!file.exists());
    }

    #[test]
    fn embedding_uninstall_is_refused_while_an_index_is_building() {
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection.execute("INSERT INTO index_generations(id,embedding_model_id,embedding_model_revision,dimension,chunker_version,state,created_at) VALUES ('g1','a/one','q8_0-01234567',384,'v1','building','now')", []).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap();
        let error = guard(
            &db,
            &DownloadRegistry::default(),
            ModelRole::Embedding,
            "a/one",
        )
        .unwrap_err();
        assert!(matches!(error, UninstallError::Busy(_)), "{error}");
    }

    #[test]
    fn embedding_uninstall_never_auto_switches_to_another_quantization() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        write(&paths.embedding_models().join("q8_0.gguf"), b"w");
        write(&paths.embedding_models().join("f16.gguf"), b"ww");
        let db = Database::in_memory().unwrap();
        let active = "embedding:a/one:q8_0-01234567";
        store_selection(&db, "embedding", active).unwrap();
        let assets = [
            asset(active, "embedding", "q8_0-01234567", "q8_0.gguf"),
            asset(
                "embedding:a/one:f16-01234567",
                "embedding",
                "f16-01234567",
                "f16.gguf",
            ),
        ];

        let targets = plan(&paths, &local(&paths, &assets), "a/one", Some("q8_0")).unwrap();
        let result = execute(&db, &paths, &targets, || {}).unwrap();

        assert!(stored(&db, "llm.embedding_selection").is_none());
        assert!(result.new_selection.is_none());
        assert!(paths.embedding_models().join("f16.gguf").is_file());
    }

    #[test]
    fn removing_a_non_active_variant_changes_no_selection() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        write(&paths.generation_models().join("q4.gguf"), b"w");
        write(&paths.generation_models().join("q8.gguf"), b"ww");
        let db = Database::in_memory().unwrap();
        let active = "generation:a/one:q8_0-01234567";
        store_selection(&db, "llm", active).unwrap();
        let assets = [
            asset(
                "generation:a/one:q4_k_m-01234567",
                "generation",
                "q4_k_m-01234567",
                "q4.gguf",
            ),
            asset(active, "generation", "q8_0-01234567", "q8.gguf"),
        ];

        let targets = plan(&paths, &local(&paths, &assets), "a/one", Some("q4_k_m")).unwrap();
        execute(&db, &paths, &targets, || {}).unwrap();

        assert_eq!(
            stored(&db, "llm.generation_selection").as_deref(),
            Some(active)
        );
    }

    #[test]
    fn uninstall_never_resets_the_first_run_flag() {
        let (_root, paths) = whisper_fixture(&["ggml-small.bin"]);
        let db = Database::in_memory().unwrap();
        db.run(|connection| {
            connection.execute("INSERT INTO settings(key,value_json,updated_at) VALUES ('models.first_run_provisioned','true','now')", []).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap();

        let targets = plan(&paths, &local(&paths, &[]), "whisper-small", None).unwrap();
        execute(&db, &paths, &targets, || {}).unwrap();

        assert_eq!(
            stored(&db, "models.first_run_provisioned").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn unknown_model_or_variant_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let local_models = local(&paths, &[]);
        assert!(matches!(
            plan(&paths, &local_models, "a/nope", None).unwrap_err(),
            UninstallError::NotInstalled(_)
        ));
        let (_root2, whisper_paths) = whisper_fixture(&["ggml-small.bin"]);
        assert!(matches!(
            plan(
                &whisper_paths,
                &local(&whisper_paths, &[]),
                "whisper-small",
                Some("q2_k")
            )
            .unwrap_err(),
            UninstallError::NotInstalled(_)
        ));
    }

    #[test]
    fn selection_change_table() {
        let removed = Vec::new();
        let standard = VariantRef {
            variant_id: "standard".into(),
            selection_value: "whisper:small:standard".into(),
            size: 500,
        };
        let small_quant = VariantRef {
            variant_id: "q5_1".into(),
            selection_value: "whisper:small:q5_1".into(),
            size: 100,
        };
        let big_quant = VariantRef {
            variant_id: "q8_0".into(),
            selection_value: "whisper:small:q8_0".into(),
            size: 900,
        };

        // Nothing active removed: always keep.
        assert_eq!(
            selection_change(ModelRole::Llm, &removed, &[standard.clone()], false, true),
            SelectionChange::Keep
        );
        // Active removed, siblings allowed, standard wins.
        assert_eq!(
            selection_change(
                ModelRole::Whisper,
                &removed,
                &[small_quant.clone(), standard.clone()],
                true,
                true
            ),
            SelectionChange::Set("whisper:small:standard".into())
        );
        // No standard: the smallest file wins.
        assert_eq!(
            selection_change(
                ModelRole::Whisper,
                &removed,
                &[big_quant.clone(), small_quant.clone()],
                true,
                true
            ),
            SelectionChange::Set("whisper:small:q5_1".into())
        );
        // Active removed, no sibling rule (embedding): clear even with siblings.
        assert_eq!(
            selection_change(
                ModelRole::Embedding,
                &removed,
                &[standard.clone()],
                true,
                false
            ),
            SelectionChange::Clear
        );
        // Active removed, nothing left: clear.
        assert_eq!(
            selection_change(ModelRole::Whisper, &removed, &[], true, true),
            SelectionChange::Clear
        );
    }
}
