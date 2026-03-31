#!/usr/bin/env node
/**
 * tauri-wrapper.mjs — Wraps the tauri CLI to inject auto-selected ports for dev mode.
 *
 * For `dev` subcommand:
 *   - Resolves port assignments via TAURI_DEV_PORT env or scripts/dev-port.sh
 *   - Injects --config with the correct devUrl
 *   - Sets all 4 port env vars in the child process
 *
 * For all other subcommands: passes through unchanged.
 */

import { execFileSync, spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = resolve(__dirname, '..');
const DEV_PORT_SCRIPT = resolve(__dirname, 'dev-port.sh');

// Parse args: node scripts/tauri-wrapper.mjs <subcommand> [args...]
const [subcommand, ...rest] = process.argv.slice(2);

/**
 * Run dev-port.sh --all and parse the KEY=VALUE output into an object.
 */
function resolveAllPorts() {
  const output = execFileSync(DEV_PORT_SCRIPT, ['--all'], {
    cwd: PROJECT_ROOT,
    encoding: 'utf-8',
  }).trim();

  const ports = {};
  for (const line of output.split('\n')) {
    const [key, value] = line.split('=');
    if (key && value) {
      ports[key.trim()] = value.trim();
    }
  }
  return ports;
}

/**
 * Build the tauri args and env, then spawn npx tauri.
 */
function run() {
  let args = subcommand ? [subcommand, ...rest] : [...rest];
  const env = { ...process.env };

  if (subcommand === 'dev') {
    let ports;

    if (env.TAURI_DEV_PORT) {
      // Port already set — derive the rest from the block (base + offsets)
      const base = parseInt(env.TAURI_DEV_PORT, 10);
      ports = {
        TAURI_DEV_PORT: String(base),
        STORYBOOK_PORT: env.STORYBOOK_PORT || String(base + 1),
        MCP_PORT: env.MCP_PORT || String(base + 2),
        HTTP_PORT: env.HTTP_PORT || String(base + 3),
      };
    } else {
      ports = resolveAllPorts();
    }

    // Set all port env vars in the child process
    for (const [key, value] of Object.entries(ports)) {
      env[key] = value;
    }

    const devUrl = `http://localhost:${ports.TAURI_DEV_PORT}`;
    const configJson = JSON.stringify({ build: { devUrl } });

    // Inject --config before any existing args
    args = [subcommand, '--config', configJson, ...rest];

    console.log(
      `[auto-port] Nuxt: ${ports.TAURI_DEV_PORT} | Storybook: ${ports.STORYBOOK_PORT} | MCP: ${ports.MCP_PORT} | HTTP: ${ports.HTTP_PORT}`
    );
  }

  const child = spawn('npx', ['tauri', ...args], {
    cwd: PROJECT_ROOT,
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });

  child.on('error', (err) => {
    console.error(`[tauri-wrapper] Failed to start tauri: ${err.message}`);
    process.exit(1);
  });

  child.on('exit', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
    } else {
      process.exit(code ?? 1);
    }
  });
}

run();
