// agent-browser.mjs: the agent-browser sidecar fetch, for release builds (ADR 0029, "Bundling
// (desktop)").
//
// `fetchAgentBrowser(triple, destPath)` downloads the pinned release asset for one Rust target
// triple, checks its SHA-256 against `crates/desktop/agent-browser.toml`, and writes it to
// `destPath`. The release script (.github/scripts/sidecars.mjs, stage F) calls it once per target
// and writes `crates/desktop/binaries/clauseo-agent-browser-<triple>[.exe]`. Nothing else calls it.
//
// Every triple takes our build today: upstream's source plus our patches, from this repository's
// release named by the pin's `patched_tag` (crates/desktop/agent-browser/README.md). A table may
// instead name upstream's own release (vercel-labs/agent-browser, tag `v<version>`), for when
// upstream takes a patch and that triple can go back to upstream's binary.
//
// Node built-ins only: no dependency to install. The URL is checked before the request (https,
// github.com, the download prefix of our release or upstream's, one plain asset name with no `%`
// in it, and exactly agent-browser-<triple>[.exe] for ours), so a wrong pin fails before any
// download. So does an all-zero sha256, the placeholder a table holds until our build workflow
// (.github/workflows/agent-browser-build.yml) has built and published it. The request follows
// redirects; the sha256 of the bytes decides trust, not the host they came from. The download
// must answer HTTP 200 and stay within 64 MB (a Content-Length over that, or a body that grows
// past it, is refused). A 120 s timeout covers the whole download, body included.
//
// Retries and the token: the download goes through fetch.mjs, which stetdoc.mjs shares. Each
// request is tried up to 5 times, 30 s apart, on a network error, a 5xx or a 429; a 404 or other
// answer and an oversized body fail at once. With GH_TOKEN set (the release job's token) the
// asset comes through the GitHub API, as `gh release download` fetches it: the release by its
// tag (`v<version>` upstream, `patched_tag` ours), then the asset's API url; without it, the
// pinned url, anonymously.
//
// Tests: `agent-browser.test.mjs` next to this file, run with
// `node --test 'crates/desktop/sidecars/*.test.mjs'`. CI does not run them: the xtask check is
// frozen, so they are run by hand, and a change here names that run in its PR.

import { createHash } from 'node:crypto';
import { chmod, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { fetchGitHubAsset } from './fetch.mjs';

/** The pin, next to this folder: `crates/desktop/agent-browser.toml`. */
const DEFAULT_PIN = new URL('../agent-browser.toml', import.meta.url);

const HOST = 'github.com';

/** Where our build is published: this repository's releases. */
const OURS = { owner: 'clauseo-ai', repo: 'clauseo-acp' };
const UPSTREAM = { owner: 'vercel-labs', repo: 'agent-browser' };

/** The workflow that builds and publishes our build, named by the placeholder refusal. */
const WORKFLOW = '.github/workflows/agent-browser-build.yml';
const PLACEHOLDER = '0'.repeat(64);

const KEY = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/;
const TABLE = /^\[([A-Za-z0-9_-]+)\]$/;
const STRING = /^"([^"\\]*)"$/;
const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;

/** The keys before the first table, each exactly once, and the pin's field each one fills. */
const TOP_KEYS = {
  version: 'version',
  patched_tag: 'patchedTag',
  upstream_commit: 'upstreamCommit',
  upstream_tarball_sha256: 'upstreamTarballSha256',
};


/**
 * Parses the pin. Its format is exact (see the header of agent-browser.toml): blank lines and
 * whole-line `#` comments, then `version`, `patched_tag`, `upstream_commit` and
 * `upstream_tarball_sha256` strings, each once, then one `[triple]` table per triple with `url`
 * and `sha256` strings, or `none = true`. Anything else throws, with the line number.
 *
 * @param {string} text the file's contents
 * @returns {{ version: string, patchedTag: string, upstreamCommit: string,
 *   upstreamTarballSha256: string,
 *   triples: Map<string, { url: string, sha256: string } | { none: true }> }}
 */
export function parsePin(text) {
  const top = {};
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
      if (!Object.hasOwn(TOP_KEYS, name) || typeof value !== 'string') {
        const keys = Object.keys(TOP_KEYS).join(', ');
        throw new Error(`agent-browser.toml: line ${lineNo}: only ${keys} come before the first table, as strings`);
      }
      if (Object.hasOwn(top, name)) throw new Error(`agent-browser.toml: ${name} twice (line ${lineNo})`);
      top[name] = value;
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

  const { version } = top;
  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error('agent-browser.toml: version = "<major.minor.patch>" is missing or not semver');
  }
  const tagShape = new RegExp(`^agent-browser-v${version.replaceAll('.', '\\.')}-clauseo\\.[1-9][0-9]*$`);
  if (!tagShape.test(top.patched_tag ?? '')) {
    throw new Error(`agent-browser.toml: patched_tag must be agent-browser-v${version}-clauseo.<n>`);
  }
  if (!COMMIT.test(top.upstream_commit ?? '')) {
    throw new Error('agent-browser.toml: upstream_commit must be 40 lowercase hex characters');
  }
  if (!SHA256.test(top.upstream_tarball_sha256 ?? '')) {
    throw new Error('agent-browser.toml: upstream_tarball_sha256 must be 64 lowercase hex characters');
  }
  const pin = { triples };
  for (const [key, field] of Object.entries(TOP_KEYS)) pin[field] = top[key];
  return pin;
}

/** The asset name our workflow uploads for `triple`: agent-browser-<triple>, .exe on Windows. */
const ourAsset = (triple) => `agent-browser-${triple}${triple.includes('-windows-') ? '.exe' : ''}`;

/** The two releases a pinned url may name, for `triple`: ours under patched_tag, or upstream's. */
function releasesFor(triple, pin) {
  return [
    { ...OURS, tag: pin.patchedTag, asset: ourAsset(triple) },
    { ...UPSTREAM, tag: `v${pin.version}`, asset: null },
  ];
}

/** The url a table holds for our build of `triple` (agent-browser-build.mjs prints it). */
export function ourAssetUrl(triple, pin) {
  const { owner, repo, tag, asset } = releasesFor(triple, pin)[0];
  return `https://${HOST}/${owner}/${repo}/releases/download/${tag}/${asset}`;
}

/**
 * Throws unless `url` is https on github.com under the download folder of one of `triple`'s
 * two releases (releasesFor): ours, with exactly our asset name for the triple, or upstream's,
 * with one plain asset name. Returns that release, its `asset` filled in.
 */
function checkUrl(url, triple, pin) {
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
    parsed.password === '' &&
    parsed.search === '';
  const folder = (release) => `/${release.owner}/${release.repo}/releases/download/${release.tag}/`;
  for (const release of releasesFor(triple, pin)) {
    const prefix = folder(release);
    if (!plain || !parsed.pathname.startsWith(prefix)) continue;
    const asset = parsed.pathname.slice(prefix.length);
    // No `/` (a sub-folder), and no `%` (an encoded slash or dot, which the URL parser keeps).
    const plainAsset = asset !== '' && !asset.includes('/') && !asset.includes('%');
    if (plainAsset && (release.asset ?? asset) === asset) return { ...release, asset };
  }
  const [ours, upstream] = releasesFor(triple, pin);
  throw new Error(
    `agent-browser: pinned url for ${triple} must be https://${HOST}${folder(ours)}${ours.asset} ` +
      `or https://${HOST}${folder(upstream)}<asset>: ${url}`,
  );
}

/**
 * Downloads the pinned agent-browser for `triple` to `destPath`.
 *
 * Returns `destPath`, or `null` when the pin says the triple has no build (`none = true`), in
 * which case nothing is written. Throws when the triple is not in the pin, the pinned URL is
 * not on the triple's pinned release, its sha256 is the all-zero placeholder (both before any
 * request), the download fails, or its SHA-256 differs from the pin; a
 * mismatch writes nothing. The bytes go to `destPath + '.partial'` and are renamed into place,
 * mode 0755 on every triple but Windows's.
 *
 * The download is fetch.mjs's: a network error, a 5xx or a 429 is tried again, up to 5 times,
 * 30 s apart. With `env.GH_TOKEN` set, the asset comes through the GitHub API (the release by
 * its tag, upstream's or ours, then the asset's API url); without it, the pinned url is fetched
 * anonymously.
 *
 * @param {string} triple a Rust target triple, e.g. `x86_64-unknown-linux-gnu`
 * @param {string} destPath where the binary goes, `.exe` included on Windows
 * @param {{ fetch?: typeof fetch, pinPath?: string | URL, wait?: (ms: number) => Promise<void>,
 *   log?: (line: string) => void, env?: Record<string, string | undefined> }} [options] `fetch`
 *   replaces the network (the tests pass a stub); `pinPath` replaces the pin file (the tests
 *   pass fixtures); `wait` replaces the 30 s between tries and `log` receives the retry lines
 *   (both for the tests); `env` is the environment GH_TOKEN is read from (default: process.env).
 * @returns {Promise<string | null>}
 */
export async function fetchAgentBrowser(triple, destPath, options = {}) {
  const log = options.log ?? console.log;
  const env = options.env ?? process.env;
  const pinText = await readFile(options.pinPath ?? DEFAULT_PIN, 'utf8');
  const pin = parsePin(pinText);

  if (!pin.triples.has(triple)) {
    throw new Error(`agent-browser: ${triple} is not in agent-browser.toml`);
  }
  const entry = pin.triples.get(triple);
  if (entry.none === true) return null;

  const release = checkUrl(entry.url, triple, pin);
  if (entry.sha256 === PLACEHOLDER) {
    throw new Error(
      `agent-browser: the pin's sha256 for ${triple} is the all-zero placeholder; run ${WORKFLOW} ` +
        'with publish, then paste the url and sha256 lines it prints into agent-browser.toml',
    );
  }

  const bytes = await fetchGitHubAsset({
    url: entry.url,
    owner: release.owner,
    repo: release.repo,
    tag: release.tag,
    assetName: release.asset,
    token: env.GH_TOKEN,
    label: `agent-browser: download for ${triple}`,
    fetch: options.fetch,
    wait: options.wait,
    log,
  });
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
