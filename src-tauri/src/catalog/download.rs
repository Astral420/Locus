//! Verified model downloads.
//!
//! A file is hashed while it streams and only moved to its final name after its size and
//! sha256 match the catalog. A bad download never leaves a file under the real name (the
//! older per-role managers renamed first and checked afterwards). Interrupted downloads
//! keep a `.part` file and resume with a Range request.

use super::types::VerifiedFile;
use reqwest::{blocking::Client, header, redirect, StatusCode};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download failed: {0}")]
    Http(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("download canceled")]
    Canceled,
    #[error("size mismatch: the catalog says {expected} bytes, received {actual}")]
    Size { expected: u64, actual: u64 },
    #[error("checksum mismatch: the catalog says {expected}, received {actual}")]
    Checksum { expected: String, actual: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
}

/// Hugging Face serves large files from its own CDN hosts after a redirect. A redirect
/// anywhere else is refused.
const REDIRECT_HOST_SUFFIXES: &[&str] = &["huggingface.co", "hf.co"];
const MAX_REDIRECTS: usize = 5;

pub fn redirect_host_allowed(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    REDIRECT_HOST_SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

/// Client for model downloads: redirects are limited to Hugging Face hosts over https.
pub fn client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // The blocking client has no per-read timeout, so a hard cap bounds a stalled
        // transfer; the user can also cancel at any time and resume later.
        .timeout(Duration::from_secs(6 * 60 * 60))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .user_agent(concat!("locus/", env!("CARGO_PKG_VERSION"), " model-download"))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if attempt.url().scheme() == "https"
                && attempt.url().host_str().is_some_and(redirect_host_allowed)
            {
                attempt.follow()
            } else {
                attempt.error("redirect to a host that is not allowed")
            }
        }))
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn hash_file_into(path: &Path, hasher: &mut Sha256) -> std::io::Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut total = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(total);
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
}

fn hex(hasher: Sha256) -> String {
    format!("{:x}", hasher.finalize())
}

/// Download `file` into `dest_dir` and return its final path.
///
/// An already-present file with the right size and checksum is reused without a request.
pub fn download_verified<F: FnMut(Progress)>(
    client: &Client,
    file: &VerifiedFile,
    dest_dir: &Path,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<PathBuf, DownloadError> {
    fs::create_dir_all(dest_dir)?;
    let destination = dest_dir.join(&file.filename);
    let partial = dest_dir.join(format!("{}.part", file.filename));
    let total = file.size_bytes;

    if destination.is_file() && fs::metadata(&destination)?.len() == total {
        let mut hasher = Sha256::new();
        hash_file_into(&destination, &mut hasher)?;
        if hex(hasher) == file.sha256 {
            on_progress(Progress { received: total, total });
            return Ok(destination);
        }
    }

    // Resume: re-hash what is already on disk, then ask for the rest.
    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    match fs::metadata(&partial) {
        Ok(meta) if meta.len() > 0 && meta.len() < total => {
            received = hash_file_into(&partial, &mut hasher)?;
        }
        Ok(_) => fs::remove_file(&partial)?,
        Err(_) => {}
    }

    let mut request = client.get(&file.url);
    if received > 0 {
        request = request.header(header::RANGE, format!("bytes={received}-"));
    }
    let mut response = request
        .send()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let status = response.status();
    let resuming = received > 0 && status == StatusCode::PARTIAL_CONTENT;
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        let _ = fs::remove_file(&partial);
        return Err(DownloadError::Http("the server rejected the resume request; try again".into()));
    }
    if !status.is_success() {
        return Err(DownloadError::Http(format!("{status} for {}", file.url)));
    }
    if !resuming {
        // A fresh start, or the server ignored the Range header and sent everything.
        received = 0;
        hasher = Sha256::new();
    }

    let mut output = if resuming {
        fs::OpenOptions::new().append(true).open(&partial)?
    } else {
        fs::File::create(&partial)?
    };
    on_progress(Progress { received, total });
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            output.flush()?;
            return Err(DownloadError::Canceled); // the .part file stays for a later resume
        }
        let read = match response.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                output.flush()?;
                return Err(DownloadError::Http(error.to_string()));
            }
        };
        output.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        received += read as u64;
        if received > total {
            drop(output);
            let _ = fs::remove_file(&partial);
            return Err(DownloadError::Size { expected: total, actual: received });
        }
        on_progress(Progress { received, total });
    }
    output.flush()?;
    drop(output);

    if received < total {
        // Connection ended early: keep the partial file so the next attempt resumes.
        return Err(DownloadError::Http(format!(
            "connection ended after {received} of {total} bytes"
        )));
    }
    let actual = hex(hasher);
    if actual != file.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(DownloadError::Checksum { expected: file.sha256.clone(), actual });
    }
    fs::rename(&partial, &destination)?;
    Ok(destination)
}

/// Which downloads are running, so a second request is refused and a cancel can find its flag.
#[derive(Clone, Default)]
pub struct DownloadRegistry(std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<AtomicBool>>>>);

impl DownloadRegistry {
    /// Start tracking `key`. `None` if it is already running.
    pub fn begin(&self, key: &str) -> Option<std::sync::Arc<AtomicBool>> {
        let mut running = self.0.lock().unwrap();
        if running.contains_key(key) {
            return None;
        }
        let flag = std::sync::Arc::new(AtomicBool::new(false));
        running.insert(key.to_string(), flag.clone());
        Some(flag)
    }

    /// Ask a running download to stop. False if nothing was running under `key`.
    pub fn cancel(&self, key: &str) -> bool {
        match self.0.lock().unwrap().get(key) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub fn finish(&self, key: &str) {
        self.0.lock().unwrap().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::{TcpListener, TcpStream},
        sync::Arc,
        thread,
    };

    #[derive(Clone, Copy)]
    enum Mode {
        /// Honours Range requests.
        Normal,
        /// Always sends the whole body with 200, ignoring Range.
        IgnoreRange,
        /// Sends only the first `n` bytes, then closes.
        Truncate(usize),
        /// Sends 5 extra bytes.
        TooLong,
        Redirect(&'static str),
    }

    fn body() -> Vec<u8> {
        (0..300_000_u32).map(|i| (i % 251) as u8).collect()
    }

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn handle(mut stream: TcpStream, data: &[u8], mode: Mode) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut range_from: Option<usize> = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(rest) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                range_from = rest.trim().trim_end_matches('-').parse().ok();
            }
        }
        let (status, payload): (&str, Vec<u8>) = match (mode, range_from) {
            (Mode::Redirect(to), _) => {
                let head = format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(head.as_bytes());
                return;
            }
            (Mode::Normal, Some(from)) => ("206 Partial Content", data[from..].to_vec()),
            (Mode::Truncate(n), _) => ("200 OK", data[..n].to_vec()),
            (Mode::TooLong, _) => ("200 OK", [data, b"extra"].concat()),
            _ => ("200 OK", data.to_vec()),
        };
        let declared = match mode {
            Mode::Truncate(_) => data.len(),
            _ => payload.len(),
        };
        let head = format!("HTTP/1.1 {status}\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n");
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&payload);
    }

    fn serve(mode: Mode) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/m.gguf", listener.local_addr().unwrap());
        let data = Arc::new(body());
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                handle(stream, &data, mode);
            }
        });
        url
    }

    fn file(url: &str) -> VerifiedFile {
        let data = body();
        VerifiedFile { filename: "m.gguf".into(), url: url.into(), size_bytes: data.len() as u64, sha256: sha(&data) }
    }

    fn run(url: &str, dir: &Path, cancel: &AtomicBool) -> (Result<PathBuf, DownloadError>, Vec<Progress>) {
        let mut seen = vec![];
        let result = download_verified(&Client::new(), &file(url), dir, cancel, |p| seen.push(p));
        (result, seen)
    }

    #[test]
    fn downloads_verifies_and_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let (result, progress) = run(&serve(Mode::Normal), dir.path(), &AtomicBool::new(false));
        let path = result.unwrap();
        assert_eq!(fs::read(&path).unwrap(), body());
        assert!(!dir.path().join("m.gguf.part").exists());
        assert_eq!(progress.last().unwrap().received, 300_000);
        assert!(progress.windows(2).all(|w| w[0].received <= w[1].received));
    }

    #[test]
    fn a_checksum_mismatch_leaves_nothing_under_the_real_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut wrong = file(&serve(Mode::Normal));
        wrong.sha256 = "0".repeat(64);
        let result = download_verified(&Client::new(), &wrong, dir.path(), &AtomicBool::new(false), |_| {});
        assert!(matches!(result, Err(DownloadError::Checksum { .. })));
        assert!(!dir.path().join("m.gguf").exists());
        assert!(!dir.path().join("m.gguf.part").exists());
    }

    #[test]
    fn a_body_longer_than_the_catalog_size_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (result, _) = run(&serve(Mode::TooLong), dir.path(), &AtomicBool::new(false));
        assert!(matches!(result, Err(DownloadError::Size { .. })));
        assert!(!dir.path().join("m.gguf").exists() && !dir.path().join("m.gguf.part").exists());
    }

    #[test]
    fn an_interrupted_download_resumes_from_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let (first, _) = run(&serve(Mode::Truncate(100_000)), dir.path(), &AtomicBool::new(false));
        assert!(matches!(first, Err(DownloadError::Http(_))));
        let part = dir.path().join("m.gguf.part");
        assert_eq!(fs::metadata(&part).unwrap().len(), 100_000);

        let (second, progress) = run(&serve(Mode::Normal), dir.path(), &AtomicBool::new(false));
        assert_eq!(fs::read(second.unwrap()).unwrap(), body());
        assert_eq!(progress[0].received, 100_000, "started from the saved bytes");
        assert!(!part.exists());
    }

    #[test]
    fn a_server_that_ignores_range_restarts_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("m.gguf.part"), &body()[..50_000]).unwrap();
        let (result, progress) = run(&serve(Mode::IgnoreRange), dir.path(), &AtomicBool::new(false));
        assert_eq!(fs::read(result.unwrap()).unwrap(), body());
        assert_eq!(progress[0].received, 0);
    }

    #[test]
    fn cancel_stops_and_keeps_the_partial_file_for_later() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(true);
        let (result, _) = run(&serve(Mode::Normal), dir.path(), &cancel);
        assert!(matches!(result, Err(DownloadError::Canceled)));
        assert!(!dir.path().join("m.gguf").exists());
    }

    #[test]
    fn a_valid_file_already_on_disk_is_reused_without_a_request() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("m.gguf"), body()).unwrap();
        let (result, progress) = run("http://127.0.0.1:1/never-contacted", dir.path(), &AtomicBool::new(false));
        assert!(result.is_ok());
        assert_eq!(progress, [Progress { received: 300_000, total: 300_000 }]);
    }

    #[test]
    fn a_corrupt_file_on_disk_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("m.gguf"), vec![0_u8; 300_000]).unwrap();
        let (result, _) = run(&serve(Mode::Normal), dir.path(), &AtomicBool::new(false));
        assert_eq!(fs::read(result.unwrap()).unwrap(), body());
    }

    #[test]
    fn the_registry_refuses_duplicates_and_routes_cancels() {
        let registry = DownloadRegistry::default();
        let flag = registry.begin("a").unwrap();
        assert!(registry.begin("a").is_none());
        assert!(registry.begin("b").is_some());
        assert!(!registry.cancel("missing"));
        assert!(!flag.load(Ordering::Relaxed));
        assert!(registry.cancel("a") && flag.load(Ordering::Relaxed));
        registry.finish("a");
        assert!(registry.begin("a").is_some(), "usable again after finish");
    }

    #[test]
    fn redirects_are_limited_to_hugging_face_hosts() {
        for ok in ["huggingface.co", "cdn-lfs.hf.co", "cas-bridge.xethub.hf.co", "CDN-LFS.HF.CO"] {
            assert!(redirect_host_allowed(ok), "{ok}");
        }
        for bad in ["evil.example", "hf.co.evil.example", "nothf.co", "huggingface.co.evil.example", "127.0.0.1"] {
            assert!(!redirect_host_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn the_real_client_refuses_to_follow_a_redirect_to_another_host() {
        let dir = tempfile::tempdir().unwrap();
        let target = serve(Mode::Normal); // a plain-http local host, which the policy must refuse
        let url = serve(Mode::Redirect(Box::leak(target.into_boxed_str())));
        let result = download_verified(&client(), &file(&url), dir.path(), &AtomicBool::new(false), |_| {});
        assert!(matches!(result, Err(DownloadError::Http(_))), "{result:?}");
        assert!(!dir.path().join("m.gguf").exists());
    }
}
