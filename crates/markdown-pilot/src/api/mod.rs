//! The API layer: generated CRUD forwarders in `generated/`, hand-written
//! modules beside them. Every entity's hand-written module re-exports its
//! generated fns, so one module path reaches both.

pub mod board;
pub mod bookmark;
pub mod generated;
pub mod note;
pub mod outline;
pub mod section;
pub mod tag;
pub mod task;
pub mod transport;
