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
    assert.equal(p.live, 0);
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
    for (const want of ["seq: 0 → 2", "`@agentclientprotocol/codex-acp` 2.10.0", "apps: 0.0.0", p.sha256, "<details>", '"format": "clauseo-engines/1"']) {
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
    assert.deepEqual(quiet.seen.map((s) => s.url), [live("engines-test")], "the test lane reads no API");
    await assert.rejects(e.plan(c.root, { env: { LANE: "beta", GITHUB_REPOSITORY: REPO }, fetchImpl: py, now: NOW }), /lane is engines or engines-test/);
    await assert.rejects(e.plan(c.root, { env: { GITHUB_REPOSITORY: REPO }, fetchImpl: fakeFetch({ [API]: 500 }), now: NOW }), /HTTP 500/);
  } finally {
    c.done();
  }
});

// ---- the seq gate ----

test("the seq gate: a 404 is 0, and only a seq above the live one passes", async () => {
  assert.equal(await e.liveSeq(fakeFetch({}), REPO, "engines"), 0);
  assert.equal(await e.liveSeq(fakeFetch({ [live("engines")]: '{"seq": 7}' }), REPO, "engines"), 7);
  await assert.rejects(e.liveSeq(fakeFetch({ [live("engines")]: "<html>" }), REPO, "engines"), /not JSON/);
  await assert.rejects(e.liveSeq(fakeFetch({ [live("engines")]: '{"seq": -1}' }), REPO, "engines"), /whole number/);
  await assert.rejects(e.liveSeq(fakeFetch({ [live("engines")]: 403 }), REPO, "engines"), /HTTP 403/);
  e.seqGate(1, 0);
  e.seqGate(3, 2);
  assert.throws(() => e.seqGate(2, 2), /not above the live list's 2/);
  assert.throws(() => e.seqGate(1, 2), /not above/);
  assert.throws(() => e.seqGate(e.MAX_SEQ + 1, 2), /2\^32/);
});

test("the plan's seq: engines.seq when above the live list, else live + 1", async () => {
  const key = makeKey();
  const c = checkout(key, { seq: "5\n" });
  try {
    const env = { GITHUB_REPOSITORY: REPO };
    const at = async (n) => (await e.plan(c.root, { env, fetchImpl: fakeFetch(n === null ? {} : { [live("engines")]: `{"seq": ${n}}` }), now: NOW })).list.seq;
    assert.equal(await at(null), 5);
    assert.equal(await at(4), 5);
    assert.equal(await at(5), 6);
    assert.equal(await at(9), 10);
    await assert.rejects(at(e.MAX_SEQ), /2\^32/);
    writeFileSync(join(c.root, e.LIVE, "engines.seq"), "5 \n");
    await assert.rejects(at(null), /engines.seq is not one decimal integer/);
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
      [live("engines")]: '{"seq": 6}',
      [e.assetUrl(REPO, "engines", "engines-3.json")]: oldText,
      [e.assetUrl(REPO, "engines", "engines-3.json.sig")]: key.sign(Buffer.from(oldText)),
    };
    const env = { GITHUB_REPOSITORY: REPO, ROLLBACK_TO: "3" };
    const p = await e.plan(c.root, { env, fetchImpl: fakeFetch(files), now: NOW });
    assert.deepEqual(p.list, { ...old, seq: 7, generated: "2026-11-24T06:00:00Z" });
    assert.ok(p.summary.includes("(rollback to 3)") && p.summary.includes("seq: 6 → 7"));
    // The test lane reads its own release and its own key file.
    const testFiles = {
      [live("engines-test")]: '{"seq": 6}',
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
    await refuse((f) => (f[live("engines")] = '{"seq": 3}'), /rollback_to 3 is not below the live seq 3/);
    await refuse((f) => delete f[live("engines")], /not below the live seq 0/);
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
    assert.equal(readFileSync(join(dir, "out"), "utf8"), `seq=2\nlive_seq=0\nsha256=${sha}\n`);
    assert.match(readFileSync(join(dir, "summary"), "utf8"), /commit: abc123/);
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

// ---- engines-publish.yml ----

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

test("engines-publish.yml: dispatch only, plan without secrets, publish the only job with an environment", () => {
  const yml = read(".github/workflows/engines-publish.yml");
  assert.match(yml, /^# /, "a header comment first");
  const code = yml.replace(/^\s*#.*\n/gm, "");
  for (const want of [/main.*deployment|deployments from `main`/s, /release-test` must get a required reviewer/]) assert.match(yml.slice(0, yml.indexOf("\nname:")), want);
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

  const ps = steps(publish);
  const index = (pattern) => ps.findIndex((s) => pattern.test(s));
  const [download, recheck, install, probe, signing, verify, upload] = [
    /download-artifact@/, /\[ "\$got" = "\$PLANNED_SHA256" \] \|\| fail/, /pnpm -C ui install --frozen-lockfile/,
    /name: The lane's key signs/, /name: Sign engines\.json/, /name: Verify the signature/, /gh release upload/,
  ].map(index);
  assert.ok(download >= 0 && download < recheck && recheck < install && install < probe && probe < signing && signing < verify && verify < upload, "publish's order");
  // The key: the probe and the sign step only, through env:, by lane.
  const withKey = ps.filter((s) => /secrets\.|TAURI_SIGNING_PRIVATE_KEY/.test(s)).map((s) => /name: (.*)/.exec(s)[1]);
  assert.deepEqual(withKey, ["The lane's key signs, and the lane's public key verifies it", "Sign engines.json"]);
  for (const i of [probe, signing]) {
    assert.match(ps[i], /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ inputs\.lane == 'engines-test' && secrets\.TAURI_SIGNING_PRIVATE_KEY_TEST \|\| inputs\.lane == 'engines' && secrets\.TAURI_SIGNING_PRIVATE_KEY \|\| '' \}\}/);
    assert.match(ps[i], /TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \$\{\{ inputs\.lane == 'engines-test' && secrets\.TAURI_SIGNING_PRIVATE_KEY_TEST_PASSWORD \|\| inputs\.lane == 'engines' && secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD \|\| '' \}\}/);
  }
  assert.match(ps[probe], /latest-json\.mjs --verify "\$dir\/\$name" "\$RUNNER_TEMP\/lane\.pub"/);
  // Sign exactly the planned file, once, and nothing else.
  assert.deepEqual([...ps[signing].matchAll(/tauri signer sign (.*)/g)].map((m) => m[1]), ['"$RUNNER_TEMP/engines/engines.json" > /dev/null']);
  assert.match(ps[verify], /\[ "\$got" = "\$PLANNED_SHA256" \]/);
  assert.match(ps[verify], /latest-json\.mjs --verify "\$RUNNER_TEMP\/engines\/engines\.json" "\$RUNNER_TEMP\/lane\.pub"/);
  assert.match(ps[verify], /engines-publish\.mjs --check-signature/);
  assert.doesNotMatch(code, /--app-version/);
  // The release: a pre-release never latest; the archive, then the .sig, then the list.
  const up = ps[upload];
  assert.match(up, /gh release create "\$LANE" [^\n]*--prerelease --latest=false --target main/);
  assert.doesNotMatch(code, /--latest(?!=false)|make_latest|gh release edit/);
  const order = ["$LANE-$SEQ.json.sig\"", "engines.json.sig\" --repo", "/engines.json\" --repo"].map((s) => up.indexOf(s));
  assert.ok(order.every((i) => i > 0) && order[0] < order[1] && order[1] < order[2], "upload order");
  assert.match(up, /GH_TOKEN: \$\{\{ github\.token \}\}/);
  assert.doesNotMatch(code, /--no-verify|--force/);
  // No expression inside a run: script, where its text would be run as shell.
  const runs = [...yml.matchAll(/^(\s+)run: (\|\n(?:\1 {2}.*\n|\n)*|.*\n)/gm)].map((m) => m[2]);
  assert.ok(runs.length >= 7);
  for (const run of runs) assert.doesNotMatch(run, /\$\{\{/, run);
});
