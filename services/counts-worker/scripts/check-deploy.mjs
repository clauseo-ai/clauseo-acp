#!/usr/bin/env node
// Refuses a wrangler.toml that departs from the counts Worker's privacy settings
// (docs/counts-worker.md, "What the Worker never keeps"). Run on the committed file in both
// workflows, and on the deploy job's generated copy with --deployed before anything reaches
// Cloudflare.
//
//   node scripts/check-deploy.mjs [path] [--deployed]
//
// The committed file must hold the placeholder database_id and the [build] command; a
// --deployed copy must hold a real id and no [build], so wrangler deploys what the tokenless
// build step made and runs no build with the token present. Every other rule is the same: only
// the keys below, each with exactly the value below.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { parse } from "smol-toml";

export const PLACEHOLDER_ID = "00000000-0000-0000-0000-000000000000";
const D1_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** Every top-level key the file may have, with the value it must have. */
const EXPECTED = {
  name: "clauseo-counts",
  main: "build/worker/shim.mjs",
  compatibility_date: "2026-11-01",
  workers_dev: false,
  preview_urls: false,
  routes: [{ pattern: "t.clauseo.chat", custom_domain: true }],
  build: { command: "worker-build --release" },
  observability: {
    enabled: false,
    logs: { enabled: false, invocation_logs: false },
    traces: { enabled: false },
  },
  triggers: { crons: ["0 3 * * *"] },
};

/** The one D1 binding, less its id. */
const D1 = { binding: "DB", database_name: "clauseo-counts", migrations_dir: "migrations" };

/**
 * Every rule `text` breaks, as one line each; empty when it passes.
 * @param {string} text the TOML
 * @param {{ deployed?: boolean }} options
 */
export function problems(text, { deployed = false } = {}) {
  const expected = { ...EXPECTED };
  if (deployed) delete expected.build;
  let config;
  try {
    config = parse(text);
  } catch (error) {
    return [`not TOML: ${error.message.split("\n")[0]}`];
  }
  const found = [];
  for (const key of Object.keys(config)) {
    if (!(key in expected) && key !== "d1_databases") {
      found.push(`${key}: not allowed (only ${[...Object.keys(expected), "d1_databases"].join(", ")})`);
    }
  }
  for (const [key, want] of Object.entries(expected)) {
    if (!isDeepStrictEqual(plain(config[key]), want)) {
      found.push(`${key}: must be ${JSON.stringify(want)}, is ${JSON.stringify(config[key])}`);
    }
  }
  const d1 = plain(config.d1_databases);
  if (!Array.isArray(d1) || d1.length !== 1) {
    found.push("d1_databases: must hold exactly one database");
    return found;
  }
  const { database_id: id, ...rest } = d1[0];
  if (!isDeepStrictEqual(rest, D1)) {
    found.push(`d1_databases: must be ${JSON.stringify(D1)} and its id, is ${JSON.stringify(rest)}`);
  }
  if (deployed && (id === PLACEHOLDER_ID || !D1_ID.test(String(id)))) {
    found.push("d1_databases.database_id: the deploy copy must hold the real id");
  }
  if (!deployed && id !== PLACEHOLDER_ID) {
    found.push(`d1_databases.database_id: the committed file holds ${PLACEHOLDER_ID}`);
  }
  return found;
}

/** The parsed value as plain objects, so a TOML date or table compares like JSON. */
function plain(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const deployed = args.includes("--deployed");
  const path = args.find((arg) => !arg.startsWith("--")) ??
    join(dirname(fileURLToPath(import.meta.url)), "..", "wrangler.toml");
  const found = problems(readFileSync(path, "utf8"), { deployed });
  if (found.length > 0) {
    for (const line of found) console.error(`::error file=${path}::${line}`);
    process.exit(1);
  }
  console.log(`${path}: the privacy settings hold${deployed ? " (deploy copy)" : ""}`);
}
