// Tests for fetch.mjs, the download the sidecars share. Plain Node, no network: the fetch is a
// stub answering from a script, and the wait is a stub that records the delays.
//
//   node --test 'crates/desktop/sidecars/*.test.mjs'

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fetchGitHubAsset, getBytes } from './fetch.mjs';

const BODY = Buffer.from('the release bytes\n');
const PINNED = 'https://github.com/someone/tool/releases/download/v1.2.3/tool-linux';
const OWNER = 'someone';
const REPO = 'tool';
const TAG = 'v1.2.3';
const ASSET = 'tool-linux';
const TAGS_URL = `https://api.github.com/repos/${OWNER}/${REPO}/releases/tags/${TAG}`;
const ASSET_API = `https://api.github.com/repos/${OWNER}/${REPO}/releases/assets/42`;

/** A fetch that answers each call from `answers` in turn: a Response, or an Error to throw. */
function scripted(answers) {
  const calls = [];
  const fetch = async (url, init) => {
    calls.push({ url, headers: init?.headers ?? {}, init });
    const next = answers.shift();
    if (next === undefined) throw new Error(`unexpected request to ${url}`);
    if (next instanceof Error) throw next;
    return next();
  };
  return { calls, fetch };
}

/** A wait that records each delay instead of sleeping. */
function waits() {
  const delays = [];
  return { delays, wait: async (ms) => void delays.push(ms) };
}

const quiet = () => {};
const ok = (bytes = BODY) => () => new Response(bytes, { status: 200 });
const status = (code) => () => new Response('x', { status: code });
const release = (assets) => () => Response.json({ assets });

test('a network error is tried again, 30 s later, and the body comes back', async () => {
  const server = scripted([new TypeError('fetch failed'), ok()]);
  const timer = waits();
  const lines = [];
  const bytes = await getBytes(PINNED, {
    label: 'test: download',
    fetch: server.fetch,
    wait: timer.wait,
    log: (line) => lines.push(line),
  });
  assert.deepEqual(bytes, BODY);
  assert.equal(server.calls.length, 2);
  assert.deepEqual(timer.delays, [30_000]);
  assert.deepEqual(lines, ['test: download failed: fetch failed (try 1 of 5); trying again in 30 s']);
});

test('a 503 then a 429 then a 200: two waits, then the body', async () => {
  const server = scripted([status(503), status(429), ok()]);
  const timer = waits();
  assert.deepEqual(await getBytes(PINNED, { fetch: server.fetch, wait: timer.wait, log: quiet }), BODY);
  assert.equal(server.calls.length, 3);
  assert.deepEqual(timer.delays, [30_000, 30_000]);
});

test('five network errors throw after exactly four waits', async () => {
  const server = scripted(Array.from({ length: 5 }, () => new TypeError('fetch failed')));
  const timer = waits();
  await assert.rejects(
    getBytes(PINNED, { label: 'test: download', fetch: server.fetch, wait: timer.wait, log: quiet }),
    /test: download failed: fetch failed$/,
  );
  assert.equal(server.calls.length, 5);
  assert.deepEqual(timer.delays, [30_000, 30_000, 30_000, 30_000]);
});

test('a 404 throws at once, with no wait', async () => {
  const server = scripted([status(404), ok()]);
  const timer = waits();
  await assert.rejects(
    getBytes(PINNED, { label: 'test: download', fetch: server.fetch, wait: timer.wait, log: quiet }),
    /test: download answered HTTP 404, not 200/,
  );
  assert.equal(server.calls.length, 1);
  assert.deepEqual(timer.delays, []);
});

test('an oversized body throws at once, with no wait: declared, then growing while read', async () => {
  const timer = waits();
  const declared = scripted([
    () => new Response(BODY, { status: 200, headers: { 'content-length': '999' } }),
    ok(),
  ]);
  await assert.rejects(
    getBytes(PINNED, { label: 'test: download', maxBytes: 4, fetch: declared.fetch, wait: timer.wait, log: quiet }),
    /test: download is over 4 bytes; nothing written/,
  );
  assert.equal(declared.calls.length, 1);

  const growing = scripted([
    () =>
      new Response(
        new ReadableStream({
          start(controller) {
            controller.enqueue(Buffer.from('abc'));
            controller.enqueue(Buffer.from('def'));
            controller.close();
          },
        }),
        { status: 200 },
      ),
    ok(),
  ]);
  await assert.rejects(
    getBytes(PINNED, { label: 'test: download', maxBytes: 4, fetch: growing.fetch, wait: timer.wait, log: quiet }),
    /test: download is over 4 bytes/,
  );
  assert.equal(growing.calls.length, 1);
  assert.deepEqual(timer.delays, []);
});

test('with a token, the release comes by its tag, then the asset by its API url, both with the token', async () => {
  const assets = [
    { name: 'tool-darwin', url: `${ASSET_API}1` },
    { name: ASSET, url: ASSET_API },
  ];
  const server = scripted([release(assets), ok()]);
  const bytes = await fetchGitHubAsset({
    url: PINNED,
    owner: OWNER,
    repo: REPO,
    tag: TAG,
    assetName: ASSET,
    token: 'ghs_test',
    fetch: server.fetch,
    wait: waits().wait,
    log: quiet,
  });
  assert.deepEqual(bytes, BODY);
  assert.deepEqual(server.calls.map((c) => c.url), [TAGS_URL, ASSET_API]);
  assert.equal(server.calls[0].headers.Authorization, 'Bearer ghs_test');
  assert.equal(server.calls[0].headers.Accept, 'application/vnd.github+json');
  assert.equal(server.calls[0].headers['X-GitHub-Api-Version'], '2022-11-28');
  assert.equal(server.calls[1].headers.Authorization, 'Bearer ghs_test');
  assert.equal(server.calls[1].headers.Accept, 'application/octet-stream');
  assert.equal(server.calls[1].headers['X-GitHub-Api-Version'], '2022-11-28');
});

test('without a token there is one request, to the plain url, with no Authorization header', async () => {
  for (const token of [undefined, '']) {
    const server = scripted([ok()]);
    const bytes = await fetchGitHubAsset({
      url: PINNED,
      owner: OWNER,
      repo: REPO,
      tag: TAG,
      assetName: ASSET,
      token,
      fetch: server.fetch,
      wait: waits().wait,
      log: quiet,
    });
    assert.deepEqual(bytes, BODY);
    assert.equal(server.calls.length, 1);
    assert.equal(server.calls[0].url, PINNED);
    assert.equal(server.calls[0].headers.Authorization, undefined);
  }
});

test('an asset whose API url is for another repo throws before the asset is fetched', async () => {
  const other = 'https://api.github.com/repos/someone-else/tool/releases/assets/42';
  const server = scripted([release([{ name: ASSET, url: other }]), ok()]);
  await assert.rejects(
    fetchGitHubAsset({
      url: PINNED,
      owner: OWNER,
      repo: REPO,
      tag: TAG,
      assetName: ASSET,
      token: 'ghs_test',
      label: 'test: download',
      fetch: server.fetch,
      wait: waits().wait,
      log: quiet,
    }),
    /test: download: release v1\.2\.3 lists no API asset tool-linux/,
  );
  assert.equal(server.calls.length, 1);
});
