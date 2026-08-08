//! Build-time utilities shared across codegen layers.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::CodegenError;

/// Write content to a file only if the content has changed.
///
/// This prevents unnecessary file-system modifications that trigger
/// file-watchers (e.g. Tauri dev) and cause infinite rebuild loops.
pub fn write_if_changed(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    let content = content.as_ref();
    if path.exists()
        && let Ok(existing) = std::fs::read(path)
        && existing == content
    {
        return Ok(());
    }
    std::fs::write(path, content)
}

/// Write content to a file and run `rustfmt`, but only if the formatted
/// result differs from what's already on disk.
///
/// This avoids touching file mtimes when nothing changed, preventing
/// infinite rebuild loops with file-watchers (e.g. Tauri dev).
///
/// Returns `CodegenError::ExternalTool` if `rustfmt` is not installed.
pub fn write_and_format(path: &Path, content: impl AsRef<str>) -> Result<(), CodegenError> {
    let formatted = rustfmt_string(content.as_ref())?;
    write_if_changed(path, formatted)
        .map_err(|e| CodegenError::Persistence(format!("Failed to write {}: {e}", path.display())))
}

/// Detect the Rust edition from the consuming crate's `Cargo.toml`.
///
/// Reads `CARGO_MANIFEST_DIR` (set by Cargo during `build.rs` execution)
/// and extracts the `edition` field. Falls back to `"2021"` if unavailable.
///
/// The result is cached per process, since `CARGO_MANIFEST_DIR` and the
/// file it points to are stable for the duration of a single build.
fn detect_edition() -> &'static str {
    static EDITION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    EDITION.get_or_init(|| {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let cargo_toml = std::path::Path::new(&manifest_dir).join("Cargo.toml");
            if let Ok(content) = std::fs::read_to_string(cargo_toml) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("edition") {
                        let rest = rest.trim().strip_prefix('=').unwrap_or(rest).trim();
                        let rest = rest.trim_matches('"').trim_matches('\'');
                        if rest.len() == 4 && rest.chars().all(|c| c.is_ascii_digit()) {
                            return rest.to_string();
                        }
                    }
                }
            }
        }
        "2021".to_string()
    })
}

/// Run `rustfmt` on a string in memory, returning the formatted result.
///
/// Auto-detects the Rust edition from `CARGO_MANIFEST_DIR/Cargo.toml` so
/// the formatting (especially import sorting) matches what `cargo fmt`
/// produces in the consuming crate. Edition 2024 uses case-sensitive
/// ASCII sort; edition 2021 uses case-insensitive sort.
///
/// Returns `CodegenError::ExternalTool` if `rustfmt` cannot be spawned.
fn rustfmt_string(input: &str) -> Result<String, CodegenError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let edition = detect_edition();

    let mut child = Command::new("rustfmt")
        .arg("--edition")
        .arg(edition)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CodegenError::ExternalTool {
            tool: "rustfmt",
            detail: format!("failed to spawn: {e}. Is rustfmt installed? Try: rustup component add rustfmt"),
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    match child.wait_with_output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8(output.stdout).unwrap_or_else(|_| input.to_string()))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!(
                "cargo:warning=ontogen: rustfmt exited with {}, falling back to unformatted output: {stderr}",
                output.status
            );
            Ok(input.to_string())
        }
        Err(e) => {
            println!("cargo:warning=ontogen: rustfmt wait failed: {e}, falling back to unformatted output");
            Ok(input.to_string())
        }
    }
}

/// Run `rustfmt` on a generated Rust file.
/// Silently ignores failures (e.g., if rustfmt is not installed).
pub fn rustfmt(path: &Path) {
    let _ = std::process::Command::new("rustfmt").arg("--edition").arg("2024").arg(path).status();
}

/// Signature of an in-process TypeScript formatting hook: full generated
/// source in, formatted source out (or a human-readable error).
pub type TsFormatFn = dyn Fn(&str) -> Result<String, String> + Send + Sync;

/// How to format generated TypeScript before writing it.
///
/// Default is [`TsFormatter::None`] — the generated output is written as-is.
/// Wire up [`TsFormatter::custom`] to format in-process with a library of your
/// choice, or [`TsFormatter::Command`] to shell out to an external tool.
#[derive(Clone, Default)]
pub enum TsFormatter {
    /// Emit the generated TypeScript unformatted — skip the formatting pass.
    #[default]
    None,
    /// Format in-process with a caller-supplied function: full generated
    /// source in, formatted source out. Construct via [`TsFormatter::custom`].
    ///
    /// An `Err` from the hook aborts the write and surfaces as a
    /// [`CodegenError`]; hooks that prefer emitting unformatted output on
    /// failure should catch internally and return `Ok(input.to_string())`.
    Custom(std::sync::Arc<TsFormatFn>),
    /// Run an external formatter, piping the TS through the child's stdin and
    /// reading the formatted result from its stdout. The output file's resolved
    /// path is appended as the final argument (so e.g. prettier's
    /// `--stdin-filepath` can resolve config). Runs from the nearest ancestor
    /// `node_modules` directory when one exists.
    ///
    /// e.g. `TsFormatter::Command(vec!["prettier".into(), "--stdin-filepath".into()])`.
    Command(Vec<String>),
}

impl TsFormatter {
    /// Wrap an in-process formatting function as [`TsFormatter::Custom`].
    ///
    /// e.g. `TsFormatter::custom(|src| my_fmt::format(src).map_err(|e| e.to_string()))`.
    pub fn custom(f: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) -> Self {
        Self::Custom(std::sync::Arc::new(f))
    }
}

impl std::fmt::Debug for TsFormatter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Custom(_) => f.write_str("Custom(..)"),
            Self::Command(cmd) => f.debug_tuple("Command").field(cmd).finish(),
        }
    }
}

/// Format TypeScript content in memory per `formatter`, then write only if
/// changed.
///
/// Mirrors `write_and_format` for Rust — formats in memory first so
/// `write_if_changed` can skip the write when content is identical,
/// preventing unnecessary mtime changes that trigger file-watchers.
pub fn write_and_format_ts(path: &Path, content: impl AsRef<str>, formatter: &TsFormatter) -> Result<(), CodegenError> {
    let content = content.as_ref();
    let formatted = match formatter {
        TsFormatter::None => content.to_string(),
        TsFormatter::Custom(format) => format(content)
            .map_err(|e| CodegenError::Client(format!("custom TS formatter failed for {}: {e}", path.display())))?,
        TsFormatter::Command(cmd) => command_format_ts(content, cmd, &resolve_ts_path(path))?,
    };
    write_if_changed(path, formatted)
        .map_err(|e| CodegenError::Client(format!("Failed to write {}: {e}", path.display())))
}

/// Canonicalize the output path so an external formatter can resolve config
/// (e.g. `.prettierrc`) from the file's own directory rather than the build
/// script's CWD (typically `src-tauri/`). Creates the parent if needed.
fn resolve_ts_path(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
        match std::fs::canonicalize(parent) {
            Ok(abs_parent) => abs_parent.join(path.file_name().unwrap_or_default()),
            Err(_) => path.to_path_buf(),
        }
    } else {
        path.to_path_buf()
    }
}

/// Walk up from `start` looking for the nearest ancestor directory that
/// contains a `node_modules` subdirectory. Returns that ancestor (where a
/// node-package-manager binary like `pnpm exec` / `npx` can resolve a
/// project-local install).
fn find_node_modules_root(start: &Path) -> Option<PathBuf> {
    let mut cur: PathBuf = start.to_path_buf();
    loop {
        if cur.join("node_modules").is_dir() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Run an external formatter command on a string in memory, returning the
/// formatted result. The command is `cmd[0]` invoked with `cmd[1..]` as
/// arguments; `virtual_path` is appended as the final argument (so e.g.
/// prettier's `--stdin-filepath` receives the file path) and used to root the
/// child at the nearest ancestor `node_modules` when one exists — which lets a
/// project-local `prettier` resolve from a build script whose CWD (typically
/// `src-tauri/`) has no `node_modules` of its own.
///
/// On a non-zero exit or wait failure, emits a `cargo:warning` and falls back
/// to the unformatted input. Returns `CodegenError::ExternalTool` only when the
/// command cannot be spawned at all.
fn command_format_ts(input: &str, cmd: &[String], virtual_path: &Path) -> Result<String, CodegenError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Some((program, args)) = cmd.split_first() else {
        return Err(CodegenError::ExternalTool {
            tool: "ts formatter command",
            detail: "TsFormatter::Command was given an empty command".to_string(),
        });
    };

    let nm_root = virtual_path.parent().and_then(find_node_modules_root);

    let mut command = Command::new(program);
    command.args(args).arg(virtual_path).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(ref cwd) = nm_root {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().map_err(|e| CodegenError::ExternalTool {
        tool: "ts formatter command",
        detail: format!("failed to spawn `{program}`: {e}"),
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    match child.wait_with_output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8(output.stdout).unwrap_or_else(|_| input.to_string()))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!(
                "cargo:warning=ontogen: ts formatter `{program}` exited with {}, falling back to unformatted output: {stderr}",
                output.status
            );
            Ok(input.to_string())
        }
        Err(e) => {
            println!(
                "cargo:warning=ontogen: ts formatter `{program}` wait failed: {e}, falling back to unformatted output"
            );
            Ok(input.to_string())
        }
    }
}

/// Remove `.rs` files from `dir` that are not in `expected`.
///
/// Call this at the start of each generator to clean up files left behind
/// by entity renames or deletions.  `expected` should contain bare filenames
/// like `"node.rs"`, `"mod.rs"`, etc.  Files whose names are not in the set
/// are deleted.  Non-`.rs` files and subdirectories are left alone.
pub fn clean_generated_dir(dir: &Path, expected: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            let name = entry.file_name().to_string_lossy().to_string();
            if !expected.contains(&name) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// Emit `cargo:rerun-if-changed` directives for all `.rs` files in a directory.
pub fn emit_rerun_directives(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

/// Emit `cargo:rerun-if-changed` directives for `.rs` files in a directory,
/// excluding subdirectories whose names are in `exclude_dirs`.
///
/// Use this when a directory contains both hand-written source files and
/// generated output subdirectories - watching generated output creates
/// a self-triggering rebuild loop.
pub fn emit_rerun_directives_excluding(dir: &Path, exclude_dirs: &[&str]) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if !exclude_dirs.iter().any(|ex| *ex == name_str.as_ref()) {
                emit_rerun_directives(&path);
            }
            continue;
        }

        if path.extension().is_some_and(|ext| ext == "rs") {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Compute a relative path from `base` directory to `target` file.
///
/// Both paths should be absolute. Returns a relative path like `../generated/api-types`.
pub fn relative_path(base: &Path, target: &Path) -> PathBuf {
    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target.components().collect();

    // Find common prefix length
    let common = base_components.iter().zip(target_components.iter()).take_while(|(a, b)| a == b).count();

    let mut result = PathBuf::new();

    // Go up from base to common ancestor
    for _ in common..base_components.len() {
        result.push("..");
    }

    // Go down to target from common ancestor
    for component in &target_components[common..] {
        if let Component::Normal(s) = component {
            result.push(s);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{TsFormatter, write_and_format_ts};

    #[test]
    fn custom_formatter_shapes_output() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("out.ts");
        let formatter = TsFormatter::custom(|src| Ok(src.to_uppercase()));
        write_and_format_ts(&path, "const x = 1;\n", &formatter).expect("write");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "CONST X = 1;\n");
    }

    #[test]
    fn custom_formatter_error_aborts_write() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("out.ts");
        let formatter = TsFormatter::custom(|_| Err("nope".to_string()));
        let err = write_and_format_ts(&path, "const x = 1;\n", &formatter).expect_err("must fail");
        assert!(err.to_string().contains("nope"), "hook error surfaces; got: {err}");
        assert!(!path.exists(), "nothing written on formatter failure");
    }

    #[test]
    fn none_formatter_passes_through() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("out.ts");
        write_and_format_ts(&path, "const  x=1\n", &TsFormatter::None).expect("write");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "const  x=1\n");
    }
}
