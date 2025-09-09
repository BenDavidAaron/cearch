use clap::{Parser, Subcommand};
mod db;
mod embed;
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
                    // Initialize embedder up-front (may download/cold-start); avoid drawing bars during this
                    let mut embedder = match embed::Embedder::new_default() {
                        Ok(e) => e,
                        Err(err) => {
                            eprintln!("error: failed to init embedder: {}", err);
                            std::process::exit(2);
                        }
                    };

                    // Open DB with model dimension; AllMiniLML6V2 is 384 dims
                    let db = match db::DB::open_with_dim(&root, 384) {
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

                    // Process each file: parse symbols, embed in chunks with a per-file bar, then insert
                    for f in files {
                        let symbols_in_file = match symbols::enumerate_symbols_in_file(&f) {
                            Ok(v) => v,
                            Err(err) => {
                                if let Some(ref mp) = mp {
                                    let _ = mp.println(format!(
                                        "warn: failed to parse {}: {}",
                                        f.display(),
                                        err
                                    ));
                                } else {
                                    eprintln!("warn: failed to parse {}: {}", f.display(), err);
                                }
                                if let Some(ref main_pb) = main_pb {
                                    main_pb.inc(1);
                                }
                                continue;
                            }
                        };

                        if symbols_in_file.is_empty() {
                            if let Some(ref main_pb) = main_pb {
                                main_pb.inc(1);
                            }
                            continue;
                        }

                        // Optional per-file bar
                        let file_pb = if let Some(ref mp) = mp {
                            let pb = mp.add(ProgressBar::new(symbols_in_file.len() as u64));
                            if let Ok(style) = ProgressStyle::with_template(
                                "  ↳ {spinner:.green} {pos}/{len} [{bar.white/black}] {per_sec} {msg}",
                            ) {
                                pb.set_style(style.progress_chars("=> "));
                            }
                            if let Some(name) = f.file_name().and_then(|s| s.to_str()) {
                                pb.set_message(name.to_string());
                            }
                            Some(pb)
                        } else {
                            None
                        };

                        // Embed in small batches to report progress without interfering with main bar
                        let batch_size: usize = 64;
                        let mut idx = 0usize;
                        while idx < symbols_in_file.len() {
                            let end = usize::min(idx + batch_size, symbols_in_file.len());
                            let chunk = &symbols_in_file[idx..end];
                            let codes = chunk.iter().map(|s| s.code.as_str());
                            let embeddings_chunk = match embedder.embed(codes) {
                                Ok(v) => v,
                                Err(err) => {
                                    if let Some(ref mp) = mp {
                                        let _ = mp.println(format!(
                                            "warn: failed to embed symbols for {}: {}",
                                            f.display(),
                                            err
                                        ));
                                    } else {
                                        eprintln!(
                                            "warn: failed to embed symbols for {}: {}",
                                            f.display(),
                                            err
                                        );
                                    }
                                    break;
                                }
                            };

                            for (sym, emb) in chunk.iter().zip(embeddings_chunk.into_iter()) {
                                let kind = match sym.kind {
                                    symbols::SymbolKind::Function => "fn",
                                    symbols::SymbolKind::Class => "class",
                                };
                                if let Err(err) = db.insert_symbol(
                                    &sym.path, sym.line, kind, &sym.name, &sym.code, &emb,
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
                            }

                            if let Some(ref file_pb) = file_pb {
                                file_pb.inc((end - idx) as u64);
                            }
                            idx = end;
                        }

                        if let Some(file_pb) = file_pb {
                            file_pb.finish_and_clear();
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
            // Pre-download default model into cache (Embedder uses .cearch)
            match embed::Embedder::new_default() {
                Ok(_) => println!("initialized: {}", cearch_dir.display()),
                Err(err) => {
                    eprintln!("error: failed to initialize model cache: {}", err);
                    std::process::exit(2);
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

            // Embed the query string
            let mut embedder = match embed::Embedder::new_default() {
                Ok(e) => e,
                Err(err) => {
                    eprintln!("error: failed to init embedder: {}", err);
                    std::process::exit(2);
                }
            };
            let embedding = match embedder.embed([query.as_str()]) {
                Ok(mut v) => {
                    if v.is_empty() {
                        eprintln!("error: empty embedding");
                        std::process::exit(2);
                    }
                    v.remove(0)
                }
                Err(err) => {
                    eprintln!("error: failed to embed query: {}", err);
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
