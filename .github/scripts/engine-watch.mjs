// Engine watch: bumps G16's pins (crates/limits/tests/live/package.json and its lock) when npm
// publishes a newer Claude or Codex adapter, and re-measures the snapshot's SIZES for the new lock.
// engine-watch.yml runs it every 6 hours; engine-watch.test.mjs tests it. Plain Node 24, no install.
//
//   node .github/scripts/engine-watch.mjs [--root <dir>] [--summary <file>]
//   node .github/scripts/engine-watch.mjs --check-sizes [--root <dir>]
//
// <dir> is the repository root (default: this checkout); only the pins pair and
// crates/agents/src/snapshot/embedded.rs below it are read or written, so a scratch copy of those
// three files with the same layout works too.
//
// The bump: npm's `latest` of claude-agent-acp, codex-acp and @openai/codex, each taken only when
// newer than the pin. The codex override goes to latest @openai/codex unless the codex-acp being
// pinned asks for a range that excludes it; then the override is the newest release in codex-acp's
// own range, and the summary says so. package.json is rewritten, the lock regenerated with
// `npm install --package-lock-only --ignore-scripts --no-audit --no-fund` in its folder, SIZES and
// SIZES_STAMP rewritten, and a markdown summary written to <file>. With nothing newer it prints
// `up to date` and changes nothing. Under GitHub Actions it also writes `bumped` (true or false)
// and `branch` to $GITHUB_OUTPUT.
//
// The engine lane's sequence number, crates/limits/tests/live/engines.seq (one decimal line), goes
// to max(the file's value, the `seq` of the published engines.json) + 1 with every bump. A 404 (no
// `engines` release yet) counts as 0; any other failure stops the bump.
//
// SIZES, as the existing numbers were measured: for each agent and platform, the entries of the
// agent's derived lock (pins.rs: reachable through dependencies, optionalDependencies and
// peerDependencies, npm's node_modules lookup) that npm installs there (os, cpu and, on Linux,
// libc glibc), summed: download bytes are each `resolved` tarball's size (the registry's
// Content-Range total for a one-byte range request; it sends no Content-Length on HEAD), unpacked
// bytes are each version's `dist.unpackedSize` from the registry, 0 for the few old packages
// that have none (a few kB each, as in the existing table). --check-sizes measures the
// current lock and compares with embedded.rs: 0 when every number matches, 1 when not.
//
// Exit status: 0 when up to date, bumped, or the sizes match; 1 on a mismatch or any failure.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const REGISTRY = "https://registry.npmjs.org/";
export const CLAUDE_ACP = "@agentclientprotocol/claude-agent-acp";
export const CODEX_ACP = "@agentclientprotocol/codex-acp";
export const CODEX = "@openai/codex";
export const SDK = "@anthropic-ai/claude-agent-sdk";
/// Agent id to root package, as embedded.rs's build_embedded names them.
export const AGENTS = { "claude-acp": CLAUDE_ACP, "codex-acp": CODEX_ACP };
export const LIVE = "crates/limits/tests/live";
export const EMBEDDED = "crates/agents/src/snapshot/embedded.rs";
export const SEQ_FILE = "engines.seq";
export const PUBLISHED_URL = "https://github.com/clauseo-ai/clauseo-acp/releases/download/engines/engines.json";

const OS_NPM = { Darwin: "darwin", Win32: "win32", Linux: "linux" };
const ARCH_NPM = { X64: "x64", Arm64: "arm64" };

// ---- semver, the little the pins need ----

/** `1.2.3-pre.1` as numbers and prerelease parts, or null. */
export function parseVersion(text) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(text.trim());
  if (!m) return null;
  return { nums: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split(".") : [] };
}

/** semver precedence: negative, zero or positive. */
export function compareVersions(a, b) {
  const x = typeof a === "string" ? parseVersion(a) : a;
  const y = typeof b === "string" ? parseVersion(b) : b;
  if (!x || !y) throw new Error(`not a version: ${!x ? a : b}`);
  for (let i = 0; i < 3; i++) if (x.nums[i] !== y.nums[i]) return x.nums[i] - y.nums[i];
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i];
    const q = y.pre[i];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    if (p === q) continue;
    const pNum = /^\d+$/.test(p);
    const qNum = /^\d+$/.test(q);
    if (pNum && qNum) return Number(p) - Number(q);
    if (pNum !== qNum) return pNum ? -1 : 1;
    return p < q ? -1 : 1;
  }
  return 0;
}

/** One comparator (`^1.2.3`, `>=1.0.0`, `1.x`, `*`) as [op, version] pairs. */
function comparators(token) {
  if (token === "" || token === "*" || token === "x" || token === "latest") return [];
  const m = /^(\^|~|>=|<=|>|<|=)?v?(\d+|x|\*)(?:\.(\d+|x|\*))?(?:\.(\d+|x|\*))?(-[0-9A-Za-z.-]+)?$/.exec(token);
  if (!m) throw new Error(`unsupported range part: ${token}`);
  const [, op = "", ...rest] = m;
  const parts = rest.slice(0, 3).map((p) => (p === undefined || p === "x" || p === "*" ? null : Number(p)));
  const pre = rest[3] ?? "";
  const wild = parts.indexOf(null);
  const [M, m2, p] = parts.map((n) => n ?? 0);
  const v = (a, b, c, tail = "") => `${a}.${b}.${c}${tail}`;
  if (op === "^") {
    const upper = M > 0 || wild === 1 ? v(M + 1, 0, 0) : m2 > 0 || wild === 2 ? v(0, m2 + 1, 0) : v(0, 0, p + 1);
    return [[">=", v(M, m2, p, pre)], ["<", `${upper}-0`]];
  }
  if (op === "~") {
    const upper = wild === 1 ? v(M + 1, 0, 0) : v(M, m2 + 1, 0);
    return [[">=", v(M, m2, p, pre)], ["<", `${upper}-0`]];
  }
  if (wild !== -1 && (op === "" || op === "=")) {
    if (wild === 0) return [];
    const upper = wild === 1 ? v(M + 1, 0, 0) : v(M, m2 + 1, 0);
    return [[">=", v(M, m2, 0)], ["<", `${upper}-0`]];
  }
  return [[op === "" ? "=" : op, v(M, m2, p, pre)]];
}

/** Whether release `version` is in npm range `range` (||, spaces, ^ ~ >= <= > < =, x ranges). */
export function satisfies(version, range) {
  const ver = parseVersion(version);
  if (!ver) return false;
  return range.split("||").some((alt) =>
    alt.trim().split(/\s+/).flatMap(comparators).every(([op, bound]) => {
      const c = compareVersions(ver, bound);
      return { "=": c === 0, ">=": c >= 0, "<=": c <= 0, ">": c > 0, "<": c < 0 }[op];
    }),
  );
}

/** The newest release (no prerelease) among `versions` in `range`, or null. */
export function maxSatisfying(versions, range) {
  return versions
    .filter((v) => parseVersion(v)?.pre.length === 0 && satisfies(v, range))
    .sort(compareVersions)
    .at(-1) ?? null;
}

// ---- the lock, as pins.rs reads it ----

/** npm's lookup of `name` from the entry at `from` (pins.rs `resolve`). */
export function resolveEntry(entries, from, name) {
  let base = from;
  for (;;) {
    const candidate = base === "" ? `node_modules/${name}` : `${base}/node_modules/${name}`;
    if (candidate in entries) return candidate;
    if (base === "") return null;
    const at = base.lastIndexOf("/node_modules/");
    base = at === -1 ? "" : base.slice(0, at);
  }
}

/** The paths reachable from `node_modules/<root>`, sorted (pins.rs `reachable`). */
export function reachable(entries, root) {
  const start = `node_modules/${root}`;
  if (!(start in entries)) throw new Error(`${root} is not in the lock`);
  const kept = new Set();
  const queue = [start];
  while (queue.length) {
    const path = queue.pop();
    if (kept.has(path)) continue;
    kept.add(path);
    const entry = entries[path];
    for (const [field, peer] of [["dependencies", false], ["optionalDependencies", false], ["peerDependencies", true]]) {
      for (const name of Object.keys(entry[field] ?? {}).sort()) {
        const found = resolveEntry(entries, path, name);
        if (found) queue.push(found);
        else if (!(peer && entry.peerDependenciesMeta?.[name]?.optional)) {
          throw new Error(`${name}, a dependency of ${path}, is not in the lock`);
        }
      }
    }
  }
  return [...kept].sort();
}

/** npm's list rule for one field (platform.rs `allowed`). */
export function allowed(list, own) {
  if (!list?.length) return true;
  if (list.some((e) => e === `!${own}`)) return false;
  const positives = list.filter((e) => !e.startsWith("!"));
  return positives.length === 0 || positives.includes(own);
}

/** Whether npm installs `entry` on SIZES's `os`/`arch` (Linux as glibc). */
export function installsOn(entry, os, arch) {
  return (
    allowed(entry.os, OS_NPM[os]) &&
    allowed(entry.cpu, ARCH_NPM[arch]) &&
    (os !== "Linux" || allowed(entry.libc, "glibc"))
  );
}

/** The package an entry at `path` is: its `name` (an alias's real package), else the path's. */
export function entryName(path, entry) {
  return entry.name ?? path.slice(path.lastIndexOf("node_modules/") + "node_modules/".length);
}

/** integrity.rs `short_hash`: SHA-256 of each part prefixed by its u64 LE length, 16 hex. */
export function shortHash(parts) {
  const hash = createHash("sha256");
  for (const part of parts) {
    const bytes = Buffer.from(part);
    const len = Buffer.alloc(8);
    len.writeBigUInt64LE(BigInt(bytes.length));
    hash.update(len);
    hash.update(bytes);
  }
  return hash.digest("hex").slice(0, 16);
}

// ---- SIZES in embedded.rs ----

const SIZES_RE = /const SIZES: \[\(&str, Os, Arch, u64, u64\); (\d+)\] = \[\n([\s\S]*?)\n\];\n/;
const STAMP_RE = /pub\(super\) const SIZES_STAMP: &str = "([0-9a-f]{16})";/;

/** SIZES's rows, in file order, and SIZES_STAMP. */
export function parseSizes(source) {
  const block = SIZES_RE.exec(source);
  const stamp = STAMP_RE.exec(source);
  if (!block || !stamp) throw new Error("embedded.rs has no SIZES block or SIZES_STAMP");
  const rows = [];
  const flat = block[2].replace(/\s+/g, " ");
  for (const m of flat.matchAll(/\( ?"([a-z-]+)", Os::(\w+), Arch::(\w+), ([\d_]+), ([\d_]+),? ?\)/g)) {
    rows.push({ agent: m[1], os: m[2], arch: m[3], download: Number(m[4].replaceAll("_", "")), unpacked: Number(m[5].replaceAll("_", "")) });
  }
  if (rows.length !== Number(block[1])) throw new Error(`SIZES says ${block[1]} rows, ${rows.length} read`);
  return { rows, stamp: stamp[1] };
}

/** `123_456_789`, as the source writes numbers. */
export function groupDigits(n) {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, "_");
}

/** One row as rustfmt lays it out: on one line when its items fit 60 columns, else one per line. */
export function renderRow({ agent, os, arch, download, unpacked }) {
  const items = [`"${agent}"`, `Os::${os}`, `Arch::${arch}`, groupDigits(download), groupDigits(unpacked)];
  const inline = items.join(", ");
  if (inline.length <= 60) return `    (${inline}),`;
  return `    (\n${items.map((item) => `        ${item},`).join("\n")}\n    ),`;
}

/** `source` with SIZES's rows and SIZES_STAMP replaced. */
export function writeSizes(source, rows, stamp) {
  if (!SIZES_RE.test(source) || !STAMP_RE.test(source)) throw new Error("embedded.rs has no SIZES block or SIZES_STAMP");
  return source
    .replace(SIZES_RE, () => `const SIZES: [(&str, Os, Arch, u64, u64); ${rows.length}] = [\n${rows.map(renderRow).join("\n")}\n];\n`)
    .replace(STAMP_RE, () => `pub(super) const SIZES_STAMP: &str = "${stamp}";`);
}

// ---- the registry ----

/** `fetch` with three tries, 1 s and 4 s apart. */
async function fetchRetry(fetchImpl, url, init) {
  let last;
  for (const wait of [0, 1000, 4000]) {
    if (wait) await new Promise((r) => setTimeout(r, wait));
    try {
      const res = await fetchImpl(url, init);
      if (res.ok) return res;
      last = new Error(`${url}: HTTP ${res.status}`);
      if (res.status < 500 && res.status !== 429) break;
    } catch (err) {
      last = err;
    }
  }
  throw last;
}

/** The registry URL of a package document. */
export function docUrl(name, version) {
  return `${REGISTRY}${name.replace("/", "%2f")}${version ? `/${version}` : ""}`;
}

export async function fetchJson(fetchImpl, url, accept) {
  const res = await fetchRetry(fetchImpl, url, accept ? { headers: { accept } } : undefined);
  return res.json();
}

/** A tarball's size: the total in Content-Range for bytes 0-0. */
export async function tarballSize(fetchImpl, url) {
  const res = await fetchRetry(fetchImpl, url, { headers: { range: "bytes=0-0" } });
  const range = res.headers.get("content-range");
  await res.arrayBuffer();
  const total = /\/(\d+)$/.exec(range ?? "")?.[1];
  if (res.status === 206 && total) return Number(total);
  const length = res.headers.get("content-length");
  if (res.status === 200 && length) return Number(length);
  throw new Error(`${url}: no size (status ${res.status}, content-range ${range})`);
}

/** `fn` over `items`, at most `limit` at a time. */
async function pool(items, limit, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i]);
      }
    }),
  );
  return out;
}

/** Every SIZES row measured for `lock` (text), in `order`'s agent/os/arch order. */
export async function measureSizes(lockText, order, fetchImpl = fetch) {
  const entries = JSON.parse(lockText).packages;
  const perAgent = Object.fromEntries(Object.entries(AGENTS).map(([id, root]) => [id, reachable(entries, root)]));
  const paths = [...new Set(Object.values(perAgent).flat())];
  const sizes = Object.fromEntries(
    await pool(paths, 16, async (path) => {
      const entry = entries[path];
      if (!entry.resolved?.startsWith(REGISTRY)) throw new Error(`${path}'s resolved is not under ${REGISTRY}`);
      const [download, doc] = await Promise.all([
        tarballSize(fetchImpl, entry.resolved),
        fetchJson(fetchImpl, docUrl(entryName(path, entry), entry.version)),
      ]);
      // Old packages (etag 1.8.1, once 1.4.0...) have no dist.unpackedSize; they count as 0, as
      // they did when the existing table was measured (--check-sizes reproduces it exactly).
      const unpacked = doc.dist?.unpackedSize;
      return [path, { download, unpacked: Number.isInteger(unpacked) ? unpacked : 0 }];
    }),
  );
  return order.map(({ agent, os, arch }) => {
    const kept = perAgent[agent].filter((path) => installsOn(entries[path], os, arch));
    return {
      agent,
      os,
      arch,
      download: kept.reduce((sum, path) => sum + sizes[path].download, 0),
      unpacked: kept.reduce((sum, path) => sum + sizes[path].unpacked, 0),
    };
  });
}

// ---- the bump ----

/** The pinned versions in package.json. */
export function currentPins(manifest) {
  return {
    claudeAcp: manifest.dependencies[CLAUDE_ACP],
    codexAcp: manifest.dependencies[CODEX_ACP],
    codex: manifest.overrides[CODEX],
  };
}

/**
 * The versions to pin: each `latest` taken only when newer. `codexAcpDoc` is the registry document
 * of the codex-acp being pinned; `codexVersions` lists every @openai/codex version (called only
 * when its range excludes the latest). Returns the target and notes for the summary.
 */
export async function planBump(current, latest, codexAcpDocFor, codexVersions) {
  const pick = (cur, next) => (compareVersions(next, cur) > 0 ? next : cur);
  const target = { claudeAcp: pick(current.claudeAcp, latest.claudeAcp), codexAcp: pick(current.codexAcp, latest.codexAcp) };
  const notes = [];
  const range = (await codexAcpDocFor(target.codexAcp)).dependencies?.[CODEX];
  let codex = latest.codex;
  if (range && !satisfies(codex, range)) {
    const own = maxSatisfying(await codexVersions(), range);
    if (!own) throw new Error(`no @openai/codex release satisfies codex-acp ${target.codexAcp}'s ${range}`);
    notes.push(
      `@openai/codex ${latest.codex} is npm's latest, but codex-acp ${target.codexAcp} asks for \`${range}\`: kept codex-acp's own choice, ${own}.`,
    );
    codex = own;
  }
  target.codex = pick(current.codex, codex);
  const changed = Object.keys(target).some((key) => target[key] !== current[key]);
  return { target, changed, notes };
}

/** engines.seq's value; anything but one decimal line is an error. */
export function parseSeq(text) {
  if (!/^\d+\n?$/.test(text)) throw new Error(`${SEQ_FILE} is not one decimal integer: ${JSON.stringify(text)}`);
  return Number(text.trim());
}

/** The published engines.json's `seq`: 0 on a 404 (nothing published yet). */
export async function publishedSeq(fetchImpl, url = PUBLISHED_URL) {
  let res;
  for (const wait of [0, 1000, 4000]) {
    if (wait) await new Promise((r) => setTimeout(r, wait));
    try {
      res = await fetchImpl(url, { redirect: "follow" });
    } catch (err) {
      res = err;
      continue;
    }
    if (res.status === 404) return 0;
    if (res.ok || (res.status < 500 && res.status !== 429)) break;
  }
  if (res instanceof Error) throw res;
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  const seq = (await res.json()).seq;
  if (!Number.isSafeInteger(seq) || seq < 0) throw new Error(`${url}: seq is not a whole number`);
  return seq;
}

/** The next seq: one past the larger of the file's and the published one. */
export async function nextSeq(fileText, fetchImpl = fetch, url = PUBLISHED_URL) {
  return Math.max(parseSeq(fileText), await publishedSeq(fetchImpl, url)) + 1;
}

export function branchName(t) {
  return `engines/claude-acp-${t.claudeAcp}_codex-acp-${t.codexAcp}_codex-${t.codex}`;
}

/** package.json with the target versions, written as npm writes it. */
export function bumpManifest(manifest, target) {
  const next = structuredClone(manifest);
  next.dependencies[CLAUDE_ACP] = target.claudeAcp;
  next.dependencies[CODEX_ACP] = target.codexAcp;
  next.overrides[CODEX] = target.codex;
  return `${JSON.stringify(next, null, 2)}\n`;
}

/** The markdown summary: the PR body. */
export function summary({ current, target, sdk, claudeCode, notes, rows, stamp, oldRows, seq }) {
  const mb = (n) => `${(n / 1e6).toFixed(1)} MB`;
  const line = (name, a, b) => `| \`${name}\` | ${a} | ${b}${a === b ? " (unchanged)" : ""} |`;
  const lines = [
    "Engine bump from engine-watch: npm published newer adapters than G16's pins",
    "(`crates/limits/tests/live/package.json` and its lock).",
    "",
    "| Package | Pinned | New |",
    "|---|---|---|",
    line(CLAUDE_ACP, current.claudeAcp, target.claudeAcp),
    line(SDK, sdk.old, sdk.new),
    line(CODEX_ACP, current.codexAcp, target.codexAcp),
    line(CODEX, current.codex, target.codex).replace("` |", "` (override) |"),
    "",
    `Claude Code inside the SDK (its \`claudeCodeVersion\`): ${claudeCode.old ?? "?"} → ${claudeCode.new ?? "?"}.`,
    "Model ids: not listed in the packages' registry metadata, so not compared here.",
    `Engine seq (\`${LIVE}/${SEQ_FILE}\`): ${seq.old} → ${seq.new}.`,
  ];
  if (notes.length) lines.push("", ...notes.map((n) => `- ${n}`));
  lines.push("", `SIZES re-measured from the registry (SIZES_STAMP ${stamp}):`, "", "| Agent | Platform | Download | Unpacked |", "|---|---|---|---|");
  rows.forEach((r, i) => {
    const old = oldRows[i];
    lines.push(`| ${r.agent} | ${OS_NPM[r.os]}-${ARCH_NPM[r.arch]} | ${mb(old.download)} → ${mb(r.download)} | ${mb(old.unpacked)} → ${mb(r.unpacked)} |`);
  });
  lines.push(
    "",
    "Before merging: the suite and a real-agent smoke on the CI thread's lanes (nothing runs on GitHub).",
    "A codex-acp bump also needs `crates/agents/src/install/codex_patch.rs` (VERSION, PRISTINE,",
    "PATCHED) redone for the new `dist/index.js`, and the lock's entry counts in",
    "`crates/agents/src/snapshot/tests.rs` checked.",
  );
  return `${lines.join("\n")}\n`;
}

function versionIn(lockText, name) {
  return JSON.parse(lockText).packages[`node_modules/${name}`]?.version ?? null;
}

function setOutput(pairs) {
  const file = process.env.GITHUB_OUTPUT;
  if (!file) return;
  appendFileSync(file, Object.entries(pairs).map(([k, v]) => `${k}=${v}\n`).join(""));
}

async function checkSizes(root) {
  const embeddedPath = join(root, EMBEDDED);
  const lockText = readFileSync(join(root, LIVE, "package-lock.json"), "utf8");
  const { rows, stamp } = parseSizes(readFileSync(embeddedPath, "utf8"));
  const measured = await measureSizes(lockText, rows);
  let ok = shortHash([lockText]) === stamp;
  console.log(`SIZES_STAMP ${stamp}, lock hash ${shortHash([lockText])}${ok ? "" : " (differs)"}`);
  measured.forEach((m, i) => {
    const r = rows[i];
    const same = m.download === r.download && m.unpacked === r.unpacked;
    ok &&= same;
    console.log(`${same ? "match" : "DIFF "} ${m.agent} ${m.os} ${m.arch}: ${m.download} ${m.unpacked}${same ? "" : ` (embedded ${r.download} ${r.unpacked})`}`);
  });
  console.log(ok ? "SIZES match the lock" : "SIZES do not match the lock");
  return ok ? 0 : 1;
}

async function bump(root, summaryPath) {
  const dir = join(root, LIVE);
  const manifestPath = join(dir, "package.json");
  const lockPath = join(dir, "package-lock.json");
  const embeddedPath = join(root, EMBEDDED);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const oldLock = readFileSync(lockPath, "utf8");
  const current = currentPins(manifest);
  const [claudeAcp, codexAcp, codex] = await Promise.all(
    [CLAUDE_ACP, CODEX_ACP, CODEX].map(async (name) => (await fetchJson(fetch, docUrl(name, "latest"))).version),
  );
  const latest = { claudeAcp, codexAcp, codex };
  const { target, changed, notes } = await planBump(
    current,
    latest,
    (v) => fetchJson(fetch, docUrl(CODEX_ACP, v)),
    async () => Object.keys((await fetchJson(fetch, docUrl(CODEX), "application/vnd.npm.install-v1+json")).versions),
  );
  if (!changed) {
    console.log("up to date");
    notes.forEach((n) => console.log(n));
    setOutput({ bumped: "false" });
    return 0;
  }
  const seqPath = join(dir, SEQ_FILE);
  const seqText = readFileSync(seqPath, "utf8");
  const seq = { old: parseSeq(seqText), new: await nextSeq(seqText) };
  writeFileSync(manifestPath, bumpManifest(manifest, target));
  writeFileSync(seqPath, `${seq.new}\n`);
  const npm = spawnSync("npm", ["install", "--package-lock-only", "--ignore-scripts", "--no-audit", "--no-fund"], {
    cwd: dir,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  if (npm.status !== 0) throw new Error(`npm install --package-lock-only failed (${npm.status ?? npm.signal})`);
  const lockText = readFileSync(lockPath, "utf8");
  const source = readFileSync(embeddedPath, "utf8");
  const { rows: oldRows } = parseSizes(source);
  const rows = await measureSizes(lockText, oldRows);
  const stamp = shortHash([lockText]);
  writeFileSync(embeddedPath, writeSizes(source, rows, stamp));
  const sdk = { old: versionIn(oldLock, SDK), new: versionIn(lockText, SDK) };
  const codeVersion = async (v) => (v ? (await fetchJson(fetch, docUrl(SDK, v))).claudeCodeVersion : null);
  const claudeCode = { old: await codeVersion(sdk.old), new: await codeVersion(sdk.new) };
  const text = summary({ current, target, sdk, claudeCode, notes, rows, stamp, oldRows, seq });
  if (summaryPath) writeFileSync(summaryPath, text);
  const branch = branchName(target);
  console.log(text);
  console.log(`bump ${branch}`);
  setOutput({ bumped: "true", branch });
  return 0;
}

async function main(argv) {
  const arg = (flag) => {
    const i = argv.indexOf(flag);
    return i === -1 ? undefined : argv[i + 1];
  };
  const root = resolve(arg("--root") ?? join(dirname(fileURLToPath(import.meta.url)), "../.."));
  return argv.includes("--check-sizes") ? checkSizes(root) : bump(root, arg("--summary"));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (err) => {
      console.error(`engine-watch: ${err.message ?? err}`);
      process.exit(1);
    },
  );
}
