# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-07

### ⚠ BREAKING CHANGES

- send a required GET argument as a query parameter with #[ontogen(query)] (#219)
  - three public structs gain a public field. ParamMeta in ontogen-core gains in_query, the servers Param gains in_query, and the servers NamingConfig gains query_args. Code that builds one of them with a struct literal listing every field must add the field or end the literal with ..Default::default(). An API module that uses the new #[ontogen(query)] argument marker needs the matching ontogen-macros, which strips the marker, and the matching ontogen-jsonapi, which adds

### Added

- send a required GET argument as a query parameter with #[ontogen(query)] (#219) **(breaking)**



## [0.1.1] - 2026-08-08

### Added

- #[ontogen::http::get] override, and fix POST dropping optional params


