use anyhow::Result;
use directories::ProjectDirs;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

pub struct Embedder {
    model: TextEmbedding,
}

impl Embedder {
    pub fn new_default() -> Result<Self> {
        let cache_dir = default_cache_dir();
        let opts = TextInitOptions::default().with_cache_dir(cache_dir);
        let model = TextEmbedding::try_new(opts)?;
        Ok(Self { model })
    }

    pub fn with_model(model: EmbeddingModel) -> Result<Self> {
        let cache_dir = default_cache_dir();
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

fn default_cache_dir() -> std::path::PathBuf {
    // ProjectDirs uses reverse domain + app name to pick platform-appropriate directories
    // e.g., macOS: ~/Library/Caches/cearch/cearch, Linux: ~/.cache/cearch/cearch, Windows: %LOCALAPPDATA%\cearch\cearch\cache
    if let Some(dirs) = ProjectDirs::from("com", "cearch", "cearch") {
        let p = dirs.cache_dir();
        std::fs::create_dir_all(p).ok();
        p.to_path_buf()
    } else {
        // Fallback to XDG-like
        let base = std::env::var("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".cache"))
                    .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()))
            });
        let p = base.join("cearch");
        std::fs::create_dir_all(&p).ok();
        p
    }
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
