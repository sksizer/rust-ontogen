//! Embedded schema fixtures for ontogen tests.
//!
//! These files are read as text by `parse_schema_dir` (via `syn::parse_file`).
//! They are not compiled as part of the crate — they live under `tests/fixtures/`
//! purely to give tests a stable, in-tree schema to parse.

mod exercise;
mod tag;
mod workout;
mod workout_set;

pub use exercise::Exercise;
pub use tag::Tag;
pub use workout::Workout;
pub use workout_set::WorkoutSet;
