//! Supported languages and their tree-sitter grammars.

use tree_sitter::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
}

impl Lang {
    /// Map a file extension to a language, or `None` if unsupported.
    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext {
            "rs" => Some(Lang::Rust),
            "py" | "pyi" => Some(Lang::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "ts" | "tsx" | "mts" | "cts" => Some(Lang::TypeScript),
            "go" => Some(Lang::Go),
            _ => None,
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Go => "go",
        }
    }

    /// The tree-sitter grammar for this language.
    pub fn tree_sitter(&self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }
}
