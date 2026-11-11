#!/usr/bin/env node
// The counts-deploy job's two steps that talk to D1 through wrangler (docs/counts-worker.md,
// "Deploying"). Run logs on this public repo are public, so neither prints what wrangler
// prints on success: no database id, no row, no total.
//
//   node scripts/deploy.mjs config [--no-create]
//     Finds the D1 database clauseo-counts by name (`wrangler d1 list --json`), or creates it in
//     the EU jurisdiction. A refused create fails the step; there is no fallback to another
//     jurisdiction. Writes wrangler.deploy.toml (wrangler.toml with the real id) and checks it
//     with check-deploy.mjs --deployed. --no-create (the `check` action) fails instead of
//     creating.
//
//   INSTALL=<id> | TALLY=<id> node scripts/deploy.mjs check-id
//     Prints how many rows hold that one install id (in `report`) or tally id (in `tally`): a
//     count per id, never a row, a total or another id.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { problems, PLACEHOLDER_ID } from "./check-deploy.mjs";

export const DATABASE = "clauseo-counts";
export const DEPLOY_CONFIG = "wrangler.deploy.toml";
const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

/** A step's failure: its message is safe to print. */
export class Refused extends Error {}

/**
 * The database's id from `wrangler d1 list --json`, or undefined when there is none.
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
  if (jurisdiction !== undefined && jurisdiction !== null && jurisdiction !== "eu") {
    throw new Refused(`${DATABASE} is not in the EU jurisdiction; delete it or fix it by hand`);
  }
  if (typeof uuid !== "string") throw new Refused(`${DATABASE} has no id in the list`);
  return uuid;
}

/**
 * Finds or creates the database, and returns the deploy copy's text, checked.
 * @param {(args: string[]) => { status: number | null, stdout: string, stderr: string }} wrangler
 * @param {string} committed wrangler.toml's text
 * @param {{ create: boolean }} options
 */
export function deployConfig(wrangler, committed, { create }) {
  const list = () => {
    const run = wrangler(["d1", "list", "--json"]);
    if (run.status !== 0) throw new Refused(`wrangler d1 list failed:\n${run.stderr}`);
    return findDatabase(run.stdout);
  };
  let id = list();
  if (id === undefined) {
    if (!create) throw new Refused(`no D1 database is named ${DATABASE}`);
    const made = wrangler(["d1", "create", DATABASE, "--jurisdiction=eu"]);
    if (made.status !== 0) {
      throw new Refused(`wrangler d1 create ${DATABASE} --jurisdiction=eu was refused:\n${made.stderr}`);
    }
    id = list();
    if (id === undefined) throw new Refused(`${DATABASE} was created but is not in the list`);
  }
  if (!committed.includes(`database_id = "${PLACEHOLDER_ID}"`)) {
    throw new Refused("wrangler.toml does not hold the placeholder database_id");
  }
  const copy = committed.replace(`database_id = "${PLACEHOLDER_ID}"`, `database_id = "${id}"`);
  const found = problems(copy, { deployed: true });
  if (found.length > 0) throw new Refused(`the deploy copy fails check-deploy:\n${found.join("\n")}`);
  return copy;
}

/**
 * The one `SELECT COUNT(*)` for the id given, after checking it is a UUID v4 (it is put in
 * the SQL, as `wrangler d1 execute` binds no parameters).
 * @param {{ INSTALL?: string, TALLY?: string }} env
 */
export function countQuery(env) {
  const install = env.INSTALL?.trim() ?? "";
  const tally = env.TALLY?.trim() ?? "";
  if ((install === "") === (tally === "")) {
    throw new Refused("give exactly one of the inputs install and tally");
  }
  const [table, column, id] = install !== "" ? ["report", "install", install] : ["tally", "tally", tally];
  if (!UUID_V4.test(id)) throw new Refused(`the ${column} id is not a lowercase UUID v4`);
  return { column, sql: `SELECT COUNT(*) AS n FROM ${table} WHERE ${column} = '${id}'` };
}

/**
 * The count from `wrangler d1 execute --json`'s output.
 * @param {string} output
 */
export function readCount(output) {
  let parsed;
  try {
    parsed = JSON.parse(output);
  } catch {
    throw new Refused("wrangler d1 execute --json did not print JSON");
  }
  const n = parsed?.[0]?.results?.[0]?.n;
  if (!Number.isInteger(n) || n < 0 || n > 1) throw new Refused("the count is not 0 or 1");
  return n;
}

function runWrangler(args) {
  const run = spawnSync("pnpm", ["exec", "wrangler", ...args], {
    cwd: join(dirname(fileURLToPath(import.meta.url)), ".."),
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  return { status: run.status, stdout: run.stdout ?? "", stderr: run.stderr ?? "" };
}

function main([command, ...flags]) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  if (command === "config") {
    const committed = readFileSync(join(root, "wrangler.toml"), "utf8");
    const copy = deployConfig(runWrangler, committed, { create: !flags.includes("--no-create") });
    writeFileSync(join(root, DEPLOY_CONFIG), copy);
    console.log(`${DEPLOY_CONFIG}: written and checked`);
  } else if (command === "check-id") {
    const { column, sql } = countQuery(process.env);
    const run = runWrangler([
      "d1", "execute", DATABASE, "--remote", "--json", "--config", DEPLOY_CONFIG, "--command", sql,
    ]);
    if (run.status !== 0) throw new Refused("wrangler d1 execute failed");
    console.log(`rows with that ${column} id: ${readCount(run.stdout)}`);
  } else {
    throw new Refused("usage: deploy.mjs config [--no-create] | check-id");
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    if (!(error instanceof Refused)) throw error;
    console.error(`::error::${error.message}`);
    process.exit(1);
  }
}
