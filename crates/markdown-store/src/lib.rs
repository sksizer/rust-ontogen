//! markdown-store — typed, lossless YAML-frontmatter markdown storage.
//!
//! Treat a folder of `---`-fenced markdown files as a datastore: parse a
//! file into a typed value plus its body, mutate it, and write it back
//! atomically — **without destroying anything a human added by hand**. The
//! crate is the runtime layer a generated (or hand-written) CRUD store sits
//! on; it knows nothing about any particular schema.
//!
//! Four ideas, four modules:
//!
//! - [`frontmatter`] — the [`Document`] round-trip model: an
//!   order-preserving YAML mapping + verbatim body. Typed reads/writes via
//!   serde; unknown keys and key order survive read-modify-write.
//! - [`wikilink`] — `[[id]]` encode/strip/parse, Obsidian-compatible, so
//!   relation fields double as a navigable graph.
//! - [`layout`] / [`id`] — ids are filename stems; path construction
//!   validates them (no traversal, ever) and [`IdStrategy`] derives them
//!   for new records.
//! - [`fsops`] / [`walk`] / [`store`] *(default features)* — atomic
//!   single-file writes (same-dir tempfile + fsync + rename),
//!   gitignore-aware sorted listing, and [`VaultHandle`]: the per-vault
//!   façade with create/read/modify/remove/list plus an intra-process
//!   write lock.
//!
//! # Quick start
//!
//! ```
//! use markdown_store::{Document, IdStrategy, VaultHandle, VaultLayout};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Task {
//!     title: String,
//!     status: String,
//!     #[serde(skip_serializing_if = "Option::is_none")]
//!     epic: Option<String>,
//! }
//!
//! let dir = tempfile::tempdir().unwrap();
//! let vault = VaultHandle::new(
//!     dir.path(),
//!     VaultLayout::PerEntityDir,
//!     IdStrategy::SlugFromField("title".into()),
//! );
//!
//! // Create: derive an id from the title, wikilink the epic reference.
//! let task = Task {
//!     title: "Ship the parser".into(),
//!     status: "open".into(),
//!     epic: Some(markdown_store::wikilink::encode("E0042")),
//! };
//! let id = vault.make_record_id("tasks", None, Some(&task.title))?;
//! let mut doc = Document::new();
//! doc.merge_serialize(&task, &["title", "status", "epic"])?;
//! doc.set_body("Parser notes go here.\n");
//! vault.create_record("tasks", &id, &doc)?;
//!
//! // Read it back, typed; strip the wikilink at the boundary.
//! let doc = vault.read_record("tasks", &id)?;
//! let read: Task = doc.deserialize()?;
//! assert_eq!(markdown_store::wikilink::strip_opt(read.epic), Some("E0042".into()));
//!
//! // A human edit (unknown key) survives the next typed update.
//! vault.modify_record("tasks", &id, |doc| {
//!     doc.set("hand_added", "still here");
//!     Ok(())
//! })?;
//! vault.modify_record("tasks", &id, |doc| {
//!     let mut t: Task = doc.deserialize()?;
//!     t.status = "closed".into();
//!     doc.merge_serialize(&t, &["title", "status", "epic"])
//! })?;
//! let final_doc = vault.read_record("tasks", &id)?;
//! assert_eq!(final_doc.get("hand_added").and_then(|v| v.as_str()), Some("still here"));
//! # Ok::<(), markdown_store::Error>(())
//! ```
//!
//! # Guarantees and limits
//!
//! - **Single-record atomicity**: a write is a same-directory tempfile +
//!   fsync + rename; readers see old-or-new, never torn. Multi-record
//!   transactions are deliberately not offered.
//! - **Stable list order**: lexicographic by filename (= by id).
//! - **Single-process stance**: clones of a [`VaultHandle`] share a write
//!   lock; concurrent writers in *other* processes are out of scope.
//! - **Lossless round-trip of meaning, not bytes**: unknown keys, key
//!   order, and the body survive; YAML cosmetics (quoting style, list
//!   layout) are normalized to the emitter's deterministic output.
//! - **Scale ceiling**: listing parses every record; the configurable
//!   [`store::DEFAULT_LIST_CAP`] makes overgrowth a loud error. This crate
//!   is for small-N, human-editable, read-heavy data — not a database.
//!
//! # Destiny note
//!
//! This crate is the write/round-trip complement to
//! [`markdown-vault`](https://github.com/sksizer/rust-markdown) (read-only
//! tag extraction) and is expected to migrate into that workspace; the
//! fence-splitting contract is kept byte-compatible with it on purpose, and
//! tag extraction is deliberately *not* duplicated here.

mod error;
pub mod frontmatter;
pub mod id;
pub mod layout;
pub mod wikilink;

#[cfg(feature = "fsops")]
pub mod fsops;
#[cfg(feature = "walk")]
pub mod walk;

#[cfg(feature = "store")]
pub mod store;

pub use crate::{error::Error, frontmatter::Document, id::IdStrategy, layout::VaultLayout};

#[cfg(feature = "walk")]
pub use crate::walk::WalkOptions;

#[cfg(feature = "store")]
pub use crate::store::VaultHandle;

/// Bulk import of the common surface:
/// `use markdown_store::prelude::*;`
///
/// Rule of thumb: applications use the prelude; libraries (and generated
/// code) spell out module paths (`markdown_store::wikilink::strip`) so call
/// sites stay greppable.
pub mod prelude {
    pub use crate::{frontmatter::Document, id::IdStrategy, layout::VaultLayout, wikilink, Error};

    #[cfg(feature = "walk")]
    pub use crate::walk::WalkOptions;

    #[cfg(feature = "store")]
    pub use crate::store::VaultHandle;
}
