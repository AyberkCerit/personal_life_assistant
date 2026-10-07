//! The models PLA can download (FR-MDL-022): the language model (verified 2026-10-03) and the
//! embedding model for semantic memory (verified 2026-10-07).

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub file_name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub url: &'static str,
    pub source: &'static str,
    pub licence: &'static str,
    pub licence_url: &'static str,
}

/// Same file as evaluated in F1 (size and SHA-256 checked against research/f1-model-eval).
static RECOMMENDED: CatalogEntry = CatalogEntry {
    id: "gemma-4-e2b-it-q3km",
    name: "Gemma 4 E2B (Q3_K_M)",
    file_name: "gemma-4-E2B-it-Q3_K_M.gguf",
    size: 2_536_786_016,
    sha256: "086e2f5ba85057f8f19712e3160a644728f74f323c9feeac4cd73fab11b43085",
    url: "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/0314792d7f1f7e229411f620751375812bb9faf2/gemma-4-E2B-it-Q3_K_M.gguf",
    source: "Hugging Face · unsloth",
    licence: "Apache-2.0",
    licence_url: "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF",
};

/// FR-MEM-004 (owner decision 2026-10-07): EmbeddingGemma 300M, run by the bundled llama-server
/// in embedding mode. Size and SHA-256 from the Hugging Face API at the pinned revision.
static EMBEDDING: CatalogEntry = CatalogEntry {
    id: "embeddinggemma-300m-q8",
    name: "EmbeddingGemma 300M (Q8_0)",
    file_name: "embeddinggemma-300M-Q8_0.gguf",
    size: 333_590_944,
    sha256: "b5ce9d77a3fc4b3b39ccb5643c36777911cc4eb46a66962eadfa3f5f60490d63",
    url: "https://huggingface.co/ggml-org/embeddinggemma-300M-GGUF/resolve/0f741b5a6585bd53aeb15cd1372c56f2a0f65e12/embeddinggemma-300M-Q8_0.gguf",
    source: "Hugging Face · ggml-org",
    licence: "Gemma Terms of Use",
    licence_url: "https://ai.google.dev/gemma/terms",
};

pub fn recommended() -> &'static CatalogEntry {
    &RECOMMENDED
}

/// The semantic memory's model (FR-MEM-004).
pub fn embedding() -> &'static CatalogEntry {
    &EMBEDDING
}

fn all() -> [&'static CatalogEntry; 2] {
    [&RECOMMENDED, &EMBEDDING]
}

pub fn by_id(id: &str) -> Option<&'static CatalogEntry> {
    all().into_iter().find(|e| e.id == id)
}

/// The catalogue entry whose file has this (lower-case) name.
pub fn by_file_name(name: &str) -> Option<&'static CatalogEntry> {
    all().into_iter().find(|e| e.file_name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::policy::{HuggingFace, UrlPolicy};

    #[test]
    fn recommends_gemma_4_e2b_q3_k_m() {
        // FR-MDL-022
        let m = recommended();
        assert_eq!(m.file_name, "gemma-4-E2B-it-Q3_K_M.gguf");
        assert_eq!(m.size, 2_536_786_016);
        assert_eq!(m.sha256.len(), 64);
        assert!(m.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(m.url.contains("/resolve/0314792d7f1f7e229411f620751375812bb9faf2/"), "pinned revision");
        assert_eq!(HuggingFace.allows(m.url), Ok(()));
        assert_eq!(by_id(m.id), Some(m));
        assert_eq!(by_id("other"), None);
    }

    #[test]
    fn the_embedding_model_is_pinned_and_verified() {
        // FR-MEM-004, owner decision 2026-10-07
        let m = embedding();
        assert_eq!((m.file_name, m.size), ("embeddinggemma-300M-Q8_0.gguf", 333_590_944));
        assert_eq!(m.sha256.len(), 64);
        assert!(m.url.contains("/resolve/0f741b5a6585bd53aeb15cd1372c56f2a0f65e12/"), "pinned revision");
        assert_eq!(HuggingFace.allows(m.url), Ok(()));
        assert_eq!(by_id(m.id), Some(m));
        assert_eq!(by_file_name("EMBEDDINGGEMMA-300M-Q8_0.GGUF"), Some(m));
    }
}
