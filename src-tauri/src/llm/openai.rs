use super::{
    parse_json, ChatCompletionRequest, ChatCompletionResponse, CompletionProvider, LlmError,
};
use crate::keychain::Keychain;
use reqwest::blocking::Client;
use serde::Deserialize;

const DEFAULT_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

pub struct OpenAiClient {
    client: Client,
    endpoint: String,
    api_key: String,
    default_model: String,
}

impl OpenAiClient {
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
            .retrieve("llm:openai")
            .map_err(|error| LlmError::Credential(error.to_string()))?;
        Ok(Self::new(key, model))
    }

    pub fn from_keychain_with_endpoint(
        keychain: &Keychain,
        endpoint: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let key = keychain
            .retrieve("llm:openai")
            .map_err(|error| LlmError::Credential(error.to_string()))?;
        Ok(Self::with_endpoint(endpoint, key, model))
    }
}

impl CompletionProvider for OpenAiClient {
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let body = serde_json::json!({
            "model": if request.model.is_empty() { &self.default_model } else { &request.model },
            "messages": request.messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens,
        });
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        let parsed: OpenAiResponse = parse_json(response)?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("response contains no choices".into()))?;
        Ok(ChatCompletionResponse {
            content: choice.message.content,
            model: parsed.model,
            prompt_tokens: parsed.usage.as_ref().and_then(|usage| usage.prompt_tokens),
            completion_tokens: parsed.usage.and_then(|usage| usage.completion_tokens),
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}
