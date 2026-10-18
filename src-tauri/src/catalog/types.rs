//! Wire types for the files published by `locus-model-catalog`.
//!
//! The catalog keeps Jan's layout: each file is a JSON list with one entry per Hugging Face
//! repository and one `quants` item per downloadable file. Locus' additions are
//! `quants[].size_bytes`, `quants[].sha256`, `revision`, and the `embedding` object.
//!
//! Everything here is untrusted input. A file is only offered for download after
//! [`Quant::verify`] accepts it.

use serde::Deserialize;

/// The only host a catalog URL may point at. Hugging Face redirects large files to its
/// CDN; that redirect is followed by the downloader, but a catalog entry itself must name
/// this host, so a bad catalog cannot send Locus to an arbitrary server.
pub const ALLOWED_HOSTS: &[&str] = &["huggingface.co"];

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub model_name: String,
    pub developer: String,
    #[serde(default)]
    pub downloads: Option<u64>,
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
    #[serde(default)]
    pub pipeline_tag: Option<String>,
    #[serde(default)]
    pub tools: Option<bool>,
    #[serde(default)]
    pub quants: Vec<Quant>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub embedding: Option<EmbeddingMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Quant {
    pub model_id: String,
    pub path: String,
    /// Human-readable size from Jan's format ("2.3 GB"). Display only.
    #[serde(default)]
    pub file_size: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EmbeddingMeta {
    pub dimension: usize,
    pub max_sequence_tokens: usize,
    #[serde(default)]
    pub query_prefix: String,
    #[serde(default)]
    pub document_prefix: String,
}

/// A file that passed every check and is safe to hand to the downloader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedFile {
    pub filename: String,
    pub url: String,
    pub size_bytes: u64,
    /// Lower-case hex.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Reject {
    #[error("no sha256")]
    NoSha256,
    #[error("sha256 is not 64 hex characters")]
    BadSha256,
    #[error("no size_bytes")]
    NoSize,
    #[error("url must be https on an allowed host")]
    BadUrl,
    #[error("url is not pinned to a commit")]
    NotPinned,
    #[error("url commit does not match the entry revision")]
    RevisionMismatch,
    #[error("unsafe or unsupported filename")]
    BadFilename,
}

impl Quant {
    /// Accept this file only if it can be downloaded and verified without trusting
    /// anything but the catalog's checksum: https on an allowed host, a commit-pinned
    /// URL (a branch name would make the checksum go stale on the next upstream push),
    /// a plain filename, and a sha256 plus size.
    pub fn verify(&self, entry_revision: Option<&str>) -> Result<VerifiedFile, Reject> {
        let sha256 = self.sha256.as_deref().ok_or(Reject::NoSha256)?;
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Reject::BadSha256);
        }
        let size_bytes = match self.size_bytes {
            Some(size) if size > 0 => size,
            _ => return Err(Reject::NoSize),
        };

        let rest = self.path.strip_prefix("https://").ok_or(Reject::BadUrl)?;
        let (host, path_and_query) = rest.split_once('/').ok_or(Reject::BadUrl)?;
        if !ALLOWED_HOSTS.contains(&host) {
            return Err(Reject::BadUrl);
        }
        let path = path_and_query.split(['?', '#']).next().unwrap_or("");
        if path.contains('\\') || path.contains("..") {
            return Err(Reject::BadUrl);
        }

        let segments: Vec<&str> = path.split('/').collect();
        let resolve = segments
            .iter()
            .position(|s| *s == "resolve")
            .ok_or(Reject::NotPinned)?;
        let commit = segments.get(resolve + 1).copied().unwrap_or("");
        if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Reject::NotPinned);
        }
        if let Some(revision) = entry_revision {
            if !revision.eq_ignore_ascii_case(commit) {
                return Err(Reject::RevisionMismatch);
            }
        }

        let filename = segments.last().copied().unwrap_or("");
        if !filename_is_safe(filename) {
            return Err(Reject::BadFilename);
        }
        Ok(VerifiedFile {
            filename: filename.to_string(),
            url: self.path.clone(),
            size_bytes,
            sha256: sha256.to_ascii_lowercase(),
        })
    }
}

fn filename_is_safe(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && (lower.ends_with(".gguf") || lower.ends_with(".bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    fn quant(path: &str) -> Quant {
        Quant {
            model_id: "m".into(),
            path: path.into(),
            file_size: Some("1 GB".into()),
            size_bytes: Some(1_000),
            sha256: Some("ab".repeat(32)),
        }
    }

    fn ok_url() -> String {
        format!("https://huggingface.co/o/r/resolve/{REV}/m-Q4_K_M.gguf")
    }

    #[test]
    fn accepts_a_pinned_hugging_face_file() {
        let file = quant(&ok_url()).verify(Some(REV)).unwrap();
        assert_eq!(file.filename, "m-Q4_K_M.gguf");
        assert_eq!(file.size_bytes, 1_000);
        assert_eq!(file.sha256, "ab".repeat(32));
    }

    #[test]
    fn sha256_is_normalised_to_lower_case() {
        let mut q = quant(&ok_url());
        q.sha256 = Some("AB".repeat(32));
        assert_eq!(q.verify(None).unwrap().sha256, "ab".repeat(32));
    }

    #[test]
    fn rejects_files_without_a_usable_checksum_or_size() {
        let mut q = quant(&ok_url());
        q.sha256 = None;
        assert_eq!(q.verify(None), Err(Reject::NoSha256));
        q.sha256 = Some("zz".repeat(32));
        assert_eq!(q.verify(None), Err(Reject::BadSha256));
        q.sha256 = Some("ab".repeat(31));
        assert_eq!(q.verify(None), Err(Reject::BadSha256));
        let mut q = quant(&ok_url());
        q.size_bytes = None;
        assert_eq!(q.verify(None), Err(Reject::NoSize));
        q.size_bytes = Some(0);
        assert_eq!(q.verify(None), Err(Reject::NoSize));
    }

    #[test]
    fn rejects_other_hosts_schemes_and_lookalikes() {
        for url in [
            format!("http://huggingface.co/o/r/resolve/{REV}/m.gguf"),
            format!("https://evil.example/o/r/resolve/{REV}/m.gguf"),
            format!("https://huggingface.co.evil.example/o/r/resolve/{REV}/m.gguf"),
            format!("https://huggingface.co@evil.example/o/r/resolve/{REV}/m.gguf"),
            format!("https://huggingface.co:8443/o/r/resolve/{REV}/m.gguf"),
            "https://huggingface.co".to_string(),
        ] {
            assert_eq!(quant(&url).verify(None), Err(Reject::BadUrl), "{url}");
        }
    }

    #[test]
    fn rejects_branch_urls_because_the_checksum_would_go_stale() {
        let q = quant("https://huggingface.co/o/r/resolve/main/m.gguf");
        assert_eq!(q.verify(None), Err(Reject::NotPinned));
    }

    #[test]
    fn rejects_a_commit_that_differs_from_the_entry_revision() {
        let other = "f".repeat(40);
        assert_eq!(
            quant(&ok_url()).verify(Some(&other)),
            Err(Reject::RevisionMismatch)
        );
    }

    #[test]
    fn rejects_unsafe_filenames_and_traversal() {
        for url in [
            format!("https://huggingface.co/o/r/resolve/{REV}/../m.gguf"),
            format!("https://huggingface.co/o/r/resolve/{REV}/m.exe"),
            format!("https://huggingface.co/o/r/resolve/{REV}/.hidden.gguf"),
            format!("https://huggingface.co/o/r/resolve/{REV}/a%2Fb.gguf"),
            format!("https://huggingface.co/o/r/resolve/{REV}/"),
        ] {
            assert!(quant(&url).verify(None).is_err(), "{url}");
        }
    }

    #[test]
    fn query_strings_do_not_leak_into_the_filename() {
        let q = quant(&format!("{}?download=true", ok_url()));
        assert_eq!(q.verify(None).unwrap().filename, "m-Q4_K_M.gguf");
    }

    #[test]
    fn deserialises_jans_shape_with_locus_additions_and_tolerates_nulls() {
        let json = r#"[{
            "model_name": "gemma-3-4b-it-GGUF", "developer": "unsloth", "downloads": null,
            "createdAt": "2025-03-12T10:00:00.000Z", "pipeline_tag": "image-text-to-text",
            "tools": false, "num_quants": 1, "num_mmproj": 0, "mmproj_models": [],
            "quants": [{"model_id": "g-Q4_K_M", "path": "https://huggingface.co/x", "file_size": "2.3 GB",
                        "size_bytes": 5, "sha256": "ab", "extra": 1}],
            "readme": null, "description": "d", "revision": null
        }]"#;
        let entries: Vec<CatalogEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries[0].developer, "unsloth");
        assert_eq!(entries[0].downloads, None);
        assert_eq!(entries[0].quants[0].size_bytes, Some(5));
    }
}
