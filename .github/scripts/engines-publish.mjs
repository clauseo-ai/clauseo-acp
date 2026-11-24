// Engines publish: builds the signed engine list engines.json (format clauseo-engines/1) from G16's
// pins (crates/limits/tests/live/package.json, its lock and engines.seq) and the snapshot's SIZES,
// for engines-publish.yml, which signs it with the updater key and uploads it to the release
// `engines` (or `engines-test`). crates/engines (clauseo-engines) is what the app checks it with;
// the self-check here mirrors its rules and does not replace them. Plain Node 24, no install;
// engines-publish.test.mjs tests it.
//
//   node .github/scripts/engines-publish.mjs --plan <out-dir> [--root <dir>]
//   node .github/scripts/engines-publish.mjs --pubkey <lane> <out-file> [--root <dir>]
//   node .github/scripts/engines-publish.mjs --check-signature <file> <public-key-file>
//   node .github/scripts/engines-publish.mjs --self-check <file>
//   node .github/scripts/engines-publish.mjs --live-check [--root <dir>]
//
// --plan reads LANE (engines or engines-test), ROLLBACK_TO (empty or a seq), APPS (engines-test
// only: comma-separated exact versions), GITHUB_REPOSITORY and GH_TOKEN (optional, for the GitHub
// API) from the environment, and writes <out-dir>/engines.json, canonical JSON (as
// crates/agents/src/pins.rs writes it). The live list is the release assets <lane>/engines.json
// and its .sig: both absent is none; one alone, or a pair whose signature (the lane's key, the
// exact trusted comment) or self-check fails, stops the plan, so an unverified seq is never used.
// The seq is engines.seq with no live list, else max(engines.seq, live + 1); a rollback
// (ROLLBACK_TO=<n>) takes the archived <lane>-<n>.json, whose signature must hold under the lane's
// key, and publishes its apps, pins and sizes again with seq live + 1 and generated now. The new
// seq must be above the live one and at most 2^16 above it (the verifier's ceiling). On the engines lane apps are tauri.conf.json's version and the latest published v*
// release's (a 404 or a tag that is not v<X.Y.Z> adds none). Under GitHub Actions it writes seq,
// live_seq and live_sha256 (each `none` with no live list) and sha256 to $GITHUB_OUTPUT, and what
// will be signed, with the seq choice, to $GITHUB_STEP_SUMMARY.
// --live-check, run by publish right before signing, downloads and verifies the live list again
// and refuses unless it is the one plan saw (PLANNED_LIVE_SHA256, or none) and SEQ is still above it
// and within the ceiling.
// --pubkey writes the lane's updater public key (Tauri's base64 form): engines, tauri.conf.json's
// plugins.updater.pubkey; engines-test, crates/desktop/release/updater-test.pub (as release.yml's
// test mode reads it for CLAUSEO_UPDATER_PUBKEY).
// --check-signature: <file>.sig is a prehashed (ED) minisign signature of <file> under the key, and
// its trusted comment is exactly `timestamp:<digits>\tfile:engines.json`.
//
// Exit status: 0 when done, 1 on a refusal or failure, 2 on a usage error.

import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { signatureCheck } from "./latest-json.mjs";

export const FORMAT = "clauseo-engines/1";
export const LANES = { engines: "release", "engines-test": "release-test" };
export const MAX_SEQ = 2 ** 32;
/** How far above the floor a list's seq may be (crates/engines refuses seq > floor + 2^16). */
export const MAX_STEP = 2 ** 16;
export const REGISTRY = "https://registry.npmjs.org/";
export const DEPENDENCIES = ["@agentclientprotocol/claude-agent-acp", "@agentclientprotocol/codex-acp"];
export const OVERRIDES = ["@openai/codex"];
export const LIVE = "crates/limits/tests/live";
export const EMBEDDED = "crates/agents/src/snapshot/embedded.rs";
const FIELDS = ["format", "seq", "generated", "apps", "package_json", "package_lock", "sizes"];
const PACKAGE_JSON_FIELDS = ["name", "version", "private", "description", "license", "dependencies", "overrides"];
const AGENTS = ["claude-acp", "codex-acp"];
const OS = { Darwin: "darwin", Win32: "win32", Linux: "linux" };
const ARCH = { X64: "x64", Arm64: "arm64" };
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;
const TEST_APP = /^\d+\.\d+\.\d+(-dev\.[0-9a-f]{7,40})?$/;
const RELEASE_TAG = /^v(\d+\.\d+\.\d+)$/;

// ---- canonical JSON, as crates/agents/src/pins.rs's `canonical` ----

/** Keys sorted at every level (by code unit, as Rust's BTreeMap sorts ASCII), two-space indent,
 * `": "`, `{}` and `[]` when empty, and a trailing newline. */
export function canonical(value) {
  const write = (v, depth) => {
    const pad = (d) => "  ".repeat(d);
    if (Array.isArray(v)) {
      if (v.length === 0) return "[]";
      return `[\n${v.map((item) => pad(depth + 1) + write(item, depth + 1)).join(",\n")}\n${pad(depth)}]`;
    }
    if (v !== null && typeof v === "object") {
      const keys = Object.keys(v).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
      if (keys.length === 0) return "{}";
      const rows = keys.map((k) => `${pad(depth + 1)}${JSON.stringify(k)}: ${write(v[k], depth + 1)}`);
      return `{\n${rows.join(",\n")}\n${pad(depth)}}`;
    }
    if (typeof v === "number" && !Number.isSafeInteger(v)) throw new Error(`not a whole number: ${v}`);
    return JSON.stringify(v);
  };
  return `${write(value, 0)}\n`;
}

// ---- SIZES in embedded.rs ----
// Copied from parseSizes in .github/scripts/engine-watch-lib.mjs (engine-watch, PR #1307), which
// was not on main yet; once it is, import it from there instead. Only the rows are read here.
const SIZES_RE = /const SIZES: \[\(&str, Os, Arch, u64, u64\); (\d+)\] = \[\n([\s\S]*?)\n\];\n/;

/** SIZES's rows, in file order, with npm's os and arch names. */
export function parseSizes(source) {
  const block = SIZES_RE.exec(source);
  if (!block) throw new Error("embedded.rs has no SIZES block");
  const rows = [];
  const flat = block[2].replace(/\s+/g, " ");
  for (const m of flat.matchAll(/\( ?"([a-z-]+)", Os::(\w+), Arch::(\w+), ([\d_]+), ([\d_]+),? ?\)/g)) {
    const [os, arch] = [OS[m[2]], ARCH[m[3]]];
    if (!os || !arch) throw new Error(`SIZES names Os::${m[2]} Arch::${m[3]}`);
    rows.push({ agent: m[1], os, arch, download: Number(m[4].replaceAll("_", "")), unpacked: Number(m[5].replaceAll("_", "")) });
  }
  if (rows.length !== Number(block[1])) throw new Error(`SIZES says ${block[1]} rows, ${rows.length} read`);
  return rows;
}

// ---- inputs ----

/** The lane, `engines` when empty. */
export function parseLane(text) {
  const lane = text || "engines";
  if (!Object.hasOwn(LANES, lane)) throw new Error(`lane is engines or engines-test, not ${JSON.stringify(text)}`);
  return lane;
}

/** ROLLBACK_TO: null when empty, else a seq from 1 to 2^32. */
export function parseRollback(text) {
  if (!text) return null;
  if (!/^[1-9]\d{0,9}$/.test(text) || Number(text) > MAX_SEQ) throw new Error(`rollback_to is not a seq: ${JSON.stringify(text)}`);
  return Number(text);
}

/** engines.seq: one decimal line. */
export function parseSeq(text) {
  if (!/^\d+\n?$/.test(text)) throw new Error(`engines.seq is not one decimal integer: ${JSON.stringify(text)}`);
  return Number(text.trim());
}

/** The engines lane's apps: main's version, then the latest v* release's, de-duplicated. */
export function enginesApps(confVersion, latestTag) {
  if (!SEMVER.test(confVersion ?? "")) throw new Error(`tauri.conf.json's version is not semver: ${JSON.stringify(confVersion)}`);
  const release = RELEASE_TAG.exec(latestTag ?? "");
  return [...new Set([confVersion, ...(release ? [release[1]] : [])])];
}

/** The test lane's apps: the APPS input, each an exact version or a dev build's, de-duplicated. */
export function testApps(text) {
  const apps = (text ?? "").split(",").map((s) => s.trim()).filter(Boolean);
  if (apps.length === 0) throw new Error("the engines-test lane needs apps (comma-separated versions)");
  for (const app of apps) {
    if (!TEST_APP.test(app)) throw new Error(`app ${JSON.stringify(app)} is not X.Y.Z or X.Y.Z-dev.<sha>`);
  }
  return [...new Set(apps)];
}

/** Refuses unless `next` is above the live seq (0 when there is no live list) and at most 2^16
 * above it. */
export function seqGate(next, live) {
  if (!(next > live)) throw new Error(`seq ${next} is not above the live list's ${live}`);
  if (next > live + MAX_STEP) throw new Error(`seq ${next} is more than 2^16 above the live list's ${live}`);
  if (next > MAX_SEQ) throw new Error(`seq ${next} is above 2^32`);
}

/** The seq to publish: engines.seq with no live list, else max(engines.seq, live + 1). */
export function chooseSeq(fileSeq, live) {
  return live === null ? fileSeq : Math.max(fileSeq, live.seq + 1);
}

/** Refuses unless the live list now is the one plan saw (by sha256, or both none), and `seq` still
 * passes the gate against it. */
export function liveUnchanged(plannedSha256, now, seq) {
  const got = now === null ? "none" : now.sha256;
  if (got !== plannedSha256) throw new Error(`the live list changed since plan: ${plannedSha256} then, ${got} now`);
  seqGate(seq, now === null ? 0 : now.seq);
}

/** RFC 3339 UTC to the second. */
export function stamp(date) {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

// ---- the network (fetchImpl stands in for fetch in the tests) ----

/** The waits before each try (the tests set them to 0). */
export const retry = { waits: [0, 1000, 4000] };

/** `url`'s body as text, or null on a 404; retries 5xx, 429 and network errors. */
export async function getText(fetchImpl, url, headers = {}) {
  let res;
  for (const wait of retry.waits) {
    if (wait) await new Promise((r) => setTimeout(r, wait));
    try {
      res = await fetchImpl(url, { redirect: "follow", headers });
    } catch (err) {
      res = err;
      continue;
    }
    if (res.status === 404) return null;
    if (res.ok || (res.status < 500 && res.status !== 429)) break;
  }
  if (res instanceof Error) throw res;
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return res.text();
}

export const assetUrl = (repo, lane, name) => `https://github.com/${repo}/releases/download/${lane}/${name}`;

/** The live list, verified: null when neither engines.json nor its .sig is on the release, else
 * { seq, sha256 }. One without the other, a signature that fails under `pubkey` or a list that fails
 * the self-check throws: an unverified seq is never used. */
export async function liveList(fetchImpl, repo, lane, pubkey) {
  const [json, sig] = await Promise.all(["engines.json", "engines.json.sig"].map((name) => getText(fetchImpl, assetUrl(repo, lane, name))));
  if (json === null && sig === null) return null;
  if (json === null || sig === null) throw new Error(`the live ${lane} release has ${json === null ? "engines.json.sig but no engines.json" : "engines.json but no engines.json.sig"}`);
  const problems = engineSignatureProblems(Buffer.from(json), sig, pubkey);
  if (problems.length) throw new Error(`the live ${lane}/engines.json does not verify: ${problems.join("; ")}`);
  let list;
  try {
    list = selfCheck(JSON.parse(json));
  } catch (err) {
    throw new Error(`the live ${lane}/engines.json: ${err.message}`);
  }
  return { seq: list.seq, sha256: createHash("sha256").update(json).digest("hex") };
}

/** The latest release's tag (GitHub's `releases/latest`), null when there is none. */
export async function latestTag(fetchImpl, repo, token) {
  const headers = { accept: "application/vnd.github+json", ...(token ? { authorization: `Bearer ${token}` } : {}) };
  const text = await getText(fetchImpl, `https://api.github.com/repos/${repo}/releases/latest`, headers);
  if (text === null) return null;
  const tag = JSON.parse(text).tag_name;
  return typeof tag === "string" ? tag : null;
}

// ---- the signature ----

/** What is wrong with `sig` (a .sig file's text) for the engine list `data` under `publicKey`:
 * latest-json.mjs's check, a prehashed (ED) signature, and the exact trusted comment. */
export function engineSignatureProblems(data, sig, publicKey) {
  const problems = signatureCheck(data, sig, publicKey);
  const lines = Buffer.from(String(sig).trim(), "base64").toString("utf8").split("\n");
  if (Buffer.from(lines[1] ?? "", "base64").subarray(0, 2).toString("latin1") !== "ED") {
    problems.push("the signature is not prehashed (ED)");
  }
  if (!commentIsExact((lines[2] ?? "").replace(/^trusted comment: /, ""))) {
    problems.push(`the trusted comment is not timestamp:<digits>\\tfile:engines.json: ${JSON.stringify(lines[2] ?? "")}`);
  }
  return problems;
}

/** `timestamp:<1 to 20 digits>\tfile:engines.json`, exactly. */
export function commentIsExact(comment) {
  return /^timestamp:\d{1,20}\tfile:engines\.json$/.test(comment);
}

// ---- the self-check, a mirror of crates/engines's rules ----

const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
const NAME_PART = /^[A-Za-z0-9~-][A-Za-z0-9._~-]*$/;
const isName = (n) => {
  if (typeof n !== "string" || n.length === 0 || n.length > 214) return false;
  const m = /^@([^/]+)\/([^/]+)$/.exec(n);
  return m ? NAME_PART.test(m[1]) && NAME_PART.test(m[2]) : !n.includes("/") && NAME_PART.test(n);
};

/** The last name of a lock key `node_modules/<name>(/node_modules/<name>)*`, else null. */
export function installedName(path) {
  const m = /^node_modules\/((?:@[^/]+\/)?[^/]+)((?:\/node_modules\/(?:@[^/]+\/)?[^/]+)*)$/.exec(path);
  if (!m) return null;
  const names = [m[1], ...m[2].split("/node_modules/").slice(1)];
  return names.every(isName) ? names.at(-1) : null;
}

/** Every rule a list must hold (crates/engines's lib.rs, rules 4 to 6); throws the first broken. */
export function selfCheck(list) {
  const fail = (what) => {
    throw new Error(`self-check: ${what}`);
  };
  if (!isObject(list)) fail("not a JSON object");
  for (const key of Object.keys(list)) if (!FIELDS.includes(key)) fail(`unknown field ${key}`);
  for (const key of FIELDS) if (!(key in list)) fail(`no ${key}`);
  if (list.format !== FORMAT) fail(`format is not ${FORMAT}`);
  if (!Number.isSafeInteger(list.seq) || list.seq < 1 || list.seq > MAX_SEQ) fail("seq is not 1 to 2^32");
  if (typeof list.generated !== "string" || !/^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T([01]\d|2[0-3]):[0-5]\d:[0-5]\dZ$/.test(list.generated)) {
    fail("generated is not YYYY-MM-DDTHH:MM:SSZ");
  }
  if (!Array.isArray(list.apps) || list.apps.length === 0 || list.apps.some((a) => typeof a !== "string" || a === "")) fail("apps is empty");
  if (!Array.isArray(list.sizes)) fail("sizes is not a list");
  const seen = new Set();
  for (const row of list.sizes) {
    if (!isObject(row) || Object.keys(row).sort().join() !== "agent,arch,download,os,unpacked") fail("a sizes row has other fields");
    if (!AGENTS.includes(row.agent) || !Object.values(OS).includes(row.os) || !Object.values(ARCH).includes(row.arch)) fail(`a sizes row names ${row.agent} ${row.os} ${row.arch}`);
    if (![row.download, row.unpacked].every((n) => Number.isSafeInteger(n) && n > 0)) fail("a size is not a positive whole number");
    const key = `${row.agent}/${row.os}/${row.arch}`;
    if (seen.has(key)) fail(`sizes repeats ${key}`);
    seen.add(key);
  }
  const pj = list.package_json;
  if (!isObject(pj) || Object.keys(pj).some((k) => !PACKAGE_JSON_FIELDS.includes(k))) fail("package_json has a field an engines pin file does not");
  const deps = pj.dependencies;
  if (!isObject(deps) || Object.keys(deps).length !== DEPENDENCIES.length || !DEPENDENCIES.every((n) => Object.hasOwn(deps, n))) {
    fail(`package_json.dependencies is not exactly ${DEPENDENCIES.join(" and ")}`);
  }
  const overrides = pj.overrides ?? {};
  if (!isObject(overrides) || Object.entries(overrides).some(([k, v]) => !OVERRIDES.includes(k) || typeof v !== "string")) fail("package_json.overrides names more than @openai/codex");
  const pins = { ...deps, ...overrides };
  for (const [name, v] of Object.entries(pins)) if (typeof v !== "string" || !SEMVER.test(v)) fail(`${name} is not one exact version: ${JSON.stringify(v)}`);
  const lock = list.package_lock;
  if (!isObject(lock) || lock.lockfileVersion !== 3 || !isObject(lock.packages)) fail("package_lock is not a v3 lock");
  if (!isObject(lock.packages[""]) || JSON.stringify(canonical(lock.packages[""].dependencies ?? null)) !== JSON.stringify(canonical(deps))) {
    fail("the lock's root dependencies are not package_json's");
  }
  for (const [path, entry] of Object.entries(lock.packages)) {
    if (path === "") continue;
    const last = installedName(path);
    if (last === null) fail(`lock key ${JSON.stringify(path)} is not node_modules/<name>`);
    if (!isObject(entry) || "link" in entry || "inBundle" in entry || typeof entry.version !== "string") fail(`${path} is a link, bundled or has no version`);
    if (typeof entry.integrity !== "string" || !/^sha512-[A-Za-z0-9+/]{86}==$/.test(entry.integrity)) fail(`${path}'s integrity is not exactly one sha512`);
    if ("name" in entry && !isName(entry.name)) fail(`${path}'s name is not an npm name`);
    const pkg = entry.name ?? last;
    if (!/^[A-Za-z0-9.+-]+$/.test(entry.version)) fail(`${path}'s version is not a version`);
    const want = `${REGISTRY}${pkg}/-/${pkg.split("/").at(-1)}-${entry.version}.tgz`;
    if (entry.resolved !== want) fail(`${path}'s resolved is not ${want}`);
  }
  for (const [name, v] of Object.entries(pins)) {
    if (lock.packages[`node_modules/${name}`]?.version !== v) fail(`the lock does not install ${name} ${v}`);
  }
  return list;
}

// ---- the plan ----

/** The lane's updater public key, Tauri's base64 form. */
export function lanePubkey(root, lane) {
  const key =
    parseLane(lane) === "engines"
      ? JSON.parse(readFileSync(join(root, "crates/desktop/tauri.conf.json"), "utf8")).plugins?.updater?.pubkey
      : readFileSync(join(root, "crates/desktop/release/updater-test.pub"), "utf8").replace(/[\r\n]/g, "");
  const text = Buffer.from(key ?? "", "base64").toString("utf8").split("\n");
  if (!text[0]?.startsWith("untrusted comment: minisign public key") || !text[1]?.startsWith("RW")) {
    throw new Error(`the ${lane} lane's updater key is not base64 of a minisign public key file`);
  }
  return key;
}

/** The list from the checkout (`apps` given), with `seq` and `generated`. */
export function buildList(root, { seq, apps, generated }) {
  const live = (name) => JSON.parse(readFileSync(join(root, LIVE, name), "utf8"));
  return {
    format: FORMAT,
    seq,
    generated,
    apps,
    package_json: live("package.json"),
    package_lock: live("package-lock.json"),
    sizes: parseSizes(readFileSync(join(root, EMBEDDED), "utf8")),
  };
}

/** The archived <lane>-<n>.json, signed under `pubkey`, again with seq `seq` and `generated`. */
export function rebuildRollback(json, sig, pubkey, { n, seq, generated }) {
  if (json === null || sig === null) throw new Error(`no archived list ${n} (or its .sig) on the release`);
  const problems = engineSignatureProblems(Buffer.from(json), sig, pubkey);
  if (problems.length) throw new Error(`the archived list ${n}'s signature: ${problems.join("; ")}`);
  const old = selfCheck(JSON.parse(json));
  if (old.seq !== n) throw new Error(`the archived list ${n} says seq ${old.seq}`);
  return { ...old, seq, generated };
}

/** The whole plan: { list, text, sha256, live, summary }. */
export async function plan(root, { env, fetchImpl = fetch, now = new Date() }) {
  const lane = parseLane(env.LANE);
  const n = parseRollback(env.ROLLBACK_TO);
  const repo = env.GITHUB_REPOSITORY;
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo ?? "")) throw new Error("GITHUB_REPOSITORY is not owner/name");
  if (lane === "engines" && env.APPS) throw new Error("apps is for the engines-test lane only");
  const pubkey = lanePubkey(root, lane);
  const live = await liveList(fetchImpl, repo, lane, pubkey);
  const liveSeq = live === null ? 0 : live.seq;
  const fileSeq = parseSeq(readFileSync(join(root, LIVE, "engines.seq"), "utf8"));
  const generated = stamp(now);
  let list;
  if (n !== null) {
    if (n >= liveSeq) throw new Error(`rollback_to ${n} is not below the live seq ${live === null ? "(none)" : liveSeq}`);
    const [json, sig] = await Promise.all(
      [`${lane}-${n}.json`, `${lane}-${n}.json.sig`].map((name) => getText(fetchImpl, assetUrl(repo, lane, name))),
    );
    list = rebuildRollback(json, sig, pubkey, { n, seq: liveSeq + 1, generated });
  } else {
    const apps =
      lane === "engines"
        ? enginesApps(JSON.parse(readFileSync(join(root, "crates/desktop/tauri.conf.json"), "utf8")).version, await latestTag(fetchImpl, repo, env.GH_TOKEN))
        : testApps(env.APPS);
    list = buildList(root, { seq: chooseSeq(fileSeq, live), apps, generated });
  }
  seqGate(list.seq, liveSeq);
  selfCheck(list);
  const text = canonical(list);
  const sha256 = createHash("sha256").update(text).digest("hex");
  return { list, text, sha256, live, fileSeq, n, summary: summary({ lane, list, text, sha256, live, fileSeq, n, sha: env.GITHUB_SHA }) };
}

/** `engines.seq N, live M (verified, sha256 …) or none, chosen K`, and why. */
export function seqChoice({ fileSeq, live, seq, n = null }) {
  const liveText = live === null ? "live none" : `live ${live.seq} (verified, sha256 ${live.sha256})`;
  const why = n !== null ? `rollback to ${n}: live + 1` : live === null ? "no live list: engines.seq" : seq === fileSeq ? "engines.seq is above live" : "live + 1";
  return `engines.seq ${fileSeq}, ${liveText}, chosen ${seq} (${why})`;
}

/** What will be signed, for the job summary. */
export function summary({ lane, list, text, sha256, live, fileSeq, n, sha }) {
  const pins = { ...list.package_json.dependencies, ...list.package_json.overrides };
  return [
    `### ${lane}: engines.json to sign${n === null ? "" : ` (rollback to ${n})`}`,
    "",
    `- seq: ${seqChoice({ fileSeq, live, seq: list.seq, n })}`,
    ...Object.entries(pins).map(([name, v]) => `- \`${name}\` ${v}`),
    `- apps: ${list.apps.join(", ")}`,
    `- commit: ${sha ?? "(local)"}`,
    `- sha256: \`${sha256}\``,
    "",
    "<details><summary>engines.json</summary>",
    "",
    "```json",
    text.trimEnd(),
    "```",
    "",
    "</details>",
    "",
  ].join("\n");
}

// ---- the command line ----

export async function main(argv, { env = process.env, fetchImpl = fetch, now = new Date(), log = console.log } = {}) {
  const at = argv.indexOf("--root");
  const root = at >= 0 ? resolve(argv[at + 1]) : join(dirname(fileURLToPath(import.meta.url)), "../..");
  const args = at >= 0 ? argv.filter((_, i) => i !== at && i !== at + 1) : argv;
  if (args[0] === "--plan" && args.length === 2) {
    const p = await plan(root, { env, fetchImpl, now });
    mkdirSync(args[1], { recursive: true });
    writeFileSync(join(args[1], "engines.json"), p.text);
    const [liveSeq, liveSha] = p.live === null ? ["none", "none"] : [p.live.seq, p.live.sha256];
    if (env.GITHUB_OUTPUT) appendFileSync(env.GITHUB_OUTPUT, `seq=${p.list.seq}\nlive_seq=${liveSeq}\nlive_sha256=${liveSha}\nsha256=${p.sha256}\n`);
    if (env.GITHUB_STEP_SUMMARY) appendFileSync(env.GITHUB_STEP_SUMMARY, p.summary);
    log(`${seqChoice({ fileSeq: p.fileSeq, live: p.live, seq: p.list.seq, n: p.n })}; sha256 ${p.sha256}`);
    return 0;
  }
  if (args[0] === "--live-check" && args.length === 1) {
    const lane = parseLane(env.LANE);
    if (!/^[1-9]\d*$/.test(env.SEQ ?? "") || !/^([0-9a-f]{64}|none)$/.test(env.PLANNED_LIVE_SHA256 ?? "")) throw new Error("SEQ or PLANNED_LIVE_SHA256 is not plan's output");
    const now = await liveList(fetchImpl, env.GITHUB_REPOSITORY, lane, lanePubkey(root, lane));
    liveUnchanged(env.PLANNED_LIVE_SHA256, now, Number(env.SEQ));
    log(`live check: the live ${lane} list is still ${env.PLANNED_LIVE_SHA256}; seq ${env.SEQ} passes`);
    return 0;
  }
  if (args[0] === "--pubkey" && args.length === 3) {
    writeFileSync(args[2], `${lanePubkey(root, args[1])}\n`);
    return 0;
  }
  if (args[0] === "--check-signature" && args.length === 3) {
    const problems = engineSignatureProblems(readFileSync(args[1]), readFileSync(`${args[1]}.sig`, "utf8"), readFileSync(args[2], "utf8"));
    for (const problem of problems) log(`${args[1]}.sig: ${problem}`);
    return problems.length ? 1 : 0;
  }
  if (args[0] === "--self-check" && args.length === 2) {
    const text = readFileSync(args[1], "utf8");
    const list = selfCheck(JSON.parse(text));
    if (canonical(list) !== text) throw new Error("self-check: the file is not canonical JSON");
    log(`self-check: ${args[1]} holds (seq ${list.seq})`);
    return 0;
  }
  log("usage: engines-publish.mjs --plan <out-dir> | --live-check | --pubkey <lane> <out-file> | --check-signature <file> <key-file> | --self-check <file>");
  return 2;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (err) => {
      console.error(`engines-publish: ${err.message ?? err}`);
      process.exit(1);
    },
  );
}
