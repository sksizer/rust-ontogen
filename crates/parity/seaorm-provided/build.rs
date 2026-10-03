//! Generates the provided-id parity schema (`../schema-provided`) against
//! the SeaORM backend under `IdStrategy::Provided`, which the shared
//! schema's stores (`SlugFromField`) cannot exercise. parity-markdown-provided
//! generates it against markdown.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../schema-provided");

    ontogen::Pipeline::new("../schema-provided")
        .seaorm("src/persistence/db/entities/generated", "src/persistence/db/conversions/generated")
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
        .store_id_strategy(ontogen::IdStrategy::Provided)
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
