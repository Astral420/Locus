use super::{
    anthropic::AnthropicClient, gemini::GeminiClient, ollama::OllamaClient, openai::OpenAiClient,
    CompletionProvider, LlmError, ProviderKind,
};
use crate::{db::Database, keychain::Keychain};
use chrono::Utc;
use serde::{Deserialize, Serialize};

const SELECTION_KEY: &str = "llm.provider_selection";
const CONFIG_PREFIX: &str = "llm.provider.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSnapshot {
    pub provider: ProviderKind,
    pub model: String,
    pub destination: String,
    pub selected_at: String,
    pub disclosure: String,
}

impl ProviderSnapshot {
    pub fn disclosure_for(provider: ProviderKind, model: &str, destination: &str) -> String {
        format!(
            "This request will send meeting text to {} at {} using model {}. \
             The provider is used only because you explicitly selected it.",
            provider.display_name(),
            destination,
            model
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub destination: String,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactedProviderConfig {
    pub provider: ProviderKind,
    pub display_name: String,
    pub model: String,
    pub destination: String,
    pub configured: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProviderConfig {
    model: String,
    destination: String,
    configured: bool,
}

/// Persists availability and the explicit selected-provider snapshot. It never
/// stores API keys or treats a configured provider as selected.
#[derive(Clone)]
pub struct ProviderSelectionService {
    database: Database,
}

impl ProviderSelectionService {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn configure(
        &self,
        provider: ProviderKind,
        model: &str,
        destination: &str,
        configured: bool,
    ) -> Result<(), String> {
        if model.trim().is_empty() || destination.trim().is_empty() {
            return Err("provider model and destination are required".into());
        }
        let key = format!("{CONFIG_PREFIX}{}", provider.as_str());
        let value = serde_json::to_string(&StoredProviderConfig {
            model: model.trim().into(),
            destination: destination.trim().into(),
            configured,
        })
        .map_err(|error| error.to_string())?;
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                        (&key, &value, Utc::now().to_rfc3339()),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
    }

    /// Saving a key is intentionally separate from provider selection. The
    /// keychain account is stable and the secret never crosses this API.
    pub fn save_api_key(
        &self,
        keychain: &Keychain,
        provider: ProviderKind,
        api_key: &str,
    ) -> Result<(), String> {
        if api_key.trim().is_empty() {
            return Err("API key cannot be empty".into());
        }
        keychain
            .store(&format!("llm:{}", provider.as_str()), api_key.trim())
            .map_err(|error| error.to_string())
    }

    pub fn select(
        &self,
        provider: ProviderKind,
        model: &str,
        destination: &str,
        accept_disclosure: bool,
    ) -> Result<ProviderSnapshot, LlmError> {
        if model.trim().is_empty() || destination.trim().is_empty() {
            return Err(LlmError::Configuration(
                "provider model and destination are required".into(),
            ));
        }
        let disclosure = ProviderSnapshot::disclosure_for(provider, model, destination);
        if !accept_disclosure {
            return Err(LlmError::DisclosureRequired);
        }
        let snapshot = ProviderSnapshot {
            provider,
            model: model.trim().into(),
            destination: destination.trim().into(),
            selected_at: Utc::now().to_rfc3339(),
            disclosure,
        };
        let value = serde_json::to_string(&snapshot)
            .map_err(|error| LlmError::Configuration(error.to_string()))?;
        self.database
            .run(move |connection| {
                connection
                    .execute(
                        "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                        (SELECTION_KEY, &value, Utc::now().to_rfc3339()),
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| LlmError::Request(error.to_string()))?;
        Ok(snapshot)
    }

    pub fn snapshot(&self) -> Result<ProviderSnapshot, LlmError> {
        self.database
            .run(|connection| {
                connection
                    .query_row(
                        "SELECT value_json FROM settings WHERE key = ?1",
                        [SELECTION_KEY],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| LlmError::Request(error.to_string()))
            .and_then(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| LlmError::Configuration(error.to_string()))
            })
    }

    pub fn configs(&self) -> Result<Vec<RedactedProviderConfig>, String> {
        let selected = self.snapshot().ok().map(|snapshot| snapshot.provider);
        self.database
            .run(move |connection| {
                let mut statement = connection
                    .prepare("SELECT key, value_json FROM settings WHERE key LIKE 'llm.provider.%'")
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([], |row| {
                        let key: String = row.get(0)?;
                        let value: String = row.get(1)?;
                        Ok((key, value))
                    })
                    .map_err(|error| error.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                rows.into_iter()
                    .filter_map(|(key, value)| {
                        let name = key.strip_prefix(CONFIG_PREFIX)?;
                        let provider = name.parse().ok()?;
                        let config: StoredProviderConfig = serde_json::from_str(&value).ok()?;
                        Some(RedactedProviderConfig {
                            provider,
                            display_name: provider.display_name().into(),
                            model: config.model,
                            destination: config.destination,
                            configured: config.configured,
                            selected: selected == Some(provider),
                        })
                    })
                    .collect::<Vec<_>>()
                    .pipe(Ok)
            })
            .map_err(|error| error.to_string())
    }

    pub fn config(&self, provider: ProviderKind) -> Result<ProviderConfig, String> {
        let key = format!("{CONFIG_PREFIX}{}", provider.as_str());
        self.database
            .run(move |connection| {
                let value: String = connection
                    .query_row(
                        "SELECT value_json FROM settings WHERE key = ?1",
                        [&key],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let config: StoredProviderConfig =
                    serde_json::from_str(&value).map_err(|error| error.to_string())?;
                Ok(ProviderConfig {
                    provider,
                    model: config.model,
                    destination: config.destination,
                    configured: config.configured,
                })
            })
            .map_err(|error| error.to_string())
    }
}

/// Build only the explicitly selected provider. This function is intentionally
/// fallible for local llama-server: its process manager must be provisioned by
/// the application runtime with a bundled binary and selected model first.
pub fn build_provider(
    snapshot: &ProviderSnapshot,
    keychain: &Keychain,
) -> Result<Box<dyn CompletionProvider>, LlmError> {
    match snapshot.provider {
        ProviderKind::Ollama => Ok(Box::new(OllamaClient::new(
            &snapshot.destination,
            &snapshot.model,
        )?)),
        ProviderKind::OpenAi => Ok(Box::new(OpenAiClient::from_keychain_with_endpoint(
            keychain,
            openai_endpoint(&snapshot.destination),
            &snapshot.model,
        )?)),
        ProviderKind::Anthropic => Ok(Box::new(AnthropicClient::from_keychain_with_endpoint(
            keychain,
            anthropic_endpoint(&snapshot.destination),
            &snapshot.model,
        )?)),
        ProviderKind::Gemini => Ok(Box::new(GeminiClient::from_keychain_with_base_url(
            keychain,
            &snapshot.destination,
            &snapshot.model,
        )?)),
        ProviderKind::LlamaServer => Err(LlmError::Configuration(
            "local llama-server must be started by the application runtime with a generation model"
                .into(),
        )),
    }
}

fn openai_endpoint(destination: &str) -> String {
    let destination = destination.trim_end_matches('/');
    if destination.ends_with("/chat/completions") {
        destination.into()
    } else if destination.ends_with("/v1") {
        format!("{destination}/chat/completions")
    } else {
        format!("{}/v1/chat/completions", destination.trim_end_matches('/'))
    }
}

fn anthropic_endpoint(destination: &str) -> String {
    let destination = destination.trim_end_matches('/');
    if destination.ends_with("/messages") {
        destination.into()
    } else if destination.ends_with("/v1") {
        format!("{destination}/messages")
    } else {
        format!("{}/v1/messages", destination.trim_end_matches('/'))
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuring_does_not_select_until_disclosure_is_accepted() {
        let db = Database::in_memory().unwrap();
        let service = ProviderSelectionService::new(db);
        service
            .configure(
                ProviderKind::OpenAi,
                "gpt-test",
                "https://api.openai.test",
                true,
            )
            .unwrap();
        assert!(matches!(service.snapshot(), Err(LlmError::Request(_))));
        assert!(matches!(
            service.select(
                ProviderKind::OpenAi,
                "gpt-test",
                "https://api.openai.test",
                false
            ),
            Err(LlmError::DisclosureRequired)
        ));
        let selected = service
            .select(
                ProviderKind::OpenAi,
                "gpt-test",
                "https://api.openai.test",
                true,
            )
            .unwrap();
        assert_eq!(selected.provider, ProviderKind::OpenAi);
        assert!(service.snapshot().is_ok());
    }
}
