# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.4.0] - 2026-08-08

### ⚠ BREAKING CHANGES

- **`biome_fmt` and the `biome` feature are removed.** TypeScript formatting is
  now a caller-supplied hook: `TsFormatter::{None, Custom, Command}` in
  `utils.rs`, with `TsFormatter::custom` / `custom_with` as constructors and
  `OnFormatError` controlling what happens when a hook fails. `None` is the
  default, so unconfigured callers get unformatted output.

  *(Landed as `fix(formatter)!: replace in-process biome with a consumer
  formatter hook`; omitted from the generated changelog and recorded here after
  the fact.)*

### Added

- path-aware TS format hook with configurable error policy **(breaking)**



## [0.3.0] - 2026-08-05

### Added

- in-process biome TypeScript formatter, feature-gated ([#120](https://github.com/sksizer/rust-ontogen/pull/120))
  — **removed again in 0.4.0**; see that entry.



## [0.2.0] - 2026-07-14

### Added

- emit axum 0.8 brace-style route params


