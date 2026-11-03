// agent-browser.mjs: the agent-browser sidecar fetch, for release builds (ADR 0029, "Bundling
// (desktop)").
//
// `fetchAgentBrowser(triple, destPath)` downloads the pinned release asset for one Rust target
// triple, checks its SHA-256 against `crates/desktop/agent-browser.toml`, and writes it to
// `destPath`. The release script (.github/scripts/sidecars.mjs, stage F) calls it once per target
// and writes `crates/desktop/binaries/agent-browser-<triple>[.exe]`. Nothing else calls it.
//
// Node built-ins only: no dependency to install. Redirects are followed; the hash, not the host
// the bytes came from, is what makes the file trusted. The URL is checked before the request
// (https, github.com, the pinned release's download prefix), so a wrong pin fails before any
// download.
//
// Tests: `agent-browser.test.mjs` next to this file, run with `node --test crates/desktop/sidecars/`.
// CI does not run them: the xtask check is frozen, so they are run by hand, and a change here
// names that run in its PR.

import { createHash } from 'node:crypto';
import { chmod, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

/** The pin, next to this folder: `crates/desktop/agent-browser.toml`. */
const DEFAULT_PIN = new URL('../agent-browser.toml', import.meta.url);

const HOST = 'github.com';
const RELEASE_PREFIX = (version) => `/vercel-labs/agent-browser/releases/download/v${version}/`;

const KEY = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/;
const TABLE = /^\[([A-Za-z0-9_-]+)\]$/;
const STRING = /^"([^"\\]*)"$/;
const SHA256 = /^[0-9a-f]{64}$/;

/**
 * Parses the pin. Its format is exact (see the header of agent-browser.toml): blank lines and
 * whole-line `#` comments, `version = "<semver>"` first, then one `[triple]` table per triple
 * with `url` and `sha256` strings, or `none = true`. Anything else throws, with the line number.
 *
 * @param {string} text the file's contents
 * @returns {{ version: string, triples: Map<string, { url: string, sha256: string } | { none: true }> }}
 */
export function parsePin(text) {
  let version;
  const triples = new Map();
  let table = null; // the current [triple] table's name, or null before the first table
  let fields = null; // its keys so far

  const finish = (lineNo) => {
    if (table === null) return;
    const ok =
      (fields.none === true && Object.keys(fields).length === 1) ||
      (typeof fields.url === 'string' &&
        typeof fields.sha256 === 'string' &&
        Object.keys(fields).length === 2);
    if (!ok) throw new Error(`agent-browser.toml: [${table}] needs url and sha256, or none = true (before line ${lineNo})`);
    triples.set(table, fields.none === true ? { none: true } : { url: fields.url, sha256: fields.sha256 });
  };

  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i += 1) {
    const lineNo = i + 1;
    const line = lines[i].replace(/\r$/, '');
    if (line.trim() === '' || line.startsWith('#')) continue;

    const header = TABLE.exec(line);
    if (header) {
      finish(lineNo);
      table = header[1];
      if (triples.has(table)) throw new Error(`agent-browser.toml: [${table}] twice (line ${lineNo})`);
      fields = {};
      continue;
    }

    const key = KEY.exec(line);
    if (!key) throw new Error(`agent-browser.toml: line ${lineNo} is not a table, a key or a comment`);
    const [, name, raw] = key;
    let value;
    if (raw === 'true') value = true;
    else {
      const quoted = STRING.exec(raw);
      if (!quoted) throw new Error(`agent-browser.toml: line ${lineNo}: ${name} needs a double-quoted string or true`);
      value = quoted[1];
    }

    if (table === null) {
      if (name !== 'version' || typeof value !== 'string') {
        throw new Error(`agent-browser.toml: line ${lineNo}: only version = "..." comes before the first table`);
      }
      if (version !== undefined) throw new Error(`agent-browser.toml: version twice (line ${lineNo})`);
      version = value;
      continue;
    }

    const allowed = value === true ? name === 'none' : name === 'url' || name === 'sha256';
    if (!allowed || name in fields) {
      throw new Error(`agent-browser.toml: [${table}] line ${lineNo}: unexpected or repeated key ${name}`);
    }
    if (name === 'sha256' && !SHA256.test(value)) {
      throw new Error(`agent-browser.toml: [${table}] sha256 must be 64 lowercase hex characters`);
    }
    fields[name] = value;
  }
  finish(lines.length);

  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error('agent-browser.toml: version = "<major.minor.patch>" is missing or not semver');
  }
  return { version, triples };
}

/** Throws unless `url` is https on github.com under the pinned release's download folder. */
function checkUrl(url, version) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`agent-browser: pinned url is not a URL: ${url}`);
  }
  const plain =
    parsed.protocol === 'https:' &&
    parsed.hostname === HOST &&
    parsed.port === '' &&
    parsed.username === '' &&
    parsed.password === '';
  const prefix = RELEASE_PREFIX(version);
  const asset = parsed.pathname.slice(prefix.length);
  if (!plain || !parsed.pathname.startsWith(prefix) || asset === '' || asset.includes('/')) {
    throw new Error(`agent-browser: pinned url must be https://${HOST}${prefix}<asset>: ${url}`);
  }
}

/**
 * Downloads the pinned agent-browser for `triple` to `destPath`.
 *
 * Returns `destPath`, or `null` when the pin says the triple has no build (`none = true`), in
 * which case nothing is written. Throws when the triple is not in the pin, the pinned URL is
 * not on the pinned release, the download fails, or its SHA-256 differs from the pin; a
 * mismatch writes nothing. The bytes go to `destPath + '.partial'` and are renamed into place,
 * mode 0755 on every triple but Windows's.
 *
 * @param {string} triple a Rust target triple, e.g. `x86_64-unknown-linux-gnu`
 * @param {string} destPath where the binary goes, `.exe` included on Windows
 * @param {{ fetch?: typeof fetch, pinPath?: string | URL }} [options] `fetch` replaces the network
 *   (the tests pass a stub); `pinPath` replaces the pin file (the tests pass fixtures).
 * @returns {Promise<string | null>}
 */
export async function fetchAgentBrowser(triple, destPath, options = {}) {
  const doFetch = options.fetch ?? globalThis.fetch;
  const pinText = await readFile(options.pinPath ?? DEFAULT_PIN, 'utf8');
  const pin = parsePin(pinText);

  if (!pin.triples.has(triple)) {
    throw new Error(`agent-browser: ${triple} is not in agent-browser.toml`);
  }
  const entry = pin.triples.get(triple);
  if (entry.none === true) return null;

  checkUrl(entry.url, pin.version);

  const response = await doFetch(entry.url, { redirect: 'follow' });
  if (!response.ok) {
    throw new Error(`agent-browser: download for ${triple} failed: HTTP ${response.status}`);
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  const digest = createHash('sha256').update(bytes).digest('hex');
  if (digest !== entry.sha256) {
    throw new Error(
      `agent-browser: sha256 for ${triple} is ${digest}, the pin says ${entry.sha256}; nothing written`,
    );
  }

  const partial = `${destPath}.partial`;
  const windows = triple.includes('-windows-');
  await mkdir(dirname(destPath), { recursive: true });
  try {
    await writeFile(partial, bytes);
    // chmod, not the writeFile mode: the umask would otherwise trim it.
    if (!windows) await chmod(partial, 0o755);
    await rename(partial, destPath);
  } catch (error) {
    await rm(partial, { force: true });
    throw error;
  }
  return destPath;
}
