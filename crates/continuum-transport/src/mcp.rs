//! MCP-specific wire types layered on top of JSON-RPC.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A tool advertised to agents via `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

impl ToolDef {
    pub fn new(name: &str, description: &str, input_schema: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Result of an MCP `tools/call`, rendered as a single text content block.
#[derive(Debug, Clone)]
pub struct CallToolResult {
    pub text: String,
    pub is_error: bool,
}

impl CallToolResult {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }

    /// Serialize to the MCP `CallToolResult` JSON shape.
    pub fn into_value(self) -> Value {
        json!({
            "content": [{ "type": "text", "text": self.text }],
            "isError": self.is_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_def_renames_input_schema() {
        let def = ToolDef::new("demo", "a demo tool", json!({ "type": "object" }));
        let v = serde_json::to_value(&def).unwrap();
        assert_eq!(v["name"], "demo");
        assert_eq!(v["description"], "a demo tool");
        assert!(v.get("inputSchema").is_some());
        assert!(v.get("input_schema").is_none());
    }

    #[test]
    fn call_tool_result_ok_shape() {
        let v = CallToolResult::ok("done").into_value();
        assert_eq!(v["isError"], false);
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][0]["text"], "done");
    }

    #[test]
    fn call_tool_result_error_shape() {
        let v = CallToolResult::error("boom").into_value();
        assert_eq!(v["isError"], true);
        assert_eq!(v["content"][0]["text"], "boom");
    }
}
