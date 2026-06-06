//! Runs the full markdown pipeline at build time: schema → markdown_io →
//! dtos → store → api. The backend is inferred (markdown is the only
//! persistence stage). Generated code is written into src/ and committed,
//! iron-log-style, so diffs are reviewable.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/schema/note.rs");
    println!("cargo:rerun-if-changed=src/schema/task.rs");
    println!("cargo:rerun-if-changed=src/schema/tag.rs");

    ontogen::Pipeline::new("src/schema")
        .markdown_io(
            "src/persistence/markdown/generated",
            ontogen::MarkdownIoOptions {
                vault_root: "data/vault".into(),
                layout: ontogen::MarkdownLayout::PerEntityDir,
                id_strategy: ontogen::IdStrategy::SlugFromField("title".into()),
                list_cap: 10_000,
            },
        )
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
        .api("src/api/generated", "AppState")
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
