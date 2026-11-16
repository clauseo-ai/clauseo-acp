// node --test scripts/bundle-hash.test.mjs: the deploy skip's hash changes with any shipped byte
// and with nothing else, and the last deploy's hash is found only where it is safe to trust.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ARTIFACT, BUNDLE, WORKER, bundleFiles, bundleHash, lastDeployed } from "./bundle-hash.mjs";

/** `files` with each path made relative to the repo root. */
function at(files) {
  return Object.fromEntries(
    Object.entries(files).map(([rel, text]) => [rel.startsWith("/") ? rel.slice(1) : `${WORKER}/${rel}`, text]),
  );
}

/** A repo holding every BUNDLE entry, with `files` (relative to the Worker's folder, or to the
 * repo root when they start with "/") written over it. */
function folder(t, files = {}) {
  const root = mkdtempSync(join(tmpdir(), "bundle-hash-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const all = {
    ...Object.fromEntries(BUNDLE.filter((e) => /\.[a-z]+$/.test(e)).map((e) => [e, `${e}\n`])),
    ...at({
      "build/worker/shim.mjs": "export default {}",
      "build/index.wasm": "\0asm",
      "migrations/0001_init.sql": "CREATE TABLE report (install TEXT);",
      "migrations/0002_rollup.sql": "CREATE TABLE tally (tally TEXT);",
      ...files,
    }),
  };
  for (const [rel, text] of Object.entries(all)) {
    mkdirSync(join(root, rel, ".."), { recursive: true });
    writeFileSync(join(root, rel), text);
  }
  return root;
}

test("the same files give the same hash, and the list is sorted and complete", (t) => {
  const a = folder(t);
  const b = folder(t);
  assert.equal(bundleHash(a), bundleHash(b));
  const files = bundleFiles(a);
  assert.deepEqual(files, [...files].sort());
  for (const rel of ["build/worker/shim.mjs", "migrations/0002_rollup.sql", "wrangler.toml", "scripts/deploy.mjs"]) {
    assert.ok(files.includes(`${WORKER}/${rel}`), rel);
  }
  assert.ok(files.includes(".github/workflows/counts-deploy.yml"));
  assert.ok(!files.some((f) => f.endsWith(".test.mjs")));
});

test("every script the deploy job runs, and all they import, is hashed", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const seen = new Set();
  const visit = (name) => {
    if (seen.has(name)) return;
    seen.add(name);
    const text = readFileSync(join(here, name), "utf8");
    for (const [, dep] of text.matchAll(/from "\.\/([\w-]+\.mjs)"/g)) visit(dep);
  };
  // The scripts counts-deploy.yml runs; check-access.mjs runs on Rohan's Mac, not in the job.
  for (const name of ["deploy.mjs", "check-deploy.mjs", "bundle-hash.mjs"]) visit(name);
  assert.ok(seen.has("access.mjs") && seen.has("r2.mjs") && seen.has("cloudflare.mjs"));
  for (const name of seen) assert.ok(BUNDLE.includes(`${WORKER}/scripts/${name}`), name);
  assert.ok(!BUNDLE.includes(`${WORKER}/scripts/check-access.mjs`));
});

test("the order the files are given in does not matter", (t) => {
  const root = folder(t);
  const files = bundleFiles(root);
  assert.equal(bundleHash(root, [...files].reverse()), bundleHash(root, files));
  assert.deepEqual(bundleFiles(root, [...BUNDLE].reverse()), files);
});

test("a changed, added or renamed migration changes the hash", (t) => {
  const base = bundleHash(folder(t));
  assert.notEqual(bundleHash(folder(t, { "migrations/0002_rollup.sql": "CREATE TABLE tally (n INT);" })), base);
  assert.notEqual(bundleHash(folder(t, { "migrations/0005_more.sql": "" })), base);
  const renamed = folder(t);
  rmSync(join(renamed, WORKER, "migrations/0002_rollup.sql"));
  writeFileSync(join(renamed, WORKER, "migrations/0003_rollup.sql"), "CREATE TABLE tally (tally TEXT);");
  assert.notEqual(bundleHash(renamed), base);
});

test("a changed bundle, config, deploy script or workflow changes the hash", (t) => {
  const base = bundleHash(folder(t));
  const changed = ["build/index.wasm", "wrangler.toml", "scripts/deploy.mjs", "pnpm-lock.yaml"];
  for (const rel of [...changed, "/.github/workflows/counts-deploy.yml"]) {
    assert.notEqual(bundleHash(folder(t, { [rel]: "changed" })), base, rel);
  }
});

test("bytes moved between two files' names and contents change the hash", (t) => {
  const a = folder(t, { "migrations/0001_init.sql": "ab", "migrations/0002_rollup.sql": "c" });
  const b = folder(t, { "migrations/0001_init.sql": "a", "migrations/0002_rollup.sql": "bc" });
  assert.notEqual(bundleHash(a), bundleHash(b));
});

test("a hidden file is left out, as the artifact leaves it out", (t) => {
  const plain = folder(t);
  const hidden = folder(t, { "build/.gitignore": "*" });
  assert.ok(!bundleFiles(hidden).includes(`${WORKER}/build/.gitignore`));
  assert.equal(bundleHash(hidden), bundleHash(plain));
});

test("a missing entry or a symlink fails", (t) => {
  const missing = folder(t);
  rmSync(join(missing, WORKER, "scripts/r2.mjs"));
  assert.throws(() => bundleFiles(missing), /scripts\/r2\.mjs: missing/);
  const linked = folder(t);
  symlinkSync("/etc/hostname", join(linked, WORKER, "build/link"));
  assert.throws(() => bundleFiles(linked), /neither/);
});

const HASH = "a".repeat(64);
const OLD = "b".repeat(64);
const REPO = "o/r";

/**
 * An API over `runs`, newest created first: { id, deploy (its conclusion, or none), done (its
 * completed_at minute), hash (the artifact's), event, repo }. Downloads return the artifact's
 * hash by artifact id (the run id here).
 */
function api(runs, { expired = false } = {}) {
  const full = runs.map((r) => ({ event: "push", repo: REPO, done: r.id, ...r }));
  const call = (path) => {
    if (path.includes("/workflows/")) {
      const page = Number(path.match(/page=(\d+)$/)[1]);
      const list = page === 1 ? full : [];
      return {
        workflow_runs: list.map((r) => ({ id: r.id, event: r.event, head_repository: { full_name: r.repo } })),
      };
    }
    const run = full.find((r) => r.id === Number(path.match(/runs\/(\d+)\//)[1]));
    call.asked.push(path);
    if (path.includes("/jobs")) {
      const jobs = [{ name: "bundle", conclusion: "success" }];
      if (run.deploy) {
        const at = new Date(Date.UTC(2026, 10, 15, 0, run.done)).toISOString();
        jobs.push({ name: "deploy", conclusion: run.deploy, completed_at: at });
      }
      return { jobs };
    }
    return {
      artifacts: run.hash ? [{ id: run.id, name: ARTIFACT, expired, created_at: "2026-11-15T00:00:00Z" }] : [],
    };
  };
  call.asked = [];
  call.download = (id) => `${full.find((r) => r.id === id).hash}\n`;
  return call;
}
const where = { repo: REPO, runId: 99 };
const last = (runs, options) => {
  const a = api(runs, options);
  return lastDeployed(a, a.download, where);
};

test("the newest deploy gives the hash; runs whose deploy was skipped are passed over", () => {
  const runs = [{ id: 99 }, { id: 5, deploy: "skipped" }, { id: 4 }, { id: 3, deploy: "success", hash: HASH },
    { id: 2, deploy: "success", hash: OLD }];
  assert.equal(last(runs), HASH);
});

test("only this repo's push and workflow_dispatch runs are trusted", () => {
  const good = { id: 2, deploy: "success", hash: OLD };
  for (const bad of [{ event: "pull_request" }, { event: "pull_request_target" }, { repo: "fork/r" },
    { event: "workflow_dispatch", repo: "fork/r" }]) {
    const a = api([{ id: 4, deploy: "success", hash: HASH, ...bad }, good]);
    assert.equal(lastDeployed(a, a.download, where), OLD, JSON.stringify(bad));
    assert.ok(!a.asked.some((p) => p.includes("/runs/4/")), "the untrusted run is not even read");
    // With no good run behind it: none, so the job deploys.
    assert.equal(last([{ id: 4, deploy: "success", hash: HASH, ...bad }]), undefined);
  }
  assert.equal(last([{ id: 4, deploy: "success", hash: HASH, event: "workflow_dispatch" }]), HASH);
});

test("the deploy that completed last wins, a re-run of an older run's included", () => {
  // Run 3 was created after run 2, but run 2's deploy was re-run and completed later.
  const runs = [{ id: 3, deploy: "success", hash: HASH, done: 10 }, { id: 2, deploy: "success", hash: OLD, done: 20 }];
  assert.equal(last(runs), OLD);
  // A failure that completed last means deploy, even with an older success.
  assert.equal(last([{ id: 3, deploy: "success", hash: HASH, done: 10 }, { id: 2, deploy: "failure", done: 20 }]),
    undefined);
});

test("a failed or cancelled deploy, or one with no hash, means deploy again", () => {
  for (const deploy of ["failure", "cancelled"]) {
    assert.equal(last([{ id: 4, deploy }, { id: 3, deploy: "success", hash: HASH }]), undefined, deploy);
  }
  assert.equal(last([{ id: 4, deploy: "success" }, { id: 3, deploy: "success", hash: HASH }]), undefined);
  assert.equal(last([{ id: 3, deploy: "success", hash: HASH }], { expired: true }), undefined);
  assert.equal(last([{ id: 3, deploy: "success", hash: "not a hash" }]), undefined);
  assert.equal(last([{ id: 5, deploy: "skipped" }]), undefined);
  assert.equal(lastDeployed(() => ({}), () => HASH, where), undefined);
});
