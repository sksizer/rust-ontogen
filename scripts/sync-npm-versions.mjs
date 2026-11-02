#!/usr/bin/env node
// Keep the npm packages in lockstep with the `ontogen` crate.
//
// The crate's version in the root Cargo.toml is the source of truth: release-plz
// bumps it, and this script writes the same version into each package under
// packages/, plus the admin layer's peer range on the admin types. A consumer can
// then read "ontogen 0.9 goes with @osdlc/ontogen-admin-layer 0.9" off the
// version alone.
//
//   node scripts/sync-npm-versions.mjs           rewrite the package.json files
//   node scripts/sync-npm-versions.mjs --check   exit 1 if any is out of step
//   node scripts/sync-npm-versions.mjs --print   print the crate version

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

const TYPES = '@osdlc/ontogen-admin-types'
const PACKAGES = ['packages/admin-types/package.json', 'packages/nuxt_admin_layer/package.json']

/** The `version` of the root Cargo.toml's `[package]` table. */
function crateVersion() {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const table = cargo.split(/^\[/m).find((section) => section.startsWith('package]'))
  const match = table?.match(/^version\s*=\s*"([^"]+)"/m)
  if (!match) throw new Error('no version in the [package] table of Cargo.toml')
  return match[1]
}

/** The package.json as it should read at `version`. */
function expected(pkg, version) {
  const next = structuredClone(pkg)
  next.version = version
  if (next.peerDependencies?.[TYPES] !== undefined) next.peerDependencies[TYPES] = `^${version}`
  return next
}

const version = crateVersion()
const mode = process.argv[2]
if (mode === '--print') {
  console.log(version)
  process.exit(0)
}

let stale = 0
for (const rel of PACKAGES) {
  const path = join(root, rel)
  const pkg = JSON.parse(readFileSync(path, 'utf8'))
  const next = expected(pkg, version)
  const text = `${JSON.stringify(next, null, 2)}\n`
  if (text === readFileSync(path, 'utf8')) continue
  if (mode === '--check') {
    console.error(`${rel}: ${pkg.name}@${pkg.version} is out of step with the ontogen crate (${version})`)
    stale += 1
  } else {
    writeFileSync(path, text)
    console.log(`${rel}: ${pkg.name} -> ${version}`)
  }
}
if (stale) {
  console.error('Run `just npm-versions` to bring them in step.')
  process.exit(1)
}
