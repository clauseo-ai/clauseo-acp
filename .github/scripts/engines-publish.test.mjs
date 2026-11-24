// Tests for engines-publish.mjs, and the rules engines-publish.yml must keep. Plain Node, no install:
//
//   node --test .github/scripts/engines-publish.test.mjs
//
// Nothing here reaches the network: the releases and the GitHub API are stand-ins, and the
// signatures are minisign's format made here with a throwaway Ed25519 key. The real cross-check
// is crates/engines (clauseo-engines) loading a list this script built, signed by tauri signer.
//
// engines-publish.yml is read as text: workflow_dispatch only; `plan` with no environment, no
// secret and contents: read; `publish` the only job with an environment (by lane), contents:
// write, re-checking plan's sha256; the key only in the probe and sign steps; every `uses:` pinned
// by a full SHA; no ${{ }} inside a run:; --prerelease --latest=false; no --app-version; no
// credentials kept by checkout; the fork guard.

import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import * as e from "./engines-publish.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const read = (path) => readFileSync(join(ROOT, path), "utf8");
const LIVE_JSON = JSON.parse(read(`${e.LIVE}/package.json`));
const LIVE_LOCK = JSON.parse(read(`${e.LIVE}/package-lock.json`));
const NOW = new Date("2026-11-24T06:00:00.789Z");
const REPO = "clauseo-ai/clauseo-acp";
const b64 = (text) => Buffer.from(text).toString("base64");
e.retry.waits = [0, 0, 0];

// ---- a throwaway minisign key, in Tauri's base64 form ----

function makeKey() {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const id = randomBytes(8);
  const raw = publicKey.export({ format: "jwk" }).x;
  const pub = Buffer.concat([Buffer.from("Ed"), id, Buffer.from(raw, "base64url")]);
  const idHex = Buffer.from(id).reverse().toString("hex").toUpperCase();
  return {
    pub: b64(`untrusted comment: minisign public key: ${idHex}\n${pub.toString("base64")}\n`),
    sign(data, comment = `timestamp:1795500000\tfile:engines.json`, alg = "ED") {
      const message = alg === "ED" ? createHash("blake2b512").update(data).digest() : data;
      const sig = sign(null, message, privateKey);
      const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), privateKey);
      const line = Buffer.concat([Buffer.from(alg), id, sig]).toString("base64");
      return b64(`untrusted comment: signature from tauri secret key\n${line}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`);
    },
  };
}

// ---- stand-ins for the network ----

/** fetch over `files` (url -> text); anything else is a 404. */
function fakeFetch(files) {
  const seen = [];
  const impl = async (url, init) => {
    seen.push({ url, init });
    const body = files[url];
    if (body instanceof Error) throw body;
    if (typeof body === "number") return { status: body, ok: false, text: async () => "" };
    if (body === undefined) return { status: 404, ok: false, text: async () => "" };
    return { status: 200, ok: true, text: async () => body };
  };
  impl.seen = seen;
  return impl;
}
const live = (lane) => e.assetUrl(REPO, lane, "engines.json");
const liveSig = (lane) => e.assetUrl(REPO, lane, "engines.json.sig");
/** A live list at `seq` on `lane`, signed by `key`: the two release assets. */
function liveFiles(key, lane, seq, { sign = (text) => key.sign(Buffer.from(text)), change = (l) => l } = {}) {
  const text = e.canonical(change({ ...good(), seq }));
  return { [live(lane)]: text, [liveSig(lane)]: sign(text) };
}
const API = `https://api.github.com/repos/${REPO}/releases/latest`;

// A checkout with a test key standing in for both lanes' public keys.
function checkout(key, { seq = "2\n", version = "0.0.0" } = {}) {
  const root = mkdtempSync(join(tmpdir(), "engines-publish-"));
  for (const dir of [e.LIVE, "crates/agents/src/snapshot", "crates/desktop/release"]) mkdirSync(join(root, dir), { recursive: true });
  writeFileSync(join(root, e.LIVE, "package.json"), read(`${e.LIVE}/package.json`));
  writeFileSync(join(root, e.LIVE, "package-lock.json"), read(`${e.LIVE}/package-lock.json`));
  writeFileSync(join(root, e.LIVE, "engines.seq"), seq);
  writeFileSync(join(root, e.EMBEDDED), read(e.EMBEDDED));
  writeFileSync(join(root, "crates/desktop/tauri.conf.json"), JSON.stringify({ version, plugins: { updater: { pubkey: key.pub } } }));
  writeFileSync(join(root, "crates/desktop/release/updater-test.pub"), `${key.pub}\n`);
  return { root, done: () => rmSync(root, { recursive: true, force: true }) };
}

const good = () => ({
  format: e.FORMAT,
  seq: 2,
  generated: "2026-11-24T06:00:00Z",
  apps: ["0.0.0"],
  package_json: structuredClone(LIVE_JSON),
  package_lock: structuredClone(LIVE_LOCK),
  sizes: e.parseSizes(read(e.EMBEDDED)),
});

// ---- the builder ----

test("canonical writes pins.rs's form: sorted keys, two spaces, {} and [], a trailing newline", () => {
  assert.equal(e.canonical({ b: [], a: { d: 1, c: "x\n" }, e: {} }), '{\n  "a": {\n    "c": "x\\n",\n    "d": 1\n  },\n  "b": [],\n  "e": {}\n}\n');
  assert.equal(e.canonical([1, [true, null]]), "[\n  1,\n  [\n    true,\n    null\n  ]\n]\n");
  assert.equal(e.canonical({ Z: 1, _: 2, a: 3, "@b": 4 }), '{\n  "@b": 4,\n  "Z": 1,\n  "_": 2,\n  "a": 3\n}\n');
  assert.throws(() => e.canonical({ x: 1.5 }), /whole number/);
});

test("the plan builds the live pair as canonical JSON with SIZES, seq and apps", async () => {
  const key = makeKey();
  const c = checkout(key);
  try {
    const p = await e.plan(c.root, { env: { LANE: "engines", GITHUB_REPOSITORY: REPO }, fetchImpl: fakeFetch({}), now: NOW });
    assert.equal(p.live, null);
    assert.equal(p.list.seq, 2);
    assert.equal(p.list.generated, "2026-11-24T06:00:00Z");
    assert.deepEqual(p.list.apps, ["0.0.0"]);
    assert.deepEqual(p.list.package_json, LIVE_JSON);
    assert.deepEqual(p.list.package_lock, LIVE_LOCK);
    assert.equal(p.list.sizes.length, 12);
    assert.deepEqual(Object.keys(p.list.sizes[0]), ["agent", "os", "arch", "download", "unpacked"]);
    assert.deepEqual(new Set(p.list.sizes.map((r) => `${r.os}-${r.arch}`)), new Set(["darwin-arm64", "darwin-x64", "win32-x64", "win32-arm64", "linux-x64", "linux-arm64"]));
    assert.equal(p.text, e.canonical(JSON.parse(p.text)), "canonical");
    assert.ok(p.text.startsWith('{\n  "apps": [\n    "0.0.0"\n  ],\n  "format": "clauseo-engines/1",\n  "generated"'));
    assert.ok(p.text.endsWith("}\n") && !p.text.endsWith("\n\n"));
    assert.deepEqual(Object.keys(JSON.parse(p.text)), ["apps", "format", "generated", "package_json", "package_lock", "seq", "sizes"]);
    assert.equal(p.sha256, createHash("sha256").update(p.text).digest("hex"));
    for (const want of ["- ref: `(local)` (NOT refs/heads/main: do not approve)\n- commit: `(local)`\n- seq: engines.seq 2, live none, archived none, chosen 2 (no live or archived list: engines.seq)\n- sha256: `", "`@agentclientprotocol/codex-acp` 2.10.0", "apps: 0.0.0", p.sha256, "<details>", '"format": "clauseo-engines/1"']) {
      assert.ok(p.summary.includes(want), want);
    }
  } finally {
    c.done();
  }
});

test("parseSizes reads embedded.rs's rows with npm's names, and refuses a short table", () => {
  const rows = e.parseSizes(read(e.EMBEDDED));
  assert.ok(rows.every((r) => ["claude-acp", "codex-acp"].includes(r.agent) && r.download > 0 && r.unpacked > r.download));
  const src = 'const SIZES: [(&str, Os, Arch, u64, u64); 2] = [\n    ("claude-acp", Os::Win32, Arch::X64, 1_000, 2_000),\n];\n';
  assert.throws(() => e.parseSizes(src), /says 2 rows, 1 read/);
  assert.deepEqual(e.parseSizes(src.replace("; 2]", "; 1]")), [{ agent: "claude-acp", os: "win32", arch: "x64", download: 1000, unpacked: 2000 }]);
  assert.throws(() => e.parseSizes(src.replace("; 2]", "; 1]").replace("Os::Win32", "Os::Plan9")), /Os::Plan9/);
});

// ---- apps ----

test("apps: the engines lane takes main's version and the latest v<X.Y.Z> release's only", () => {
  assert.deepEqual(e.enginesApps("0.0.0", null), ["0.0.0"]);
  assert.deepEqual(e.enginesApps("0.2.0", "v0.1.3"), ["0.2.0", "0.1.3"]);
  assert.deepEqual(e.enginesApps("0.2.0", "v0.2.0"), ["0.2.0"]);
  for (const tag of ["python-runtime-3.13.9", "test-v0.1.0", "v0.1", "v0.1.0-rc.1", "0.1.0"]) assert.deepEqual(e.enginesApps("0.2.0", tag), ["0.2.0"], tag);
  assert.throws(() => e.enginesApps("", null), /not semver/);
});

test("apps: the test lane takes exact versions and dev builds, and refuses anything else", () => {
  assert.deepEqual(e.testApps("0.1.0, 0.0.0-dev.1a2b3c4,0.1.0"), ["0.1.0", "0.0.0-dev.1a2b3c4"]);
  assert.deepEqual(e.testApps(`1.2.3-dev.${"a".repeat(40)}`), [`1.2.3-dev.${"a".repeat(40)}`]);
  for (const bad of ["", " , ", "0.1", "v0.1.0", "0.1.0-rc.1", "0.1.0-dev.123456", "0.1.0-dev.ABCDEFG", `0.1.0-dev.${"a".repeat(41)}`, "*"]) {
    assert.throws(() => e.testApps(bad), /apps|is not/, bad);
  }
});

test("apps through the plan: the API's latest release, an apps input refused on engines, needed on engines-test", async () => {
  const key = makeKey();
  const c = checkout(key, { version: "0.2.0" });
  try {
    const f = fakeFetch({ [API]: JSON.stringify({ tag_name: "v0.1.0" }) });
    const p = await e.plan(c.root, { env: { LANE: "", GITHUB_REPOSITORY: REPO, GH_TOKEN: "t0ken" }, fetchImpl: f, now: NOW });
    assert.deepEqual(p.list.apps, ["0.2.0", "0.1.0"]);
    assert.equal(f.seen.find((s) => s.url === API).init.headers.authorization, "Bearer t0ken");
    const py = fakeFetch({ [API]: JSON.stringify({ tag_name: "python-runtime-3.13" }) });
    assert.deepEqual((await e.plan(c.root, { env: { GITHUB_REPOSITORY: REPO }, fetchImpl: py, now: NOW })).list.apps, ["0.2.0"]);
    await assert.rejects(e.plan(c.root, { env: { GITHUB_REPOSITORY: REPO, APPS: "0.1.0" }, fetchImpl: py, now: NOW }), /engines-test lane only/);
    await assert.rejects(e.plan(c.root, { env: { LANE: "engines-test", GITHUB_REPOSITORY: REPO }, fetchImpl: py, now: NOW }), /needs apps/);
    const quiet = fakeFetch({});
    const t = await e.plan(c.root, { env: { LANE: "engines-test", APPS: "0.0.0-dev.abcdef0", GITHUB_REPOSITORY: REPO }, fetchImpl: quiet, now: NOW });
    assert.deepEqual(t.list.apps, ["0.0.0-dev.abcdef0"]);
    assert.deepEqual(quiet.seen.map((s) => s.url), [live("engines-test"), liveSig("engines-test"), `https://api.github.com/repos/${REPO}/releases/tags/engines-test`], "the test lane reads no latest release");
    await assert.rejects(e.plan(c.root, { env: { LANE: "beta", GITHUB_REPOSITORY: REPO }, fetchImpl: py, now: NOW }), /lane is engines or engines-test/);
    await assert.rejects(e.plan(c.root, { env: { GITHUB_REPOSITORY: REPO }, fetchImpl: fakeFetch({ [API]: 500 }), now: NOW }), /HTTP 500/);
  } finally {
    c.done();
  }
});

// ---- the seq gate ----

test("the live list: none when both files are absent, else verified or the plan stops", async () => {
  const key = makeKey();
  const pub = key.pub;
  assert.equal(await e.liveList(fakeFetch({}), REPO, "engines", pub), null);
  const files = liveFiles(key, "engines", 7);
  const got = await e.liveList(fakeFetch(files), REPO, "engines", pub);
  assert.deepEqual(got, { seq: 7, sha256: createHash("sha256").update(files[live("engines")]).digest("hex") });
  const bad = async (f, pattern) => assert.rejects(e.liveList(fakeFetch(f), REPO, "engines", pub), pattern);
  await bad({ [live("engines")]: files[live("engines")] }, /engines\.json but no engines\.json\.sig/);
  await bad({ [liveSig("engines")]: files[liveSig("engines")] }, /engines\.json\.sig but no engines\.json/);
  await bad(liveFiles(key, "engines", 7, { sign: (text) => makeKey().sign(Buffer.from(text)) }), /does not verify: the signature is by another key/);
  await bad(liveFiles(key, "engines", 7, { sign: (text) => key.sign(Buffer.from(text), "timestamp:1\tfile:engines.json\tversion:0.1.0") }), /does not verify: the trusted comment is not/);
  await bad({ ...files, [live("engines")]: files[live("engines")].replace('"seq": 7', '"seq": 9') }, /does not verify: the signature does not match the file/);
  await bad(liveFiles(key, "engines", 7, { change: (l) => ({ ...l, apps: [] }) }), /the live engines\/engines\.json: self-check: apps is empty/);
  await bad({ ...files, [liveSig("engines")]: 403 }, /HTTP 403/);
});

test("the seq gate: above live, at most 2^16 above it, at most 2^32", () => {
  e.seqGate(1, 0);
  e.seqGate(3, 2);
  e.seqGate(2 + e.MAX_STEP, 2);
  assert.throws(() => e.seqGate(2, 2), /not above the live list's 2/);
  assert.throws(() => e.seqGate(1, 2), /not above/);
  assert.throws(() => e.seqGate(3 + e.MAX_STEP, 2), /more than 2\^16 above the live list's 2/);
  assert.throws(() => e.seqGate(e.MAX_STEP + 1, 0), /more than 2\^16/);
  assert.throws(() => e.seqGate(e.MAX_SEQ + 1, e.MAX_SEQ), /2\^32/);
  assert.equal(e.chooseSeq(5, null), 5);
  assert.equal(e.chooseSeq(5, 4), 5);
  assert.equal(e.chooseSeq(5, 5), 6);
  assert.equal(e.floorOf(null, null), null);
  assert.equal(e.floorOf({ seq: 6 }, null), 6);
  assert.equal(e.floorOf(null, 3), 3);
  assert.equal(e.floorOf({ seq: 6 }, 9), 9);
  assert.equal(e.seqChoice({ fileSeq: 2, live: { seq: 6, sha256: "ab" }, seq: 7 }), "engines.seq 2, live 6 (verified, sha256 ab), archived none, chosen 7 (floor + 1)");
  assert.equal(e.seqChoice({ fileSeq: 8, live: { seq: 6, sha256: "ab" }, archived: 6, seq: 8 }), "engines.seq 8, live 6 (verified, sha256 ab), archived 6 (verified), chosen 8 (engines.seq is above the floor)");
});

test("publish's live check: the live list must be the one plan saw, and the seq still pass", async () => {
  const sha = "a".repeat(64);
  e.liveUnchanged("none", null, 2);
  e.liveUnchanged(sha, { seq: 6, sha256: sha }, 7);
  assert.throws(() => e.liveUnchanged("none", { seq: 6, sha256: sha }, 7), /changed since plan: none then, a{64} now/);
  assert.throws(() => e.liveUnchanged(sha, null, 7), /changed since plan: a{64} then, none now/);
  assert.throws(() => e.liveUnchanged(sha, { seq: 6, sha256: "b".repeat(64) }, 7), /changed since plan/);
  assert.throws(() => e.liveUnchanged(sha, { seq: 7, sha256: sha }, 7), /not above/);
  assert.throws(() => e.liveUnchanged(sha, { seq: 6, sha256: sha }, 7 + e.MAX_STEP), /more than 2\^16/);
  assert.throws(() => e.liveUnchanged(sha, { seq: 6, sha256: sha }, 7, 9), /not above the live list's 9/);
  e.liveUnchanged(sha, { seq: 6, sha256: sha }, 10, 9);
  // From the command line, against the real download and verification.
  const key = makeKey();
  const c = checkout(key);
  try {
    const files = liveFiles(key, "engines", 6);
    const liveSha = createHash("sha256").update(files[live("engines")]).digest("hex");
    const run = (env, f) => e.main(["--root", c.root, "--live-check"], { env: { GITHUB_REPOSITORY: REPO, LANE: "engines", ...env }, fetchImpl: fakeFetch(f), log: () => {} });
    assert.equal(await run({ SEQ: "7", PLANNED_LIVE_SHA256: liveSha, PLANNED_FLOOR: "0" }, files), 0);
    assert.equal(await run({ SEQ: "2", PLANNED_LIVE_SHA256: "none", PLANNED_FLOOR: "0" }, {}), 0);
    await assert.rejects(run({ SEQ: "7", PLANNED_LIVE_SHA256: "none", PLANNED_FLOOR: "0" }, files), /changed since plan/);
    await assert.rejects(run({ SEQ: "7", PLANNED_LIVE_SHA256: liveSha, PLANNED_FLOOR: "0" }, { [live("engines")]: files[live("engines")] }), /no engines\.json\.sig/);
    await assert.rejects(run({ SEQ: "", PLANNED_LIVE_SHA256: liveSha, PLANNED_FLOOR: "0" }, files), /not plan's output/);
  } finally {
    c.done();
  }
});

test("the plan's seq: engines.seq when above the live list, else live + 1", async () => {
  const key = makeKey();
  const c = checkout(key, { seq: "5\n" });
  try {
    const env = { GITHUB_REPOSITORY: REPO };
    const at = async (n) => (await e.plan(c.root, { env, fetchImpl: fakeFetch(n === null ? {} : liveFiles(key, "engines", n)), now: NOW })).list.seq;
    assert.equal(await at(null), 5);
    assert.equal(await at(4), 5);
    assert.equal(await at(5), 6);
    assert.equal(await at(9), 10);
    await assert.rejects(at(e.MAX_SEQ), /2\^32/);
    writeFileSync(join(c.root, e.LIVE, "engines.seq"), `${9 + e.MAX_STEP + 1}\n`);
    await assert.rejects(at(9), /more than 2\^16 above the live list's 9/);
    assert.equal(await at(10), 10 + e.MAX_STEP);
    writeFileSync(join(c.root, e.LIVE, "engines.seq"), "5 \n");
    await assert.rejects(at(null), /engines.seq is not one decimal integer/);
  } finally {
    c.done();
  }
});

test("the archived floor: the highest <lane>-<n>.json that verifies, and the plan's seq above it", async () => {
  const key = makeKey();
  const api = `https://api.github.com/repos/${REPO}`;
  const archive = (n, sign = (text) => key.sign(Buffer.from(text)), seq = n) => {
    const text = e.canonical({ ...good(), seq });
    return { [e.assetUrl(REPO, "engines", `engines-${n}.json`)]: text, [e.assetUrl(REPO, "engines", `engines-${n}.json.sig`)]: sign(text) };
  };
  const names = (...ns) => JSON.stringify(ns.map((n) => ({ name: n })));
  const release = (assets, pages = { 1: assets }) => ({
    [`${api}/releases/tags/engines`]: '{"id": 42}',
    ...Object.fromEntries(Object.entries(pages).map(([page, body]) => [`${api}/releases/42/assets?per_page=100&page=${page}`, body])),
  });
  const floor = (files) => e.archivedFloor(fakeFetch(files), REPO, "engines", key.pub, "t0ken");
  assert.equal(await floor({}), null);
  assert.equal(await floor(release("[]")), null);
  const assets = names("engines.json", "engines.json.sig", "engines-3.json", "engines-3.json.sig", "engines-9.json", "engines-9.json.sig", "engines-12.json", "engines-test-40.json");
  assert.equal(await floor({ ...release(assets), ...archive(3), ...archive(9), ...archive(12) }), 12);
  // A forged, mislabelled or half-missing higher one is passed over for the next that verifies.
  assert.equal(await floor({ ...release(assets), ...archive(3), ...archive(9), ...archive(12, (text) => makeKey().sign(Buffer.from(text))) }), 9);
  assert.equal(await floor({ ...release(assets), ...archive(3), ...archive(9, undefined, 4), [e.assetUrl(REPO, "engines", "engines-12.json")]: "{}" }), 3);
  // Every page is read.
  const full = JSON.stringify(Array.from({ length: 100 }, (_, i) => ({ name: `other-${i}` })));
  assert.equal(await floor({ ...release(null, { 1: full, 2: names("engines-5.json") }), ...archive(5) }), 5);
  await assert.rejects(floor({ ...release("{}") }), /not a list/);
  // At most 20 tries, highest first; archives with none of those verifying stop the plan.
  const many = Array.from({ length: 25 }, (_, i) => i + 1);
  const forged = Object.assign({}, ...many.slice(5).map((n) => archive(n, (text) => makeKey().sign(Buffer.from(text)))));
  const manyRelease = release(names(...many.map((n) => `engines-${n}.json`)));
  await assert.rejects(floor({ ...manyRelease, ...forged, ...archive(5) }), /^Error: 25 archived lists, none of the highest 20 verifies$/);
  assert.equal(await floor({ ...manyRelease, ...forged, ...archive(6) }), 6);
  await assert.rejects(floor({ ...release(names("engines-3.json")) }), /1 archived lists, none of the highest 20 verifies/);
  // In the plan: the floor is max(live, archived), and the seq goes one above it.
  const c = checkout(key);
  try {
    const files = { ...liveFiles(key, "engines", 6), ...release(assets), ...archive(9) };
    const p = await e.plan(c.root, { env: { GITHUB_REPOSITORY: REPO, GITHUB_REF: "refs/heads/main", GITHUB_SHA: "abc" }, fetchImpl: fakeFetch(files), now: NOW });
    assert.equal(p.list.seq, 10);
    assert.match(p.summary, /- ref: `refs\/heads\/main`\n- commit: `abc`\n- seq: engines\.seq 2, live 6 \(verified, sha256 [0-9a-f]{64}\), archived 9 \(verified\), chosen 10 \(floor \+ 1\)\n- sha256: `[0-9a-f]{64}`\n/);
  } finally {
    c.done();
  }
});

// ---- rollback ----

test("rollback: the archived list, its signature checked, again with seq live + 1 and generated now", async () => {
  const key = makeKey();
  const c = checkout(key);
  try {
    const old = { ...good(), seq: 3, generated: "2026-10-01T00:00:00Z", apps: ["0.1.0"] };
    const oldText = e.canonical(old);
    const files = {
      ...liveFiles(key, "engines", 6),
      [e.assetUrl(REPO, "engines", "engines-3.json")]: oldText,
      [e.assetUrl(REPO, "engines", "engines-3.json.sig")]: key.sign(Buffer.from(oldText)),
    };
    const env = { GITHUB_REPOSITORY: REPO, ROLLBACK_TO: "3" };
    const p = await e.plan(c.root, { env, fetchImpl: fakeFetch(files), now: NOW });
    assert.deepEqual(p.list, { ...old, seq: 7, generated: "2026-11-24T06:00:00Z" });
    assert.ok(p.summary.includes("(rollback to 3)") && /seq: engines\.seq 2, live 6 \(verified, sha256 [0-9a-f]{64}\), archived none, chosen 7 \(rollback to 3: floor \+ 1\)/.test(p.summary));
    // The test lane reads its own release and its own key file.
    const testFiles = {
      ...liveFiles(key, "engines-test", 6),
      [e.assetUrl(REPO, "engines-test", "engines-test-3.json")]: oldText,
      [e.assetUrl(REPO, "engines-test", "engines-test-3.json.sig")]: key.sign(Buffer.from(oldText)),
    };
    const t = await e.plan(c.root, { env: { ...env, LANE: "engines-test" }, fetchImpl: fakeFetch(testFiles), now: NOW });
    assert.equal(t.list.seq, 7);

    const other = makeKey();
    const refuse = async (change, pattern) => {
      const f = { ...files };
      change(f);
      await assert.rejects(e.plan(c.root, { env, fetchImpl: fakeFetch(f), now: NOW }), pattern);
    };
    await refuse((f) => (f[e.assetUrl(REPO, "engines", "engines-3.json.sig")] = other.sign(Buffer.from(oldText))), /signature: the signature is by another key/);
    await refuse((f) => (f[e.assetUrl(REPO, "engines", "engines-3.json")] = oldText.replace('"0.1.0"', '"9.9.9"')), /does not match the file/);
    await refuse((f) => delete f[e.assetUrl(REPO, "engines", "engines-3.json.sig")], /no archived list 3/);
    await refuse((f) => Object.assign(f, liveFiles(key, "engines", 3)), /rollback_to 3 is not below the floor 3/);
    await refuse((f) => {
      delete f[live("engines")];
      delete f[liveSig("engines")];
    }, /not below the floor \(none\)/);
    const lie = e.canonical({ ...old, seq: 4 });
    await refuse((f) => {
      f[e.assetUrl(REPO, "engines", "engines-3.json")] = lie;
      f[e.assetUrl(REPO, "engines", "engines-3.json.sig")] = key.sign(Buffer.from(lie));
    }, /says seq 4/);
    for (const bad of ["0", "x", "-1", "03", String(e.MAX_SEQ + 1)]) assert.throws(() => e.parseRollback(bad), /not a seq/, bad);
    assert.equal(e.parseRollback(""), null);
  } finally {
    c.done();
  }
});

// ---- the signature ----

test("the signature check: prehashed, the lane's key, and the trusted comment exactly timestamp and file", () => {
  const key = makeKey();
  const data = Buffer.from(e.canonical(good()));
  assert.deepEqual(e.engineSignatureProblems(data, key.sign(data), key.pub), []);
  const bad = (sig, pattern) => assert.match(e.engineSignatureProblems(data, sig, key.pub).join("; "), pattern);
  bad(key.sign(data, "timestamp:1795500000\tfile:engines.json\tversion:0.1.0"), /trusted comment is not/);
  bad(key.sign(data, "timestamp:1795500000\tfile:latest.json"), /trusted comment is not/);
  bad(key.sign(data, "timestamp:\tfile:engines.json"), /trusted comment is not/);
  bad(key.sign(data, "file:engines.json"), /trusted comment is not/);
  bad(key.sign(data, undefined, "Ed"), /not prehashed/);
  bad(makeKey().sign(data), /another key/);
  bad(key.sign(Buffer.from("other")), /does not match the file/);
  assert.ok(e.commentIsExact("timestamp:0\tfile:engines.json"));
  for (const c of ["timestamp:1\tfile:engines.json ", "timestamp:1 \tfile:engines.json", `timestamp:${"1".repeat(21)}\tfile:engines.json`, "timestamp:1\tfile:engines.json\n"]) {
    assert.ok(!e.commentIsExact(c), JSON.stringify(c));
  }
});

test("--check-signature and --self-check from the command line", async () => {
  const key = makeKey();
  const dir = mkdtempSync(join(tmpdir(), "engines-publish-"));
  try {
    const text = e.canonical(good());
    writeFileSync(join(dir, "engines.json"), text);
    writeFileSync(join(dir, "engines.json.sig"), key.sign(Buffer.from(text)));
    writeFileSync(join(dir, "lane.pub"), key.pub);
    const lines = [];
    const log = (l) => lines.push(l);
    assert.equal(await e.main(["--check-signature", join(dir, "engines.json"), join(dir, "lane.pub")], { log }), 0);
    assert.equal(await e.main(["--self-check", join(dir, "engines.json")], { log }), 0);
    writeFileSync(join(dir, "engines.json.sig"), key.sign(Buffer.from(text), "timestamp:1\tfile:engines.json\tversion:1.0.0"));
    assert.equal(await e.main(["--check-signature", join(dir, "engines.json"), join(dir, "lane.pub")], { log }), 1);
    writeFileSync(join(dir, "engines.json"), JSON.stringify(good()));
    await assert.rejects(e.main(["--self-check", join(dir, "engines.json")], { log }), /not canonical/);
    assert.equal(await e.main(["--sign"], { log }), 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("--plan writes engines.json and the step outputs; --pubkey writes each lane's key", async () => {
  const key = makeKey();
  const c = checkout(key);
  const dir = mkdtempSync(join(tmpdir(), "engines-publish-out-"));
  try {
    const env = { GITHUB_REPOSITORY: REPO, GITHUB_OUTPUT: join(dir, "out"), GITHUB_STEP_SUMMARY: join(dir, "summary"), GITHUB_SHA: "abc123" };
    assert.equal(await e.main(["--root", c.root, "--plan", join(dir, "list")], { env, fetchImpl: fakeFetch({}), now: NOW, log: () => {} }), 0);
    const text = readFileSync(join(dir, "list/engines.json"), "utf8");
    const sha = createHash("sha256").update(text).digest("hex");
    assert.equal(readFileSync(join(dir, "out"), "utf8"), `seq=2\nlive_seq=none\nlive_sha256=none\nfloor_seq=0\nsha256=${sha}\n`);
    assert.match(readFileSync(join(dir, "summary"), "utf8"), /commit: `abc123`/);
    assert.equal(await e.main(["--root", c.root, "--pubkey", "engines-test", join(dir, "pub")], { log: () => {} }), 0);
    assert.equal(readFileSync(join(dir, "pub"), "utf8"), `${key.pub}\n`);
    // The real checkout's keys are both minisign public keys, and not the same one.
    assert.notEqual(e.lanePubkey(ROOT, "engines"), e.lanePubkey(ROOT, "engines-test"));
    writeFileSync(join(c.root, "crates/desktop/release/updater-test.pub"), "bm90IGEga2V5\n");
    assert.throws(() => e.lanePubkey(c.root, "engines-test"), /not base64 of a minisign public key/);
  } finally {
    c.done();
    rmSync(dir, { recursive: true, force: true });
  }
});

// ---- the self-check ----

test("the self-check holds for the live pair and refuses each broken rule", () => {
  e.selfCheck(good());
  const lockKey = Object.keys(LIVE_LOCK.packages).find((k) => k.endsWith("/zod"));
  const cases = [
    [(l) => (l.extra = 1), /unknown field extra/],
    [(l) => delete l.sizes, /no sizes/],
    [(l) => (l.format = "clauseo-engines/2"), /format/],
    [(l) => (l.seq = 0), /seq/],
    [(l) => (l.seq = e.MAX_SEQ + 1), /seq/],
    [(l) => (l.seq = "2"), /seq/],
    [(l) => (l.generated = "2026-11-24T06:00:00.000Z"), /generated/],
    [(l) => (l.generated = "2026-13-24T06:00:00Z"), /generated/],
    [(l) => (l.apps = []), /apps is empty/],
    [(l) => (l.apps = [""]), /apps is empty/],
    [(l) => (l.sizes[0].download = 0), /positive/],
    [(l) => (l.sizes[0].os = "freebsd"), /names/],
    [(l) => (l.sizes[0].extra = 1), /other fields/],
    [(l) => l.sizes.push({ ...l.sizes[0] }), /repeats/],
    [(l) => (l.package_json.scripts = { postinstall: "x" }), /a field an engines pin file does not/],
    [(l) => (l.package_json.dependencies.leftpad = "1.0.0"), /not exactly/],
    [(l) => delete l.package_json.dependencies["@agentclientprotocol/codex-acp"], /not exactly/],
    [(l) => (l.package_json.overrides.leftpad = "1.0.0"), /overrides/],
    [(l) => (l.package_json.dependencies["@agentclientprotocol/codex-acp"] = "^2.10.0"), /not one exact version/],
    [(l) => (l.package_json.overrides["@openai/codex"] = "npm:@evil/codex@1.0.0"), /not one exact version/],
    [(l) => (l.package_lock.lockfileVersion = 2), /v3 lock/],
    [(l) => (l.package_lock.packages[""].dependencies = {}), /root dependencies/],
    [(l) => (l.package_lock.packages["../evil"] = l.package_lock.packages[lockKey]), /is not node_modules/],
    [(l) => (l.package_lock.packages["node_modules/a\\b"] = l.package_lock.packages[lockKey]), /is not node_modules/],
    [(l) => (l.package_lock.packages[lockKey].link = true), /link/],
    [(l) => (l.package_lock.packages[lockKey].inBundle = true), /bundled/],
    [(l) => delete l.package_lock.packages[lockKey].integrity, /exactly one sha512/],
    [(l) => (l.package_lock.packages[lockKey].integrity += " sha512-" + "A".repeat(86) + "=="), /exactly one sha512/],
    [(l) => (l.package_lock.packages[lockKey].integrity = "sha1-" + "A".repeat(26) + "="), /exactly one sha512/],
    [(l) => (l.package_lock.packages[lockKey].resolved = "https://evil.example/zod.tgz"), /resolved is not https:\/\/registry\.npmjs\.org\/zod\/-\/zod-/],
    [(l) => (l.package_lock.packages[lockKey].resolved = "git+https://github.com/colinhacks/zod.git"), /resolved/],
    [(l) => (l.package_lock.packages[lockKey].name = "../x"), /not an npm name/],
    [(l) => (l.package_lock.packages["node_modules/@openai/codex"].version = "0.1.0"), /resolved|does not install/],
  ];
  for (const [change, pattern] of cases) {
    const list = good();
    change(list);
    assert.throws(() => e.selfCheck(list), pattern, String(change));
  }
  // An installed engine at another version than its pin, its tarball consistent.
  const list = good();
  const entry = list.package_lock.packages["node_modules/@openai/codex"];
  entry.resolved = entry.resolved.replace(entry.version, "0.1.0");
  entry.version = "0.1.0";
  assert.throws(() => e.selfCheck(list), /does not install @openai\/codex/);
  assert.equal(e.installedName("node_modules/@a/b/node_modules/c"), "c");
  assert.equal(e.installedName("node_modules/.bin"), null);
  assert.equal(e.installedName("node_modules/@a"), null);
});
