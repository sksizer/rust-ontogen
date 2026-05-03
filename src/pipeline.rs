//! Fluent builder API on top of the generator functions.
//!
//! `Pipeline` is opt-in sugar: it wires together `parse_schema`, `gen_seaorm`,
//! `gen_markdown_io`, `gen_dtos`, `gen_store`, `gen_api`, and `gen_servers` with
//! sensible defaults so simple `build.rs` files don't have to spell out every
//! config struct field.
//!
//! The existing config structs and generator functions remain the canonical
//! API — `Pipeline` is a thin wrapper that constructs them under the hood.
//!
//! # Example
//!
//! ```no_run
//! ontogen::Pipeline::new("src/schema")
//!     .seaorm("src/persistence/entities", "src/persistence/conversions")
//!     .store("src/store/generated", Some("src/store/hooks"))
//!     .api("src/api/v1/generated", "AppState")
//!     .build()
//!     .expect("ontogen pipeline failed");
//! ```
//!
//! # Stage order
//!
//! Method call order on the builder is irrelevant. `build()` always runs stages
//! in the dependency order: `schema → seaorm/markdown_io/dtos → store → api → servers`.
//! Each stage's structured output (e.g., `SeaOrmOutput`, `ApiOutput`) is threaded
//! through to the next stage automatically.
//!
//! # Defaults
//!
//! - `schema_module_path` defaults to `"crate::schema"` (matches `StoreConfig`
//!   and `ApiConfig` defaults).
//! - `api(state_type)` is required — there's no universal default for the
//!   AppState type, so the builder asks for it explicitly.
//! - `store_type` (used by `api` and `servers`) defaults to `Some("Store")` once
//!   the store stage is enabled, otherwise `None`.
//! - All optional stages (seaorm, markdown_io, dtos, store, api, servers) are
//!   skipped unless their builder method is called.

use std::path::PathBuf;

use crate::ir::{ApiOutput, SchemaOutput, SeaOrmOutput};
use crate::{
    ApiConfig, CodegenError, DEFAULT_SCHEMA_MODULE_PATH, DtoConfig, MarkdownIoConfig, SchemaConfig, SeaOrmConfig,
    ServersConfig, StoreConfig, gen_api, gen_dtos, gen_markdown_io, gen_seaorm, gen_servers, gen_store, parse_schema,
};

/// Default store type name used for the `api` and `servers` stages once a
/// store stage has been registered.
const DEFAULT_STORE_TYPE: &str = "Store";

// ── Per-stage staged config ─────────────────────────────────────────

/// Internal state for the SeaORM stage.
struct SeaOrmStage {
    entity_output: PathBuf,
    conversion_output: PathBuf,
    skip_conversions: Vec<String>,
}

/// Internal state for the markdown-io stage.
struct MarkdownIoStage {
    output_dir: PathBuf,
}

/// Internal state for the standalone DTO stage.
struct DtoStage {
    output_dir: PathBuf,
}

/// Internal state for the store stage.
struct StoreStage {
    output_dir: PathBuf,
    hooks_dir: Option<PathBuf>,
}

/// Internal state for the API stage.
struct ApiStage {
    output_dir: PathBuf,
    state_type: String,
    exclude: Vec<String>,
    scan_dirs: Vec<PathBuf>,
    store_type: Option<String>,
}

/// Internal state for the servers stage.
struct ServersStage {
    config: ServersConfig,
    scan_dirs: Vec<PathBuf>,
}

// ── Builder ─────────────────────────────────────────────────────────

/// Fluent builder over the ontogen generator pipeline.
///
/// Construct with [`Pipeline::new`], opt into the stages you want via the
/// per-stage methods, then run with [`Pipeline::build`].
///
/// See the [module docs](self) for the full pipeline shape and defaults.
pub struct Pipeline {
    schema_dir: PathBuf,
    schema_module_path: String,

    seaorm: Option<SeaOrmStage>,
    markdown_io: Option<MarkdownIoStage>,
    dtos: Option<DtoStage>,
    store: Option<StoreStage>,
    api: Option<ApiStage>,
    servers: Option<ServersStage>,
}

impl Pipeline {
    /// Start a new pipeline rooted at `schema_dir`.
    ///
    /// `schema_dir` is the only universally-required input — every pipeline
    /// begins with `parse_schema`. All other stages are opt-in.
    pub fn new(schema_dir: impl Into<PathBuf>) -> Self {
        Self {
            schema_dir: schema_dir.into(),
            schema_module_path: DEFAULT_SCHEMA_MODULE_PATH.to_string(),
            seaorm: None,
            markdown_io: None,
            dtos: None,
            store: None,
            api: None,
            servers: None,
        }
    }

    /// Override the schema module path used by the `store` and `api` stages.
    ///
    /// Defaults to `"crate::schema"`.
    #[must_use]
    pub fn schema_module_path(mut self, path: impl Into<String>) -> Self {
        self.schema_module_path = path.into();
        self
    }

    // ── seaorm ──────────────────────────────────────────────────────

    /// Enable the SeaORM persistence stage.
    ///
    /// Generates entity types into `entity_output` and `From<…>`/`Into<…>`
    /// conversions into `conversion_output`. Use [`Pipeline::seaorm_skip_conversions`]
    /// to opt specific entities out of conversion generation.
    #[must_use]
    pub fn seaorm(mut self, entity_output: impl Into<PathBuf>, conversion_output: impl Into<PathBuf>) -> Self {
        self.seaorm = Some(SeaOrmStage {
            entity_output: entity_output.into(),
            conversion_output: conversion_output.into(),
            skip_conversions: Vec::new(),
        });
        self
    }

    /// Set the list of entity names to skip in SeaORM conversion generation.
    ///
    /// Has no effect unless [`Pipeline::seaorm`] has been called.
    #[must_use]
    pub fn seaorm_skip_conversions(mut self, skip: Vec<String>) -> Self {
        if let Some(stage) = self.seaorm.as_mut() {
            stage.skip_conversions = skip;
        }
        self
    }

    // ── markdown_io ─────────────────────────────────────────────────

    /// Enable markdown I/O generation (parser dispatch, writers, fs ops).
    #[must_use]
    pub fn markdown_io(mut self, output_dir: impl Into<PathBuf>) -> Self {
        self.markdown_io = Some(MarkdownIoStage { output_dir: output_dir.into() });
        self
    }

    // ── dtos ────────────────────────────────────────────────────────

    /// Enable standalone Create/Update DTO generation.
    ///
    /// Note: `gen_store` already produces DTOs internally — only enable this
    /// stage if you want DTOs without a full store layer.
    #[must_use]
    pub fn dtos(mut self, output_dir: impl Into<PathBuf>) -> Self {
        self.dtos = Some(DtoStage { output_dir: output_dir.into() });
        self
    }

    // ── store ───────────────────────────────────────────────────────

    /// Enable the store stage.
    ///
    /// `output_dir` receives the generated CRUD modules. `hooks_dir`, when
    /// `Some`, scaffolds per-entity hook files (created once, never overwritten).
    #[must_use]
    pub fn store<P>(mut self, output_dir: impl Into<PathBuf>, hooks_dir: Option<P>) -> Self
    where
        P: Into<PathBuf>,
    {
        self.store = Some(StoreStage { output_dir: output_dir.into(), hooks_dir: hooks_dir.map(Into::into) });
        self
    }

    // ── api ─────────────────────────────────────────────────────────

    /// Enable the API stage.
    ///
    /// `output_dir` receives generated CRUD forwarders. `state_type` is the
    /// name of your application state type (e.g., `"AppState"`); there is no
    /// universal default, so it must be supplied.
    ///
    /// Use [`Pipeline::api_exclude`], [`Pipeline::api_scan_dirs`], and
    /// [`Pipeline::api_store_type`] for less-common knobs.
    #[must_use]
    pub fn api(mut self, output_dir: impl Into<PathBuf>, state_type: impl Into<String>) -> Self {
        self.api = Some(ApiStage {
            output_dir: output_dir.into(),
            state_type: state_type.into(),
            exclude: Vec::new(),
            scan_dirs: Vec::new(),
            store_type: None,
        });
        self
    }

    /// Exclude entities by name from API generation.
    ///
    /// Has no effect unless [`Pipeline::api`] has been called.
    #[must_use]
    pub fn api_exclude(mut self, exclude: Vec<String>) -> Self {
        if let Some(stage) = self.api.as_mut() {
            stage.exclude = exclude;
        }
        self
    }

    /// Add directories to scan for hand-written API modules.
    ///
    /// Scanned modules are merged with generated CRUD modules into a unified
    /// `ApiOutput`. Has no effect unless [`Pipeline::api`] has been called.
    #[must_use]
    pub fn api_scan_dirs(mut self, scan_dirs: Vec<PathBuf>) -> Self {
        if let Some(stage) = self.api.as_mut() {
            stage.scan_dirs = scan_dirs;
        }
        self
    }

    /// Override the Store type name used by the API stage.
    ///
    /// When a `store` stage is registered and this override is not set, the
    /// builder defaults to `Some("Store")`. Pass `None` to explicitly disable.
    /// Has no effect unless [`Pipeline::api`] has been called.
    #[must_use]
    pub fn api_store_type(mut self, store_type: Option<String>) -> Self {
        if let Some(stage) = self.api.as_mut() {
            stage.store_type = store_type;
        }
        self
    }

    // ── servers ─────────────────────────────────────────────────────

    /// Enable the servers stage with a fully-formed `ServersConfig`.
    ///
    /// The servers config has a much wider surface than the other stages
    /// (transport choice, naming overrides, client generators, route prefixes,
    /// etc.), so the builder accepts it as-is rather than re-modelling each
    /// knob. Use [`Pipeline::servers_scan_dirs`] to set the scan dirs passed
    /// to `gen_servers`; defaults to `[]`.
    #[must_use]
    pub fn servers(mut self, config: ServersConfig) -> Self {
        self.servers = Some(ServersStage { config, scan_dirs: Vec::new() });
        self
    }

    /// Override the source directories scanned by `gen_servers` when no
    /// `ApiOutput` is available.
    ///
    /// When the api stage is enabled, its `ApiOutput` is used and these scan
    /// dirs are ignored. Has no effect unless [`Pipeline::servers`] has been
    /// called.
    #[must_use]
    pub fn servers_scan_dirs(mut self, scan_dirs: Vec<PathBuf>) -> Self {
        if let Some(stage) = self.servers.as_mut() {
            stage.scan_dirs = scan_dirs;
        }
        self
    }

    // ── execution ───────────────────────────────────────────────────

    /// Execute the pipeline.
    ///
    /// Stages run in the canonical order regardless of method call order:
    /// `schema → seaorm → markdown_io → dtos → store → api → servers`.
    /// Returns on the first error, with the originating stage's variant of
    /// [`CodegenError`].
    pub fn build(self) -> Result<(), CodegenError> {
        // Stage 1: parse schema (always)
        let schema: SchemaOutput = parse_schema(&SchemaConfig { schema_dir: self.schema_dir.clone() })?;

        // Stage 2a: SeaORM
        let seaorm_out: Option<SeaOrmOutput> = match self.seaorm {
            Some(stage) => Some(gen_seaorm(
                &schema.entities,
                &SeaOrmConfig {
                    entity_output: stage.entity_output,
                    conversion_output: stage.conversion_output,
                    skip_conversions: stage.skip_conversions,
                },
            )?),
            None => None,
        };

        // Stage 2b: markdown I/O (independent of seaorm/store)
        if let Some(stage) = self.markdown_io {
            gen_markdown_io(&schema.entities, &MarkdownIoConfig { output_dir: stage.output_dir })?;
        }

        // Stage 2c: standalone DTOs (independent of store)
        if let Some(stage) = self.dtos {
            gen_dtos(&schema.entities, &DtoConfig { output_dir: stage.output_dir })?;
        }

        // Stage 3: store
        let store_enabled = self.store.is_some();
        if let Some(stage) = self.store {
            gen_store(
                &schema.entities,
                seaorm_out.as_ref(),
                &StoreConfig {
                    output_dir: stage.output_dir,
                    hooks_dir: stage.hooks_dir,
                    schema_module_path: self.schema_module_path.clone(),
                },
            )?;
        }

        // Stage 4: API (depends on schema; consumes nothing structured upstream)
        let api_out: Option<ApiOutput> = match self.api {
            Some(stage) => {
                // If store stage was registered and the user didn't override store_type,
                // default to Some("Store") so generated API can call store methods.
                let resolved_store_type = match (store_enabled, stage.store_type) {
                    (_, Some(explicit)) => Some(explicit),
                    (true, None) => Some(DEFAULT_STORE_TYPE.to_string()),
                    (false, None) => None,
                };

                Some(gen_api(
                    &schema.entities,
                    &ApiConfig {
                        output_dir: stage.output_dir,
                        exclude: stage.exclude,
                        scan_dirs: stage.scan_dirs,
                        state_type: stage.state_type,
                        store_type: resolved_store_type,
                        schema_module_path: self.schema_module_path.clone(),
                    },
                )?)
            }
            None => None,
        };

        // Stage 5: servers
        if let Some(stage) = self.servers {
            // Auto-forward parsed entities to the admin-registry generator,
            // unless the caller has already set them explicitly. Without this,
            // admin-registry.ts ships with empty `fields: []` for every entity.
            let mut servers_config = stage.config;
            if servers_config.schema_entities.is_empty() {
                servers_config.schema_entities = schema.entities.clone();
            }
            gen_servers(api_out.as_ref(), &stage.scan_dirs, &servers_config)?;
        }

        Ok(())
    }
}
