pub mod capture;
pub mod catalog;
pub mod commands;
pub mod contracts;
pub mod db;
pub mod deletion;
pub mod keychain;
pub mod knowledge;
pub mod llm;
pub mod paths;
pub mod pipeline;
pub mod sidecar;
pub mod storage;
pub mod transcription;
pub mod updates;

use capture::CaptureManager;
use db::Database;
use llm::model_manager::EmbeddingModelManager;
use llm::model_manager::GenerationModelManager;
use pipeline::orchestrator::PipelineOrchestrator;
use sidecar::client::SidecarManager;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub capture: CaptureManager,
    pub media_root: PathBuf,
    pub paths: paths::AppPaths,
    pub executor: pipeline::executor::PipelineExecutor,
    pub database: Database,
    pub transcription: transcription::TranscriptionService,
    pub providers: llm::providers::ProviderSelectionService,
    pub summarization: llm::summarization::SummarizationService,
    pub knowledge: knowledge::KnowledgeService,
    pub sidecar: Arc<Mutex<SidecarManager>>,
    pub catalog: catalog::CatalogService,
    pub downloads: catalog::download::DownloadRegistry,
    pub embedding_models: EmbeddingModelManager,
    pub generation_models: GenerationModelManager,
    pub storage: storage::StorageService,
    pub pipeline: PipelineOrchestrator,
    pub embedding_server: Arc<Mutex<Option<Arc<llm::llama_cpp::LlamaServerManager>>>>,
    /// The generation server, separate from embeddings so a switched chat model
    /// never re-indexes and an embedding model never answers a question.
    pub generation_server: Arc<Mutex<Option<Arc<llm::llama_cpp::LlamaServerManager>>>>,
}

/// Models root recorded by "move models" in Settings, if it still exists.
fn stored_models_root(database: &Database) -> Option<PathBuf> {
    database
        .run(|connection| {
            Ok(connection
                .query_row(
                    "SELECT value_json FROM settings WHERE key='storage.models_root'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .ok())
        })
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<String>(&json).ok())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn build_state(app: &tauri::AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    let mut paths = paths::AppPaths::resolve(data_dir, app.path().resource_dir().ok());
    paths.create_directories()?;
    // Earlier builds kept everything relative to the working directory.
    if let Ok(working_directory) = std::env::current_dir() {
        match paths::adopt_legacy_working_directory_data(&working_directory, &paths) {
            Ok(adopted) if adopted != paths::LegacyAdoption::default() => {
                eprintln!("locus: imported earlier data from {}: {adopted:?}", working_directory.display());
            }
            Ok(_) => {}
            Err(error) => eprintln!("locus: could not import earlier data: {error}"),
        }
    }
    let database = Database::open(&paths.database)?;
    if let Some(models) = stored_models_root(&database) {
        paths.models = models;
        paths.create_directories()?;
    }
    let _ = deletion::retry_pending_cleanup(&database, &paths.media);
    let sidecar = Arc::new(Mutex::new(
        SidecarManager::new(paths.python.clone(), paths.sidecar_root.clone())
            .with_environment(paths.sidecar_environment()),
    ));
    let embedding_models =
        EmbeddingModelManager::new(paths.embedding_models()).with_database(database.clone());
    let generation_models =
        GenerationModelManager::new(paths.generation_models()).with_database(database.clone());
    let storage = storage::StorageService::new(
        database.clone(),
        paths.media.clone(),
        paths.models.clone(),
    );
    let _ = storage.recover_incomplete();
    let catalog = catalog::CatalogService::new(paths.data_dir.join("catalog"));
    Ok(AppState {
        capture: CaptureManager::with_audio(Some(database.clone())),
        media_root: paths.media.clone(),
        transcription: transcription::TranscriptionService::new(database.clone()),
        providers: llm::providers::ProviderSelectionService::new(database.clone()),
        summarization: llm::summarization::SummarizationService::new(database.clone()),
        knowledge: knowledge::KnowledgeService::new(
            database.clone(),
            Arc::clone(&sidecar),
            paths.documents.clone(),
        ),
        sidecar,
        catalog,
        downloads: Default::default(),
        embedding_models,
        generation_models,
        storage,
        pipeline: PipelineOrchestrator::new(database.clone()),
        embedding_server: Arc::new(Mutex::new(None)),
        generation_server: Arc::new(Mutex::new(None)),
        executor: pipeline::executor::PipelineExecutor::new(),
        paths,
        database,
    })
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = build_state(app.handle())?;
            // Recordings are played back through the asset protocol; allow exactly
            // the media directory this build uses.
            let _ = app.asset_protocol_scope().allow_directory(&state.paths.media, true);
            state.capture.start_watchdog();
            app.manage(state);
            app.state::<AppState>().executor.start(app.handle().clone());
            commands::model_downloads::spawn_first_run_provisioning(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if !matches!(state.capture.state().state, contracts::CaptureLifecycle::Idle) {
                    api.prevent_close();
                    let _ = window.emit("capture-backgrounded", serde_json::json!({
                        "message": "Capture continues in the background. Reopen Locus to pause or stop it."
                    }));
                    #[cfg(not(target_os = "linux"))]
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_recording_state,
            commands::recording::prewarm_capture,
            commands::recording::list_screen_sources,
            commands::recording::list_recoverable_captures,
            commands::recording::recover_capture,
            commands::recording::start_recording,
            commands::recording::pause_recording,
            commands::recording::resume_recording,
            commands::recording::stop_recording,
            commands::transcription::get_transcription_status,
            commands::transcription::list_transcript_segments,
            commands::transcription::set_meeting_language,
            commands::transcription::rename_speaker,
            commands::summarization::configure_provider,
            commands::summarization::save_provider_api_key,
            commands::summarization::select_provider,
            commands::summarization::get_provider_configs,
            commands::summarization::get_selected_provider,
            commands::summarization::set_meeting_type,
            commands::summarization::set_meeting_title,
            commands::summarization::trigger_summary,
            commands::summarization::get_summary_revisions,
            commands::summarization::get_action_items,
            commands::summarization::toggle_action_item,
            commands::summarization::retry_summary,
            commands::meetings::list_meetings,
            commands::meetings::get_meeting,
            commands::meetings::delete_meeting,
            commands::meetings::get_pipeline_status,
            commands::meetings::get_meeting_media,
            commands::meetings::retry_pipeline_step,
            commands::meetings::get_slides,
            commands::models::list_models,
            commands::models::refresh_model_catalog,
            commands::model_downloads::download_model,
            commands::model_downloads::cancel_model_download,
            commands::model_uninstall::uninstall_model,
            commands::models::select_model,
            commands::models::get_gpu_backend,
            commands::models::get_llama_backend,
            commands::storage::get_storage_info,
            commands::storage::migrate_models,
            commands::storage::relocate_storage,
            commands::storage::retry_cleanup,
            commands::updates::check_for_update,
            commands::updates::verify_update_package,
            commands::recording::show_recording_window,
            commands::knowledge::list_documents,
            commands::knowledge::upload_document,
            commands::knowledge::link_document,
            commands::knowledge::unlink_document,
            commands::knowledge::create_knowledge_thread,
            commands::knowledge::list_knowledge_threads,
            commands::knowledge::delete_knowledge_thread,
            commands::knowledge::list_knowledge_messages,
            commands::knowledge::search_knowledge,
            commands::knowledge::send_knowledge_message,
            commands::knowledge::ensure_meeting_knowledge_sources,
            commands::knowledge::index_knowledge_source,
            commands::knowledge::retry_knowledge_index,
            commands::knowledge::get_knowledge_index_status,
            commands::knowledge::list_embedding_models,
            commands::knowledge::skip_embedding_setup,
            commands::knowledge::import_embedding_model,
            commands::knowledge::select_embedding_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Locus");
}
