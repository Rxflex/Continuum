//! MCP message dispatch. Translates JSON-RPC methods into tool calls and the
//! lifecycle responses agents expect.

use std::sync::Arc;

use continuum_core::MCP_PROTOCOL_VERSION;
use continuum_transport::jsonrpc::{error_codes, JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use serde_json::{json, Value};

use crate::{tools, Daemon};

/// Parse and handle one MCP message. Returns `None` for notifications, which
/// take no response.
pub async fn handle_message(raw: &str, daemon: &Arc<Daemon>) -> Option<JsonRpcResponse> {
    let req: JsonRpcRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            return Some(JsonRpcResponse::fail(
                Value::Null,
                JsonRpcError::new(error_codes::PARSE_ERROR, format!("parse error: {e}")),
            ));
        }
    };

    let is_notification = req.is_notification();
    let id = req.id.clone().unwrap_or(Value::Null);

    let outcome: Result<Value, JsonRpcError> = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "continuum",
                "version": env!("CARGO_PKG_VERSION"),
            },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => match serde_json::to_value(tools::tool_defs()) {
            Ok(list) => Ok(json!({ "tools": list })),
            Err(e) => Err(JsonRpcError::new(
                error_codes::INTERNAL_ERROR,
                e.to_string(),
            )),
        },
        "tools/call" => tools::call(req.params, daemon).await,
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        m if m.starts_with("notifications/") => return None,
        other => Err(JsonRpcError::new(
            error_codes::METHOD_NOT_FOUND,
            format!("method not found: {other}"),
        )),
    };

    if is_notification {
        return None;
    }
    Some(match outcome {
        Ok(value) => JsonRpcResponse::ok(id, value),
        Err(err) => JsonRpcResponse::fail(id, err),
    })
}
