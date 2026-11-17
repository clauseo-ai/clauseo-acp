// Tests for agent-browser.mjs. Plain Node, no install, and no network: the fetch is a stub, and
// the pins are fixtures written to a temp folder (plus the real pin, read but never fetched).
//
//   node --test 'crates/desktop/sidecars/*.test.mjs'
//
// CI does not run this file (the xtask check is frozen); see the header of agent-browser.mjs.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, test } from "node:test";

import { fetchAgentBrowser, ourAssetUrl, parsePin } from "./agent-browser.mjs";

const root = await mkdtemp(join(tmpdir(), "agent-browser-test-"));
after(() => rm(root, { recursive: true, force: true }));

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const VERSION = "0.41.1";
const PREFIX = `https://github.com/vercel-labs/agent-browser/releases/download/v${VERSION}`;
const TAG = `agent-browser-v${VERSION}-clauseo.3`;
const OURS = `https://github.com/clauseo-ai/clauseo-acp/releases/download/${TAG}`;
const COMMIT = "5868f7c05020e047c00e68e4abf9253cce864f41";
const TARBALL = "ca59c4590a5a265d106c494d7726b9cde8575536d31834709dd525da8d778f6f";
/** The five keys before the first table, as a well-formed pin holds them. */
const TOP = [
  `version = "${VERSION}"`,
  `patched_tag = "${TAG}"`,
  `upstream_commit = "${COMMIT}"`,
  `rustc_commit = "${"d".repeat(40)}"`,
  `upstream_tarball_sha256 = "${TARBALL}"`,
].join("\n");
const BODY = Buffer.from("#!fake agent-browser binary\n");

let counter = 0;
/** A fixture pin with one triple (linux x64 unless given), its url and sha as given. */
async function pinFile(url, digest, triple = "x86_64-unknown-linux-gnu") {
  counter += 1;
  const path = join(root, `pin-${counter}.toml`);
  await writeFile(
    path,
    `${TOP}\n\n[${triple}]\nurl = "${url}"\nsha256 = "${digest}"\n`,
  );
  return path;
}

/**
 * A stub fetch: records the calls and answers with `status`, `headers` and a body. `body` is a
 * Buffer, a list of chunks (for a body that grows past the cap), or null (a body-less response).
 */
function stubFetch(body = BODY, status = 200, headers = {}) {
  const calls = [];
  const fetch = async (url, init) => {
    calls.push({ url, init });
    const chunks = body === null ? null : Array.isArray(body) ? body : [body];
    return {
      ok: status >= 200 && status < 300,
      status,
      headers: new Headers(headers),
      body:
        chunks === null
          ? null
          : new ReadableStream({
              start(controller) {
                for (const chunk of chunks) controller.enqueue(chunk);
                controller.close();
              },
            }),
      arrayBuffer: async () => BODY,
    };
  };
  return { fetch, calls };
}

async function destIn(name) {
  const dir = await mkdtemp(join(root, "dest-"));
  return join(dir, name);
}

async function exists(path) {
  return stat(path).then(() => true, () => false);
}

/** A fetch that answers each call from `answers` in turn: a function returning a Response. */
function scripted(answers) {
  const calls = [];
  const fetch = async (url, init) => {
    calls.push({ url, headers: init?.headers ?? {} });
    const next = answers.shift();
    if (next === undefined) throw new Error(`unexpected request to ${url}`);
    return next();
  };
  return { calls, fetch };
}

/** A wait that records each delay instead of sleeping. */
function waits() {
  const delays = [];
  return { delays, wait: async (ms) => void delays.push(ms) };
}

describe("fetchAgentBrowser", () => {
  test("an unknown triple throws and fetches nothing", async () => {
    const { fetch, calls } = stubFetch();
    const dest = await destIn("agent-browser-riscv64.bin");
    await assert.rejects(
      fetchAgentBrowser("riscv64gc-unknown-linux-gnu", dest, { fetch, env: {} }),
      /not in agent-browser\.toml/,
    );
    assert.equal(calls.length, 0);
    assert.equal(await exists(dest), false);
  });

  test("a url off github.com's release folder throws before any download", async () => {
    const bad = [
      `https://evil.example/vercel-labs/agent-browser/releases/download/v${VERSION}/agent-browser-linux-x64`,
      `https://github.com.evil.example/vercel-labs/agent-browser/releases/download/v${VERSION}/agent-browser-linux-x64`,
      `http://github.com/vercel-labs/agent-browser/releases/download/v${VERSION}/agent-browser-linux-x64`,
      `https://github.com/someone/agent-browser/releases/download/v${VERSION}/agent-browser-linux-x64`,
      `https://github.com/vercel-labs/agent-browser/releases/download/v9.9.9/agent-browser-linux-x64`,
      `https://github.com/vercel-labs/agent-browser/releases/download/v${VERSION}/nested/agent-browser-linux-x64`,
      `https://user@github.com/vercel-labs/agent-browser/releases/download/v${VERSION}/agent-browser-linux-x64`,
    ];
    for (const url of bad) {
      const pinPath = await pinFile(url, sha(BODY));
      const { fetch, calls } = stubFetch();
      const dest = await destIn("agent-browser-linux-x64");
      await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /pinned url/, url);
      assert.equal(calls.length, 0, url);
    }
  });

  test("a sha256 mismatch throws and writes nothing", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(Buffer.from("the pinned bytes")));
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /sha256 for/);
    assert.equal(calls.length, 1);
    assert.equal(await exists(dest), false);
    assert.equal(await exists(`${dest}.partial`), false);
  });

  test("a failed HTTP status throws and writes nothing", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch } = stubFetch(BODY, 404);
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /HTTP 404/);
    assert.equal(await exists(dest), false);
  });

  test("a 204 with the right bytes throws and writes nothing: only 200 is a download", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch } = stubFetch(BODY, 204);
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /not 200/);
    assert.equal(await exists(dest), false);
    assert.equal(await exists(`${dest}.partial`), false);
  });

  test("an encoded slash, an encoded dot pair or a dot-dot path in the asset name is refused", async () => {
    const bad = [
      `${PREFIX}/..%2F..%2Fx`,
      `${PREFIX}/a%2Fb`,
      `${PREFIX}/../../x`,
    ];
    for (const url of bad) {
      const pinPath = await pinFile(url, sha(BODY));
      const { fetch, calls } = stubFetch(BODY);
      const dest = await destIn("agent-browser-linux-x64");
      await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /pinned url/, url);
      assert.equal(calls.length, 0, url);
    }
  });

  test("the download carries a timeout signal", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch, calls } = stubFetch(BODY);
    await fetchAgentBrowser("x86_64-unknown-linux-gnu", await destIn("agent-browser-linux-x64"), { fetch, pinPath, env: {} });
    assert.ok(calls[0].init.signal instanceof AbortSignal);
    assert.equal(calls[0].init.signal.aborted, false);
  });

  test("a Content-Length over 64 MB is refused, and the body is cancelled unread", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    let cancelled = false;
    const fetch = async () => ({
      status: 200,
      headers: new Headers({ "content-length": String(64 * 1024 * 1024 + 1) }),
      body: new ReadableStream({
        start(controller) {
          controller.enqueue(BODY);
          controller.close();
        },
        cancel() {
          cancelled = true;
        },
      }),
    });
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /over 67108864 bytes/);
    assert.equal(cancelled, true);
    assert.equal(await exists(dest), false);
  });

  test("a body that grows past 64 MB is refused and writes nothing", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const chunk = new Uint8Array(16 * 1024 * 1024);
    const { fetch } = stubFetch([chunk, chunk, chunk, chunk, chunk]); // 80 MB, no Content-Length
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), /over 67108864 bytes/);
    assert.equal(await exists(dest), false);
    assert.equal(await exists(`${dest}.partial`), false);
  });

  test("a body-less 200 is read through arrayBuffer and checked like any other", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch } = stubFetch(null);
    const dest = await destIn("agent-browser-linux-x64");
    await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} });
    assert.deepEqual(await readFile(dest), BODY);
  });

  test("a matching sha256 writes the file, mode 0755, and no .partial", async (t) => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-x86_64-unknown-linux-gnu");
    const returned = await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} });
    assert.equal(returned, dest);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].url, `${PREFIX}/agent-browser-linux-x64`);
    assert.equal(calls[0].init.redirect, "follow");
    assert.deepEqual(await readFile(dest), BODY);
    assert.equal(await exists(`${dest}.partial`), false);
    if (process.platform === "win32") {
      t.skip("POSIX mode bits do not apply on Windows");
      return;
    }
    assert.equal((await stat(dest)).mode & 0o777, 0o755);
  });

  test("a triple with none = true returns null and writes nothing", async () => {
    const pinPath = join(root, "none.toml");
    await writeFile(pinPath, `${TOP}\n\n[x86_64-unknown-linux-gnu]\nnone = true\n`);
    const { fetch, calls } = stubFetch();
    const dest = await destIn("agent-browser-x86_64-unknown-linux-gnu");
    assert.equal(await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} }), null);
    assert.equal(calls.length, 0);
    assert.equal(await exists(dest), false);
  });

  test("a Windows triple downloads our build, its destination taking .exe as the caller gives it", async () => {
    const url = `${OURS}/agent-browser-x86_64-pc-windows-msvc.exe`;
    const pinPath = await pinFile(url, sha(BODY), "x86_64-pc-windows-msvc");
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-x86_64-pc-windows-msvc.exe");
    await fetchAgentBrowser("x86_64-pc-windows-msvc", dest, { fetch, pinPath, env: {} });
    assert.equal(calls[0].url, url);
    assert.deepEqual(await readFile(dest), BODY);
  });

  test("a Windows url off both releases, or with another triple's asset, throws before any download", async () => {
    const bad = [
      `https://github.com/vercel-labs/agent-browser/releases/download/${TAG}/agent-browser-x86_64-pc-windows-msvc.exe`,
      `${OURS}/agent-browser-aarch64-pc-windows-msvc.exe`,
      `${OURS}/agent-browser-win32-x64.exe`,
      `https://github.com/clauseo-ai/clauseo-acp/releases/download/agent-browser-v${VERSION}-clauseo.2/agent-browser-x86_64-pc-windows-msvc.exe`,
      `https://github.com/someone/clauseo-acp/releases/download/${TAG}/agent-browser-x86_64-pc-windows-msvc.exe`,
      `${OURS}/agent-browser-x86_64-pc-windows-msvc.exe?x=1`,
    ];
    for (const url of bad) {
      const pinPath = await pinFile(url, sha(BODY), "x86_64-pc-windows-msvc");
      const { fetch, calls } = stubFetch(BODY);
      const dest = await destIn("agent-browser-x86_64-pc-windows-msvc.exe");
      await assert.rejects(fetchAgentBrowser("x86_64-pc-windows-msvc", dest, { fetch, pinPath, env: {} }), /pinned url/, url);
      assert.equal(calls.length, 0, url);
    }
  });

  test("a Linux url on our release takes agent-browser-<triple>, with no .exe", async () => {
    const good = `${OURS}/agent-browser-x86_64-unknown-linux-gnu`;
    const pinPath = await pinFile(good, sha(BODY));
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-linux-x64");
    await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath, env: {} });
    assert.deepEqual(calls.map((c) => c.url), [good]);
    for (const url of [`${good}.exe`, `${OURS}/agent-browser-linux-x64`, `${OURS}/agent-browser-aarch64-unknown-linux-gnu`]) {
      const badPin = await pinFile(url, sha(BODY));
      const stub = stubFetch(BODY);
      await assert.rejects(
        fetchAgentBrowser("x86_64-unknown-linux-gnu", await destIn("x"), { fetch: stub.fetch, pinPath: badPin, env: {} }),
        /pinned url/,
        url,
      );
      assert.equal(stub.calls.length, 0, url);
    }
  });

  test("a Windows table may still name upstream's release (for a triple handed back upstream)", async () => {
    const url = `${PREFIX}/agent-browser-win32-x64.exe`;
    const pinPath = await pinFile(url, sha(BODY), "x86_64-pc-windows-msvc");
    const { fetch, calls } = stubFetch(BODY);
    await fetchAgentBrowser("x86_64-pc-windows-msvc", await destIn("a.exe"), { fetch, pinPath, env: {} });
    assert.deepEqual(calls.map((c) => c.url), [url]);
  });

  test("the all-zero placeholder sha256 throws before any download, naming the workflow", async () => {
    const pinPath = await pinFile(
      `${OURS}/agent-browser-aarch64-pc-windows-msvc.exe`,
      "0".repeat(64),
      "aarch64-pc-windows-msvc",
    );
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-aarch64-pc-windows-msvc.exe");
    await assert.rejects(
      fetchAgentBrowser("aarch64-pc-windows-msvc", dest, { fetch, pinPath, env: { GH_TOKEN: "ghs_test" } }),
      /placeholder; run \.github\/workflows\/agent-browser-build\.yml/,
    );
    assert.equal(calls.length, 0);
    assert.equal(await exists(dest), false);
  });

  test("with GH_TOKEN a Windows build comes from our release, by patched_tag", async () => {
    const pinPath = await pinFile(
      `${OURS}/agent-browser-aarch64-pc-windows-msvc.exe`,
      sha(BODY),
      "aarch64-pc-windows-msvc",
    );
    const assetApi = "https://api.github.com/repos/clauseo-ai/clauseo-acp/releases/assets/9";
    const release = { assets: [{ name: "agent-browser-aarch64-pc-windows-msvc.exe", url: assetApi }] };
    const server = scripted([() => Response.json(release), () => new Response(BODY, { status: 200 })]);
    const dest = await destIn("agent-browser-aarch64-pc-windows-msvc.exe");
    await fetchAgentBrowser("aarch64-pc-windows-msvc", dest, {
      fetch: server.fetch,
      pinPath,
      env: { GH_TOKEN: "ghs_test" },
      wait: waits().wait,
      log: () => {},
    });
    assert.deepEqual(server.calls.map((c) => c.url), [
      `https://api.github.com/repos/clauseo-ai/clauseo-acp/releases/tags/${TAG}`,
      assetApi,
    ]);
    assert.deepEqual(await readFile(dest), BODY);
  });

  test("a 503 then a 200 succeeds after one wait", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const server = scripted([
      () => new Response("busy", { status: 503 }),
      () => new Response(BODY, { status: 200 }),
    ]);
    const timer = waits();
    const dest = await destIn("agent-browser-linux-x64");
    await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, {
      fetch: server.fetch,
      pinPath,
      env: {},
      wait: timer.wait,
      log: () => {},
    });
    assert.equal(server.calls.length, 2);
    assert.deepEqual(timer.delays, [30_000]);
    assert.deepEqual(await readFile(dest), BODY);
  });

  test("with GH_TOKEN it goes through the API: the release by its tag, then the asset's API url", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const assetApi = "https://api.github.com/repos/vercel-labs/agent-browser/releases/assets/7";
    const release = {
      assets: [
        { name: "agent-browser-darwin-x64", url: `${assetApi}1` },
        { name: "agent-browser-linux-x64", url: assetApi },
      ],
    };
    const server = scripted([() => Response.json(release), () => new Response(BODY, { status: 200 })]);
    const dest = await destIn("agent-browser-linux-x64");
    await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, {
      fetch: server.fetch,
      pinPath,
      env: { GH_TOKEN: "ghs_test" },
      wait: waits().wait,
      log: () => {},
    });
    assert.deepEqual(server.calls.map((c) => c.url), [
      `https://api.github.com/repos/vercel-labs/agent-browser/releases/tags/v${VERSION}`,
      assetApi,
    ]);
    for (const call of server.calls) assert.equal(call.headers.Authorization, "Bearer ghs_test");
    assert.equal(server.calls[0].headers.Accept, "application/vnd.github+json");
    assert.equal(server.calls[1].headers.Accept, "application/octet-stream");
    assert.deepEqual(await readFile(dest), BODY);
  });
});

describe("parsePin", () => {
  test("reads the real pin: the upstream source, six triples, each with a url and a sha256", async () => {
    const text = await readFile(new URL("../agent-browser.toml", import.meta.url), "utf8");
    const pin = parsePin(text);
    assert.equal(pin.version, VERSION);
    assert.equal(pin.patchedTag, TAG);
    assert.equal(pin.upstreamCommit, COMMIT);
    assert.equal(pin.upstreamTarballSha256, TARBALL);
    assert.deepEqual(
      [...pin.triples.keys()].sort(),
      [
        "aarch64-apple-darwin",
        "aarch64-pc-windows-msvc",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
      ],
    );
    for (const [triple, entry] of pin.triples) {
      assert.equal(entry.none, undefined, `${triple} has no build`);
      assert.match(entry.sha256, /^[0-9a-f]{64}$/, triple);
      // macOS and Windows take our build today, Linux upstream's (see the pin's header).
      if (triple.endsWith("-unknown-linux-gnu")) {
        assert.ok(entry.url.startsWith(`${PREFIX}/agent-browser-linux-`), triple);
        continue;
      }
      const ext = triple.endsWith("-pc-windows-msvc") ? ".exe" : "";
      assert.equal(entry.url, ourAssetUrl(triple, pin), triple);
      assert.equal(entry.url, `${OURS}/agent-browser-${triple}${ext}`, triple);
    }
  });

  test("rejects what the format does not allow", () => {
    const cases = {
      "no version": `[x86_64-unknown-linux-gnu]\nurl = "https://github.com/a"\nsha256 = "${"0".repeat(64)}"\n`,
      "not semver": `version = "v1"\n`,
      "no patched_tag": TOP.replace(/^patched_tag.*\n/m, ""),
      "patched_tag of another version": TOP.replace(TAG, "agent-browser-v0.41.2-clauseo.1"),
      "patched_tag numbered 0": TOP.replace(TAG, `agent-browser-v${VERSION}-clauseo.0`),
      "patched_tag with a dot wildcard": TOP.replace(TAG, "agent-browser-v0x41x1-clauseo.1"),
      "short upstream_commit": TOP.replace(COMMIT, COMMIT.slice(1)),
      "upper-case upstream_tarball_sha256": TOP.replace(TARBALL, TARBALL.toUpperCase()),
      "no upstream_tarball_sha256": TOP.replace(/\nupstream_tarball_sha256.*$/, ""),
      "no rustc_commit": TOP.replace(/^rustc_commit.*\n/m, ""),
      "short rustc_commit": TOP.replace("d".repeat(40), "d".repeat(39)),
      "a top key twice": `${TOP}\nupstream_commit = "${COMMIT}"\n`,
      "an unknown top key": `${TOP}\nupstream = "x"\n`,
      "a top key inside a table": `${TOP}\n[t]\nnone = true\npatched_tag = "${TAG}"\n`,
      "missing sha256": `${TOP}\n[t]\nurl = "https://github.com/a"\n`,
      "upper-case hex": `${TOP}\n[t]\nurl = "https://github.com/a"\nsha256 = "${"A".repeat(64)}"\n`,
      "unknown key": `${TOP}\n[t]\nurl = "https://github.com/a"\nsha256 = "${"0".repeat(64)}"\nsize = "1"\n`,
      "duplicate table": `${TOP}\n[t]\nnone = true\n[t]\nnone = true\n`,
      "none with url": `${TOP}\n[t]\nnone = true\nurl = "https://github.com/a"\n`,
      "inline table": `${TOP}\n[t]\nurl = { a = "b" }\n`,
      "trailing comment": `${TOP}\n[t]\nnone = true # why\n`,
    };
    for (const [name, text] of Object.entries(cases)) {
      assert.throws(() => parsePin(text), /agent-browser\.toml/, name);
    }
  });
});
