use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use tree_sitter_python as tspy;
use tree_sitter_rust as tsrs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub path: PathBuf,
    pub line: usize,
    pub kind: SymbolKind,
    pub name: String,
    pub code: String,
}

struct LanguageConfig {
    language: fn() -> Language,
    extensions: &'static [&'static str],
    function_query: &'static str,
    class_query: Option<&'static str>,
}

fn lang_python() -> Language {
    tspy::LANGUAGE.into()
}

fn lang_rust() -> Language {
    tsrs::LANGUAGE.into()
}

fn language_registry() -> &'static [LanguageConfig] {
    &[
        LanguageConfig {
            language: lang_python,
            extensions: &["py"],
            function_query: r#"(function_definition name: (identifier) @name) @node"#,
            class_query: Some(r#"(class_definition name: (identifier) @name) @node"#),
        },
        LanguageConfig {
            language: lang_rust,
            extensions: &["rs"],
            function_query: r#"(function_item name: (identifier) @name) @node"#,
            class_query: None,
        },
    ]
}

fn language_config_for_path(path: &Path) -> Option<&'static LanguageConfig> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    language_registry()
        .iter()
        .find(|&cfg| cfg.extensions.iter().any(|e| *e == ext))
}

/// Enumerate symbols (functions/classes) for a single source file.
pub fn enumerate_symbols_in_file(path: &Path) -> Result<Vec<Symbol>, String> {
    let cfg = match language_config_for_path(path) {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };

    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;

    let mut parser = Parser::new();
    let language = (cfg.language)();
    parser
        .set_language(&language)
        .map_err(|_| "failed to set language".to_string())?;

    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| "failed to parse source".to_string())?;

    let mut symbols: Vec<Symbol> = Vec::new();
    let root = tree.root_node();

    // Helper to run a query and push symbols
    let mut run_query = |query_src: &str, kind: SymbolKind| -> Result<(), String> {
        let query = Query::new(&language, query_src)
            .map_err(|e| format!("invalid query for {}: {:?}", path.display(), e))?;
        let name_idx = query
            .capture_index_for_name("name")
            .ok_or_else(|| "query missing @name capture".to_string())?;
        let node_idx = query
            .capture_index_for_name("node")
            .ok_or_else(|| "query missing @node capture".to_string())?;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, source.as_bytes());
        while let Some(m) = matches.next() {
            let mut name_text: Option<String> = None;
            let mut def_node: Option<tree_sitter::Node> = None;
            for c in m.captures {
                if c.index == name_idx {
                    name_text = Some(source[c.node.byte_range()].to_string());
                } else if c.index == node_idx {
                    def_node = Some(c.node);
                }
            }

            if let (Some(name), Some(def_node)) = (name_text, def_node) {
                let line = def_node.start_position().row + 1;
                let code = source[def_node.byte_range()].to_string();
                symbols.push(Symbol {
                    path: path.to_path_buf(),
                    line,
                    kind: kind.clone(),
                    name,
                    code,
                });
            }
        }
        Ok(())
    };

    // Functions
    run_query(cfg.function_query, SymbolKind::Function)?;
    // Classes (if provided)
    if let Some(class_q) = cfg.class_query {
        run_query(class_q, SymbolKind::Class)?;
    }
    Ok(symbols)
}
