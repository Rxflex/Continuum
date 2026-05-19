//! Tree-sitter parsing: turn a source file into graph nodes.
//!
//! Rather than `.scm` query files, the indexer walks the syntax tree directly
//! and classifies nodes per language. This keeps grammar handling in plain Rust
//! and degrades gracefully -- an unknown node kind is simply skipped.

use continuum_graph::{CallSite, GraphNode, NodeKind};
use tree_sitter::{Node, Parser};

use crate::languages::Lang;

/// Result of parsing one file: its file node plus every symbol within it.
pub struct ParsedFile {
    pub file_node: GraphNode,
    pub symbols: Vec<GraphNode>,
}

struct RawSym {
    kind: NodeKind,
    name: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    end_line: usize,
    signature: String,
    source: String,
}

struct RawCall {
    name: String,
    line: usize,
    byte: usize,
}

/// Parse `source` for `rel_path`. Returns `None` if the grammar fails to load
/// or the file cannot be parsed at all.
pub fn parse(rel_path: &str, source: &str, lang: Lang) -> Option<ParsedFile> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.tree_sitter()).is_err() {
        return None;
    }
    let tree = parser.parse(source, None)?;
    let src = source.as_bytes();

    let mut syms: Vec<RawSym> = Vec::new();
    let mut calls: Vec<RawCall> = Vec::new();
    walk(tree.root_node(), src, lang, &mut syms, &mut calls);

    let mut nodes: Vec<GraphNode> = syms
        .iter()
        .map(|s| GraphNode {
            id: format!("{}::{}::{}", rel_path, s.name, s.start_line),
            kind: s.kind,
            name: s.name.clone(),
            path: rel_path.to_string(),
            language: String::new(),
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature.clone(),
            source: s.source.clone(),
            docstring: None,
            calls: Vec::new(),
        })
        .collect();

    // Attribute each call to the innermost enclosing symbol.
    for call in &calls {
        let mut best: Option<usize> = None;
        let mut best_span = usize::MAX;
        for (i, s) in syms.iter().enumerate() {
            if s.start_byte <= call.byte && call.byte < s.end_byte {
                let span = s.end_byte - s.start_byte;
                if span < best_span {
                    best_span = span;
                    best = Some(i);
                }
            }
        }
        if let Some(i) = best {
            nodes[i].calls.push(CallSite { name: call.name.clone(), line: call.line });
        }
    }

    Some(ParsedFile {
        file_node: GraphNode::file(rel_path, lang.slug()),
        symbols: nodes,
    })
}

fn walk(node: Node, src: &[u8], lang: Lang, syms: &mut Vec<RawSym>, calls: &mut Vec<RawCall>) {
    if let Some((kind, name_field)) = def_spec(lang, node.kind()) {
        if let Some(name_node) = node.child_by_field_name(name_field) {
            if let Ok(name) = name_node.utf8_text(src) {
                let source = text_of(node, src);
                let signature =
                    source.lines().next().unwrap_or("").trim().to_string();
                syms.push(RawSym {
                    kind,
                    name: name.to_string(),
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    start_line: node.start_position().row + 1,
                    end_line: node.end_position().row + 1,
                    signature,
                    source,
                });
            }
        }
    }
    if let Some(callee) = callee_name(lang, node, src) {
        calls.push(RawCall {
            name: callee,
            line: node.start_position().row + 1,
            byte: node.start_byte(),
        });
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.named_children(&mut cursor).collect();
    for child in children {
        walk(child, src, lang, syms, calls);
    }
}

/// Definition node kinds per language: `(symbol kind, name field name)`.
fn def_spec(lang: Lang, kind: &str) -> Option<(NodeKind, &'static str)> {
    match (lang, kind) {
        (Lang::Rust, "function_item") => Some((NodeKind::Function, "name")),
        (Lang::Rust, "struct_item") => Some((NodeKind::Struct, "name")),
        (Lang::Rust, "enum_item") => Some((NodeKind::Enum, "name")),
        (Lang::Rust, "trait_item") => Some((NodeKind::Trait, "name")),

        (Lang::Python, "function_definition") => Some((NodeKind::Function, "name")),
        (Lang::Python, "class_definition") => Some((NodeKind::Class, "name")),

        (Lang::JavaScript | Lang::TypeScript, "function_declaration") => {
            Some((NodeKind::Function, "name"))
        }
        (Lang::JavaScript | Lang::TypeScript, "generator_function_declaration") => {
            Some((NodeKind::Function, "name"))
        }
        (Lang::JavaScript | Lang::TypeScript, "class_declaration") => {
            Some((NodeKind::Class, "name"))
        }
        (Lang::JavaScript | Lang::TypeScript, "method_definition") => {
            Some((NodeKind::Method, "name"))
        }
        (Lang::TypeScript, "interface_declaration") => Some((NodeKind::Interface, "name")),

        (Lang::Go, "function_declaration") => Some((NodeKind::Function, "name")),
        (Lang::Go, "method_declaration") => Some((NodeKind::Method, "name")),
        (Lang::Go, "type_spec") => Some((NodeKind::Struct, "name")),

        _ => None,
    }
}

/// Extract the callee name if `node` is a call expression.
fn callee_name(lang: Lang, node: Node, src: &[u8]) -> Option<String> {
    let kind = node.kind();
    let is_call = match lang {
        Lang::Rust => kind == "call_expression" || kind == "macro_invocation",
        Lang::Python => kind == "call",
        Lang::JavaScript | Lang::TypeScript | Lang::Go => kind == "call_expression",
    };
    if !is_call {
        return None;
    }
    let fn_node = if lang == Lang::Rust && kind == "macro_invocation" {
        node.child_by_field_name("macro")?
    } else {
        node.child_by_field_name("function")?
    };
    name_from_callee(fn_node, src)
}

fn name_from_callee(node: Node, src: &[u8]) -> Option<String> {
    let text = |n: Node| n.utf8_text(src).ok().map(str::to_string);
    match node.kind() {
        "identifier" | "field_identifier" | "property_identifier" | "type_identifier" => {
            text(node)
        }
        "field_expression" => node.child_by_field_name("field").and_then(text),
        "scoped_identifier" => node.child_by_field_name("name").and_then(text),
        "member_expression" => node.child_by_field_name("property").and_then(text),
        "attribute" => node.child_by_field_name("attribute").and_then(text),
        "selector_expression" => node.child_by_field_name("field").and_then(text),
        _ => None,
    }
}

fn text_of(node: Node, src: &[u8]) -> String {
    std::str::from_utf8(&src[node.start_byte()..node.end_byte()])
        .unwrap_or("")
        .to_string()
}
