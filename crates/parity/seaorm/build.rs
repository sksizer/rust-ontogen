//! Generates the shared parity schema (`../schema`) against the SeaORM
//! backend. parity-markdown generates the same schema against the markdown
//! backend with the same default `IdStrategy` (`Fixed` and `Stamped`
//! override it in the schema); the generated code is committed so diffs are
//! reviewable.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../schema");

    ontogen::Pipeline::new("../schema")
        .seaorm("src/persistence/db/entities/generated", "src/persistence/db/conversions/generated")
        .dtos("src/schema/dto")
        .store("src/store/generated", Some::<std::path::PathBuf>("src/store/hooks".into()))
        .store_id_strategy(ontogen::IdStrategy::SlugFromField("title".into()))
        .build()
        .unwrap_or_else(|e| panic!("ontogen pipeline failed: {e}"));
}
