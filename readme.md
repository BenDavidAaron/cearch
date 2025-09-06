# Cearch

`cearch` (sea-urch) is a codebase search tool that parses the AST of your library, isolates all logic, and builds embeddings that you can query against.

It works by using treesitter to parse the logic of your programs, and then embeds each logical unit using an open embedding model for code. After that, all embeddings are stored for lookup in a HNSW index.

Interface

`cearch index`

Walks through all code files nested below the provided path, builds an AST with treesitter, and then extracts all logical unit's source code to embed. Each logical unit is embedded and a line number and file path are asaved in a local nearest neighbors datbase for retrieval.

`cearch query "def identity(x):" -n 7`

A query string is embedded and it's 7 nearest neighbors are returned.

`cearch clean`

The embedding model and index are deleted
