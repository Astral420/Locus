use crate::{
    catalog::{
        model::{CatalogModel, CatalogVariant, ModelRole},
        CatalogSnapshot,
    },
    contracts::{CatalogRefreshDto, GpuBackendDto, ModelAssetDto, ModelVariantDto},
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

    // Reflect what is really on disk: the model is a release/maintainer input
    // that may not be present, and the pipeline blocks without it.
    let whisper_found = crate::paths::find_whisper_model(
        &state.paths,
        None,
        std::env::var_os(crate::paths::ENV_WHISPER_MODEL).map(std::path::PathBuf::from),
    )
    .is_some();
    // Reads the cached catalog only; refresh_model_catalog is what uses the network.
    let snapshot = state.catalog.snapshot();
    let whisper_files: Vec<String> = std::fs::read_dir(state.paths.whisper_models())
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    Ok(build_model_list(
        &snapshot,
        &installed,
        active_generation.as_deref(),
        active_embedding.as_deref(),
        whisper_found,
        &whisper_files,
    ))
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

/// The chat catalog holds thousands of repositories; the UI gets the best-ranked slice
/// (recommended models first, then most downloaded) until search exists.
const LLM_LIST_LIMIT: usize = 60;

/// (id, catalog_id, role, state) rows of the `model_assets` table.
type InstalledRow = (String, String, String, String);

fn installed_state(
    installed: &[InstalledRow],
    db_role: &str,
    catalog_id: &str,
    active: Option<&str>,
    catalog_row_id: String,
) -> (String, &'static str) {
    let row = installed
        .iter()
        .find(|(_, catalog, role, _)| catalog == catalog_id && role == db_role);
    let id = row.map(|entry| entry.0.clone()).unwrap_or(catalog_row_id);
    let status = if active.map(|value| value.trim_matches('"')) == Some(id.as_str())
        || active == Some(catalog_id)
    {
        "active"
    } else if row.is_some() {
        "installed"
    } else {
        "available"
    };
    (id, status)
}

pub(crate) fn build_model_list(
    snapshot: &CatalogSnapshot,
    installed: &[InstalledRow],
    active_generation: Option<&str>,
    active_embedding: Option<&str>,
    whisper_found: bool,
    whisper_files: &[String],
) -> Vec<ModelAssetDto> {
    let mut assets = Vec::new();

    // The first whisper row always describes what is really on disk; catalog data only
    // adds the author, description and downloadable variants.
    let mut small = ModelAssetDto {
        id: "whisper:small:bundled".into(),
        name: if whisper_found {
            "Whisper Small".into()
        } else {
            "Whisper Small (not installed)".into()
        },
        role: "whisper".into(),
        parameter_size: "244M".into(),
        quantization: None,
        memory_ram: "1 GB".into(),
        memory_vram: "1 GB".into(),
        disk_size: "466 MiB".into(),
        status: if whisper_found { "bundled" } else { "available" }.into(),
        ..Default::default()
    };
    if let Some(model) = snapshot.whisper.iter().find(|m| m.catalog_id == "whisper-small") {
        small.author = Some(model.author.clone());
        small.description = model.description.clone();
        small.downloads = Some(model.downloads);
        small.tags = model.tags.clone();
        small.variants = model.variants.iter().map(variant_dto).collect();
        small.catalog_id = Some(model.catalog_id.clone());
    }
    assets.push(small);
    for model in snapshot.whisper.iter().filter(|m| m.catalog_id != "whisper-small") {
        let installed = model.variants.iter().any(|v| whisper_files.contains(&v.file.filename));
        assets.push(catalog_row(
            model,
            format!("whisper:{}:catalog", whisper_selector(&model.catalog_id)),
            if installed { "installed" } else { "available" },
        ));
    }

    if snapshot.llm.is_empty() {
        // No verified catalog cached yet: keep the built-in placeholders.
        for model in GenerationModelManager::catalog() {
            let (id, status) = installed_state(
                installed,
                "generation",
                &model.id,
                active_generation,
                format!("generation:{}:catalog", model.id),
            );
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
                ..Default::default()
            });
        }
    } else {
        for model in snapshot.llm.iter().take(LLM_LIST_LIMIT) {
            let (id, status) = installed_state(
                installed,
                "generation",
                &model.catalog_id,
                active_generation,
                format!("generation:{}:catalog", model.catalog_id),
            );
            assets.push(catalog_row(model, id, status));
        }
    }

    if snapshot.embedding.is_empty() {
        for model in EmbeddingModelManager::catalog() {
            let (id, status) = installed_state(
                installed,
                "embedding",
                &model.id,
                active_embedding,
                format!("embedding:{}:catalog", model.id),
            );
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
                ..Default::default()
            });
        }
    } else {
        for model in &snapshot.embedding {
            let (id, status) = installed_state(
                installed,
                "embedding",
                &model.catalog_id,
                active_embedding,
                format!("embedding:{}:catalog", model.catalog_id),
            );
            assets.push(catalog_row(model, id, status));
        }
    }
    assets
}

fn preferred_variant(model: &CatalogModel) -> &CatalogVariant {
    let wanted = match model.role {
        ModelRole::Llm => "q4_k_m",
        ModelRole::Embedding => "q8_0",
        ModelRole::Whisper => "standard",
    };
    model
        .variants
        .iter()
        .find(|v| v.id == wanted)
        .unwrap_or(&model.variants[0])
}

fn quantization_label(variant: &CatalogVariant) -> Option<String> {
    variant.quantization.as_ref().map(|q| q.to_ascii_uppercase())
}

/// "whisper-small-en" -> "small.en": the size part of a whisper selection id, which the
/// transcriber matches against `ggml-<size>*.bin` file names.
fn whisper_selector(catalog_id: &str) -> String {
    let family = catalog_id.trim_start_matches("whisper-");
    match family.strip_suffix("-en") {
        Some(base) => format!("{base}.en"),
        None => family.to_string(),
    }
}

fn variant_dto(variant: &CatalogVariant) -> ModelVariantDto {
    ModelVariantDto {
        id: Some(variant.id.clone()),
        quantization: quantization_label(variant).unwrap_or_else(|| "Full precision".into()),
        disk_size: format_bytes(variant.file.size_bytes),
        memory_ram: Some(format_bytes(variant.ram_bytes)),
        url: Some(variant.file.url.clone()),
    }
}

fn catalog_row(model: &CatalogModel, id: String, status: &str) -> ModelAssetDto {
    let variant = preferred_variant(model);
    ModelAssetDto {
        id,
        name: model.display_name.clone(),
        role: model.role.as_str().into(),
        parameter_size: match (model.role, &model.embedding) {
            (ModelRole::Embedding, Some(meta)) => format!("{}d", meta.dimension),
            (ModelRole::Whisper, _) => whisper_parameters(&model.catalog_id).into(),
            _ => llm_parameters(&model.display_name),
        },
        quantization: quantization_label(variant),
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
        variants: model.variants.iter().map(variant_dto).collect(),
        catalog_id: Some(model.catalog_id.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::types::{EmbeddingMeta, VerifiedFile};

    fn variant(id: &str, size: u64) -> CatalogVariant {
        CatalogVariant {
            id: id.into(),
            quantization: (id != "standard").then(|| id.into()),
            file: VerifiedFile {
                filename: format!("{id}.gguf"),
                url: format!("https://huggingface.co/o/r/resolve/{}/{id}.gguf", "a".repeat(40)),
                size_bytes: size,
                sha256: "ab".repeat(32),
            },
            ram_bytes: size * 2,
        }
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

    #[test]
    fn without_a_catalog_the_built_in_rows_remain() {
        let list = build_model_list(&CatalogSnapshot::default(), &[], None, None, false, &[]);
        assert_eq!(by_role(&list, "whisper").len(), 1);
        assert_eq!(by_role(&list, "whisper")[0].name, "Whisper Small (not installed)");
        assert_eq!(by_role(&list, "llm").len(), 2);
        assert_eq!(by_role(&list, "embedding").len(), 1);
        assert!(list.iter().all(|m| m.variants.is_empty() && m.author.is_none()));
    }

    #[test]
    fn whisper_small_row_keeps_describing_the_disk_and_gains_catalog_variants() {
        let snapshot = CatalogSnapshot {
            whisper: vec![
                model(ModelRole::Whisper, "whisper-small", "Whisper Small", vec![variant("q5_1", 190_000_000), variant("standard", 488_000_000)]),
                model(ModelRole::Whisper, "whisper-tiny", "Whisper Tiny", vec![variant("standard", 77_000_000)]),
            ],
            ..Default::default()
        };
        let list = build_model_list(&snapshot, &[], None, None, true, &[]);
        let whisper = by_role(&list, "whisper");
        assert_eq!(whisper.len(), 2, "whisper-small must not be listed twice");
        assert_eq!((whisper[0].id.as_str(), whisper[0].status.as_str()), ("whisper:small:bundled", "bundled"));
        assert_eq!(whisper[0].variants.len(), 2);
        assert_eq!(whisper[0].variants[1].quantization, "Full precision");
        assert_eq!(whisper[0].author.as_deref(), Some("someone"));
        assert_eq!(whisper[1].id, "whisper:tiny:catalog");
        assert_eq!(whisper[1].catalog_id.as_deref(), Some("whisper-tiny"));
        assert_eq!(whisper[0].catalog_id.as_deref(), Some("whisper-small"));
        assert_eq!(whisper[0].variants[0].id.as_deref(), Some("q5_1"));
        assert_eq!((whisper[1].status.as_str(), whisper[1].parameter_size.as_str()), ("available", "39M"));
    }

    #[test]
    fn a_downloaded_whisper_file_marks_its_model_installed() {
        let snapshot = CatalogSnapshot {
            whisper: vec![
                model(ModelRole::Whisper, "whisper-tiny", "Whisper Tiny", vec![variant("standard", 77)]),
                model(ModelRole::Whisper, "whisper-base", "Whisper Base", vec![variant("standard", 142)]),
            ],
            ..Default::default()
        };
        let files = vec!["standard.gguf".to_string()];
        let none = build_model_list(&snapshot, &[], None, None, false, &[]);
        assert!(by_role(&none, "whisper").iter().all(|m| m.status != "installed"));
        // Both fixtures name their file "standard.gguf", so either match marks both rows.
        let some = build_model_list(&snapshot, &[], None, None, false, &files);
        assert_eq!(by_role(&some, "whisper").iter().filter(|m| m.status == "installed").count(), 2);
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
        let list = build_model_list(&snapshot, &[], None, None, false, &[]);
        let llms = by_role(&list, "llm");
        assert_eq!(llms.len(), 1);
        let row = llms[0];
        assert_eq!(row.id, "generation:unsloth/gemma-3-4b-it:catalog");
        assert_eq!(row.parameter_size, "4B");
        assert_eq!(row.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(row.disk_size, "2.5 GB");
        assert_eq!(row.downloads, Some(7));
        assert!(row.variants.iter().all(|v| v.url.as_deref().is_some_and(|u| u.starts_with("https://huggingface.co/"))));
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
        let installed = vec![
            ("row-1".to_string(), "a/one".to_string(), "generation".to_string(), "ready".to_string()),
            ("row-2".to_string(), "a/two".to_string(), "generation".to_string(), "ready".to_string()),
            ("row-x".to_string(), "a/three".to_string(), "embedding".to_string(), "ready".to_string()),
        ];
        let list = build_model_list(&snapshot, &installed, Some("\"row-2\""), None, false, &[]);
        let state = |id: &str| list.iter().find(|m| m.id == id).unwrap().status.clone();
        assert_eq!(state("row-1"), "installed");
        assert_eq!(state("row-2"), "active");
        assert_eq!(state("generation:a/three:catalog"), "available", "a different role must not match");
    }

    #[test]
    fn the_llm_list_is_capped() {
        let llm = (0..LLM_LIST_LIMIT + 25)
            .map(|i| model(ModelRole::Llm, &format!("d/m{i}"), "m-1b", vec![variant("q4_k_m", 1)]))
            .collect();
        let list = build_model_list(&CatalogSnapshot { llm, ..Default::default() }, &[], None, None, false, &[]);
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
        let list = build_model_list(&snapshot, &[], None, None, false, &[]);
        let row = by_role(&list, "embedding")[0];
        assert_eq!((row.parameter_size.as_str(), row.quantization.as_deref()), ("384d", Some("Q8_0")));
        assert_eq!(row.id, "embedding:all-minilm-l6-v2:catalog");
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
}
