set allow-duplicate-recipes := true
set allow-duplicate-variables := true
set shell := ["bash", "-euo", "pipefail", "-c"]

# ---------------------------------------------------------------------------- #
#                                 DEPENDENCIES                                 #
# ---------------------------------------------------------------------------- #

# Rust: https://rust-lang.org/tools/install
cargo := require("cargo")
rustc := require("rustc")

# ---------------------------------------------------------------------------- #
#                                    RECIPES                                   #
# ---------------------------------------------------------------------------- #

# Show available commands
default:
    @just --list

# Build the program
build:
    cargo build

# Run the program
run:
    cargo run

# Check code formatting
format-check:
    cargo fmt --all --check

# Format code
format:
    cargo fmt --all
alias fmt := format

# Run clippy lints
lint:
    cargo clippy -- --deny warnings

# Run all code checks
full-check: format-check lint
alias fc := full-check

# Fix formatting (alias for format)
full-write: format
alias fw := full-write

# Run tests
test:
    cargo test

# ---------------------------------------------------------------------------- #
#                                   RELEASE                                    #
# ---------------------------------------------------------------------------- #

# Generate changelog from conventional commits
changelog:
    git-cliff --output CHANGELOG.md

# Check for semver violations against the latest git tag
semver-check:
    cargo semver-checks --baseline-rev "$(git describe --tags --abbrev=0)"

# Dry-run a release (default: patch bump)
release-dry-run level="patch":
    cargo release {{level}} --no-confirm

# Perform a release (patch, minor, or major)
release level="patch":
    cargo release {{level}} --execute

# ---------------------------------------------------------------------------- #
#                               QUALITY CHECK                                  #
# ---------------------------------------------------------------------------- #

# Run checks with AI-powered auto-fix (dry-run by default; --execute to run)
quality-check *args:
    bash scripts/quality_check.sh {{args}}
alias qc := quality-check

# ---------------------------------------------------------------------------- #
#                                  TEMPLATE                                    #
# ---------------------------------------------------------------------------- #

# Bring repo up to date with upstream template (dry-run by default; --execute to run, optional target dir)
bring-up-to-date *args:
    bash scripts/bring_up_to_date.sh {{args}}
alias butd := bring-up-to-date

# Bring all projects in downstream.txt up to date in parallel (dry-run by default; --execute to run)
bring-up-to-date-all *args:
    bash scripts/bring_up_to_date_all.sh {{args}}
alias butda := bring-up-to-date-all

# ---------------------------------------------------------------------------- #
#                               CARGO UPDATE                                   #
# ---------------------------------------------------------------------------- #

# Update cargo dependencies, run checks, and open a PR (dry-run by default; --execute to run, optional target dir)
cargo-update *args:
    bash scripts/cargo_update.sh {{args}}
alias cu := cargo-update

# Update cargo dependencies in all downstream projects in parallel (dry-run by default; --execute to run)
cargo-update-all *args:
    bash scripts/cargo_update_all.sh {{args}}
alias cua := cargo-update-all
