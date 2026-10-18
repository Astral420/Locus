//! HTTP access for the catalog, behind a trait so the service is testable without a network.

use super::CatalogError;
use std::{io::Read, time::Duration};

/// Largest catalog file accepted. model_catalog.json is about 6 MB today.
pub const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Fetched {
    NotModified,
    Body { bytes: Vec<u8>, etag: Option<String> },
}

pub trait Transport: Send + Sync {
    /// Conditional GET. `etag` is the validator saved from the previous download.
    fn get(&self, url: &str, etag: Option<&str>) -> Result<Fetched, CatalogError>;
}

pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("locus/", env!("CARGO_PKG_VERSION"), " model-catalog"))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client }
    }
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for ReqwestTransport {
    fn get(&self, url: &str, etag: Option<&str>) -> Result<Fetched, CatalogError> {
        let mut request = self.client.get(url);
        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let response = request.send().map_err(|e| CatalogError::Http(e.to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(Fetched::NotModified);
        }
        if !status.is_success() {
            return Err(CatalogError::Http(format!("{status} for {url}")));
        }
        if response.content_length().is_some_and(|n| n > MAX_CATALOG_BYTES) {
            return Err(CatalogError::TooLarge);
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = Vec::new();
        response
            .take(MAX_CATALOG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| CatalogError::Http(e.to_string()))?;
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(CatalogError::TooLarge);
        }
        Ok(Fetched::Body { bytes, etag })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    /// One-shot HTTP server that records the request headers it saw.
    fn serve(response: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file.json", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut seen = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                seen.push_str(&line);
            }
            stream.write_all(response.as_bytes()).unwrap();
            seen
        });
        (url, handle)
    }

    #[test]
    fn returns_body_and_etag() {
        let (url, server) = serve("HTTP/1.1 200 OK\r\nETag: \"abc\"\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]");
        let fetched = ReqwestTransport::new().get(&url, None).unwrap();
        assert_eq!(fetched, Fetched::Body { bytes: b"[]".to_vec(), etag: Some("\"abc\"".into()) });
        assert!(!server.join().unwrap().to_ascii_lowercase().contains("if-none-match"));
    }

    #[test]
    fn sends_the_saved_etag_and_understands_304() {
        let (url, server) = serve("HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n");
        let fetched = ReqwestTransport::new().get(&url, Some("\"abc\"")).unwrap();
        assert_eq!(fetched, Fetched::NotModified);
        assert!(server.join().unwrap().to_ascii_lowercase().contains("if-none-match: \"abc\""));
    }

    #[test]
    fn http_errors_are_errors_not_bodies() {
        let (url, _server) = serve("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(matches!(ReqwestTransport::new().get(&url, None), Err(CatalogError::Http(_))));
    }

    #[test]
    fn oversized_bodies_are_refused() {
        let (url, _server) = serve("HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\nConnection: close\r\n\r\n");
        assert!(matches!(ReqwestTransport::new().get(&url, None), Err(CatalogError::TooLarge)));
    }
}
