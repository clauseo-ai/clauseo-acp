#!/usr/bin/env node
// The counts-deploy workflow's skip (docs/counts-worker.md, "Deploying"): one sha256 over
// everything the deploy job ships or applies, and the hash of the last deploy, so a push that
// would ship the same Worker skips the deploy and its approval.
//
//   node scripts/bundle-hash.mjs files   the files hashed, one per line (the artifact's list)
//   node scripts/bundle-hash.mjs hash    their sha256
//   node scripts/bundle-hash.mjs last    the hash the last deploy on main uploaded, or nothing
//     Reads the actions API with `gh api` (GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID from the
//     environment). It walks the workflow's finished runs on main, newest first, and stops at the
//     first whose `deploy` job ran: if that job succeeded and its run holds the artifact
//     counts-worker-deployed, it prints the hash in it; otherwise nothing, so the job deploys.
//
// Every miss fails safe: a missing, expired or malformed hash, or an API error, means "deploy".

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { lstatSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Refused } from "./errors.mjs";

/**
 * What the deploy job ships or applies, relative to services/counts-worker: the built bundle,
 * the config the deploy copy is made from, the migrations, the scripts the deploy job runs and
 * the lockfile that pins wrangler (which bundles build/ again on upload). A directory stands for
 * every file under it. The deploy copy itself (wrangler.deploy.toml) is not here: it holds the
 * database's real id, which only a step with the token may see, and it is wrangler.toml with
 * that id put in, [build] taken out and the R2 binding kept only while the bucket exists.
 */
export const BUNDLE = [
  "build",
  "migrations",
  "wrangler.toml",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "scripts/access.mjs",
  "scripts/bundle-hash.mjs",
  "scripts/check-deploy.mjs",
  "scripts/cloudflare.mjs",
  "scripts/deploy.mjs",
  "scripts/errors.mjs",
  "scripts/r2.mjs",
];

export const ARTIFACT = "counts-worker-deployed";
const HASH = /^[0-9a-f]{64}$/;

/**
 * Every file `entries` names under `root`, sorted. A hidden file under a directory (worker-build
 * writes build/.gitignore) is left out, as the artifact leaves it out; wrangler ships only what
 * build/worker/shim.mjs imports. A missing entry or a symlink fails.
 */
export function bundleFiles(root, entries = BUNDLE) {
  const out = [];
  const walk = (rel) => {
    let stat;
    try {
      stat = lstatSync(join(root, rel));
    } catch {
      throw new Refused(`${rel}: missing`);
    }
    if (stat.isDirectory()) {
      for (const child of readdirSync(join(root, rel))) {
        if (!child.startsWith(".")) walk(`${rel}/${child}`);
      }
    } else if (stat.isFile()) {
      out.push(rel);
    } else {
      throw new Refused(`${rel}: neither a file nor a directory`);
    }
  };
  for (const entry of entries) walk(entry);
  return [...new Set(out)].sort();
}

/** The sha256 of each file's path, size and bytes, in path order. */
export function bundleHash(root, files = bundleFiles(root)) {
  const hash = createHash("sha256");
  for (const rel of [...files].sort()) {
    const bytes = readFileSync(join(root, rel));
    hash.update(`${rel}\0${bytes.length}\0`);
    hash.update(bytes);
  }
  return hash.digest("hex");
}

/**
 * The hash the last deploy uploaded, or undefined. `api(path)` answers an actions API GET as
 * parsed JSON; `download(runId)` answers the artifact's hash file's text.
 * @param {(path: string) => any} api
 * @param {(runId: number) => string} download
 * @param {{ repo: string, runId: number, pages?: number }} where
 */
export function lastDeployed(api, download, { repo, runId, pages = 3 }) {
  for (let page = 1; page <= pages; page++) {
    const runs = api(
      `repos/${repo}/actions/workflows/counts-deploy.yml/runs?branch=main&status=completed&per_page=30&page=${page}`,
    )?.workflow_runs;
    if (!Array.isArray(runs) || runs.length === 0) return undefined;
    for (const run of runs) {
      if (run.id === runId) continue;
      const jobs = api(`repos/${repo}/actions/runs/${run.id}/jobs?filter=latest&per_page=100`)?.jobs;
      if (!Array.isArray(jobs)) return undefined;
      const deploy = jobs.find((job) => job.name === "deploy");
      if (!deploy || deploy.conclusion === "skipped") continue;
      // The deploy job ran: this run's Worker is the live one, unless the job failed part way.
      if (deploy.conclusion !== "success") return undefined;
      const artifacts = api(`repos/${repo}/actions/runs/${run.id}/artifacts?name=${ARTIFACT}`)?.artifacts;
      const found = Array.isArray(artifacts) && artifacts.find((a) => a.name === ARTIFACT && !a.expired);
      if (!found) return undefined;
      const text = download(run.id).trim();
      return HASH.test(text) ? text : undefined;
    }
  }
  return undefined;
}

function gh(args) {
  const run = spawnSync("gh", args, { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  if (run.status !== 0) throw new Refused(`gh ${args[0]} failed`);
  return run.stdout;
}

function last(env) {
  const repo = env.GITHUB_REPOSITORY;
  const runId = Number(env.GITHUB_RUN_ID);
  const api = (path) => JSON.parse(gh(["api", "-H", "Accept: application/vnd.github+json", path]));
  const download = (id) => {
    const dir = mkdtempSync(join(tmpdir(), "deployed-"));
    try {
      gh(["run", "download", String(id), "--repo", repo, "--name", ARTIFACT, "--dir", dir]);
      return readFileSync(join(dir, "hash"), "utf8");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  };
  return lastDeployed(api, download, { repo, runId });
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  try {
    if (command === "files") console.log(bundleFiles(root).join("\n"));
    else if (command === "hash") console.log(bundleHash(root));
    else if (command === "last") {
      let found;
      try {
        found = last(process.env);
      } catch (error) {
        // Fail safe: no hash means the job deploys.
        console.error(`no last hash (${error instanceof Refused ? error.message : error?.name})`);
      }
      if (found) console.log(found);
    } else throw new Refused("usage: bundle-hash.mjs files | hash | last");
  } catch (error) {
    console.error(`::error::${error instanceof Refused ? error.message : "bundle-hash.mjs failed"}`);
    process.exit(1);
  }
}
