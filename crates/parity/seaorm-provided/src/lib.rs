//! The provided-id parity schema's SeaORM store. Hand-written here: the
//! consumer contract the generated code needs (`AppError`, `Store`, the
//! event types) and the table setup ontogen does not emit. Everything under
//! `generated/` and `dto/` is build.rs output.

pub mod persistence;
pub mod schema;
pub mod store;

pub use store::Store;
