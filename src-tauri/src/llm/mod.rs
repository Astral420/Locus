//! LLM providers, prompt routing, summarization, and generation-model setup.
//!
//! The frontend only receives redacted provider metadata. Credentials are kept
//! in the OS keychain and are read by the provider clients at request time.

pub mod anthropic;
pub mod gemini;
pub mod llama_cpp;
pub mod model_manager;
pub mod ollama;
pub mod openai;
pub mod prompts;
pub mod providers;
pub mod summarization;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    LlamaServer,
    Ollama,
    OpenAi,
    Anthropic,
    Gemini,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LlamaServer => "llama_server",
            Self::Ollama => "ollama",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::LlamaServer => "Local llama-server",
            Self::Ollama => "Ollama",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Google Gemini",
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = LlmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "llama_server" | "llama-server" => Ok(Self::LlamaServer),
            "ollama" => Ok(Self::Ollama),
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "gemini" => Ok(Self::Gemini),
            _ => Err(LlmError::Configuration(format!(
                "unknown provider `{value}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionResponse {
    pub content: String,
    pub model: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

/// Implemented by every selected provider and deliberately small so summary
/// generation can be tested with a deterministic in-process fake.
pub trait CompletionProvider: Send + Sync {
    fn complete(&self, request: &ChatCompletionRequest)
        -> Result<ChatCompletionResponse, LlmError>;
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider configuration error: {0}")]
    Configuration(String),
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("provider returned HTTP status {status}")]
    HttpStatus { status: u16 },
    #[error("provider returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("provider credential is unavailable: {0}")]
    Credential(String),
    #[error("provider disclosure is required before sending meeting content")]
    DisclosureRequired,
    #[error("provider is not selected")]
    NotSelected,
}

pub(crate) fn response_error(response: reqwest::blocking::Response) -> LlmError {
    LlmError::HttpStatus {
        status: response.status().as_u16(),
    }
}

pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
) -> Result<T, LlmError> {
    if !response.status().is_success() {
        return Err(response_error(response));
    }
    response
        .json()
        .map_err(|error| LlmError::InvalidResponse(error.to_string()))
}
