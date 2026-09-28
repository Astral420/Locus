use crate::{
    contracts::{GpuBackendDto, ModelAssetDto},
    llm::model_manager::{EmbeddingModelManager, GenerationModelManager},
    transcription::{detect_backend, vulkan_runtime_ready, BackendKind},
    AppState,
};
use rusqlite::OptionalExtension;
use tauri::State;

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelAssetDto>, String> {
    let installed = state
        .database
        .run(|connection| {
            let mut statement = connection
                .prepare("SELECT id, catalog_id, role, state FROM model_assets")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            rows
        })
        .map_err(|e| e.to_string())?;
    let active_generation: Option<String> = state
        .database
        .run(|connection| {
            connection
                .query_row(
                    "SELECT value_json FROM settings WHERE key='llm.generation_selection'",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())
        })
        .map_err(|e| e.to_string())?;
    let active_embedding: Option<String> = state
        .database
        .run(|connection| {
            connection
                .query_row(
                    "SELECT value_json FROM settings WHERE key='llm.embedding_selection'",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())
        })
        .map_err(|e| e.to_string())?;

    let mut assets = vec![ModelAssetDto {
        id: "whisper:small:bundled".into(),
        name: "Whisper Small (bundled)".into(),
        role: "whisper".into(),
        parameter_size: "244M".into(),
        quantization: None,
        memory_ram: "1 GB".into(),
        memory_vram: "1 GB".into(),
        disk_size: "466 MiB".into(),
        status: "bundled".into(),
        download_progress: None,
    }];
    for model in GenerationModelManager::catalog() {
        let row = installed
            .iter()
            .find(|(_, catalog, role, _)| catalog == &model.id && role == "generation");
        let id = row
            .map(|entry| entry.0.clone())
            .unwrap_or_else(|| format!("generation:{}:catalog", model.id));
        let status = if active_generation
            .as_deref()
            .map(|value| value.trim_matches('"'))
            == Some(id.as_str())
            || active_generation.as_deref() == Some(model.id.as_str())
        {
            "active"
        } else if row.is_some() {
            "installed"
        } else {
            "available"
        };
        assets.push(ModelAssetDto {
            id,
            name: model.display_name,
            role: "llm".into(),
            parameter_size: model.id.clone(),
            quantization: Some("Q4_K_M".into()),
            memory_ram: format_bytes(model.ram_bytes),
            memory_vram: "—".into(),
            disk_size: format_bytes(model.size_bytes),
            status: status.into(),
            download_progress: None,
        });
    }
    for model in EmbeddingModelManager::catalog() {
        let row = installed
            .iter()
            .find(|(_, catalog, role, _)| catalog == &model.id && role == "embedding");
        let id = row
            .map(|entry| entry.0.clone())
            .unwrap_or_else(|| format!("embedding:{}:catalog", model.id));
        let status = if active_embedding
            .as_deref()
            .map(|value| value.trim_matches('"'))
            == Some(id.as_str())
            || active_embedding.as_deref() == Some(model.id.as_str())
        {
            "active"
        } else if row.is_some() {
            "installed"
        } else {
            "available"
        };
        assets.push(ModelAssetDto {
            id,
            name: model.display_name,
            role: "embedding".into(),
            parameter_size: format!("{}d", model.dimension),
            quantization: Some("Q8_0".into()),
            memory_ram: format_bytes(model.ram_bytes),
            memory_vram: "—".into(),
            disk_size: format_bytes(model.size_bytes),
            status: status.into(),
            download_progress: None,
        });
    }
    Ok(assets)
}

#[tauri::command]
pub fn select_model(
    state: State<'_, AppState>,
    model_id: String,
    role: String,
) -> Result<(), String> {
    if model_id.trim().is_empty() || !matches!(role.as_str(), "whisper" | "llm" | "embedding") {
        return Err("a valid model id and role are required".into());
    }
    let key = match role.as_str() {
        "llm" => "llm.generation_selection",
        "embedding" => "llm.embedding_selection",
        _ => "transcription.whisper_selection",
    };
    let value = serde_json::to_string(&model_id).map_err(|e| e.to_string())?;
    state.database.run(move |connection| {
        connection.execute("INSERT INTO settings(key,value_json,updated_at) VALUES (?1,?2,datetime('now')) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at", (&key, &value)).map(|_| ()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_gpu_backend(_state: State<'_, AppState>) -> GpuBackendDto {
    let backend = detect_backend();
    let name = match backend {
        BackendKind::Metal => "metal",
        BackendKind::Cuda => "cuda",
        BackendKind::Rocm => "rocm",
        BackendKind::Vulkan => "vulkan",
        BackendKind::Cpu => "cpu",
    };
    let reason = match backend {
        BackendKind::Vulkan => "Intel Mac AMD dGPU detected; packaged Vulkan runtime selected with MoltenVK workarounds".into(),
        BackendKind::Metal => "Apple GPU selected by the validated whisper.cpp build".into(),
        BackendKind::Cuda => "CUDA device marker detected".into(),
        BackendKind::Rocm => "ROCm device marker detected".into(),
        BackendKind::Cpu => "No validated GPU runtime is available; CPU correctness baseline selected".into(),
    };
    GpuBackendDto {
        backend: name.into(),
        architecture: std::env::consts::ARCH.into(),
        device: std::env::var("LOCUS_VULKAN_DEVICE").ok(),
        reason,
        vulkan_ready: vulkan_runtime_ready(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    }
}
