//! Public types for the ontogen-ts emitter.
//!
//! These define the API surface that ontogen (and any future consumer) talks
//! to. The phase-1 design pass for OF-015 pinned each of these shapes:
//!
//! - [`TypePath`] keys the type pool and root list — fully-qualified, one or
//!   more segments, never empty.
//! - [`EmitConfig`] gathers the knobs callers can tune per-build (external
//!   types, BigInt behavior, default case transform, strictness).
//! - [`EmitError`] enumerates every way emission can fail. There is no
//!   "warn-and-continue" tier — see OF-015 scope item 6.
//! - [`BigIntBehavior`] picks the TS rendering for 64-bit integers; the
//!   default mirrors the OF-014 spike (plain `number`).
//! - [`RenameAll`] enumerates the eight serde `rename_all` modes. PR 2
//!   implements the actual transforms; PR 1 just declares the shape.

use std::collections::BTreeMap;

/// Fully-qualified canonical path to a type in the user's crate (or an
/// external crate).
///
/// The pool walker normalizes `use` statements and `crate::` prefixes into
/// canonical form before constructing a `TypePath`, so two references to the
/// same item always produce the same key.
///
/// Invariant: `segments` is non-empty. Construct via [`TypePath::new`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypePath {
    segments: Vec<String>,
}

impl TypePath {
    /// Build a [`TypePath`] from one or more segments.
    ///
    /// Returns [`TypePathError::Empty`] if the segment list is empty.
    pub fn new(segments: Vec<String>) -> Result<Self, TypePathError> {
        if segments.is_empty() {
            return Err(TypePathError::Empty);
        }
        Ok(Self { segments })
    }

    /// Borrowed view of the path segments.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Last segment — the terminal ident of the type.
    pub fn terminal(&self) -> &str {
        // `segments` is guaranteed non-empty by the constructor.
        self.segments.last().expect("TypePath invariant: non-empty segments").as_str()
    }
}

impl std::fmt::Display for TypePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for segment in &self.segments {
            if !first {
                f.write_str("::")?;
            }
            f.write_str(segment)?;
            first = false;
        }
        Ok(())
    }
}

/// Construction error for [`TypePath`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypePathError {
    /// Caller supplied an empty segment list.
    Empty,
}

impl std::fmt::Display for TypePathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("TypePath must have at least one segment"),
        }
    }
}

impl std::error::Error for TypePathError {}

/// How `u64` / `i64` / `usize` / `isize` are rendered in TypeScript.
///
/// JavaScript `number` is a double-precision float; values above 2^53 lose
/// precision. Consumers who need to send arbitrarily large integers over the
/// wire pick [`BigIntBehavior::BigInt`] (TS `bigint` literal type) or
/// [`BigIntBehavior::String`] (string-serialized integers). Default is
/// [`BigIntBehavior::Number`] to match the OF-014 spike's effective behavior
/// and what most consumers expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BigIntBehavior {
    /// Render as TS `number`. Default. May silently truncate above 2^53.
    #[default]
    Number,
    /// Render as TS `bigint`. Requires the consumer to use `bigint` literals.
    BigInt,
    /// Render as TS `string`. Wire payload becomes a JSON string; consumers
    /// parse it themselves.
    String,
}

/// Serde's eight `rename_all` modes.
///
/// PR 1 declares the enum shape so [`EmitConfig::case_default`] type-checks;
/// PR 2 implements the actual case-transform table and property-tests it
/// against `serde_json::to_string`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameAll {
    /// `"lowercase"` — all-lower, no separator.
    Lowercase,
    /// `"UPPERCASE"` — all-upper, no separator.
    Uppercase,
    /// `"PascalCase"`.
    PascalCase,
    /// `"camelCase"`.
    CamelCase,
    /// `"snake_case"`.
    SnakeCase,
    /// `"SCREAMING_SNAKE_CASE"`.
    ScreamingSnakeCase,
    /// `"kebab-case"`.
    KebabCase,
    /// `"SCREAMING-KEBAB-CASE"`.
    ScreamingKebabCase,
}

/// Per-build configuration for an [`crate::emit`] call.
///
/// The full surface is in place for PR 1, but later PRs wire the individual
/// fields into the emission path:
///
/// - `external_types` is consumed by PR 3's use-resolution + external-types
///   lookup
/// - `bigint_behavior` is consumed by PR 1's `emit_type` for `u64`/`i64`
/// - `case_default` is consumed by PR 2's serde-rename engine
/// - `strict_unsupported` is documented but, per the OF-015 design pass, the
///   emitter is hard-error only — the field exists today to keep the
///   `EmitConfig` shape stable across the PR series, and the strict path is
///   the only path the emitter takes regardless of the flag's value. The
///   field will be removed entirely in a later PR if no consumer surfaces a
///   reason to keep it; see OF-015 scope item 6 for the rationale.
#[derive(Debug, Clone, Default)]
pub struct EmitConfig {
    /// Canonical-path → TS rendering map for types ontogen-ts treats as
    /// terminal. Defaults shipped by PR 3 (`chrono::DateTime` → `"string"`,
    /// etc.); user overrides merge on top.
    pub external_types: BTreeMap<String, String>,
    /// Rendering for 64-bit integer types. Defaults to
    /// [`BigIntBehavior::Number`].
    pub bigint_behavior: BigIntBehavior,
    /// Default `rename_all` mode applied to types that don't specify one.
    /// `None` means "respect each type's own annotation; emit fields as-is
    /// otherwise."
    pub case_default: Option<RenameAll>,
    /// Reserved for future use. Per the OF-015 design pass the emitter is
    /// hard-error only; this flag is currently a no-op and exists to keep
    /// the public shape stable across the PR series.
    pub strict_unsupported: bool,
}

/// Every way emission can fail.
///
/// Per the OF-015 design pass these are *hard errors only*: there is no
/// `FallbackRecord` placeholder, no warning-and-continue, no silent untyping.
/// Either a type emits cleanly or the build fails with one of these.
///
/// Errors collect into `Vec<EmitError>` at the [`crate::emit`] boundary so a
/// single build surfaces every problem rather than first-fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// The type's Rust shape isn't in phase-1's supported subset (tuple
    /// struct, unit struct, runtime-coordination wrapper like `Mutex<T>`,
    /// user-defined generic, etc.).
    UnsupportedShape {
        /// Path of the offending type.
        type_path: TypePath,
        /// Human-readable explanation.
        reason: String,
    },
    /// A `#[serde(...)]` attribute isn't supported in phase 1 (e.g.
    /// `rename(serialize = "...", deserialize = "...")`, or `tag`/`content`/
    /// `untagged`/`flatten` — see OF-015 phase 2).
    UnsupportedSerdeAttr {
        /// Path of the type carrying the attribute.
        type_path: TypePath,
        /// Name of the offending attribute (e.g. `"split-rename"`).
        attr: String,
    },
    /// A referenced ident couldn't be resolved against the type pool or the
    /// external-types table.
    UnresolvedReference {
        /// The unresolved name as it appeared in source.
        name: String,
        /// Path of the type that referenced it.
        referenced_by: TypePath,
    },
    /// Two reachable types render to the same TS name.
    NameCollision {
        /// The colliding TS name.
        name: String,
        /// All canonical paths that resolved to that name.
        paths: Vec<TypePath>,
    },
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedShape { type_path, reason } => {
                write!(f, "unsupported shape at `{type_path}`: {reason}")
            }
            Self::UnsupportedSerdeAttr { type_path, attr } => {
                write!(f, "unsupported serde attribute `{attr}` on `{type_path}`")
            }
            Self::UnresolvedReference { name, referenced_by } => {
                write!(f, "unresolved reference `{name}` (from `{referenced_by}`)")
            }
            Self::NameCollision { name, paths } => {
                write!(f, "TS name collision on `{name}` between ")?;
                let mut first = true;
                for path in paths {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "`{path}`")?;
                    first = false;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EmitError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_path_rejects_empty() {
        let err = TypePath::new(Vec::new()).expect_err("empty path should fail");
        assert_eq!(err, TypePathError::Empty);
    }

    #[test]
    fn type_path_accepts_single_segment() {
        let path = TypePath::new(vec!["Foo".to_string()]).expect("single segment is valid");
        assert_eq!(path.segments(), &["Foo".to_string()]);
        assert_eq!(path.terminal(), "Foo");
        assert_eq!(path.to_string(), "Foo");
    }

    #[test]
    fn type_path_accepts_multi_segment() {
        let path = TypePath::new(vec!["crate".to_string(), "models".to_string(), "Workout".to_string()])
            .expect("multi-segment is valid");
        assert_eq!(path.terminal(), "Workout");
        assert_eq!(path.to_string(), "crate::models::Workout");
    }

    #[test]
    fn bigint_behavior_default_is_number() {
        assert_eq!(BigIntBehavior::default(), BigIntBehavior::Number);
    }

    #[test]
    fn emit_config_default_is_empty_and_lax() {
        let config = EmitConfig::default();
        assert!(config.external_types.is_empty());
        assert_eq!(config.bigint_behavior, BigIntBehavior::Number);
        assert_eq!(config.case_default, None);
        assert!(!config.strict_unsupported);
    }

    #[test]
    fn emit_error_display_renders_reasonably() {
        let tp = TypePath::new(vec!["crate".to_string(), "Foo".to_string()]).unwrap();
        let err = EmitError::UnsupportedShape { type_path: tp.clone(), reason: "tuple struct".to_string() };
        assert_eq!(err.to_string(), "unsupported shape at `crate::Foo`: tuple struct");

        let err = EmitError::NameCollision {
            name: "Foo".to_string(),
            paths: vec![tp.clone(), TypePath::new(vec!["other".to_string(), "Foo".to_string()]).unwrap()],
        };
        assert_eq!(err.to_string(), "TS name collision on `Foo` between `crate::Foo`, `other::Foo`");
    }
}
