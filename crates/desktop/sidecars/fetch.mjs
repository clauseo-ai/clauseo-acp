// fetch.mjs: the download the release sidecars share (stetdoc.mjs and agent-browser.mjs beside
// it), Node built-ins only.
//
// `getBytes(url, options)` returns the body of one GET. The response must answer HTTP 200 and stay
// within 64 MB (a Content-Length over that, or a body that grows past it, is refused), and a
// 120 s timeout covers the whole download, body included. Each request is tried up to 5 times,
// 30 s apart, on a network error, a 5xx or a 429 (github.com's anonymous 504s, #850); a 404 or
// any other answer, and an oversized body, fail at once.
//
// `fetchGitHubAsset(options)` gets a release asset by name. With a token (the release job's
// GH_TOKEN) it goes through the GitHub API, as `gh release download` fetches it: the release by
// its tag, then the asset's API url with Accept: application/octet-stream. The asset url the
// release lists must be https://api.github.com/repos/<owner>/<repo>/releases/assets/<digits>.
// Without a token it is one getBytes of the pinned url, anonymously, with no Authorization.
//
// The callers check their own pinned urls and checksums; this file only moves the bytes.
//
// Tests: `fetch.test.mjs` next to this file, run with
// `node --test 'crates/desktop/sidecars/*.test.mjs'`.

/** How many times a request is tried, the wait between tries, the timeout and the size cap. */
const ATTEMPTS = 5;
const RETRY_MS = 30_000;
const TIMEOUT_MS = 120_000;
/** The largest download we accept: the pinned archives and binaries are at most about 18 MB. */
const MAX_BYTES = 64 * 1024 * 1024;

/** A failure worth another try: a network error, a 5xx or a 429. */
class Transient extends Error {}

/**
 * The response body, at most `maxBytes`: refused up front when Content-Length says more, and
 * while reading when the body grows past it. `label` leads the messages.
 */
async function readCapped(response, label, maxBytes) {
  const tooBig = `${label} is over ${maxBytes} bytes; nothing written`;
  const declared = response.headers?.get('content-length');
  if (declared !== null && declared !== undefined && Number(declared) > maxBytes) {
    await response.body?.cancel(); // drop the connection; nothing is read
    throw new Error(tooBig);
  }
  if (!response.body) {
    const bytes = Buffer.from(await response.arrayBuffer());
    if (bytes.length > maxBytes) throw new Error(tooBig);
    return bytes;
  }
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    let done;
    let value;
    try {
      ({ done, value } = await reader.read());
    } catch (error) {
      throw new Transient(`${label} broke off: ${error.message}`);
    }
    if (done) break;
    total += value.byteLength;
    if (total > maxBytes) {
      await reader.cancel();
      throw new Error(tooBig);
    }
    chunks.push(value);
  }
  return Buffer.concat(chunks, total);
}

/**
 * The body of one GET of `url`, tried again on a Transient failure (see the header).
 *
 * @param {string} url
 * @param {{ headers?: Record<string, string>, label?: string, fetch?: typeof fetch,
 *   wait?: (ms: number) => Promise<void>, log?: (line: string) => void, maxBytes?: number,
 *   timeoutMs?: number, attempts?: number, retryMs?: number }} [options] `label` leads every
 *   message, e.g. `stetdoc: download for <triple>` (default: the url); `fetch` replaces the
 *   network, `wait` the wait between tries and `log` gets each retry line (the tests pass stubs)
 * @returns {Promise<Buffer>} the body, at most `maxBytes`
 * @throws when the download fails: at once, or after `attempts` tries
 */
export async function getBytes(url, options = {}) {
  const {
    headers = {},
    label = url,
    fetch: doFetch = globalThis.fetch,
    wait = (ms) => new Promise((done) => setTimeout(done, ms)),
    log = console.log,
    maxBytes = MAX_BYTES,
    timeoutMs = TIMEOUT_MS,
    attempts = ATTEMPTS,
    retryMs = RETRY_MS,
  } = options;

  for (let attempt = 1; ; attempt += 1) {
    try {
      let response;
      try {
        response = await doFetch(url, {
          headers,
          redirect: 'follow',
          signal: AbortSignal.timeout(timeoutMs),
        });
      } catch (error) {
        throw new Transient(`${label} failed: ${error.message}`);
      }
      if (response.status !== 200) {
        const message = `${label} answered HTTP ${response.status}, not 200`;
        await response.body?.cancel();
        throw response.status === 429 || response.status >= 500
          ? new Transient(message)
          : new Error(message);
      }
      return await readCapped(response, label, maxBytes);
    } catch (error) {
      if (!(error instanceof Transient) || attempt >= attempts) throw error;
      log(`${error.message} (try ${attempt} of ${attempts}); trying again in ${retryMs / 1000} s`);
      await wait(retryMs);
    }
  }
}

/** Whether `url` is the API url of one asset of `owner`/`repo`. */
function isAssetApiUrl(url, owner, repo) {
  const prefix = `https://api.github.com/repos/${owner}/${repo}/releases/assets/`;
  if (typeof url !== 'string' || !url.startsWith(prefix)) return false;
  return /^\d+$/.test(url.slice(prefix.length));
}

/**
 * The bytes of the release asset `assetName` of `owner`/`repo` at `tag`.
 *
 * With `token`, the release is read through the API and the asset comes from its API url (the
 * `url` is not used then); without one, `url` is fetched anonymously. Every request goes through
 * getBytes, so it is tried again on the same failures.
 *
 * @param {{ url: string, owner: string, repo: string, tag: string, assetName: string,
 *   token?: string } & Parameters<typeof getBytes>[1]} options the getBytes options follow
 *   `token`; `label` leads the messages, as in getBytes
 * @returns {Promise<Buffer>}
 */
export async function fetchGitHubAsset({ url, owner, repo, tag, assetName, token, ...options }) {
  if (!token) return getBytes(url, options);

  const label = options.label ?? url;
  const auth = { Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28' };
  const release = JSON.parse(
    (
      await getBytes(`https://api.github.com/repos/${owner}/${repo}/releases/tags/${tag}`, {
        ...options,
        headers: { ...auth, Accept: 'application/vnd.github+json' },
      })
    ).toString('utf8'),
  );
  const asset = (release.assets ?? []).find((candidate) => candidate.name === assetName);
  if (!asset || !isAssetApiUrl(asset.url, owner, repo)) {
    throw new Error(`${label}: release ${tag} lists no API asset ${assetName}`);
  }
  return getBytes(asset.url, {
    ...options,
    headers: { ...auth, Accept: 'application/octet-stream' },
  });
}
