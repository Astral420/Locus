//! The generation llama-server used by summarization and Knowledge chat.
//!
//! Embedding and generation deliberately keep separate processes: an embedding
//! model in `--embedding` mode cannot generate, and a generation server must not
//! share the index's model identity. Both slots follow the same rule — reuse a
//! healthy server launched with the wanted weights, otherwise relaunch — so a
//! switched model never answers with the previous model's weights.

use super::{
    backend::{detect_llama_backend, LlamaBackend},
    engine::resolved_engine,
    llama_cpp::{
        plan_server, LlamaServerConfig, LlamaServerManager, ServerPlan,
        DEFAULT_GENERATION_CONTEXT,
    },
    model_manager::GenerationModelManager,
    providers::{build_provider, ProviderSnapshot},
    ChatCompletionRequest, ChatCompletionResponse, CompletionProvider, LlmError, ProviderKind,
};
use crate::{keychain::Keychain, AppState};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// `<data_dir>/logs/llama-generation.log`, the file the engine's own output
/// goes to. `launch` creates the directory.
pub fn generation_log_path(state: &AppState) -> PathBuf {
    state
        .paths
        .data_dir
        .join("logs")
        .join("llama-generation.log")
}

/// Resolve the generation provider against the activated model and the shared
/// generation server slot. The slot is validated on every request: a server
/// running different weights (or a dead one) is replaced, and the previous one
/// stops as soon as its last in-flight provider is dropped.
pub fn generation_server_with(
    models: &GenerationModelManager,
    slot: &Mutex<Option<Arc<LlamaServerManager>>>,
    binary: PathBuf,
    backend: LlamaBackend,
    log_path: Option<PathBuf>,
) -> Result<Arc<LlamaServerManager>, String> {
    let asset = models.selected()?;
    if !asset.path.is_file() {
        return Err(
            "The selected local language model is missing from the models folder. Download it again in Models."
                .into(),
        );
    }
    let mut guard = slot
        .lock()
        .map_err(|_| "generation server lock is poisoned".to_string())?;
    let plan = plan_server(
        guard
            .as_ref()
            .map(|server| (server.model_path(), server.health().is_ok())),
        asset.path.as_path(),
    );
    match plan {
        ServerPlan::Reuse => match guard.as_ref() {
            Some(server) => Ok(Arc::clone(server)),
            None => Err("generation server plan was inconsistent".into()),
        },
        ServerPlan::Relaunch => {
            let mut config = LlamaServerConfig::new(binary);
            config.model_path = Some(asset.path.clone());
            config.embedding = false;
            config.context_size = Some(DEFAULT_GENERATION_CONTEXT);
            config.llama_backend = Some(backend);
            config.log_path = log_path;
            let new_server = Arc::new(LlamaServerManager::launch(config).map_err(|error| {
                format!("the local language engine is unavailable: {error}")
            })?);
            let old = guard.replace(Arc::clone(&new_server));
            drop(guard);
            drop(old);
            Ok(new_server)
        }
    }
}

/// The selected provider's `CompletionProvider` for a local model.
pub struct LocalGenerationProvider(Arc<LlamaServerManager>);

impl LocalGenerationProvider {
    pub fn new(server: Arc<LlamaServerManager>) -> Self {
        Self(server)
    }
}

impl CompletionProvider for LocalGenerationProvider {
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        self.0.complete(request)
    }
}

/// Build the provider for the explicitly selected destination. Only
/// `llama_server` is provisioned here; every remote kind keeps its existing
/// client and its existing authorization rules.
pub fn build_runtime_provider(
    state: &AppState,
    snapshot: &ProviderSnapshot,
    keychain: &Keychain,
) -> Result<Box<dyn CompletionProvider>, LlmError> {
    match snapshot.provider {
        ProviderKind::LlamaServer => {
            let server = generation_server_with(
                &state.generation_models,
                &state.generation_server,
                resolved_engine().binary.clone(),
                detect_llama_backend().backend,
                Some(generation_log_path(state)),
            )
            .map_err(LlmError::Configuration)?;
            Ok(Box::new(LocalGenerationProvider::new(server)))
        }
        _ => build_provider(snapshot, keychain),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::llm::model_manager::GenerationModelAsset;
    use crate::llm::ChatMessage;
    use std::{
        fs,
        path::Path,
        time::{Duration, Instant},
    };

    const FAKE_SERVER: &str = r##"#!/usr/bin/env python3
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
model = ""
index = 0
while index < len(args):
    if args[index] == "--port" and index + 1 < len(args):
        port = int(args[index + 1])
        index += 2
    elif args[index] == "--model" and index + 1 < len(args):
        model = args[index + 1]
        index += 2
    else:
        index += 1

base = os.path.basename(model)
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "launches.log"), "a") as log:
    log.write(model + "\n")


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
            self._reply(200, {"model": base, "choices": [{"message": {"content": "generated by " + base}}]})
        elif self.path == "/shutdown":
            self._reply(200, {"status": "ok"})
            threading.Thread(target=self.server.shutdown, daemon=True).start()
        else:
            self._reply(404, {})


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"##;

    const CRASHING_ONCE_SERVER: &str = r##"#!/usr/bin/env python3
import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

args = sys.argv[1:]
port = 0
model = ""
index = 0
while index < len(args):
    if args[index] == "--port" and index + 1 < len(args):
        port = int(args[index + 1])
        index += 2
    elif args[index] == "--model" and index + 1 < len(args):
        model = args[index + 1]
        index += 2
    else:
        index += 1

directory = os.path.dirname(os.path.abspath(__file__))
base = os.path.basename(model)
with open(os.path.join(directory, "launches.log"), "a") as log:
    log.write(model + "\n")
marker = os.path.join(directory, "crashed-once")


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
            if not os.path.exists(marker):
                open(marker, "w").close()
                print("ggml_metal: error: command buffer failed", file=sys.stderr, flush=True)
                os._exit(9)
            self._reply(200, {"model": base, "choices": [{"message": {"content": "generated by " + base}}]})
        elif self.path == "/shutdown":
            self._reply(200, {"status": "ok"})
            threading.Thread(target=self.server.shutdown, daemon=True).start()
        else:
            self._reply(404, {})


HTTPServer(("127.0.0.1", port), Handler).serve_forever()
"##;

    fn python3_available() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn write_fake_server(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-generation-server.py");
        fs::write(&path, FAKE_SERVER).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn write_crashing_once_server(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("crashing-generation-server.py");
        fs::write(&path, CRASHING_ONCE_SERVER).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn wait_until_unreachable(url: &str, timeout: Duration) -> bool {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(300))
            .build()
            .unwrap();
        let deadline = Instant::now() + timeout;
        loop {
            if client.get(format!("{url}/health")).send().is_err() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        binary: PathBuf,
        database: Database,
        manager: GenerationModelManager,
        slot: Mutex<Option<Arc<LlamaServerManager>>>,
    }

    fn fixture() -> Option<Fixture> {
        if !python3_available() {
            eprintln!("skipping generation server test: python3 is not available");
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let binary = write_fake_server(dir.path());
        let database = Database::in_memory().unwrap();
        let manager = GenerationModelManager::new(dir.path().join("models"))
            .with_database(database.clone());
        Some(Fixture {
            dir,
            binary,
            database,
            manager,
            slot: Mutex::new(None),
        })
    }

    impl Fixture {
        fn register(&self, filename: &str) -> GenerationModelAsset {
            let path = self.dir.path().join("models").join(filename);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("weights:{filename}")).unwrap();
            self.manager
                .register_verified(path, filename, "r1", "verified")
                .unwrap()
        }

        fn activate(&self, asset: &GenerationModelAsset) {
            let value = serde_json::to_string(&asset.id).unwrap();
            self.database
                .run(move |connection| {
                    connection
                        .execute(
                            "INSERT INTO settings(key, value_json, updated_at) VALUES ('llm.generation_selection', ?1, datetime('now')) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",
                            [&value],
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })
                .unwrap();
        }

        fn provider(&self) -> Result<LocalGenerationProvider, String> {
            generation_server_with(
                &self.manager,
                &self.slot,
                self.binary.clone(),
                LlamaBackend::Cpu,
                None,
            )
            .map(LocalGenerationProvider::new)
        }

        fn launches(&self) -> usize {
            fs::read_to_string(self.dir.path().join("launches.log"))
                .map(|contents| contents.lines().count())
                .unwrap_or(0)
        }

        fn running_model_path(&self) -> Option<PathBuf> {
            self.slot
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|server| server.model_path().map(Path::to_path_buf))
        }

        fn running_base_url(&self) -> String {
            self.slot
                .lock()
                .unwrap()
                .as_ref()
                .expect("a server should be running")
                .base_url()
                .to_string()
        }
    }

    fn ask(provider: &LocalGenerationProvider) -> String {
        provider
            .complete(&ChatCompletionRequest {
                model: "local".into(),
                messages: vec![ChatMessage::user("hello")],
                temperature: 0.1,
                max_tokens: Some(64),
            })
            .unwrap()
            .content
    }

    #[test]
    fn the_same_selection_reuses_the_running_server() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf");
        fixture.activate(&a);
        assert_eq!(ask(&fixture.provider().unwrap()), "generated by alpha.gguf");
        assert_eq!(ask(&fixture.provider().unwrap()), "generated by alpha.gguf");
        assert_eq!(fixture.launches(), 1);
    }

    #[test]
    fn switching_the_selected_model_relaunches_with_the_new_weights() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf");
        let b = fixture.register("beta.gguf");
        fixture.activate(&a);
        assert_eq!(ask(&fixture.provider().unwrap()), "generated by alpha.gguf");
        fixture.activate(&b);
        assert_eq!(ask(&fixture.provider().unwrap()), "generated by beta.gguf");
        assert_eq!(fixture.running_model_path(), Some(b.path.clone()));
        assert_eq!(fixture.launches(), 2);
    }

    #[test]
    fn no_selected_model_is_a_clear_error_before_anything_launches() {
        let Some(fixture) = fixture() else {
            return;
        };
        let error = match fixture.provider() {
            Ok(_) => panic!("a provider must not be built without a model"),
            Err(error) => error,
        };
        assert!(error.contains("No local language model is selected"), "{error}");
        assert_eq!(fixture.launches(), 0);
    }

    #[test]
    fn the_old_server_stops_after_its_last_provider_is_dropped() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf");
        let b = fixture.register("beta.gguf");
        fixture.activate(&a);
        let provider_a = fixture.provider().unwrap();
        let old_url = fixture.running_base_url();
        fixture.activate(&b);
        assert_eq!(ask(&fixture.provider().unwrap()), "generated by beta.gguf");
        drop(provider_a);
        assert!(
            wait_until_unreachable(&old_url, Duration::from_secs(5)),
            "the old generation server should stop once its last provider is dropped"
        );
    }

    #[test]
    fn a_process_that_died_mid_request_is_relaunched_on_the_next_request() {
        let Some(fixture) = fixture() else {
            return;
        };
        let a = fixture.register("alpha.gguf");
        fixture.activate(&a);
        let binary = write_crashing_once_server(fixture.dir.path());
        let log_path = fixture.dir.path().join("logs/llama-generation.log");
        let request = ChatCompletionRequest {
            model: "local".into(),
            messages: vec![ChatMessage::user("hello")],
            temperature: 0.1,
            max_tokens: Some(64),
        };

        let provider = generation_server_with(
            &fixture.manager,
            &fixture.slot,
            binary.clone(),
            LlamaBackend::Cpu,
            Some(log_path.clone()),
        )
        .unwrap();
        let error = provider
            .complete(&request)
            .expect_err("the first request must see the crash")
            .to_string();
        assert!(error.contains("stopped unexpectedly"), "{error}");
        assert!(error.contains("exit status: 9"), "{error}");
        assert!(error.contains(&log_path.display().to_string()), "{error}");
        assert!(error.contains("command buffer failed"), "{error}");
        assert_eq!(fixture.launches(), 1);

        // The slot only reuses servers whose health check passes, so the dead
        // process forces a relaunch rather than answering from a closed port.
        let restarted = generation_server_with(
            &fixture.manager,
            &fixture.slot,
            binary,
            LlamaBackend::Cpu,
            Some(log_path),
        )
        .unwrap();
        assert_eq!(
            restarted.complete(&request).unwrap().content,
            "generated by alpha.gguf"
        );
        assert_eq!(fixture.launches(), 2);
    }
}

