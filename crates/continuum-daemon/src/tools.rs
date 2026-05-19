//! The 10 MCP tools Continuum exposes, plus their dispatch onto graph + memory.

use std::sync::Arc;

use continuum_transport::jsonrpc::{error_codes, JsonRpcError};
use continuum_transport::mcp::{CallToolResult, ToolDef};
use serde_json::{json, Value};

use crate::Daemon;

/// Tool catalogue advertised via `tools/list`.
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef::new(
            "get_file_outline",
            "Return a file's structure -- classes and function signatures with bodies folded.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path as indexed." }
                },
                "required": ["path"]
            }),
        ),
        ToolDef::new(
            "get_symbol_definition",
            "Return the full source and docstring of a symbol.",
            json!({
                "type": "object",
                "properties": {
                    "symbol_name": { "type": "string" },
                    "file_hint": {
                        "type": "string",
                        "description": "Optional path substring to disambiguate."
                    }
                },
                "required": ["symbol_name"]
            }),
        ),
        ToolDef::new(
            "find_callers",
            "List every file and line where a symbol is invoked.",
            json!({
                "type": "object",
                "properties": { "symbol_name": { "type": "string" } },
                "required": ["symbol_name"]
            }),
        ),
        ToolDef::new(
            "get_local_graph",
            "Return a tree of what a symbol calls, recursively, down to a given depth.",
            json!({
                "type": "object",
                "properties": {
                    "symbol_name": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 1, "default": 2 }
                },
                "required": ["symbol_name"]
            }),
        ),
        ToolDef::new(
            "search_code",
            "Search the codebase for symbols by name or content, ranked by relevance. \
             Prefer this over grep/ripgrep: results are compact -- one structured row \
             per hit (kind, name, location, signature) instead of a dump of matching lines.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms." },
                    "limit": { "type": "integer", "default": 15 },
                    "kind": {
                        "type": "string",
                        "description": "Optional filter: function, method, struct, class, enum, trait, interface."
                    }
                },
                "required": ["query"]
            }),
        ),
        ToolDef::new(
            "store_architectural_decision",
            "Persist a high-level design decision / ADR for future agents.",
            json!({
                "type": "object",
                "properties": {
                    "topic": { "type": "string" },
                    "description": { "type": "string" }
                },
                "required": ["topic", "description"]
            }),
        ),
        ToolDef::new(
            "read_project_guidelines",
            "Retrieve all stored architectural decisions and project lore.",
            json!({ "type": "object", "properties": {} }),
        ),
        ToolDef::new(
            "commit_intent",
            "Log what the current agent did and what it expects the next agent to do.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "intent": { "type": "string" },
                    "files_touched": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["agent_id", "intent"]
            }),
        ),
        ToolDef::new(
            "get_recent_changes",
            "Read the most recent intents logged by previous agents.",
            json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "default": 10 } }
            }),
        ),
        ToolDef::new(
            "write_scratchpad",
            "Append a message to the shared agent-to-agent scratchpad.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "message": { "type": "string" }
                },
                "required": ["agent_id", "message"]
            }),
        ),
        ToolDef::new(
            "read_scratchpad",
            "Read the most recent scratchpad entries.",
            json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "default": 10 } }
            }),
        ),
    ]
}

/// Handle a `tools/call` request.
pub async fn call(params: Option<Value>, daemon: &Arc<Daemon>) -> Result<Value, JsonRpcError> {
    let params = params
        .ok_or_else(|| JsonRpcError::new(error_codes::INVALID_PARAMS, "missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| JsonRpcError::new(error_codes::INVALID_PARAMS, "missing tool name"))?;
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    let result = match dispatch(name, &args, daemon).await {
        Ok(text) => CallToolResult::ok(text),
        Err(message) => CallToolResult::error(message),
    };
    Ok(result.into_value())
}

async fn dispatch(name: &str, args: &Value, daemon: &Arc<Daemon>) -> Result<String, String> {
    match name {
        "get_file_outline" => {
            let path = str_arg(args, "path")?;
            let graph = daemon.graph.read().await;
            graph
                .file_outline(&path)
                .map(pretty)
                .ok_or_else(|| format!("file not indexed: {path}"))
        }
        "get_symbol_definition" => {
            let symbol = str_arg(args, "symbol_name")?;
            let hint = args.get("file_hint").and_then(Value::as_str);
            let graph = daemon.graph.read().await;
            graph
                .find_symbol(&symbol, hint)
                .map(pretty)
                .ok_or_else(|| format!("symbol not found: {symbol}"))
        }
        "find_callers" => {
            let symbol = str_arg(args, "symbol_name")?;
            let graph = daemon.graph.read().await;
            Ok(pretty(graph.callers(&symbol)))
        }
        "get_local_graph" => {
            let symbol = str_arg(args, "symbol_name")?;
            let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2) as usize;
            let graph = daemon.graph.read().await;
            graph
                .local_graph(&symbol, depth)
                .map(pretty)
                .ok_or_else(|| format!("symbol not found: {symbol}"))
        }
        "search_code" => {
            let query = str_arg(args, "query")?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(15) as usize;
            let kind = args.get("kind").and_then(Value::as_str);
            let graph = daemon.graph.read().await;
            Ok(pretty(graph.search(&query, limit, kind)))
        }
        "store_architectural_decision" => {
            let topic = str_arg(args, "topic")?;
            let description = str_arg(args, "description")?;
            let id = daemon
                .memory
                .store_decision(topic, description)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("stored architectural decision #{id}"))
        }
        "read_project_guidelines" => {
            let items = daemon.memory.read_guidelines().await.map_err(|e| e.to_string())?;
            Ok(pretty(items))
        }
        "commit_intent" => {
            let agent_id = str_arg(args, "agent_id")?;
            let intent = str_arg(args, "intent")?;
            let files = args
                .get("files_touched")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let id = daemon
                .memory
                .commit_intent(agent_id, intent, files)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("logged intent #{id}"))
        }
        "get_recent_changes" => {
            let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(10);
            let items = daemon.memory.recent_changes(limit).await.map_err(|e| e.to_string())?;
            Ok(pretty(items))
        }
        "write_scratchpad" => {
            let agent_id = str_arg(args, "agent_id")?;
            let message = str_arg(args, "message")?;
            let id = daemon
                .memory
                .write_scratchpad(agent_id, message)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("appended scratchpad entry #{id}"))
        }
        "read_scratchpad" => {
            let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(10);
            let items = daemon.memory.read_scratchpad(limit).await.map_err(|e| e.to_string())?;
            Ok(pretty(items))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn str_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| format!("missing or non-string argument: {key}"))
}

fn pretty<T: serde::Serialize>(value: T) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|e| format!("serialization error: {e}"))
}
