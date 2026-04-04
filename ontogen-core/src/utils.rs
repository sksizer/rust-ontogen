//! Build-time utilities shared across codegen layers.

use std::collections::HashSet;
use std::path::Path;

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
    write_if_changed(path, formatted).map_err(|e| CodegenError::Persistence(format!("Failed to write {}: {e}", path.display())))
}

/// Run `rustfmt` on a string in memory, returning the formatted result.
///
/// Returns `CodegenError::ExternalTool` if `rustfmt` cannot be spawned.
fn rustfmt_string(input: &str) -> Result<String, CodegenError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
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
            println!("cargo:warning=ontogen: rustfmt exited with {}, falling back to unformatted output: {stderr}", output.status);
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

/// Format TypeScript content in memory via prettier, then write only if changed.
///
/// Mirrors `write_and_format` for Rust — formats in memory first so
/// `write_if_changed` can skip the write when content is identical,
/// preventing unnecessary mtime changes that trigger file-watchers.
///
/// Returns `CodegenError::ExternalTool` if `npx` / `prettier` is not installed.
pub fn write_and_format_ts(path: &Path, content: impl AsRef<str>) -> Result<(), CodegenError> {
    let formatted = prettier_string(content.as_ref())?;
    write_if_changed(path, formatted).map_err(|e| CodegenError::Client(format!("Failed to write {}: {e}", path.display())))
}

/// Run `prettier` on a string in memory, returning the formatted result.
///
/// Returns `CodegenError::ExternalTool` if `npx` cannot be spawned.
fn prettier_string(input: &str) -> Result<String, CodegenError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("npx")
        .args(["prettier", "--parser", "typescript", "--single-quote"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CodegenError::ExternalTool {
            tool: "npx (prettier)",
            detail: format!("failed to spawn: {e}. Is Node.js installed? Try: npm install -g prettier"),
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
            println!("cargo:warning=ontogen: prettier exited with {}, falling back to unformatted output: {stderr}", output.status);
            Ok(input.to_string())
        }
        Err(e) => {
            println!("cargo:warning=ontogen: prettier wait failed: {e}, falling back to unformatted output");
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
/// generated output subdirectories — watching generated output creates
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
