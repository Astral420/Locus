//! Client for the `locus-model-catalog` repository.
//!
//! The app reads the three catalog files from a local cache and never blocks on the
//! network: [`CatalogService::refresh`] downloads newer copies (conditional GET) on
//! request, and [`CatalogService::snapshot`] turns whatever is cached into models. A new
//! file replaces the cached one only if it parses and still yields at least one
//! verifiable model, so a bad publish cannot wipe a working catalog.

pub mod download;
pub mod model;
pub mod transport;
pub mod types;

use model::{build_embedding, build_llm, build_whisper, CatalogModel, ModelRole, Skipped};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};
use transport::{Fetched, ReqwestTransport, Transport};
use types::CatalogEntry;

pub const DEFAULT_BASE_URL: &str =
    "https://raw.githubusercontent.com/Astral420/locus-model-catalog/main";
/// Overrides [`DEFAULT_BASE_URL`], e.g. a local server or a fork.
pub const ENV_CATALOG_URL: &str = "LOCUS_CATALOG_URL";

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("catalog request failed: {0}")]
    Http(String),
    #[error("catalog file is larger than the allowed maximum")]
    TooLarge,
    #[error("{file} is not a valid catalog: {message}")]
    Parse { file: &'static str, message: String },
    #[error("{0} has no verifiable models; keeping the previous copy")]
    NoUsableModels(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogFile {
    Whisper,
    Llm,
    Embedding,
}

impl CatalogFile {
    pub const ALL: [CatalogFile; 3] = [CatalogFile::Whisper, CatalogFile::Llm, CatalogFile::Embedding];

    pub fn filename(self) -> &'static str {
        match self {
            CatalogFile::Whisper => "whisper_catalog.json",
            CatalogFile::Llm => "model_catalog.json",
            CatalogFile::Embedding => "embedding_catalog.json",
        }
    }

    fn build(self, entries: &[CatalogEntry]) -> (Vec<CatalogModel>, Skipped) {
        match self {
            CatalogFile::Whisper => build_whisper(entries),
            CatalogFile::Llm => build_llm(entries),
            CatalogFile::Embedding => build_embedding(entries),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshReport {
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    /// (file, reason). A failed file keeps whatever copy was already cached.
    pub failed: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogSnapshot {
    pub whisper: Vec<CatalogModel>,
    pub llm: Vec<CatalogModel>,
    pub embedding: Vec<CatalogModel>,
    /// Files that are missing or unreadable, so the UI can say why a list is empty.
    pub problems: Vec<String>,
    pub skipped: Skipped,
}

impl CatalogSnapshot {
    pub fn models(&self, role: ModelRole) -> &[CatalogModel] {
        match role {
            ModelRole::Whisper => &self.whisper,
            ModelRole::Llm => &self.llm,
            ModelRole::Embedding => &self.embedding,
        }
    }

    pub fn find(&self, catalog_id: &str) -> Option<&CatalogModel> {
        self.whisper
            .iter()
            .chain(&self.llm)
            .chain(&self.embedding)
            .find(|m| m.catalog_id == catalog_id)
    }
}

type ParsedCache = HashMap<CatalogFile, (SystemTime, u64, Arc<Vec<CatalogEntry>>)>;

#[derive(Clone)]
pub struct CatalogService {
    dir: PathBuf,
    base_url: String,
    transport: Arc<dyn Transport>,
    parsed: Arc<Mutex<ParsedCache>>,
}

impl CatalogService {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let base_url = std::env::var(ENV_CATALOG_URL).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_transport(dir, base_url, Arc::new(ReqwestTransport::new()))
    }

    pub fn with_transport(
        dir: impl Into<PathBuf>,
        base_url: impl Into<String>,
        transport: Arc<dyn Transport>,
    ) -> Self {
        Self {
            dir: dir.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            transport,
            parsed: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn path(&self, file: CatalogFile) -> PathBuf {
        self.dir.join(file.filename())
    }

    fn etag_path(&self, file: CatalogFile) -> PathBuf {
        self.dir.join(format!("{}.etag", file.filename()))
    }

    /// Download newer copies of all three files. Blocking: call from a worker thread.
    pub fn refresh(&self) -> RefreshReport {
        let mut report = RefreshReport::default();
        for file in CatalogFile::ALL {
            let name = file.filename().to_string();
            match self.refresh_one(file) {
                Ok(true) => report.updated.push(name),
                Ok(false) => report.unchanged.push(name),
                Err(error) => report.failed.push((name, error.to_string())),
            }
        }
        report
    }

    fn refresh_one(&self, file: CatalogFile) -> Result<bool, CatalogError> {
        let cached = self.path(file).is_file();
        let etag = if cached { fs::read_to_string(self.etag_path(file)).ok() } else { None };
        let url = format!("{}/{}", self.base_url, file.filename());
        let (bytes, new_etag) = match self.transport.get(&url, etag.as_deref().map(str::trim))? {
            Fetched::NotModified if cached => return Ok(false),
            Fetched::NotModified => {
                return Err(CatalogError::Http("304 but nothing is cached".into()));
            }
            Fetched::Body { bytes, etag } => (bytes, etag),
        };

        let entries = parse(file, &bytes)?;
        if file.build(&entries).0.is_empty() {
            return Err(CatalogError::NoUsableModels(file.filename()));
        }
        if cached && fs::read(self.path(file)).is_ok_and(|old| old == bytes) {
            return Ok(false);
        }

        fs::create_dir_all(&self.dir)?;
        write_atomic(&self.path(file), &bytes)?;
        match new_etag {
            Some(etag) => write_atomic(&self.etag_path(file), etag.as_bytes())?,
            None => {
                let _ = fs::remove_file(self.etag_path(file));
            }
        }
        Ok(true)
    }

    /// Everything currently cached, as models. Never touches the network.
    pub fn snapshot(&self) -> CatalogSnapshot {
        let mut snapshot = CatalogSnapshot::default();
        for file in CatalogFile::ALL {
            let entries = match self.load(file) {
                Ok(Some(entries)) => entries,
                Ok(None) => continue,
                Err(error) => {
                    snapshot.problems.push(error.to_string());
                    continue;
                }
            };
            let (models, skipped) = file.build(&entries);
            snapshot.skipped.entries_without_verified_files += skipped.entries_without_verified_files;
            snapshot.skipped.files_rejected += skipped.files_rejected;
            match file {
                CatalogFile::Whisper => snapshot.whisper = models,
                CatalogFile::Llm => snapshot.llm = models,
                CatalogFile::Embedding => snapshot.embedding = models,
            }
        }
        snapshot
    }

    fn load(&self, file: CatalogFile) -> Result<Option<Arc<Vec<CatalogEntry>>>, CatalogError> {
        let path = self.path(file);
        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let stamp = (metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), metadata.len());
        if let Some((modified, len, entries)) = self.parsed.lock().unwrap().get(&file) {
            if (*modified, *len) == stamp {
                return Ok(Some(Arc::clone(entries)));
            }
        }
        let entries = Arc::new(parse(file, &fs::read(&path)?)?);
        self.parsed
            .lock()
            .unwrap()
            .insert(file, (stamp.0, stamp.1, Arc::clone(&entries)));
        Ok(Some(entries))
    }
}

fn parse(file: CatalogFile, bytes: &[u8]) -> Result<Vec<CatalogEntry>, CatalogError> {
    let entries: Vec<CatalogEntry> = serde_json::from_slice(bytes).map_err(|e| CatalogError::Parse {
        file: file.filename(),
        message: e.to_string(),
    })?;
    if entries.is_empty() {
        return Err(CatalogError::Parse { file: file.filename(), message: "the list is empty".into() });
    }
    Ok(entries)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    /// Serves canned responses and records every request.
    struct Fake {
        responses: StdMutex<HashMap<String, Result<Fetched, String>>>,
        requests: StdMutex<Vec<(String, Option<String>)>>,
    }

    impl Fake {
        fn new(items: Vec<(&str, Result<Fetched, String>)>) -> Arc<Self> {
            Arc::new(Self {
                responses: StdMutex::new(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()),
                requests: StdMutex::new(vec![]),
            })
        }
        fn set(&self, file: &str, response: Result<Fetched, String>) {
            self.responses.lock().unwrap().insert(file.to_string(), response);
        }
    }

    impl Transport for Fake {
        fn get(&self, url: &str, etag: Option<&str>) -> Result<Fetched, CatalogError> {
            self.requests.lock().unwrap().push((url.to_string(), etag.map(str::to_string)));
            let file = url.rsplit('/').next().unwrap();
            match self.responses.lock().unwrap().remove(file) {
                Some(Ok(fetched)) => Ok(fetched),
                Some(Err(message)) => Err(CatalogError::Http(message)),
                None => Err(CatalogError::Http(format!("no canned response for {file}"))),
            }
        }
    }

    fn body(json: &str, etag: &str) -> Result<Fetched, String> {
        Ok(Fetched::Body { bytes: json.as_bytes().to_vec(), etag: Some(etag.into()) })
    }

    fn whisper_json(size: u64) -> String {
        format!(
            r#"[{{"model_name":"whisper.cpp","developer":"ggerganov","downloads":5,"revision":"{REV}",
              "quants":[{{"model_id":"ggml-small","file_size":"1 GB","size_bytes":{size},
                "path":"https://huggingface.co/ggerganov/whisper.cpp/resolve/{REV}/ggml-small.bin",
                "sha256":"{}"}}]}}]"#,
            "ab".repeat(32)
        )
    }

    const LEGACY_LLM: &str = r#"[{"model_name":"old-GGUF","developer":"x","quants":[
        {"model_id":"o-Q4_K_M","path":"https://huggingface.co/x/old/resolve/main/o.gguf","file_size":"1 GB"}]}]"#;

    fn service(dir: &Path, fake: &Arc<Fake>) -> CatalogService {
        CatalogService::with_transport(dir, "https://example.test/cat/", fake.clone())
    }

    #[test]
    fn refresh_writes_files_and_snapshot_reads_them_without_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(vec![
            ("whisper_catalog.json", body(&whisper_json(488), "\"v1\"")),
            ("model_catalog.json", Err("offline".into())),
            ("embedding_catalog.json", Err("offline".into())),
        ]);
        let svc = service(dir.path(), &fake);
        let report = svc.refresh();
        assert_eq!(report.updated, ["whisper_catalog.json"]);
        assert_eq!(report.failed.len(), 2);
        assert_eq!(fake.requests.lock().unwrap()[0].0, "https://example.test/cat/whisper_catalog.json");

        let before = fake.requests.lock().unwrap().len();
        let snapshot = svc.snapshot();
        assert_eq!(fake.requests.lock().unwrap().len(), before, "snapshot must not use the network");
        assert_eq!(snapshot.whisper[0].catalog_id, "whisper-small");
        assert!(snapshot.llm.is_empty() && snapshot.problems.is_empty());
        assert!(snapshot.find("whisper-small").is_some() && snapshot.find("nope").is_none());
    }

    #[test]
    fn second_refresh_sends_the_etag_and_a_304_keeps_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(vec![
            ("whisper_catalog.json", body(&whisper_json(488), "\"v1\"")),
            ("model_catalog.json", Err("x".into())),
            ("embedding_catalog.json", Err("x".into())),
        ]);
        let svc = service(dir.path(), &fake);
        svc.refresh();
        fake.set("whisper_catalog.json", Ok(Fetched::NotModified));
        fake.set("model_catalog.json", Err("x".into()));
        fake.set("embedding_catalog.json", Err("x".into()));
        let report = svc.refresh();
        assert_eq!(report.unchanged, ["whisper_catalog.json"]);
        let sent = fake.requests.lock().unwrap();
        let second = sent.iter().filter(|(u, _)| u.ends_with("whisper_catalog.json")).nth(1).unwrap();
        assert_eq!(second.1.as_deref(), Some("\"v1\""));
        drop(sent);
        assert_eq!(svc.snapshot().whisper.len(), 1);
    }

    #[test]
    fn identical_content_is_reported_unchanged_even_without_a_304() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(vec![
            ("whisper_catalog.json", body(&whisper_json(488), "\"a\"")),
            ("model_catalog.json", Err("x".into())),
            ("embedding_catalog.json", Err("x".into())),
        ]);
        let svc = service(dir.path(), &fake);
        svc.refresh();
        fake.set("whisper_catalog.json", body(&whisper_json(488), "\"b\""));
        fake.set("model_catalog.json", Err("x".into()));
        fake.set("embedding_catalog.json", Err("x".into()));
        assert_eq!(svc.refresh().unchanged, ["whisper_catalog.json"]);
    }

    #[test]
    fn a_bad_publish_never_replaces_a_working_cache() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(vec![
            ("whisper_catalog.json", body(&whisper_json(488), "\"good\"")),
            ("model_catalog.json", Err("x".into())),
            ("embedding_catalog.json", Err("x".into())),
        ]);
        let svc = service(dir.path(), &fake);
        svc.refresh();

        for bad in ["not json", "[]", LEGACY_LLM] {
            fake.set("whisper_catalog.json", body(bad, "\"bad\""));
            fake.set("model_catalog.json", Err("x".into()));
            fake.set("embedding_catalog.json", Err("x".into()));
            let report = svc.refresh();
            assert_eq!(report.failed.iter().filter(|(f, _)| f == "whisper_catalog.json").count(), 1, "{bad}");
        }
        let snapshot = svc.snapshot();
        assert_eq!(snapshot.whisper.len(), 1, "previous copy must survive");
        assert_eq!(fs::read_to_string(dir.path().join("whisper_catalog.json.etag")).unwrap(), "\"good\"");
    }

    #[test]
    fn a_legacy_catalog_without_checksums_is_refused_on_first_download() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new(vec![
            ("whisper_catalog.json", Err("x".into())),
            ("model_catalog.json", body(LEGACY_LLM, "\"l\"")),
            ("embedding_catalog.json", Err("x".into())),
        ]);
        let svc = service(dir.path(), &fake);
        let report = svc.refresh();
        assert!(report.failed.iter().any(|(f, m)| f == "model_catalog.json" && m.contains("no verifiable models")));
        assert!(!dir.path().join("model_catalog.json").exists());
    }

    #[test]
    fn a_corrupt_cache_file_is_reported_as_a_problem_not_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("whisper_catalog.json"), "{{{").unwrap();
        let svc = service(dir.path(), &Fake::new(vec![]));
        let snapshot = svc.snapshot();
        assert!(snapshot.whisper.is_empty());
        assert_eq!(snapshot.problems.len(), 1);
        assert!(snapshot.problems[0].contains("whisper_catalog.json"));
    }

    #[test]
    fn snapshot_reparses_after_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service(dir.path(), &Fake::new(vec![]));
        fs::write(dir.path().join("whisper_catalog.json"), whisper_json(488)).unwrap();
        assert_eq!(svc.snapshot().whisper[0].variants[0].file.size_bytes, 488);
        fs::write(dir.path().join("whisper_catalog.json"), whisper_json(99999)).unwrap();
        assert_eq!(svc.snapshot().whisper[0].variants[0].file.size_bytes, 99999);
    }

    /// Fixtures are produced by the real `prepare_*_catalog.py` scripts of locus-model-catalog
    /// (with synthetic Hub responses), so this fails if either side changes the format.
    #[test]
    fn parses_the_files_the_catalog_scripts_generate() {
        let dir = tempfile::tempdir().unwrap();
        for (name, json) in [
            ("whisper_catalog.json", include_str!("testdata/whisper_catalog.json")),
            ("model_catalog.json", include_str!("testdata/model_catalog.json")),
            ("embedding_catalog.json", include_str!("testdata/embedding_catalog.json")),
        ] {
            fs::write(dir.path().join(name), json).unwrap();
        }
        let snapshot = service(dir.path(), &Fake::new(vec![])).snapshot();
        assert!(snapshot.problems.is_empty(), "{:?}", snapshot.problems);

        let ids = |models: &[CatalogModel]| models.iter().map(|m| m.catalog_id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&snapshot.whisper), ["whisper-tiny", "whisper-small", "whisper-small-en"]);
        let small = snapshot.find("whisper-small").unwrap();
        assert_eq!(small.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["q5_1", "standard"]);

        // Ranking puts the recommended Qwen first; the pre-sha256 entry is dropped, not offered.
        assert_eq!(ids(&snapshot.llm), ["unsloth/qwen3-4b-instruct-2507", "unsloth/gemma-3-4b-it"]);
        assert_eq!(snapshot.skipped.entries_without_verified_files, 1);
        let gemma = snapshot.find("unsloth/gemma-3-4b-it").unwrap();
        assert_eq!(gemma.variants.len(), 2, "the mmproj file is not a model variant of its own");
        assert!(gemma.variants.iter().all(|v| v.file.url.contains("/resolve/0123456789abcdef0123456789abcdef01234567/")));

        let minilm = snapshot.find("all-minilm-l6-v2").unwrap();
        assert_eq!(minilm.embedding.as_ref().unwrap().dimension, 384);
        assert_eq!(minilm.variants[0].id, "q8_0");
    }

    #[test]
    fn empty_cache_directory_gives_an_empty_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = service(dir.path(), &Fake::new(vec![])).snapshot();
        assert!(snapshot.whisper.is_empty() && snapshot.llm.is_empty() && snapshot.embedding.is_empty());
        assert!(snapshot.problems.is_empty());
    }
}
