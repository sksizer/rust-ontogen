# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-04-07

### Added

- implement ontogen build-time code generator for ontology-driven applications
- add iron-log example project demonstrating full ontogen pipeline
- add nuxt admin layer and per-field registry generation
- restore as full project from template-tauri-nuxt
- add i64, bool, and option variants to field type handling
- add junction operations, naming improvements, and scan-mode fixes
- client generators in public API, transport import fixes
- cruet integration and entity-first naming convention
- query params threading and first-class pagination

### Changed

- extract shared types and utilities into ontogen-core crate
- use write-if-changed pattern and update schema for new entity model
- format generated files in memory before writing
- extract shared types to @ontogen/admin-types and remove project-scoping

### Fixed

- add full template-tauri-nuxt project structure to iron-log example
- resolve CI formatting and clippy failures
- resolve prettier config lookup and clean up generated output
- generate unscoped handlers for store-based modules without route_prefix
- resolve clippy warnings from newer toolchain
- junction naming consistency across transports


