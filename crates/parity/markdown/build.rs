//! Generates the shared parity schema (`../schema`) against the markdown
//! backend. parity-seaorm generates the same schema against SeaORM with the
//! same default `IdStrategy` (`Fixed` overrides it in the schema); the
//! generated code is committed so diffs are reviewable.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../schema");

    ontogen::Pipeline::new("../schema")
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
        .store_id_strategy(ontogen::IdStrategy::SlugFromField("title".into()))
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
