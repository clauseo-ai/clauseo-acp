// Tests for agent-browser.mjs. Plain Node, no install, and no network: the fetch is a stub, and
// the pins are fixtures written to a temp folder (plus the real pin, read but never fetched).
//
//   node --test crates/desktop/sidecars/
//
// CI does not run this file (the xtask check is frozen); see the header of agent-browser.mjs.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, stat, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, test } from "node:test";

import { fetchAgentBrowser, parsePin } from "./agent-browser.mjs";

const root = await mkdtemp(join(tmpdir(), "agent-browser-test-"));
after(() => rm(root, { recursive: true, force: true }));

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const VERSION = "0.41.1";
const PREFIX = `https://github.com/vercel-labs/agent-browser/releases/download/v${VERSION}`;
const BODY = Buffer.from("#!fake agent-browser binary\n");

let counter = 0;
/** A fixture pin with one triple (linux x64 unless given), its url and sha as given. */
async function pinFile(url, digest, triple = "x86_64-unknown-linux-gnu") {
  counter += 1;
  const path = join(root, `pin-${counter}.toml`);
  await writeFile(
    path,
    `version = "${VERSION}"\n\n[${triple}]\nurl = "${url}"\nsha256 = "${digest}"\n`,
  );
  return path;
}

/** A stub fetch: records the calls, answers with `body` and `status`. */
function stubFetch(body = BODY, status = 200) {
  const calls = [];
  const fetch = async (url, init) => {
    calls.push({ url, init });
    return {
      ok: status >= 200 && status < 300,
      status,
      arrayBuffer: async () => body.buffer.slice(body.byteOffset, body.byteOffset + body.byteLength),
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

describe("fetchAgentBrowser", () => {
  test("an unknown triple throws and fetches nothing", async () => {
    const { fetch, calls } = stubFetch();
    const dest = await destIn("agent-browser-riscv64.bin");
    await assert.rejects(
      fetchAgentBrowser("riscv64gc-unknown-linux-gnu", dest, { fetch }),
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
      await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath }), /pinned url/, url);
      assert.equal(calls.length, 0, url);
    }
  });

  test("a sha256 mismatch throws and writes nothing", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(Buffer.from("the pinned bytes")));
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath }), /sha256 for/);
    assert.equal(calls.length, 1);
    assert.equal(await exists(dest), false);
    assert.equal(await exists(`${dest}.partial`), false);
  });

  test("a failed HTTP status throws and writes nothing", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch } = stubFetch(BODY, 404);
    const dest = await destIn("agent-browser-linux-x64");
    await assert.rejects(fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath }), /HTTP 404/);
    assert.equal(await exists(dest), false);
  });

  test("a matching sha256 writes the file, mode 0755, and no .partial", async (t) => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-linux-x64`, sha(BODY));
    const { fetch, calls } = stubFetch(BODY);
    const dest = await destIn("agent-browser-x86_64-unknown-linux-gnu");
    const returned = await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath });
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
    await writeFile(pinPath, `version = "${VERSION}"\n\n[x86_64-unknown-linux-gnu]\nnone = true\n`);
    const { fetch, calls } = stubFetch();
    const dest = await destIn("agent-browser-x86_64-unknown-linux-gnu");
    assert.equal(await fetchAgentBrowser("x86_64-unknown-linux-gnu", dest, { fetch, pinPath }), null);
    assert.equal(calls.length, 0);
    assert.equal(await exists(dest), false);
  });

  test("a Windows triple's destination takes .exe as the caller gives it", async () => {
    const pinPath = await pinFile(`${PREFIX}/agent-browser-win32-x64.exe`, sha(BODY), "x86_64-pc-windows-msvc");
    const { fetch } = stubFetch(BODY);
    const dest = await destIn("agent-browser-x86_64-pc-windows-msvc.exe");
    await fetchAgentBrowser("x86_64-pc-windows-msvc", dest, { fetch, pinPath });
    assert.deepEqual(await readFile(dest), BODY);
  });
});

describe("parsePin", () => {
  test("reads the real pin: six triples, all with a url and a sha256 or none", async () => {
    const text = await readFile(new URL("../agent-browser.toml", import.meta.url), "utf8");
    const pin = parsePin(text);
    assert.equal(pin.version, VERSION);
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
      assert.ok(entry.none === true || /^[0-9a-f]{64}$/.test(entry.sha256), triple);
    }
  });

  test("rejects what the format does not allow", () => {
    const cases = {
      "no version": `[x86_64-unknown-linux-gnu]\nurl = "https://github.com/a"\nsha256 = "${"0".repeat(64)}"\n`,
      "not semver": `version = "v1"\n`,
      "missing sha256": `version = "1.0.0"\n[t]\nurl = "https://github.com/a"\n`,
      "upper-case hex": `version = "1.0.0"\n[t]\nurl = "https://github.com/a"\nsha256 = "${"A".repeat(64)}"\n`,
      "unknown key": `version = "1.0.0"\n[t]\nurl = "https://github.com/a"\nsha256 = "${"0".repeat(64)}"\nsize = "1"\n`,
      "duplicate table": `version = "1.0.0"\n[t]\nnone = true\n[t]\nnone = true\n`,
      "none with url": `version = "1.0.0"\n[t]\nnone = true\nurl = "https://github.com/a"\n`,
      "inline table": `version = "1.0.0"\n[t]\nurl = { a = "b" }\n`,
      "trailing comment": `version = "1.0.0"\n[t]\nnone = true # why\n`,
    };
    for (const [name, text] of Object.entries(cases)) {
      assert.throws(() => parsePin(text), /agent-browser\.toml/, name);
    }
  });
});
