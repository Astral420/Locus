//! Derives per-variant install/active state from what is on disk and the stored
//! selections, independent of Tauri or the database. `list_models` turns this
//! into the DTOs the Model Manager renders.

use crate::catalog::model::{parse_whisper_id, CatalogModel, CatalogVariant, ModelRole};
use crate::commands::models::whisper_selector;
use crate::paths::AppPaths;
use std::path::{Path, PathBuf};

/// One `model_assets` row, reduced to what state derivation needs.
pub struct InstalledAsset {
    pub id: String,
    pub catalog_id: String,
    pub role: String,
    pub state: String,
    pub artifact_revision: String,
    /// File name only, resolved against the managed folder for this role.
    pub relative_path: String,
}

/// The three folders Locus owns. Files elsewhere are never deletable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedFolders {
    pub whisper: Option<PathBuf>,
    pub embedding: Option<PathBuf>,
    pub generation: Option<PathBuf>,
}

impl ManagedFolders {
    pub fn for_paths(paths: &AppPaths) -> Self {
        Self {
            whisper: Some(paths.whisper_models()),
            embedding: Some(paths.embedding_models()),
            generation: Some(paths.generation_models()),
        }
    }

    pub fn dir(&self, role: ModelRole) -> Option<&Path> {
        match role {
            ModelRole::Whisper => self.whisper.as_deref(),
            ModelRole::Embedding => self.embedding.as_deref(),
            ModelRole::Llm => self.generation.as_deref(),
        }
    }

    fn dir_for_db_role(&self, role: &str) -> Option<&Path> {
        match role {
            "whisper" => self.whisper.as_deref(),
            "embedding" => self.embedding.as_deref(),
            "generation" => self.generation.as_deref(),
            _ => None,
        }
    }
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
    /// Managed folders; empty in tests that only exercise catalog rows.
    pub managed: ManagedFolders,
}

/// A database row only counts as installed while its file is still at
/// `<managed folder>/<relative_path>`. When no managed folder is configured
/// (unit tests), the row is trusted as-is.
pub fn asset_file_present(local: &LocalModels, asset: &InstalledAsset) -> bool {
    match local.managed.dir_for_db_role(&asset.role) {
        Some(dir) => dir.join(&asset.relative_path).is_file(),
        None => true,
    }
}

/// Whether any ready asset of `role` for `catalog_id` is present on disk.
pub fn installed_ready(local: &LocalModels, catalog_id: &str, role: &str) -> bool {
    local.assets.iter().any(|asset| {
        asset.catalog_id == catalog_id
            && asset.role == role
            && asset.state == "ready"
            && asset_file_present(local, asset)
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantState {
    pub installed: bool,
    pub active: bool,
    pub select_id: Option<String>,
    pub removable: bool,
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
            && asset_file_present(local, asset)
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
            // A whisper file is removable only when it sits in Locus's own folder;
            // the resolver also searches the models root and bundled dirs.
            let removable = installed
                && local
                    .managed
                    .whisper
                    .as_ref()
                    .is_some_and(|dir| dir.join(&variant.file.filename).is_file());
            VariantState {
                installed,
                active,
                select_id,
                removable,
            }
        }
        ModelRole::Llm | ModelRole::Embedding => {
            let asset = ready_asset(local, model, variant);
            let active_selection = match model.role {
                ModelRole::Llm => trim(local.active_generation),
                _ => trim(local.active_embedding),
            };
            let active = asset.is_some_and(|asset| active_selection == Some(asset.id.as_str()));
            // Database assets always resolve inside the role's managed folder, so
            // one that is present and installed is always removable.
            let removable = asset.is_some() && local.managed.dir(model.role).is_some();
            VariantState {
                installed: asset.is_some(),
                active,
                select_id: asset.map(|asset| asset.id.clone()),
                removable,
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
            relative_path: format!("{id}.gguf"),
        }
    }

    fn local(assets: &[InstalledAsset]) -> LocalModels<'_> {
        LocalModels {
            assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
            managed: ManagedFolders::default(),
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
            managed: ManagedFolders::default(),
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

    #[test]
    fn an_asset_whose_file_is_gone_does_not_count_as_installed() {
        let root = tempfile::tempdir().unwrap();
        let managed = ManagedFolders {
            whisper: None,
            embedding: None,
            generation: Some(root.path().join("generation")),
        };
        std::fs::create_dir_all(managed.generation.as_ref().unwrap()).unwrap();
        let m = model(ModelRole::Llm, "a/one", vec![variant("q4_k_m", "m.gguf")]);
        let asset = InstalledAsset {
            id: "generation:a/one:q4_k_m-1".into(),
            catalog_id: "a/one".into(),
            role: "generation".into(),
            state: "ready".into(),
            artifact_revision: "q4_k_m-1".into(),
            relative_path: "m.gguf".into(),
        };
        let assets = [asset];
        let local = LocalModels {
            assets: &assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
            managed: managed.clone(),
        };
        assert!(!variant_state(&local, &m, &m.variants[0]).installed);
        assert!(!installed_ready(&local, "a/one", "generation"));

        std::fs::write(managed.generation.as_ref().unwrap().join("m.gguf"), b"w").unwrap();
        let local = LocalModels {
            assets: &assets,
            whisper_files: &[],
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
            managed,
        };
        assert!(variant_state(&local, &m, &m.variants[0]).installed);
        assert!(installed_ready(&local, "a/one", "generation"));
    }

    #[test]
    fn removable_is_true_only_inside_the_managed_folder() {
        let root = tempfile::tempdir().unwrap();
        let whisper_dir = root.path().join("models/whisper");
        let models_root = root.path().join("models");
        std::fs::create_dir_all(&whisper_dir).unwrap();
        let m = model(
            ModelRole::Whisper,
            "whisper-small",
            vec![variant("standard", "ggml-small.bin")],
        );
        let variant = &m.variants[0];

        // Present only in the models root: installed, not removable.
        std::fs::write(models_root.join("ggml-small.bin"), b"m").unwrap();
        let files = vec!["ggml-small.bin".to_string()];
        let local = LocalModels {
            assets: &[],
            whisper_files: &files,
            active_whisper_file: None,
            active_generation: None,
            active_embedding: None,
            managed: ManagedFolders {
                whisper: Some(whisper_dir.clone()),
                embedding: None,
                generation: None,
            },
        };
        let state = variant_state(&local, &m, variant);
        assert!(state.installed);
        assert!(!state.removable);

        // Present in the managed folder: removable.
        std::fs::write(whisper_dir.join("ggml-small.bin"), b"m").unwrap();
        let state = variant_state(&local, &m, variant);
        assert!(state.installed && state.removable);
    }
}
