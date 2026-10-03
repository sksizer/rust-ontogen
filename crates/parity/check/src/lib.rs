//! The runtime parity test of ADR 0006 §8, in `tests/`. The shared schema
//! in `crates/parity/schema` is generated against SQLite (parity-seaorm)
//! and against a markdown vault (parity-markdown); `crates/parity/
//! schema-provided` likewise under `IdStrategy::Provided`. The test loads
//! identical records into both and asserts identical results. A divergence
//! is a bug in a backend, not in the test.
