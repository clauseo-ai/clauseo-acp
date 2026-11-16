#!/usr/bin/env node
// The counts-deploy workflow's skip (docs/counts-worker.md, "Deploying"): one sha256 over
// everything the deploy job ships or applies, and the hash of the last deploy, so a push that
// would ship the same Worker skips the deploy and its approval.
//
//   node scripts/bundle-hash.mjs files   the files hashed, one per line, relative to the repo
//                                        root (the artifact's list)
//   node scripts/bundle-hash.mjs hash    their sha256
//   node scripts/bundle-hash.mjs last    the hash the last deploy on main uploaded, or nothing
//     Reads the actions API with `gh api` (GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID from the
//     environment). Of the workflow's finished runs on main, it trusts only this repo's push and
//     workflow_dispatch runs: a pull request's run could upload a chosen hash. Of those runs'
//     `deploy` jobs that ran, the one that completed last put the live Worker up (a re-run of
//     an older run's deploy included): if it succeeded and its run holds the artifact
//     counts-worker-deployed, it prints the hash in it; otherwise nothing, so the job deploys.
//
// Every miss fails safe: a missing, expired or malformed hash, or an API error, means "deploy".

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { lstatSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Refused } from "./errors.mjs";

/** The Worker's folder, relative to the repo root. */
export const WORKER = "services/counts-worker";

/**
 * What the deploy job ships or applies, relative to the repo root: the built bundle, the config
 * the deploy copy is made from, the migrations, the scripts the deploy job runs, the lockfile
 * that pins wrangler (which bundles build/ again on upload), and the workflow itself, whose
 * deploy steps and their env decide what a deploy does. A directory stands for
 * every file under it. The deploy copy itself (wrangler.deploy.toml) is not here: it holds the
 * database's real id, which only a step with the token may see, and it is wrangler.toml with
 * that id put in, [build] taken out and the R2 binding kept only while the bucket exists.
 */
export const BUNDLE = [
  ".github/workflows/counts-deploy.yml",
  ...[
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
  ].map((entry) => `${WORKER}/${entry}`),
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

/** A run whose artifacts can be trusted: this repo's push or workflow_dispatch. */
export function trusted(run, repo) {
  return (run?.event === "push" || run?.event === "workflow_dispatch") && run?.head_repository?.full_name === repo;
}

/**
 * The hash the last deploy uploaded, or undefined. `api(path)` answers an actions API GET as
 * parsed JSON; `download(artifactId)` answers the artifact's hash file's text.
 * @param {(path: string) => any} api
 * @param {(artifactId: number) => string} download
 * @param {{ repo: string, runId: number, pages?: number }} where
 */
export function lastDeployed(api, download, { repo, runId, pages = 3 }) {
  // The deploy job that completed last, of every trusted run scanned.
  let newest;
  for (let page = 1; page <= pages; page++) {
    const runs = api(
      `repos/${repo}/actions/workflows/counts-deploy.yml/runs?branch=main&status=completed&per_page=30&page=${page}`,
    )?.workflow_runs;
    if (!Array.isArray(runs)) return undefined;
    for (const run of runs) {
      if (run.id === runId || !trusted(run, repo)) continue;
      const jobs = api(`repos/${repo}/actions/runs/${run.id}/jobs?filter=latest&per_page=100`)?.jobs;
      if (!Array.isArray(jobs)) return undefined;
      const deploy = jobs.find((job) => job.name === "deploy");
      if (!deploy || deploy.conclusion === "skipped") continue;
      const completed = Date.parse(deploy.completed_at);
      // A deploy that ran with no completion time: its Worker may be live, so trust nothing.
      if (Number.isNaN(completed)) return undefined;
      if (!newest || completed > newest.completed) newest = { run, deploy, completed };
    }
    if (runs.length < 30) break;
  }
  // The live Worker is the last deploy's; one that failed part way may have put it up.
  if (!newest || newest.deploy.conclusion !== "success") return undefined;
  const artifacts = api(`repos/${repo}/actions/runs/${newest.run.id}/artifacts?name=${ARTIFACT}`)?.artifacts;
  const found = (Array.isArray(artifacts) ? artifacts : [])
    .filter((a) => a.name === ARTIFACT && !a.expired)
    .sort((x, y) => Date.parse(y.created_at) - Date.parse(x.created_at))[0];
  if (!found) return undefined;
  const text = String(download(found.id)).trim();
  return HASH.test(text) ? text : undefined;
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
      const zip = spawnSync("gh", ["api", `repos/${repo}/actions/artifacts/${id}/zip`], { maxBuffer: 1024 * 1024 });
      if (zip.status !== 0) throw new Refused("gh api failed");
      writeFileSync(join(dir, "a.zip"), zip.stdout);
      const hash = spawnSync("unzip", ["-p", join(dir, "a.zip"), "hash"], { encoding: "utf8" });
      if (hash.status !== 0) throw new Refused("the artifact holds no hash");
      return hash.stdout;
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  };
  return lastDeployed(api, download, { repo, runId });
}

// The repo root: this file is services/counts-worker/scripts/bundle-hash.mjs.
const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

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
