// Tests for agent-browser-build.mjs: the command line, the pin it reads, the patch order, the
// sha256 refusal, the archive's header and the build environment. No network: `check` runs on a
// small archive made here, through the real tar and git, with a stub fetch.
//
//   node --test 'crates/desktop/sidecars/*.test.mjs'
//
// CI does not run this file (the xtask check is frozen); see the header of agent-browser.mjs.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, test } from "node:test";
import { gzipSync } from "node:zlib";

import {
  TARGETS,
  buildEnv,
  checkCommitted,
  checkSha,
  checkVersion,
  main,
  orderPatches,
  parseArgs,
  pinLines,
  rustcCommit,
  tarInfo,
  testEnv,
  toolchainChannel,
  workParent,
} from "./agent-browser-build.mjs";
import { parsePin } from "./agent-browser.mjs";

const root = await mkdtemp(join(tmpdir(), "agent-browser-build-test-"));
after(() => rm(root, { recursive: true, force: true }));

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const COMMIT = "0123456789abcdef0123456789abcdef01234567";
const MTIME = 1761884640;

/** One 512-byte ustar header. */
function header(name, { type = "0", size = 0, mtime = MTIME, prefix = "" } = {}) {
  const block = Buffer.alloc(512);
  block.write(name, 0, "utf8");
  block.write(type === "5" ? "0000755\0" : "0000644\0", 100);
  block.write(`${size.toString(8).padStart(11, "0")}\0`, 124);
  block.write(`${mtime.toString(8).padStart(11, "0")}\0`, 136);
  block.write(type, 156);
  block.write("ustar\0", 257);
  block.write("00", 263);
  block.write(prefix, 345, "utf8");
  // The checksum: the header's bytes summed with the checksum field as spaces.
  block.write("        ", 148);
  const sum = block.reduce((total, byte) => total + byte, 0);
  block.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148);
  return block;
}

/** A data block run, padded to 512. */
const pad = (data) => Buffer.concat([data, Buffer.alloc((512 - (data.length % 512)) % 512)]);

/** A pax record set as a header's data. */
function pax(records) {
  let text = "";
  for (const [key, value] of Object.entries(records)) {
    const body = ` ${key}=${value}\n`;
    let length = body.length + 1;
    while (`${length}${body}`.length !== length) length += 1;
    text += `${length}${body}`;
  }
  return Buffer.from(text);
}

/** GitHub's archive shape: a global pax header with the commit, a top folder, then `files`. */
function archive(commit, files, { symlink } = {}) {
  const top = `agent-browser-${commit}/`;
  const parts = [];
  if (commit !== null) {
    const data = pax({ comment: commit });
    parts.push(header("pax_global_header", { type: "g", size: data.length, mtime: 0 }), pad(data));
  }
  parts.push(header(top, { type: "5" }));
  for (const [name, text] of Object.entries(files)) {
    const data = Buffer.from(text);
    parts.push(header(`${top}${name}`, { size: data.length }), pad(data));
  }
  if (symlink) parts.push(header(`${top}${symlink}`, { type: "2" }));
  parts.push(Buffer.alloc(1024));
  return gzipSync(Buffer.concat(parts));
}

const CARGO = '[package]\nname = "agent-browser"\nversion = "0.41.1"\n';
const LOCK = '[[package]]\nname = "aes"\nversion = "0.8.4"\n\n[[package]]\nname = "agent-browser"\nversion = "0.41.1"\n';
/** The version patch, as 0003: Cargo.toml and Cargo.lock to `version`. */
const versionPatch = (version) =>
  [
    "diff --git a/cli/Cargo.lock b/cli/Cargo.lock",
    "--- a/cli/Cargo.lock",
    "+++ b/cli/Cargo.lock",
    "@@ -4,4 +4,4 @@",
    " ",
    " [[package]]",
    ' name = "agent-browser"',
    '-version = "0.41.1"',
    `+version = "${version}"`,
    "diff --git a/cli/Cargo.toml b/cli/Cargo.toml",
    "--- a/cli/Cargo.toml",
    "+++ b/cli/Cargo.toml",
    "@@ -1,3 +1,3 @@",
    " [package]",
    ' name = "agent-browser"',
    '-version = "0.41.1"',
    `+version = "${version}"`,
    "",
  ].join("\n");
const PATCH = versionPatch("0.41.1-clauseo.1");
const SOURCE = { "cli/Cargo.toml": CARGO, "cli/Cargo.lock": LOCK };

let counter = 0;
/** A pin fixture for `gz`'s commit and hash, and a patch folder holding `patches`. */
async function fixture(gz, patches, commit = COMMIT, build = 1) {
  counter += 1;
  const dir = join(root, `fixture-${counter}`);
  const patchDir = join(dir, "patches");
  await mkdir(patchDir, { recursive: true });
  for (const [name, text] of Object.entries(patches)) await writeFile(join(patchDir, name), text);
  const pinPath = join(dir, "agent-browser.toml");
  await writeFile(
    pinPath,
    [
      'version = "0.41.1"',
      `patched_tag = "agent-browser-v0.41.1-clauseo.${build}"`,
      `upstream_commit = "${commit}"`,
      `upstream_tarball_sha256 = "${sha(gz)}"`,
      `rustc_commit = "${"d".repeat(40)}"`,
      "",
    ].join("\n"),
  );
  return { pinPath, patchDir };
}

/** A fetch answering every request with `gz`, recording the urls. */
function serve(gz) {
  const calls = [];
  return { calls, fetch: async (url) => (calls.push(url), new Response(gz, { status: 200 })) };
}

describe("parseArgs", () => {
  test("reads each command", () => {
    assert.deepEqual(parseArgs(["check"]), { command: "check" });
    assert.deepEqual(parseArgs(["tag"]), { command: "tag" });
    assert.deepEqual(parseArgs(["toolchain"]), { command: "toolchain" });
    for (const target of TARGETS) {
      assert.deepEqual(parseArgs(["build", "--out", "dist", "--target", target]), {
        command: "build",
        target,
        out: "dist",
      });
    }
  });

  test("refuses anything else", () => {
    const bad = [
      [],
      ["fetch"],
      ["check", "--target", TARGETS[0]],
      ["build"],
      ["build", "--target", TARGETS[0]],
      ["build", "--out", "dist"],
      ["build", "--target", "riscv64gc-unknown-linux-gnu", "--out", "dist"],
      ["build", "--target", "x86_64-unknown-linux-musl", "--out", "dist"],
      ["build", "--target", TARGETS[0], "--out"],
      ["build", "--target", "--out", "dist"],
      ["build", "--target", TARGETS[0], "--target", TARGETS[1], "--out", "dist"],
      ["build", "--target", TARGETS[0], "--out", "dist", "--keep", "yes"],
    ];
    for (const argv of bad) assert.throws(() => parseArgs(argv), undefined, argv.join(" "));
  });
});

describe("orderPatches", () => {
  test("orders by number and skips dot files", () => {
    assert.deepEqual(
      orderPatches(["0002-b.patch", ".gitkeep", "0001-windows-pipe-and-hardening.patch"]),
      ["0001-windows-pipe-and-hardening.patch", "0002-b.patch"],
    );
  });

  test("refuses no patch, a gap, a number twice and a misnamed file", () => {
    const bad = {
      empty: [],
      "only a dot file": [".gitkeep"],
      gap: ["0001-a.patch", "0003-c.patch"],
      "not from 0001": ["0002-a.patch"],
      twice: ["0001-a.patch", "0001-b.patch"],
      "a .diff": ["0001-a.patch", "0002-b.diff"],
      "a README": ["0001-a.patch", "README.md"],
      "upper case": ["0001-Windows.patch"],
    };
    for (const [name, names] of Object.entries(bad)) assert.throws(() => orderPatches(names), /patch/, name);
  });
});

describe("the pin", () => {
  test("the real pin gives the build its source, and each triple its own url", async () => {
    const pin = parsePin(await readFile(new URL("../agent-browser.toml", import.meta.url), "utf8"));
    assert.match(pin.upstreamCommit, /^[0-9a-f]{40}$/);
    assert.match(pin.upstreamTarballSha256, /^[0-9a-f]{64}$/);
    assert.equal(TARGETS.length, 6);
    assert.deepEqual([...pin.triples.keys()].sort(), [...TARGETS].sort());
    for (const triple of TARGETS.filter((t) => !t.endsWith("-unknown-linux-gnu"))) {
      const lines = pinLines(triple, pin, "f".repeat(64));
      assert.equal(lines.split("\n")[0], `url = "${pin.triples.get(triple).url}"`, triple);
      assert.equal(lines.split("\n")[1], `sha256 = "${"f".repeat(64)}"`, triple);
    }
  });

  test("tag prints patched_tag, toolchain rust-toolchain.toml's channel", async () => {
    const lines = [];
    await main(["tag"], { log: (line) => lines.push(line) });
    await main(["toolchain"], { log: (line) => lines.push(line) });
    const channel = toolchainChannel(await readFile(new URL("../../../rust-toolchain.toml", import.meta.url), "utf8"));
    assert.deepEqual(lines, ["agent-browser-v0.41.1-clauseo.1", channel]);
  });

  test("toolchainChannel refuses a file with no channel", () => {
    assert.equal(toolchainChannel('[toolchain]\nchannel = "1.99.0"\nprofile = "minimal"\n'), "1.99.0");
    assert.throws(() => toolchainChannel('[toolchain]\nprofile = "minimal"\n'), /channel/);
  });
});

describe("checkSha and tarInfo", () => {
  test("checkSha refuses other bytes", () => {
    const bytes = Buffer.from("upstream");
    checkSha(bytes, sha(bytes), "x");
    assert.throws(() => checkSha(bytes, sha(Buffer.from("other")), "the source"), /the source: sha256 is .* refused/);
  });

  test("tarInfo reads the commit, the commit time and the symlinks", () => {
    const gz = archive(COMMIT, SOURCE, { symlink: ".claude" });
    assert.deepEqual(tarInfo(gz), {
      commit: COMMIT,
      mtime: MTIME,
      symlinks: [`agent-browser-${COMMIT}/.claude`],
    });
    assert.equal(tarInfo(archive(null, { "cli/Cargo.toml": CARGO })).commit, null);
  });
});

describe("buildEnv", () => {
  test("Windows: crt-static, /Brepro and the remaps, SOURCE_DATE_EPOCH and the toolchain; no RUSTFLAGS", () => {
    const env = buildEnv({
      triple: "aarch64-pc-windows-msvc",
      src: "C:\\t\\agent-browser-x",
      targetDir: "C:\\t\\target",
      epoch: MTIME,
      channel: "1.99.0",
      env: { RUSTFLAGS: "-C opt-level=0", PATH: "p" },
      home: "C:\\Users\\r",
    });
    assert.deepEqual(env.CARGO_ENCODED_RUSTFLAGS.split("\x1f"), [
      "-C",
      "target-feature=+crt-static",
      "-C",
      "link-arg=/Brepro",
      "--remap-path-prefix=C:\\t\\agent-browser-x=/agent-browser",
      `--remap-path-prefix=${join("C:\\Users\\r", ".cargo")}=/cargo`,
      "--remap-path-prefix=C:\\t\\target=/target",
    ]);
    assert.equal(env.RUSTFLAGS, undefined);
    assert.equal(env.CARGO_TARGET_DIR, "C:\\t\\target");
    assert.equal(env.SOURCE_DATE_EPOCH, String(MTIME));
    assert.equal(env.RUSTUP_TOOLCHAIN, "1.99.0");
    assert.equal(env.PATH, "p");
    const own = buildEnv({ triple: "x86_64-unknown-linux-gnu", src: "s", targetDir: "t", epoch: 1, channel: "c", env: { CARGO_HOME: "/ch" } });
    assert.ok(own.CARGO_ENCODED_RUSTFLAGS.split("\x1f").includes("--remap-path-prefix=/ch=/cargo"));
  });

  test("Mac and Linux: the remaps only, no MSVC flags", () => {
    for (const triple of TARGETS.filter((t) => !t.endsWith("-pc-windows-msvc"))) {
      const env = buildEnv({ triple, src: "/s", targetDir: "/t", epoch: 7, channel: "c", env: { CARGO_HOME: "/h" } });
      assert.deepEqual(
        env.CARGO_ENCODED_RUSTFLAGS.split("\x1f"),
        ["--remap-path-prefix=/s=/agent-browser", "--remap-path-prefix=/h=/cargo", "--remap-path-prefix=/t=/target"],
        triple,
      );
      assert.equal(env.SOURCE_DATE_EPOCH, "7", triple);
    }
  });

  test("pinLines: .exe on Windows only", async () => {
    const pin = parsePin(await readFile(new URL("../agent-browser.toml", import.meta.url), "utf8"));
    assert.match(pinLines("aarch64-apple-darwin", pin, "a".repeat(64)), /\/agent-browser-aarch64-apple-darwin"\n/);
    assert.match(pinLines("x86_64-pc-windows-msvc", pin, "a".repeat(64)), /\/agent-browser-x86_64-pc-windows-msvc\.exe"\n/);
  });
});

describe("check", () => {
  const quiet = () => {};

  test("applies the patches to the pinned archive, through tar and git", async () => {
    const gz = archive(COMMIT, SOURCE, { symlink: ".claude" });
    const { pinPath, patchDir } = await fixture(gz, { "0001-version.patch": PATCH });
    const server = serve(gz);
    const lines = [];
    await main(["check"], { pinPath, patchDir, fetch: server.fetch, log: (line) => lines.push(line) });
    assert.deepEqual(server.calls, [`https://github.com/vercel-labs/agent-browser/archive/${COMMIT}.tar.gz`]);
    assert.match(lines.at(-1), /^ok: 1 patch\(es\) apply cleanly/);
  });

  test("a patch that does not apply fails", async () => {
    const gz = archive(COMMIT, { "cli/Cargo.toml": CARGO.replace("0.41.1", "0.41.0") });
    const { pinPath, patchDir } = await fixture(gz, { "0001-version.patch": PATCH });
    await assert.rejects(main(["check"], { pinPath, patchDir, fetch: serve(gz).fetch, log: quiet }), /git .*apply/);
  });

  test("an archive whose sha256 is not the pin's is refused", async () => {
    const gz = archive(COMMIT, SOURCE);
    const { pinPath, patchDir } = await fixture(gz, { "0001-version.patch": PATCH });
    const other = archive(COMMIT, { "cli/Cargo.toml": `${CARGO}# changed\n` });
    await assert.rejects(main(["check"], { pinPath, patchDir, fetch: serve(other).fetch, log: quiet }), /refused/);
  });

  test("an archive of another commit is refused, even with the pinned sha256", async () => {
    const gz = archive("f".repeat(40), SOURCE);
    const { pinPath, patchDir } = await fixture(gz, { "0001-version.patch": PATCH });
    await assert.rejects(main(["check"], { pinPath, patchDir, fetch: serve(gz).fetch, log: quiet }), /names commit/);
  });

  test("a version patch that is not patched_tag's version is refused", async () => {
    const gz = archive(COMMIT, SOURCE);
    // A .2 rebuild whose version patch still says .1: the pin's tag is .2 here.
    const { pinPath, patchDir } = await fixture(gz, { "0001-version.patch": PATCH }, COMMIT, 2);
    await assert.rejects(
      main(["check"], { pinPath, patchDir, fetch: serve(gz).fetch, log: quiet }),
      /Cargo\.toml says version 0\.41\.1-clauseo\.1, but patched_tag agent-browser-v0\.41\.1-clauseo\.2 needs 0\.41\.1-clauseo\.2/,
    );
  });

  test("a misnamed patch fails before any download", async () => {
    const gz = archive(COMMIT, SOURCE);
    const { pinPath, patchDir } = await fixture(gz, { "windows.patch": PATCH });
    const server = serve(gz);
    await assert.rejects(main(["check"], { pinPath, patchDir, fetch: server.fetch, log: quiet }), /NNNN/);
    assert.equal(server.calls.length, 0);
  });
});

describe("testEnv", () => {
  const mac = (arch) => ({ platform: "darwin", arch });
  test("x86_64 macOS tested on an ARM64 Mac (Rosetta) gets 60 s per CLI subprocess test", () => {
    const env = testEnv({ PATH: "/bin" }, "x86_64-apple-darwin", mac("arm64"));
    assert.deepEqual(env, { PATH: "/bin", AB_TEST_SUBPROCESS_TIMEOUT_SECS: "60" });
  });

  test("a value already set is kept", () => {
    const env = { AB_TEST_SUBPROCESS_TIMEOUT_SECS: "120" };
    assert.equal(testEnv(env, "x86_64-apple-darwin", mac("arm64")).AB_TEST_SUBPROCESS_TIMEOUT_SECS, "120");
  });

  test("native builds keep the tests' own 10 s", () => {
    for (const [triple, host] of [
      ["x86_64-apple-darwin", mac("x64")],
      ["aarch64-apple-darwin", mac("arm64")],
      ["x86_64-pc-windows-msvc", { platform: "win32", arch: "x64" }],
      ["aarch64-pc-windows-msvc", { platform: "win32", arch: "arm64" }],
    ]) {
      const env = { PATH: "/bin" };
      assert.equal(testEnv(env, triple, host), env, triple);
    }
  });
});

describe("workParent", () => {
  test("macOS builds under /tmp, so upstream's Unix socket tests stay under 104 bytes", () => {
    assert.equal(workParent("darwin"), "/tmp");
    assert.equal(workParent("linux"), tmpdir());
    assert.equal(workParent("win32"), tmpdir());
    // The longest socket path upstream's tests make there: /private/tmp/ab-XXXXXX/s/cli/target/t/ab-XXXXXX/x.sock.
    assert.ok("/private/tmp/ab-XXXXXX/s/cli/target/t/ab-XXXXXX/x.sock".length < 104);
  });
});

describe("checkVersion", () => {
  const tag = "agent-browser-v0.41.1-clauseo.2";
  const toml = (v) => `[package]\nname = "agent-browser"\nversion = "${v}"\n\n[dependencies]\nversion = "9"\n`;
  const lock = (v) => LOCK.replace(/"0\.41\.1"$/m, `"${v}"`);

  test("passes when Cargo.toml and Cargo.lock both say patched_tag's version", () => {
    checkVersion(toml("0.41.1-clauseo.2"), lock("0.41.1-clauseo.2"), tag);
  });

  test("refuses either file at another version, or with none", () => {
    assert.throws(() => checkVersion(toml("0.41.1-clauseo.1"), lock("0.41.1-clauseo.2"), tag), /Cargo\.toml says version 0\.41\.1-clauseo\.1/);
    assert.throws(() => checkVersion(toml("0.41.1-clauseo.2"), lock("0.41.1-clauseo.1"), tag), /Cargo\.lock says version 0\.41\.1-clauseo\.1/);
    assert.throws(() => checkVersion(toml("0.41.1-clauseo.2"), "", tag), /Cargo\.lock says version null/);
    assert.throws(() => checkVersion("[dependencies]\nversion = \"0.41.1-clauseo.2\"\n", lock("0.41.1-clauseo.2"), tag), /Cargo\.toml says version null/);
  });

  test("the real patches give the real pin's version", async () => {
    const pin = parsePin(await readFile(new URL("../agent-browser.toml", import.meta.url), "utf8"));
    const patch = await readFile(new URL("../agent-browser/patches/0003-version-suffix.patch", import.meta.url), "utf8");
    const added = [...patch.matchAll(/^\+version = "([^"]*)"$/gm)].map((m) => m[1]);
    assert.deepEqual(added, Array(2).fill(pin.patchedTag.replace(/^agent-browser-v/, "")));
  });
});

describe("build's refusals", () => {
  test("checkCommitted refuses any git status line for the patches folder", () => {
    checkCommitted("", "patches");
    checkCommitted("\n", "patches");
    assert.throws(() => checkCommitted("?? patches/0003-x.patch\n", "patches"), /uncommitted/);
    assert.throws(() => checkCommitted(" M patches/0001-x.patch\n", "patches"), /0001-x/);
  });

  test("rustcCommit reads rustc -vV's commit-hash, and nothing else", () => {
    const hash = "b940084d7eb6a299eb4bfeb8e34901bc051e7ac4";
    const verbose = `rustc 1.99.0 (b940084d7 2026-10-01)\nbinary: rustc\ncommit-hash: ${hash}\nrelease: 1.99.0\n`;
    assert.equal(rustcCommit(verbose), hash);
    assert.equal(rustcCommit(verbose.replace(hash, "unknown")), null);
    assert.equal(rustcCommit("binary: rustc\n"), null);
  });

  test("the real pin's rustc_commit is a full commit", async () => {
    const pin = parsePin(await readFile(new URL("../agent-browser.toml", import.meta.url), "utf8"));
    assert.match(pin.rustcCommit, /^[0-9a-f]{40}$/);
  });
});
