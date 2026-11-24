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
// workflow_dispatch with min_age_hours through env:, the fork guard, no credentials kept by checkout,
// the token only in the push step, no ${{ }} inside a run:, and no force push.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gunzipSync, gzipSync } from "node:zlib";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import * as w from "./engine-watch.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const read = (path) => readFileSync(join(ROOT, path), "utf8");
w.retry.waits = [0, 0, 0];

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
  // Today's table, whatever the pins: the first row's place, and whole byte counts everywhere.
  assert.deepEqual([rows[0].agent, rows[0].os, rows[0].arch], ["claude-acp", "Darwin", "Arm64"]);
  assert.ok(rows.every((r) => Number.isSafeInteger(r.download) && Number.isSafeInteger(r.unpacked) && r.download > 0 && r.unpacked > r.download));
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
const NOW = Date.parse("2026-11-24T12:00:00Z");
const ago = (hours) => new Date(NOW - hours * 3_600_000).toISOString();
/** A packument: `versions` maps a version to { hours (age), deprecated, codex (range), att }. */
function packument(latest, versions) {
  const doc = { "dist-tags": { latest }, versions: {}, time: {} };
  for (const [v, o] of Object.entries(versions)) {
    doc.versions[v] = { version: v, dist: o.att === false ? {} : { attestations: { url: "x" } } };
    if (o.deprecated) doc.versions[v].deprecated = o.deprecated;
    if (o.codex) doc.versions[v].dependencies = { [w.CODEX]: o.codex };
    if (o.hours !== undefined) doc.time[v] = ago(o.hours);
  }
  return doc;
}
const old = (extra = {}) => ({ hours: 999, ...extra });
function pks({ claude = ["0.89.1", {}], acp = ["2.2.2", {}], codex = ["0.162.1", {}] } = {}) {
  return {
    [w.CLAUDE_ACP]: packument(claude[0], { "0.89.1": old(), ...claude[1] }),
    [w.CODEX_ACP]: packument(acp[0], { "2.2.2": old({ codex: "^0.160.1" }), ...acp[1] }),
    [w.CODEX]: packument(codex[0], { "0.162.1": old(), ...codex[1] }),
  };
}
const SCHEDULED = { now: NOW, minAge: 24 };

test("versions are rebuilt from their parts: a newline, a leading v, spaces", () => {
  assert.equal(w.cleanVersion("0.110.1\n"), "0.110.1");
  assert.equal(w.cleanVersion("v2.10.0"), "2.10.0");
  assert.equal(w.cleanVersion("  0.171.0 "), "0.171.0");
  for (const bad of ["0.1", "0.1.0; rm -rf /", "1.2.3\nbranch=evil", "", null]) assert.throws(() => w.cleanVersion(bad));
  assert.equal(w.branchName({ claudeAcp: "v0.110.1\n", codexAcp: " 2.10.0", codex: "0.171.0 " }), "engines/claude-acp-0.110.1_codex-acp-2.10.0_codex-0.171.0");
  assert.throws(() => w.branchName({ ...CURRENT, codex: "0.171.0-alpha.1" }), /not a branch/);
  assert.deepEqual(w.currentPins({ dependencies: { [w.CLAUDE_ACP]: "v0.89.1\n", [w.CODEX_ACP]: "2.2.2" }, overrides: { [w.CODEX]: " 0.162.1" } }), CURRENT);
});

test("the cool-down: 24 h on a schedule whatever the input, min_age_hours by hand, checked", () => {
  assert.equal(w.minAgeHours("schedule", "0"), 24);
  assert.equal(w.minAgeHours("schedule", undefined), 24);
  assert.equal(w.minAgeHours("workflow_dispatch", "0"), 0);
  assert.equal(w.minAgeHours("workflow_dispatch", "48"), 48);
  assert.equal(w.minAgeHours("workflow_dispatch", ""), 24);
  assert.equal(w.minAgeHours(undefined, undefined), 24);
  for (const bad of ["-1", "1.5", "24; echo", "x"]) assert.throws(() => w.minAgeHours("workflow_dispatch", bad), /non-negative integer/);
});

test("judge: a pre-release, a deprecated, a too young latest are refused; age in hours", () => {
  const doc = packument("1.0.0", { "1.0.0": { hours: 30 }, "1.1.0-beta.1": { hours: 99 }, "0.9.0": { hours: 99, deprecated: "use 1.x\n`x`" }, "1.0.1": { hours: 23.9 } });
  assert.deepEqual(w.judge(doc, "1.0.0", SCHEDULED), { version: "1.0.0", ok: true, ageHours: 30 });
  assert.equal(w.judge(doc, "1.1.0-beta.1", SCHEDULED).reason, "a pre-release");
  assert.equal(w.judge(doc, "0.9.0", SCHEDULED).reason, "deprecated: use 1.x x");
  assert.deepEqual(w.judge(doc, "1.0.1", SCHEDULED), { version: "1.0.1", ok: false, ageHours: 23, reason: "23 h old, under the 24 h cool-down" });
  assert.equal(w.judge(doc, "1.0.1", { now: NOW, minAge: 0 }).ok, true);
  assert.equal(w.judge(packument("2.0.0", { "2.0.0": {} }), "2.0.0", SCHEDULED).reason, "no publish time in the registry");
  assert.equal(w.judge(doc, "3.0.0", SCHEDULED).reason, "not among the registry's versions");
  assert.equal(w.judge(packument("24.0.0", { "24.0.0": { hours: 24 } }), "24.0.0", SCHEDULED).ok, true);
});

test("planBump: nothing newer is no change, and an older latest never downgrades", () => {
  assert.equal(w.planBump(CURRENT, pks(), SCHEDULED).changed, false);
  const older = w.planBump(CURRENT, pks({ claude: ["0.88.0", { "0.88.0": old() }] }), SCHEDULED);
  assert.deepEqual([older.changed, older.target], [false, CURRENT]);
});

test("planBump: every latest taken when ripe, codex within codex-acp's range", () => {
  const plan = w.planBump(CURRENT, pks({
    claude: ["0.110.1", { "0.110.1": { hours: 30 } }],
    acp: ["2.10.0", { "2.10.0": { hours: 40, codex: "^0.171.0" } }],
    codex: ["0.171.4", { "0.171.4": { hours: 50 } }],
  }), SCHEDULED);
  assert.deepEqual(plan.target, { claudeAcp: "0.110.1", codexAcp: "2.10.0", codex: "0.171.4" });
  assert.deepEqual(plan.notes, []);
  assert.deepEqual([plan.verdicts.claudeAcp.ageHours, plan.verdicts.codexAcp.ageHours, plan.verdicts.codex.ageHours], [30, 40, 50]);
});

test("planBump: a latest codex outside codex-acp's range gives the newest release inside it", () => {
  const plan = w.planBump(CURRENT, pks({
    acp: ["2.10.0", { "2.10.0": { hours: 40, codex: "^0.171.0" } }],
    codex: ["0.172.0", { "0.172.0": old(), "0.171.0": old(), "0.171.1": old(), "0.171.2-alpha.1": old(), "0.171.1-linux-x64": old() }],
  }), SCHEDULED);
  assert.equal(plan.target.codex, "0.171.1");
  assert.match(plan.notes[0], /0\.172\.0 is npm's latest, but codex-acp 2\.10\.0 asks for `\^0\.171\.0`: the newest release in that range is 0\.171\.1/);
});

test("planBump: a pre-release or deprecated latest stays at its pin, and the others still bump", () => {
  const plan = w.planBump(CURRENT, pks({
    claude: ["0.111.0-rc.1", { "0.111.0-rc.1": old() }],
    acp: ["2.10.0", { "2.10.0": old({ codex: "^0.171.0", deprecated: "broken" }) }],
    codex: ["0.163.0", { "0.163.0": old() }],
  }), SCHEDULED);
  assert.deepEqual(plan.target, { claudeAcp: "0.89.1", codexAcp: "2.2.2", codex: "0.162.1" });
  assert.match(plan.notes.join("\n"), /claude-agent-acp 0\.111\.0-rc\.1 not taken, kept at 0\.89\.1: a pre-release/);
  assert.match(plan.notes.join("\n"), /codex-acp 2\.10\.0 not taken, kept at 2\.2\.2: deprecated: broken/);
  const mixed = w.planBump(CURRENT, pks({ claude: ["0.110.1", { "0.110.1": old() }], acp: ["2.10.0", { "2.10.0": old({ codex: "^0.171.0", deprecated: "x" }) }] }), SCHEDULED);
  assert.deepEqual([mixed.changed, mixed.target.claudeAcp, mixed.target.codexAcp], [true, "0.110.1", "2.2.2"]);
});

test("planBump: a too young codex holds the codex side together; codex-acp without a range is named", () => {
  const young = w.planBump(CURRENT, pks({
    claude: ["0.110.1", { "0.110.1": { hours: 25 } }],
    acp: ["2.10.0", { "2.10.0": { hours: 40, codex: "^0.171.0" } }],
    codex: ["0.171.0", { "0.171.0": { hours: 2 } }],
  }), SCHEDULED);
  assert.deepEqual(young.target, { claudeAcp: "0.110.1", codexAcp: "2.2.2", codex: "0.162.1" });
  assert.match(young.notes.join("\n"), /@openai\/codex 0\.171\.0 not taken, kept at 0\.162\.1: 2 h old, under the 24 h cool-down/);
  assert.match(young.notes.join("\n"), /codex-acp 2\.10\.0 not taken, kept at 2\.2\.2 with @openai\/codex 0\.162\.1/);
  const byHand = w.planBump(CURRENT, pks({ acp: ["2.10.0", { "2.10.0": { hours: 1, codex: "^0.171.0" } }], codex: ["0.171.0", { "0.171.0": { hours: 1 } }] }), { now: NOW, minAge: 0 });
  assert.deepEqual(byHand.target, { claudeAcp: "0.89.1", codexAcp: "2.10.0", codex: "0.171.0" });
  const noRange = w.planBump(CURRENT, pks({ acp: ["2.3.0", { "2.3.0": old() }], codex: ["0.163.0", { "0.163.0": old() }] }), SCHEDULED);
  assert.deepEqual(noRange.target, { claudeAcp: "0.89.1", codexAcp: "2.3.0", codex: "0.163.0" });
  assert.match(noRange.notes[0], /codex-acp 2\.3\.0 declares no @openai\/codex range/);
});

test("assertPair throws on a proposed codex outside codex-acp's own range", () => {
  const pk = pks({ acp: ["2.10.0", { "2.10.0": old({ codex: "^0.171.0" }) }] });
  assert.throws(() => w.assertPair(CURRENT, { ...CURRENT, codexAcp: "2.10.0", codex: "0.172.0" }, pk), /asks for @openai\/codex \^0\.171\.0, not 0\.172\.0/);
  assert.doesNotThrow(() => w.assertPair(CURRENT, { ...CURRENT, codexAcp: "2.10.0", codex: "0.171.3" }, pk));
  assert.doesNotThrow(() => w.assertPair(CURRENT, { ...CURRENT, claudeAcp: "0.110.1" }, pk));
});

test("pushPlan: an existing branch, whatever it holds, is never pushed over", () => {
  const branch = "engines/claude-acp-0.110.1_codex-acp-2.10.0_codex-0.171.0";
  const exists = w.pushPlan(branch, `${"a".repeat(40)}\trefs/heads/${branch}\n`);
  assert.equal(exists.push, false);
  assert.match(exists.message, /^::notice::.*exists already \(aaaaaaaaaaaa\): nothing pushed/);
  assert.equal(w.pushPlan(branch, "").push, true);
  assert.equal(w.pushPlan(branch, "\n").push, true);
  assert.throws(() => w.pushPlan("main", ""), /not a branch/);
  assert.throws(() => w.pushPlan(`${branch}\nx`, ""), /not a branch/);
});

test("bumpManifest keeps every other field and npm's layout", () => {
  const text = read(`${w.LIVE}/package.json`);
  const manifest = JSON.parse(text);
  assert.equal(w.bumpManifest(manifest, w.currentPins(manifest)), text);
  const next = JSON.parse(w.bumpManifest(manifest, { claudeAcp: "9.9.9", codexAcp: "8.8.8", codex: "7.7.7" }));
  assert.deepEqual(w.currentPins(next), { claudeAcp: "9.9.9", codexAcp: "8.8.8", codex: "7.7.7" });
  assert.equal(next.description, manifest.description);
});

test("engines.seq is one decimal line, at least list 1", () => {
  const text = read(`${w.LIVE}/${w.SEQ_FILE}`);
  assert.match(text, /^[1-9]\d*\n$/);
  assert.equal(`${w.parseSeq(text)}\n`, text);
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


test("published seq: a network error, a 5xx and a body that is not JSON stop the bump", async () => {
  await assert.rejects(w.publishedSeq(async () => { throw new TypeError("fetch failed"); }), /fetch failed/);
  let tries = 0;
  await assert.rejects(w.publishedSeq(async () => (tries++, new Response("down", { status: 503 }))), /HTTP 503/);
  assert.equal(tries, 3);
  await assert.rejects(w.publishedSeq(async () => new Response("<html>", { status: 200 })), /not JSON/);
});

test("measureSizes refuses a lock entry resolved outside the registry", async () => {
  const lock = { packages: { "": {}, [`node_modules/${w.CLAUDE_ACP}`]: { version: "1.0.0", resolved: "https://evil.example/cl.tgz" }, [`node_modules/${w.CODEX_ACP}`]: { version: "1.0.0", resolved: `${w.REGISTRY}cx.tgz` } } };
  await assert.rejects(w.measureSizes(JSON.stringify(lock), [], async () => assert.fail("no fetch")), /resolved is not under https:\/\/registry\.npmjs\.org\//);
});

/** A gzipped tar of { name: bytes }; a name over 100 bytes gets a pax path record. */
function tgz(files) {
  const blocks = [];
  const header = (name, size, type) => {
    const h = Buffer.alloc(512);
    h.write(name.slice(0, 100), 0);
    h.write(size.toString(8).padStart(11, "0"), 124);
    h.write(type, 156);
    h.write("ustar\u000000", 257);
    return h;
  };
  const pad = (b) => Buffer.concat([b, Buffer.alloc((512 - (b.length % 512)) % 512)]);
  for (const [name, bytes] of Object.entries(files)) {
    if (name.length > 100) {
      const rec = `path=${name}\n`;
      const body = Buffer.from(`${rec.length + 3} ${rec}`);
      blocks.push(header("PaxHeader", body.length, "x"), pad(body));
    }
    blocks.push(header(name, bytes.length, "0"), pad(Buffer.from(bytes)));
  }
  return gzipSync(Buffer.concat([...blocks, Buffer.alloc(1024)]));
}
const sri = (b) => `sha512-${createHash("sha512").update(b).digest("base64")}`;
const sha256 = (b) => createHash("sha256").update(b).digest("hex");

test("LAYOUTS: sac.rs parses and rewrites to itself; the tar reader finds rg.exe, pax names too", async () => {
  const source = read(w.SAC);
  const { rows, codex } = w.parseLayouts(source);
  assert.equal(codex, JSON.parse(read(`${w.LIVE}/package.json`)).overrides[w.CODEX]);
  assert.equal(w.writeLayouts(source, rows, codex), source);
  const moved = w.writeLayouts(source, rows.map((r) => ({ ...r, sha256: "f".repeat(64) })), "9.9.9");
  assert.deepEqual(w.parseLayouts(moved), { rows: rows.map((r) => ({ ...r, sha256: "f".repeat(64) })), codex: "9.9.9" });
  assert.match(moved, /codex-9\.9\.9-win32-x64\.tgz/);
  assert.match(moved, /`@openai\/codex` 9\.9\.9 ships/);
  // Other notes in the doc comment (main's "unchanged since <v>") are left as written.
  assert.equal(/unchanged since [\d.]+/.exec(moved)?.[0], /unchanged since [\d.]+/.exec(source)?.[0]);
  const long = `package/${"d".repeat(90)}/codex-path/rg.exe`;
  const tar = tgz({ "package/a.txt": "a", "package/v/codex-path/rg.exe": "RG", [long]: "LONG" });
  const lock = JSON.stringify({ packages: { "": {}, "node_modules/x": { resolved: "u", integrity: sri(tar) } } });
  const got = await w.hashLayouts(lock, [{ dir: "node_modules/x/v" }, { dir: `node_modules/x/${"d".repeat(90)}` }], async () => tar);
  assert.deepEqual(got.map((r) => r.sha256), [sha256("RG"), sha256("LONG")]);
  await assert.rejects(w.hashLayouts(lock, [{ dir: "node_modules/x/v" }], async () => Buffer.concat([tar, Buffer.from("x")])), /does not match the lock's integrity/);
  await assert.rejects(w.hashLayouts(lock, [{ dir: "node_modules/x/none" }], async () => tar), /has no package\/none\/codex-path\/rg\.exe/);
});

test("npm's --before carries the cool-down to the whole tree, and is left out at 0", () => {
  assert.equal(w.npmArgs(24, NOW).at(-1), "--before=2026-11-23T12:00:00.000Z");
  assert.equal(w.npmArgs(48, NOW).at(-1), "--before=2026-11-22T12:00:00.000Z");
  assert.ok(!w.npmArgs(0, NOW).some((a) => a.startsWith("--before")));
});

test("the tar reader refuses a size that is not octal; downloads stay on the registry and need a 2xx", async () => {
  const tar = gunzipSync(tgz({ "package/a": "abc" }));
  assert.equal(w.tarFile(tar, "package/a").toString(), "abc");
  const bad = Buffer.from(tar);
  bad.write("00000009zz\0", 124, "latin1");
  assert.throws(() => w.tarFile(bad, "package/a"), /size is not octal/);
  const big = Buffer.from(tar);
  big.write("777777777777", 124, "latin1");
  assert.throws(() => w.tarFile(big, "package/a"), /runs past the archive/);
  const ok = w.registryDownload(async () => new Response("tgz"));
  assert.equal((await ok(`${w.REGISTRY}a/-/a-1.0.0.tgz`)).toString(), "tgz");
  await assert.rejects(ok("https://evil.example/a.tgz"), /not under https:\/\/registry\.npmjs\.org\//);
  await assert.rejects(w.registryDownload(async () => new Response("no", { status: 404 }))(`${w.REGISTRY}a.tgz`), /HTTP 404/);
});

/** A scratch repository root with the five files the script reads, and a registry stand-in. */
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "engine-watch-"));
  const put = (rel, text) => (mkdirSync(dirname(join(root, rel)), { recursive: true }), writeFileSync(join(root, rel), text));
  const tars = { x64: tgz({ "package/vendor/x86_64-pc-windows-msvc/codex-path/rg.exe": "rg x64 new" }), arm: tgz({ "package/vendor/aarch64-pc-windows-msvc/codex-path/rg.exe": "rg arm new" }) };
  const R = w.REGISTRY;
  const entry = (name, v, extra = {}) => ({ version: v, resolved: `${R}${name}/-/${v}.tgz`, integrity: "sha512-AA==", ...extra });
  const lock = (acp, sdk, cx, codex) => JSON.stringify({ name: "x", lockfileVersion: 3, packages: {
    "": {},
    [`node_modules/${w.CLAUDE_ACP}`]: entry("cl", acp, { dependencies: { [w.SDK]: sdk } }),
    [`node_modules/${w.SDK}`]: entry("sdk", sdk),
    [`node_modules/${w.CODEX_ACP}`]: entry("cx", cx, { dependencies: { [w.CODEX]: codex } }),
    [`node_modules/${w.CODEX}`]: entry("codex", codex, { optionalDependencies: { "@openai/codex-win32-x64": "1", "@openai/codex-win32-arm64": "1" } }),
    "node_modules/@openai/codex-win32-x64": { name: w.CODEX, version: `${codex}-win32-x64`, resolved: `${R}x64.tgz`, integrity: sri(tars.x64), os: ["win32"], cpu: ["x64"] },
    "node_modules/@openai/codex-win32-arm64": { name: w.CODEX, version: `${codex}-win32-arm64`, resolved: `${R}arm.tgz`, integrity: sri(tars.arm), os: ["win32"], cpu: ["arm64"] },
  } }, null, 2);
  // The pins before the bump the stand-in registry offers, whatever today's are.
  put(`${w.LIVE}/package.json`, w.bumpManifest(JSON.parse(read(`${w.LIVE}/package.json`)), CURRENT));
  put(`${w.LIVE}/package-lock.json`, lock("0.89.1", "0.3.293", "2.2.2", "0.162.1"));
  put(`${w.LIVE}/${w.SEQ_FILE}`, "1\n");
  put(w.EMBEDDED, `#[cfg(test)]\npub(super) const SIZES_STAMP: &str = "0000000000000000";\n\nconst SIZES: [(&str, Os, Arch, u64, u64); 2] = [\n    ("claude-acp", Os::Win32, Arch::X64, 1, 2),\n    ("codex-acp", Os::Win32, Arch::Arm64, 3, 4),\n];\n`);
  const sac = read(w.SAC);
  put(w.SAC, w.writeLayouts(sac, w.parseLayouts(sac).rows, CURRENT.codex));
  const docs = {
    [w.CLAUDE_ACP]: packument("0.110.1", { "0.89.1": old(), "0.110.1": { hours: 30 } }),
    [w.CODEX_ACP]: packument("2.10.0", { "2.2.2": old(), "2.10.0": { hours: 40, codex: "^0.171.0" } }),
    [w.CODEX]: packument("0.172.0", { "0.162.1": old(), "0.171.0": { hours: 50, att: false }, "0.172.0": { hours: 1 } }),
    [w.SDK]: packument("0.3.330", { "0.3.293": old({ att: false }), "0.3.330": { hours: 60, att: false } }),
  };
  docs[w.SDK].versions["0.3.330"].claudeCodeVersion = "2.1.330";
  const fetchImpl = async (url, init) => {
    if (url === w.PUBLISHED_URL) return new Response("Not Found", { status: 404 });
    const name = url.endsWith("x64.tgz") ? "x64" : url.endsWith("arm.tgz") ? "arm" : null;
    if (init?.headers?.range) return new Response("x", { status: 206, headers: { "content-range": `bytes 0-0/${name ? tars[name].length : 10}` } });
    if (name) return new Response(tars[name]);
    const key = decodeURIComponent(url.slice(R.length));
    if (docs[key]) return Response.json(docs[key]);
    return Response.json({ dist: { unpackedSize: 100 } });
  };
  const npmCalls = [];
  const runNpm = (dir, args) => (npmCalls.push(args), writeFileSync(join(dir, "package-lock.json"), lock("0.110.1", "0.3.330", "2.10.0", "0.171.0")));
  return { root, fetchImpl, runNpm, npmCalls, tars, readRel: (rel) => readFileSync(join(root, rel), "utf8"), done: () => rmSync(root, { recursive: true, force: true }) };
}

test("bump end to end on a stand-in registry and npm, then --check-sizes on its result", async () => {
  const f = fixture();
  try {
    const logs = [];
    const summaryPath = join(f.root, "summary.md");
    const out = await w.bump(f.root, { summaryPath, minAge: 24, fetchImpl: f.fetchImpl, runNpm: f.runNpm, now: NOW, log: (l) => logs.push(l) });
    assert.deepEqual(out, { code: 0, branch: "engines/claude-acp-0.110.1_codex-acp-2.10.0_codex-0.171.0" });
    assert.deepEqual(w.currentPins(JSON.parse(f.readRel(`${w.LIVE}/package.json`))), { claudeAcp: "0.110.1", codexAcp: "2.10.0", codex: "0.171.0" });
    assert.equal(f.readRel(`${w.LIVE}/${w.SEQ_FILE}`), "2\n");
    const sizes = w.parseSizes(f.readRel(w.EMBEDDED));
    assert.equal(sizes.stamp, w.shortHash([f.readRel(`${w.LIVE}/package-lock.json`)]));
    assert.deepEqual(sizes.rows[0], { agent: "claude-acp", os: "Win32", arch: "X64", download: 20, unpacked: 200 });
    assert.deepEqual(sizes.rows[1], { agent: "codex-acp", os: "Win32", arch: "Arm64", download: 20 + f.tars.arm.length, unpacked: 300 });
    const layouts = w.parseLayouts(f.readRel(w.SAC));
    assert.deepEqual([layouts.codex, ...layouts.rows.map((r) => r.sha256)], ["0.171.0", sha256("rg x64 new"), sha256("rg arm new")]);
    const text = readFileSync(summaryPath, "utf8");
    assert.match(text, /\| `@agentclientprotocol\/claude-agent-acp` \| 0\.89\.1 \| 0\.110\.1 \| 30 h \| yes → yes \|/);
    assert.match(text, /\| `@anthropic-ai\/claude-agent-sdk` \| 0\.3\.293 \| 0\.3\.330 \| 60 h \| no → no \|/);
    assert.match(text, /\| `@openai\/codex` \(override\) \| 0\.162\.1 \| 0\.171\.0 \| 50 h \| yes → no \*\*lost\*\* \|/);
    assert.match(text, /\*\*@openai\/codex 0\.171\.0 has no provenance attestations; 0\.162\.1 had them\.\*\*/);
    assert.match(text, /^npm signatures and provenance were not verified\.$/m);
    assert.match(text, /0\.172\.0 is npm's latest, but codex-acp 2\.10\.0 asks for/);
    assert.match(text, /rg\.exe re-hashed in `crates\/agents\/src\/sac\.rs` LAYOUTS for @openai\/codex 0\.171\.0, and it changed/);
    assert.match(text, /codex_patch\.rs/);
    assert.match(text, /engines\.seq`\): 1 → 2/);
    assert.match(text, /2\.1\.330/);
    assert.deepEqual(f.npmCalls, [["install", "--package-lock-only", "--ignore-scripts", "--no-audit", "--no-fund", "--before=2026-11-23T12:00:00.000Z"]]);
    assert.equal(logs.at(-1), `bump ${out.branch}`);
    assert.equal(await w.checkSizes(f.root, { fetchImpl: f.fetchImpl, log: () => {} }), 0);
    writeFileSync(join(f.root, w.EMBEDDED), f.readRel(w.EMBEDDED).replace("unpacked", "").replace(/, 200\)/, ", 201)"));
    const lines = [];
    assert.equal(await w.checkSizes(f.root, { fetchImpl: f.fetchImpl, log: (l) => lines.push(l) }), 1);
    assert.match(lines.join("\n"), /DIFF  claude-acp Win32 X64: 20 200 \(embedded 20 201\)/);
    const again = await w.bump(f.root, { minAge: 24, fetchImpl: f.fetchImpl, runNpm: () => assert.fail("no npm"), now: NOW, log: () => {} });
    assert.deepEqual(again, { code: 0 });
  } finally {
    f.done();
  }
});

test("engine-watch.yml: pinned actions, least permissions, fork guard, token only where it pushes", () => {
  const yml = read(".github/workflows/engine-watch.yml");
  assert.match(yml, /^# /, "a header comment first");
  for (const m of yml.matchAll(/^\s*(?:-\s*)?uses:\s*(\S+)/gm)) {
    assert.match(m[1], /^[^@]+@[0-9a-f]{40}$/, `${m[1]} is not pinned by a full SHA`);
  }
  assert.match(yml, /schedule:\n\s+- cron: "0 \*\/6 \* \* \*"/);
  assert.match(yml, /workflow_dispatch:\n\s+inputs:\n\s+min_age_hours:\n(?:\s+.*\n)*?\s+default: "24"/);
  assert.match(yml, /MIN_AGE_HOURS: \$\{\{ inputs\.min_age_hours \}\}/);
  assert.match(yml, /^permissions: \{\}$/m);
  const granted = [...yml.matchAll(/^\s+([a-z-]+): (write|read)$/gm)].map((m) => `${m[1]}: ${m[2]}`);
  assert.deepEqual(granted.sort(), ["contents: write"]);
  assert.match(yml, /if: github\.repository == 'clauseo-ai\/clauseo-acp' && github\.ref == 'refs\/heads\/main'/);
  assert.match(yml, /persist-credentials: false/);
  assert.doesNotMatch(yml, /secrets\./);
  assert.doesNotMatch(yml, /--force|\^\{tree\}|gh pr create|--no-verify/);
  assert.match(yml, /\bcommit -s\b/);
  assert.match(yml, /ls-remote/);
  assert.match(yml, /engine-watch\.mjs --push-plan/);
  // No expression inside a run: script, where its text would be run as shell.
  const runs = [...yml.matchAll(/^(\s+)run: (\|\n(?:\1  .*\n|\n)*|.*\n)/gm)].map((m) => m[2]);
  assert.ok(runs.length >= 4);
  for (const run of runs) assert.doesNotMatch(run, /\$\{\{/, run);
  // The token reaches one step, the push, and only through env:.
  const steps = yml.slice(yml.indexOf("\n    steps:\n")).split(/\n(?= {6}- )/);
  const withToken = steps.filter((s) => /github\.token|GITHUB_TOKEN/.test(s));
  assert.equal(withToken.length, 1);
  assert.match(withToken[0], /git -c "http\.https:\/\/github\.com\/\.extraheader=AUTHORIZATION: basic \$auth" push origin/);
  assert.doesNotMatch(withToken[0], /commit|npm|node /);
});
