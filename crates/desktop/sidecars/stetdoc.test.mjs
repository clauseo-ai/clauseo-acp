// Tests for stetdoc.mjs. Plain Node, no network: the fetch is a stub answering archives built
// here, and the pins are fixtures, except the checked-in pin, which is parsed as it is.
//
//   node --test 'crates/desktop/sidecars/*.test.mjs'

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, test } from 'node:test';
import { gzipSync } from 'node:zlib';
import { RELEASE_TARGETS } from '../../../.github/scripts/sidecars.mjs';
import { assetName, extractProgram, fetchStetdoc, parsePin } from './stetdoc.mjs';

const scratch = await mkdtemp(join(tmpdir(), 'stetdoc-test-'));
after(() => rm(scratch, { recursive: true, force: true }));

const VERSION = '9.9.9';
const PROGRAM = Buffer.from('#!fake stetdoc\n');
const url = (triple, version = VERSION) =>
  `https://github.com/clauseo-ai/stetdoc/releases/download/v${version}/${assetName(triple)}`;

/** A ustar archive (gzipped) of `entries`, [name, contents, type] each; type '0' unless given. */
function tarGz(entries) {
  const blocks = [];
  for (const [name, body, type = '0'] of entries) {
    const header = Buffer.alloc(512);
    header.write(name, 0, 'latin1');
    header.write('0000755\0', 100);
    header.write(`${body.length.toString(8).padStart(11, '0')}\0`, 124);
    header.write(type, 156);
    header.write('ustar\0', 257);
    header.write('00', 263);
    header.fill(' ', 148, 156);
    const sum = header.reduce((a, b) => a + b, 0);
    header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148);
    blocks.push(header, body, Buffer.alloc((512 - (body.length % 512)) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  return gzipSync(Buffer.concat(blocks));
}

/** A zip of `entries`, [name, contents] each, stored (no compression). */
function zip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, body] of entries) {
    const nameBytes = Buffer.from(name);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(body.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt32LE(body.length, 20);
    central.writeUInt32LE(body.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt32LE(offset, 42);
    locals.push(local, nameBytes, body);
    centrals.push(central, nameBytes);
    offset += 30 + nameBytes.length + body.length;
  }
  const dir = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(dir.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, dir, end]);
}

/** The release archive for `triple`: a folder with the program and a licence file. */
function archive(triple) {
  const folder = `stetdoc-${triple}`;
  if (triple.includes('-windows-')) {
    return zip([[`${folder}/`, Buffer.alloc(0)], [`${folder}/LICENSE-MIT`, Buffer.from('MIT')], [`${folder}/stetdoc.exe`, PROGRAM]]);
  }
  return tarGz([[`${folder}/`, Buffer.alloc(0), '5'], [`${folder}/LICENSE-MIT`, Buffer.from('MIT')], [`${folder}/stetdoc`, PROGRAM]]);
}

const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');

/** A fixture pin with every release target pinned, except those in `pending`. */
async function pin({ pending = [], body = (t) => `url = "${url(t)}"\nsha256 = "${sha(archive(t))}"\n` } = {}) {
  let text = `version = "${VERSION}"\n`;
  for (const triple of RELEASE_TARGETS) {
    text += `\n[${triple}]\n${pending.includes(triple) ? 'pending = "PENDING_TEST: not yet"\n' : body(triple)}`;
  }
  const dir = await mkdtemp(join(scratch, 'pin-'));
  const path = join(dir, 'stetdoc.toml');
  await writeFile(path, text);
  return { dir, path };
}

const serve = (bytesFor) => {
  const calls = [];
  const fetch = async (requested) => {
    calls.push(requested);
    return new Response(bytesFor(requested), { status: 200 });
  };
  return { calls, fetch };
};
const quiet = () => {};

test('the checked-in pin names every release target, each pinned, none pending', async () => {
  const text = await readFile(new URL('../stetdoc.toml', import.meta.url), 'utf8');
  const { version, triples } = parsePin(text);
  assert.match(version, /^\d+\.\d+\.\d+$/);
  assert.deepEqual([...triples.keys()].sort(), [...RELEASE_TARGETS].sort());
  for (const [triple, entry] of triples) {
    assert.ok(!('pending' in entry), `${triple} is still pending`);
    assert.equal(entry.url, url(triple, version), triple);
    assert.match(entry.sha256, /^[0-9a-f]{64}$/, triple);
  }
});

test('each release target fetches its archive and writes only the program, executable off Windows', async () => {
  const fixture = await pin();
  const server = serve((requested) => archive(RELEASE_TARGETS.find((t) => requested === url(t))));
  for (const triple of RELEASE_TARGETS) {
    const dest = join(fixture.dir, `out-${triple}`);
    const lines = [];
    assert.equal(await fetchStetdoc(triple, dest, { env: {}, pinPath: fixture.path, fetch: server.fetch, log: (l) => lines.push(l) }), dest);
    assert.deepEqual(await readFile(dest), PROGRAM, triple);
    assert.match(lines.join('\n'), /sha256 [0-9a-f]{64} matches the pin/, triple);
    if (process.platform !== 'win32' && !triple.includes('-windows-')) {
      assert.equal((await stat(dest)).mode & 0o777, 0o755, triple);
    }
  }
  assert.deepEqual(server.calls, RELEASE_TARGETS.map((t) => url(t)));
});

test('a pending triple fails loudly, before any download, and writes nothing', async () => {
  const fixture = await pin({ pending: ['x86_64-pc-windows-msvc'] });
  const server = serve(() => Buffer.alloc(0));
  const dest = join(fixture.dir, 'pending.exe');
  await assert.rejects(
    fetchStetdoc('x86_64-pc-windows-msvc', dest, { env: {}, pinPath: fixture.path, fetch: server.fetch, log: quiet }),
    /x86_64-pc-windows-msvc is PENDING in crates\/desktop\/stetdoc.toml \(PENDING_TEST: not yet\)/,
  );
  assert.deepEqual(server.calls, []);
  assert.deepEqual(await readdir(fixture.dir), ['stetdoc.toml']);
});

test('a checksum mismatch fails and writes nothing', async () => {
  const fixture = await pin();
  const server = serve(() => Buffer.from('tampered'));
  const dest = join(fixture.dir, 'stetdoc');
  await assert.rejects(
    fetchStetdoc('aarch64-apple-darwin', dest, { env: {}, pinPath: fixture.path, fetch: server.fetch, log: quiet }),
    /sha256 for aarch64-apple-darwin is [0-9a-f]{64}, the pin says [0-9a-f]{64}; nothing written/,
  );
  assert.deepEqual(await readdir(fixture.dir), ['stetdoc.toml']);
});

test('a url that is not the pinned release asset for the triple fails before any download', async () => {
  const wrong = [
    (t) => url(t).replace('clauseo-ai', 'someone-else'),
    (t) => url(t, '1.0.0'),
    (t) => url(t).replace('https:', 'http:'),
    (t) => url('x86_64-unknown-linux-gnu').replace('x86_64-unknown-linux-gnu', `${t}-other`),
    (t) => `${url(t)}?x=1`,
  ];
  for (const make of wrong) {
    const fixture = await pin({ body: (t) => `url = "${make(t)}"\nsha256 = "${'0'.repeat(64)}"\n` });
    const server = serve(() => Buffer.alloc(0));
    await assert.rejects(
      fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'x'), { env: {}, pinPath: fixture.path, fetch: server.fetch, log: quiet }),
      /pinned url for aarch64-apple-darwin must be/,
    );
    assert.deepEqual(server.calls, []);
  }
});

test('a triple missing from the pin, or a non-200 answer, fails', async () => {
  const fixture = await pin();
  await assert.rejects(
    fetchStetdoc('riscv64gc-unknown-linux-gnu', join(fixture.dir, 'x'), { env: {}, pinPath: fixture.path, fetch: serve(() => '').fetch }),
    /not in stetdoc.toml/,
  );
  const notFound = async () => new Response('gone', { status: 404 });
  await assert.rejects(
    fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'x'), { env: {}, pinPath: fixture.path, fetch: notFound }),
    /answered HTTP 404/,
  );
});

test('the parser refuses a table with both pending and url, an empty pending, and unknown keys', () => {
  const sha256 = '0'.repeat(64);
  const bad = [
    `version = "1.0.0"\n[a]\nurl = "x"\nsha256 = "${sha256}"\npending = "why"\n`,
    'version = "1.0.0"\n[a]\npending = ""\n',
    'version = "1.0.0"\n[a]\nnone = "true"\n',
    'version = "1.0.0"\n[a]\nurl = "x"\n',
    `version = "1.0.0"\n[a]\nurl = "x"\nsha256 = "ABC"\n`,
    '[a]\npending = "why"\n',
    `version = "1.0"\n[a]\npending = "why"\n`,
    'version = "1.0.0"\n[a]\npending = "why"\n[a]\npending = "why"\n',
  ];
  for (const text of bad) assert.throws(() => parsePin(text), /stetdoc.toml/, text);
});

test('an archive with no program, or two, is refused', () => {
  assert.throws(() => extractProgram(tarGz([['d/README.md', Buffer.from('x')]]), 'stetdoc', false), /0 files named stetdoc/);
  assert.throws(
    () => extractProgram(tarGz([['a/stetdoc', PROGRAM], ['b/stetdoc', PROGRAM]]), 'stetdoc', false),
    /2 files named stetdoc/,
  );
  assert.throws(() => extractProgram(zip([['d/stetdoc', PROGRAM]]), 'stetdoc.exe', true), /0 files named stetdoc.exe/);
  // A link or folder named stetdoc is not the program.
  assert.throws(() => extractProgram(tarGz([['d/stetdoc', Buffer.alloc(0), '2']]), 'stetdoc', false), /0 files/);
});

/** A fetch that answers each call from `answers` in turn (a Response, or an Error to throw), recording url and headers. */
function scripted(answers) {
  const calls = [];
  const fetch = async (requested, init) => {
    calls.push({ url: requested, headers: init?.headers ?? {} });
    const next = answers.shift();
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

const ok = (triple) => () => new Response(archive(triple), { status: 200 });
const status = (code) => () => new Response('x', { status: code });

test('a network error, a 5xx and a 429 are tried again, 30 s apart, then the download succeeds', async () => {
  const fixture = await pin();
  const triple = 'aarch64-apple-darwin';
  const server = scripted([new TypeError('fetch failed'), status(504), status(429), ok(triple)]);
  const timer = waits();
  const dest = join(fixture.dir, 'retried');
  await fetchStetdoc(triple, dest, { env: {}, pinPath: fixture.path, fetch: server.fetch, wait: timer.wait, log: quiet });
  assert.deepEqual(await readFile(dest), PROGRAM);
  assert.deepEqual(timer.delays, [30_000, 30_000, 30_000]);
  assert.deepEqual(server.calls.map((c) => c.url), Array(4).fill(url(triple)));
});

test('after 5 failed tries the download fails, and writes nothing', async () => {
  const fixture = await pin();
  const server = scripted(Array.from({ length: 5 }, () => status(503)));
  const timer = waits();
  await assert.rejects(
    fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'x'), { env: {}, pinPath: fixture.path, fetch: server.fetch, wait: timer.wait, log: quiet }),
    /answered HTTP 503/,
  );
  assert.equal(server.calls.length, 5);
  assert.deepEqual(timer.delays, Array(4).fill(30_000));
  assert.deepEqual(await readdir(fixture.dir), ['stetdoc.toml']);
});

test('a 404 is not tried again', async () => {
  const fixture = await pin();
  const server = scripted([status(404), ok('aarch64-apple-darwin')]);
  const timer = waits();
  await assert.rejects(
    fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'x'), { env: {}, pinPath: fixture.path, fetch: server.fetch, wait: timer.wait, log: quiet }),
    /answered HTTP 404/,
  );
  assert.equal(server.calls.length, 1);
  assert.deepEqual(timer.delays, []);
});

test('a checksum mismatch is not tried again', async () => {
  const fixture = await pin();
  const server = scripted([() => new Response('tampered', { status: 200 }), ok('aarch64-apple-darwin')]);
  const timer = waits();
  await assert.rejects(
    fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'x'), { env: {}, pinPath: fixture.path, fetch: server.fetch, wait: timer.wait, log: quiet }),
    /sha256 for aarch64-apple-darwin is [0-9a-f]{64}, the pin says/,
  );
  assert.equal(server.calls.length, 1);
  assert.deepEqual(timer.delays, []);
});

test('with GH_TOKEN set, the asset comes through the GitHub API with the token, and the sha256 still decides', async () => {
  const fixture = await pin();
  const triple = 'x86_64-apple-darwin';
  const assetUrl = 'https://api.github.com/repos/clauseo-ai/stetdoc/releases/assets/42';
  const release = { assets: [{ name: 'other.tar.gz', url: `${assetUrl}0` }, { name: assetName(triple), url: assetUrl }] };
  const server = scripted([status(502), () => Response.json(release), ok(triple)]);
  const timer = waits();
  const dest = join(fixture.dir, 'api');
  await fetchStetdoc(triple, dest, { env: { GH_TOKEN: 'ghs_test' }, pinPath: fixture.path, fetch: server.fetch, wait: timer.wait, log: quiet });
  assert.deepEqual(await readFile(dest), PROGRAM);
  const tagUrl = `https://api.github.com/repos/clauseo-ai/stetdoc/releases/tags/v${VERSION}`;
  assert.deepEqual(server.calls.map((c) => c.url), [tagUrl, tagUrl, assetUrl]);
  for (const call of server.calls) assert.equal(call.headers.Authorization, 'Bearer ghs_test');
  assert.equal(server.calls[2].headers.Accept, 'application/octet-stream');
  assert.deepEqual(timer.delays, [30_000]);

  // The same route with bytes that do not match the pin fails, once.
  const bad = scripted([() => Response.json(release), () => new Response('tampered', { status: 200 })]);
  await assert.rejects(
    fetchStetdoc(triple, join(fixture.dir, 'bad'), { env: { GH_TOKEN: 'ghs_test' }, pinPath: fixture.path, fetch: bad.fetch, wait: timer.wait, log: quiet }),
    /sha256 for x86_64-apple-darwin is/,
  );
  assert.equal(bad.calls.length, 2);
});

test('without GH_TOKEN no Authorization header is sent', async () => {
  const fixture = await pin();
  const server = scripted([ok('aarch64-apple-darwin')]);
  await fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'anon'), { env: { GH_TOKEN: '' }, pinPath: fixture.path, fetch: server.fetch, log: quiet });
  assert.equal(server.calls[0].url, url('aarch64-apple-darwin'));
  assert.equal(server.calls[0].headers.Authorization, undefined);
});

test('with GH_TOKEN, a release that lists no API url for the asset fails before any download', async () => {
  const fixture = await pin();
  const release = { assets: [{ name: assetName('aarch64-apple-darwin'), url: 'https://example.com/evil' }] };
  const server = scripted([() => Response.json(release)]);
  await assert.rejects(
    fetchStetdoc('aarch64-apple-darwin', join(fixture.dir, 'x'), { env: { GH_TOKEN: 't' }, pinPath: fixture.path, fetch: server.fetch, log: quiet }),
    /lists no API asset/,
  );
  assert.equal(server.calls.length, 1);
});
