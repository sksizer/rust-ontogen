set allow-duplicate-recipes := true
set allow-duplicate-variables := true
set shell := ["bash", "-euo", "pipefail", "-c"]

# ---------------------------------------------------------------------------- #
#                                 DEPENDENCIES                                 #
# ---------------------------------------------------------------------------- #

# Rust: https://rust-lang.org/tools/install
cargo := require("cargo")
rustc := require("rustc")

# pnpm: https://pnpm.io/installation (used by packages/nuxt_admin_layer's test
# suite and to type-check the generated TypeScript)
pnpm := require("pnpm")

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

# Run clippy lints (whole workspace, all targets, so every member crate is gated).
# markdown-store and ontogen-jsonapi get an extra no-default-features pass:
# their feature-gated modules can hide dead code from the default-features
# workspace lint, and ontogen-jsonapi's core is meant to build without
# `axum` for the build-time generator. Their rustdoc, private items
# included, must also build without warnings (broken intra-doc links).
lint:
    cargo clippy --workspace --all-targets -- --deny warnings
    cargo clippy -p markdown-store --no-default-features -- --deny warnings
    cargo clippy -p ontogen-jsonapi --no-default-features --all-targets -- --deny warnings
    RUSTDOCFLAGS="-D warnings" cargo doc -p markdown-store --no-deps --document-private-items
    RUSTDOCFLAGS="-D warnings" cargo doc -p ontogen-jsonapi --no-deps --document-private-items
    node scripts/sync-npm-versions.mjs --check

# Run all code checks (matches CI: format-check + lint + tests)
full-check: format-check lint test
alias fc := full-check

# Fix formatting (alias for format)
full-write: format
alias fw := full-write

# Run tests (whole workspace, so every member crate is gated). The last
# line tests markdown-store's doc examples on their own: there the crate
# builds without its `uuid` feature, which parity-markdown turns on for the
# whole workspace, so the compile_fail example on `IdStrategy` proves the
# `Uuid` variant does not exist without the feature.
test: && test-admin-layer test-ts-clients
    cargo test --workspace
    cargo test -p ontogen-jsonapi --no-default-features
    cargo test -p markdown-store --doc

# Run packages/nuxt_admin_layer's vitest suite (regression tests for the Nuxt
# admin layer; the layer itself has no Nuxt app of its own to build inside,
# so these mount its composables/components/pages directly - see
# packages/nuxt_admin_layer/vitest.config.ts).
test-admin-layer:
    pnpm install --frozen-lockfile
    pnpm --filter @osdlc/ontogen-admin-layer test

# Type-check the TypeScript the name-safety fixture generates (its build
# script rewrites crates/name-safety/generated-ts when the crate builds, as
# `cargo test --workspace` does) under `tsc --strict`, Tauri's API stubbed.
test-ts-clients:
    pnpm install --frozen-lockfile
    pnpm exec tsc -p crates/name-safety/tsconfig.json

# The examples live outside the root cargo workspace and commit their generated
# trees so diffs stay reviewable, but nothing regenerates them automatically —
# each one's build.rs only runs when that example is built. The Examples
# workflow fails a PR whose committed output no longer matches.
#
# iron-log is a Tauri app: on Linux it needs the GTK/WebKit -dev packages the
# Examples workflow installs. It builds as-is on macOS.
#
# This list is duplicated by the matrix in .github/workflows/examples-ci.yml.
# Adding an example means editing both. They are kept separate deliberately:
# CI runs one job per example for parallelism and to gate the apt install on
# the Tauri entry, neither of which survives being driven from here. If they
# disagree, CI is the one that gates, and an example missing from this recipe
# just means a slower path to discovering that.
#
# Note the one difference: CI builds with `--locked` so a stale lockfile fails
# loudly, while this recipe deliberately does not, because refreshing those
# lockfiles is part of what it is for.
#
# Rebuild every example's committed generator output, then commit what moves.
regen-examples:
    cargo build --manifest-path examples/iron-log-md/Cargo.toml
    cargo build --manifest-path examples/tasks-tracker/Cargo.toml
    cargo build --manifest-path examples/notes-kb/Cargo.toml
    cargo check --manifest-path examples/iron-log/src-tauri/Cargo.toml
    cargo check --manifest-path examples/ipc-shell/Cargo.toml
alias rex := regen-examples

# Builds the example, serves a temp copy of its vault on PORT (default 39302)
# and stops only the server it started, so the committed vault is never
# written. The client covers list, pagination, sort, filter, include, create,
# patch, relationship fetches and mutations, delete and the 404 after it,
# using only kitsu's documented options. Needs Node and npm. The Examples
# workflow runs it in the tasks-tracker job.
#
# Drive examples/tasks-tracker with a third-party JSON:API client (kitsu).
conformance-tasks-tracker:
    examples/tasks-tracker/conformance/run.sh

# Alias retained for muscle memory: same as full-check now that tests are folded in.
ci: full-check

# Build API docs (no deps)
doc:
    cargo doc --no-deps

# Build API docs and open in the browser
doc-open:
    cargo doc --no-deps --open

# List dependencies with newer versions available (requires cargo-outdated)
outdated:
    cargo outdated

# Check dependencies for known security advisories (requires cargo-audit)
audit:
    cargo audit

# Install optional cargo tools used by other recipes (cargo-outdated, cargo-audit)
install-tools:
    cargo install --locked cargo-outdated cargo-audit

# ---------------------------------------------------------------------------- #
#                                   RELEASE                                    #
# ---------------------------------------------------------------------------- #

# Generate changelog from conventional commits
changelog:
    git-cliff --output CHANGELOG.md

# Check for semver violations against the latest git tag
semver-check:
    cargo semver-checks --baseline-rev "$(git describe --tags --abbrev=0)"

# NOTE: releases are automated by the release-plz CI workflow
# (.github/workflows/release-plz.yml). On every push to main it opens/updates a
# "chore: release" PR; merging that PR tags, publishes to crates.io, and creates
# the GitHub Release. See RELEASE.md. The `changelog` and `semver-check` recipes
# above stay for local previewing; there is no local "run a release" recipe.

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

# ---------------------------------------------------------------------------- #
#                             TEMPLATE REVIEW                                  #
# ---------------------------------------------------------------------------- #

# Review a downstream project for improvements to backport into the template (dry-run by default; --execute to run, optional target dir)
template-review *args:
    bash scripts/template_review.sh {{args}}
alias tr := template-review

# Review all downstream projects in parallel for backport candidates (dry-run by default; --execute to run)
template-review-all *args:
    bash scripts/template_review_all.sh {{args}}
alias tra := template-review-all

# ---------------------------------------------------------------------------- #
#                            TEMPLATE BACKPORT                                 #
# ---------------------------------------------------------------------------- #

# Backport improvements from a downstream project into a fresh template clone and open a PR (dry-run by default; --execute to run)
template-backport *args:
    bash scripts/template_backport.sh {{args}}
alias tb := template-backport

# Backport improvements from all downstream projects, one PR each (dry-run by default; --execute to run)
template-backport-all *args:
    bash scripts/template_backport_all.sh {{args}}
alias tba := template-backport-all

# ---------------------------------------------------------------------------- #
#                                   COUSINS                                    #
# ---------------------------------------------------------------------------- #

# Review one cousin repo (by name from scripts/cousins.json) for template-sourced improvements (dry-run by default; --execute to run)
cousin-review *args:
    bash scripts/cousin_review.sh {{args}}
alias cr := cousin-review

# Review every cousin in scripts/cousins.json in parallel (dry-run by default; --execute to run)
cousin-review-all *args:
    bash scripts/cousin_review_all.sh {{args}}
alias cra := cousin-review-all

# Apply template-sourced changes to a cousin's opted-in paths and open a PR (dry-run by default; --execute to run)
cousin-apply *args:
    bash scripts/cousin_apply.sh {{args}}
alias ca := cousin-apply

# Apply changes to every cousin, one PR each (dry-run by default; --execute to run)
cousin-apply-all *args:
    bash scripts/cousin_apply_all.sh {{args}}
alias caa := cousin-apply-all

# ---------------------------------------------------------------------------- #
#                                ADMIN PACKAGES                                #
# ---------------------------------------------------------------------------- #

# The npm packages are versioned in lockstep with the ontogen crate: the
# crate's version in Cargo.toml is the source of truth, and `lint` fails when
# a package is out of step. release-plz bumps the crate; its workflow then
# runs `npm-versions` on the release PR, and publishes both packages when the
# release is cut (.github/workflows/release-plz.yml).

# Write the ontogen crate's version into the npm packages
npm-versions:
    node scripts/sync-npm-versions.mjs

# Pack @osdlc/ontogen-admin-types and @osdlc/ontogen-admin-layer into dist/ and check the tarballs are publishable
pack-admin:
    #!/usr/bin/env bash
    set -euo pipefail
    node scripts/sync-npm-versions.mjs --check
    mkdir -p dist
    rm -f dist/osdlc-ontogen-admin-*.tgz
    (cd packages/admin-types && pnpm pack --pack-destination "$(pwd)/../../dist")
    (cd packages/nuxt_admin_layer && pnpm pack --pack-destination "$(pwd)/../../dist")
    # pnpm rewrites `workspace:` ranges when it packs; a survivor would make the
    # published package uninstallable.
    for tgz in dist/osdlc-ontogen-admin-*.tgz; do
      if tar -xOzf "$tgz" package/package.json | grep -q '"workspace:'; then
        echo "$tgz: package.json still has a workspace: range" >&2
        exit 1
      fi
    done
    ls -1 dist/osdlc-ontogen-admin-*.tgz

# Publish both packages to npm, skipping a version already there. CI runs this
# on release; by hand it is only needed for the very first publish
publish-admin *flags: pack-admin
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(node scripts/sync-npm-versions.mjs --print)
    # Types first: the layer's peer range points at them.
    for name in ontogen-admin-types ontogen-admin-layer; do
      if npm view "@osdlc/$name@$version" version >/dev/null 2>&1; then
        echo "@osdlc/$name@$version is already on npm; skipping"
        continue
      fi
      # The ./ matters: npm reads `dist/<name>.tgz` as the GitHub repo
      # shorthand `<user>/<repo>` and tries to git-clone it.
      npm publish "./dist/osdlc-$name-$version.tgz" --access public {{flags}}
    done

# Stage the admin packages on npm, once, before their first CI release. npm
# can only add a trusted publisher to a package that exists, so this publishes
# a real build as a `0.0.1-stage` placeholder under the `stage` tag, then adds
# the GitHub Actions trusted publisher (release-plz.yml) that CI publishes
# through. package.json is restored after packing. Skips a package already on
# npm. Needs `npm login` as an owner of @osdlc, two-factor auth, and npm 11.15
# or later. `just stage-admin --dry-run` publishes and changes nothing.
stage-admin *flags:
    #!/usr/bin/env bash
    set -euo pipefail
    stage=0.0.1-stage
    out="$(mktemp -d)"
    backup="$(mktemp -d)"
    cp packages/admin-types/package.json "$backup/types.json"
    cp packages/nuxt_admin_layer/package.json "$backup/layer.json"
    restore() {
      cp "$backup/types.json" packages/admin-types/package.json
      cp "$backup/layer.json" packages/nuxt_admin_layer/package.json
    }
    trap restore EXIT
    for dir in packages/admin-types packages/nuxt_admin_layer; do
      node -e '
        const fs = require("fs"); const [path, v] = process.argv.slice(1)
        const p = JSON.parse(fs.readFileSync(path, "utf8")); p.version = v
        if (p.peerDependencies?.["@osdlc/ontogen-admin-types"]) p.peerDependencies["@osdlc/ontogen-admin-types"] = v
        fs.writeFileSync(path, JSON.stringify(p, null, 2) + "\n")
      ' "$dir/package.json" "$stage"
      (cd "$dir" && pnpm pack --pack-destination "$out" >/dev/null)
    done
    restore
    trap - EXIT
    echo "npm user: $(npm whoami 2>/dev/null || echo '(not logged in)')"
    # Types first: the layer's peer range points at them.
    for name in ontogen-admin-types ontogen-admin-layer; do
      pkg="@osdlc/$name"
      echo; echo "$pkg"
      if npm view "$pkg" name >/dev/null 2>&1; then
        echo "  already on npm; not publishing a placeholder"
      else
        npm publish "$out/osdlc-$name-$stage.tgz" --access public --tag stage {{flags}}
      fi
      npm trust github "$pkg" --file release-plz.yml --repository sksizer/rust-ontogen --allow-publish --yes {{flags}}
    done
    rm -rf "$out" "$backup"
