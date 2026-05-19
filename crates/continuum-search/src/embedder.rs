//! The embedding model wrapper.
//!
//! Uses model2vec static embeddings: a distilled token→vector table with mean
//! pooling. Pure Rust, no ONNX runtime, instant inference — it builds on any
//! toolchain and adds no native-library dependency.

use anyhow::Result;
use model2vec_rs::model::StaticModel;

/// HuggingFace repo of the static embedding model (~30 MB, downloaded once).
const MODEL_REPO: &str = "minishlab/potion-base-8M";

/// Loaded embedding model. Cheap to call; clone-free, shared behind an `Arc`.
pub struct Embedder {
    model: StaticModel,
}

impl Embedder {
    /// Load the model, downloading it from HuggingFace on first use.
    pub fn load() -> Result<Embedder> {
        let model = StaticModel::from_pretrained(MODEL_REPO, None, Some(true), None)?;
        Ok(Embedder { model })
    }

    /// Embed a batch of texts. Output vectors are L2-normalized by the model.
    pub fn embed(&self, texts: &[String]) -> Vec<Vec<f32>> {
        if texts.is_empty() {
            return Vec::new();
        }
        self.model.encode(texts)
    }

    /// Embed a single text.
    pub fn embed_one(&self, text: &str) -> Vec<f32> {
        self.embed(std::slice::from_ref(&text.to_string()))
            .into_iter()
            .next()
            .unwrap_or_default()
    }
}
