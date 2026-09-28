use super::{
    parse_json, ChatCompletionRequest, ChatCompletionResponse, CompletionProvider, LlmError,
};
use crate::transcription::{detect_backend, vulkan_environment, BackendKind};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LlamaServerConfig {
    pub binary_path: PathBuf,
    pub model_path: Option<PathBuf>,
    pub host: IpAddr,
    pub port: Option<u16>,
    pub startup_timeout: Duration,
    pub embedding: bool,
    /// Explicit for packaged variants; `None` performs the same validated
    /// runtime selection as Whisper and falls back to CPU.
    pub gpu_backend: Option<BackendKind>,
}

impl LlamaServerConfig {
    pub fn new(binary_path: impl Into<PathBuf>) -> Self {
        Self {
            binary_path: binary_path.into(),
            model_path: None,
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: None,
            startup_timeout: Duration::from_secs(10),
            embedding: false,
            gpu_backend: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaLaunchSpec {
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub backend: BackendKind,
}

pub fn build_launch_spec(config: &LlamaServerConfig, port: u16) -> LlamaLaunchSpec {
    let backend = config.gpu_backend.unwrap_or_else(detect_backend);
    let mut args = vec![
        "--host".into(),
        config.host.to_string(),
        "--port".into(),
        port.to_string(),
    ];
    if let Some(model) = &config.model_path {
        args.extend(["--model".into(), model.display().to_string()]);
    }
    if config.embedding {
        args.push("--embedding".into());
    }
    let mut environment = Vec::new();
    if backend == BackendKind::Vulkan {
        // MoltenVK on AMD dGPUs must use one device, avoid F16 driver paths,
        // and disable flash attention to prevent corrupt completions.
        args.extend([
            "--n-gpu-layers".into(),
            "-1".into(),
            "--flash-attn".into(),
            "0".into(),
        ]);
        environment.extend(vulkan_environment().map(|(key, value)| (key.into(), value)));
    }
    LlamaLaunchSpec {
        args,
        environment,
        backend,
    }
}

pub fn is_loopback_host(host: IpAddr) -> bool {
    host.is_loopback()
}

/// Owns a llama-server child process. The server is always bound to loopback
/// and every launch receives a fresh bearer token.
pub struct LlamaServerManager {
    child: Mutex<Child>,
    client: Client,
    base_url: String,
    auth_token: String,
    model: Option<PathBuf>,
}

impl LlamaServerManager {
    pub fn launch(config: LlamaServerConfig) -> Result<Self, LlmError> {
        if !is_loopback_host(config.host) {
            return Err(LlmError::Configuration(
                "llama-server may only bind to loopback".into(),
            ));
        }
        if !config.binary_path.exists() {
            return Err(LlmError::Configuration(format!(
                "llama-server binary does not exist: {}",
                config.binary_path.display()
            )));
        }
        if let Some(model) = &config.model_path {
            if !model.exists() {
                return Err(LlmError::Configuration(format!(
                    "llama-server model does not exist: {}",
                    model.display()
                )));
            }
        }

        let port = config.port.unwrap_or_else(find_free_port);
        let auth_token = Uuid::new_v4().to_string();
        let launch = build_launch_spec(&config, port);
        let mut command = Command::new(&config.binary_path);
        command
            .args(&launch.args)
            .arg("--api-key")
            .arg(&auth_token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (key, value) in &launch.environment {
            command.env(key, value);
        }
        let child = command
            .spawn()
            .map_err(|error| LlmError::Request(format!("failed to start llama-server: {error}")))?;
        let manager = Self {
            child: Mutex::new(child),
            client: Client::new(),
            base_url: format!("http://{}:{port}", config.host),
            auth_token,
            model: config.model_path,
        };
        let deadline = Instant::now() + config.startup_timeout;
        loop {
            if manager.health().is_ok() {
                return Ok(manager);
            }
            if Instant::now() >= deadline {
                return Err(LlmError::Request(
                    "llama-server did not become healthy before the startup deadline".into(),
                ));
            }
            if manager
                .child
                .lock()
                .map_err(|_| LlmError::Request("llama-server process lock poisoned".into()))?
                .try_wait()
                .map_err(|error| LlmError::Request(error.to_string()))?
                .is_some()
            {
                return Err(LlmError::Request(
                    "llama-server exited during startup".into(),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    pub fn model_path(&self) -> Option<&Path> {
        self.model.as_deref()
    }

    pub fn health(&self) -> Result<(), LlmError> {
        let response = self
            .client
            .get(format!("{}/health", self.base_url))
            .bearer_auth(&self.auth_token)
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(super::response_error(response))
        }
    }

    pub fn complete_prompt(&self, prompt: &str, model: &str) -> Result<String, LlmError> {
        let response = self
            .client
            .post(format!("{}/completion", self.base_url))
            .bearer_auth(&self.auth_token)
            .json(&serde_json::json!({"prompt": prompt, "n_predict": 4096, "temperature": 0.1, "model": model}))
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        let parsed: CompletionResponse = parse_json(response)?;
        Ok(parsed.content)
    }

    pub fn embed(&self, input: &str, model: &str) -> Result<Vec<f32>, LlmError> {
        let response = self
            .client
            .post(format!("{}/embedding", self.base_url))
            .bearer_auth(&self.auth_token)
            .json(&serde_json::json!({"content": input, "model": model}))
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        let parsed: EmbeddingResponse = parse_json(response)?;
        parsed.into_embedding()
    }

    pub fn shutdown(&self) -> Result<(), LlmError> {
        // llama-server versions that expose the endpoint can exit cleanly;
        // the bounded wait plus kill fallback guarantees the owned child is
        // reaped even when an older binary does not implement it.
        let _ = self
            .client
            .post(format!("{}/shutdown", self.base_url))
            .bearer_auth(&self.auth_token)
            .timeout(Duration::from_secs(1))
            .send();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let exited = self
                .child
                .lock()
                .map_err(|_| LlmError::Request("llama-server process lock poisoned".into()))?
                .try_wait()
                .map_err(|error| LlmError::Request(error.to_string()))?
                .is_some();
            if exited {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let mut child = self
            .child
            .lock()
            .map_err(|_| LlmError::Request("llama-server process lock poisoned".into()))?;
        if child
            .try_wait()
            .map_err(|error| LlmError::Request(error.to_string()))?
            .is_none()
        {
            child
                .kill()
                .map_err(|error| LlmError::Request(error.to_string()))?;
        }
        child
            .wait()
            .map(|_| ())
            .map_err(|error| LlmError::Request(error.to_string()))
    }
}

impl CompletionProvider for LlamaServerManager {
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let prompt = request
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(ChatCompletionResponse {
            content: self.complete_prompt(&prompt, &request.model)?,
            model: Some(request.model.clone()),
            prompt_tokens: None,
            completion_tokens: None,
        })
    }
}

impl Drop for LlamaServerManager {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn find_free_port() -> u16 {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .unwrap_or(11435)
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    #[serde(alias = "content", alias = "response")]
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EmbeddingResponse {
    Single { embedding: Vec<f32> },
    Batch { data: Vec<EmbeddingData> },
}

impl EmbeddingResponse {
    fn into_embedding(self) -> Result<Vec<f32>, LlmError> {
        match self {
            Self::Single { embedding } if !embedding.is_empty() => Ok(embedding),
            Self::Batch { mut data } => data
                .pop()
                .map(|entry| entry.embedding)
                .filter(|embedding| !embedding.is_empty())
                .ok_or_else(|| LlmError::InvalidResponse("embedding is empty".into())),
            _ => Err(LlmError::InvalidResponse("embedding is empty".into())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_configuration_rejects_non_loopback() {
        let mut config = LlamaServerConfig::new("/tmp/missing-llama-server");
        config.host = "192.0.2.10".parse().unwrap();
        assert!(matches!(
            LlamaServerManager::launch(config),
            Err(LlmError::Configuration(message)) if message.contains("loopback")
        ));
    }

    #[test]
    fn vulkan_launch_disables_flash_attention_and_f16() {
        let mut config = LlamaServerConfig::new("/tmp/llama-server");
        config.gpu_backend = Some(BackendKind::Vulkan);
        let spec = build_launch_spec(&config, 1234);
        assert_eq!(spec.backend, BackendKind::Vulkan);
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair[0] == "--flash-attn" && pair[1] == "0"));
        assert!(spec
            .environment
            .iter()
            .any(|(key, value)| key == "GGML_VK_DISABLE_F16" && value == "1"));
        assert!(spec
            .environment
            .iter()
            .any(|(key, _value)| key == "GGML_VK_VISIBLE_DEVICES"));
    }
}
