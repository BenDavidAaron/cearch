use anyhow::{Result, anyhow};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

pub struct Embedder {
    model: TextEmbedding,
}

impl Embedder {
    pub fn new_default() -> Result<Self> {
        let cache_dir = repo_cearch_dir()?;
        let opts = TextInitOptions::default().with_cache_dir(cache_dir);
        let model = TextEmbedding::try_new(opts)?;
        Ok(Self { model })
    }

    pub fn with_model(model: EmbeddingModel) -> Result<Self> {
        let cache_dir = repo_cearch_dir()?;
        let options: TextInitOptions = TextInitOptions::new(model).with_cache_dir(cache_dir);
        let model = TextEmbedding::try_new(options)?;
        Ok(Self { model })
    }

    pub fn embed<'a, T: AsRef<str> + 'a>(
        &mut self,
        snippets: impl IntoIterator<Item = T>,
    ) -> Result<Vec<Vec<f32>>> {
        let texts: Vec<String> = snippets
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let embs = self.model.embed(texts, None)?;
        Ok(embs)
    }
}

fn repo_cearch_dir() -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;
    let root = crate::index::find_git_root(&cwd)
        .ok_or_else(|| anyhow!("not inside a git repository: {}", cwd.display()))?;
    let dir = root.join(".cearch");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_initialize_default_model() {
        let result = Embedder::new_default();
        assert!(result.is_ok());
    }

    #[test]
    fn can_embed_simple_snippets() {
        let mut embedder = Embedder::new_default().expect("init model");
        let snippets = vec![
            "fn add(a: i32, b: i32) -> i32 { a + b }",
            "def add(a, b):\n    return a + b\n",
        ];
        let embeddings = embedder.embed(&snippets).expect("embed");
        assert_eq!(embeddings.len(), snippets.len());
        for vector in embeddings {
            assert!(!vector.is_empty());
        }
    }
}
