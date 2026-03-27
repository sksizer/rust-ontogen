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

# Check for semver violations against the latest git tag
semver-check:
    cargo semver-checks --baseline-rev "$(git describe --tags --abbrev=0)"

# Dry-run a release (default: patch bump)
release-dry-run level="patch":
    cargo release {{level}} --no-confirm

# Perform a release (patch, minor, or major)
release level="patch":
    cargo release {{level}} --execute
