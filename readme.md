# Cearch

`cearch` (sea-urch) is a codebase search tool that parses the code files in your git repos, isolates the logical units using treesitter, embeds them, and then allows you to do nearest neighbor lookups of your code to detect duplicated and similar logic in your codebase.

![Sea Urch Logo](sea-urch.png)

## Usage

1. prepare your repo by running `cearch init` from any path in your git repo, this will:
   - add `.cearch/` to your .gitignore
   - download an embedding model and cache it in `.cearch/`
2. index your repo by running `cearch index`
3. search indexed symbols by using `cearch query -n $NUM_HITS`
4. delete your saved index and cached embedding models with `cearch clean`

## Development

1. Clone the repo using git
1. `cargo build`
1. Make commits
1. Submit a PR

## TODOs

- Embedding model selection
- Bring your own embedding models
- Graph embed AST of logical Units?
- Support for JavaScript
- output in `.json` or `.yaml`
- refactor per-repository operations into a mod
- clean up interfaces
- write tests
