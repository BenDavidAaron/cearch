use clap::{Parser, Subcommand};

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
        /// Path to the repository root to index
        path: String,
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
        Commands::Index { path: _, force: _ } => {
            todo!("implement index subcommand")
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
