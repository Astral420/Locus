//! Where Locus keeps its data.
//!
//! Everything user-visible lives under one per-user app-data directory chosen by
//! the OS (PRD NFR9 / SPEC "app data directory"), never under the process
//! working directory. Before this module the database, recordings and models
//! were opened with relative paths, so launching from a different directory
//! silently started a second, empty library.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const DATABASE_FILE: &str = "locus.sqlite";

/// Environment overrides, mainly for development, tests and portable installs.
pub const ENV_DATA_DIR: &str = "LOCUS_DATA_DIR";
pub const ENV_PYTHON: &str = "LOCUS_PYTHON";
pub const ENV_SIDECAR_DIR: &str = "LOCUS_SIDECAR_DIR";
pub const ENV_WHISPER_MODEL: &str = "LOCUS_WHISPER_MODEL";

#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Root of all writable user data.
    pub data_dir: PathBuf,
    pub database: PathBuf,
    /// Recordings: `<media>/<meeting_id>/{audio.wav,recording.mp4,slides/}`.
    pub media: PathBuf,
    /// Uploaded knowledge-base documents.
    pub documents: PathBuf,
    /// Writable model store (imported / downloaded).
    pub models: PathBuf,
    /// Read-only locations searched for models shipped with the app or placed
    /// by a maintainer (resource dir, then the repository `models/` in dev).
    pub bundled_models: Vec<PathBuf>,
    /// Directory that contains the `sidecar/` Python package.
    pub sidecar_root: PathBuf,
    pub python: String,
}

impl AppPaths {
    /// `data_dir` is Tauri's `app_data_dir()`; `resource_dir` is Tauri's
    /// `resource_dir()` when available.
    pub fn resolve(data_dir: PathBuf, resource_dir: Option<PathBuf>) -> Self {
        let data_dir = std::env::var_os(ENV_DATA_DIR)
            .map(PathBuf::from)
            .unwrap_or(data_dir);
        let dev_root = dev_repository_root();

        let mut bundled_models = Vec::new();
        if let Some(resources) = &resource_dir {
            bundled_models.push(resources.join("models"));
            // `bundle.resources` lists "../models"; Tauri places entries with a
            // parent-directory prefix under `_up_/`.
            bundled_models.push(resources.join("_up_").join("models"));
        }
        if let Some(root) = &dev_root {
            bundled_models.push(root.join("models"));
        }

        let sidecar_root = std::env::var_os(ENV_SIDECAR_DIR)
            .map(PathBuf::from)
            .or_else(|| {
                resource_dir
                    .clone()
                    .filter(|dir| has_sidecar_package(dir))
            })
            .or_else(|| dev_root.clone().filter(|dir| has_sidecar_package(dir)))
            .unwrap_or_else(|| data_dir.clone());

        let python = std::env::var(ENV_PYTHON)
            .ok()
            .or_else(|| venv_python(&sidecar_root))
            .unwrap_or_else(default_python);

        Self {
            database: data_dir.join(DATABASE_FILE),
            media: data_dir.join("media"),
            documents: data_dir.join("documents"),
            models: data_dir.join("models"),
            bundled_models,
            sidecar_root,
            python,
            data_dir,
        }
    }

    /// Where the sidecar should look for models: the writable store first (what the user added),
    /// then the ones shipped with the app. Read by `sidecar/src/resources.py`.
    pub fn sidecar_environment(&self) -> Vec<(String, String)> {
        let dirs = std::iter::once(self.models.clone()).chain(self.bundled_models.iter().cloned());
        std::env::join_paths(dirs)
            .map(|joined| vec![("LOCUS_MODEL_DIRS".to_owned(), joined.to_string_lossy().to_string())])
            .unwrap_or_default()
    }

    pub fn embedding_models(&self) -> PathBuf {
        self.models.join("embedding")
    }

    pub fn generation_models(&self) -> PathBuf {
        self.models.join("generation")
    }

    pub fn whisper_models(&self) -> PathBuf {
        self.models.join("whisper")
    }

    pub fn create_directories(&self) -> io::Result<()> {
        for dir in [
            &self.data_dir,
            &self.media,
            &self.documents,
            &self.models,
            &self.embedding_models(),
            &self.generation_models(),
            &self.whisper_models(),
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Directories searched for Whisper `ggml-*.bin` files, writable first.
    pub fn whisper_search_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.whisper_models(), self.models.clone()];
        dirs.extend(self.bundled_models.iter().cloned());
        dirs
    }

    /// Layout rooted at one directory, for tests.
    pub fn for_root(root: &Path) -> Self {
        Self {
            data_dir: root.to_path_buf(),
            database: root.join(DATABASE_FILE),
            media: root.join("media"),
            documents: root.join("documents"),
            models: root.join("models"),
            bundled_models: vec![root.join("bundled-models")],
            sidecar_root: root.to_path_buf(),
            python: default_python(),
        }
    }
}

fn default_python() -> String {
    if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

/// A project virtualenv next to the sidecar (`.venv`, the usual
/// `python3 -m venv .venv && pip install -r sidecar/requirements.txt`), so the
/// optional OCR / slide / diarization packages are found without activating it.
fn venv_python(sidecar_root: &Path) -> Option<String> {
    let relative: &[&str] = if cfg!(windows) {
        &[".venv/Scripts/python.exe", "sidecar/.venv/Scripts/python.exe"]
    } else {
        &[".venv/bin/python", "sidecar/.venv/bin/python"]
    };
    relative
        .iter()
        .map(|path| sidecar_root.join(path))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
}

fn has_sidecar_package(dir: &Path) -> bool {
    dir.join("sidecar").join("src").join("main.py").is_file()
}

/// The repository checkout this binary was built from, if it still exists.
/// Only meaningful for development builds; installed apps use resource_dir.
fn dev_repository_root() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent()?;
    root.join("sidecar").is_dir().then(|| root.to_path_buf())
}

/// Result of [`adopt_legacy_working_directory_data`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LegacyAdoption {
    pub database: bool,
    pub media: bool,
    pub documents: bool,
}

/// One-time, non-destructive import of data created by earlier builds, which
/// stored `locus.sqlite`, `media/` and `documents/` relative to the working
/// directory. Originals are copied, never moved or deleted, and nothing is
/// imported when the new location already holds a library.
pub fn adopt_legacy_working_directory_data(
    legacy_root: &Path,
    paths: &AppPaths,
) -> io::Result<LegacyAdoption> {
    let mut adopted = LegacyAdoption::default();
    if same_directory(legacy_root, &paths.data_dir) {
        return Ok(adopted);
    }
    let legacy_db = legacy_root.join(DATABASE_FILE);
    if legacy_db.is_file() && !paths.database.exists() {
        fs::create_dir_all(&paths.data_dir)?;
        // Copy the WAL/SHM siblings too: committed transactions may only live there.
        for suffix in ["", "-wal", "-shm"] {
            let from = legacy_root.join(format!("{DATABASE_FILE}{suffix}"));
            if from.is_file() {
                fs::copy(&from, paths.data_dir.join(format!("{DATABASE_FILE}{suffix}")))?;
            }
        }
        adopted.database = true;
    }
    for (name, target, flag) in [
        ("media", &paths.media, &mut adopted.media),
        ("documents", &paths.documents, &mut adopted.documents),
    ] {
        let from = legacy_root.join(name);
        if from.is_dir() && !dir_has_entries(target) {
            copy_dir_recursive(&from, target)?;
            *flag = true;
        }
    }
    Ok(adopted)
}

fn same_directory(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn dir_has_entries(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn copy_dir_recursive(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Finds a Whisper model file. Order: explicit environment override, the
/// user's selection (`whisper:<size>:<origin>`), the small model, then any
/// `ggml-*.bin`. Returns the path and the directory kind it was found in.
pub fn find_whisper_model(
    paths: &AppPaths,
    selection: Option<&str>,
    env_override: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = env_override.filter(|path| path.is_file()) {
        return Some(path);
    }
    let wanted_size = selection
        .and_then(|id| id.trim_matches('"').split(':').nth(1))
        .filter(|size| !size.is_empty())
        .map(str::to_owned);
    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in paths.whisper_search_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("ggml-") && name.ends_with(".bin"))
            })
            .collect();
        found.sort();
        candidates.extend(found);
    }
    let rank = |path: &PathBuf| -> u8 {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if let Some(size) = &wanted_size {
            if name.starts_with(&format!("ggml-{size}")) {
                return 0;
            }
        }
        if name.starts_with("ggml-small") {
            1
        } else {
            2
        }
    };
    candidates.sort_by_key(|path| rank(path));
    candidates.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_models_are_searched_in_both_resource_layouts() {
        let resources = tempfile::tempdir().unwrap();
        let paths = AppPaths::resolve(
            tempfile::tempdir().unwrap().path().to_path_buf(),
            Some(resources.path().to_path_buf()),
        );
        assert!(paths.bundled_models.contains(&resources.path().join("models")));
        assert!(paths.bundled_models.contains(&resources.path().join("_up_").join("models")));
        // and a model sitting in the `_up_` layout is actually found
        fs::create_dir_all(resources.path().join("_up_/models")).unwrap();
        fs::write(resources.path().join("_up_/models/ggml-small.bin"), b"m").unwrap();
        assert!(find_whisper_model(&paths, None, None).is_some());
    }

    #[test]
    fn the_sidecar_is_told_where_models_live_user_store_first() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        let environment = paths.sidecar_environment();
        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0].0, "LOCUS_MODEL_DIRS");
        let dirs: Vec<PathBuf> = std::env::split_paths(&environment[0].1).collect();
        assert_eq!(dirs[0], paths.models);
        assert!(dirs.contains(&paths.bundled_models[0]));
    }

    #[test]
    fn a_project_virtualenv_is_preferred_over_system_python() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(venv_python(root.path()), None);
        let bin = root.path().join(if cfg!(windows) { ".venv/Scripts" } else { ".venv/bin" });
        fs::create_dir_all(&bin).unwrap();
        let python = bin.join(if cfg!(windows) { "python.exe" } else { "python" });
        fs::write(&python, b"").unwrap();
        assert_eq!(venv_python(root.path()), Some(python.to_string_lossy().to_string()));
    }

    #[test]
    fn layout_is_rooted_in_the_data_dir() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        assert_eq!(paths.database, root.path().join("locus.sqlite"));
        assert_eq!(paths.media, root.path().join("media"));
        assert_eq!(paths.embedding_models(), root.path().join("models/embedding"));
        paths.create_directories().unwrap();
        assert!(paths.generation_models().is_dir() && paths.whisper_models().is_dir());
    }

    #[test]
    fn legacy_data_is_copied_once_and_never_deleted() {
        let legacy = tempfile::tempdir().unwrap();
        let new_root = tempfile::tempdir().unwrap();
        fs::write(legacy.path().join("locus.sqlite"), b"db").unwrap();
        fs::write(legacy.path().join("locus.sqlite-wal"), b"wal").unwrap();
        fs::create_dir_all(legacy.path().join("media/m1")).unwrap();
        fs::write(legacy.path().join("media/m1/audio.wav"), b"wav").unwrap();
        let paths = AppPaths::for_root(new_root.path());

        let first = adopt_legacy_working_directory_data(legacy.path(), &paths).unwrap();
        assert_eq!(
            first,
            LegacyAdoption {
                database: true,
                media: true,
                documents: false
            }
        );
        assert_eq!(fs::read(paths.database.clone()).unwrap(), b"db");
        assert!(paths.data_dir.join("locus.sqlite-wal").is_file());
        assert!(paths.media.join("m1/audio.wav").is_file());
        assert!(legacy.path().join("locus.sqlite").is_file(), "originals stay");

        // A second launch must not overwrite the now-live library.
        fs::write(&paths.database, b"newer").unwrap();
        let second = adopt_legacy_working_directory_data(legacy.path(), &paths).unwrap();
        assert_eq!(second, LegacyAdoption::default());
        assert_eq!(fs::read(&paths.database).unwrap(), b"newer");
    }

    #[test]
    fn adoption_is_skipped_when_legacy_and_data_dir_are_the_same() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("locus.sqlite"), b"db").unwrap();
        let paths = AppPaths::for_root(root.path());
        assert_eq!(
            adopt_legacy_working_directory_data(root.path(), &paths).unwrap(),
            LegacyAdoption::default()
        );
    }

    #[test]
    fn whisper_model_lookup_prefers_selection_then_small() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_root(root.path());
        paths.create_directories().unwrap();
        assert!(find_whisper_model(&paths, None, None).is_none());
        fs::write(paths.whisper_models().join("ggml-base.bin"), b"b").unwrap();
        fs::write(paths.models.join("ggml-small.bin"), b"s").unwrap();
        fs::create_dir_all(&paths.bundled_models[0]).unwrap();
        fs::write(paths.bundled_models[0].join("ggml-medium.bin"), b"m").unwrap();
        let name = |p: Option<PathBuf>| {
            p.unwrap().file_name().unwrap().to_string_lossy().to_string()
        };
        assert_eq!(name(find_whisper_model(&paths, None, None)), "ggml-small.bin");
        assert_eq!(
            name(find_whisper_model(&paths, Some("\"whisper:medium:bundled\""), None)),
            "ggml-medium.bin"
        );
        let explicit = root.path().join("custom.bin");
        fs::write(&explicit, b"x").unwrap();
        assert_eq!(
            find_whisper_model(&paths, None, Some(explicit.clone())),
            Some(explicit)
        );
    }
}
