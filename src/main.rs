use clap::{Parser, Subcommand};
mod db;
mod embed;
mod index;
mod symbols;

use indicatif::{ProgressBar, ProgressStyle};

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
        Commands::Index { force: _ } => {
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
                    let progress_bar = ProgressBar::new(files.len() as u64);
                    let progress_style = ProgressStyle::default_bar().template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} {msg}");
                    match progress_style {
                        Ok(style) => progress_bar.set_style(style),
                        Err(err) => eprintln!("error: failed to set progress style: {}", err),
                    }
                    let mut all_symbols = Vec::new();
                    for f in files {
                        progress_bar.inc(1);
                        match symbols::enumerate_symbols_in_file(&f) {
                            Ok(mut symbols) => all_symbols.append(&mut symbols),
                            Err(err) => eprintln!("warn: failed to parse {}: {}", f.display(), err),
                        }
                    }

                    let mut embedder = match embed::Embedder::new_default() {
                        Ok(e) => e,
                        Err(err) => {
                            eprintln!("error: failed to init embedder: {}", err);
                            std::process::exit(2);
                        }
                    };

                    let embeddings =
                        match embedder.embed(all_symbols.iter().map(|s| s.code.as_str())) {
                            Ok(v) => v,
                            Err(err) => {
                                eprintln!("error: failed to embed: {}", err);
                                std::process::exit(2);
                            }
                        };

                    // Open with model dimension; AllMiniLML6V2 is 384 dims
                    let db = match db::DB::open_with_dim(&root, 384) {
                        Ok(db) => db,
                        Err(err) => {
                            eprintln!("error: failed to open sqlite index: {}", err);
                            std::process::exit(2);
                        }
                    };

                    for (sym, emb) in all_symbols.into_iter().zip(embeddings.into_iter()) {
                        let kind = match sym.kind {
                            symbols::SymbolKind::Function => "fn",
                            symbols::SymbolKind::Class => "class",
                        };
                        if let Err(err) =
                            db.insert_symbol(&sym.path, sym.line, kind, &sym.name, &sym.code, &emb)
                        {
                            eprintln!(
                                "warn: failed to insert symbol {}:{}: {}",
                                sym.path.display(),
                                sym.line,
                                err
                            );
                        }
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
