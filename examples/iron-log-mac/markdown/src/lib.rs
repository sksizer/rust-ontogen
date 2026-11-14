//! The training log on a markdown vault, exported to Swift.
//!
//! Everything above the store is shared with `iron-log-sqlite`; this file holds
//! only what differs: how the backend opens.

pub mod api;
pub mod domain;
pub mod native;
pub mod persistence;
pub mod schema;
pub mod store;

use std::sync::{Arc, OnceLock};

use schema::{AppError, EntityChange};
use store::Store;

uniffi::setup_scaffolding!("iron_log_core");

impl From<markdown_store::Error> for AppError {
    fn from(e: markdown_store::Error) -> Self {
        AppError::Md(e.to_string())
    }
}

pub struct AppState {
    store: Store,
}

impl AppState {
    /// Open the vault under `data_dir`, creating nothing until the first write.
    pub async fn open(data_dir: &str) -> Result<Self, AppError> {
        let root = std::path::Path::new(data_dir).join("vault");
        Ok(Self { store: Store::new(persistence::markdown::generated::open_vault(root)) })
    }

    /// Accessor the generated methods call.
    pub async fn store(&self) -> Result<&Store, AppError> {
        Ok(&self.store)
    }

    /// The store's change channel, for the `entity_changes` feed.
    pub fn changes(&self) -> tokio::sync::broadcast::Receiver<EntityChange> {
        self.store.subscribe()
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("start the tokio runtime"))
}

/// Open the log under `data_dir`.
#[uniffi::export]
pub fn open_iron_log(data_dir: String) -> Result<Arc<native::NativeApi>, native::ApiError> {
    let state = runtime().block_on(AppState::open(&data_dir)).map_err(native::ontogen_app_error)?;
    Ok(native::NativeApi::new(Arc::new(state), runtime().handle().clone()))
}

/// Which backend this build of the core stores in.
#[uniffi::export]
pub fn backend_name() -> String {
    "Markdown".to_string()
}
