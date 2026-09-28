use super::{
    parse_json, ChatCompletionRequest, ChatCompletionResponse, CompletionProvider, LlmError,
};
use crate::keychain::Keychain;
use reqwest::blocking::Client;
use serde::Deserialize;

const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicClient {
    client: Client,
    endpoint: String,
    api_key: String,
    default_model: String,
}

impl AnthropicClient {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_endpoint(DEFAULT_ENDPOINT, api_key, model)
    }

    pub fn with_endpoint(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            default_model: model.into(),
        }
    }

    pub fn from_keychain(keychain: &Keychain, model: impl Into<String>) -> Result<Self, LlmError> {
        let key = keychain
            .retrieve("llm:anthropic")
            .map_err(|error| LlmError::Credential(error.to_string()))?;
        Ok(Self::new(key, model))
    }

    pub fn from_keychain_with_endpoint(
        keychain: &Keychain,
        endpoint: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let key = keychain
            .retrieve("llm:anthropic")
            .map_err(|error| LlmError::Credential(error.to_string()))?;
        Ok(Self::with_endpoint(endpoint, key, model))
    }
}

impl CompletionProvider for AnthropicClient {
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let mut system = None;
        let mut messages = Vec::new();
        for message in &request.messages {
            if message.role == "system" {
                system = Some(message.content.clone());
            } else {
                messages.push(serde_json::json!({
                    "role": message.role,
                    "content": message.content,
                }));
            }
        }
        let body = serde_json::json!({
            "model": if request.model.is_empty() { &self.default_model } else { &request.model },
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "temperature": request.temperature,
            "system": system,
            "messages": messages,
        });
        let response = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body)
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        let parsed: AnthropicResponse = parse_json(response)?;
        let content = parsed
            .content
            .into_iter()
            .find(|block| block.kind == "text")
            .map(|block| block.text)
            .ok_or_else(|| LlmError::InvalidResponse("response contains no text block".into()))?;
        Ok(ChatCompletionResponse {
            content,
            model: parsed.model,
            prompt_tokens: parsed.usage.as_ref().and_then(|usage| usage.input_tokens),
            completion_tokens: parsed.usage.and_then(|usage| usage.output_tokens),
        })
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}
