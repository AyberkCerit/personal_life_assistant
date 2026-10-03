//! The models PLA can download (FR-MDL-022): one recommended entry, verified 2026-10-03.

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

pub fn recommended() -> &'static CatalogEntry {
    &RECOMMENDED
}

pub fn by_id(id: &str) -> Option<&'static CatalogEntry> {
    (RECOMMENDED.id == id).then_some(&RECOMMENDED)
}

/// The catalogue entry whose file has this (lower-case) name.
pub fn by_file_name(name: &str) -> Option<&'static CatalogEntry> {
    (RECOMMENDED.file_name.eq_ignore_ascii_case(name)).then_some(&RECOMMENDED)
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
}
