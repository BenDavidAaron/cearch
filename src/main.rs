use clap::{Parser, Subcommand};
mod index;

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
                    for f in files {
                        println!("{}", f.display());
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
