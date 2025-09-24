use clap::{Parser, Subcommand};
use std::path::Path;
mod ast_embed;
mod db;
mod index;
mod symbols;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

#[derive(Parser, Debug)]
#[command(
    name = "cearch",
    about = "Codebase semantic search toolkit",
    version,
    author
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Index a repository into embeddings and a vector index
    Index {
        /// Optional flag to force re-indexing
        #[arg(long)]
        force: bool,
        /// Verbose output (show progress bars)
        #[arg(short = 'v', long)]
        verbose: bool,
    },
    /// Initialize cearch in this repo (.cearch dir, .gitignore, and model cache)
    Init {},
    /// Query the index with a code snippet or description
    Query {
        /// The query string
        query: String,
        /// Number of results to return
        #[arg(short = 'n', long, default_value_t = 7)]
        num_results: usize,
    },
    /// Clean the index and embeddings for a repository
    Clean {},
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index { force: _, verbose } => {
            let cwd = match std::env::current_dir() {
                Ok(dir) => dir,
                Err(err) => {
                    eprintln!("error: failed to read current directory: {}", err);
                    std::process::exit(2);
                }
            };

            let root = match index::find_git_root(&cwd) {
                Some(dir) => dir,
                None => {
                    eprintln!("error: not inside a git repository: {}", cwd.display());
                    std::process::exit(2);
                }
            };
            match index::list_git_tracked_files(&root) {
                Ok(files) => {
                    // Get AST embedder dimension
                    let embedder = symbols::SymbolEmbedder::new();
                    let ast_dim = embedder.dimension();

                    // Open DB with AST embedding dimension
                    let db = match db::DB::open_with_dim(&root, ast_dim) {
                        Ok(db) => db,
                        Err(err) => {
                            eprintln!("error: failed to open sqlite index: {}", err);
                            std::process::exit(2);
                        }
                    };

                    // Optional progress
                    let mp = if verbose {
                        Some(MultiProgress::new())
                    } else {
                        None
                    };
                    let main_pb = if let Some(ref mp) = mp {
                        let pb = mp.add(ProgressBar::new(files.len() as u64));
                        if let Ok(style) = ProgressStyle::with_template(
                            "{spinner:.green} {pos}/{len} [{bar:40.white/black}] {per_sec} ETA {eta} {msg}",
                        ) {
                            pb.set_style(style.progress_chars("=> "));
                        }
                        pb.set_message(String::from("Indexing repo"));
                        Some(pb)
                    } else {
                        None
                    };

                    // Process files with AST embeddings using batch processing for vocabulary consistency
                    let file_paths: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();
                    let all_symbols = match symbols::enumerate_symbols_batch(&file_paths) {
                        Ok(symbols) => symbols,
                        Err(err) => {
                            eprintln!("error: failed to process symbols: {}", err);
                            std::process::exit(2);
                        }
                    };

                    // Insert symbols with progress tracking
                    if let Some(ref main_pb) = main_pb {
                        main_pb.set_length(all_symbols.len() as u64);
                    }

                    for sym in all_symbols {
                        let kind = match sym.kind {
                            symbols::SymbolKind::Function => "fn",
                            symbols::SymbolKind::Class => "class",
                        };

                        if let Err(err) = db.insert_symbol(
                            &sym.path,
                            sym.line,
                            kind,
                            &sym.name,
                            &sym.code,
                            &sym.ast_embedding,
                        ) {
                            if let Some(ref mp) = mp {
                                let _ = mp.println(format!(
                                    "warn: failed to insert symbol {}:{}: {}",
                                    sym.path.display(),
                                    sym.line,
                                    err
                                ));
                            } else {
                                eprintln!(
                                    "warn: failed to insert symbol {}:{}: {}",
                                    sym.path.display(),
                                    sym.line,
                                    err
                                );
                            }
                        }

                        if let Some(ref main_pb) = main_pb {
                            main_pb.inc(1);
                        }
                    }

                    if let Some(main_pb) = main_pb {
                        main_pb.finish_with_message("indexing complete");
                    }
                }
                Err(err) => {
                    eprintln!("error: {}", err);
                    std::process::exit(2);
                }
            }
        }
        Commands::Init {} => {
            // Resolve repo root
            let cwd = match std::env::current_dir() {
                Ok(dir) => dir,
                Err(err) => {
                    eprintln!("error: failed to read current directory: {}", err);
                    std::process::exit(2);
                }
            };
            let root = match index::find_git_root(&cwd) {
                Some(dir) => dir,
                None => {
                    eprintln!("error: not inside a git repository: {}", cwd.display());
                    std::process::exit(2);
                }
            };
            let cearch_dir = root.join(".cearch");
            if let Err(err) = std::fs::create_dir_all(&cearch_dir) {
                eprintln!("error: creating {}: {}", cearch_dir.display(), err);
                std::process::exit(2);
            }
            // Update .gitignore
            let gi = root.join(".gitignore");
            let entry = ".cearch/\n";
            let needs_append = match std::fs::read_to_string(&gi) {
                Ok(s) => !s.lines().any(|l| {
                    let t = l.trim();
                    t == ".cearch/" || t == ".cearch"
                }),
                Err(_) => true,
            };
            if needs_append {
                if let Err(err) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&gi)
                    .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()))
                {
                    eprintln!("warn: failed to update {}: {}", gi.display(), err);
                }
            }
        }
        Commands::Query { query, num_results } => {
            // Resolve repo root from current working directory
            let cwd = match std::env::current_dir() {
                Ok(dir) => dir,
                Err(err) => {
                    eprintln!("error: failed to read current directory: {}", err);
                    std::process::exit(2);
                }
            };
            let root = match index::find_git_root(&cwd) {
                Some(dir) => dir,
                None => {
                    eprintln!("error: not inside a git repository: {}", cwd.display());
                    std::process::exit(2);
                }
            };

            // Create AST embedder and try to embed the query as code
            let embedder = symbols::SymbolEmbedder::new();
            let embedding = match symbols::embed_query_snippet(&query, &embedder) {
                Ok(emb) => emb,
                Err(err) => {
                    eprintln!("error: failed to parse query as code: {}", err);
                    eprintln!("Try providing a valid code snippet in Rust or Python syntax.");
                    std::process::exit(2);
                }
            };

            // Open DB and perform KNN
            let db = match db::DB::open_read(&root) {
                Ok(db) => db,
                Err(err) => {
                    eprintln!("error: failed to open sqlite index: {}", err);
                    std::process::exit(2);
                }
            };

            match db.knn(&embedding, num_results) {
                Ok(results) => {
                    for (path, line, name, dist) in results {
                        let rel = path.strip_prefix(&root).unwrap_or(&path);
                        println!("{}:{} {} {:.3}", rel.display(), line, name, dist);
                    }
                }
                Err(err) => {
                    eprintln!("error: knn failed: {}", err);
                    std::process::exit(2);
                }
            }
        }
        Commands::Clean {} => {
            // Resolve repo root from current working directory
            let cwd = match std::env::current_dir() {
                Ok(dir) => dir,
                Err(err) => {
                    eprintln!("error: failed to read current directory: {}", err);
                    std::process::exit(2);
                }
            };
            let root = match index::find_git_root(&cwd) {
                Some(dir) => dir,
                None => {
                    eprintln!("error: not inside a git repository: {}", cwd.display());
                    std::process::exit(2);
                }
            };
            let cearch_dir = root.join(".cearch");
            if let Err(err) = std::fs::remove_dir_all(&cearch_dir) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("error: failed to delete .cearch directory: {}", err);
                    std::process::exit(2);
                }
            } else {
                // Remove .cearch entries from .gitignore if present
                let gi = root.join(".gitignore");
                if let Ok(contents) = std::fs::read_to_string(&gi) {
                    let filtered = contents
                        .lines()
                        .filter(|l| {
                            let t = l.trim();
                            !(t == ".cearch/" || t == ".cearch")
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Err(err) = std::fs::write(
                        &gi,
                        if filtered.is_empty() {
                            String::new()
                        } else {
                            format!("{}\n", filtered)
                        },
                    ) {
                        eprintln!("warn: failed to update {}: {}", gi.display(), err);
                    }
                }
                println!("cleaned: {}", cearch_dir.display());
            }
        }
    }
}
