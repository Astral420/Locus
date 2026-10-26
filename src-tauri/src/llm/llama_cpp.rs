use super::{
    backend::LlamaBackend, parse_json, ChatCompletionRequest, ChatCompletionResponse,
    CompletionProvider, LlmError,
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
    /// The backend chosen by the llama-engine detection. `None` keeps the
    /// historical flag and environment behavior exactly as it was.
    pub llama_backend: Option<LlamaBackend>,
    /// Generation context size; ignored by backends that do not take `-c`.
    pub context_size: Option<u32>,
    /// When set, the child's stdout and stderr are appended to this file and the
    /// last lines are quoted in startup errors. When `None` the output is
    /// discarded, which is only right for tests.
    pub log_path: Option<PathBuf>,
}

const EMBEDDING_GPU_ENV: &str = "LOCUS_LLAMA_EMBEDDING_GPU";
/// Default generation window. Summarization sends at most
/// `MAX_SINGLE_CONTEXT_CHARS` (see `llm::summarization`) so the prompt and the
/// reserved completion must fit here together.
pub const DEFAULT_GENERATION_CONTEXT: u32 = 8192;
const METAL_AMD_GENERATION_TIMEOUT: Duration = Duration::from_secs(120);

/// Safety net for one generation request, not a throughput limit: prompt
/// evaluation plus up to `SUMMARY_MAX_TOKENS` tokens on a quantized model can
/// take minutes, and the blocking reqwest client would otherwise abort at its
/// 30-second default. Tests shorten this through `set_generation_timeout`.
pub const GENERATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Health checks are cheap and must never inherit the generation allowance.
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// Embeddings are short; a hung embedding server must not stall indexing.
const EMBEDDING_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

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
            llama_backend: None,
            context_size: None,
            log_path: None,
        }
    }

    /// First-run shader compilation on the patched AMD Metal engine can take
    /// well over the default timeout; embeddings keep their current value.
    fn effective_startup_timeout(&self) -> Duration {
        match &self.llama_backend {
            Some(LlamaBackend::MetalAmd { .. }) if !self.embedding => METAL_AMD_GENERATION_TIMEOUT,
            _ => self.startup_timeout,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaLaunchSpec {
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    /// Inherited variables the child must not see, for example a `TOSH_*`
    /// setting from the parent process.
    pub environment_removed: Vec<String>,
    pub backend: BackendKind,
}

pub fn build_launch_spec(config: &LlamaServerConfig, port: u16) -> LlamaLaunchSpec {
    match &config.llama_backend {
        Some(LlamaBackend::MetalAmd { device_index, .. }) => {
            build_metal_amd_spec(config, port, *device_index)
        }
        Some(LlamaBackend::Cpu) => build_legacy_spec(config, port, BackendKind::Cpu),
        Some(LlamaBackend::AppleMetal) => build_legacy_spec(config, port, BackendKind::Metal),
        Some(LlamaBackend::Vulkan) => build_legacy_spec(config, port, BackendKind::Vulkan),
        None => {
            let backend = config.gpu_backend.unwrap_or_else(detect_backend);
            build_legacy_spec(config, port, backend)
        }
    }
}

fn base_args(config: &LlamaServerConfig, port: u16) -> Vec<String> {
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
    args
}

/// The pre-change behavior for an explicitly known backend, kept byte-identical
/// so that callers without a detected llama backend are unaffected.
fn build_legacy_spec(
    config: &LlamaServerConfig,
    port: u16,
    backend: BackendKind,
) -> LlamaLaunchSpec {
    let mut args = base_args(config, port);
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
    if !config.embedding {
        // Metal takes every layer; embeddings keep their existing flags.
        if backend == BackendKind::Metal {
            args.extend(["-ngl".into(), "99".into()]);
        }
        // Only generation takes `-c`, and only when the caller sized the window.
        if let Some(context_size) = config.context_size {
            args.extend(["-c".into(), context_size.to_string()]);
        }
    }
    LlamaLaunchSpec {
        args,
        environment,
        environment_removed: Vec::new(),
        backend,
    }
}

fn build_metal_amd_spec(
    config: &LlamaServerConfig,
    port: u16,
    device_index: u32,
) -> LlamaLaunchSpec {
    let mut args = base_args(config, port);
    let mut environment = Vec::new();
    if config.embedding {
        if embedding_gpu_opt_in() {
            args.extend(["-ngl".into(), "99".into(), "-fa".into(), "auto".into()]);
            environment = metal_amd_environment(device_index);
        } else {
            // CPU first: embeddings are cheap and the AMD attention kernels are
            // not qualified for them yet.
            args.extend(["-ngl".into(), "0".into(), "--device".into(), "none".into()]);
        }
    } else {
        args.extend([
            "-ngl".into(),
            "99".into(),
            "-fa".into(),
            "auto".into(),
            "-c".into(),
            config
                .context_size
                .unwrap_or(DEFAULT_GENERATION_CONTEXT)
                .to_string(),
        ]);
        environment = metal_amd_environment(device_index);
    }
    let environment_removed = inherited_engine_environment(&environment);
    LlamaLaunchSpec {
        args,
        environment,
        environment_removed,
        backend: BackendKind::Metal,
    }
}

fn metal_amd_environment(device_index: u32) -> Vec<(String, String)> {
    vec![
        ("TOSH_FA_AMD".into(), "1".into()),
        ("GGML_METAL_DEVICE_INDEX".into(), device_index.to_string()),
    ]
}

fn embedding_gpu_opt_in() -> bool {
    std::env::var(EMBEDDING_GPU_ENV)
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

/// Inherited engine variables that must not reach the child. Keys the spec
/// explicitly sets are excluded because the child receives the new value.
fn inherited_engine_environment(environment: &[(String, String)]) -> Vec<String> {
    std::env::vars_os()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .filter(|key| key.starts_with("TOSH_") || key.starts_with("GGML_METAL_"))
        .filter(|key| !environment.iter().any(|(set, _)| set == key))
        .collect()
}

pub fn is_loopback_host(host: IpAddr) -> bool {
    host.is_loopback()
}

/// Whether the server in a slot may serve the wanted model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerPlan {
    Reuse,
    Relaunch,
}

/// A running server is only reused when it was launched with the wanted model
/// path and still answers health checks; every other state relaunches it. Used
/// for both the embedding and the generation slot so the two roles behave alike.
pub fn plan_server(current: Option<(Option<&Path>, bool)>, wanted: &Path) -> ServerPlan {
    match current {
        Some((Some(path), true)) if path == wanted => ServerPlan::Reuse,
        _ => ServerPlan::Relaunch,
    }
}

/// Owns a llama-server child process. The server is always bound to loopback
/// and every launch receives a fresh bearer token.
pub struct LlamaServerManager {
    child: Mutex<Child>,
    client: Client,
    base_url: String,
    auth_token: String,
    model: Option<PathBuf>,
    /// Read-only source for the log tail quoted when the child dies.
    log_path: Option<PathBuf>,
    /// Per-request allowance for generation. The client itself has no total
    /// timeout; health and embedding requests set their own short ones.
    generation_timeout: Duration,
}

fn build_client() -> Result<Client, LlmError> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        // No client-wide request timeout: the blocking default is 30 seconds,
        // which aborts a real summary long before the model finishes.
        .timeout(None::<Duration>)
        .build()
        .map_err(|error| {
            LlmError::Configuration(format!(
                "could not create the llama-server HTTP client: {error}"
            ))
        })
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
        let startup_timeout = config.effective_startup_timeout();
        let auth_token = Uuid::new_v4().to_string();
        let launch = build_launch_spec(&config, port);
        let log_file = open_log(&config.log_path)?;
        let mut command = Command::new(&config.binary_path);
        command.args(&launch.args).arg("--api-key").arg(&auth_token);
        command.stdin(Stdio::null());
        match &log_file {
            Some(file) => {
                command.stdout(Stdio::from(file.try_clone().map_err(|error| {
                    LlmError::Configuration(format!("could not duplicate the engine log: {error}"))
                })?));
                command.stderr(Stdio::from(file.try_clone().map_err(|error| {
                    LlmError::Configuration(format!("could not duplicate the engine log: {error}"))
                })?));
            }
            None => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        for key in &launch.environment_removed {
            command.env_remove(key);
        }
        for (key, value) in &launch.environment {
            command.env(key, value);
        }
        let child = command
            .spawn()
            .map_err(|error| LlmError::Request(format!("failed to start llama-server: {error}")))?;
        let manager = Self {
            child: Mutex::new(child),
            client: build_client()?,
            base_url: format!("http://{}:{port}", config.host),
            auth_token,
            model: config.model_path,
            log_path: config.log_path.clone(),
            generation_timeout: GENERATION_REQUEST_TIMEOUT,
        };
        let deadline = Instant::now() + startup_timeout;
        loop {
            if manager.health().is_ok() {
                return Ok(manager);
            }
            if Instant::now() >= deadline {
                return Err(LlmError::Request(format!(
                    "llama-server did not become healthy before the startup deadline{}",
                    log_tail(config.log_path.as_deref())
                )));
            }
            if manager
                .child
                .lock()
                .map_err(|_| LlmError::Request("llama-server process lock poisoned".into()))?
                .try_wait()
                .map_err(|error| LlmError::Request(error.to_string()))?
                .is_some()
            {
                return Err(LlmError::Request(format!(
                    "llama-server exited during startup{}",
                    log_tail(config.log_path.as_deref())
                )));
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
            .timeout(HEALTH_CHECK_TIMEOUT)
            .send()
            .map_err(|error| LlmError::Request(describe_request_error(&error)))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(super::response_error(response))
        }
    }

    /// Raw `/completion` prompt evaluation. Kept for callers that need to send
    /// an already-formatted prompt; chat requests go through `complete`.
    #[allow(dead_code)]
    pub fn complete_prompt(&self, prompt: &str, model: &str) -> Result<String, LlmError> {
        let response = self
            .client
            .post(format!("{}/completion", self.base_url))
            .bearer_auth(&self.auth_token)
            .timeout(self.generation_timeout)
            .json(&serde_json::json!({"prompt": prompt, "n_predict": 4096, "temperature": 0.1, "model": model}))
            .send()
            .map_err(|error| self.request_failure(&error))?;
        let parsed: CompletionResponse = parse_json(response)?;
        Ok(parsed.content)
    }

    pub fn embed(&self, input: &str, model: &str) -> Result<Vec<f32>, LlmError> {
        let response = self
            .client
            .post(format!("{}/embedding", self.base_url))
            .bearer_auth(&self.auth_token)
            .timeout(EMBEDDING_REQUEST_TIMEOUT)
            .json(&serde_json::json!({"content": input, "model": model}))
            .send()
            .map_err(|error| LlmError::Request(describe_request_error(&error)))?;
        let parsed: EmbeddingResponse = parse_json(response)?;
        parsed.into_embedding()
    }

    /// Reads the child's exit status, giving a just-crashed process a moment to
    /// be reaped before the error is built.
    fn wait_for_child_exit(
        &self,
        grace: Duration,
    ) -> Result<Option<std::process::ExitStatus>, LlmError> {
        let deadline = Instant::now() + grace;
        loop {
            let status = self
                .child
                .lock()
                .map_err(|_| LlmError::Request("llama-server process lock poisoned".into()))?
                .try_wait()
                .map_err(|error| LlmError::Request(error.to_string()))?;
            if status.is_some() || Instant::now() >= deadline {
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// Classifies a failed send and, when the child is gone, says so with the
    /// exit status and the engine log tail instead of only the generic reqwest
    /// "error sending request" text.
    fn request_failure(&self, error: &reqwest::Error) -> LlmError {
        let description = describe_request_error(error);
        match self.wait_for_child_exit(Duration::from_millis(300)) {
            Ok(Some(status)) => {
                let mut message = format!(
                    "the local language model stopped unexpectedly ({})",
                    describe_exit_status(status)
                );
                message.push_str(&log_tail(self.log_path.as_deref()));
                LlmError::Request(message)
            }
            Ok(None) => {
                let hint = match self.log_path.as_deref() {
                    Some(path) => format!(
                        "The model server is still running; the request may have been too slow or too large. See {}.",
                        path.display()
                    ),
                    None => "The model server is still running; the request may have been too slow or too large.".into(),
                };
                LlmError::Request(format!("{description}. {hint}"))
            }
            Err(error) => LlmError::Request(format!("{description}. {error}")),
        }
    }

    #[cfg(test)]
    fn set_generation_timeout(&mut self, timeout: Duration) {
        self.generation_timeout = timeout;
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
    /// `/v1/chat/completions` so the engine applies the model's own chat
    /// template, and so `temperature`/`max_tokens` reach the sampler instead of
    /// being replaced by fixed values.
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .bearer_auth(&self.auth_token)
            .timeout(self.generation_timeout)
            .json(&serde_json::json!({
                "model": request.model,
                "messages": request.messages,
                "temperature": request.temperature,
                "max_tokens": request.max_tokens,
                "stream": false,
            }))
            .send()
            .map_err(|error| self.request_failure(&error))?;
        let parsed: ChatCompletionsResponse = parse_json(response)?;
        parsed.into_response(request.model.clone())
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
struct ChatCompletionsResponse {
    #[serde(default)]
    choices: Vec<ChatCompletionsChoice>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ChatCompletionsUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsChoice {
    message: ChatCompletionsMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsMessage {
    /// `null` is returned by some builds when the model produces no text.
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionsUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

impl ChatCompletionsResponse {
    fn into_response(self, requested_model: String) -> Result<ChatCompletionResponse, LlmError> {
        let content = self
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| {
                LlmError::InvalidResponse("chat completion contained no message content".into())
            })?;
        Ok(ChatCompletionResponse {
            content,
            model: self.model.or(Some(requested_model)),
            prompt_tokens: self.usage.as_ref().and_then(|usage| usage.prompt_tokens),
            completion_tokens: self.usage.as_ref().and_then(|usage| usage.completion_tokens),
        })
    }
}

/// Opens the engine log in append mode, creating its directory.
fn open_log(path: &Option<PathBuf>) -> Result<Option<std::fs::File>, LlmError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            LlmError::Configuration(format!(
                "could not create the engine log directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(Some)
        .map_err(|error| {
            LlmError::Configuration(format!(
                "could not open the engine log at {}: {error}",
                path.display()
            ))
        })
}

/// Classifies a failed send and keeps the cause chain reqwest hides behind its
/// "error sending request for url (...)" message.
fn describe_request_error(error: &reqwest::Error) -> String {
    let mut causes = Vec::new();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !text.is_empty() && !causes.iter().any(|existing| existing == &text) {
            causes.push(text);
        }
        source = cause.source();
    }
    let classification = if error.is_timeout() {
        "timed out"
    } else if error.is_connect() {
        "could not connect"
    } else if error.is_body() || error.is_decode() {
        "connection closed while reading the response"
    } else {
        "connection closed"
    };
    let mut message = format!("{classification}: {error}");
    if !causes.is_empty() {
        message.push_str(" (");
        message.push_str(&causes.join(": "));
        message.push(')');
    }
    message
}

fn describe_exit_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status: {code}"),
        None => status.to_string(),
    }
}

/// The last lines of the engine log, for a startup failure the user can read.
/// The API token is passed on the command line, never on stdout/stderr.
fn log_tail(path: Option<&Path>) -> String {
    const TAIL_LINES: usize = 20;
    let Some(path) = path else {
        return String::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    let start = lines.len().saturating_sub(TAIL_LINES);
    format!(
        "\nlast {count} lines of {}:\n{}",
        path.display(),
        lines[start..].join("\n"),
        count = lines.len() - start
    )
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

    fn config_with(backend: BackendKind, embedding: bool) -> LlamaServerConfig {
        let mut config = LlamaServerConfig::new("/tmp/llama-server");
        config.gpu_backend = Some(backend);
        config.model_path = Some("/tmp/model.gguf".into());
        config.embedding = embedding;
        config
    }

    fn metal_amd_config(embedding: bool) -> LlamaServerConfig {
        let mut config = config_with(BackendKind::Metal, embedding);
        config.llama_backend = Some(LlamaBackend::MetalAmd {
            device_index: 1,
            device_name: "AMD Radeon Pro".into(),
        });
        config
    }

    fn env_value<'a>(spec: &'a LlamaLaunchSpec, key: &str) -> Option<&'a str> {
        spec.environment
            .iter()
            .find(|(set, _)| set == key)
            .map(|(_, value)| value.as_str())
    }

    fn assert_args_contain(spec: &LlamaLaunchSpec, expected: &[&str]) {
        let found = spec.args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        });
        assert!(found, "expected args {expected:?} in {:?}", spec.args);
    }

    fn assert_golden(config: &LlamaServerConfig, args: &[&str], environment: &[(&str, &str)]) {
        let spec = build_launch_spec(config, 1234);
        assert_eq!(
            spec.args.iter().map(String::as_str).collect::<Vec<_>>(),
            args
        );
        assert_eq!(
            spec.environment
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>(),
            environment
        );
        assert!(spec.environment_removed.is_empty());
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    #[test]
    fn metal_amd_generation_sets_flash_attn_env_device_index_and_context() {
        let mut config = metal_amd_config(false);
        config.context_size = Some(4096);
        let spec = build_launch_spec(&config, 1234);
        assert_eq!(spec.backend, BackendKind::Metal);
        assert_args_contain(&spec, &["-ngl", "99", "-fa", "auto", "-c", "4096"]);
        assert_eq!(env_value(&spec, "TOSH_FA_AMD"), Some("1"));
        assert_eq!(env_value(&spec, "GGML_METAL_DEVICE_INDEX"), Some("1"));
        assert!(!spec.args.iter().any(|arg| arg == "--embedding"));

        let default_context = build_launch_spec(&metal_amd_config(false), 1234);
        assert_args_contain(&default_context, &["-c", "8192"]);
    }

    #[test]
    fn metal_amd_embedding_runs_on_cpu_by_default() {
        let _guard = env_lock();
        std::env::remove_var(EMBEDDING_GPU_ENV);
        let spec = build_launch_spec(&metal_amd_config(true), 1234);
        assert_args_contain(&spec, &["--embedding", "-ngl", "0", "--device", "none"]);
        assert!(spec.environment.is_empty());
        assert!(!spec.args.iter().any(|arg| arg == "-fa"));
    }

    #[test]
    fn metal_amd_embedding_gpu_opt_in_uses_the_gpu_flags() {
        let _guard = env_lock();
        std::env::set_var(EMBEDDING_GPU_ENV, "1");
        let spec = build_launch_spec(&metal_amd_config(true), 1234);
        std::env::remove_var(EMBEDDING_GPU_ENV);
        assert_args_contain(&spec, &["--embedding", "-ngl", "99", "-fa", "auto"]);
        assert!(!spec.args.iter().any(|arg| arg == "--device"));
        assert!(!spec.args.iter().any(|arg| arg == "-c"));
        assert_eq!(env_value(&spec, "TOSH_FA_AMD"), Some("1"));
        assert_eq!(env_value(&spec, "GGML_METAL_DEVICE_INDEX"), Some("1"));
    }

    #[test]
    fn inherited_tosh_and_ggml_metal_env_is_not_leaked_to_the_child() {
        let _guard = env_lock();
        std::env::set_var("TOSH_LEGACY_FLAG", "9");
        std::env::set_var("TOSH_FA_AMD", "9");
        std::env::set_var("GGML_METAL_LEGACY", "7");
        std::env::set_var("GGML_METAL_DEVICE_INDEX", "7");
        std::env::set_var("LOCUS_TEST_UNRELATED", "keep");
        let spec = build_launch_spec(&metal_amd_config(false), 1234);
        std::env::remove_var("TOSH_LEGACY_FLAG");
        std::env::remove_var("TOSH_FA_AMD");
        std::env::remove_var("GGML_METAL_LEGACY");
        std::env::remove_var("GGML_METAL_DEVICE_INDEX");
        std::env::remove_var("LOCUS_TEST_UNRELATED");

        assert!(spec
            .environment_removed
            .iter()
            .any(|key| key == "TOSH_LEGACY_FLAG"));
        assert!(spec
            .environment_removed
            .iter()
            .any(|key| key == "GGML_METAL_LEGACY"));
        assert!(!spec
            .environment_removed
            .iter()
            .any(|key| key == "LOCUS_TEST_UNRELATED"));
        // Overridden keys are set explicitly, so the inherited value never wins.
        assert_eq!(env_value(&spec, "TOSH_FA_AMD"), Some("1"));
        assert_eq!(env_value(&spec, "GGML_METAL_DEVICE_INDEX"), Some("1"));
    }

    #[test]
    fn none_backend_output_is_unchanged_from_the_pre_change_golden() {
        // Golden strings recorded before this change by running a temporary
        // `capture_pre_change_golden` test against the unmodified
        // `build_launch_spec` and copying its printed args/env.
        let generation = [
            "--host",
            "127.0.0.1",
            "--port",
            "1234",
            "--model",
            "/tmp/model.gguf",
        ];
        // Apple Metal generation is the one deliberate change to these golden
        // strings: it now offloads every layer, which the golden recorded before
        // this task did not.
        let metal_generation = [
            "--host",
            "127.0.0.1",
            "--port",
            "1234",
            "--model",
            "/tmp/model.gguf",
            "-ngl",
            "99",
        ];
        let embedding = [
            "--host",
            "127.0.0.1",
            "--port",
            "1234",
            "--model",
            "/tmp/model.gguf",
            "--embedding",
        ];
        let vulkan_generation = [
            "--host",
            "127.0.0.1",
            "--port",
            "1234",
            "--model",
            "/tmp/model.gguf",
            "--n-gpu-layers",
            "-1",
            "--flash-attn",
            "0",
        ];
        let vulkan_embedding = [
            "--host",
            "127.0.0.1",
            "--port",
            "1234",
            "--model",
            "/tmp/model.gguf",
            "--embedding",
            "--n-gpu-layers",
            "-1",
            "--flash-attn",
            "0",
        ];
        let vulkan_environment = [
            ("GGML_VK_DISABLE_F16", "1"),
            ("GGML_VK_VISIBLE_DEVICES", "0"),
        ];

        assert_golden(&config_with(BackendKind::Cpu, false), &generation, &[]);
        assert_golden(&config_with(BackendKind::Cpu, true), &embedding, &[]);
        assert_golden(
            &config_with(BackendKind::Metal, false),
            &metal_generation,
            &[],
        );
        assert_golden(&config_with(BackendKind::Metal, true), &embedding, &[]);
        assert_golden(
            &config_with(BackendKind::Vulkan, false),
            &vulkan_generation,
            &vulkan_environment,
        );
        assert_golden(
            &config_with(BackendKind::Vulkan, true),
            &vulkan_embedding,
            &vulkan_environment,
        );
    }

    #[test]
    fn vulkan_and_apple_metal_specs_are_unchanged() {
        for embedding in [false, true] {
            let legacy_vulkan = config_with(BackendKind::Vulkan, embedding);
            let mut explicit_vulkan = config_with(BackendKind::Vulkan, embedding);
            explicit_vulkan.llama_backend = Some(LlamaBackend::Vulkan);
            assert_eq!(
                build_launch_spec(&legacy_vulkan, 1234),
                build_launch_spec(&explicit_vulkan, 1234)
            );

            let mut mismatched_gpu = config_with(BackendKind::Cpu, embedding);
            mismatched_gpu.llama_backend = Some(LlamaBackend::Vulkan);
            assert_eq!(
                build_launch_spec(&legacy_vulkan, 1234),
                build_launch_spec(&mismatched_gpu, 1234)
            );

            let legacy_metal = config_with(BackendKind::Metal, embedding);
            let mut explicit_metal = config_with(BackendKind::Metal, embedding);
            explicit_metal.llama_backend = Some(LlamaBackend::AppleMetal);
            assert_eq!(
                build_launch_spec(&legacy_metal, 1234),
                build_launch_spec(&explicit_metal, 1234)
            );
        }
    }

    #[test]
    fn generation_timeout_is_120s_for_metal_amd_only() {
        let mut generation = metal_amd_config(false);
        generation.startup_timeout = Duration::from_secs(10);
        assert_eq!(
            generation.effective_startup_timeout(),
            Duration::from_secs(120)
        );

        let mut embedding = metal_amd_config(true);
        embedding.startup_timeout = Duration::from_secs(10);
        assert_eq!(
            embedding.effective_startup_timeout(),
            Duration::from_secs(10)
        );

        let mut cpu = config_with(BackendKind::Cpu, false);
        cpu.llama_backend = Some(LlamaBackend::Cpu);
        cpu.startup_timeout = Duration::from_secs(10);
        assert_eq!(cpu.effective_startup_timeout(), Duration::from_secs(10));

        let mut unchanged = config_with(BackendKind::Cpu, false);
        unchanged.startup_timeout = Duration::from_secs(10);
        assert_eq!(
            unchanged.effective_startup_timeout(),
            Duration::from_secs(10)
        );
    }

    #[cfg(unix)]
    const FAKE_SERVER_WITHOUT_SHUTDOWN: &str = r##"#!/usr/bin/env python3
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
index = 0
while index < len(args):
    if args[index] == "--port" and index + 1 < len(args):
        port = int(args[index + 1])
        index += 2
    else:
        index += 1

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "pid"), "w") as pid_file:
    pid_file.write(str(os.getpid()))

print("fake llama-server device: AMD Radeon RX 6600", file=sys.stderr, flush=True)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def _reply(self, status, payload):
        body = json.dumps(payload).encode("utf-8") if not isinstance(payload, bytes) else payload
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._reply(200, {"status": "ok"})
        else:
            self._reply(404, {})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0) or 0)
        raw = self.rfile.read(length) if length else b"{}"
        if self.path == "/v1/chat/completions":
            request = json.loads(raw.decode("utf-8") or "{}")
            rendered = "|".join(
                message.get("role", "") + "=" + message.get("content", "")
                for message in request.get("messages", [])
            )
            content = "answer[" + rendered + "]temp=" + str(request.get("temperature")) + ";max=" + str(request.get("max_tokens"))
            self._reply(
                200,
                {
                    "model": request.get("model", ""),
                    "choices": [{"message": {"role": "assistant", "content": content}}],
                    "usage": {"prompt_tokens": 11, "completion_tokens": 4},
                },
            )
        else:
            self._reply(404, {})


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"##;

    #[cfg(unix)]
    const FAKE_SERVER_THAT_EXITS: &str = r##"#!/usr/bin/env python3
import sys

print("error: failed to load model /tmp/model.gguf", file=sys.stderr, flush=True)
print("error: unsupported metal device index 7", file=sys.stderr, flush=True)
sys.exit(1)
"##;

    #[cfg(unix)]
    const FAKE_SERVER_THAT_DELAYS: &str = r##"#!/usr/bin/env python3
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
index = 0
while index < len(args):
    if args[index] == "--port" and index + 1 < len(args):
        port = int(args[index + 1])
        index += 2
    else:
        index += 1

print("fake llama-server device: AMD Radeon RX 6600", file=sys.stderr, flush=True)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def _reply(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self._reply(200, {"status": "ok"})
        else:
            self._reply(404, {})

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0) or 0)
        if length:
            self.rfile.read(length)
        if self.path == "/v1/chat/completions":
            time.sleep(1.0)
            self._reply(200, {"model": "local", "choices": [{"message": {"content": "late"}}]})
        else:
            self._reply(404, {})


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"##;

    #[cfg(unix)]
    const FAKE_SERVER_THAT_CRASHES_MID_REQUEST: &str = r##"#!/usr/bin/env python3
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
index = 0
while index < len(args):
    if args[index] == "--port" and index + 1 < len(args):
        port = int(args[index + 1])
        index += 2
    else:
        index += 1

print("fake llama-server device: AMD Radeon RX 6600", file=sys.stderr, flush=True)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        if self.path == "/health":
            body = b'{"status":"ok"}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0) or 0)
        if length:
            self.rfile.read(length)
        if self.path == "/v1/chat/completions":
            print("ggml_metal: error: command buffer failed", file=sys.stderr, flush=True)
            os._exit(9)
        else:
            self.send_response(404)
            self.end_headers()


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"##;

    #[cfg(unix)]
    fn python3_available() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(unix)]
    fn write_executable_script(dir: &Path, contents: &str) -> PathBuf {
        write_named_executable_script(dir, "fake-llama-server.py", contents)
    }

    #[cfg(unix)]
    fn write_named_executable_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[cfg(unix)]
    fn process_is_alive(pid: i32) -> bool {
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_kills_a_server_that_has_no_shutdown_endpoint() {
        if !python3_available() {
            eprintln!("skipping shutdown test: python3 is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_executable_script(dir.path(), FAKE_SERVER_WITHOUT_SHUTDOWN);
        let manager = LlamaServerManager::launch(LlamaServerConfig::new(binary.as_path()))
            .expect("fake server should become healthy");
        let pid: i32 = std::fs::read_to_string(dir.path().join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(process_is_alive(pid));

        let started = Instant::now();
        manager.shutdown().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "shutdown took {:?}",
            started.elapsed()
        );
        assert!(!process_is_alive(pid));
    }

    #[cfg(unix)]
    fn config_for_fake_server(binary: &Path) -> LlamaServerConfig {
        let mut config = LlamaServerConfig::new(binary);
        // The fake server only needs a model path to echo; it must exist.
        config.model_path = None;
        config
    }

    #[cfg(unix)]
    #[test]
    fn chat_completion_uses_the_chat_endpoint_and_forwards_sampling_options() {
        if !python3_available() {
            eprintln!("skipping chat completion test: python3 is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_executable_script(dir.path(), FAKE_SERVER_WITHOUT_SHUTDOWN);
        let manager = LlamaServerManager::launch(config_for_fake_server(&binary)).unwrap();

        let response = manager
            .complete(&ChatCompletionRequest {
                model: "local-model".into(),
                messages: vec![
                    crate::llm::ChatMessage::system("be terse"),
                    crate::llm::ChatMessage::user("hello"),
                ],
                temperature: 0.25,
                max_tokens: Some(256),
            })
            .unwrap();

        // The fake server renders the request it received back into the content,
        // so roles, temperature and max_tokens prove what went over the wire.
        assert_eq!(
            response.content,
            "answer[system=be terse|user=hello]temp=0.25;max=256"
        );
        assert_eq!(response.model.as_deref(), Some("local-model"));
        assert_eq!(response.prompt_tokens, Some(11));
        assert_eq!(response.completion_tokens, Some(4));
        manager.shutdown().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_engine_log_captures_the_child_output() {
        if !python3_available() {
            eprintln!("skipping engine log test: python3 is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_executable_script(dir.path(), FAKE_SERVER_WITHOUT_SHUTDOWN);
        let log_path = dir.path().join("logs/llama-generation.log");
        let mut config = config_for_fake_server(&binary);
        config.log_path = Some(log_path.clone());
        let manager = LlamaServerManager::launch(config).unwrap();

        let logged = std::fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            logged.contains("AMD Radeon RX 6600"),
            "engine log should keep the child's stderr, got: {logged:?}"
        );
        manager.shutdown().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_startup_failure_quotes_the_engine_log_and_never_the_api_token() {
        let dir = tempfile::tempdir().unwrap();
        let binary = write_named_executable_script(dir.path(), "exiting-server.py", FAKE_SERVER_THAT_EXITS);
        let log_path = dir.path().join("logs/llama-generation.log");
        let mut config = config_for_fake_server(&binary);
        config.log_path = Some(log_path.clone());
        // Do not wait the default ten seconds for a child that already exited.
        config.startup_timeout = Duration::from_secs(5);

        let error = match LlamaServerManager::launch(config) {
            Ok(_) => panic!("a server that exits immediately must not become healthy"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("exited during startup"), "{error}");
        assert!(error.contains("failed to load model"), "{error}");
        assert!(error.contains("unsupported metal device index 7"), "{error}");

        let logged = std::fs::read_to_string(&log_path).unwrap_or_default();
        // The bearer token is passed as an argument, so it must never be in the log.
        assert!(!logged.contains("--api-key"), "{logged:?}");
        assert_eq!(logged.matches("error:").count(), 2, "{logged:?}");
    }

    #[test]
    fn generation_requests_have_no_thirty_second_cap() {
        // reqwest's blocking client defaults to a 30 s total request timeout,
        // which aborted real summaries. Generation keeps a named allowance that
        // is far above it; tests shorten it per manager.
        assert_ne!(GENERATION_REQUEST_TIMEOUT, Duration::from_secs(30));
        assert!(GENERATION_REQUEST_TIMEOUT >= Duration::from_secs(30 * 60));
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_generation_is_reported_as_timed_out_with_the_log_path() {
        if !python3_available() {
            eprintln!("skipping timeout test: python3 is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_named_executable_script(
            dir.path(),
            "slow-server.py",
            FAKE_SERVER_THAT_DELAYS,
        );
        let log_path = dir.path().join("logs/llama-generation.log");
        let mut config = config_for_fake_server(&binary);
        config.log_path = Some(log_path.clone());
        let mut manager = LlamaServerManager::launch(config).unwrap();
        // The fake server answers after one second; a 150 ms allowance makes
        // the classification testable without waiting for the real ceiling.
        manager.set_generation_timeout(Duration::from_millis(150));

        let error = manager
            .complete(&ChatCompletionRequest {
                model: "local".into(),
                messages: vec![crate::llm::ChatMessage::user("hello")],
                temperature: 0.1,
                max_tokens: Some(16),
            })
            .expect_err("the delayed answer must exceed the test allowance")
            .to_string();

        assert!(error.contains("timed out"), "{error}");
        assert!(error.contains(&log_path.display().to_string()), "{error}");
        assert!(error.contains("still running"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_dies_mid_request_reports_its_exit_status_and_log_tail() {
        if !python3_available() {
            eprintln!("skipping crash test: python3 is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_named_executable_script(
            dir.path(),
            "crashing-server.py",
            FAKE_SERVER_THAT_CRASHES_MID_REQUEST,
        );
        let log_path = dir.path().join("logs/llama-generation.log");
        let mut config = config_for_fake_server(&binary);
        config.log_path = Some(log_path.clone());
        let manager = LlamaServerManager::launch(config).unwrap();

        let error = manager
            .complete(&ChatCompletionRequest {
                model: "local".into(),
                messages: vec![crate::llm::ChatMessage::user("hello")],
                temperature: 0.1,
                max_tokens: Some(16),
            })
            .expect_err("a crashed server cannot answer")
            .to_string();

        assert!(error.contains("stopped unexpectedly"), "{error}");
        assert!(error.contains("exit status: 9"), "{error}");
        assert!(error.contains(&log_path.display().to_string()), "{error}");
        assert!(error.contains("command buffer failed"), "{error}");
    }

    #[test]
    fn generation_context_size_is_passed_only_to_generation_servers() {
        let mut generation = config_with(BackendKind::Cpu, false);
        generation.context_size = Some(16384);
        assert_args_contain(&build_launch_spec(&generation, 1234), &["-c", "16384"]);

        let mut embedding = config_with(BackendKind::Cpu, true);
        embedding.context_size = Some(16384);
        assert!(!build_launch_spec(&embedding, 1234)
            .args
            .iter()
            .any(|arg| arg == "-c"));
    }

    #[test]
    fn apple_metal_generation_offloads_every_layer() {
        let mut generation = config_with(BackendKind::Metal, false);
        generation.llama_backend = Some(LlamaBackend::AppleMetal);
        assert_args_contain(&build_launch_spec(&generation, 1234), &["-ngl", "99"]);

        let mut embedding = config_with(BackendKind::Metal, true);
        embedding.llama_backend = Some(LlamaBackend::AppleMetal);
        assert!(!build_launch_spec(&embedding, 1234)
            .args
            .iter()
            .any(|arg| arg == "-ngl"));
    }

    /// Manual L2 helper: prints the real machine's launch specs for both roles.
    #[test]
    #[ignore]
    fn print_launch_spec_for_real_machine() {
        let choice = crate::llm::backend::detect_llama_backend();
        let mut config = LlamaServerConfig::new("/tmp/llama-server");
        config.model_path = Some("/tmp/model.gguf".into());
        config.llama_backend = Some(choice.backend.clone());
        let generation = build_launch_spec(&config, 8080);
        config.embedding = true;
        let embedding = build_launch_spec(&config, 8081);
        println!("backend: {:?} ({})", choice.backend, choice.reason);
        for (label, spec) in [("generation", generation), ("embedding", embedding)] {
            println!("{label} args: {:?}", spec.args);
            println!("{label} env: {:?}", spec.environment);
            println!("{label} removed: {:?}", spec.environment_removed);
        }
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
