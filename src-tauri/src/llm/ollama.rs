use super::{
    parse_json, ChatCompletionRequest, ChatCompletionResponse, CompletionProvider, LlmError,
};
use reqwest::{blocking::Client, Url};
use serde::Deserialize;
use std::net::IpAddr;

pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointDestination {
    Loopback,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OllamaEndpoint {
    pub url: String,
    pub destination: EndpointDestination,
    pub reachable: bool,
}

pub fn classify_endpoint(endpoint: &str) -> Result<EndpointDestination, LlmError> {
    let url = Url::parse(endpoint)
        .map_err(|error| LlmError::Configuration(format!("invalid Ollama endpoint: {error}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| LlmError::Configuration("Ollama endpoint has no host".into()))?;
    let loopback = match host {
        "localhost" => true,
        value => value
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false),
    };
    Ok(if loopback {
        EndpointDestination::Loopback
    } else {
        EndpointDestination::Remote
    })
}

pub struct OllamaClient {
    client: Client,
    endpoint: String,
    default_model: String,
}

impl OllamaClient {
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Result<Self, LlmError> {
        let endpoint = endpoint.into().trim_end_matches('/').to_owned();
        Url::parse(&endpoint).map_err(|error| LlmError::Configuration(error.to_string()))?;
        Ok(Self {
            client: Client::new(),
            endpoint,
            default_model: model.into(),
        })
    }

    pub fn discover(endpoint: impl Into<String>) -> Result<OllamaEndpoint, LlmError> {
        let endpoint = endpoint.into().trim_end_matches('/').to_owned();
        let destination = classify_endpoint(&endpoint)?;
        let response = Client::new()
            .get(format!("{endpoint}/api/tags"))
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        Ok(OllamaEndpoint {
            url: endpoint,
            destination,
            reachable: response.status().is_success(),
        })
    }

    pub fn endpoint(&self) -> OllamaEndpoint {
        OllamaEndpoint {
            url: self.endpoint.clone(),
            destination: classify_endpoint(&self.endpoint).unwrap_or(EndpointDestination::Remote),
            reachable: false,
        }
    }
}

impl CompletionProvider for OllamaClient {
    fn complete(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, LlmError> {
        let model = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };
        let response = self
            .client
            .post(format!("{}/api/chat", self.endpoint))
            .json(&serde_json::json!({
                "model": model,
                "messages": request.messages,
                "stream": false,
                "options": {"temperature": request.temperature},
            }))
            .send()
            .map_err(|error| LlmError::Request(error.to_string()))?;
        let parsed: OllamaResponse = parse_json(response)?;
        Ok(ChatCompletionResponse {
            content: parsed.message.content,
            model: parsed.model,
            prompt_tokens: parsed.prompt_eval_count,
            completion_tokens: parsed.eval_count,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    model: Option<String>,
    message: OllamaMessage,
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_classification_is_explicit() {
        assert_eq!(
            classify_endpoint("http://127.0.0.1:11434").unwrap(),
            EndpointDestination::Loopback
        );
        assert_eq!(
            classify_endpoint("http://localhost:11434").unwrap(),
            EndpointDestination::Loopback
        );
        assert_eq!(
            classify_endpoint("https://ollama.example.test").unwrap(),
            EndpointDestination::Remote
        );
    }
}
