use std::collections::HashMap;
use tree_sitter::Node;

/// Different strategies for encoding AST structures into vectors
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum EmbeddingStrategy {
    /// Frequency-based encoding of node types with structural features
    NodeTypeFrequency { vocab_size: usize },
    /// Path-based encoding using root-to-leaf paths
    PathBased { max_paths: usize, max_depth: usize },
    /// Combined approach using multiple features
    Combined { vocab_size: usize, max_paths: usize },
}

/// AST structural features extracted from a tree-sitter node
#[derive(Debug, Clone)]
pub struct AstStructure {
    /// Count of each node type in the subtree
    pub node_types: HashMap<String, u32>,
    /// Maximum depth from this node to any leaf
    pub max_depth: u32,
    /// Total number of nodes in subtree
    pub total_nodes: u32,
    /// Average branching factor
    pub avg_branching_factor: f32,
    /// All root-to-leaf paths (as sequences of node types)
    pub paths: Vec<Vec<String>>,
    /// Structural relationships (parent-child node type pairs)
    pub relationships: HashMap<(String, String), u32>,
}

impl AstStructure {
    /// Extract structural features from a tree-sitter AST node
    pub fn from_node(node: Node) -> Self {
        let mut node_types = HashMap::new();
        let mut relationships = HashMap::new();
        let mut paths = Vec::new();
        let mut max_depth = 0;
        let mut total_nodes = 0;
        let mut total_children = 0;

        // Breadth-first traversal to extract features
        Self::extract_features(
            node,
            0,
            &mut node_types,
            &mut relationships,
            &mut paths,
            &mut max_depth,
            &mut total_nodes,
            &mut total_children,
            Vec::new(),
        );

        let avg_branching_factor = if total_nodes > 0 {
            total_children as f32 / total_nodes as f32
        } else {
            0.0
        };

        AstStructure {
            node_types,
            max_depth,
            total_nodes,
            avg_branching_factor,
            paths,
            relationships,
        }
    }

    fn extract_features(
        node: Node,
        depth: u32,
        node_types: &mut HashMap<String, u32>,
        relationships: &mut HashMap<(String, String), u32>,
        paths: &mut Vec<Vec<String>>,
        max_depth: &mut u32,
        total_nodes: &mut u32,
        total_children: &mut u32,
        current_path: Vec<String>,
    ) {
        *max_depth = (*max_depth).max(depth);
        *total_nodes += 1;

        let node_type = node.kind().to_string();
        *node_types.entry(node_type.clone()).or_insert(0) += 1;

        let mut extended_path = current_path;
        extended_path.push(node_type.clone());

        let child_count = node.child_count();
        *total_children += child_count as u32;

        if child_count == 0 {
            // Leaf node - record the path
            paths.push(extended_path);
        } else {
            // Process children and record relationships
            for i in 0..child_count {
                if let Some(child) = node.child(i) {
                    let child_type = child.kind().to_string();
                    let relationship = (node_type.clone(), child_type);
                    *relationships.entry(relationship).or_insert(0) += 1;

                    Self::extract_features(
                        child,
                        depth + 1,
                        node_types,
                        relationships,
                        paths,
                        max_depth,
                        total_nodes,
                        total_children,
                        extended_path.clone(),
                    );
                }
            }
        }
    }
}

/// AST embedding generator
pub struct AstEmbedder {
    strategy: EmbeddingStrategy,
    /// Global vocabulary of node types (built from training data)
    node_vocabulary: HashMap<String, usize>,
}

impl AstEmbedder {
    pub fn new(strategy: EmbeddingStrategy) -> Self {
        Self {
            strategy,
            node_vocabulary: HashMap::new(),
        }
    }

    /// Build vocabulary from a collection of AST structures
    pub fn build_vocabulary(&mut self, structures: &[AstStructure]) {
        let mut type_counts: HashMap<String, u32> = HashMap::new();

        // Count all node types across all structures
        for structure in structures {
            for (node_type, count) in &structure.node_types {
                *type_counts.entry(node_type.clone()).or_insert(0) += count;
            }
        }

        // Sort by frequency and create vocabulary mapping
        let mut sorted_types: Vec<_> = type_counts.into_iter().collect();
        sorted_types.sort_by(|a, b| b.1.cmp(&a.1));

        self.node_vocabulary = sorted_types
            .into_iter()
            .enumerate()
            .map(|(idx, (node_type, _))| (node_type, idx))
            .collect();
    }

    /// Convert AST structure to fixed-length vector
    pub fn embed(&self, structure: &AstStructure) -> Vec<f32> {
        match &self.strategy {
            EmbeddingStrategy::NodeTypeFrequency { vocab_size } => {
                self.embed_node_frequency(structure, *vocab_size)
            }
            EmbeddingStrategy::PathBased {
                max_paths,
                max_depth,
            } => self.embed_path_based(structure, *max_paths, *max_depth),
            EmbeddingStrategy::Combined {
                vocab_size,
                max_paths,
            } => self.embed_combined(structure, *vocab_size, *max_paths),
        }
    }

    fn embed_node_frequency(&self, structure: &AstStructure, vocab_size: usize) -> Vec<f32> {
        let mut features = vec![0.0; vocab_size + 4]; // +4 for structural features

        // Node type frequencies (normalized)
        let total_count: u32 = structure.node_types.values().sum();
        if total_count > 0 {
            for (node_type, &count) in &structure.node_types {
                if let Some(&idx) = self.node_vocabulary.get(node_type) {
                    if idx < vocab_size {
                        features[idx] = count as f32 / total_count as f32;
                    }
                }
            }
        }

        // Structural features (normalized)
        features[vocab_size] = (structure.max_depth as f32).ln() / 5.0; // Log-normalized depth
        features[vocab_size + 1] = (structure.total_nodes as f32).ln() / 10.0; // Log-normalized size
        features[vocab_size + 2] = structure.avg_branching_factor / 5.0; // Normalized branching
        features[vocab_size + 3] = (structure.paths.len() as f32).ln() / 8.0; // Log-normalized path count

        features
    }

    fn embed_path_based(
        &self,
        structure: &AstStructure,
        max_paths: usize,
        max_depth: usize,
    ) -> Vec<f32> {
        let feature_size = max_paths * max_depth + 3; // +3 for structural features
        let mut features = vec![0.0; feature_size];

        // Encode paths as sequences of node type indices
        for (path_idx, path) in structure.paths.iter().take(max_paths).enumerate() {
            for (depth_idx, node_type) in path.iter().take(max_depth).enumerate() {
                if let Some(&vocab_idx) = self.node_vocabulary.get(node_type) {
                    let feature_idx = path_idx * max_depth + depth_idx;
                    if feature_idx < max_paths * max_depth {
                        // Use normalized vocabulary index
                        features[feature_idx] =
                            (vocab_idx as f32 + 1.0) / (self.node_vocabulary.len() as f32 + 1.0);
                    }
                }
            }
        }

        // Add structural summary features
        let base_idx = max_paths * max_depth;
        features[base_idx] = (structure.max_depth as f32) / 50.0;
        features[base_idx + 1] = (structure.total_nodes as f32).ln() / 10.0;
        features[base_idx + 2] = structure.avg_branching_factor / 5.0;

        features
    }

    fn embed_combined(
        &self,
        structure: &AstStructure,
        vocab_size: usize,
        max_paths: usize,
    ) -> Vec<f32> {
        let freq_features = self.embed_node_frequency(structure, vocab_size);
        let path_features = self.embed_path_based(structure, max_paths, 10); // Fixed max depth for paths

        // Relationship features (top parent-child pairs)
        let mut rel_features = vec![0.0; vocab_size]; // Same size as vocabulary
        let total_rels: u32 = structure.relationships.values().sum();
        if total_rels > 0 {
            for ((parent_type, child_type), &count) in &structure.relationships {
                if let (Some(&parent_idx), Some(&child_idx)) = (
                    self.node_vocabulary.get(parent_type),
                    self.node_vocabulary.get(child_type),
                ) {
                    // Combine parent and child indices into a single feature
                    let combined_idx = (parent_idx + child_idx) % vocab_size;
                    rel_features[combined_idx] += count as f32 / total_rels as f32;
                }
            }
        }

        // Concatenate all feature vectors
        let mut combined = freq_features;
        combined.extend(path_features);
        combined.extend(rel_features);
        combined
    }

    /// Get the dimension of vectors produced by this embedder
    pub fn dimension(&self) -> usize {
        match &self.strategy {
            EmbeddingStrategy::NodeTypeFrequency { vocab_size } => vocab_size + 4,
            EmbeddingStrategy::PathBased {
                max_paths,
                max_depth,
            } => max_paths * max_depth + 3,
            EmbeddingStrategy::Combined {
                vocab_size,
                max_paths,
            } => {
                // freq_features + path_features + rel_features
                (vocab_size + 4) + (max_paths * 10 + 3) + vocab_size
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn setup_parser() -> Parser {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        parser
    }

    #[test]
    fn test_ast_structure_extraction() {
        let mut parser = setup_parser();
        let source_code = "fn add(a: i32, b: i32) -> i32 { a + b }";
        let tree = parser.parse(source_code, None).unwrap();
        let root = tree.root_node();

        let structure = AstStructure::from_node(root);

        assert!(structure.total_nodes > 0);
        assert!(structure.max_depth > 0);
        assert!(!structure.node_types.is_empty());
        assert!(!structure.paths.is_empty());
    }

    #[test]
    fn test_embedder_dimensions() {
        let strategy = EmbeddingStrategy::NodeTypeFrequency { vocab_size: 100 };
        let embedder = AstEmbedder::new(strategy);
        assert_eq!(embedder.dimension(), 104); // 100 + 4 structural features

        let strategy = EmbeddingStrategy::PathBased {
            max_paths: 10,
            max_depth: 8,
        };
        let embedder = AstEmbedder::new(strategy);
        assert_eq!(embedder.dimension(), 83); // 10*8 + 3

        let strategy = EmbeddingStrategy::Combined {
            vocab_size: 50,
            max_paths: 5,
        };
        let embedder = AstEmbedder::new(strategy);
        assert_eq!(embedder.dimension(), 157); // (50+4) + (5*10+3) + 50
    }

    #[test]
    fn test_embedding_generation() {
        let mut parser = setup_parser();
        let source_code = "fn test() -> i32 { let x = 42; x }";
        let tree = parser.parse(source_code, None).unwrap();
        let structure = AstStructure::from_node(tree.root_node());

        let mut embedder =
            AstEmbedder::new(EmbeddingStrategy::NodeTypeFrequency { vocab_size: 50 });
        embedder.build_vocabulary(&[structure.clone()]);

        let embedding = embedder.embed(&structure);
        assert_eq!(embedding.len(), embedder.dimension());

        // Should have some non-zero features
        assert!(embedding.iter().any(|&x| x > 0.0));
    }
}
