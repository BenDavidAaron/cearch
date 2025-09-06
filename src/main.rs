use clap::{Parser, Subcommand};
mod db;
mod embed;
mod index;
mod symbols;

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
    /// Query the index with a code snippet or description
    Query {
        /// The query string
        query: String,
        /// Number of results to return
        #[arg(short = 'n', long, default_value_t = 7)]
        num_results: usize,
    },
    /// Clean the index and embeddings for a repository
    Clean {
        /// Path to the repository whose index should be cleaned
        path: String,
    },
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
                    let mut all_symbols = Vec::new();
                    for f in files {
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

                    let db = match db::DB::open(&root) {
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
        Commands::Query {
            query: _,
            num_results: _,
        } => {
            todo!("implement query subcommand")
        }
        Commands::Clean { path: _ } => {
            todo!("implement clean subcommand")
        }
    }
}
