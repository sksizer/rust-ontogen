//! A warning when the consumer's `ontogen-macros` is too old for the
//! argument markers its API source uses.
//!
//! `#[ontogen(query)]` compiles only when the fn-level macro removes it,
//! which `ontogen-macros` does from the version this `ontogen` was built
//! with. An older one passes the marker through and rustc fails with
//! `expected non-macro attribute, found attribute macro`, which names
//! neither the crate nor the fix. The build script runs before that
//! compile, so it reads the lock cargo uses and says which crate to
//! upgrade.
//!
//! It only warns when it is sure: the lock is the workspace root's, the
//! building package is in it, and the package's own `ontogen-macros`
//! dependency resolves to one version. Anything else stays silent.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::parse::ApiModule;

/// The `ontogen-macros` version this `ontogen` was built with.
const MACROS_VERSION: &str = ontogen_macros::__version!();

/// The newest published `ontogen-macros` that strips no argument marker.
/// A crates.io copy at or below it fails whatever `ontogen` pins, which
/// covers an `ontogen` taken from git or a path before the release that
/// bumps the macros.
const LAST_NON_STRIPPING: (u64, u64, u64) = (0, 1, 1);

/// The `source` cargo writes for a crates.io package.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

static WARNED: AtomicBool = AtomicBool::new(false);

/// Print a `cargo:warning` once per build when an argument in `modules`
/// carries `#[ontogen(query)]` in its source and the building package's
/// own `ontogen-macros` cannot strip it (see [`stale_warning`]). Call it
/// before the config-side `query_args` are applied: those need no macro.
pub(crate) fn warn_if_stale(modules: &[ApiModule]) {
    let marked = modules.iter().any(|m| {
        m.functions.iter().flat_map(|f| &f.params).chain(m.events.iter().flat_map(|e| &e.params)).any(|p| p.in_query)
    });
    if !marked || WARNED.load(Ordering::Relaxed) {
        return;
    }
    let (Some(dir), Ok(name), Ok(version)) =
        (std::env::var_os("CARGO_MANIFEST_DIR"), std::env::var("CARGO_PKG_NAME"), std::env::var("CARGO_PKG_VERSION"))
    else {
        return;
    };
    let Some(lock) = lock_path(Path::new(&dir)) else { return };
    let Ok(text) = std::fs::read_to_string(&lock) else { return };
    if let Some(warning) = stale_warning(&text, &name, &version, MACROS_VERSION) {
        WARNED.store(true, Ordering::Relaxed);
        println!("cargo:warning={warning}");
    }
}

/// The `Cargo.lock` cargo uses for the package at `manifest_dir`: the one
/// beside the nearest manifest, its own included, that declares
/// `[workspace]`, or its own when none does. `None` where that is not
/// certain: a package that names its root with `package.workspace`, or a
/// root that excludes members.
pub(crate) fn lock_path(manifest_dir: &Path) -> Option<PathBuf> {
    let own = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).ok()?;
    if declares_workspace(&own) {
        return Some(manifest_dir.join("Cargo.lock"));
    }
    if own.lines().any(|l| l.trim_start().starts_with("workspace")) {
        return None;
    }
    for dir in manifest_dir.ancestors().skip(1) {
        let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml")) else { continue };
        if declares_workspace(&manifest) {
            return (!manifest.lines().any(|l| l.trim_start().starts_with("exclude"))).then(|| dir.join("Cargo.lock"));
        }
    }
    Some(manifest_dir.join("Cargo.lock"))
}

fn declares_workspace(manifest: &str) -> bool {
    manifest.lines().any(|l| l.trim() == "[workspace]")
}

/// The warning for `lock` when the package `name` `version` depends on an
/// `ontogen-macros` that cannot strip argument markers: one older than
/// `required`, or a crates.io copy no newer than [`LAST_NON_STRIPPING`].
/// `None` when it can, or when the lock does not say for certain.
pub(crate) fn stale_warning(lock: &str, name: &str, version: &str, required: &str) -> Option<String> {
    let need = numeric(required)?;
    let packages = packages(lock);
    let own: Vec<&LockPackage> = packages.iter().filter(|p| p.name == name && p.version == version).collect();
    let [own] = own[..] else { return None };
    let deps: Vec<&str> =
        own.dependencies.iter().map(String::as_str).filter(|d| d.split(' ').next() == Some("ontogen-macros")).collect();
    let [dep] = deps[..] else { return None };
    let macros = resolve(&packages, dep)?;
    let have = numeric(&macros.version)?;
    let published_old = macros.source.as_deref() == Some(CRATES_IO) && have <= LAST_NON_STRIPPING;
    if have >= need && !published_old {
        return None;
    }
    let found = if published_old {
        format!("the crates.io ontogen-macros {}", macros.version)
    } else {
        format!("ontogen-macros {}", macros.version)
    };
    Some(format!(
        "ontogen: `#[ontogen(query)]` needs ontogen-macros {required} or newer, but `{name}` depends on {found}, \
         which leaves the marker in place: rustc will fail with `expected non-macro attribute, found attribute \
         macro`. Use the ontogen-macros your ontogen pins ({required}, from the same source as ontogen)"
    ))
}

/// One `[[package]]` of a `Cargo.lock`.
#[derive(Debug, Default)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    dependencies: Vec<String>,
}

fn packages(lock: &str) -> Vec<LockPackage> {
    let quoted = |l: &str, key: &str| {
        l.strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(String::from)
    };
    lock.split("[[package]]")
        .skip(1)
        .map(|block| {
            let mut p = LockPackage::default();
            let mut in_deps = false;
            for line in block.lines().map(str::trim) {
                if in_deps {
                    match line.strip_prefix('"').and_then(|l| l.trim_end_matches(',').strip_suffix('"')) {
                        Some(dep) => p.dependencies.push(dep.to_string()),
                        None => in_deps = false,
                    }
                } else if let Some(v) = quoted(line, "name") {
                    p.name = v;
                } else if let Some(v) = quoted(line, "version") {
                    p.version = v;
                } else if let Some(v) = quoted(line, "source") {
                    p.source = Some(v);
                } else if line.starts_with("dependencies = [") {
                    in_deps = !line.ends_with(']');
                }
            }
            p
        })
        .collect()
}

/// The package a dependency entry names: `name`, `name version` or
/// `name version (source)`, and only when exactly one package fits.
fn resolve<'a>(packages: &'a [LockPackage], dep: &str) -> Option<&'a LockPackage> {
    let mut parts = dep.splitn(3, ' ');
    let name = parts.next()?;
    let version = parts.next();
    let source = parts.next().and_then(|s| s.strip_prefix('(')?.strip_suffix(')'));
    let fits: Vec<&LockPackage> = packages
        .iter()
        .filter(|p| p.name == name)
        .filter(|p| version.is_none_or(|v| p.version == v))
        .filter(|p| source.is_none_or(|s| p.source.as_deref() == Some(s)))
        .collect();
    match fits[..] {
        [one] => Some(one),
        _ => None,
    }
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

    /// A workspace lock: member `a` on the macros under test (a path
    /// dependency, no `source`), member `b` pinning the published 0.1.1.
    const WORKSPACE_LOCK: &str = "\
version = 4

[[package]]
name = \"a\"
version = \"0.1.0\"
dependencies = [
 \"ontogen\",
 \"ontogen-macros 0.2.0\",
]

[[package]]
name = \"b\"
version = \"0.1.0\"
dependencies = [
 \"ontogen-macros 0.1.1\",
]

[[package]]
name = \"ontogen\"
version = \"0.11.0\"
dependencies = [
 \"ontogen-macros 0.2.0\",
]

[[package]]
name = \"ontogen-macros\"
version = \"0.1.1\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
checksum = \"0000\"
dependencies = [
 \"syn\",
]

[[package]]
name = \"ontogen-macros\"
version = \"0.2.0\"
dependencies = [
 \"quote\",
 \"syn\",
]
";

    #[test]
    fn a_member_on_the_new_macros_is_not_warned_about_another_members_old_pin() {
        assert_eq!(stale_warning(WORKSPACE_LOCK, "a", "0.1.0", "0.2.0"), None);
    }

    #[test]
    fn the_member_that_pins_the_old_macros_is_warned() {
        let warning = stale_warning(WORKSPACE_LOCK, "b", "0.1.0", "0.2.0").unwrap();
        assert!(warning.contains("`b` depends on the crates.io ontogen-macros 0.1.1"), "{warning}");
        assert!(warning.contains("expected non-macro attribute, found attribute macro"), "{warning}");
    }

    /// Before the release that bumps the macros, `ontogen` from git pins a
    /// path or git 0.1.1 that strips markers; the crates.io 0.1.1 does not.
    #[test]
    fn the_published_non_stripping_macros_is_warned_at_the_same_version() {
        let lock = "\
[[package]]
name = \"a\"
version = \"0.1.0\"
dependencies = [
 \"ontogen-macros 0.1.1 (registry+https://github.com/rust-lang/crates.io-index)\",
]

[[package]]
name = \"ontogen-macros\"
version = \"0.1.1\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"ontogen-macros\"
version = \"0.1.1\"
source = \"git+https://github.com/sksizer/rust-ontogen?rev=abc#abc\"
";
        let warning = stale_warning(lock, "a", "0.1.0", "0.1.1").unwrap();
        assert!(warning.contains("depends on the crates.io ontogen-macros 0.1.1"), "{warning}");
        let git = lock.replace(
            "0.1.1 (registry+https://github.com/rust-lang/crates.io-index)",
            "0.1.1 (git+https://github.com/sksizer/rust-ontogen?rev=abc#abc)",
        );
        assert_eq!(stale_warning(&git, "a", "0.1.0", "0.1.1"), None);
    }

    #[test]
    fn an_uncertain_lock_is_silent() {
        // The package is not in the lock, or has no direct dependency.
        assert_eq!(stale_warning(WORKSPACE_LOCK, "c", "0.1.0", "0.2.0"), None);
        assert_eq!(stale_warning(WORKSPACE_LOCK, "ontogen-macros", "0.2.0", "0.2.0"), None);
        // A bare entry with two candidate packages does not resolve.
        let ambiguous = WORKSPACE_LOCK.replace("\"ontogen-macros 0.1.1\",", "\"ontogen-macros\",");
        assert_eq!(stale_warning(&ambiguous, "b", "0.1.0", "0.2.0"), None);
    }

    /// A stale per-crate lock inside a workspace is ignored, as cargo
    /// ignores it: the workspace root's lock is the one read.
    #[test]
    fn the_workspace_roots_lock_is_read_not_a_members_own() {
        let ws = tempfile::tempdir().unwrap();
        let a = ws.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(ws.path().join("Cargo.toml"), "[workspace]\nmembers = [\"a\"]\n").unwrap();
        std::fs::write(a.join("Cargo.toml"), "[package]\nname = \"a\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(a.join("Cargo.lock"), "stale").unwrap();
        assert_eq!(lock_path(&a), Some(ws.path().join("Cargo.lock")));

        // A standalone package reads its own; uncertain layouts read none.
        let solo = tempfile::tempdir().unwrap();
        std::fs::write(solo.path().join("Cargo.toml"), "[package]\nname = \"s\"\n").unwrap();
        assert_eq!(lock_path(solo.path()), Some(solo.path().join("Cargo.lock")));
        std::fs::write(a.join("Cargo.toml"), "[package]\nname = \"a\"\nworkspace = \"..\"\n").unwrap();
        assert_eq!(lock_path(&a), None);
        std::fs::write(a.join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        std::fs::write(ws.path().join("Cargo.toml"), "[workspace]\nmembers = [\"a\"]\nexclude = [\"x\"]\n").unwrap();
        assert_eq!(lock_path(&a), None);
    }

    #[test]
    fn this_workspace_resolves_the_macros_version_it_was_built_with() {
        let lock = include_str!("../../Cargo.lock");
        assert!(numeric(MACROS_VERSION).is_some(), "{MACROS_VERSION}");
        assert!(lock.contains(&format!("name = \"ontogen-macros\"\nversion = \"{MACROS_VERSION}\"")));
        assert_eq!(stale_warning(lock, "markdown-pilot", "0.1.0", MACROS_VERSION), None);
        assert!(stale_warning(lock, "markdown-pilot", "0.1.0", "99.0.0").is_some());
        assert_eq!(
            lock_path(Path::new(env!("CARGO_MANIFEST_DIR"))),
            Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"))
        );
    }
}
