#!/usr/bin/env node
// Every wrangler call of the counts-deploy job (docs/counts-worker.md, "Deploying"). Run logs on
// this public repo are public, so wrangler's output never reaches the log as it is: each line
// goes through `redact`, which masks UUIDs, 32-hex ids, emails and "…'s Account" names. The
// database id is also masked for the rest of the job with ::add-mask:: once found.
//
//   node scripts/deploy.mjs config [--no-create]
//     Finds the D1 database clauseo-counts by name (`wrangler d1 list --json`), or creates it in
//     the EU jurisdiction. A database whose jurisdiction is not "eu", or a refused create, fails
//     the step; there is no fallback. Writes wrangler.deploy.toml: wrangler.toml with the real
//     id and without [build], checked with check-deploy.mjs --deployed. --no-create (the
//     `check` action) fails instead of creating.
//   node scripts/deploy.mjs migrate    wrangler d1 migrations apply --remote
//   node scripts/deploy.mjs deploy     wrangler deploy (of the build the tokenless step made)
//   node scripts/deploy.mjs canary
//     Prints how many rows the fixed canary install id has in `report` and the canary tally id
//     has in `tally`, then deletes them, so the live check leaves nothing behind.
//
// CLOUDFLARE_API_TOKEN and the optional CLOUDFLARE_ACCOUNT_ID come from the environment (both
// secrets, which GitHub masks). An empty CLOUDFLARE_ACCOUNT_ID is dropped, so wrangler picks
// the one account the token can see.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { problems, PLACEHOLDER_ID } from "./check-deploy.mjs";

export const DATABASE = "clauseo-counts";
export const DEPLOY_CONFIG = "wrangler.deploy.toml";
/** The install id the live check's test machine is given before counts turn on. */
export const CANARY_INSTALL = "c0ffee00-0000-4000-8000-000000000001";
/** The tally id of the live check's hand-sent tally. */
export const CANARY_TALLY = "c0ffee00-0000-4000-8000-000000000002";
const BUILD_TABLE = '[build]\ncommand = "worker-build --release"\n\n';

/** A step's failure: its message is safe to print. */
export class Refused extends Error {}

/** `text` with every id, email and account name masked. */
export function redact(text) {
  return text
    .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, "<id>")
    .replace(/(?<![0-9a-z])[0-9a-f]{32}(?![0-9a-z])/gi, "<id>")
    .replace(/[^\s@`'"<>()[\]{},;:]+@[^\s@`'"<>()[\]{},;:]+/g, "<email>")
    .replace(/[^`\n:"(]*'s Account/gi, "<account>");
}

/**
 * The database's id from `wrangler d1 list --json`, or undefined when there is none. The API
 * reports `jurisdiction` for a database created with one
 * (https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/list/);
 * anything but "eu", a missing field included, is refused.
 * @param {string} listJson
 */
export function findDatabase(listJson) {
  let list;
  try {
    list = JSON.parse(listJson);
  } catch {
    throw new Refused("wrangler d1 list --json did not print JSON");
  }
  if (!Array.isArray(list)) throw new Refused("wrangler d1 list --json did not print a list");
  const matches = list.filter((db) => db?.name === DATABASE);
  if (matches.length > 1) throw new Refused(`more than one D1 database is named ${DATABASE}`);
  if (matches.length === 0) return undefined;
  const { uuid, jurisdiction } = matches[0];
  if (jurisdiction !== "eu") {
    throw new Refused(`${DATABASE} is not in the EU jurisdiction; delete it or fix it by hand`);
  }
  if (typeof uuid !== "string") throw new Refused(`${DATABASE} has no id in the list`);
  return uuid;
}

/**
 * Finds or creates the database, and returns its id and the deploy copy's text, checked.
 * @param {(args: string[]) => { status: number | null, stdout: string, stderr: string }} wrangler
 * @param {string} committed wrangler.toml's text
 * @param {{ create: boolean }} options
 */
export function deployConfig(wrangler, committed, { create }) {
  const list = () => {
    const run = wrangler(["d1", "list", "--json"]);
    if (run.status !== 0) throw new Refused(`wrangler d1 list failed:\n${redact(run.stderr)}`);
    return findDatabase(run.stdout);
  };
  let id = list();
  if (id === undefined) {
    if (!create) throw new Refused(`no D1 database is named ${DATABASE}`);
    const made = wrangler(["d1", "create", DATABASE, "--jurisdiction=eu"]);
    if (made.status !== 0) {
      throw new Refused(`wrangler d1 create ${DATABASE} --jurisdiction=eu was refused:\n${redact(made.stderr)}`);
    }
    id = list();
    if (id === undefined) throw new Refused(`${DATABASE} was created but is not in the list`);
  }
  const placeholder = `database_id = "${PLACEHOLDER_ID}"`;
  if (!committed.includes(placeholder) || !committed.includes(BUILD_TABLE)) {
    throw new Refused("wrangler.toml does not hold the placeholder database_id and [build]");
  }
  const copy = committed.replace(placeholder, `database_id = "${id}"`).replace(BUILD_TABLE, "");
  const found = problems(copy, { deployed: true });
  if (found.length > 0) throw new Refused(`the deploy copy fails check-deploy:\n${redact(found.join("\n"))}`);
  return { id, copy };
}

/** The canary's two statements: its row counts, then their deletion. */
export const CANARY_COUNT =
  `SELECT (SELECT COUNT(*) FROM report WHERE install = '${CANARY_INSTALL}') AS report, ` +
  `(SELECT COUNT(*) FROM tally WHERE tally = '${CANARY_TALLY}') AS tally`;
export const CANARY_DELETE =
  `DELETE FROM report WHERE install = '${CANARY_INSTALL}'; DELETE FROM tally WHERE tally = '${CANARY_TALLY}'`;

/**
 * The canary's counts from `wrangler d1 execute --json`'s output.
 * @param {string} output
 */
export function readCanary(output) {
  let parsed;
  try {
    parsed = JSON.parse(output);
  } catch {
    throw new Refused("wrangler d1 execute --json did not print JSON");
  }
  const row = parsed?.[0]?.results?.[0];
  const counts = { report: row?.report, tally: row?.tally };
  if (!Object.values(counts).every((n) => Number.isInteger(n) && n >= 0)) {
    throw new Refused("the canary counts are not whole numbers");
  }
  return counts;
}

/** The environment wrangler runs with: an empty CLOUDFLARE_ACCOUNT_ID is left out. */
export function wranglerEnv(env) {
  const out = { ...env, WRANGLER_SEND_METRICS: "false" };
  if (!out.CLOUDFLARE_ACCOUNT_ID) delete out.CLOUDFLARE_ACCOUNT_ID;
  return out;
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function runWrangler(args) {
  const run = spawnSync("pnpm", ["exec", "wrangler", ...args], {
    cwd: root,
    encoding: "utf8",
    env: wranglerEnv(process.env),
    maxBuffer: 16 * 1024 * 1024,
  });
  return { status: run.status, stdout: run.stdout ?? "", stderr: run.stderr ?? "" };
}

/** Runs wrangler and prints its output redacted; fails when wrangler does. */
function runShown(args) {
  const run = runWrangler(args);
  for (const text of [run.stdout, run.stderr]) if (text.trim() !== "") console.log(redact(text).trimEnd());
  if (run.status !== 0) throw new Refused(`wrangler ${args[0]} failed`);
}

function execute(sql) {
  const run = runWrangler(["d1", "execute", DATABASE, "--remote", "--json", "--config", DEPLOY_CONFIG, "--command", sql]);
  if (run.status !== 0) throw new Refused(`wrangler d1 execute failed:\n${redact(run.stderr)}`);
  return run.stdout;
}

function main([command, ...flags]) {
  if (command === "config") {
    const committed = readFileSync(join(root, "wrangler.toml"), "utf8");
    const { id, copy } = deployConfig(runWrangler, committed, { create: !flags.includes("--no-create") });
    console.log(`::add-mask::${id}`);
    writeFileSync(join(root, DEPLOY_CONFIG), copy);
    console.log(`${DEPLOY_CONFIG}: written and checked`);
  } else if (command === "migrate") {
    runShown(["d1", "migrations", "apply", DATABASE, "--remote", "--config", DEPLOY_CONFIG]);
  } else if (command === "deploy") {
    runShown(["deploy", "--config", DEPLOY_CONFIG]);
  } else if (command === "canary") {
    const counts = readCanary(execute(CANARY_COUNT));
    console.log(`canary report rows: ${counts.report}`);
    console.log(`canary tally rows: ${counts.tally}`);
    execute(CANARY_DELETE);
    console.log("canary rows deleted");
  } else {
    throw new Refused("usage: deploy.mjs config [--no-create] | migrate | deploy | canary");
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    if (!(error instanceof Refused)) throw error;
    console.error(`::error::${error.message.split("\n")[0]}`);
    const rest = error.message.split("\n").slice(1).join("\n");
    if (rest.trim() !== "") console.error(rest);
    process.exit(1);
  }
}
