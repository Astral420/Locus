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
}

const EMBEDDING_GPU_ENV: &str = "LOCUS_LLAMA_EMBEDDING_GPU";
const DEFAULT_GENERATION_CONTEXT: u32 = 8192;
const METAL_AMD_GENERATION_TIMEOUT: Duration = Duration::from_secs(120);

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
        let startup_timeout = config.effective_startup_timeout();
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
            client: Client::new(),
            base_url: format!("http://{}:{port}", config.host),
            auth_token,
            model: config.model_path,
        };
        let deadline = Instant::now() + startup_timeout;
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
        assert_golden(&config_with(BackendKind::Metal, false), &generation, &[]);
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
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-llama-server.py");
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
