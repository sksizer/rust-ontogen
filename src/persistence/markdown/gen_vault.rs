//! Generate `VAULT_ROOT` and `open_vault`, the one constructor for the
//! consumer's `markdown_store::VaultHandle`, built from the same
//! configuration the rest of the markdown output comes from. Consumers call
//! it instead of repeating the layout, list cap and OKF options by hand. The
//! id strategy is not vault configuration: the generated store passes it to
//! each create (`StoreConfig::id_strategy`). Both items go into the output
//! directory's `mod.rs`, beside the per-entity module declarations, so no
//! entity's module name can collide with them.

use crate::MarkdownIoConfig;
use crate::ir::MarkdownLayout;

/// Generate the `VAULT_ROOT` and `open_vault` items for `config`.
pub fn generate_open_vault(config: &MarkdownIoConfig) -> String {
    let layout = match config.layout {
        MarkdownLayout::PerEntityDir => "PerEntityDir",
        MarkdownLayout::Flat => "Flat",
    };

    let mut code = String::with_capacity(1024);
    code.push_str(
        "/// Where the vault's records live, as configured at build time. A relative\n\
         /// path resolves against the working directory the program runs from, not\n\
         /// the crate root.\n",
    );
    code.push_str(&format!("pub const VAULT_ROOT: &str = {:?};\n\n", config.vault_root.to_string_lossy()));
    code.push_str(
        "/// Open the vault at `root` with the layout, list cap and OKF options\n\
         /// configured at build time. Pass [`VAULT_ROOT`] to use the configured\n\
         /// location, or any other directory (a test's tempdir, say).\n",
    );
    code.push_str("pub fn open_vault(root: impl Into<std::path::PathBuf>) -> markdown_store::VaultHandle {\n");
    code.push_str(&format!("    markdown_store::VaultHandle::new(root, markdown_store::VaultLayout::{layout})\n"));
    code.push_str(&format!("        .with_list_cap({})\n", config.list_cap));
    // Only what differs from the runtime's default policy is spelled out, so
    // a vault with both options off reads as the plain handle it is.
    let mut policy = Vec::new();
    if config.okf.index {
        policy.push("index: true".to_string());
    }
    if let Some(actor) = &config.okf.generated_by {
        policy.push(format!("generated_by: Some({actor:?}.into())"));
    }
    if !policy.is_empty() {
        code.push_str("        .with_okf(markdown_store::OkfPolicy {\n");
        for field in policy {
            code.push_str(&format!("            {field},\n"));
        }
        code.push_str("            ..Default::default()\n        })\n");
    }
    code.push_str("}\n");
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::OkfOptions;

    fn config(okf: OkfOptions) -> MarkdownIoConfig {
        MarkdownIoConfig {
            output_dir: "unused".into(),
            vault_root: "data/vault".into(),
            layout: MarkdownLayout::PerEntityDir,
            list_cap: 10_000,
            okf,
        }
    }

    #[test]
    fn open_vault_applies_the_configuration() {
        let code = generate_open_vault(&config(OkfOptions::default()));
        assert!(code.contains("pub const VAULT_ROOT: &str = \"data/vault\";"), "{code}");
        assert!(
            code.contains("markdown_store::VaultHandle::new(root, markdown_store::VaultLayout::PerEntityDir)\n"),
            "{code}"
        );
        assert!(code.contains(".with_list_cap(10000)\n}"), "{code}");
        assert!(!code.contains("with_okf"), "the default policy is not spelled out: {code}");
        syn::parse_file(&code).expect("valid Rust");
    }

    #[test]
    fn okf_options_thread_into_open_vault() {
        let code =
            generate_open_vault(&config(OkfOptions { index: true, generated_by: Some("notes-kb/0.1.0".into()) }));
        assert!(
            code.contains(
                ".with_okf(markdown_store::OkfPolicy {\n            index: true,\n            \
                 generated_by: Some(\"notes-kb/0.1.0\".into()),\n            ..Default::default()\n        })\n}"
            ),
            "{code}"
        );
        syn::parse_file(&code).expect("valid Rust");

        let code = generate_open_vault(&config(OkfOptions { index: true, generated_by: None }));
        assert!(code.contains("OkfPolicy {\n            index: true,\n            ..Default::default()"), "{code}");
        assert!(!code.contains("generated_by"), "no stamping unless configured: {code}");
    }

    #[test]
    fn every_layout_maps_to_its_runtime_variant() {
        let mut flat = config(OkfOptions::default());
        flat.layout = MarkdownLayout::Flat;
        let code = generate_open_vault(&flat);
        assert!(code.contains("markdown_store::VaultHandle::new(root, markdown_store::VaultLayout::Flat)\n"), "{code}");
        assert!(!code.contains("IdStrategy"), "the id strategy is the store's, not the vault's: {code}");
    }
}
