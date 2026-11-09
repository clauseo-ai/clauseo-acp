// Tests for sidecars.mjs. Plain Node, no install, no network and no cargo: cargo is a stub that
// writes a fake binary where cargo would, agent-browser's fetch is a stub answering a fixture
// pin's asset, and stetdoc's fetch is a stub writing a fake program (stetdoc.test.mjs tests the
// real one), except where a test runs the real one on a fixture pin.
//
//   node --test .github/scripts/sidecars.test.mjs
//
// ci.yml's fast lane runs it beside latest-json.test.mjs.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  MACOS_HELPER,
  PREFIX,
  RELEASE_TARGETS,
  SIDECARS,
  STETDOC,
  buildSetupExe,
  overlayProblem,
  placeSidecars,
  reproducibleEnv,
  safeWorkspaceOption,
} from "./sidecars.mjs";

const scratch = await mkdtemp(join(tmpdir(), "sidecars-test-"));
after(() => rm(scratch, { recursive: true, force: true }));

const VERSION = "0.41.1";
const BODY = Buffer.from("#!fake agent-browser\n");
const SHA = createHash("sha256").update(BODY).digest("hex");
const STETDOC_BODY = Buffer.from("#!fake stetdoc\n");
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

/**
 * A fresh repo root, with a cargo stub (synchronous, as cargo runs) and a fetch stub that record
 * their calls. `safeWorkspace` is release.yml's switch, off (left out) unless given.
 */
async function setup(pinOptions, { safeWorkspace } = {}) {
  const root = await mkdtemp(join(scratch, "root-"));
  const cargoCalls = [];
  const cargoEnvs = [];
  const fetchCalls = [];
  const stetdocCalls = [];
  const options = {
    root,
    targetDir: "target",
    safeWorkspace,
    commitTime: () => "1700000000",
    cargo: (args, extra) => {
      cargoCalls.push(args);
      cargoEnvs.push(extra);
      const triple = args[args.indexOf("--target") + 1];
      const name = args.includes("--bin") ? args[args.indexOf("--bin") + 1] : args[args.indexOf("-p") + 1];
      const out = join(root, "target", triple, "release", `${name}${triple.includes("-windows-") ? ".exe" : ""}`);
      mkdirSync(dirname(out), { recursive: true });
      writeFileSync(out, `fake ${name}\n`);
    },
    fetchStetdoc: async (triple, dest) => {
      stetdocCalls.push(triple);
      mkdirSync(dirname(dest), { recursive: true });
      writeFileSync(dest, STETDOC_BODY);
      return dest;
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
  return { root, run, overlay, binaries, cargoCalls, cargoEnvs, fetchCalls, stetdocCalls, options };
}

const WINDOWS = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"];
const isWindows = (triple) => triple.includes("-windows-");
const isDarwin = (triple) => triple.endsWith("-apple-darwin");
/** A darwin overlay's bundle.macOS: the helper at Contents/Helpers/clauseo-agent-browser. */
const HELPER = (triple) => ({ files: { "Helpers/clauseo-agent-browser": `binaries/clauseo-agent-browser-${triple}` } });
/** stetdoc's entry, in its own folder: Contents/Helpers/stetdoc on darwin, <resources>/stetdoc elsewhere. */
const STETDOC_MAC = (triple) => ({ "Helpers/stetdoc/stetdoc": `binaries/clauseo-stetdoc-${triple}` });
const STETDOC_RES = (triple) => {
  const ext = triple.includes("-windows-") ? ".exe" : "";
  return { [`binaries/clauseo-stetdoc-${triple}${ext}`]: `stetdoc/stetdoc${ext}` };
};
/** A darwin overlay's bundle.macOS with both helpers. */
const MAC_FILES = (triple) => ({ files: { ...HELPER(triple).files, ...STETDOC_MAC(triple) } });
const SETUP_CARGO = (triple) =>
  ["build", "--release", "--locked", "-p", "clauseo-sandbox", "--bin", "clauseo-sandbox-setup", "--target", triple];

test("the table ships clauseo-mcp and clauseo-agent-browser on every release target, guard parked", () => {
  assert.deepEqual(
    SIDECARS.map((s) => [s.name, s.targets, s.macosHelper === true, s.ownFolder === true]),
    [
      ["clauseo-mcp", RELEASE_TARGETS, false, false],
      ["clauseo-agent-browser", RELEASE_TARGETS, true, false],
      ["clauseo-sandbox-setup", WINDOWS, false, false],
      ["clauseo-stetdoc", RELEASE_TARGETS, false, true],
    ],
  );
  assert.equal(MACOS_HELPER, "clauseo-agent-browser");
  assert.equal(STETDOC, "clauseo-stetdoc");
  // Only the sandbox's set-up tool is the Safe workspace's.
  assert.deepEqual(
    SIDECARS.filter((s) => s.safeWorkspace).map((s) => s.name),
    ["clauseo-sandbox-setup"],
  );
});

test("with the Safe workspace on, clauseo-sandbox-setup ships on the Windows targets only, built from clauseo-sandbox", async () => {
  const setupRow = SIDECARS.find((s) => s.name === "clauseo-sandbox-setup");
  assert.deepEqual(setupRow.targets, RELEASE_TARGETS.filter(isWindows));
  for (const triple of RELEASE_TARGETS) assert.equal(setupRow.targets.includes(triple), isWindows(triple), triple);
  for (const triple of RELEASE_TARGETS) {
    const env = await setup(undefined, { safeWorkspace: true });
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
    assert.equal(overlayProblem(await env.overlay(), triple), null, triple);
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

test("with the Safe workspace off, the default, no target builds, places or lists a sandbox exe", async () => {
  for (const safeWorkspace of [undefined, false]) {
    for (const triple of RELEASE_TARGETS) {
      const env = await setup(undefined, { safeWorkspace });
      const mac = isDarwin(triple);
      // On macOS agent-browser goes to Contents/Helpers (bundle.macOS.files), not externalBin.
      const externalBin = mac ? ["binaries/clauseo-mcp"] : ["binaries/clauseo-mcp", "binaries/clauseo-agent-browser"];
      assert.deepEqual(await env.run(triple), externalBin, triple);
      assert.deepEqual(
        await env.overlay(),
        { bundle: mac ? { externalBin, macOS: MAC_FILES(triple) } : { externalBin, resources: STETDOC_RES(triple) } },
        triple,
      );
      assert.equal(overlayProblem(await env.overlay(), triple), null, triple);
      for (const file of await env.binaries()) assert.doesNotMatch(file, /sandbox/, `${triple}: ${file}`);
      assert.deepEqual(env.cargoCalls.filter((args) => args.includes("clauseo-sandbox")), [], triple);
    }
  }
});

test("the switch's option is exactly --safe-workspace=true or =false, and the CLI refuses any other", () => {
  assert.equal(safeWorkspaceOption("--safe-workspace=true"), true);
  assert.equal(safeWorkspaceOption("--safe-workspace=false"), false);
  for (const bad of ["--safe-workspace", "--safe-workspace=", "--safe-workspace=1", "--safe-workspace=True"]) {
    assert.equal(safeWorkspaceOption(bad), null, bad);
  }
  const script = join(dirname(fileURLToPath(import.meta.url)), "sidecars.mjs");
  for (const args of [
    ["--safe-workspace=yes", "x86_64-pc-windows-msvc"],
    ["--safe-workspace=true"],
    ["--safe-workspace=true", "--check"],
    ["--safe-workspace=true", "--setup-exe", "x86_64-pc-windows-msvc"],
  ]) {
    const result = spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
    assert.equal(result.status, 2, `${args.join(" ")}: ${result.stderr}`);
    assert.match(result.stderr, /^usage: /, args.join(" "));
  }
});

for (const triple of RELEASE_TARGETS) {
  const win = isWindows(triple);
  const mac = isDarwin(triple);
  test(`${triple}: with the Safe workspace on, the overlay lists every sidecar for the target, and the copies are named for it`, async () => {
    const env = await setup(undefined, { safeWorkspace: true });
    const names = ["clauseo-mcp", "clauseo-agent-browser", ...(win ? ["clauseo-sandbox-setup"] : []), "clauseo-stetdoc"];
    // On macOS agent-browser goes to Contents/Helpers (bundle.macOS.files), not externalBin;
    // stetdoc is never in externalBin, but in a folder of its own.
    const externalBin = names
      .filter((name) => name !== "clauseo-stetdoc" && !(mac && name === "clauseo-agent-browser"))
      .map((name) => `binaries/${name}`);
    assert.deepEqual(await env.run(triple), externalBin);
    assert.deepEqual(
      await env.overlay(),
      { bundle: mac ? { externalBin, macOS: MAC_FILES(triple) } : { externalBin, resources: STETDOC_RES(triple) } },
    );
    assert.equal(overlayProblem(await env.overlay(), triple), null);
    const ext = win ? ".exe" : "";
    assert.deepEqual(await env.binaries(), names.map((name) => `${name}-${triple}${ext}`).sort());
    assert.deepEqual(env.cargoCalls, [
      ["build", "--release", "--locked", "-p", "clauseo-mcp", "--target", triple],
      ...(win ? [SETUP_CARGO(triple)] : []),
    ]);
    assert.deepEqual(env.fetchCalls, [ASSET]);
    assert.deepEqual(env.stetdocCalls, [triple]);
    const bin = join(env.root, "crates", "desktop", "binaries");
    assert.equal(await readFile(join(bin, `clauseo-mcp-${triple}${ext}`), "utf8"), "fake clauseo-mcp\n");
    assert.deepEqual(await readFile(join(bin, `clauseo-agent-browser-${triple}${ext}`)), BODY);
    assert.deepEqual(await readFile(join(bin, `clauseo-stetdoc-${triple}${ext}`)), STETDOC_BODY);
    if (win) {
      const copy = join(bin, `clauseo-sandbox-setup-${triple}.exe`);
      assert.equal(await readFile(copy, "utf8"), "fake clauseo-sandbox-setup\n");
    }
  });
}

test("a triple the pin marks none leaves agent-browser out of the overlay", async () => {
  const env = await setup({ none: ["aarch64-pc-windows-msvc"] }, { safeWorkspace: true });
  const externalBin = ["binaries/clauseo-mcp", "binaries/clauseo-sandbox-setup"];
  assert.deepEqual(await env.run("aarch64-pc-windows-msvc"), externalBin);
  assert.deepEqual(await env.overlay(), { bundle: { externalBin, resources: STETDOC_RES("aarch64-pc-windows-msvc") } });
  assert.deepEqual(await env.binaries(), [
    "clauseo-mcp-aarch64-pc-windows-msvc.exe",
    "clauseo-sandbox-setup-aarch64-pc-windows-msvc.exe",
    "clauseo-stetdoc-aarch64-pc-windows-msvc.exe",
  ]);
  assert.deepEqual(env.fetchCalls, []);
});

test("a darwin triple the pin marks none has only stetdoc in bundle.macOS, and the check allows that", async () => {
  const env = await setup({ none: ["aarch64-apple-darwin"] });
  assert.deepEqual(await env.run("aarch64-apple-darwin"), ["binaries/clauseo-mcp"]);
  assert.deepEqual(await env.overlay(), {
    bundle: { externalBin: ["binaries/clauseo-mcp"], macOS: { files: STETDOC_MAC("aarch64-apple-darwin") } },
  });
  assert.equal(overlayProblem(await env.overlay(), "aarch64-apple-darwin"), null);
});

test("only clauseo-agent-browser may be a macOS helper", async () => {
  const env = await setup();
  const sidecars = [{ name: "clauseo-mcp", targets: RELEASE_TARGETS, macosHelper: true, place: async () => null }];
  await assert.rejects(placeSidecars("aarch64-apple-darwin", { root: env.root, sidecars }), /only clauseo-agent-browser/);
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
    "clauseo-stetdoc-x86_64-unknown-linux-gnu",
  ]);
});

test("the overlay check refuses any other key, a non-list, and a name without the prefix", () => {
  const ok = { bundle: { externalBin: ["binaries/clauseo-mcp", "binaries/clauseo-agent-browser"] } };
  const linux = "x86_64-unknown-linux-gnu";
  assert.equal(overlayProblem(ok, linux), null);
  assert.equal(overlayProblem({ bundle: { externalBin: [] } }, linux), null);
  assert.match(overlayProblem(ok, "riscv64gc-unknown-linux-gnu"), /not a release target/);
  assert.match(overlayProblem(ok), /not a release target/);
  for (const bad of [
    null,
    [],
    {},
    { bundle: { externalBin: [] }, identifier: "x" },
    { bundle: { externalBin: [], macOS: {} } },
    { bundle: { externalBin: "binaries/clauseo-mcp" } },
  ]) {
    assert.equal(overlayProblem(bad, linux), "it may hold only bundle.externalBin, a list, and bundle.resources", JSON.stringify(bad));
  }
  assert.match(overlayProblem({ bundle: { externalBin: ["binaries/agent-browser"] } }, linux), /binaries\/clauseo-/);
  assert.match(overlayProblem({ bundle: { externalBin: ["../clauseo-mcp"] } }, linux), /binaries\/clauseo-/);
});

test("the overlay check allows bundle.macOS.files on darwin only, holding exactly the helper", () => {
  const externalBin = ["binaries/clauseo-mcp"];
  for (const triple of RELEASE_TARGETS) {
    const withHelper = { bundle: { externalBin, macOS: HELPER(triple) } };
    if (isDarwin(triple)) {
      assert.equal(overlayProblem(withHelper, triple), null, triple);
    } else {
      assert.equal(overlayProblem(withHelper, triple), "it may hold only bundle.externalBin, a list, and bundle.resources", triple);
    }
  }
  const mac = "aarch64-apple-darwin";
  const shape = "it may hold only bundle.externalBin, a list, and bundle.macOS.files";
  assert.equal(overlayProblem({ bundle: { macOS: HELPER(mac) } }, mac), shape);
  assert.equal(overlayProblem({ bundle: { externalBin, macOS: HELPER(mac), resources: [] } }, mac), shape);
  assert.equal(overlayProblem({ bundle: { externalBin, resources: STETDOC_RES(mac) } }, mac), shape);
  assert.equal(overlayProblem({ bundle: { externalBin, macOS: MAC_FILES(mac) } }, mac), null);
  assert.equal(overlayProblem({ bundle: { externalBin, macOS: { files: STETDOC_MAC(mac) } } }, mac), null);
  assert.equal(overlayProblem({ bundle: { externalBin: "binaries/clauseo-mcp", macOS: HELPER(mac) } }, mac), shape);
  for (const macOS of [
    HELPER("x86_64-apple-darwin"),
    { files: {} },
    {},
    { files: { "MacOS/clauseo-agent-browser": "binaries/clauseo-agent-browser-aarch64-apple-darwin" } },
    { files: { "Resources/clauseo-agent-browser": "binaries/clauseo-agent-browser-aarch64-apple-darwin" } },
    { files: { "Helpers/clauseo-agent-browser": "../clauseo-agent-browser-aarch64-apple-darwin" } },
    { files: { "Helpers/clauseo-mcp": "binaries/clauseo-mcp-aarch64-apple-darwin" } },
    { ...HELPER(mac), minimumSystemVersion: "10.0" },
    { files: { ...HELPER(mac).files, "Helpers/other": "binaries/clauseo-mcp-aarch64-apple-darwin" } },
  ]) {
    assert.match(overlayProblem({ bundle: { externalBin, macOS } }, mac), /bundle\.macOS may hold only/, JSON.stringify(macOS));
  }
  const both = { bundle: { externalBin: [...externalBin, "binaries/clauseo-agent-browser"], macOS: HELPER(mac) } };
  assert.match(overlayProblem(both, mac), /may not be in externalBin/);
  assert.match(overlayProblem({ bundle: { externalBin: ["binaries/x"], macOS: HELPER(mac) } }, mac), /binaries\/clauseo-/);
});

test("the overlay check allows bundle.resources off darwin only, holding exactly stetdoc's folder", () => {
  const externalBin = ["binaries/clauseo-mcp"];
  for (const triple of RELEASE_TARGETS.filter((t) => !isDarwin(t))) {
    assert.equal(overlayProblem({ bundle: { externalBin, resources: STETDOC_RES(triple) } }, triple), null, triple);
    const ext = isWindows(triple) ? ".exe" : "";
    for (const resources of [
      {},
      [],
      STETDOC_RES(triple === "x86_64-unknown-linux-gnu" ? "x86_64-pc-windows-msvc" : "x86_64-unknown-linux-gnu"),
      { [`binaries/clauseo-stetdoc-${triple}${ext}`]: `stetdoc${ext}` },
      { [`binaries/clauseo-stetdoc-${triple}${ext}`]: `../stetdoc/stetdoc${ext}` },
      { [`binaries/clauseo-mcp-${triple}${ext}`]: `stetdoc/stetdoc${ext}` },
      { ...STETDOC_RES(triple), "blocklist/x": "blocklist/x" },
    ]) {
      assert.match(
        overlayProblem({ bundle: { externalBin, resources } }, triple) ?? "",
        /bundle\.resources may hold only|it may hold only/,
        `${triple} ${JSON.stringify(resources)}`,
      );
    }
    const both = { bundle: { externalBin: [...externalBin, "binaries/clauseo-stetdoc"], resources: STETDOC_RES(triple) } };
    assert.match(overlayProblem(both, triple), /clauseo-stetdoc is in bundle.resources, so it may not be in externalBin/);
  }
});

test("only clauseo-stetdoc may have a folder of its own", async () => {
  const env = await setup();
  const sidecars = [{ name: "clauseo-mcp", targets: RELEASE_TARGETS, ownFolder: true, place: async () => null }];
  await assert.rejects(placeSidecars("x86_64-unknown-linux-gnu", { root: env.root, sidecars }), /only clauseo-stetdoc/);
});

// The real stetdoc fetch on a fixture pin: a pending target fails the seam, loudly, and leaves no overlay.
test("a target stetdoc's pin marks pending fails the seam and leaves no overlay", async () => {
  const env = await setup();
  const pinPath = join(env.root, "stetdoc.toml");
  await writeFile(pinPath, 'version = "9.9.9"\n\n[x86_64-pc-windows-msvc]\npending = "PENDING_TEST: not yet"\n');
  const { fetchStetdoc: _stub, ...options } = env.options;
  await assert.rejects(
    placeSidecars("x86_64-pc-windows-msvc", { ...options, stetdocOptions: { pinPath, fetch: async () => assert.fail("no download") } }),
    /x86_64-pc-windows-msvc is PENDING in crates\/desktop\/stetdoc.toml/,
  );
  await assert.rejects(env.overlay(), { code: "ENOENT" });
});

test("only the setup exe's build gets the path remaps, after the job's rustflags", async () => {
  const env = await setup(undefined, { safeWorkspace: true });
  await env.run("x86_64-pc-windows-msvc");
  const at = env.cargoCalls.findIndex((args) => args.includes("clauseo-sandbox"));
  const flags = env.cargoEnvs[at].CARGO_ENCODED_RUSTFLAGS.split("\x1f");
  assert.deepEqual(flags.slice(-3).map((flag) => flag.split("=").at(-1)), ["/clauseo", "/cargo", "/target"]);
  assert.ok(flags.includes(`--remap-path-prefix=${join(env.root, "target")}=/target`), flags.join(" "));
  assert.equal(env.cargoEnvs[at].SOURCE_DATE_EPOCH, "1700000000");
  const others = env.cargoEnvs.filter((_, n) => n !== at);
  assert.deepEqual(others, others.map(() => undefined));
});

test("the remaps keep the job's rustflags, encoded or not, the target dir's last; the commit's time", () => {
  const remaps = [
    "--remap-path-prefix=/r=/clauseo",
    "--remap-path-prefix=/h=/cargo",
    "--remap-path-prefix=/r/t=/target",
  ];
  const where = (env) => ({ root: "/r", targetDir: "/r/t", commitTime: "17", env });
  const flags = (env) => reproducibleEnv(where(env)).CARGO_ENCODED_RUSTFLAGS.split("\x1f");
  assert.equal(reproducibleEnv(where({})).SOURCE_DATE_EPOCH, "17");
  assert.equal(reproducibleEnv(where({ SOURCE_DATE_EPOCH: "9" })).SOURCE_DATE_EPOCH, "9");
  assert.deepEqual(flags({ CARGO_HOME: "/h", RUSTFLAGS: " -C  force-frame-pointers=yes " }), [
    "-C",
    "force-frame-pointers=yes",
    ...remaps,
  ]);
  assert.deepEqual(flags({ CARGO_HOME: "/h", RUSTFLAGS: "-C x", CARGO_ENCODED_RUSTFLAGS: "-C\x1fy=a b" }), [
    "-C",
    "y=a b",
    ...remaps,
  ]);
  assert.deepEqual(flags({ CARGO_HOME: "/h" }), remaps);
});

test("--setup-exe's build places nothing, and refuses a target the exe doesn't ship on", async () => {
  const env = await setup();
  const path = await buildSetupExe("aarch64-pc-windows-msvc", env.options);
  assert.equal(path, join(env.root, "target", "aarch64-pc-windows-msvc", "release", "clauseo-sandbox-setup.exe"));
  assert.deepEqual(env.cargoCalls, [SETUP_CARGO("aarch64-pc-windows-msvc")]);
  await assert.rejects(readdir(join(env.root, "crates")), { code: "ENOENT" });
  await assert.rejects(buildSetupExe("x86_64-unknown-linux-gnu", env.options), /Windows release targets only/);
});
