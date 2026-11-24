// engine-watch's building blocks: npm's semver rules as far as the pins need them, G16's lock read
// as crates/agents/src/pins.rs reads it, the snapshot's SIZES (measure, parse, rewrite), the
// registry fetches and engines.seq. engine-watch.mjs re-exports all of it; engine-watch.test.mjs
// tests it.

import { createHash } from "node:crypto";

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

export const OS_NPM = { Darwin: "darwin", Win32: "win32", Linux: "linux" };
export const ARCH_NPM = { X64: "x64", Arm64: "arm64" };

// ---- semver, the little the pins need ----

/** `1.2.3-pre.1` as numbers and prerelease parts, or null. */
export function parseVersion(text) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(text.trim());
  if (!m) return null;
  return { nums: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split(".") : [] };
}

/**
 * `text` rebuilt from its parsed parts (`v1.2.3\n` and ` 1.2.3 ` give `1.2.3`), so nothing but
 * digits, dots and a checked pre-release reaches a branch name or an output. Throws on anything else.
 */
export function cleanVersion(text) {
  const v = typeof text === "string" ? parseVersion(text) : null;
  if (!v) throw new Error(`not a version: ${JSON.stringify(text)}`);
  return `${v.nums.join(".")}${v.pre.length ? `-${v.pre.join(".")}` : ""}`;
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

/** The waits before each try of a registry or release fetch; tests set them to 0. */
export const retry = { waits: [0, 1000, 4000] };

/** `fetch` with three tries, 1 s and 4 s apart. */
async function fetchRetry(fetchImpl, url, init) {
  let last;
  for (const wait of retry.waits) {
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

/** The registry URL of a package document (the whole packument without `version`). */
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

// ---- engines.seq ----

/** engines.seq's value; anything but one decimal line is an error. */
export function parseSeq(text) {
  if (!/^\d+\n?$/.test(text)) throw new Error(`${SEQ_FILE} is not one decimal integer: ${JSON.stringify(text)}`);
  return Number(text.trim());
}

/** The published engines.json's `seq`: 0 on a 404 (nothing published yet). */
export async function publishedSeq(fetchImpl, url = PUBLISHED_URL) {
  let res;
  for (const wait of retry.waits) {
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
  let seq;
  try {
    seq = JSON.parse(await res.text()).seq;
  } catch {
    throw new Error(`${url}: not JSON`);
  }
  if (!Number.isSafeInteger(seq) || seq < 0) throw new Error(`${url}: seq is not a whole number`);
  return seq;
}

/** The next seq: one past the larger of the file's and the published one. */
export async function nextSeq(fileText, fetchImpl = fetch, url = PUBLISHED_URL) {
  return Math.max(parseSeq(fileText), await publishedSeq(fetchImpl, url)) + 1;
}

