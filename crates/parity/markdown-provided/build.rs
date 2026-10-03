//! Generates the provided-id parity schema (`../schema-provided`) against
//! the markdown backend under `IdStrategy::Provided`, which the shared
//! schema's stores (`SlugFromField`) cannot exercise. parity-seaorm-provided
//! generates it against SeaORM.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../schema-provided");

    ontogen::Pipeline::new("../schema-provided")
        .markdown_io(
            "src/persistence/markdown/generated",
            ontogen::MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: ontogen::MarkdownLayout::PerEntityDir,
                list_cap: 10_000,
                okf: ontogen::OkfOptions::default(),
            },
        )
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
        .store_id_strategy(ontogen::IdStrategy::Provided)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
