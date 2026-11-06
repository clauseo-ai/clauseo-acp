// Tests for sidecars.mjs. Plain Node, no install, no network and no cargo: cargo is a stub that
// writes a fake binary where cargo would, and agent-browser's fetch is a stub answering a
// fixture pin's asset.
//
//   node --test .github/scripts/sidecars.test.mjs
//
// ci.yml's fast lane runs it beside latest-json.test.mjs.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { PREFIX, RELEASE_TARGETS, SIDECARS, overlayProblem, placeSidecars } from "./sidecars.mjs";

const scratch = await mkdtemp(join(tmpdir(), "sidecars-test-"));
after(() => rm(scratch, { recursive: true, force: true }));

const VERSION = "0.41.1";
const BODY = Buffer.from("#!fake agent-browser\n");
const SHA = createHash("sha256").update(BODY).digest("hex");
const ASSET = `https://github.com/vercel-labs/agent-browser/releases/download/v${VERSION}/agent-browser-fake`;

/** A pin with each triple an asset, except those in `none` (none = true) and `missing` (absent). */
async function pin(dir, { none = [], missing = [] } = {}) {
  let text = `version = "${VERSION}"\n`;
  for (const triple of RELEASE_TARGETS.filter((t) => !missing.includes(t))) {
    text += none.includes(triple) ? `\n[${triple}]\nnone = true\n` : `\n[${triple}]\nurl = "${ASSET}"\nsha256 = "${SHA}"\n`;
  }
  const path = join(dir, "agent-browser.toml");
  await writeFile(path, text);
  return path;
}

/** A fresh repo root, with a cargo stub (synchronous, as cargo runs) and a fetch stub that record their calls. */
async function setup(pinOptions) {
  const root = await mkdtemp(join(scratch, "root-"));
  const cargoCalls = [];
  const fetchCalls = [];
  const options = {
    root,
    targetDir: "target",
    cargo: (args) => {
      cargoCalls.push(args);
      const triple = args[args.indexOf("--target") + 1];
      const name = args.includes("--bin") ? args[args.indexOf("--bin") + 1] : args[args.indexOf("-p") + 1];
      const out = join(root, "target", triple, "release", `${name}${triple.includes("-windows-") ? ".exe" : ""}`);
      mkdirSync(dirname(out), { recursive: true });
      writeFileSync(out, `fake ${name}\n`);
    },
    agentBrowserOptions: {
      pinPath: await pin(root, pinOptions),
      fetch: async (url) => {
        fetchCalls.push(url);
        return new Response(BODY, { status: 200 });
      },
    },
  };
  const run = (triple) => placeSidecars(triple, options);
  const overlay = async () =>
    JSON.parse(await readFile(join(root, "crates", "desktop", "release", "sidecars.generated.json"), "utf8"));
  const binaries = async () => (await readdir(join(root, "crates", "desktop", "binaries"))).sort();
  return { root, run, overlay, binaries, cargoCalls, fetchCalls };
}

const WINDOWS = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];
const isWindows = (triple) => triple.includes("-windows-");
const SETUP_CARGO = (triple) =>
  ["build", "--release", "--locked", "-p", "clauseo-sandbox", "--bin", "clauseo-sandbox-setup", "--target", triple];

test("the table ships clauseo-mcp and clauseo-agent-browser on every release target, guard parked", () => {
  assert.deepEqual(
    SIDECARS.map((s) => [s.name, s.targets]),
    [
      ["clauseo-mcp", RELEASE_TARGETS],
      ["clauseo-agent-browser", RELEASE_TARGETS],
      ["clauseo-sandbox-setup", WINDOWS],
    ],
  );
});

test("the table lists clauseo-sandbox-setup on the Windows targets only, built from clauseo-sandbox", async () => {
  const setupRow = SIDECARS.find((s) => s.name === "clauseo-sandbox-setup");
  assert.deepEqual(setupRow.targets, RELEASE_TARGETS.filter(isWindows));
  for (const triple of RELEASE_TARGETS) assert.equal(setupRow.targets.includes(triple), isWindows(triple), triple);
  for (const triple of RELEASE_TARGETS) {
    const env = await setup();
    const externalBin = await env.run(triple);
    assert.equal(externalBin.includes("binaries/clauseo-sandbox-setup"), isWindows(triple), triple);
    const setupCalls = env.cargoCalls.filter((args) => args.includes("clauseo-sandbox"));
    assert.deepEqual(setupCalls, isWindows(triple) ? [SETUP_CARGO(triple)] : [], triple);
  }
});

// The .deb and .rpm install sidecars into /usr/bin, where a bare agent-browser would collide.
test("every overlay entry starts with binaries/clauseo-, on every target", async () => {
  assert.equal(PREFIX, "clauseo-");
  for (const triple of RELEASE_TARGETS) {
    const env = await setup();
    await env.run(triple);
    const { externalBin } = (await env.overlay()).bundle;
    assert.ok(externalBin.length > 0, triple);
    for (const entry of externalBin) assert.match(entry, /^binaries\/clauseo-/, `${triple}: ${entry}`);
    assert.equal(overlayProblem(await env.overlay()), null, triple);
    for (const file of await env.binaries()) assert.match(file, /^clauseo-/, `${triple}: ${file}`);
  }
});

test("a sidecar named without the clauseo- prefix fails before anything is placed", async () => {
  const env = await setup();
  const sidecars = [{ name: "agent-browser", targets: RELEASE_TARGETS, place: async () => null }];
  await assert.rejects(
    placeSidecars("x86_64-unknown-linux-gnu", { root: env.root, sidecars }),
    /agent-browser does not start with clauseo-/,
  );
  await assert.rejects(env.overlay(), { code: "ENOENT" });
});

for (const triple of RELEASE_TARGETS) {
  const win = isWindows(triple);
  test(`${triple}: the overlay lists every sidecar for the target, and the copies are named for it`, async () => {
    const env = await setup();
    const names = ["clauseo-mcp", "clauseo-agent-browser", ...(win ? ["clauseo-sandbox-setup"] : [])];
    const externalBin = names.map((name) => `binaries/${name}`);
    assert.deepEqual(await env.run(triple), externalBin);
    assert.deepEqual(await env.overlay(), { bundle: { externalBin } });
    const ext = win ? ".exe" : "";
    assert.deepEqual(await env.binaries(), names.map((name) => `${name}-${triple}${ext}`).sort());
    assert.deepEqual(env.cargoCalls, [
      ["build", "--release", "--locked", "-p", "clauseo-mcp", "--target", triple],
      ...(win ? [SETUP_CARGO(triple)] : []),
    ]);
    assert.deepEqual(env.fetchCalls, [ASSET]);
    const bin = join(env.root, "crates", "desktop", "binaries");
    assert.equal(await readFile(join(bin, `clauseo-mcp-${triple}${ext}`), "utf8"), "fake clauseo-mcp\n");
    assert.deepEqual(await readFile(join(bin, `clauseo-agent-browser-${triple}${ext}`)), BODY);
    if (win) {
      const copy = join(bin, `clauseo-sandbox-setup-${triple}.exe`);
      assert.equal(await readFile(copy, "utf8"), "fake clauseo-sandbox-setup\n");
    }
  });
}

test("a triple the pin marks none leaves agent-browser out of the overlay", async () => {
  const env = await setup({ none: ["aarch64-pc-windows-msvc"] });
  const externalBin = ["binaries/clauseo-mcp", "binaries/clauseo-sandbox-setup"];
  assert.deepEqual(await env.run("aarch64-pc-windows-msvc"), externalBin);
  assert.deepEqual(await env.overlay(), { bundle: { externalBin } });
  assert.deepEqual(await env.binaries(), [
    "clauseo-mcp-aarch64-pc-windows-msvc.exe",
    "clauseo-sandbox-setup-aarch64-pc-windows-msvc.exe",
  ]);
  assert.deepEqual(env.fetchCalls, []);
});

test("a triple missing from the pin fails, and leaves no overlay", async () => {
  const env = await setup({ missing: ["x86_64-apple-darwin"] });
  await assert.rejects(env.run("x86_64-apple-darwin"), /x86_64-apple-darwin is not in agent-browser\.toml/);
  await assert.rejects(env.overlay(), { code: "ENOENT" });
});

test("a triple that is not a release target fails before any build", async () => {
  const env = await setup();
  await assert.rejects(env.run("aarch64-unknown-linux-gnu"), /not a release target/);
  assert.deepEqual(env.cargoCalls, []);
});

test("a failed cargo build fails, and leaves no overlay, not even an earlier run's", async () => {
  const env = await setup();
  await env.run("x86_64-unknown-linux-gnu");
  await assert.rejects(
    placeSidecars("x86_64-unknown-linux-gnu", {
      root: env.root,
      cargo: () => {
        throw new Error("cargo build failed (exit 101)");
      },
    }),
    /exit 101/,
  );
  await assert.rejects(env.overlay(), { code: "ENOENT" });
});

test("this target's copies from an earlier run are removed first, other targets' kept", async () => {
  const env = await setup({ none: ["x86_64-unknown-linux-gnu"] });
  const bin = join(env.root, "crates", "desktop", "binaries");
  await mkdir(bin, { recursive: true });
  await writeFile(join(bin, "clauseo-agent-browser-x86_64-unknown-linux-gnu"), "old\n");
  await writeFile(join(bin, "clauseo-agent-browser-x86_64-unknown-linux-gnu.partial"), "a cut-off download\n");
  await writeFile(join(bin, "clauseo-agent-browser-aarch64-apple-darwin"), "another target's\n");
  await env.run("x86_64-unknown-linux-gnu");
  assert.deepEqual(await env.binaries(), [
    "clauseo-agent-browser-aarch64-apple-darwin",
    "clauseo-mcp-x86_64-unknown-linux-gnu",
  ]);
});

test("the overlay check refuses any other key, a non-list, and a name without the prefix", () => {
  const ok = { bundle: { externalBin: ["binaries/clauseo-mcp", "binaries/clauseo-agent-browser"] } };
  assert.equal(overlayProblem(ok), null);
  assert.equal(overlayProblem({ bundle: { externalBin: [] } }), null);
  for (const bad of [
    null,
    [],
    {},
    { bundle: { externalBin: [] }, identifier: "x" },
    { bundle: { externalBin: [], resources: [] } },
    { bundle: { externalBin: "binaries/clauseo-mcp" } },
  ]) {
    assert.equal(overlayProblem(bad), "it may hold only bundle.externalBin, a list", JSON.stringify(bad));
  }
  assert.match(overlayProblem({ bundle: { externalBin: ["binaries/agent-browser"] } }), /binaries\/clauseo-/);
  assert.match(overlayProblem({ bundle: { externalBin: ["../clauseo-mcp"] } }), /binaries\/clauseo-/);
});
