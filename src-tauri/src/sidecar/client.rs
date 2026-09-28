use super::protocol::{
    decode_frame, encode_request, parse_result, DecodedFrame, ProtocolError, RequestTracker,
    RpcNotification, DEFAULT_MAX_FRAME_SIZE,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SidecarError {
    #[error("sidecar is not running")]
    NotRunning,
    #[error("sidecar process failed: {0}")]
    Process(String),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Handshake {
    pub protocol_version: String,
    pub sidecar_version: String,
    pub capabilities: Vec<String>,
}

pub struct SidecarClient {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Result<Vec<u8>, String>>,
    tracker: RequestTracker,
    max_frame_size: usize,
    timeout: Duration,
    pub notifications: Vec<RpcNotification>,
}

impl SidecarClient {
    pub fn spawn(
        python: &str,
        working_directory: impl AsRef<std::path::Path>,
    ) -> Result<Self, SidecarError> {
        let mut child = Command::new(python)
            .args(["-m", "sidecar.src.main"])
            .current_dir(working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| SidecarError::Process(e.to_string()))?;
        let stdin = child.stdin.take().ok_or(SidecarError::NotRunning)?;
        let stdout = child.stdout.take().ok_or(SidecarError::NotRunning)?;
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("locus-sidecar-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = Vec::new();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            if tx.send(Ok(line)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(Err(error.to_string()));
                            break;
                        }
                    }
                }
            })
            .map_err(|e| SidecarError::Process(e.to_string()))?;
        Ok(Self {
            child,
            stdin,
            frames: rx,
            tracker: RequestTracker::default(),
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            timeout: Duration::from_secs(10),
            notifications: Vec::new(),
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn handshake(&mut self) -> Result<Handshake, SidecarError> {
        self.call("handshake", serde_json::json!({}))
    }
    pub fn health(&mut self) -> Result<Value, SidecarError> {
        self.call("health", serde_json::json!({}))
    }

    pub fn call<T: DeserializeOwned, P: Serialize>(
        &mut self,
        method: &str,
        params: P,
    ) -> Result<T, SidecarError> {
        if self
            .child
            .try_wait()
            .map_err(|e| SidecarError::Process(e.to_string()))?
            .is_some()
        {
            return Err(SidecarError::NotRunning);
        }
        let id = self.tracker.new_id();
        self.stdin
            .write_all(&encode_request(
                method,
                params,
                id.clone(),
                self.max_frame_size,
            )?)
            .and_then(|_| self.stdin.flush())
            .map_err(|e| SidecarError::Process(e.to_string()))?;
        loop {
            let frame = self
                .frames
                .recv_timeout(self.timeout)
                .map_err(|error| {
                    if error == mpsc::RecvTimeoutError::Timeout {
                        SidecarError::Protocol(ProtocolError::Timeout(self.timeout))
                    } else {
                        SidecarError::NotRunning
                    }
                })?
                .map_err(SidecarError::Process)?;
            match decode_frame(&frame, self.max_frame_size)? {
                DecodedFrame::Notification(notification) => self.notifications.push(notification),
                DecodedFrame::Response(response) => {
                    if response.id.as_ref() != Some(&id) {
                        continue;
                    }
                    self.tracker.finish(&id)?;
                    return Ok(parse_result(response)?);
                }
            }
        }
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for SidecarClient {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Owns the sidecar process and exposes lazy start/restart semantics to the pipeline.
pub struct SidecarManager {
    python: String,
    working_directory: std::path::PathBuf,
    client: Option<SidecarClient>,
}

impl SidecarManager {
    pub fn new(
        python: impl Into<String>,
        working_directory: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            python: python.into(),
            working_directory: working_directory.into(),
            client: None,
        }
    }

    pub fn ensure_started(&mut self) -> Result<&mut SidecarClient, SidecarError> {
        if self.client.is_none() {
            self.client = Some(SidecarClient::spawn(&self.python, &self.working_directory)?);
        }
        let dead = self
            .client
            .as_mut()
            .ok_or(SidecarError::NotRunning)?
            .child
            .try_wait()
            .map_err(|e| SidecarError::Process(e.to_string()))?
            .is_some();
        if dead {
            self.shutdown();
            self.client = Some(SidecarClient::spawn(&self.python, &self.working_directory)?);
        }
        self.client.as_mut().ok_or(SidecarError::NotRunning)
    }

    pub fn health(&mut self) -> Result<Value, SidecarError> {
        self.ensure_started()?.health()
    }
    pub fn restart(&mut self) -> Result<Handshake, SidecarError> {
        self.shutdown();
        let client = self.ensure_started()?;
        client.handshake()
    }
    pub fn shutdown(&mut self) {
        if let Some(mut client) = self.client.take() {
            client.stop();
        }
    }
}
impl Drop for SidecarManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starts_handshakes_and_checks_health() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let Ok(mut client) = SidecarClient::spawn("python3", root) else {
            return;
        };
        let handshake = client.handshake().unwrap();
        assert_eq!(handshake.protocol_version, "2.0");
        let health: Value = client.health().unwrap();
        assert_eq!(health["ok"], true);
    }
}
