//! Install the ontogen admin layer into a downstream Nuxt application.
//!
//! This module performs a targeted, idempotent edit of a consumer's
//! `nuxt.config.ts` to add the admin-layer package to its `extends` array.
//! It does not generate Rust code — it modifies a TypeScript config file
//! to register the layer.
//!
//! For TypeScript admin-registry _generation_ (the data the layer consumes),
//! see [`crate::servers::generators::admin`] which is wired through the
//! servers pipeline.

use std::path::PathBuf;

use crate::CodegenError;
use crate::utils;

/// Configuration for [`install`].
///
/// Wires the bundled Nuxt admin layer into a downstream Nuxt application by
/// adding the layer path to the `extends` array in `nuxt.config.ts`. The
/// install is idempotent: re-running on an already-installed config is a no-op.
pub struct AdminLayerConfig {
    /// Path to the Nuxt app's `nuxt.config.ts`.
    pub nuxt_config: PathBuf,
    /// Relative path from the `nuxt.config.ts` to the admin layer package
    /// (e.g., `"../crates/ontogen/packages/nuxt_admin_layer"`).
    pub layer_path: String,
}

/// Install the ontogen admin layer into a Nuxt app.
///
/// Checks if the `extends` field in `nuxt.config.ts` already includes the
/// admin layer path. If not, adds it. This is idempotent — safe to call
/// from `build.rs` on every build.
///
/// Implementation note: this is a string-level edit, not a real TypeScript
/// parse. It recognizes two shapes — a top-level `extends: [...]` array, or
/// no `extends` at all (in which case one is inserted after the opening
/// `defineNuxtConfig({`). Other shapes (e.g. `extends` as a non-array, or
/// inside a spread) emit a `cargo:warning` and return `Ok(())` without
/// modification, leaving the manual fix to the user.
///
/// # Errors
///
/// Returns [`CodegenError::Client`] if `nuxt.config.ts` cannot be read or
/// written.
pub fn install(config: &AdminLayerConfig) -> Result<(), CodegenError> {
    let content = std::fs::read_to_string(&config.nuxt_config)
        .map_err(|e| CodegenError::Client(format!("Failed to read {}: {e}", config.nuxt_config.display())))?;

    // Already installed — nothing to do
    if content.contains(&config.layer_path) {
        return Ok(());
    }

    let new_content = if content.contains("extends:") || content.contains("extends :") {
        // extends exists but doesn't contain our layer — append to the array
        // Match `extends: [...]` and insert our path
        if let Some(bracket_pos) = content.find("extends:").and_then(|i| content[i..].find('[').map(|j| i + j)) {
            let mut result = String::with_capacity(content.len() + config.layer_path.len() + 10);
            result.push_str(&content[..bracket_pos + 1]);
            result.push_str(&format!("'{}', ", config.layer_path));
            result.push_str(&content[bracket_pos + 1..]);
            result
        } else {
            // extends exists but isn't an array — don't touch it, warn instead
            println!(
                "cargo:warning=ontogen: nuxt.config.ts has `extends` but not as an array — add '{}' manually",
                config.layer_path
            );
            return Ok(());
        }
    } else {
        // No extends field — add one after defineNuxtConfig({
        let insert_marker = "defineNuxtConfig({";
        if let Some(pos) = content.find(insert_marker) {
            let insert_at = pos + insert_marker.len();
            let mut result = String::with_capacity(content.len() + config.layer_path.len() + 30);
            result.push_str(&content[..insert_at]);
            result.push_str(&format!("\n  extends: ['{}'],", config.layer_path));
            result.push_str(&content[insert_at..]);
            result
        } else {
            println!(
                "cargo:warning=ontogen: could not find defineNuxtConfig({{ in nuxt.config.ts — add extends manually"
            );
            return Ok(());
        }
    };

    utils::write_if_changed(&config.nuxt_config, new_content.as_bytes())
        .map_err(|e| CodegenError::Client(format!("Failed to write {}: {e}", config.nuxt_config.display())))
}
