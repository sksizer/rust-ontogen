//! A warning when the consumer's `ontogen-macros` is too old for the
//! argument markers its API source uses.
//!
//! `#[ontogen(query)]` compiles only when the fn-level macro removes it,
//! which `ontogen-macros` does from the version this `ontogen` was built
//! with. An older one passes the marker through and rustc fails with
//! `expected non-macro attribute, found attribute macro`, which names
//! neither the crate nor the fix. The build script runs before that
//! compile, so it reads the consumer's `Cargo.lock` and says which crate
//! to upgrade.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::parse::ApiModule;

/// The `ontogen-macros` version this `ontogen` was built with.
const MACROS_VERSION: &str = ontogen_macros::__version!();

static WARNED: AtomicBool = AtomicBool::new(false);

/// Print a `cargo:warning` once per build when an argument in `modules`
/// carries `#[ontogen(query)]` in its source and the `Cargo.lock` above
/// `CARGO_MANIFEST_DIR` resolves an `ontogen-macros` older than
/// [`MACROS_VERSION`]. Call it before the config-side `query_args` are
/// applied: those need no macro.
pub(crate) fn warn_if_stale(modules: &[ApiModule]) {
    let marked = modules.iter().any(|m| {
        m.functions.iter().flat_map(|f| &f.params).chain(m.events.iter().flat_map(|e| &e.params)).any(|p| p.in_query)
    });
    if !marked || WARNED.load(Ordering::Relaxed) {
        return;
    }
    let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") else { return };
    let Some(lock) = Path::new(&dir).ancestors().map(|d| d.join("Cargo.lock")).find(|p| p.is_file()) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&lock) else { return };
    if let Some(warning) = stale_warning(&text, MACROS_VERSION) {
        WARNED.store(true, Ordering::Relaxed);
        println!("cargo:warning={warning}");
    }
}

/// The warning for a `Cargo.lock` (`lock`) that resolves an
/// `ontogen-macros` older than `required`, or `None`.
pub(crate) fn stale_warning(lock: &str, required: &str) -> Option<String> {
    let need = numeric(required)?;
    let stale = lock
        .split("[[package]]")
        .filter(|block| block.lines().any(|l| l.trim() == "name = \"ontogen-macros\""))
        .filter_map(|block| block.lines().find_map(|l| l.trim().strip_prefix("version = \"")?.strip_suffix('"')))
        .find(|v| numeric(v).is_some_and(|have| have < need))?;
    Some(format!(
        "ontogen: `#[ontogen(query)]` needs ontogen-macros {required} or newer, but Cargo.lock resolves \
         ontogen-macros {stale}, which leaves the marker in place: rustc will fail with `expected non-macro \
         attribute, found attribute macro`. Set `ontogen-macros = \"={required}\"` beside your `ontogen` version"
    ))
}

/// `major.minor.patch` as numbers, ignoring any pre-release or build suffix.
fn numeric(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = "\
[[package]]
name = \"ontogen\"
version = \"0.11.0\"

[[package]]
name = \"ontogen-macros\"
version = \"0.1.1\"
dependencies = [
 \"syn\",
]
";

    #[test]
    fn an_older_macros_crate_is_named_with_the_version_to_set() {
        let warning = stale_warning(LOCK, "0.2.0").unwrap();
        assert!(warning.contains("ontogen-macros 0.1.1"), "{warning}");
        assert!(warning.contains("`ontogen-macros = \"=0.2.0\"`"), "{warning}");
        assert!(warning.contains("expected non-macro attribute, found attribute macro"), "{warning}");
    }

    #[test]
    fn the_same_or_a_newer_macros_crate_is_fine() {
        assert_eq!(stale_warning(LOCK, "0.1.1"), None);
        assert_eq!(stale_warning(LOCK, "0.1.0"), None);
        assert_eq!(stale_warning("[[package]]\nname = \"syn\"\nversion = \"0.0.1\"\n", "0.2.0"), None);
    }

    #[test]
    fn this_workspace_resolves_the_macros_version_it_was_built_with() {
        assert!(numeric(MACROS_VERSION).is_some(), "{MACROS_VERSION}");
        assert!(
            include_str!("../../Cargo.lock")
                .contains(&format!("name = \"ontogen-macros\"\nversion = \"{MACROS_VERSION}\""))
        );
        assert_eq!(stale_warning(include_str!("../../Cargo.lock"), MACROS_VERSION), None);
    }
}
