//! JSON-RPC 2.0 message types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An incoming JSON-RPC request (or notification, when `id` is absent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// A request without an `id` is a notification -- no response is sent.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// An outgoing JSON-RPC response. Exactly one of `result` / `error` is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn fail(id: Value, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// Standard JSON-RPC 2.0 error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_with_id_is_not_a_notification() {
        let req: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap();
        assert!(!req.is_notification());
        assert_eq!(req.method, "ping");
    }

    #[test]
    fn request_without_id_is_a_notification() {
        let req: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn ok_response_has_result_and_no_error() {
        let resp = JsonRpcResponse::ok(json!(1), json!({ "x": 2 }));
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["x"], 2);
        assert!(v.get("error").is_none());
    }

    #[test]
    fn fail_response_has_error_and_no_result() {
        let resp = JsonRpcResponse::fail(
            json!(7),
            JsonRpcError::new(error_codes::METHOD_NOT_FOUND, "nope"),
        );
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["error"]["code"], error_codes::METHOD_NOT_FOUND);
        assert_eq!(v["error"]["message"], "nope");
        assert!(v.get("result").is_none());
    }
}
