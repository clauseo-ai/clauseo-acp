// Tests for engine-watch.mjs, and the rules engine-watch.yml must keep. Plain Node, no install:
//
//   node --test .github/scripts/engine-watch.test.mjs
//
// Nothing here reaches the network: the registry and the published engines.json are stand-ins.
// The real measure is `node .github/scripts/engine-watch.mjs --check-sizes`, which re-measures
// today's lock from the registry and must reproduce embedded.rs's SIZES exactly.
//
// engine-watch.yml is read as text: every `uses:` pinned by a 40-character SHA, the job's
// permissions no more than contents: write, the 6-hour schedule plus
// workflow_dispatch, the push guarded by a tree comparison, and a refused PR not failing the job.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import * as w from "./engine-watch.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const read = (path) => readFileSync(join(ROOT, path), "utf8");

test("versions compare by semver precedence", () => {
  assert.ok(w.compareVersions("0.110.1", "0.89.1") > 0);
  assert.ok(w.compareVersions("2.10.0", "2.2.2") > 0);
  assert.ok(w.compareVersions("0.171.0-alpha.13", "0.171.0") < 0);
  assert.ok(w.compareVersions("1.0.0-alpha.10", "1.0.0-alpha.9") > 0);
  assert.equal(w.compareVersions("1.2.3", "1.2.3"), 0);
});

test("ranges: caret on 0.x, tilde, comparators, x ranges, alternatives", () => {
  assert.ok(w.satisfies("0.171.5", "^0.171.0"));
  assert.ok(!w.satisfies("0.172.0", "^0.171.0"));
  assert.ok(w.satisfies("1.9.0", "^1.5.0"));
  assert.ok(!w.satisfies("2.0.0", "^1.5.0"));
  assert.ok(w.satisfies("0.0.3", "^0.0.3") && !w.satisfies("0.0.4", "^0.0.3"));
  assert.ok(w.satisfies("2.9.4", "~2.9.1") && !w.satisfies("2.10.0", "~2.9.1"));
  assert.ok(w.satisfies("0.93.0", ">=0.93.0") && !w.satisfies("0.92.9", ">=0.93.0"));
  assert.ok(w.satisfies("1.4.0", ">=1.0.0 <2.0.0") && !w.satisfies("2.0.0", ">=1.0.0 <2.0.0"));
  assert.ok(w.satisfies("4.6.7", "4.x") && w.satisfies("4.6.7", "*"));
  assert.ok(w.satisfies("3.0.0", "^1.0.0 || ^3.0.0"));
  assert.ok(w.satisfies("0.162.1", "0.162.1") && !w.satisfies("0.162.2", "0.162.1"));
  assert.ok(!w.satisfies("0.171.0-alpha.1", "^0.171.0"));
  assert.equal(w.maxSatisfying(["0.171.0", "0.171.2", "0.171.3-alpha.1", "0.172.0", "0.171.1-linux-x64"], "^0.171.0"), "0.171.2");
});

const LOCK = {
  "": { dependencies: { a: "1.0.0", b: "1.0.0" } },
  "node_modules/a": { version: "1.0.0", resolved: `${w.REGISTRY}a/-/a-1.0.0.tgz`, dependencies: { c: "1" }, optionalDependencies: { "a-linux": "1", "a-win": "1" }, peerDependencies: { p: "1", q: "1" }, peerDependenciesMeta: { q: { optional: true } } },
  "node_modules/a/node_modules/c": { version: "2.0.0", resolved: `${w.REGISTRY}c/-/c-2.0.0.tgz` },
  "node_modules/c": { version: "1.0.0", resolved: `${w.REGISTRY}c/-/c-1.0.0.tgz` },
  "node_modules/a-linux": { version: "1.0.0", resolved: `${w.REGISTRY}a-linux/-/a-linux-1.0.0.tgz`, os: ["linux"], cpu: ["x64"], libc: ["glibc"], optional: true },
  "node_modules/a-win": { name: "a", version: "1.0.0-win32", resolved: `${w.REGISTRY}a/-/a-1.0.0-win32.tgz`, os: ["win32"], optional: true },
  "node_modules/p": { version: "1.0.0", resolved: `${w.REGISTRY}p/-/p-1.0.0.tgz`, dependencies: { c: "1" } },
  "node_modules/b": { version: "1.0.0", resolved: `${w.REGISTRY}b/-/b-1.0.0.tgz` },
};

test("reachable follows pins.rs: nested lookup first, peers, a missing optional peer skipped", () => {
  assert.deepEqual(w.reachable(LOCK, "a"), [
    "node_modules/a",
    "node_modules/a-linux",
    "node_modules/a-win",
    "node_modules/a/node_modules/c",
    "node_modules/c",
    "node_modules/p",
  ]);
  const broken = structuredClone(LOCK);
  delete broken["node_modules/p"];
  assert.throws(() => w.reachable(broken, "a"), /p, a dependency of node_modules\/a/);
  assert.equal(w.entryName("node_modules/a-win", LOCK["node_modules/a-win"]), "a");
  assert.equal(w.entryName("node_modules/a/node_modules/c", {}), "c");
});

test("platform rule: os, cpu, Linux as glibc, negations", () => {
  const linux = LOCK["node_modules/a-linux"];
  assert.ok(w.installsOn(linux, "Linux", "X64"));
  assert.ok(!w.installsOn(linux, "Linux", "Arm64"));
  assert.ok(!w.installsOn({ ...linux, libc: ["musl"] }, "Linux", "X64"));
  assert.ok(w.installsOn({ os: ["darwin"], libc: ["musl"] }, "Darwin", "Arm64"));
  assert.ok(!w.installsOn({ os: ["!win32"] }, "Win32", "X64"));
});

test("shortHash is integrity.rs's: today's lock hashes to SIZES_STAMP", () => {
  const { stamp } = w.parseSizes(read(w.EMBEDDED));
  assert.equal(w.shortHash([read(`${w.LIVE}/package-lock.json`)]), stamp);
});

test("SIZES rewrite reproduces embedded.rs byte for byte, rustfmt's layout included", () => {
  const source = read(w.EMBEDDED);
  const { rows, stamp } = w.parseSizes(source);
  assert.equal(rows.length, 12);
  assert.deepEqual(rows[0], { agent: "claude-acp", os: "Darwin", arch: "Arm64", download: 112_272_099, unpacked: 284_447_720 });
  assert.equal(w.writeSizes(source, rows, stamp), source);
  const bigger = rows.map((r) => ({ ...r, download: r.download + 1 }));
  const rewritten = w.writeSizes(source, bigger, "0123456789abcdef");
  assert.deepEqual(w.parseSizes(rewritten), { rows: bigger, stamp: "0123456789abcdef" });
  assert.equal(w.renderRow({ agent: "codex-acp", os: "Darwin", arch: "X64", download: 1, unpacked: 2 }), '    ("codex-acp", Os::Darwin, Arch::X64, 1, 2),');
  assert.equal(w.groupDigits(1234567), "1_234_567");
});

/** A registry stand-in: tarball sizes by URL, unpackedSize by `name/version`. */
function fakeRegistry(tarballs, unpacked) {
  return async (url, init) => {
    if (init?.headers?.range) {
      const size = tarballs[url];
      return new Response("x", { status: 206, headers: { "content-range": `bytes 0-0/${size}` } });
    }
    const key = decodeURIComponent(url.slice(w.REGISTRY.length));
    const size = unpacked[key];
    return Response.json({ dist: size === undefined ? {} : { unpackedSize: size } });
  };
}

test("measureSizes sums each agent's packages per platform; no unpackedSize counts 0", async () => {
  const lock = {
    packages: {
      "": {},
      [`node_modules/${w.CLAUDE_ACP}`]: { version: "1.0.0", resolved: `${w.REGISTRY}cl.tgz`, dependencies: { old: "1" }, optionalDependencies: { "sdk-linux": "1", "sdk-mac": "1" } },
      "node_modules/old": { version: "1.0.0", resolved: `${w.REGISTRY}old.tgz` },
      "node_modules/sdk-linux": { version: "1.0.0", resolved: `${w.REGISTRY}sl.tgz`, os: ["linux"], cpu: ["x64"], libc: ["glibc"] },
      "node_modules/sdk-mac": { version: "1.0.0", resolved: `${w.REGISTRY}sm.tgz`, os: ["darwin"] },
      [`node_modules/${w.CODEX_ACP}`]: { version: "2.0.0", resolved: `${w.REGISTRY}cx.tgz`, dependencies: { old: "1" } },
    },
  };
  const fetchImpl = fakeRegistry(
    { [`${w.REGISTRY}cl.tgz`]: 10, [`${w.REGISTRY}old.tgz`]: 1, [`${w.REGISTRY}sl.tgz`]: 100, [`${w.REGISTRY}sm.tgz`]: 1000, [`${w.REGISTRY}cx.tgz`]: 5 },
    { [`${w.CLAUDE_ACP}/1.0.0`]: 20, "sdk-linux/1.0.0": 200, "sdk-mac/1.0.0": 2000, [`${w.CODEX_ACP}/2.0.0`]: 7 },
  );
  const order = [
    { agent: "claude-acp", os: "Linux", arch: "X64" },
    { agent: "claude-acp", os: "Darwin", arch: "Arm64" },
    { agent: "codex-acp", os: "Win32", arch: "X64" },
  ];
  assert.deepEqual(await w.measureSizes(JSON.stringify(lock), order, fetchImpl), [
    { agent: "claude-acp", os: "Linux", arch: "X64", download: 111, unpacked: 220 },
    { agent: "claude-acp", os: "Darwin", arch: "Arm64", download: 1011, unpacked: 2020 },
    { agent: "codex-acp", os: "Win32", arch: "X64", download: 6, unpacked: 7 },
  ]);
});

const CURRENT = { claudeAcp: "0.89.1", codexAcp: "2.2.2", codex: "0.162.1" };

test("planBump: nothing newer is no change, and an older latest never downgrades", async () => {
  const doc = async () => ({ dependencies: { [w.CODEX]: "^0.162.1" } });
  const same = await w.planBump(CURRENT, { ...CURRENT }, doc, async () => []);
  assert.equal(same.changed, false);
  const older = await w.planBump(CURRENT, { claudeAcp: "0.88.0", codexAcp: "2.2.2", codex: "0.162.1" }, doc, async () => []);
  assert.equal(older.changed, false);
  assert.deepEqual(older.target, CURRENT);
});

test("planBump: codex goes to latest when codex-acp's range allows it", async () => {
  const doc = async (v) => (assert.equal(v, "2.10.0"), { dependencies: { [w.CODEX]: "^0.171.0" } });
  const plan = await w.planBump(CURRENT, { claudeAcp: "0.110.1", codexAcp: "2.10.0", codex: "0.171.4" }, doc, async () => assert.fail("not needed"));
  assert.deepEqual(plan, { target: { claudeAcp: "0.110.1", codexAcp: "2.10.0", codex: "0.171.4" }, changed: true, notes: [] });
  assert.equal(w.branchName(plan.target), "engines/claude-acp-0.110.1_codex-acp-2.10.0_codex-0.171.4");
});

test("planBump: codex-acp's own choice when its range excludes the latest codex, and it says so", async () => {
  const doc = async () => ({ dependencies: { [w.CODEX]: "^0.171.0" } });
  const versions = async () => ["0.170.0", "0.171.0", "0.171.1", "0.171.2-alpha.1", "0.171.1-linux-x64", "0.172.0"];
  const plan = await w.planBump(CURRENT, { claudeAcp: "0.89.1", codexAcp: "2.10.0", codex: "0.172.0" }, doc, versions);
  assert.equal(plan.target.codex, "0.171.1");
  assert.match(plan.notes[0], /0\.172\.0 is npm's latest, but codex-acp 2\.10\.0 asks for `\^0\.171\.0`: kept codex-acp's own choice, 0\.171\.1/);
});

test("bumpManifest keeps every other field and npm's layout", () => {
  const text = read(`${w.LIVE}/package.json`);
  const manifest = JSON.parse(text);
  assert.equal(w.bumpManifest(manifest, w.currentPins(manifest)), text);
  const next = JSON.parse(w.bumpManifest(manifest, { claudeAcp: "9.9.9", codexAcp: "8.8.8", codex: "7.7.7" }));
  assert.deepEqual(w.currentPins(next), { claudeAcp: "9.9.9", codexAcp: "8.8.8", codex: "7.7.7" });
  assert.equal(next.description, manifest.description);
});

test("engines.seq is one decimal line, list 1 on this branch", () => {
  assert.equal(w.parseSeq(read(`${w.LIVE}/${w.SEQ_FILE}`)), 1);
  assert.equal(read(`${w.LIVE}/${w.SEQ_FILE}`), "1\n");
  assert.throws(() => w.parseSeq("one\n"));
  assert.throws(() => w.parseSeq("1\n2\n"));
});

test("next seq: a 404 (nothing published) is the file's value + 1", async () => {
  const fetchImpl = async () => new Response("Not Found", { status: 404 });
  assert.equal(await w.nextSeq("1\n", fetchImpl), 2);
  assert.equal(await w.nextSeq("7\n", fetchImpl), 8);
});

test("next seq: a published seq ahead of the file is the published one + 1", async () => {
  let asked;
  const fetchImpl = async (url) => ((asked = url), Response.json({ seq: 12, agents: [] }));
  assert.equal(await w.nextSeq("3\n", fetchImpl), 13);
  assert.equal(asked, w.PUBLISHED_URL);
  assert.equal(await w.nextSeq("20\n", fetchImpl), 21);
});

test("next seq: any other failure stops the bump", async () => {
  await assert.rejects(w.nextSeq("1\n", async () => new Response("no", { status: 403 })), /HTTP 403/);
  await assert.rejects(w.nextSeq("1\n", async () => Response.json({ seq: "x" })), /seq is not a whole number/);
});

test("summary names old → new, the SDK, the seq, notes, and the lanes", () => {
  const row = { agent: "claude-acp", os: "Linux", arch: "X64", download: 2e6, unpacked: 4e6 };
  const text = w.summary({
    current: CURRENT,
    target: { claudeAcp: "0.110.1", codexAcp: "2.2.2", codex: "0.171.0" },
    sdk: { old: "0.3.293", new: "0.3.330" },
    claudeCode: { old: "2.1.293", new: "2.1.330" },
    notes: ["a note"],
    rows: [row],
    oldRows: [{ ...row, download: 1e6 }],
    stamp: "0123456789abcdef",
    seq: { old: 1, new: 2 },
  });
  assert.match(text, /\| `@agentclientprotocol\/claude-agent-acp` \| 0\.89\.1 \| 0\.110\.1 \|/);
  assert.match(text, /\| `@agentclientprotocol\/codex-acp` \| 2\.2\.2 \| 2\.2\.2 \(unchanged\) \|/);
  assert.match(text, /\| `@openai\/codex` \(override\) \| 0\.162\.1 \| 0\.171\.0 \|/);
  assert.match(text, /\| `@anthropic-ai\/claude-agent-sdk` \| 0\.3\.293 \| 0\.3\.330 \|/);
  assert.match(text, /engines\.seq`\): 1 → 2/);
  assert.match(text, /- a note/);
  assert.match(text, /\| claude-acp \| linux-x64 \| 1\.0 MB → 2\.0 MB \| 4\.0 MB → 4\.0 MB \|/);
  assert.match(text, /CI thread's lanes/);
});

test("engine-watch.yml: pinned actions, least permissions, schedule, guarded push", () => {
  const yml = read(".github/workflows/engine-watch.yml");
  assert.match(yml, /^# /, "a header comment first");
  for (const m of yml.matchAll(/^\s*(?:-\s*)?uses:\s*(\S+)/gm)) {
    assert.match(m[1], /^[^@]+@[0-9a-f]{40}$/, `${m[1]} is not pinned by a full SHA`);
  }
  assert.match(yml, /schedule:\n\s+- cron: "0 \*\/6 \* \* \*"/);
  assert.match(yml, /workflow_dispatch:/);
  assert.match(yml, /^permissions: \{\}$/m);
  const granted = [...yml.matchAll(/^\s+(contents|pull-requests|[a-z-]+): (write|read)$/gm)].map((m) => `${m[1]}: ${m[2]}`);
  assert.deepEqual(granted.sort(), ["contents: write"]);
  assert.doesNotMatch(yml, /secrets\.(?!GITHUB_TOKEN)/);
  assert.match(yml, /\bcommit -s\b/);
  assert.doesNotMatch(yml, /--no-verify/);
  assert.match(yml, /ls-remote/);
  assert.match(yml, /\^\{tree\}/);
  assert.doesNotMatch(yml, /gh pr create/);
  assert.match(yml, /engine-watch\.mjs/);
});
