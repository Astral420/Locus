use crate::{
    knowledge::{
        DocumentDto, IndexStatusDto, KnowledgeMessageDto, KnowledgeScope, KnowledgeThreadDto,
        SearchResultDto, UploadDocumentResult,
    },
    llm::{
        llama_cpp::LlamaServerConfig,
        model_manager::{EmbeddingModelCatalogEntry, EmbeddingModelManager},
        providers::build_provider,
    },
    AppState,
};
use base64::Engine;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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

fn embedding_provider(
    state: &AppState,
) -> Result<crate::knowledge::LlamaEmbeddingProvider, String> {
    let asset = state.embedding_models.selected().map_err(|_| {
        "semantic search is unavailable: import or download an embedding model, then select it"
            .to_string()
    })?;
    if !asset.path.is_file() {
        return Err("selected embedding model is unavailable at its managed path".into());
    }
    let mut runtime = state
        .embedding_server
        .lock()
        .map_err(|_| "embedding server lock is poisoned".to_string())?;
    let server = if let Some(server) = runtime.as_ref() {
        Arc::clone(server)
    } else {
        let binary = std::env::var_os("LOCUS_LLAMA_SERVER_BIN")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("llama-server"));
        let mut config = LlamaServerConfig::new(binary);
        config.model_path = Some(asset.path.clone());
        config.embedding = true;
        let server = Arc::new(
            crate::llm::llama_cpp::LlamaServerManager::launch(config)
                .map_err(|error| format!("embedding engine is unavailable: {error}"))?,
        );
        *runtime = Some(Arc::clone(&server));
        server
    };
    crate::knowledge::LlamaEmbeddingProvider::new(
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
