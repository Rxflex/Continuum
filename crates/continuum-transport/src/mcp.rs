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
        Self { text: text.into(), is_error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: true }
    }

    /// Serialize to the MCP `CallToolResult` JSON shape.
    pub fn into_value(self) -> Value {
        json!({
            "content": [{ "type": "text", "text": self.text }],
            "isError": self.is_error,
        })
    }
}
