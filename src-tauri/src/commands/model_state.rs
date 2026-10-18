//! Derives per-variant install/active state from what is on disk and the stored
//! selections, independent of Tauri or the database. `list_models` turns this
//! into the DTOs the Model Manager renders.

use crate::catalog::model::{parse_whisper_id, CatalogModel, CatalogVariant, ModelRole};
use crate::commands::models::whisper_selector;

/// One `model_assets` row, reduced to what state derivation needs.
pub struct InstalledAsset {
    pub id: String,
    pub catalog_id: String,
    pub role: String,
    pub state: String,
    pub artifact_revision: String,
}

/// What is really present locally.
pub struct LocalModels<'a> {
    pub assets: &'a [InstalledAsset],
    /// File names found in the whisper search dirs.
    pub whisper_files: &'a [String],
    /// File name returned by `find_whisper_model`.
    pub active_whisper_file: Option<&'a str>,
    /// Raw setting values; they may still be JSON-quoted.
    pub active_generation: Option<&'a str>,
    pub active_embedding: Option<&'a str>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantState {
    pub installed: bool,
    pub active: bool,
    pub select_id: Option<String>,
}

/// `ggml-small.en-q5_1.bin` -> `("whisper-small-en", "q5_1")`. `None` for files
/// that are not whisper models (vision encoders, VAD weights, ...).
pub fn whisper_file_identity(file_name: &str) -> Option<(String, String)> {
    let stem = file_name.strip_suffix(".bin")?;
    let (family, english, quant) = parse_whisper_id(stem)?;
    let catalog_id = format!("whisper-{family}{}", if english { "-en" } else { "" });
    let variant = quant.unwrap_or_else(|| "standard".to_string());
    Some((catalog_id, variant))
}

/// The variant part of `"{variant_id}-{commit}"` asset revisions.
pub fn variant_id_from_revision(artifact_revision: &str) -> Option<&str> {
    artifact_revision.rsplit_once('-').map(|(variant, _)| variant)
}

fn db_role(role: ModelRole) -> &'static str {
    match role {
        ModelRole::Llm => "generation",
        ModelRole::Embedding => "embedding",
        ModelRole::Whisper => "whisper",
    }
}

fn trim(selection: Option<&str>) -> Option<&str> {
    selection.map(|value| value.trim_matches('"'))
}

fn ready_asset<'a>(
    local: &'a LocalModels,
    model: &CatalogModel,
    variant: &CatalogVariant,
) -> Option<&'a InstalledAsset> {
    let role = db_role(model.role);
    local.assets.iter().find(|asset| {
        asset.catalog_id == model.catalog_id
            && asset.role == role
            && asset.state == "ready"
            && variant_id_from_revision(&asset.artifact_revision) == Some(variant.id.as_str())
    })
}

pub fn variant_state(
    local: &LocalModels,
    model: &CatalogModel,
    variant: &CatalogVariant,
) -> VariantState {
    match model.role {
        ModelRole::Whisper => {
            let installed = local
                .whisper_files
                .iter()
                .any(|file| file == &variant.file.filename);
            let active = local
                .active_whisper_file
                .and_then(whisper_file_identity)
                .is_some_and(|(catalog_id, id)| catalog_id == model.catalog_id && id == variant.id);
            let select_id = installed.then(|| {
                format!("whisper:{}:{}", whisper_selector(&model.catalog_id), variant.id)
            });
            VariantState {
                installed,
                active,
                select_id,
            }
        }
        ModelRole::Llm | ModelRole::Embedding => {
            let asset = ready_asset(local, model, variant);
            let active_selection = match model.role {
                ModelRole::Llm => trim(local.active_generation),
                _ => trim(local.active_embedding),
            };
            let active = asset.is_some_and(|asset| active_selection == Some(asset.id.as_str()));
            VariantState {
                installed: asset.is_some(),
                active,
                select_id: asset.map(|asset| asset.id.clone()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::types::VerifiedFile;

    fn variant(id: &str, filename: &str) -> CatalogVariant {
        CatalogVariant {
            id: id.into(),
            quantization: (id != "standard").then(|| id.into()),
            file: VerifiedFile {
                filename: filename.into(),
                url: format!(
                    "https://huggingface.co/o/r/resolve/{}/{}",
                    "a".repeat(40),
                    filename
                ),
                size_bytes: 1,
                sha256: "ab".repeat(32),
            },
            ram_bytes: 1,
        }
    }

    fn model(role: ModelRole, catalog_id: &str, variants: Vec<CatalogVariant>) -> CatalogModel {
        CatalogModel {
            catalog_id: catalog_id.into(),
            display_name: catalog_id.into(),
            role,
            author: "a".into(),
            description: None,
            downloads: 0,
            tags: vec![],
            revision: None,
            variants,
            embedding: None,
        }
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

    fn local(assets: &[InstalledAsset]) -> LocalModels<'_> {
        LocalModels {
            assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
        }
    }

    #[test]
    fn whisper_file_identity_maps_files_to_catalog_ids() {
        assert_eq!(
            whisper_file_identity("ggml-small.en-q5_1.bin"),
            Some(("whisper-small-en".into(), "q5_1".into()))
        );
        assert_eq!(
            whisper_file_identity("ggml-small.bin"),
            Some(("whisper-small".into(), "standard".into()))
        );
        assert_eq!(
            whisper_file_identity("ggml-large-v3-turbo.bin"),
            Some(("whisper-large-v3-turbo".into(), "standard".into()))
        );
        assert_eq!(whisper_file_identity("ggml-silero-v5.1.2.bin"), None);
    }

    #[test]
    fn variant_ids_come_from_the_asset_revision() {
        assert_eq!(variant_id_from_revision("q4_k_m-01234567"), Some("q4_k_m"));
        assert_eq!(variant_id_from_revision("legacy"), None);
    }

    #[test]
    fn only_ready_assets_of_the_right_role_count_as_installed() {
        let m = model(ModelRole::Llm, "a/one", vec![variant("q4_k_m", "m.gguf")]);
        let v = &m.variants[0];
        let ready = asset(
            "generation:a/one:q4_k_m-1",
            "a/one",
            "generation",
            "ready",
            "q4_k_m-1",
        );
        assert!(variant_state(&local(&[ready]), &m, v).installed);

        let pending = asset(
            "generation:a/one:q4_k_m-2",
            "a/one",
            "generation",
            "downloading",
            "q4_k_m-2",
        );
        let wrong_role = asset(
            "embedding:a/one:q4_k_m-3",
            "a/one",
            "embedding",
            "ready",
            "q4_k_m-3",
        );
        assert!(!variant_state(&local(&[pending]), &m, v).installed);
        assert!(!variant_state(&local(&[wrong_role]), &m, v).installed);
    }

    #[test]
    fn an_active_selection_marks_exactly_one_variant() {
        let m = model(
            ModelRole::Llm,
            "a/one",
            vec![variant("q4_k_m", "m.gguf"), variant("q8_0", "n.gguf")],
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
            active_generation: Some("\"generation:a/one:q8_0-1\""),
            active_embedding: None,
        };
        let states: Vec<VariantState> = m
            .variants
            .iter()
            .map(|v| variant_state(&local, &m, v))
            .collect();
        assert_eq!(states.iter().filter(|state| state.active).count(), 1);
        assert!(states[0].installed && !states[0].active);
        assert!(states[1].installed && states[1].active);
    }
}
