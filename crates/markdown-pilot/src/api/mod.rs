//! The API layer: generated CRUD forwarders in `generated/`, hand-written
//! modules beside them. A hand-written module named after an entity
//! re-exports that entity's generated fns, so it shadows the glob below.

pub mod bookmark;
pub mod generated;
pub mod task;
pub mod transport;
pub use generated::*;
