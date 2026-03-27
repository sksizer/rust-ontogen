# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-03-25

### Added

- Schema parsing from `#[ontology(...)]` annotations via `syn`
- SeaORM entity, junction table, and conversion generation
- Markdown I/O generation (parser dispatch, writers, filesystem operations)
- Create/Update DTO generation
- Store layer generation with CRUD methods, update structs, and lifecycle hook scaffolding
- API layer generation with CRUD forwarding and scan-and-merge for custom endpoints
- Server transport generation for Axum HTTP, Tauri IPC, and MCP
- Client generation for TypeScript and admin registries
- Typed intermediate representations (`SchemaOutput`, `SeaOrmOutput`, `StoreOutput`, `ApiOutput`, `ServersOutput`)
- Independent generator functions that can run standalone or be chained
- `ontogen-macros` proc-macro crate for `#[derive(OntologyEntity)]` attribute passthrough
