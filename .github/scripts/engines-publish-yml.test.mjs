// The rules engines-publish.yml must keep, read as text, and a mutated copy per rule that the lint
// must reject. Plain Node, no install:
//
//   node --test .github/scripts/engines-publish-yml.test.mjs
//
// workflow_dispatch only; `plan` with no environment, no secret and contents: read; `publish` the
// only job with an environment (by lane), contents: write, re-checking plan's sha256 and the live
// list right before signing; the key only in the probe and sign steps, where no pnpm runs, after an
// install with --ignore-scripts; every `uses:` pinned by a full SHA; no ${{ }} inside a run:;
// --prerelease --latest=false; no --app-version; no credentials kept by checkout; the fork guard and
// the ref check on both jobs; each upload retried.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const YML = readFileSync(join(ROOT, ".github/workflows/engines-publish.yml"), "utf8");

/** The text of job `name`, up to the next job. */
function job(yml, name) {
  const jobs = yml.slice(yml.indexOf("\njobs:\n"));
  const at = jobs.indexOf(`\n  ${name}:\n`);
  assert.ok(at >= 0, `job ${name}`);
  const rest = jobs.slice(at + 1);
  const end = rest.slice(1).search(/\n {2}[a-z][\w-]*:\n/);
  return end < 0 ? rest : rest.slice(0, end + 1);
}
const steps = (text) => text.slice(text.indexOf("\n    steps:\n")).split(/\n(?= {6}- )/).slice(1);

/** Throws on the first rule `yml` breaks. */
function lint(yml) {
  assert.match(yml, /^# /, "a header comment first");
  const code = yml.replace(/^\s*#.*\n/gm, "");
  const header = yml.slice(0, yml.indexOf("\nname:"));
  for (const want of [/deployments from `main`/, /release-test` must get a required reviewer/, /`v\*` tags must be limited to the owner by a ruleset/, /BEFORE APPROVING publish[\s\S]*ref is\n# refs\/heads\/main; the commit is on main; the pins/]) {
    assert.match(header, want);
  }
  // workflow_dispatch only, with the three inputs.
  const on = yml.slice(yml.indexOf("\non:\n"), yml.indexOf("\npermissions:"));
  assert.deepEqual([...on.matchAll(/^ {2}([a-z_]+):/gm)].map((m) => m[1]), ["workflow_dispatch"]);
  assert.match(on, /lane:\n(?:\s+.*\n)*?\s+options: \[engines, engines-test\]\n\s+default: engines/);
  assert.match(on, /rollback_to:/);
  assert.match(on, /apps:/);
  assert.doesNotMatch(yml, /^\s+(push|pull_request|pull_request_target|schedule|workflow_run):/m);
  for (const m of yml.matchAll(/^\s*(?:-\s*)?uses:\s*(\S+)/gm)) assert.match(m[1], /^[^@]+@[0-9a-f]{40}$/, `${m[1]} is not pinned by a full SHA`);
  assert.match(yml, /^permissions: \{\}$/m);
  assert.match(yml, /concurrency:\n {2}group: engines-publish-\$\{\{ inputs\.lane \}\}\n {2}cancel-in-progress: false/);

  const jobNames = [...yml.slice(yml.indexOf("\njobs:\n")).matchAll(/^ {2}([a-z][\w-]*):$/gm)].map((m) => m[1]);
  assert.deepEqual(jobNames, ["plan", "publish"]);
  const plan = job(yml, "plan");
  const publish = job(yml, "publish");
  for (const j of [plan, publish]) {
    assert.match(j, /if: github\.repository == 'clauseo-ai\/clauseo-acp' && github\.ref == 'refs\/heads\/main'/);
    assert.match(j, /persist-credentials: false/);
    const granted = [...j.matchAll(/^ {6}([a-z-]+): (write|read)$/gm)].map((m) => `${m[1]}: ${m[2]}`);
    assert.deepEqual(granted, [j === plan ? "contents: read" : "contents: write"]);
  }
  // plan: no environment and no secret; it hands on the file and its sha256.
  assert.doesNotMatch(plan, /environment:|secrets\./);
  assert.match(plan, /sha256: \$\{\{ steps\.plan\.outputs\.sha256 \}\}/);
  assert.match(plan, /engines-publish\.mjs --plan/);
  assert.match(plan, /upload-artifact@/);
  // publish: the only environment, by lane.
  assert.equal([...yml.matchAll(/^\s+environment:/gm)].length, 1);
  assert.match(publish, /needs: plan\n/);
  assert.match(publish, /environment: \$\{\{ inputs\.lane == 'engines-test' && 'release-test' \|\| 'release' \}\}/);
  assert.match(publish, /PLANNED_SHA256: \$\{\{ needs\.plan\.outputs\.sha256 \}\}/);
  assert.match(plan, /live_sha256: \$\{\{ steps\.plan\.outputs\.live_sha256 \}\}/);
  assert.match(publish, /PLANNED_LIVE_SHA256: \$\{\{ needs\.plan\.outputs\.live_sha256 \}\}/);
  assert.match(plan, /floor_seq: \$\{\{ steps\.plan\.outputs\.floor_seq \}\}/);
  assert.match(publish, /PLANNED_FLOOR: \$\{\{ needs\.plan\.outputs\.floor_seq \}\}/);

  const ps = steps(publish);
  const index = (pattern) => ps.findIndex((s) => pattern.test(s));
  const [download, recheck, install, probe, liveCheck, signing, verify, upload] = [
    /download-artifact@/, /\[ "\$got" = "\$PLANNED_SHA256" \] \|\| fail/, /pnpm -C ui install /,
    /name: The lane's key signs/, /engines-publish\.mjs --live-check/, /name: Sign engines\.json/, /name: Verify the signature/, /gh release upload/,
  ].map(index);
  assert.ok(download >= 0 && download < recheck && recheck < install && install < probe && probe < liveCheck && liveCheck === signing - 1 && signing < verify && verify < upload, "publish's order: the live check right before signing");
  assert.doesNotMatch(ps[liveCheck], /secrets\.|TAURI_SIGNING|continue-on-error|if:/);
  // Before the key, only the Tauri CLI, with no install script; no pnpm where the key is.
  assert.match(ps[install], /run: pnpm -C ui install --frozen-lockfile --ignore-scripts --filter \.\n/);
  assert.equal(ps.filter((s) => /pnpm [^\n]*install/.test(s)).length, 1);
  for (const s of ps.filter((s) => /TAURI_SIGNING_PRIVATE_KEY/.test(s))) assert.doesNotMatch(s, /\bpnpm\b|npx|npm /, s);
  // The key: the probe and the sign step only, through env:, by lane.
  const withKey = ps.filter((s) => /secrets\.|TAURI_SIGNING_PRIVATE_KEY/.test(s)).map((s) => /name: (.*)/.exec(s)[1]);
  assert.deepEqual(withKey, ["The lane's key signs, and the lane's public key verifies it", "Sign engines.json"]);
  for (const i of [probe, signing]) {
    assert.match(ps[i], /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ inputs\.lane == 'engines-test' && secrets\.TAURI_SIGNING_PRIVATE_KEY_TEST \|\| inputs\.lane == 'engines' && secrets\.TAURI_SIGNING_PRIVATE_KEY \|\| '' \}\}/);
    assert.match(ps[i], /TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \$\{\{ inputs\.lane == 'engines-test' && secrets\.TAURI_SIGNING_PRIVATE_KEY_TEST_PASSWORD \|\| inputs\.lane == 'engines' && secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD \|\| '' \}\}/);
  }
  assert.match(ps[probe], /latest-json\.mjs --verify "\$dir\/\$name" "\$RUNNER_TEMP\/lane\.pub"/);
  // Sign exactly the planned file, once, and nothing else.
  assert.deepEqual([...ps[signing].matchAll(/signer sign (.*)/g)].map((m) => m[0]), ['signer sign "$RUNNER_TEMP/engines/engines.json" > /dev/null']);
  for (const i of [probe, signing]) assert.match(ps[i], /\n {10}node ui\/node_modules\/@tauri-apps\/cli\/tauri\.js signer sign /);
  assert.match(ps[verify], /\[ "\$got" = "\$PLANNED_SHA256" \]/);
  assert.match(ps[verify], /latest-json\.mjs --verify "\$RUNNER_TEMP\/engines\/engines\.json" "\$RUNNER_TEMP\/lane\.pub"/);
  assert.match(ps[verify], /engines-publish\.mjs --check-signature/);
  assert.doesNotMatch(code, /--app-version/);
  // The release: a pre-release never latest; the archive, then the .sig, then the list.
  const up = ps[upload];
  assert.match(up, /gh release create "\$LANE" [^\n]*--prerelease --latest=false --target main/);
  assert.doesNotMatch(code, /--latest(?!=false)|make_latest|gh release edit/);
  const order = ['up "$arc/$LANE-$SEQ.json" "$arc/$LANE-$SEQ.json.sig"\n', 'up "$src/engines.json.sig"\n', 'up "$src/engines.json"\n'].map((s) => up.indexOf(s));
  assert.ok(order.every((i) => i > 0) && order[0] < order[1] && order[1] < order[2], "upload order");
  // Each upload tried 3 times, with a backoff.
  assert.match(up, /for try in 1 2 3; do\n\s+gh release upload "\$LANE" "\$@" --repo "\$GITHUB_REPOSITORY" --clobber && return 0\n[\s\S]*?sleep \$\(\(try \* 10\)\)/);
  assert.equal([...up.matchAll(/gh release upload/g)].length, 1);
  assert.match(up, /GH_TOKEN: \$\{\{ github\.token \}\}/);
  assert.doesNotMatch(code, /--no-verify|--force/);
  // No expression inside a run: script, where its text would be run as shell.
  const runs = [...yml.matchAll(/^(\s+)run: (\|\n(?:\1 {2}.*\n|\n)*|.*\n)/gm)].map((m) => m[2]);
  assert.ok(runs.length >= 7);
  for (const run of runs) assert.doesNotMatch(run, /\$\{\{/, run);
}

test("engines-publish.yml keeps every rule", () => lint(YML));

/** YML with the `nth` (or, when negative, the last) occurrence of `from` replaced by `to`. */
function mutate(from, to, nth = 0) {
  const at = [];
  for (let i = YML.indexOf(from); i >= 0; i = YML.indexOf(from, i + 1)) at.push(i);
  assert.ok(at.length > 0, `the mutation's text is in the file: ${from}`);
  const i = nth < 0 ? at.at(nth) : at[nth];
  return YML.slice(0, i) + to + YML.slice(i + from.length);
}

const KEY = "${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}";
const MUTATIONS = {
  "an environment on plan": mutate("    permissions:\n      contents: read\n", "    environment: release\n    permissions:\n      contents: read\n"),
  "plan's permissions widened": mutate("      contents: read\n", "      contents: write\n"),
  "publish's permissions widened": mutate("      contents: write\n", "      contents: write\n      actions: write\n"),
  "an unpinned action": mutate("actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1", "actions/setup-node@v7"),
  "an expression in a run:": mutate('--plan "$RUNNER_TEMP/engines"', '--plan "${{ runner.temp }}/engines"'),
  "the key in another step's env": mutate("      - name: Upload to the release\n        env:\n", `      - name: Upload to the release\n        env:\n          TAURI_SIGNING_PRIVATE_KEY: ${KEY}\n`),
  "the key in the live check's env": mutate("      - name: The live list is still the one plan saw\n", `      - name: The live list is still the one plan saw\n        env:\n          TAURI_SIGNING_PRIVATE_KEY: ${KEY}\n`),
  "plan's ref check removed": mutate(" && github.ref == 'refs/heads/main'", ""),
  "publish's ref check removed": mutate(" && github.ref == 'refs/heads/main'", "", -1),
  "install scripts run": mutate("--frozen-lockfile --ignore-scripts --filter .", "--frozen-lockfile --filter ."),
  "the whole workspace installed": mutate("--ignore-scripts --filter .\n", "--ignore-scripts\n"),
  "pnpm with the key (sign)": mutate('node ui/node_modules/@tauri-apps/cli/tauri.js signer sign "$RUNNER_TEMP', 'pnpm -s -C ui tauri signer sign "$RUNNER_TEMP'),
  "pnpm with the key (probe)": mutate('node ui/node_modules/@tauri-apps/cli/tauri.js signer sign "$dir', 'pnpm -s -C ui tauri signer sign "$dir'),
  "a push trigger": mutate("on:\n  workflow_dispatch:\n", "on:\n  push:\n    branches: [main]\n  workflow_dispatch:\n"),
  "no --latest=false": mutate("--prerelease --latest=false", "--prerelease"),
  "no sha re-check": mutate('[ "$got" = "$PLANNED_SHA256" ] || fail', "true || fail"),
  "--app-version": mutate('signer sign "$RUNNER_TEMP', 'signer sign --app-version 0.1.0 "$RUNNER_TEMP'),
  "persist-credentials: true": mutate("persist-credentials: false", "persist-credentials: true", -1),
  "no live check before signing": mutate("        run: node .github/scripts/engines-publish.mjs --live-check\n", "        run: echo skipped\n"),
  "no upload retry": mutate("for try in 1 2 3; do", "for try in 1; do"),
};

for (const [name, yml] of Object.entries(MUTATIONS)) {
  test(`the lint rejects engines-publish.yml with ${name}`, () => {
    assert.notEqual(yml, YML);
    assert.throws(() => lint(yml), assert.AssertionError);
  });
}
