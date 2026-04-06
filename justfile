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

# Run all code checks
full-check:
    cargo fmt --all --check
    cargo clippy -- --deny warnings
alias fc := full-check

full-write:
    cargo fmt --all
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

# Update cargo dependencies, run checks, and open a PR (dry-run by default; --execute to run, optional target dir)
cargo-update *args:
    bash scripts/cargo_update.sh {{args}}
alias cu := cargo-update

# Update cargo dependencies in all downstream projects in parallel (dry-run by default; --execute to run)
cargo-update-all *args:
    bash scripts/cargo_update_all.sh {{args}}
alias cua := cargo-update-all
