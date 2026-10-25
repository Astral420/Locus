use crate::{
    contracts::MeetingType,
    llm::{
        generation_server::build_runtime_provider,
        providers::{ProviderSelectionService, ProviderSnapshot, RedactedProviderConfig},
        summarization::{ActionItemDto, SummarizationService, SummaryRevisionDto},
        ProviderKind,
    },
    AppState,
};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectProviderInput {
    pub provider: ProviderKind,
    pub model: String,
    pub destination: String,
    pub accept_disclosure: bool,
}

#[tauri::command]
pub fn configure_provider(
    state: State<'_, AppState>,
    provider: ProviderKind,
    model: String,
    destination: String,
    configured: bool,
) -> Result<(), String> {
    state
        .providers
        .configure(provider, &model, &destination, configured)
}

#[tauri::command]
pub fn save_provider_api_key(
    state: State<'_, AppState>,
    provider: ProviderKind,
    api_key: String,
) -> Result<(), String> {
    let keychain = crate::keychain::Keychain::new("com.locus.app");
    state.providers.save_api_key(&keychain, provider, &api_key)
}

#[tauri::command]
pub fn select_provider(
    state: State<'_, AppState>,
    input: SelectProviderInput,
) -> Result<ProviderSnapshot, String> {
    state
        .providers
        .select(
            input.provider,
            &input.model,
            &input.destination,
            input.accept_disclosure,
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_provider_configs(
    state: State<'_, AppState>,
) -> Result<Vec<RedactedProviderConfig>, String> {
    state.providers.configs()
}

#[tauri::command]
pub fn get_selected_provider(state: State<'_, AppState>) -> Result<ProviderSnapshot, String> {
    state
        .providers
        .snapshot()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_meeting_type(
    state: State<'_, AppState>,
    meeting_id: String,
    meeting_type: MeetingType,
) -> Result<(), String> {
    state
        .summarization
        .set_meeting_type(&meeting_id, meeting_type)
}

#[tauri::command]
pub fn set_meeting_title(
    state: State<'_, AppState>,
    meeting_id: String,
    title: String,
) -> Result<(), String> {
    state.summarization.set_user_title(&meeting_id, &title)
}

/// The runtime integration supplies a concrete provider after selection. Until
/// then this command returns an actionable error and never sends transcript
/// content to a different provider.
///
/// Runs off the UI thread: the first patched-Metal launch compiles shaders and
/// can take up to two minutes, which would otherwise freeze the window.
#[tauri::command]
pub async fn trigger_summary(
    app: tauri::AppHandle,
    meeting_id: String,
    regenerate: bool,
) -> Result<SummaryRevisionDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        summarize_now(&app.state::<AppState>(), &meeting_id, regenerate)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn summarize_now(
    state: &AppState,
    meeting_id: &str,
    regenerate: bool,
) -> Result<SummaryRevisionDto, String> {
    let snapshot = state
        .providers
        .snapshot()
        .map_err(|error| error.to_string())?;
    let keychain = crate::keychain::Keychain::new("com.locus.app");
    let provider =
        build_runtime_provider(state, &snapshot, &keychain).map_err(|error| error.to_string())?;
    state.summarization.summarize_with_provider(
        meeting_id,
        &snapshot,
        provider.as_ref(),
        regenerate,
    )
}

#[tauri::command]
pub fn get_summary_revisions(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<SummaryRevisionDto>, String> {
    state.summarization.list_revisions(&meeting_id)
}

#[tauri::command]
pub fn get_action_items(
    state: State<'_, AppState>,
    summary_revision_id: String,
) -> Result<Vec<ActionItemDto>, String> {
    state.summarization.action_items(&summary_revision_id)
}

#[tauri::command]
pub fn toggle_action_item(
    state: State<'_, AppState>,
    action_item_id: String,
    completed: bool,
) -> Result<(), String> {
    state
        .summarization
        .toggle_action_item(&action_item_id, completed)
}

#[tauri::command]
pub async fn retry_summary(
    app: tauri::AppHandle,
    meeting_id: String,
) -> Result<SummaryRevisionDto, String> {
    trigger_summary(app, meeting_id, true).await
}

#[allow(dead_code)]
fn _services_are_send_safe(
    _providers: &ProviderSelectionService,
    _summarization: &SummarizationService,
) {
}
