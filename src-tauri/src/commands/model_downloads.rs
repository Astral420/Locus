//! Downloading catalog models: the user-triggered download command, cancel, and the
//! background first-run download of the default whisper and embedding models.

use crate::{
    catalog::{
        download::{client, download_verified, DownloadError, Progress},
        model::{CatalogModel, CatalogVariant, ModelRole},
        CatalogSnapshot,
    },
    paths::AppPaths,
    AppState,
};
use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};

pub const EVENT: &str = "model-download-progress";
const PROVISIONED_KEY: &str = "models.first_run_provisioned";
/// Set to any value to skip the first-run background download (CI, offline builds).
pub const ENV_NO_AUTO_DOWNLOAD: &str = "LOCUS_NO_AUTO_DOWNLOAD";

/// What the app downloads on a fresh install. Variants fall back to the smallest file
/// if the catalog stops offering the named one.
pub const DEFAULT_WHISPER: (&str, &str) = ("whisper-small", "standard");
pub const DEFAULT_EMBEDDING: (&str, &str) = ("all-minilm-l6-v2", "q8_0");

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelDownloadEvent {
    pub catalog_id: String,
    /// Shown in the sidebar's Downloads popover.
    pub display_name: String,
    pub variant_id: String,
    /// "running" | "done" | "failed" | "canceled"
    pub state: &'static str,
    pub received: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub path: PathBuf,
    /// The `model_assets` id; whisper files are found on disk and have none.
    pub asset_id: Option<String>,
    /// Kept so a first-run download can also select the model it just installed.
    pub embedding: Option<crate::llm::model_manager::EmbeddingModelAsset>,
}

fn commit_prefix(url: &str) -> String {
    url.split("/resolve/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .map(|commit| commit.chars().take(8).collect())
        .unwrap_or_default()
}

pub fn pick_variant<'a>(model: &'a CatalogModel, wanted: Option<&str>) -> Result<&'a CatalogVariant, String> {
    match wanted {
        Some(id) => model
            .variants
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| format!("{} has no variant {id}", model.catalog_id)),
        None => model.variants.first().ok_or_else(|| format!("{} has no downloadable files", model.catalog_id)),
    }
}

/// Download one variant, verify it, and register it with the right manager.
pub fn install(
    paths: &AppPaths,
    state: &AppState,
    model: &CatalogModel,
    variant: &CatalogVariant,
    cancel: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<Installed, String> {
    install_with(paths, &state.embedding_models, &state.generation_models, model, variant, cancel, on_progress)
}

pub(crate) fn install_with(
    paths: &AppPaths,
    embedding: &crate::llm::model_manager::EmbeddingModelManager,
    generation: &crate::llm::model_manager::GenerationModelManager,
    model: &CatalogModel,
    variant: &CatalogVariant,
    cancel: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<Installed, String> {
    let directory = match model.role {
        ModelRole::Whisper => paths.whisper_models(),
        ModelRole::Embedding => paths.embedding_models(),
        ModelRole::Llm => paths.generation_models(),
    };
    let path = download_verified(&client(), &variant.file, &directory, cancel, on_progress)
        .map_err(|error| error.to_string())?;
    // Unique per variant and upstream commit, because (catalog_id, revision) is unique in the database.
    let revision = format!("{}-{}", variant.id, commit_prefix(&variant.file.url));
    match model.role {
        ModelRole::Whisper => Ok(Installed { path, asset_id: None, embedding: None }),
        ModelRole::Embedding => {
            let dimension = model.embedding.as_ref().map_or(0, |meta| meta.dimension);
            let asset = embedding
                .register_verified(path.clone(), &model.catalog_id, &revision, dimension, &variant.file.sha256)
                .map_err(|e| e.to_string())?;
            Ok(Installed { path, asset_id: Some(asset.id.clone()), embedding: Some(asset) })
        }
        ModelRole::Llm => {
            let asset = generation
                .register_verified(path.clone(), &model.catalog_id, &revision, &variant.file.sha256)
                .map_err(|e| e.to_string())?;
            Ok(Installed { path, asset_id: Some(asset.id), embedding: None })
        }
    }
}

// ---------- commands ----------

fn emit(app: &AppHandle, model: &CatalogModel, variant: &CatalogVariant, state: &'static str, p: Progress, error: Option<String>) {
    let _ = app.emit(
        EVENT,
        ModelDownloadEvent {
            catalog_id: model.catalog_id.clone(),
            display_name: model.display_name.clone(),
            variant_id: variant.id.clone(),
            state,
            received: p.received,
            total: p.total,
            error,
        },
    );
}

/// Blocking: download, register, and report progress events. Shared by the command and
/// the first-run provisioning.
fn run_install(app: &AppHandle, model: &CatalogModel, variant: &CatalogVariant) -> Result<Installed, String> {
    let state = app.state::<AppState>();
    let Some(cancel) = state.downloads.begin(&model.catalog_id) else {
        return Err(format!("{} is already downloading", model.catalog_id));
    };
    let total = variant.file.size_bytes;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let result = install(&state.paths, &state, model, variant, &cancel, |progress| {
        // At most ~8 events a second; the final state is always sent below.
        if last_emit.elapsed() >= Duration::from_millis(120) {
            last_emit = Instant::now();
            emit(app, model, variant, "running", progress, None);
        }
    });
    state.downloads.finish(&model.catalog_id);
    match &result {
        Ok(_) => emit(app, model, variant, "done", Progress { received: total, total }, None),
        Err(message) if message == &DownloadError::Canceled.to_string() => {
            emit(app, model, variant, "canceled", Progress { received: 0, total }, None)
        }
        Err(message) => emit(app, model, variant, "failed", Progress { received: 0, total }, Some(message.clone())),
    }
    result
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog_id: String,
    variant_id: Option<String>,
) -> Result<(), String> {
    let snapshot = state.catalog.snapshot();
    let model = snapshot
        .find(&catalog_id)
        .cloned()
        .ok_or_else(|| format!("{catalog_id} is not in the model catalog"))?;
    let variant = pick_variant(&model, variant_id.as_deref())?.clone();
    tauri::async_runtime::spawn_blocking(move || run_install(&app, &model, &variant).map(|_| ()))
        .await
        .map_err(|e| e.to_string())?
}

/// Stops a running download; the partial file is kept so it can resume later.
#[tauri::command]
pub fn cancel_model_download(state: State<'_, AppState>, catalog_id: String) -> bool {
    state.downloads.cancel(&catalog_id)
}

// ---------- first-run provisioning ----------

#[derive(Debug, Default)]
pub struct Plan {
    pub downloads: Vec<(CatalogModel, CatalogVariant)>,
    /// Defaults that are needed but not (yet) in the catalog.
    pub unavailable: Vec<&'static str>,
}

fn default_variant<'a>(model: &'a CatalogModel, preferred: &str) -> &'a CatalogVariant {
    model
        .variants
        .iter()
        .find(|v| v.id == preferred)
        .unwrap_or(&model.variants[0])
}

pub fn plan_provisioning(snapshot: &CatalogSnapshot, whisper_present: bool, embedding_present: bool) -> Plan {
    let mut plan = Plan::default();
    for (needed, (id, variant), name) in [
        (!whisper_present, DEFAULT_WHISPER, "whisper"),
        (!embedding_present, DEFAULT_EMBEDDING, "embedding"),
    ] {
        if !needed {
            continue;
        }
        match snapshot.find(id) {
            Some(model) if !model.variants.is_empty() => {
                plan.downloads.push((model.clone(), default_variant(model, variant).clone()));
            }
            _ => plan.unavailable.push(name),
        }
    }
    plan
}

fn setting(app: &AppHandle, key: &'static str) -> Option<String> {
    app.state::<AppState>()
        .database
        .run(move |connection| {
            connection
                .query_row("SELECT value_json FROM settings WHERE key=?1", [key], |row| row.get::<_, String>(0))
                .optional()
                .map_err(|e| e.to_string())
        })
        .ok()
        .flatten()
}

fn mark_provisioned(app: &AppHandle) {
    let _ = app.state::<AppState>().database.run(|connection| {
        connection
            .execute(
                "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, 'true', ?2) ON CONFLICT(key) DO UPDATE SET value_json='true', updated_at=excluded.updated_at",
                (PROVISIONED_KEY, Utc::now().to_rfc3339()),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    });
}

fn embedding_installed(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .database
        .run(|connection| {
            connection
                .query_row("SELECT COUNT(*) FROM model_assets WHERE role='embedding' AND state='ready'", [], |row| row.get::<_, i64>(0))
                .map_err(|e| e.to_string())
        })
        .map(|count| count > 0)
        .unwrap_or(false)
}

/// Fetch the catalog, then download whichever default models are missing. Runs on its own
/// thread so startup never waits for the network. A failure leaves the "provisioned" flag
/// unset, so the next launch tries again; once everything needed is present it stops for good.
pub fn spawn_first_run_provisioning(app: AppHandle) {
    if std::env::var_os(ENV_NO_AUTO_DOWNLOAD).is_some() {
        return;
    }
    std::thread::spawn(move || {
        if setting(&app, PROVISIONED_KEY).as_deref() == Some("true") {
            return;
        }
        let state = app.state::<AppState>();
        let whisper_present = crate::paths::find_whisper_model(
            &state.paths,
            None,
            std::env::var_os(crate::paths::ENV_WHISPER_MODEL).map(PathBuf::from),
        )
        .is_some();
        let embedding_present = embedding_installed(&app);
        if whisper_present && embedding_present {
            mark_provisioned(&app);
            return;
        }
        let report = state.catalog.refresh();
        if !report.updated.is_empty() {
            let _ = app.emit("model-catalog-updated", &report.updated);
        }
        let plan = plan_provisioning(&state.catalog.snapshot(), whisper_present, embedding_present);
        let mut failed = false;
        for (model, variant) in &plan.downloads {
            match run_install(&app, model, variant) {
                Ok(installed) => {
                    if let Some(asset) = &installed.embedding {
                        if setting(&app, "llm.embedding_selection").is_none() {
                            let _ = state.embedding_models.select(asset);
                        }
                    }
                }
                Err(error) => {
                    failed = true;
                    eprintln!("locus: first-run download of {} failed: {error}", model.catalog_id);
                }
            }
        }
        if !failed && plan.unavailable.is_empty() {
            mark_provisioned(&app);
        } else if !plan.unavailable.is_empty() {
            eprintln!("locus: default models not in the catalog yet: {:?}", plan.unavailable);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::types::{EmbeddingMeta, VerifiedFile},
        llm::model_manager::{EmbeddingModelManager, GenerationModelManager},
    };
    use sha2::{Digest, Sha256};
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::Arc,
        thread,
    };

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn data() -> Vec<u8> {
        (0..40_000_u32).map(|i| (i % 253) as u8).collect()
    }

    /// Plain-http file server on a random port.
    fn serve() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/resolve/{COMMIT}/file.bin", listener.local_addr().unwrap());
        let body = Arc::new(data());
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        url
    }

    fn variant(id: &str, filename: &str, url: String) -> CatalogVariant {
        let bytes = data();
        CatalogVariant {
            id: id.into(),
            quantization: Some(id.into()),
            file: VerifiedFile { filename: filename.into(), url, size_bytes: bytes.len() as u64, sha256: format!("{:x}", Sha256::digest(&bytes)) },
            ram_bytes: 1,
        }
    }

    fn model(role: ModelRole, id: &str, variants: Vec<CatalogVariant>) -> CatalogModel {
        CatalogModel {
            catalog_id: id.into(),
            display_name: id.into(),
            role,
            author: "a".into(),
            description: None,
            downloads: 0,
            tags: vec![],
            revision: None,
            variants,
            embedding: (role == ModelRole::Embedding).then(|| EmbeddingMeta { dimension: 384, max_sequence_tokens: 256, query_prefix: String::new(), document_prefix: String::new() }),
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        paths: AppPaths,
        embedding: EmbeddingModelManager,
        generation: GenerationModelManager,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        let embedding = EmbeddingModelManager::new(paths.embedding_models());
        let generation = GenerationModelManager::new(paths.generation_models());
        Fixture { _root: root, paths, embedding, generation }
    }

    #[test]
    fn a_whisper_download_lands_where_the_transcriber_looks_for_models() {
        let f = fixture();
        let m = model(ModelRole::Whisper, "whisper-small", vec![variant("standard", "ggml-small.bin", serve())]);
        let installed = install_with(&f.paths, &f.embedding, &f.generation, &m, &m.variants[0], &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(installed.path, f.paths.whisper_models().join("ggml-small.bin"));
        assert_eq!(installed.asset_id, None);
        let found = crate::paths::find_whisper_model(&f.paths, None, None).unwrap();
        assert_eq!(found, installed.path);
    }

    #[test]
    fn an_embedding_download_is_registered_with_its_dimension_without_rehashing() {
        let f = fixture();
        let m = model(ModelRole::Embedding, "all-minilm-l6-v2", vec![variant("q8_0", "minilm.Q8_0.gguf", serve())]);
        let installed = install_with(&f.paths, &f.embedding, &f.generation, &m, &m.variants[0], &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(installed.path, f.paths.embedding_models().join("minilm.Q8_0.gguf"));
        assert_eq!(installed.asset_id.as_deref(), Some("embedding:all-minilm-l6-v2:q8_0-01234567"));
    }

    #[test]
    fn an_llm_download_is_registered_as_a_generation_asset() {
        let f = fixture();
        let m = model(ModelRole::Llm, "unsloth/gemma-3-4b-it", vec![variant("q4_k_m", "gemma-Q4_K_M.gguf", serve())]);
        let installed = install_with(&f.paths, &f.embedding, &f.generation, &m, &m.variants[0], &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(installed.path, f.paths.generation_models().join("gemma-Q4_K_M.gguf"));
        assert_eq!(installed.asset_id.as_deref(), Some("generation:unsloth/gemma-3-4b-it:q4_k_m-01234567"));
    }

    #[test]
    fn a_tampered_download_is_not_installed_or_registered() {
        let f = fixture();
        let mut v = variant("q8_0", "minilm.Q8_0.gguf", serve());
        v.file.sha256 = "0".repeat(64);
        let m = model(ModelRole::Embedding, "all-minilm-l6-v2", vec![v]);
        let error = install_with(&f.paths, &f.embedding, &f.generation, &m, &m.variants[0], &AtomicBool::new(false), |_| {}).unwrap_err();
        assert!(error.contains("checksum mismatch"), "{error}");
        assert!(!f.paths.embedding_models().join("minilm.Q8_0.gguf").exists());
    }

    #[test]
    fn a_canceled_download_reports_the_canceled_message() {
        let f = fixture();
        let m = model(ModelRole::Whisper, "whisper-small", vec![variant("standard", "ggml-small.bin", serve())]);
        let error = install_with(&f.paths, &f.embedding, &f.generation, &m, &m.variants[0], &AtomicBool::new(true), |_| {}).unwrap_err();
        assert_eq!(error, DownloadError::Canceled.to_string());
    }

    #[test]
    fn variant_choice_defaults_to_the_smallest_and_rejects_unknown_ids() {
        let m = model(ModelRole::Llm, "x", vec![variant("q4_k_m", "a.gguf", "u".into()), variant("q8_0", "b.gguf", "u".into())]);
        assert_eq!(pick_variant(&m, None).unwrap().id, "q4_k_m");
        assert_eq!(pick_variant(&m, Some("q8_0")).unwrap().id, "q8_0");
        assert!(pick_variant(&m, Some("q2_k")).is_err());
    }

    #[test]
    fn provisioning_plans_only_what_is_missing_and_reports_what_the_catalog_lacks() {
        let whisper = model(ModelRole::Whisper, DEFAULT_WHISPER.0, vec![variant("q5_1", "a.bin", "u".into()), variant("standard", "b.bin", "u".into())]);
        let embedding = model(ModelRole::Embedding, DEFAULT_EMBEDDING.0, vec![variant("q5_k_m", "c.gguf", "u".into())]);
        let snapshot = CatalogSnapshot { whisper: vec![whisper], embedding: vec![embedding], ..Default::default() };

        let both = plan_provisioning(&snapshot, false, false);
        assert_eq!(both.downloads.len(), 2);
        assert_eq!(both.downloads[0].1.id, "standard", "the named default variant");
        assert_eq!(both.downloads[1].1.id, "q5_k_m", "falls back when q8_0 is not offered");
        assert!(both.unavailable.is_empty());

        let only_embedding = plan_provisioning(&snapshot, true, false);
        assert_eq!(only_embedding.downloads.len(), 1);
        assert_eq!(only_embedding.downloads[0].0.catalog_id, DEFAULT_EMBEDDING.0);

        assert!(plan_provisioning(&snapshot, true, true).downloads.is_empty());

        let empty = plan_provisioning(&CatalogSnapshot::default(), false, false);
        assert!(empty.downloads.is_empty());
        assert_eq!(empty.unavailable, ["whisper", "embedding"]);
    }

    #[test]
    fn download_events_serialize_for_the_frontend() {
        let event = ModelDownloadEvent { catalog_id: "a".into(), display_name: "A".into(), variant_id: "b".into(), state: "running", received: 5, total: 10, error: None };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"catalog_id":"a","display_name":"A","variant_id":"b","state":"running","received":5,"total":10}"#
        );
    }
}
