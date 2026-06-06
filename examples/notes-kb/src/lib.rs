//! notes-kb — an Obsidian-vault-shaped knowledge base on the markdown
//! backend: wikilinked notes, the generated API, and a graph view.

pub mod api;
pub mod persistence;
pub mod schema;
pub mod store;

use schema::AppError;
use store::Store;

pub struct AppState {
    store: Store,
}

impl AppState {
    pub fn new(vault: markdown_store::VaultHandle) -> Self {
        Self { store: Store::new(vault) }
    }

    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }
}

/// Tag extraction over the SAME vault files via the sibling rust-markdown
/// workspace's `markdown-vault` crate — the two-crate boundary demo:
/// markdown-store owns write/round-trip, markdown-vault owns read-only
/// extraction. Feature-gated because the path dependency needs the sibling
/// checkout (`cargo run --features vault-tags -- tags`).
#[cfg(feature = "vault-tags")]
pub fn vault_tags(root: &std::path::Path) -> Vec<String> {
    use markdown_vault::prelude::*;
    let policy = TagPolicy::obsidian();
    let opts = WalkOptions::default();
    let mut tags: Vec<String> = extract_tags_from_dir(root, &policy, &opts)
        .map(|by_file| by_file.into_values().flatten().map(|t| t.to_string()).collect())
        .unwrap_or_default();
    tags.sort();
    tags.dedup();
    tags
}
