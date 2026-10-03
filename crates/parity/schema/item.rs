use ontogen_macros::OntologyEntity;
use serde::{Deserialize, Serialize};

/// A string enum. Both backends store the serialized name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Alpha,
    Beta,
    Gamma,
}

/// One field of every type ADR 0006 §2 lists as sortable (and so has to
/// hold the same value on both backends), a self-referential
/// `belongs_to`/`has_many` pair over an `Option` foreign key, and a
/// `many_to_many` to `Tag`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, OntologyEntity)]
#[ontology(entity, directory = "items", table = "items")]
pub struct Item {
    #[ontology(id)]
    pub id: String,

    pub title: String,
    pub int32: i32,
    pub int64: i64,
    pub float32: f32,
    pub float64: f64,
    pub flag: bool,
    #[ontology(enum_field)]
    pub kind: Kind,

    pub maybe_text: Option<String>,
    pub maybe_int32: Option<i32>,
    pub maybe_int64: Option<i64>,
    pub maybe_float32: Option<f32>,
    pub maybe_float64: Option<f64>,
    pub maybe_flag: Option<bool>,
    #[ontology(enum_field)]
    pub maybe_kind: Option<Kind>,

    // Integer primitives the parser files under `Other` / `OptionEnum`.
    pub n_u8: u8,
    pub n_u16: u16,
    pub n_u32: u32,
    pub n_usize: usize,
    pub n_u128: u128,
    pub n_i8: i8,
    pub n_i16: i16,
    pub n_isize: isize,
    pub n_i128: i128,
    pub maybe_u32: Option<u32>,

    #[ontology(relation(belongs_to, target = "Item"))]
    pub parent_id: Option<String>,

    #[ontology(relation(has_many, target = "Item", foreign_key = "parent_id"))]
    pub children: Vec<String>,

    #[ontology(relation(many_to_many, target = "Tag"))]
    pub tags: Vec<String>,

    #[ontology(body)]
    pub body: String,
}
