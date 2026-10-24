use crate::{
    knowledge::{
        DocumentDto, IndexStatusDto, KnowledgeMessageDto, KnowledgeScope, KnowledgeThreadDto,
        LlamaEmbeddingProvider, SearchResultDto, UploadDocumentResult,
    },
    llm::{
        backend::{detect_llama_backend, LlamaBackend},
        engine::resolve_llama_server,
        llama_cpp::{LlamaServerConfig, LlamaServerManager},
        model_manager::{EmbeddingModelCatalogEntry, EmbeddingModelManager},
        providers::build_provider,
    },
    AppState,
};
use base64::Engine;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKnowledgeThreadInput {
    pub scope: KnowledgeScope,
    pub meeting_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadDocumentInput {
    pub filename: String,
    pub media_type: Option<String>,
    pub content_base64: String,
    pub meeting_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelDto {
    pub id: String,
    pub display_name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub ram_bytes: u64,
    pub dimension: usize,
    pub installed: bool,
    pub selected: bool,
    pub setup_skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEmbeddingModelInput {
    pub source_path: String,
    pub catalog_id: String,
    pub artifact_revision: String,
    pub dimension: usize,
    pub expected_sha256: Option<String>,
}

#[tauri::command]
pub fn list_documents(state: State<'_, AppState>) -> Result<Vec<DocumentDto>, String> {
    state.knowledge.list_documents()
}

#[tauri::command]
pub fn upload_document(
    state: State<'_, AppState>,
    input: UploadDocumentInput,
) -> Result<UploadDocumentResult, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(input.content_base64)
        .map_err(|_| "document payload is not valid base64".to_string())?;
    state.knowledge.ingest_document(
        &input.filename,
        input.media_type.as_deref(),
        &bytes,
        input.meeting_id.as_deref(),
    )
}

#[tauri::command]
pub fn link_document(
    state: State<'_, AppState>,
    document_id: String,
    meeting_id: String,
) -> Result<(), String> {
    state.knowledge.link_document(&document_id, &meeting_id)
}

#[tauri::command]
pub fn unlink_document(
    state: State<'_, AppState>,
    document_id: String,
    meeting_id: String,
) -> Result<(), String> {
    state.knowledge.unlink_document(&document_id, &meeting_id)
}

#[tauri::command]
pub fn create_knowledge_thread(
    state: State<'_, AppState>,
    input: CreateKnowledgeThreadInput,
) -> Result<KnowledgeThreadDto, String> {
    state
        .knowledge
        .create_thread(input.scope, input.meeting_id.as_deref())
}

#[tauri::command]
pub fn list_knowledge_threads(
    state: State<'_, AppState>,
) -> Result<Vec<KnowledgeThreadDto>, String> {
    state.knowledge.list_threads()
}

#[tauri::command]
pub fn delete_knowledge_thread(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<(), String> {
    state.knowledge.delete_thread(&thread_id)
}

#[tauri::command]
pub fn list_knowledge_messages(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<Vec<KnowledgeMessageDto>, String> {
    state.knowledge.list_messages(&thread_id)
}

#[tauri::command]
pub fn search_knowledge(
    state: State<'_, AppState>,
    scope: KnowledgeScope,
    meeting_id: Option<String>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResultDto>, String> {
    let embedding = embedding_provider(&state)?;
    state.knowledge.search(
        &scope,
        meeting_id.as_deref(),
        &query,
        &embedding,
        limit.unwrap_or(10),
    )
}

#[tauri::command]
pub fn send_knowledge_message(
    state: State<'_, AppState>,
    thread_id: String,
    content: String,
) -> Result<KnowledgeMessageDto, String> {
    let embedding = embedding_provider(&state)?;
    let snapshot = state
        .providers
        .snapshot()
        .map_err(|error| error.to_string())?;
    let keychain = crate::keychain::Keychain::new("com.locus.app");
    let completion = build_provider(&snapshot, &keychain).map_err(|error| error.to_string())?;
    state.knowledge.send_message(
        &thread_id,
        &content,
        &snapshot,
        &embedding,
        completion.as_ref(),
    )
}

#[tauri::command]
pub fn ensure_meeting_knowledge_sources(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<(), String> {
    state.knowledge.ensure_meeting_sources(&meeting_id)
}

#[tauri::command]
pub fn index_knowledge_source(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<IndexStatusDto, String> {
    let embedding = embedding_provider(&state)?;
    state.knowledge.index_source(&source_id, &embedding)
}

#[tauri::command]
pub fn retry_knowledge_index(state: State<'_, AppState>, source_id: String) -> Result<(), String> {
    state.knowledge.retry_index(&source_id)
}

#[tauri::command]
pub fn get_knowledge_index_status(state: State<'_, AppState>) -> Result<IndexStatusDto, String> {
    state.knowledge.index_status()
}

#[tauri::command]
pub fn list_embedding_models(state: State<'_, AppState>) -> Result<Vec<EmbeddingModelDto>, String> {
    let selected = state
        .embedding_models
        .selected()
        .ok()
        .map(|asset| asset.catalog_id);
    let setup_skipped = state
        .database
        .run(|connection| {
            let value: Option<String> = connection
                .query_row(
                    "SELECT value_json FROM settings WHERE key='llm.embedding_setup_skipped'",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            Ok(value.as_deref() == Some("true"))
        })
        .map_err(|error| error.to_string())?;
    let installed = state
        .database
        .run(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT catalog_id FROM model_assets WHERE role='embedding' AND state='ready'",
                )
                .map_err(|error| error.to_string())?;
            let result = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string());
            result
        })
        .map_err(|error| error.to_string())?;
    Ok(EmbeddingModelManagerRows::from_catalog(
        EmbeddingModelManager::catalog(),
        &installed,
        selected.as_deref(),
        setup_skipped,
    ))
}

#[tauri::command]
pub fn skip_embedding_setup(state: State<'_, AppState>) -> Result<(), String> {
    state.embedding_models.mark_setup_skipped()
}

#[tauri::command]
pub fn import_embedding_model(
    state: State<'_, AppState>,
    input: ImportEmbeddingModelInput,
) -> Result<EmbeddingModelDto, String> {
    let source = std::path::PathBuf::from(&input.source_path);
    if !source.is_file() {
        return Err("embedding model import path is not a file".into());
    }
    let asset = state
        .embedding_models
        .import(
            &source,
            &input.catalog_id,
            &input.artifact_revision,
            input.dimension,
            input.expected_sha256.as_deref(),
        )
        .map_err(|error| error.to_string())?;
    Ok(EmbeddingModelDto {
        id: asset.id,
        display_name: input.catalog_id,
        filename: asset
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        size_bytes: std::fs::metadata(&asset.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        ram_bytes: 0,
        dimension: asset.dimension,
        installed: true,
        selected: false,
        setup_skipped: false,
    })
}

#[tauri::command]
pub fn select_embedding_model(
    state: State<'_, AppState>,
    catalog_id: String,
    artifact_revision: String,
) -> Result<(), String> {
    state
        .embedding_models
        .select_installed(&catalog_id, &artifact_revision)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) fn embedding_provider(state: &AppState) -> Result<LlamaEmbeddingProvider, String> {
    let binary = resolve_llama_server().binary;
    let choice = detect_llama_backend();
    eprintln!("llama backend: {:?} ({})", choice.backend, choice.reason);
    embedding_provider_with(
        &state.embedding_models,
        &state.embedding_server,
        binary,
        Some(choice.backend),
    )
}

/// Whether the currently running embedding server may serve the wanted model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmbeddingServerPlan {
    Reuse,
    Relaunch,
}

/// A running server is only reused when it was launched with the wanted model
/// path and still answers health checks; every other state relaunches it.
pub(crate) fn plan_embedding_server(
    current: Option<(Option<&Path>, bool)>,
    wanted: &Path,
) -> EmbeddingServerPlan {
    match current {
        Some((Some(path), true)) if path == wanted => EmbeddingServerPlan::Reuse,
        _ => EmbeddingServerPlan::Relaunch,
    }
}

/// Resolve the embedding provider against the managed assets and the shared
/// server slot. The slot is validated against the selected model on every
/// request; a server running different weights (or a dead one) is replaced.
pub(crate) fn embedding_provider_with(
    models: &EmbeddingModelManager,
    slot: &Mutex<Option<Arc<LlamaServerManager>>>,
    binary: PathBuf,
    llama_backend: Option<LlamaBackend>,
) -> Result<LlamaEmbeddingProvider, String> {
    let asset = models.selected().map_err(|_| {
        "semantic search is unavailable: import or download an embedding model, then select it"
            .to_string()
    })?;
    if !asset.path.is_file() {
        return Err("selected embedding model is unavailable at its managed path".into());
    }
    let server = {
        let mut guard = slot
            .lock()
            .map_err(|_| "embedding server lock is poisoned".to_string())?;
        let plan = plan_embedding_server(
            guard
                .as_ref()
                .map(|server| (server.model_path(), server.health().is_ok())),
            asset.path.as_path(),
        );
        match plan {
            EmbeddingServerPlan::Reuse => match guard.as_ref() {
                Some(server) => Arc::clone(server),
                None => return Err("embedding server plan was inconsistent".into()),
            },
            EmbeddingServerPlan::Relaunch => {
                let mut config = LlamaServerConfig::new(binary);
                config.model_path = Some(asset.path.clone());
                config.embedding = true;
                config.llama_backend = llama_backend.clone();
                let new_server = Arc::new(
                    LlamaServerManager::launch(config)
                        .map_err(|error| format!("embedding engine is unavailable: {error}"))?,
                );
                let old = guard.replace(Arc::clone(&new_server));
                drop(guard);
                drop(old);
                new_server
            }
        }
    };
    LlamaEmbeddingProvider::new(
        asset.id,
        asset.artifact_revision,
        asset.path.to_string_lossy(),
        asset.dimension,
        server,
    )
}

struct EmbeddingModelManagerRows;

impl EmbeddingModelManagerRows {
    fn from_catalog(
        catalog: Vec<EmbeddingModelCatalogEntry>,
        installed: &[String],
        selected: Option<&str>,
        setup_skipped: bool,
    ) -> Vec<EmbeddingModelDto> {
        catalog
            .into_iter()
            .map(|entry| EmbeddingModelDto {
                installed: installed.iter().any(|id| id == &entry.id),
                selected: selected == Some(entry.id.as_str()),
                id: entry.id,
                display_name: entry.display_name,
                filename: entry.filename,
                size_bytes: entry.size_bytes,
                ram_bytes: entry.ram_bytes,
                dimension: entry.dimension,
                setup_skipped,
            })
            .collect()
    }
}

#[cfg(test)]
mod embedding_server_plan_tests {
    use super::{plan_embedding_server, EmbeddingServerPlan};
    use std::path::Path;

    #[test]
    fn plan_reuses_a_healthy_server_for_the_same_model_path() {
        let path = Path::new("/models/a.gguf");
        assert_eq!(
            plan_embedding_server(Some((Some(path), true)), path),
            EmbeddingServerPlan::Reuse
        );
    }

    #[test]
    fn plan_relaunches_when_the_selected_model_path_differs() {
        let running = Path::new("/models/a.gguf");
        let wanted = Path::new("/models/b.gguf");
        assert_eq!(
            plan_embedding_server(Some((Some(running), true)), wanted),
            EmbeddingServerPlan::Relaunch
        );
    }

    #[test]
    fn plan_relaunches_when_the_server_is_unhealthy() {
        let path = Path::new("/models/a.gguf");
        assert_eq!(
            plan_embedding_server(Some((Some(path), false)), path),
            EmbeddingServerPlan::Relaunch
        );
    }

    #[test]
    fn plan_launches_when_the_slot_is_empty() {
        let wanted = Path::new("/models/a.gguf");
        assert_eq!(
            plan_embedding_server(None, wanted),
            EmbeddingServerPlan::Relaunch
        );
    }

    #[test]
    fn plan_relaunches_when_the_server_has_no_model_path() {
        let wanted = Path::new("/models/a.gguf");
        assert_eq!(
            plan_embedding_server(Some((None, true)), wanted),
            EmbeddingServerPlan::Relaunch
        );
    }
}

#[cfg(all(test, unix))]
mod embedding_server_tests {
    use super::{embedding_provider_with, LlamaEmbeddingProvider, LlamaServerManager};
    use crate::db::Database;
    use crate::knowledge::EmbeddingProvider;
    use crate::llm::model_manager::{EmbeddingModelAsset, EmbeddingModelManager};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    const FAKE_SERVER: &str = r##"#!/usr/bin/env python3
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
model = ""
index = 0
while index < len(args):
    if args[index] == "--port" and index + 1 < len(args):
        port = int(args[index + 1])
        index += 2
    elif args[index] == "--model" and index + 1 < len(args):
        model = args[index + 1]
        index += 2
    else:
        index += 1

base = os.path.basename(model)
code = sum(ord(character) for character in base)
vector = [float(code), float(len(base)), 1.0, 0.0]

log_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "launches.log")
with open(log_path, "a") as log:
    log.write(model + "\n")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def _reply(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._reply(200, {"status": "ok"})
        else:
            self._reply(404, {})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0) or 0)
        if length:
            self.rfile.read(length)
        if self.path == "/embedding":
            self._reply(200, {"embedding": vector})
        elif self.path == "/shutdown":
            self._reply(200, {"status": "ok"})
            threading.Thread(target=self.server.shutdown, daemon=True).start()
        else:
            self._reply(404, {})


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"##;

    fn python3_available() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn write_fake_server(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-llama-server.py");
        fs::write(&path, FAKE_SERVER).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn tagged_vector(path: &Path) -> Vec<f32> {
        let base = path.file_name().unwrap().to_string_lossy().to_string();
        let code: u32 = base.bytes().map(u32::from).sum();
        vec![code as f32, base.len() as f32, 1.0, 0.0]
    }

    fn wait_until_unreachable(url: &str, timeout: Duration) -> bool {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap();
        let deadline = Instant::now() + timeout;
        loop {
            if client.get(format!("{url}/health")).send().is_err() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        binary: PathBuf,
        manager: EmbeddingModelManager,
        slot: Mutex<Option<Arc<LlamaServerManager>>>,
    }

    fn fixture() -> Option<Fixture> {
        if !python3_available() {
            eprintln!("skipping embedding server test: python3 is not available");
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_fake_server(dir.path());
        let manager = EmbeddingModelManager::new(dir.path().join("models"))
            .with_database(Database::in_memory().unwrap());
        Some(Fixture {
            dir,
            binary,
            manager,
            slot: Mutex::new(None),
        })
    }

    impl Fixture {
        fn register(&self, filename: &str, dimension: usize) -> EmbeddingModelAsset {
            let path = self.dir.path().join("models").join(filename);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("weights:{filename}")).unwrap();
            self.manager
                .register_verified(path, filename, "r1", dimension, "verified")
                .unwrap()
        }

        fn select(&self, asset: &EmbeddingModelAsset) {
            self.manager.select(asset).unwrap();
        }

        fn provider(&self) -> Result<LlamaEmbeddingProvider, String> {
            embedding_provider_with(&self.manager, &self.slot, self.binary.clone(), None)
        }

        fn launches(&self) -> usize {
            fs::read_to_string(self.dir.path().join("launches.log"))
                .map(|contents| contents.lines().count())
                .unwrap_or(0)
        }

        fn running_model_path(&self) -> Option<PathBuf> {
            self.slot
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|server| server.model_path().map(Path::to_path_buf))
        }

        fn running_base_url(&self) -> String {
            self.slot
                .lock()
                .unwrap()
                .as_ref()
                .expect("a server should be running")
                .base_url()
                .to_string()
        }
    }

    #[test]
    fn switching_the_selected_embedding_model_relaunches_the_server_with_the_new_weights() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf", 4);
        let b = fixture.register("beta.gguf", 4);
        fixture.select(&a);
        let provider_a = fixture.provider().unwrap();
        assert_eq!(provider_a.embed("query").unwrap(), tagged_vector(&a.path));
        fixture.select(&b);
        let provider_b = fixture.provider().unwrap();
        assert_eq!(provider_b.embed("query").unwrap(), tagged_vector(&b.path));
        assert_eq!(fixture.running_model_path(), Some(b.path.clone()));
        assert_eq!(fixture.launches(), 2);
    }

    #[test]
    fn the_same_selection_reuses_the_running_server() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf", 4);
        fixture.select(&a);
        let first = fixture.provider().unwrap();
        let second = fixture.provider().unwrap();
        assert_eq!(first.embed("query").unwrap(), tagged_vector(&a.path));
        assert_eq!(second.embed("query").unwrap(), tagged_vector(&a.path));
        assert_eq!(fixture.launches(), 1);
    }

    #[test]
    fn an_in_flight_provider_keeps_working_on_the_old_server_after_a_switch() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf", 4);
        let b = fixture.register("beta.gguf", 4);
        fixture.select(&a);
        let provider_a = fixture.provider().unwrap();
        fixture.select(&b);
        let provider_b = fixture.provider().unwrap();
        assert_eq!(provider_b.embed("query").unwrap(), tagged_vector(&b.path));
        assert_eq!(provider_a.embed("query").unwrap(), tagged_vector(&a.path));
    }

    #[test]
    fn the_old_server_stops_after_its_last_provider_is_dropped() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf", 4);
        let b = fixture.register("beta.gguf", 4);
        fixture.select(&a);
        let provider_a = fixture.provider().unwrap();
        let old_url = fixture.running_base_url();
        fixture.select(&b);
        let provider_b = fixture.provider().unwrap();
        assert_eq!(provider_b.embed("query").unwrap(), tagged_vector(&b.path));
        drop(provider_a);
        assert!(
            wait_until_unreachable(&old_url, Duration::from_secs(5)),
            "the old embedding server should stop once its last provider is dropped"
        );
    }

    #[test]
    fn a_dead_server_is_relaunched() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf", 4);
        fixture.select(&a);
        let provider_a = fixture.provider().unwrap();
        assert_eq!(provider_a.embed("query").unwrap(), tagged_vector(&a.path));
        let dead = fixture.slot.lock().unwrap().as_ref().unwrap().clone();
        dead.shutdown().unwrap();
        let provider_again = fixture.provider().unwrap();
        assert_eq!(
            provider_again.embed("query").unwrap(),
            tagged_vector(&a.path)
        );
        assert_eq!(fixture.launches(), 2);
    }

    #[test]
    fn a_failed_launch_leaves_the_slot_unchanged() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf", 4);
        let b = fixture.register("beta.gguf", 4);
        fixture.select(&a);
        let _provider_a = fixture.provider().unwrap();
        fixture.select(&b);
        let missing = fixture.dir.path().join("does-not-exist-llama-server");
        let error = match embedding_provider_with(&fixture.manager, &fixture.slot, missing, None) {
            Ok(_) => panic!("launching with a missing binary should fail"),
            Err(error) => error,
        };
        assert!(error.contains("embedding engine is unavailable"), "{error}");
        assert_eq!(fixture.running_model_path(), Some(a.path.clone()));
        assert_eq!(fixture.launches(), 1);
    }
}
