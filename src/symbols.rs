use crate::ast_embed::{AstEmbedder, AstStructure, EmbeddingStrategy};
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};
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
    pub ast_embedding: Vec<f32>,
}

/// Global AST embedder instance for consistent embeddings
pub struct SymbolEmbedder {
    ast_embedder: AstEmbedder,
}

impl SymbolEmbedder {
    pub fn new() -> Self {
        let strategy = EmbeddingStrategy::Combined {
            vocab_size: 100,
            max_paths: 20,
        };
        Self {
            ast_embedder: AstEmbedder::new(strategy),
        }
    }

    pub fn embed_symbol(&self, node: Node) -> Vec<f32> {
        let structure = AstStructure::from_node(node);
        self.ast_embedder.embed(&structure)
    }

    pub fn dimension(&self) -> usize {
        self.ast_embedder.dimension()
    }

    pub fn build_vocabulary_from_structures(&mut self, structures: &[AstStructure]) {
        self.ast_embedder.build_vocabulary(structures);
    }
}

/// Extract AST structures from symbols for vocabulary building
pub fn extract_ast_structures_from_files(paths: &[&Path]) -> Result<Vec<AstStructure>, String> {
    let mut all_structures = Vec::new();

    for path in paths {
        let structures = extract_ast_structures_from_file(path)?;
        all_structures.extend(structures);
    }

    Ok(all_structures)
}

/// Extract just AST structures from a file without embeddings
fn extract_ast_structures_from_file(path: &Path) -> Result<Vec<AstStructure>, String> {
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

    let mut structures = Vec::new();
    let root = tree.root_node();

    // Helper to run a query and extract AST structures
    let mut extract_structures = |query_src: &str| -> Result<(), String> {
        let query = Query::new(&language, query_src)
            .map_err(|e| format!("invalid query for {}: {:?}", path.display(), e))?;
        let node_idx = query
            .capture_index_for_name("node")
            .ok_or_else(|| "query missing @node capture".to_string())?;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, source.as_bytes());
        while let Some(m) = matches.next() {
            for c in m.captures {
                if c.index == node_idx {
                    let structure = AstStructure::from_node(c.node);
                    structures.push(structure);
                }
            }
        }
        Ok(())
    };

    // Extract structures from functions
    extract_structures(cfg.function_query)?;

    // Extract structures from classes (if provided)
    if let Some(class_q) = cfg.class_query {
        extract_structures(class_q)?;
    }

    Ok(structures)
}

/// Batch process files with vocabulary building for consistent embeddings
pub fn enumerate_symbols_batch(paths: &[&Path]) -> Result<Vec<Symbol>, String> {
    // First pass: extract all AST structures to build vocabulary
    let all_structures = extract_ast_structures_from_files(paths)?;

    // Build embedder with vocabulary from all structures
    let mut embedder = SymbolEmbedder::new();
    embedder.build_vocabulary_from_structures(&all_structures);

    // Second pass: extract symbols with embeddings
    let mut all_symbols = Vec::new();
    for path in paths {
        let symbols = enumerate_symbols_in_file_with_embedder(path, Some(&embedder))?;
        all_symbols.extend(symbols);
    }

    Ok(all_symbols)
}

/// Generate AST embedding for a code snippet query
pub fn embed_query_snippet(query: &str, embedder: &SymbolEmbedder) -> Result<Vec<f32>, String> {
    // Try to parse the query with different language parsers
    let languages = [
        ("rust", lang_rust as fn() -> Language),
        ("python", lang_python as fn() -> Language),
    ];

    for (_lang_name, lang_fn) in &languages {
        if let Ok(embedding) = try_parse_and_embed(query, lang_fn(), embedder) {
            return Ok(embedding);
        }
    }

    // If direct parsing fails, try wrapping in function contexts
    for (_lang_name, lang_fn) in &languages {
        let wrapped_query = match *_lang_name {
            "rust" => format!("fn query_fn() {{ {} }}", query),
            "python" => format!("def query_fn():\n    {}", query.replace('\n', "\n    ")),
            _ => continue,
        };

        if let Ok(embedding) = try_parse_and_embed(&wrapped_query, lang_fn(), embedder) {
            return Ok(embedding);
        }
    }

    Err("Could not parse query as valid code in any supported language".to_string())
}

/// Try to parse code with a specific language and generate embedding
fn try_parse_and_embed(
    code: &str,
    language: Language,
    embedder: &SymbolEmbedder,
) -> Result<Vec<f32>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| "failed to set language".to_string())?;

    let tree = parser
        .parse(code, None)
        .ok_or_else(|| "failed to parse code".to_string())?;

    let root = tree.root_node();

    // Check if parsing was successful (no error nodes)
    if has_error_nodes(root) {
        return Err("parsing resulted in error nodes".to_string());
    }

    // Generate embedding for the entire parsed tree
    let embedding = embedder.embed_symbol(root);
    Ok(embedding)
}

/// Check if AST has any error nodes indicating failed parsing
fn has_error_nodes(node: Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if has_error_nodes(child) {
                return true;
            }
        }
    }

    false
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
/// This function is kept for backward compatibility and single-file processing.
#[allow(dead_code)]
pub fn enumerate_symbols_in_file(path: &Path) -> Result<Vec<Symbol>, String> {
    enumerate_symbols_in_file_with_embedder(path, None)
}

/// Enumerate symbols with optional pre-built embedder for consistency
fn enumerate_symbols_in_file_with_embedder(
    path: &Path,
    embedder: Option<&SymbolEmbedder>,
) -> Result<Vec<Symbol>, String> {
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

    // Create AST embedder for this file if not provided
    let default_embedder;
    let embedder = match embedder {
        Some(emb) => emb,
        None => {
            default_embedder = SymbolEmbedder::new();
            &default_embedder
        }
    };

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

                // Generate AST structural embedding
                let ast_embedding = embedder.embed_symbol(def_node);

                symbols.push(Symbol {
                    path: path.to_path_buf(),
                    line,
                    kind: kind.clone(),
                    name,
                    code,
                    ast_embedding,
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
