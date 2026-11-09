// stetdoc.mjs: the stetdoc fetch, for release builds (the private task row U13). The same
// mechanism as agent-browser.mjs beside it: a pin next to this folder (`crates/desktop/
// stetdoc.toml`), read with a small exact parser, and a download checked against the pin's
// SHA-256 before anything is written.
//
// `fetchStetdoc(triple, destPath)` downloads the pinned release asset for one Rust target triple,
// checks its SHA-256, takes the one program out of the archive (`stetdoc`, or `stetdoc.exe` on
// Windows; the licence files stay behind) and writes it to `destPath`. The release script
// (.github/scripts/sidecars.mjs) calls it once per target and writes
// `crates/desktop/binaries/clauseo-stetdoc-<triple>[.exe]`. Nothing else calls it, and nothing
// fetches stetdoc at runtime.
//
// A triple the pin marks `pending` throws: the release of that triple fails, never skips stetdoc.
//
// Node built-ins only: no dependency to install. The URL is checked before the request (https,
// github.com, the pinned release's download folder, the asset named for the triple), so a wrong
// pin fails before any download. The request follows redirects; the sha256 of the bytes decides
// trust, not the host they came from. The download must answer HTTP 200 and stay within 64 MB,
// within a 120 s timeout, body included. With GH_TOKEN set (the release job's token) the asset
// comes through the GitHub API, as `gh release download` fetches it: the release by its tag,
// then the asset's API url with Accept: application/octet-stream; without it, the pinned url,
// anonymously. Each request goes through fetch.mjs, the helper agent-browser.mjs shares: tried up
// to 5 times, 30 s apart, on a network error, a 5xx or a 429 (github.com's anonymous 504s, #850);
// a 404 or other answer, an oversized body and a checksum mismatch fail at once. The archive is
// read in memory: a .tar.gz with gunzip and the ustar headers, a .zip through its central
// directory (stored or deflated entries).
//
// Tests: `stetdoc.test.mjs` next to this file, run with
// `node --test 'crates/desktop/sidecars/*.test.mjs'`.

import { createHash } from 'node:crypto';
import { chmod, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { gunzipSync, inflateRawSync } from 'node:zlib';
import { fetchGitHubAsset } from './fetch.mjs';

/** The pin, next to this folder: `crates/desktop/stetdoc.toml`. */
const DEFAULT_PIN = new URL('../stetdoc.toml', import.meta.url);

const HOST = 'github.com';
const RELEASE_PREFIX = (version) => `/clauseo-ai/stetdoc/releases/download/v${version}/`;

const KEY = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/;
const TABLE = /^\[([A-Za-z0-9_-]+)\]$/;
const STRING = /^"([^"\\]*)"$/;
const SHA256 = /^[0-9a-f]{64}$/;

const isWindows = (triple) => triple.includes('-windows-');

/** The program inside the archive, and its asset's name, for `triple`. */
export const programName = (triple) => (isWindows(triple) ? 'stetdoc.exe' : 'stetdoc');
export const assetName = (triple) => `stetdoc-${triple}${isWindows(triple) ? '.zip' : '.tar.gz'}`;

/**
 * Parses the pin. Its format is exact (see the header of stetdoc.toml): blank lines and
 * whole-line `#` comments, `version = "<semver>"` first, then one `[triple]` table per triple
 * with `url` and `sha256` strings, or a `pending` string. Anything else throws, with the line.
 *
 * @param {string} text the file's contents
 * @returns {{ version: string,
 *   triples: Map<string, { url: string, sha256: string } | { pending: string }> }}
 */
export function parsePin(text) {
  let version;
  const triples = new Map();
  let table = null;
  let fields = null;

  const finish = (lineNo) => {
    if (table === null) return;
    const keys = Object.keys(fields).sort().join(' ');
    if (keys !== 'pending' && keys !== 'sha256 url') {
      throw new Error(`stetdoc.toml: [${table}] needs url and sha256, or pending (before line ${lineNo})`);
    }
    triples.set(table, keys === 'pending' ? { pending: fields.pending } : { url: fields.url, sha256: fields.sha256 });
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
      if (triples.has(table)) throw new Error(`stetdoc.toml: [${table}] twice (line ${lineNo})`);
      fields = {};
      continue;
    }

    const key = KEY.exec(line);
    if (!key) throw new Error(`stetdoc.toml: line ${lineNo} is not a table, a key or a comment`);
    const [, name, raw] = key;
    const quoted = STRING.exec(raw);
    if (!quoted) throw new Error(`stetdoc.toml: line ${lineNo}: ${name} needs a double-quoted string`);
    const value = quoted[1];

    if (table === null) {
      if (name !== 'version') {
        throw new Error(`stetdoc.toml: line ${lineNo}: only version = "..." comes before the first table`);
      }
      if (version !== undefined) throw new Error(`stetdoc.toml: version twice (line ${lineNo})`);
      version = value;
      continue;
    }

    if (!['url', 'sha256', 'pending'].includes(name) || name in fields) {
      throw new Error(`stetdoc.toml: [${table}] line ${lineNo}: unexpected or repeated key ${name}`);
    }
    if (name === 'sha256' && !SHA256.test(value)) {
      throw new Error(`stetdoc.toml: [${table}] sha256 must be 64 lowercase hex characters`);
    }
    if (name === 'pending' && value.trim() === '') {
      throw new Error(`stetdoc.toml: [${table}] pending must say why`);
    }
    fields[name] = value;
  }
  finish(lines.length);

  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error('stetdoc.toml: version = "<major.minor.patch>" is missing or not semver');
  }
  return { version, triples };
}

/** Throws unless `url` is https on github.com, the pinned release's asset for `triple`. */
function checkUrl(url, version, triple) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`stetdoc: pinned url is not a URL: ${url}`);
  }
  const want = `${RELEASE_PREFIX(version)}${assetName(triple)}`;
  const plain =
    parsed.protocol === 'https:' &&
    parsed.hostname === HOST &&
    parsed.port === '' &&
    parsed.username === '' &&
    parsed.password === '' &&
    parsed.search === '' &&
    parsed.hash === '';
  if (!plain || parsed.pathname !== want) {
    throw new Error(`stetdoc: pinned url for ${triple} must be https://${HOST}${want}: ${url}`);
  }
}

/** A path's last component, `/` or `\` separated. */
const baseName = (path) => path.replace(/\/+$/, '').split(/[/\\]/).pop();

/**
 * The regular files of a ustar archive (`bytes`, already gunzipped), as [path, contents] pairs.
 * Directories, links and pax or GNU metadata entries are skipped.
 */
export function tarFiles(bytes) {
  const files = [];
  let at = 0;
  while (at + 512 <= bytes.length) {
    const header = bytes.subarray(at, at + 512);
    if (header.every((b) => b === 0)) break;
    const field = (start, len) => header.subarray(start, start + len).toString('latin1').replace(/\0.*$/s, '');
    const size = Number.parseInt(field(124, 12).trim() || '0', 8);
    if (!Number.isFinite(size) || size < 0) throw new Error('stetdoc: a tar header has no valid size');
    const type = field(156, 1);
    const prefix = field(257, 6).startsWith('ustar') ? field(345, 155) : '';
    const name = prefix ? `${prefix}/${field(0, 100)}` : field(0, 100);
    const start = at + 512;
    if (start + size > bytes.length) throw new Error(`stetdoc: tar entry ${name} runs past the archive`);
    if (type === '0' || type === '') files.push([name, bytes.subarray(start, start + size)]);
    at = start + Math.ceil(size / 512) * 512;
  }
  return files;
}

/** The files of a zip archive, as [path, contents] pairs; stored or deflated entries only. */
export function zipFiles(bytes) {
  let end = -1;
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 22 - 65_535); i -= 1) {
    if (bytes.readUInt32LE(i) === 0x06054b50) {
      end = i;
      break;
    }
  }
  if (end < 0) throw new Error('stetdoc: the zip has no end-of-central-directory record');
  const count = bytes.readUInt16LE(end + 10);
  let at = bytes.readUInt32LE(end + 16);
  const files = [];
  for (let n = 0; n < count; n += 1) {
    if (bytes.readUInt32LE(at) !== 0x02014b50) throw new Error('stetdoc: the zip central directory is malformed');
    const method = bytes.readUInt16LE(at + 10);
    const compressed = bytes.readUInt32LE(at + 20);
    const size = bytes.readUInt32LE(at + 24);
    const nameLen = bytes.readUInt16LE(at + 28);
    const extraLen = bytes.readUInt16LE(at + 30);
    const commentLen = bytes.readUInt16LE(at + 32);
    const local = bytes.readUInt32LE(at + 42);
    const name = bytes.subarray(at + 46, at + 46 + nameLen).toString('utf8');
    at += 46 + nameLen + extraLen + commentLen;
    if (name.endsWith('/')) continue;
    if (bytes.readUInt32LE(local) !== 0x04034b50) throw new Error(`stetdoc: zip entry ${name} has no local header`);
    const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
    const raw = bytes.subarray(start, start + compressed);
    let data;
    if (method === 0) data = raw;
    else if (method === 8) data = inflateRawSync(raw);
    else throw new Error(`stetdoc: zip entry ${name} uses compression method ${method}`);
    if (data.length !== size) throw new Error(`stetdoc: zip entry ${name} is ${data.length} bytes, its header says ${size}`);
    files.push([name, data]);
  }
  return files;
}

/** The one program named `program` in `archive` (a .tar.gz, or a .zip when `zip`); throws on none or several. */
export function extractProgram(archive, program, zip) {
  const files = zip ? zipFiles(archive) : tarFiles(gunzipSync(archive));
  const found = files.filter(([path]) => baseName(path) === program);
  if (found.length !== 1) {
    throw new Error(`stetdoc: the archive holds ${found.length} files named ${program}, not one`);
  }
  return found[0][1];
}

/**
 * Downloads the pinned stetdoc for `triple` and writes its program to `destPath`.
 *
 * Returns `destPath`. Throws when the triple is not in the pin or is `pending` there, the pinned
 * URL is not the pinned release's asset for the triple, the download fails, its SHA-256 differs
 * from the pin, or the archive does not hold exactly one program; each writes nothing. The
 * program goes to `destPath + '.partial'` and is renamed into place, mode 0755 on every triple
 * but Windows's.
 *
 * @param {string} triple a Rust target triple, e.g. `aarch64-apple-darwin`
 * @param {string} destPath where the program goes, `.exe` included on Windows
 * @param {{ fetch?: typeof fetch, pinPath?: string | URL, log?: (line: string) => void,
 *   wait?: (ms: number) => Promise<void>, env?: Record<string, string | undefined> }} [options]
 *   `fetch` replaces the network, `pinPath` the pin, `wait` the 30 s between tries and `env`
 *   the environment GH_TOKEN is read from (the tests); `log` gets the retry and verified lines
 * @returns {Promise<string>}
 */
export async function fetchStetdoc(triple, destPath, options = {}) {
  const log = options.log ?? console.log;
  const pin = parsePin(await readFile(options.pinPath ?? DEFAULT_PIN, 'utf8'));

  if (!pin.triples.has(triple)) throw new Error(`stetdoc: ${triple} is not in stetdoc.toml`);
  const entry = pin.triples.get(triple);
  if ('pending' in entry) {
    throw new Error(
      `stetdoc: ${triple} is PENDING in crates/desktop/stetdoc.toml (${entry.pending}); ` +
        'no release of it can be built until the pin holds its url and sha256',
    );
  }
  checkUrl(entry.url, pin.version, triple);

  const bytes = await fetchGitHubAsset({
    url: entry.url,
    owner: 'clauseo-ai',
    repo: 'stetdoc',
    tag: `v${pin.version}`,
    assetName: assetName(triple),
    token: (options.env ?? process.env).GH_TOKEN,
    label: `stetdoc: download for ${triple}`,
    fetch: options.fetch,
    wait: options.wait,
    log,
  });
  const digest = createHash('sha256').update(bytes).digest('hex');
  if (digest !== entry.sha256) {
    throw new Error(`stetdoc: sha256 for ${triple} is ${digest}, the pin says ${entry.sha256}; nothing written`);
  }
  log(`stetdoc ${pin.version} ${assetName(triple)}: ${bytes.length} bytes, sha256 ${digest} matches the pin`);

  const program = extractProgram(bytes, programName(triple), isWindows(triple));
  const partial = `${destPath}.partial`;
  await mkdir(dirname(destPath), { recursive: true });
  try {
    await writeFile(partial, program);
    if (!isWindows(triple)) await chmod(partial, 0o755);
    await rename(partial, destPath);
  } catch (error) {
    await rm(partial, { force: true });
    throw error;
  }
  return destPath;
}
