use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    io,
    time::{Duration, Instant},
};
use thiserror::Error;

pub const JSONRPC_VERSION: &str = "2.0";
pub const DEFAULT_MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcRequest<'a, T: Serialize> {
    pub jsonrpc: &'static str,
    pub method: &'a str,
    pub params: T,
    pub id: RequestId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: Option<RequestId>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("frame is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("frame exceeds {limit} bytes")]
    FrameTooLarge { limit: usize },
    #[error("malformed JSON-RPC frame: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid JSON-RPC version")]
    InvalidVersion,
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
    #[error("unknown response id")]
    UnknownResponseId,
    #[error("response did not contain a result")]
    MissingResult,
    #[error("remote error {0}: {1}")]
    Remote(i64, String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

pub fn encode_request<T: Serialize>(
    method: &str,
    params: T,
    id: RequestId,
    max: usize,
) -> Result<Vec<u8>, ProtocolError> {
    let request = RpcRequest {
        jsonrpc: JSONRPC_VERSION,
        method,
        params,
        id,
    };
    let mut bytes = serde_json::to_vec(&request)?;
    if bytes.len() + 1 > max {
        return Err(ProtocolError::FrameTooLarge { limit: max });
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn decode_frame(line: &[u8], max: usize) -> Result<DecodedFrame, ProtocolError> {
    if line.len() > max {
        return Err(ProtocolError::FrameTooLarge { limit: max });
    }
    let text = std::str::from_utf8(line)?.trim();
    let value: Value = serde_json::from_str(text)?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some(JSONRPC_VERSION) {
        return Err(ProtocolError::InvalidVersion);
    }
    if value.get("method").is_some() {
        Ok(DecodedFrame::Notification(serde_json::from_value(value)?))
    } else {
        Ok(DecodedFrame::Response(serde_json::from_value(value)?))
    }
}

pub enum DecodedFrame {
    Response(RpcResponse),
    Notification(RpcNotification),
}

#[derive(Default)]
pub struct RequestTracker {
    next: i64,
    pending: HashMap<RequestId, Instant>,
}
impl RequestTracker {
    pub fn new_id(&mut self) -> RequestId {
        self.next += 1;
        let id = RequestId::Number(self.next);
        self.pending.insert(id.clone(), Instant::now());
        id
    }
    pub fn finish(&mut self, id: &RequestId) -> Result<(), ProtocolError> {
        self.pending
            .remove(id)
            .map(|_| ())
            .ok_or(ProtocolError::UnknownResponseId)
    }
    pub fn expire(&mut self, deadline: Instant) {
        self.pending.retain(|_, started| *started < deadline);
    }
}

pub fn parse_result<T: DeserializeOwned>(response: RpcResponse) -> Result<T, ProtocolError> {
    if let Some(error) = response.error {
        return Err(ProtocolError::Remote(error.code, error.message));
    }
    response
        .result
        .map(serde_json::from_value)
        .transpose()?
        .ok_or(ProtocolError::MissingResult)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_request_and_response() {
        let encoded = encode_request(
            "health",
            json!({}),
            RequestId::String("a".into()),
            DEFAULT_MAX_FRAME_SIZE,
        )
        .unwrap();
        assert!(matches!(
            decode_frame(&encoded, DEFAULT_MAX_FRAME_SIZE).unwrap(),
            DecodedFrame::Response(_) | DecodedFrame::Notification(_)
        ));
        let response = b"{\"jsonrpc\":\"2.0\",\"id\":\"a\",\"result\":{\"ok\":true}}\n";
        let DecodedFrame::Response(response) =
            decode_frame(response, DEFAULT_MAX_FRAME_SIZE).unwrap()
        else {
            panic!()
        };
        let result: serde_json::Value = parse_result(response).unwrap();
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn rejects_malformed_or_oversized_frames() {
        assert!(decode_frame(b"not-json\n", DEFAULT_MAX_FRAME_SIZE).is_err());
        assert!(matches!(
            decode_frame(b"{}", 1),
            Err(ProtocolError::FrameTooLarge { .. })
        ));
        assert!(matches!(
            decode_frame(br#"{"jsonrpc":"1.0"}"#, DEFAULT_MAX_FRAME_SIZE),
            Err(ProtocolError::InvalidVersion)
        ));
    }

    #[test]
    fn tracker_rejects_unknown_ids() {
        let mut tracker = RequestTracker::default();
        let id = tracker.new_id();
        assert!(tracker.finish(&id).is_ok());
        assert!(matches!(
            tracker.finish(&id),
            Err(ProtocolError::UnknownResponseId)
        ));
    }
}
