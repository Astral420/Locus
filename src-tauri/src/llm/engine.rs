//! Resolves the `llama-server` used by the embedding server.
//!
//! macOS x86_64 releases ship a patched engine inside the app. Every other
//! platform (and local development without the engine staged) keeps using
//! `llama-server` from `PATH`, exactly as before. Resolution lives here so the
//! app never has more than one answer to "which binary, and is it patched".

use serde::Deserialize;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

const MANIFEST_NAME: &str = "engine.json";
const ENGINE_DIR: &str = "llama-engine";
const LLAMA_SERVER_NAME: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineSource {
    EnvOverride,
    Bundled,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EngineManifest {
    pub toshllm_tag: String,
    pub toshllm_commit: String,
    pub llama_commit: String,
    pub arch: String,
    pub has_kernels: bool,
    pub llama_server_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEngine {
    pub binary: PathBuf,
    pub source: EngineSource,
    pub manifest: Option<EngineManifest>,
}

impl ResolvedEngine {
    fn from_binary(binary: PathBuf, source: EngineSource) -> Self {
        let manifest = binary.parent().and_then(read_manifest);
        Self {
            binary,
            source,
            manifest,
        }
    }
}

/// Finds the engine, preferring an explicit override, then the app bundle and a
/// source checkout, then `PATH`. Never panics; when nothing exists it returns
/// the bare name so the caller keeps the previous behavior.
pub fn resolve_llama_server() -> ResolvedEngine {
    let env_override = std::env::var_os("LOCUS_LLAMA_SERVER_BIN").map(std::path::PathBuf::from);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let cwd_candidates = [PathBuf::from("src-tauri")
        .join(ENGINE_DIR)
        .join(LLAMA_SERVER_NAME)];
    let path_var = std::env::var_os("PATH");

    let resolved = resolve_with(env_override, exe_dir.as_deref(), &cwd_candidates, path_var);
    eprintln!(
        "locus: llama-server source={:?} binary={}{}",
        resolved.source,
        resolved.binary.display(),
        if resolved.manifest.is_some() {
            " (patched engine)"
        } else {
            ""
        }
    );
    resolved
}

/// Pure resolution core. `exe_dir` is the running executable's directory,
/// `cwd_candidates` are paths relative to the current directory, and
/// `path_var` is a `PATH` value to search.
pub(crate) fn resolve_with(
    env_override: Option<PathBuf>,
    exe_dir: Option<&Path>,
    cwd_candidates: &[PathBuf],
    path_var: Option<OsString>,
) -> ResolvedEngine {
    if let Some(binary) = env_override.filter(|candidate| candidate.is_file()) {
        return ResolvedEngine::from_binary(binary, EngineSource::EnvOverride);
    }

    if let Some(dir) = exe_dir {
        let bundled = dir
            .join("../Resources")
            .join(ENGINE_DIR)
            .join(LLAMA_SERVER_NAME);
        if bundled.is_file() {
            return ResolvedEngine::from_binary(bundled, EngineSource::Bundled);
        }
    }

    for candidate in cwd_candidates {
        if candidate.is_file() {
            return ResolvedEngine::from_binary(candidate.clone(), EngineSource::Bundled);
        }
    }

    if let Some(path_var) = path_var {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(LLAMA_SERVER_NAME);
            if candidate.is_file() {
                return ResolvedEngine::from_binary(candidate, EngineSource::Path);
            }
        }
    }

    ResolvedEngine {
        binary: PathBuf::from("llama-server"),
        source: EngineSource::Path,
        manifest: None,
    }
}

/// Reads `engine.json` from `dir`. Returns `None` when the manifest is missing
/// or invalid, which is the normal case for a stock or `PATH` llama-server.
pub fn read_manifest(dir: &Path) -> Option<EngineManifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_NAME)).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn write_file(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"engine").unwrap();
    }

    fn write_manifest(dir: &Path, arch: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(MANIFEST_NAME),
            format!(
                r#"{{"toshllm_tag":"v1","toshllm_commit":"aaa","llama_commit":"bbb",
                    "arch":"{arch}","has_kernels":true,"llama_server_sha256":"ccc"}}"#
            ),
        )
        .unwrap();
    }

    fn path_var(dir: &Path) -> OsString {
        std::env::join_paths([dir]).unwrap()
    }

    #[test]
    fn env_override_wins_over_bundled_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let env_bin = dir.path().join("override/llama-server");
        let bundled = dir
            .path()
            .join("exe/../Resources/llama-engine/llama-server");
        let on_path = dir.path().join("path/llama-server");
        write_file(&env_bin);
        write_file(&bundled);
        write_file(&on_path);

        let resolved = resolve_with(
            Some(env_bin.clone()),
            Some(&dir.path().join("exe")),
            &[],
            Some(path_var(&dir.path().join("path"))),
        );

        assert_eq!(resolved.binary, env_bin);
        assert_eq!(resolved.source, EngineSource::EnvOverride);
    }

    #[test]
    fn bundled_engine_is_found_relative_to_the_executable() {
        let dir = tempfile::tempdir().unwrap();
        let exe_dir = dir.path().join("exe");
        let bundled = exe_dir.join("../Resources/llama-engine/llama-server");
        write_file(&bundled);

        let resolved = resolve_with(None, Some(&exe_dir), &[], None);

        assert_eq!(resolved.binary, bundled);
        assert_eq!(resolved.source, EngineSource::Bundled);
    }

    #[test]
    fn source_checkout_engine_is_found_when_not_bundled() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = dir.path().join("src-tauri/llama-engine/llama-server");
        let on_path = dir.path().join("path/llama-server");
        write_file(&checkout);
        write_file(&on_path);

        let resolved = resolve_with(
            None,
            Some(&dir.path().join("exe")),
            &[checkout.clone()],
            Some(path_var(&dir.path().join("path"))),
        );

        assert_eq!(resolved.binary, checkout);
        assert_eq!(resolved.source, EngineSource::Bundled);
    }

    #[test]
    fn falls_back_to_the_bare_name_when_nothing_exists() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();

        let resolved = resolve_with(
            None,
            Some(&dir.path().join("exe")),
            &[dir.path().join("src-tauri/llama-engine/llama-server")],
            Some(path_var(&empty)),
        );

        assert_eq!(resolved.binary, PathBuf::from("llama-server"));
        assert_eq!(resolved.source, EngineSource::Path);
        assert!(resolved.manifest.is_none());
    }

    #[test]
    fn manifest_is_read_from_the_binary_directory_and_invalid_json_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("engine/llama-server");
        write_file(&binary);
        write_manifest(binary.parent().unwrap(), "x86_64");

        let manifest = read_manifest(binary.parent().unwrap()).unwrap();
        assert_eq!(manifest.arch, "x86_64");
        assert_eq!(manifest.toshllm_tag, "v1");
        assert!(manifest.has_kernels);

        std::fs::write(binary.parent().unwrap().join(MANIFEST_NAME), b"{not json").unwrap();
        assert!(read_manifest(binary.parent().unwrap()).is_none());
    }

    #[test]
    fn env_override_picks_up_a_sibling_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("override/llama-server");
        write_file(&binary);
        write_manifest(binary.parent().unwrap(), "x86_64");

        let resolved = resolve_with(Some(binary.clone()), None, &[], None);

        assert_eq!(resolved.source, EngineSource::EnvOverride);
        assert_eq!(resolved.manifest.unwrap().llama_commit, "bbb");
    }

    #[test]
    fn a_directory_named_llama_server_is_not_a_binary() {
        let dir = tempfile::tempdir().unwrap();
        let override_dir = dir.path().join("override/llama-server");
        let bundled_dir = dir
            .path()
            .join("exe/../Resources/llama-engine/llama-server");
        let checkout_dir = dir.path().join("src-tauri/llama-engine/llama-server");
        let path_dir = dir.path().join("path/llama-server");
        for path in [&override_dir, &bundled_dir, &checkout_dir, &path_dir] {
            std::fs::create_dir_all(path).unwrap();
        }

        let resolved = resolve_with(
            Some(override_dir),
            Some(&dir.path().join("exe")),
            &[checkout_dir],
            Some(path_var(&dir.path().join("path"))),
        );

        assert_eq!(resolved.binary, PathBuf::from("llama-server"));
        assert_eq!(resolved.source, EngineSource::Path);
    }
}
