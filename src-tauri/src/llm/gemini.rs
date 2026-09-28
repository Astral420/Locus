use super::{
    parse_json, ChatCompletionRequest, ChatCompletionResponse, CompletionProvider, LlmError,
};
use crate::keychain::Keychain;
use reqwest::blocking::Client;
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiClient {
    client: Client,
    base_url: String,
    api_key: String,
    default_model: String,
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_base_url(DEFAULT_BASE_URL, api_key, model)
    }

    pub fn with_base_url(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into().trim_end_matches('/').into(),
            api_key: api_key.into(),
            default_model: model.into(),
        }
    }

    pub fn from_keychain(keychain: &Keychain, model: impl Into<String>) -> Result<Self, LlmError> {
        let key = keychain
            .retrieve("llm:gemini")
            .map_err(|error| LlmError::Credential(error.to_string()))?;
        Ok(Self::new(key, model))
    }

    pub fn from_keychain_with_base_url(
        keychain: &Keychain,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let key = keychain
            .retrieve("llm:gemini")
            .map_err(|error| LlmError::Credential(error.to_string()))?;
        Ok(Self::with_base_url(base_url, key, model))
    }
}

impl CompletionProvider for GeminiClient {
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };
        let contents = request
            .messages
            .iter()
            .filter(|message| message.role != "system")
            .map(|message| {
                serde_json::json!({
                    "role": if message.role == "assistant" { "model" } else { "user" },
                    "parts": [{"text": message.content}],
                })
            })
            .collect::<Vec<_>>();
        let system_instruction = request
            .messages
            .iter()
            .find(|message| message.role == "system")
            .map(|message| serde_json::json!({"parts": [{"text": message.content}]}));
        let body = serde_json::json!({
            "systemInstruction": system_instruction,
            "contents": contents,
            "generationConfig": {"temperature": request.temperature},
        });
        let url = format!("{}/models/{}:generateContent", self.base_url, model);
        let response = self
            .client
            .post(url)
            .query(&[("key", &self.api_key)])
            .json(&body)
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        let parsed: GeminiResponse = parse_json(response)?;
        let content = parsed
            .candidates
            .into_iter()
            .next()
            .and_then(|candidate| candidate.content)
            .map(|content| {
                content
                    .parts
                    .into_iter()
                    .filter_map(|part| part.text)
                    .collect::<Vec<_>>()
                    .join("")
            })
            .filter(|content| !content.is_empty())
            .ok_or_else(|| LlmError::InvalidResponse("response contains no text".into()))?;
        Ok(ChatCompletionResponse {
            content,
            model: Some(model.clone()),
            prompt_tokens: parsed
                .usage_metadata
                .as_ref()
                .and_then(|usage| usage.prompt_token_count),
            completion_tokens: parsed
                .usage_metadata
                .and_then(|usage| usage.candidates_token_count),
        })
    }
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiUsage {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
}
