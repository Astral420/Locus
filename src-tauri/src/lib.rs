pub mod capture;
pub mod commands;
pub mod contracts;
pub mod db;
pub mod deletion;
pub mod keychain;
pub mod knowledge;
pub mod llm;
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
    pub database: Database,
    pub transcription: transcription::TranscriptionService,
    pub providers: llm::providers::ProviderSelectionService,
    pub summarization: llm::summarization::SummarizationService,
    pub knowledge: knowledge::KnowledgeService,
    pub sidecar: Arc<Mutex<SidecarManager>>,
    pub embedding_models: EmbeddingModelManager,
    pub generation_models: GenerationModelManager,
    pub storage: storage::StorageService,
    pub pipeline: PipelineOrchestrator,
    pub embedding_server: Arc<Mutex<Option<Arc<llm::llama_cpp::LlamaServerManager>>>>,
}

pub fn run() {
    let database = Database::open("locus.sqlite").expect("database worker must start");
    let _ = deletion::retry_pending_cleanup(&database, "media");
    let sidecar = Arc::new(Mutex::new(SidecarManager::new("python3", ".")));
    let embedding_models =
        EmbeddingModelManager::new("models/embedding").with_database(database.clone());
    let generation_models =
        GenerationModelManager::new("models/generation").with_database(database.clone());
    let storage = storage::StorageService::new(database.clone(), "media", "models");
    let _ = storage.recover_incomplete();
    let pipeline = PipelineOrchestrator::new(database.clone());
    let state = AppState {
        capture: CaptureManager::with_audio(Some(database.clone())),
        media_root: PathBuf::from("media"),
        database: database.clone(),
        transcription: transcription::TranscriptionService::new(database.clone()),
        providers: llm::providers::ProviderSelectionService::new(database.clone()),
        summarization: llm::summarization::SummarizationService::new(database.clone()),
        knowledge: knowledge::KnowledgeService::new(
            database.clone(),
            Arc::clone(&sidecar),
            "documents",
        ),
        sidecar,
        embedding_models,
        generation_models,
        storage,
        pipeline,
        embedding_server: Arc::new(Mutex::new(None)),
    };
    state.capture.start_watchdog();
    tauri::Builder::default()
        .manage(state)
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
            commands::models::select_model,
            commands::models::get_gpu_backend,
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
