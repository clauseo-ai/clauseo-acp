// engine-watch's re-hash of Codex's bundled ripgrep for crates/agents/src/sac.rs: LAYOUTS pins
// each Windows layout's rg.exe by SHA-256, and a stale pin quietly turns Smart App Control's
// protection off. For each LAYOUTS row it downloads the lock entry's tarball (the
// @openai/codex-win32-* alias), checks it against the lock's sha512 integrity, gunzips it in
// memory, reads `package/<layout>/codex-path/rg.exe` with the small tar reader below and hashes it.
// Nothing is written to disk and nothing from the tarball runs. engine-watch.mjs calls it;
// engine-watch.test.mjs tests it.

import { createHash } from "node:crypto";
import { gunzipSync } from "node:zlib";

import { REGISTRY } from "./engine-watch-lib.mjs";

export const SAC = "crates/agents/src/sac.rs";
const LAYOUTS_RE = /const LAYOUTS: \[\(&str, &str\); (\d+)\] = \[\n([\s\S]*?)\n\];\n/;
const ROW_RE = /\(\s*"([^"]+)",\s*"([0-9a-f]{64})",?\s*\)/g;
const CODEX_RE = /const LAYOUTS_CODEX: &str = "([^"]+)";/;

/** LAYOUTS's rows ({ dir, sha256 }, in file order) and LAYOUTS_CODEX. */
export function parseLayouts(source) {
  const block = LAYOUTS_RE.exec(source);
  const codex = CODEX_RE.exec(source);
  if (!block || !codex) throw new Error("sac.rs has no LAYOUTS block or LAYOUTS_CODEX");
  const rows = [...block[2].matchAll(ROW_RE)].map((m) => ({ dir: m[1], sha256: m[2] }));
  if (rows.length !== Number(block[1])) throw new Error(`LAYOUTS says ${block[1]} rows, ${rows.length} read`);
  return { rows, codex: codex[1] };
}

/**
 * `source` with each row's hash and LAYOUTS_CODEX replaced, and in LAYOUTS's doc comment the
 * version Codex ships and the tarball names. Only hashes change inside the block, so its layout
 * stays.
 */
export function writeLayouts(source, rows, codex) {
  const { rows: old } = parseLayouts(source);
  if (old.length !== rows.length || old.some((row, i) => row.dir !== rows[i].dir)) {
    throw new Error("LAYOUTS's folders changed; rewrite sac.rs by hand");
  }
  let out = source.replace(LAYOUTS_RE, (block) => {
    let i = 0;
    return block.replace(ROW_RE, (row) => row.replace(/"[0-9a-f]{64}"/, `"${rows[i++].sha256}"`));
  });
  out = out.replace(CODEX_RE, `const LAYOUTS_CODEX: &str = "${codex}";`);
  const start = out.indexOf("/// Codex's Windows layouts");
  const end = out.indexOf("const LAYOUTS:", start);
  if (start !== -1 && end !== -1) {
    // Only the version Codex ships and the tarball names follow the pin; other notes stay.
    const doc = out
      .slice(start, end)
      .replace(/(`@openai\/codex` )\d+\.\d+\.\d+( ships)/, `$1${codex}$2`)
      .replace(/codex-\d+\.\d+\.\d+-win32-/g, `codex-${codex}-win32-`);
    out = out.slice(0, start) + doc + out.slice(end);
  }
  return out;
}

/** The text of a NUL-padded tar header field. */
function field(header, at, len) {
  const bytes = header.subarray(at, at + len);
  const nul = bytes.indexOf(0);
  return bytes.subarray(0, nul === -1 ? len : nul).toString("utf8");
}

/**
 * The regular file at `path` in the tar `bytes`, or null. Reads ustar (with its prefix field),
 * pax `path` records and GNU long names; nothing else is interpreted.
 */
export function tarFile(bytes, path) {
  let longName = null;
  for (let at = 0; at + 512 <= bytes.length; ) {
    const header = bytes.subarray(at, at + 512);
    if (header.every((b) => b === 0)) return null;
    let size;
    if (header[124] & 0x80) {
      size = Number(header.readBigUInt64BE(128));
    } else {
      const sizeField = header.subarray(124, 136).toString("latin1").replace(/^[\0 ]+|[\0 ]+$/g, "");
      if (!/^[0-7]+$/.test(sizeField)) throw new Error(`tar: a header's size is not octal: ${JSON.stringify(sizeField)}`);
      size = Number.parseInt(sizeField, 8);
    }
    if (!Number.isSafeInteger(size) || at + 512 + size > bytes.length) throw new Error("tar: a header's size runs past the archive");
    const type = String.fromCharCode(header[156] || 48);
    const body = bytes.subarray(at + 512, at + 512 + size);
    at += 512 + Math.ceil(size / 512) * 512;
    if (type === "x") {
      const m = /(?:^|\n)\d+ path=([^\n]*)\n/.exec(body.toString("utf8"));
      longName = m ? m[1] : longName;
      continue;
    }
    if (type === "L") {
      longName = field(body, 0, body.length);
      continue;
    }
    if (type === "g") continue;
    const prefix = field(header, 345, 155);
    const name = longName ?? (prefix ? `${prefix}/${field(header, 0, 100)}` : field(header, 0, 100));
    longName = null;
    if ((type === "0" || type === "7") && name === path) return body;
  }
  return null;
}

/** A tarball download that refuses a URL outside the registry and any answer but 2xx. */
export function registryDownload(fetchImpl) {
  return async (url) => {
    if (typeof url !== "string" || !url.startsWith(REGISTRY)) throw new Error(`${url}: not under ${REGISTRY}`);
    const res = await fetchImpl(url);
    if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
    return Buffer.from(await res.arrayBuffer());
  };
}

/** `bytes`' sha512 checked against an npm `integrity` (`sha512-<base64>`); throws on a mismatch. */
export function checkIntegrity(bytes, integrity, what) {
  const want = /^sha512-([A-Za-z0-9+/=]+)$/.exec(integrity ?? "")?.[1];
  if (!want) throw new Error(`${what}: the lock's integrity is not one sha512`);
  const got = createHash("sha512").update(bytes).digest("base64");
  if (got !== want) throw new Error(`${what}: the tarball does not match the lock's integrity`);
}

/**
 * Each LAYOUTS row's rg.exe SHA-256 for `lock` (text): the row's lock entry is the longest entry
 * path its folder starts with. `download(url)` returns the tarball's bytes.
 */
export async function hashLayouts(lockText, rows, download) {
  const entries = JSON.parse(lockText).packages;
  return Promise.all(
    rows.map(async ({ dir }) => {
      const path = Object.keys(entries)
        .filter((p) => p && dir.startsWith(`${p}/`))
        .sort((a, b) => b.length - a.length)[0];
      if (!path) throw new Error(`no lock entry holds ${dir}`);
      const entry = entries[path];
      const tgz = await download(entry.resolved);
      checkIntegrity(tgz, entry.integrity, path);
      const inner = `package/${dir.slice(path.length + 1)}/codex-path/rg.exe`;
      // A cap on the unpacked tar: about twice codex's largest Windows package today (0.172.0's
      // win32-x64 unpacks to 523 MiB, so 512 MiB would already refuse it).
      const rg = tarFile(gunzipSync(tgz, { maxOutputLength: 1 << 30 }), inner);
      if (!rg) throw new Error(`${entry.resolved} has no ${inner}`);
      return { dir, sha256: createHash("sha256").update(rg).digest("hex") };
    }),
  );
}
