use crate::{
    catalog::{
        model::{CatalogModel, CatalogVariant, ModelRole},
        CatalogSnapshot,
    },
    commands::{
        model_downloads::{DEFAULT_EMBEDDING, DEFAULT_WHISPER},
        model_state::{self, InstalledAsset, LocalModels, VariantState},
    },
    contracts::{CatalogRefreshDto, GpuBackendDto, ModelAssetDto, ModelVariantDto},
    db::Database,
    llm::model_manager::{EmbeddingModelManager, GenerationModelManager},
    transcription::{detect_backend, vulkan_runtime_ready, BackendKind},
    AppState,
};
use rusqlite::OptionalExtension;
use tauri::State;

/// The chat catalog holds thousands of repositories; the UI gets the best-ranked slice
/// (recommended models first, then most downloaded) until search exists.
const LLM_LIST_LIMIT: usize = 60;

fn stored_setting(state: &AppState, key: &'static str) -> Result<Option<String>, String> {
    state
        .database
        .run(move |connection| {
            connection
                .query_row("SELECT value_json FROM settings WHERE key=?1", [key], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelAssetDto>, String> {
    let assets: Vec<InstalledAsset> = state
        .database
        .run(|connection| {
            let mut statement = connection
                .prepare("SELECT id, catalog_id, role, state, artifact_revision FROM model_assets")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok(InstalledAsset {
                        id: row.get(0)?,
                        catalog_id: row.get(1)?,
                        role: row.get(2)?,
                        state: row.get(3)?,
                        artifact_revision: row.get(4)?,
                    })
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            rows
        })
        .map_err(|e| e.to_string())?;

    let active_generation = stored_setting(&state, "llm.generation_selection")?;
    let active_embedding = stored_setting(&state, "llm.embedding_selection")?;
    let whisper_selection = stored_setting(&state, "transcription.whisper_selection")?;

    // Reflect what is really on disk: the model is a release/maintainer input
    // that may not be present, and the pipeline blocks without it.
    let active_whisper_file = crate::paths::find_whisper_model(
        &state.paths,
        whisper_selection.as_deref(),
        std::env::var_os(crate::paths::ENV_WHISPER_MODEL).map(std::path::PathBuf::from),
    )
    .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()));

    let whisper_files: Vec<String> = state
        .paths
        .whisper_search_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    // Reads the cached catalog only; refresh_model_catalog is what uses the network.
    let snapshot = state.catalog.snapshot();
    let local = LocalModels {
        assets: &assets,
        whisper_files: &whisper_files,
        active_whisper_file: active_whisper_file.as_deref(),
        active_generation: active_generation.as_deref(),
        active_embedding: active_embedding.as_deref(),
    };
    Ok(build_model_list(&snapshot, &local))
}

/// Downloads newer copies of the model catalog files. Cached copies are kept when the
/// network or the published files are bad, so this is safe to call at any time.
#[tauri::command]
pub async fn refresh_model_catalog(
    state: State<'_, AppState>,
) -> Result<CatalogRefreshDto, String> {
    let catalog = state.catalog.clone();
    let report = tauri::async_runtime::spawn_blocking(move || catalog.refresh())
        .await
        .map_err(|e| e.to_string())?;
    Ok(CatalogRefreshDto {
        updated: report.updated,
        unchanged: report.unchanged,
        failed: report
            .failed
            .into_iter()
            .map(|(file, reason)| format!("{file}: {reason}"))
            .collect(),
    })
}

pub(crate) fn build_model_list(
    snapshot: &CatalogSnapshot,
    local: &LocalModels,
) -> Vec<ModelAssetDto> {
    let mut assets = Vec::new();

    // A fallback row keeps Whisper Small visible when the catalog is empty (offline
    // first launch); with a catalog every whisper model is a normal catalog row.
    if snapshot.whisper.is_empty() {
        assets.push(fallback_whisper_row(local));
    } else {
        for model in &snapshot.whisper {
            assets.push(catalog_row(model, local));
        }
    }

    if snapshot.llm.is_empty() {
        // No verified catalog cached yet: keep the built-in placeholders.
        for model in GenerationModelManager::catalog() {
            assets.push(placeholder_row(
                local,
                &model.id,
                &model.display_name,
                ModelRole::Llm,
                model.id.clone(),
                "Q4_K_M",
                model.ram_bytes,
                model.size_bytes,
                false,
            ));
        }
    } else {
        for model in snapshot.llm.iter().take(LLM_LIST_LIMIT) {
            assets.push(catalog_row(model, local));
        }
    }

    if snapshot.embedding.is_empty() {
        for model in EmbeddingModelManager::catalog() {
            assets.push(placeholder_row(
                local,
                &model.id,
                &model.display_name,
                ModelRole::Embedding,
                format!("{}d", model.dimension),
                "Q8_0",
                model.ram_bytes,
                model.size_bytes,
                model.id == DEFAULT_EMBEDDING.0,
            ));
        }
    } else {
        for model in &snapshot.embedding {
            assets.push(catalog_row(model, local));
        }
    }
    assets
}

fn is_default_model(catalog_id: &str) -> bool {
    catalog_id == DEFAULT_WHISPER.0 || catalog_id == DEFAULT_EMBEDDING.0
}

/// The DTO id of a model whose headline variant is not installed yet.
fn placeholder_id(model: &CatalogModel) -> String {
    match model.role {
        ModelRole::Llm => format!("generation:{}:catalog", model.catalog_id),
        ModelRole::Embedding => format!("embedding:{}:catalog", model.catalog_id),
        ModelRole::Whisper => format!("whisper:{}:catalog", whisper_selector(&model.catalog_id)),
    }
}

/// 1. the active variant, 2. the first installed variant, 3. the preferred one.
fn headline_index(model: &CatalogModel, states: &[VariantState]) -> usize {
    if let Some(index) = states.iter().position(|state| state.active) {
        return index;
    }
    if let Some(index) = states.iter().position(|state| state.installed) {
        return index;
    }
    let preferred = match model.role {
        ModelRole::Llm => "q4_k_m",
        ModelRole::Embedding => "q8_0",
        ModelRole::Whisper => "standard",
    };
    model
        .variants
        .iter()
        .position(|variant| variant.id == preferred)
        .unwrap_or(0)
}

fn model_status(local: &LocalModels, model: &CatalogModel, states: &[VariantState]) -> &'static str {
    if states.iter().any(|state| state.active) {
        return "active";
    }
    match model.role {
        ModelRole::Whisper => {
            if states.iter().any(|state| state.installed) {
                "installed"
            } else {
                "available"
            }
        }
        ModelRole::Llm | ModelRole::Embedding => {
            let db_role = if model.role == ModelRole::Llm {
                "generation"
            } else {
                "embedding"
            };
            let ready = local.assets.iter().any(|asset| {
                asset.catalog_id == model.catalog_id && asset.role == db_role && asset.state == "ready"
            });
            if !ready {
                return "available";
            }
            let selection = match model.role {
                ModelRole::Llm => local.active_generation,
                _ => local.active_embedding,
            }
            .map(|value| value.trim_matches('"'));
            if selection == Some(model.catalog_id.as_str()) {
                "active"
            } else {
                "installed"
            }
        }
    }
}

fn quantization_label(variant: &CatalogVariant) -> Option<String> {
    variant.quantization.as_ref().map(|q| q.to_ascii_uppercase())
}

/// "whisper-small-en" -> "small.en": the size part of a whisper selection id, which the
/// transcriber matches against `ggml-<size>*.bin` file names.
pub(crate) fn whisper_selector(catalog_id: &str) -> String {
    let family = catalog_id.trim_start_matches("whisper-");
    match family.strip_suffix("-en") {
        Some(base) => format!("{base}.en"),
        None => family.to_string(),
    }
}

fn variant_dto(variant: &CatalogVariant, state: &VariantState) -> ModelVariantDto {
    ModelVariantDto {
        id: Some(variant.id.clone()),
        quantization: quantization_label(variant).unwrap_or_else(|| "Full precision".into()),
        disk_size: format_bytes(variant.file.size_bytes),
        memory_ram: Some(format_bytes(variant.ram_bytes)),
        url: Some(variant.file.url.clone()),
        installed: state.installed,
        active: state.active,
        select_id: state.select_id.clone(),
    }
}

fn catalog_row(model: &CatalogModel, local: &LocalModels) -> ModelAssetDto {
    let states: Vec<VariantState> = model
        .variants
        .iter()
        .map(|variant| model_state::variant_state(local, model, variant))
        .collect();
    let status = model_status(local, model, &states);
    let headline = headline_index(model, &states);
    let variant = &model.variants[headline];
    ModelAssetDto {
        id: states[headline]
            .select_id
            .clone()
            .unwrap_or_else(|| placeholder_id(model)),
        name: model.display_name.clone(),
        role: model.role.as_str().into(),
        parameter_size: match (model.role, &model.embedding) {
            (ModelRole::Embedding, Some(meta)) => format!("{}d", meta.dimension),
            (ModelRole::Whisper, _) => whisper_parameters(&model.catalog_id).into(),
            _ => llm_parameters(&model.display_name),
        },
        quantization: Some(
            quantization_label(variant).unwrap_or_else(|| "Full precision".into()),
        ),
        memory_ram: format_bytes(variant.ram_bytes),
        memory_vram: "—".into(),
        disk_size: format_bytes(variant.file.size_bytes),
        status: status.into(),
        download_progress: None,
        author: Some(model.author.clone()),
        description: model.description.clone(),
        downloads: Some(model.downloads),
        tags: model.tags.clone(),
        fits: None,
        variants: model
            .variants
            .iter()
            .zip(&states)
            .map(|(variant, state)| variant_dto(variant, state))
            .collect(),
        catalog_id: Some(model.catalog_id.clone()),
        is_default: is_default_model(&model.catalog_id),
    }
}

/// One built-in row for an empty (offline) catalog; no variants, disk state from the
/// resolver and the files on disk. Never `bundled`.
fn fallback_whisper_row(local: &LocalModels) -> ModelAssetDto {
    let identity = |file: &str| model_state::whisper_file_identity(file);
    let belongs = |catalog_id: &str, file: &str| {
        identity(file).is_some_and(|(found, _)| found == catalog_id)
    };
    let active = local
        .active_whisper_file
        .is_some_and(|file| belongs("whisper-small", file));
    let installed = local
        .whisper_files
        .iter()
        .any(|file| belongs("whisper-small", file));
    ModelAssetDto {
        id: "whisper:small:catalog".into(),
        name: "Whisper Small".into(),
        role: "whisper".into(),
        parameter_size: "244M".into(),
        quantization: None,
        memory_ram: "1 GB".into(),
        memory_vram: "1 GB".into(),
        disk_size: "466 MiB".into(),
        status: if active {
            "active"
        } else if installed {
            "installed"
        } else {
            "available"
        }
        .into(),
        is_default: true,
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn placeholder_row(
    local: &LocalModels,
    catalog_id: &str,
    name: &str,
    role: ModelRole,
    parameter_size: String,
    quantization: &str,
    ram_bytes: u64,
    size_bytes: u64,
    is_default: bool,
) -> ModelAssetDto {
    let db_role = match role {
        ModelRole::Llm => "generation",
        ModelRole::Embedding => "embedding",
        ModelRole::Whisper => "whisper",
    };
    let selection = match role {
        ModelRole::Llm => local.active_generation,
        ModelRole::Embedding => local.active_embedding,
        ModelRole::Whisper => None,
    }
    .map(|value| value.trim_matches('"'));
    let ready = local.assets.iter().any(|asset| {
        asset.catalog_id == catalog_id && asset.role == db_role && asset.state == "ready"
    });
    let status = if selection == Some(catalog_id) {
        "active"
    } else if ready {
        "installed"
    } else {
        "available"
    };
    let id = match role {
        ModelRole::Llm => format!("generation:{catalog_id}:catalog"),
        ModelRole::Embedding => format!("embedding:{catalog_id}:catalog"),
        ModelRole::Whisper => format!("whisper:{}:catalog", whisper_selector(catalog_id)),
    };
    ModelAssetDto {
        id,
        name: name.into(),
        role: role.as_str().into(),
        parameter_size,
        quantization: Some(quantization.into()),
        memory_ram: format_bytes(ram_bytes),
        memory_vram: "—".into(),
        disk_size: format_bytes(size_bytes),
        status: status.into(),
        is_default,
        ..Default::default()
    }
}

/// "gemma-3-4b-it" -> "4B", "Llama-3.2-3B-Instruct" -> "3B"; "—" when the name has none.
fn llm_parameters(name: &str) -> String {
    name.split(['-', '_', ' '])
        .find(|token| {
            token
                .strip_suffix(['b', 'B'])
                .is_some_and(|n| !n.is_empty() && n.parse::<f32>().is_ok())
        })
        .map(|token| token.to_ascii_uppercase())
        .unwrap_or_else(|| "—".into())
}

fn whisper_parameters(catalog_id: &str) -> &'static str {
    let family = catalog_id.trim_start_matches("whisper-").trim_end_matches("-en");
    match family {
        "tiny" => "39M",
        "base" => "74M",
        "small" => "244M",
        "medium" => "769M",
        "large-v3-turbo" => "809M",
        "large-v1" | "large-v2" | "large-v3" => "1.55B",
        _ => "—",
    }
}

/// One shared settings upsert for the UI's "Set active" and the automatic
/// activation after a download.
pub(crate) fn store_selection(
    database: &Database,
    role: &str,
    model_id: &str,
) -> Result<(), String> {
    let key = match role {
        "llm" => "llm.generation_selection",
        "embedding" => "llm.embedding_selection",
        _ => "transcription.whisper_selection",
    };
    let value = serde_json::to_string(model_id).map_err(|e| e.to_string())?;
    database.run(move |connection| {
        connection.execute("INSERT INTO settings(key,value_json,updated_at) VALUES (?1,?2,datetime('now')) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at", (&key, &value)).map(|_| ()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())
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
    store_selection(&state.database, &role, &model_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::types::{EmbeddingMeta, VerifiedFile};
    use crate::commands::model_state::InstalledAsset;

    fn named_variant(id: &str, filename: &str, size: u64) -> CatalogVariant {
        CatalogVariant {
            id: id.into(),
            quantization: (id != "standard").then(|| id.into()),
            file: VerifiedFile {
                filename: filename.into(),
                url: format!("https://huggingface.co/o/r/resolve/{}/{filename}", "a".repeat(40)),
                size_bytes: size,
                sha256: "ab".repeat(32),
            },
            ram_bytes: size * 2,
        }
    }

    fn variant(id: &str, size: u64) -> CatalogVariant {
        named_variant(id, &format!("{id}.gguf"), size)
    }

    fn model(role: ModelRole, id: &str, name: &str, variants: Vec<CatalogVariant>) -> CatalogModel {
        CatalogModel {
            catalog_id: id.into(),
            display_name: name.into(),
            role,
            author: "someone".into(),
            description: Some("d".into()),
            downloads: 7,
            tags: vec!["Tools".into()],
            revision: None,
            variants,
            embedding: (role == ModelRole::Embedding).then(|| EmbeddingMeta {
                dimension: 384,
                max_sequence_tokens: 256,
                query_prefix: String::new(),
                document_prefix: String::new(),
            }),
        }
    }

    fn by_role<'a>(list: &'a [ModelAssetDto], role: &str) -> Vec<&'a ModelAssetDto> {
        list.iter().filter(|m| m.role == role).collect()
    }

    fn asset(id: &str, catalog_id: &str, role: &str, state: &str, revision: &str) -> InstalledAsset {
        InstalledAsset {
            id: id.into(),
            catalog_id: catalog_id.into(),
            role: role.into(),
            state: state.into(),
            artifact_revision: revision.into(),
        }
    }

    fn none_local() -> LocalModels<'static> {
        LocalModels {
            assets: &[],
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
        }
    }

    #[test]
    fn without_a_catalog_the_built_in_rows_remain() {
        let list = build_model_list(&CatalogSnapshot::default(), &none_local());
        let whisper = by_role(&list, "whisper");
        assert_eq!(whisper.len(), 1);
        assert_eq!(whisper[0].name, "Whisper Small");
        assert!(whisper[0].is_default);
        assert_eq!(by_role(&list, "llm").len(), 2);
        assert_eq!(by_role(&list, "embedding").len(), 1);
        assert!(list.iter().all(|m| m.variants.is_empty() && m.author.is_none()));
    }

    #[test]
    fn whisper_models_are_listed_once_each_with_variants() {
        let snapshot = CatalogSnapshot {
            whisper: vec![
                model(
                    ModelRole::Whisper,
                    "whisper-small",
                    "Whisper Small",
                    vec![
                        named_variant("q5_1", "ggml-small-q5_1.bin", 190_000_000),
                        named_variant("standard", "ggml-small.bin", 488_000_000),
                    ],
                ),
                model(
                    ModelRole::Whisper,
                    "whisper-tiny",
                    "Whisper Tiny",
                    vec![named_variant("standard", "ggml-tiny.bin", 77_000_000)],
                ),
            ],
            ..Default::default()
        };
        let list = build_model_list(&snapshot, &none_local());
        let whisper = by_role(&list, "whisper");
        assert_eq!(whisper.len(), 2, "whisper-small must not be listed twice");
        assert_eq!(whisper[0].id, "whisper:small:catalog");
        assert_eq!(whisper[0].variants.len(), 2);
        assert_eq!(whisper[0].quantization.as_deref(), Some("Full precision"));
        assert_eq!(whisper[0].author.as_deref(), Some("someone"));
        assert_eq!(whisper[1].id, "whisper:tiny:catalog");
        assert_eq!(whisper[1].catalog_id.as_deref(), Some("whisper-tiny"));
        assert_eq!(whisper[0].catalog_id.as_deref(), Some("whisper-small"));
        assert_eq!(whisper[0].variants[0].id.as_deref(), Some("q5_1"));
        assert_eq!(
            (whisper[1].status.as_str(), whisper[1].parameter_size.as_str()),
            ("available", "39M")
        );
    }

    #[test]
    fn a_downloaded_whisper_file_marks_its_model_installed() {
        let snapshot = CatalogSnapshot {
            whisper: vec![
                model(
                    ModelRole::Whisper,
                    "whisper-tiny",
                    "Whisper Tiny",
                    vec![named_variant("standard", "ggml-tiny.bin", 77)],
                ),
                model(
                    ModelRole::Whisper,
                    "whisper-base",
                    "Whisper Base",
                    vec![named_variant("standard", "ggml-base.bin", 142)],
                ),
            ],
            ..Default::default()
        };
        let none = build_model_list(&snapshot, &none_local());
        assert!(by_role(&none, "whisper").iter().all(|m| m.status != "installed"));
        let files = vec!["ggml-tiny.bin".to_string()];
        let local = LocalModels {
            assets: &[],
            whisper_files: &files,
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
        };
        let some = build_model_list(&snapshot, &local);
        let whisper = by_role(&some, "whisper");
        assert_eq!(whisper.iter().filter(|m| m.status == "installed").count(), 1);
        assert_eq!(whisper[0].status, "installed");
    }

    #[test]
    fn whisper_selection_ids_match_what_the_transcriber_searches_for() {
        assert_eq!(whisper_selector("whisper-small"), "small");
        assert_eq!(whisper_selector("whisper-small-en"), "small.en");
        assert_eq!(whisper_selector("whisper-large-v3-turbo"), "large-v3-turbo");
    }

    #[test]
    fn catalog_llms_replace_the_placeholders_and_expose_download_urls() {
        let snapshot = CatalogSnapshot {
            llm: vec![model(
                ModelRole::Llm,
                "unsloth/gemma-3-4b-it",
                "gemma-3-4b-it",
                vec![variant("q4_k_m", 2_500_000_000), variant("q8_0", 4_100_000_000)],
            )],
            ..Default::default()
        };
        let list = build_model_list(&snapshot, &none_local());
        let llms = by_role(&list, "llm");
        assert_eq!(llms.len(), 1);
        let row = llms[0];
        assert_eq!(row.id, "generation:unsloth/gemma-3-4b-it:catalog");
        assert_eq!(row.parameter_size, "4B");
        assert_eq!(row.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(row.disk_size, "2.5 GB");
        assert_eq!(row.downloads, Some(7));
        assert!(row
            .variants
            .iter()
            .all(|v| v.url.as_deref().is_some_and(|u| u.starts_with("https://huggingface.co/"))));
    }

    #[test]
    fn installed_and_active_state_follow_the_database_rows() {
        let snapshot = CatalogSnapshot {
            llm: vec![
                model(ModelRole::Llm, "a/one", "one-1b", vec![variant("q4_k_m", 1)]),
                model(ModelRole::Llm, "a/two", "two-2b", vec![variant("q4_k_m", 1)]),
                model(ModelRole::Llm, "a/three", "three-3b", vec![variant("q4_k_m", 1)]),
            ],
            ..Default::default()
        };
        let assets = vec![
            asset(
                "generation:a/one:q4_k_m-1",
                "a/one",
                "generation",
                "ready",
                "q4_k_m-1",
            ),
            asset(
                "generation:a/two:q4_k_m-1",
                "a/two",
                "generation",
                "ready",
                "q4_k_m-1",
            ),
            asset(
                "embedding:a/three:q4_k_m-1",
                "a/three",
                "embedding",
                "ready",
                "q4_k_m-1",
            ),
        ];
        let local = LocalModels {
            assets: &assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: Some("\"generation:a/two:q4_k_m-1\""),
            active_embedding: None,
        };
        let list = build_model_list(&snapshot, &local);
        let state = |id: &str| list.iter().find(|m| m.id == id).unwrap().status.clone();
        assert_eq!(state("generation:a/one:q4_k_m-1"), "installed");
        assert_eq!(state("generation:a/two:q4_k_m-1"), "active");
        assert_eq!(
            state("generation:a/three:catalog"),
            "available",
            "a different role must not match"
        );
    }

    #[test]
    fn the_llm_list_is_capped() {
        let llm = (0..LLM_LIST_LIMIT + 25)
            .map(|i| model(ModelRole::Llm, &format!("d/m{i}"), "m-1b", vec![variant("q4_k_m", 1)]))
            .collect();
        let list = build_model_list(
            &CatalogSnapshot { llm, ..Default::default() },
            &none_local(),
        );
        assert_eq!(by_role(&list, "llm").len(), LLM_LIST_LIMIT);
    }

    #[test]
    fn catalog_embeddings_show_their_dimension_and_prefer_q8() {
        let snapshot = CatalogSnapshot {
            embedding: vec![model(
                ModelRole::Embedding,
                "all-minilm-l6-v2",
                "all-MiniLM-L6-v2",
                vec![variant("q8_0", 30_000_000), variant("f16", 45_000_000)],
            )],
            ..Default::default()
        };
        let list = build_model_list(&snapshot, &none_local());
        let row = by_role(&list, "embedding")[0];
        assert_eq!(
            (row.parameter_size.as_str(), row.quantization.as_deref()),
            ("384d", Some("Q8_0"))
        );
        assert_eq!(row.id, "embedding:all-minilm-l6-v2:catalog");
        assert!(row.is_default, "all-minilm-l6-v2 is the first-run embedder");
    }

    #[test]
    fn parameter_sizes_come_from_the_model_name() {
        assert_eq!(llm_parameters("Llama-3.2-3B-Instruct"), "3B");
        assert_eq!(llm_parameters("Qwen3-4B-Instruct-2507"), "4B");
        assert_eq!(llm_parameters("Qwen3.5-35B-A3B"), "35B");
        assert_eq!(llm_parameters("gemma-3-1b-it"), "1B");
        assert_eq!(llm_parameters("some-model"), "—");
        assert_eq!(whisper_parameters("whisper-small-en"), "244M");
        assert_eq!(whisper_parameters("whisper-large-v3-turbo"), "809M");
    }

    #[test]
    fn no_status_is_ever_bundled() {
        let whisper = model(
            ModelRole::Whisper,
            "whisper-small",
            "Whisper Small",
            vec![named_variant("standard", "ggml-small.bin", 1)],
        );
        let llm = model(ModelRole::Llm, "a/one", "one-1b", vec![variant("q4_k_m", 1)]);
        let embedding = model(
            ModelRole::Embedding,
            "all-minilm-l6-v2",
            "all-MiniLM-L6-v2",
            vec![variant("q8_0", 1)],
        );
        let full = CatalogSnapshot {
            whisper: vec![whisper],
            llm: vec![llm],
            embedding: vec![embedding],
            ..Default::default()
        };
        let files = vec!["ggml-small.bin".to_string()];
        let assets = vec![asset(
            "generation:a/one:q4_k_m-1",
            "a/one",
            "generation",
            "ready",
            "q4_k_m-1",
        )];
        let with_disk = LocalModels {
            assets: &assets,
            whisper_files: &files,
            active_whisper_file: Some("ggml-small.bin"),
            active_generation: Some("\"generation:a/one:q4_k_m-1\""),
            active_embedding: None,
        };
        for row in build_model_list(&full, &with_disk) {
            assert_ne!(row.status, "bundled");
        }
        for row in build_model_list(&CatalogSnapshot::default(), &none_local()) {
            assert_ne!(row.status, "bundled");
        }
    }

    #[test]
    fn whisper_small_is_default_and_active_after_first_run_state() {
        let small = model(
            ModelRole::Whisper,
            "whisper-small",
            "Whisper Small",
            vec![
                named_variant("q5_1", "ggml-small-q5_1.bin", 190_000_000),
                named_variant("standard", "ggml-small.bin", 488_000_000),
            ],
        );
        let files = vec!["ggml-small.bin".to_string()];
        let local = LocalModels {
            assets: &[],
            whisper_files: &files,
            active_whisper_file: Some("ggml-small.bin"),
            active_generation: None,
            active_embedding: None,
        };
        let list = build_model_list(
            &CatalogSnapshot {
                whisper: vec![small],
                ..Default::default()
            },
            &local,
        );
        let row = &list[0];
        assert_eq!(row.status, "active");
        assert!(row.is_default);
        assert_eq!(row.quantization.as_deref(), Some("Full precision"));
        let standard = row
            .variants
            .iter()
            .find(|v| v.id.as_deref() == Some("standard"))
            .unwrap();
        assert!(standard.active && standard.installed);
        let quantized = row
            .variants
            .iter()
            .find(|v| v.id.as_deref() == Some("q5_1"))
            .unwrap();
        assert!(!quantized.active && !quantized.installed);
    }

    #[test]
    fn headline_variant_follows_active_then_installed_then_preferred() {
        let m = model(
            ModelRole::Llm,
            "a/one",
            "one-1b",
            vec![variant("q4_k_m", 2), variant("q8_0", 3)],
        );
        let snapshot = CatalogSnapshot {
            llm: vec![m],
            ..Default::default()
        };
        let headline = |local: &LocalModels| {
            let list = build_model_list(&snapshot, local);
            by_role(&list, "llm")[0].quantization.clone()
        };

        // Nothing installed: the preferred q4_k_m.
        assert_eq!(headline(&none_local()), Some("Q4_K_M".into()));

        // Only q8_0 installed: the installed variant beats the preferred one.
        let q8 = vec![asset(
            "generation:a/one:q8_0-1",
            "a/one",
            "generation",
            "ready",
            "q8_0-1",
        )];
        let installed = LocalModels {
            assets: &q8,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
        };
        assert_eq!(headline(&installed), Some("Q8_0".into()));

        // Both installed, q8_0 active: the active variant wins.
        let both = vec![
            asset(
                "generation:a/one:q4_k_m-1",
                "a/one",
                "generation",
                "ready",
                "q4_k_m-1",
            ),
            asset(
                "generation:a/one:q8_0-1",
                "a/one",
                "generation",
                "ready",
                "q8_0-1",
            ),
        ];
        let active = LocalModels {
            assets: &both,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: Some("\"generation:a/one:q8_0-1\""),
            active_embedding: None,
        };
        assert_eq!(headline(&active), Some("Q8_0".into()));
    }

    #[test]
    fn a_second_installed_quantization_does_not_hide_the_first() {
        let m = model(
            ModelRole::Llm,
            "a/one",
            "one-1b",
            vec![variant("q4_k_m", 2), variant("q8_0", 3)],
        );
        let assets = vec![
            asset(
                "generation:a/one:q4_k_m-1",
                "a/one",
                "generation",
                "ready",
                "q4_k_m-1",
            ),
            asset(
                "generation:a/one:q8_0-1",
                "a/one",
                "generation",
                "ready",
                "q8_0-1",
            ),
        ];
        let local = LocalModels {
            assets: &assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: Some("\"generation:a/one:q4_k_m-1\""),
            active_embedding: None,
        };
        let list = build_model_list(
            &CatalogSnapshot {
                llm: vec![m],
                ..Default::default()
            },
            &local,
        );
        let row = by_role(&list, "llm")[0];
        assert_eq!(row.id, "generation:a/one:q4_k_m-1");
        assert_eq!(row.status, "active");
        assert_eq!(row.variants.iter().filter(|v| v.installed).count(), 2);
    }

    #[test]
    fn empty_catalog_gives_one_fallback_whisper_row_with_disk_state() {
        let files = vec!["ggml-small.bin".to_string()];
        let local = LocalModels {
            assets: &[],
            whisper_files: &files,
            active_whisper_file: Some("ggml-small.bin"),
            active_generation: None,
            active_embedding: None,
        };
        let list = build_model_list(&CatalogSnapshot::default(), &local);
        let whisper = by_role(&list, "whisper");
        assert_eq!(whisper.len(), 1);
        let row = whisper[0];
        assert_eq!(row.name, "Whisper Small");
        assert_eq!(row.id, "whisper:small:catalog");
        assert!(row.catalog_id.is_none());
        assert!(row.variants.is_empty());
        assert!(row.is_default);
        assert_eq!(row.status, "active");

        let none = build_model_list(&CatalogSnapshot::default(), &none_local());
        assert_eq!(by_role(&none, "whisper")[0].status, "available");
    }

    #[test]
    fn variants_are_present_on_installed_models() {
        let m = model(
            ModelRole::Llm,
            "a/one",
            "one-1b",
            vec![variant("q4_k_m", 2), variant("q8_0", 3)],
        );
        let assets = vec![asset(
            "generation:a/one:q4_k_m-1",
            "a/one",
            "generation",
            "ready",
            "q4_k_m-1",
        )];
        let local = LocalModels {
            assets: &assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: Some("\"generation:a/one:q4_k_m-1\""),
            active_embedding: None,
        };
        let list = build_model_list(
            &CatalogSnapshot {
                llm: vec![m],
                ..Default::default()
            },
            &local,
        );
        let row = by_role(&list, "llm")[0];
        assert_eq!(row.status, "active");
        assert_eq!(row.variants.len(), 2);
        assert!(row.variants.iter().any(|v| v.installed));
    }
}
