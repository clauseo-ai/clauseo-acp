// sidecars.mjs: every sidecar a release ships, built or fetched for one target, and the
// release-only overlay that bundles them (ADR 0035, "Sidecars").
//
//   node .github/scripts/sidecars.mjs [--safe-workspace=true|false] <target-triple>
//   node .github/scripts/sidecars.mjs --check <target-triple>   (the overlay holds only its allowed shape)
//   node .github/scripts/sidecars.mjs --setup-exe <target-triple>   (that exe only; prints its path)
//
// release.yml's sidecar seam runs it in each `build` leg, after the installs and before any
// secret. For the target it:
//   - builds or fetches each sidecar in SIDECARS below, and copies it to
//     crates/desktop/binaries/<name>-<triple> (.exe on Windows), Tauri's externalBin naming.
//     Every name starts with clauseo-: the .deb and .rpm install sidecars into /usr/bin, beside
//     other programs (agent-browser itself, from a global npm install, for one);
//   - writes crates/desktop/release/sidecars.generated.json, holding only
//     {"bundle":{"externalBin":[...]}} for the sidecars it placed, and on a darwin target
//     {"macOS":{"files":{"Helpers/clauseo-agent-browser": ...}}} in bundle beside it: the helper
//     lives in Clauseo.app/Contents/Helpers, Apple's folder for nested helper code, not in
//     Contents/MacOS beside the app (which the agent's sandbox can reach). Tauri copies
//     macOS.files without signing them, so release.yml signs the helper before `tauri bundle`;
//   - places stetdoc in a folder of its own (STETDOC below; the private task row U13), so that
//     only that folder goes first on the agents' PATH: on a darwin target in bundle.macOS.files
//     as Helpers/stetdoc/stetdoc (signed with the browser helper), elsewhere in bundle.resources
//     as stetdoc/stetdoc[.exe] (on Windows in the install folder, which is the resource folder,
//     and signed with the other files of binaries/). The pin is crates/desktop/stetdoc.toml; a
//     target it marks pending fails here, loudly.
// The legs pass that file last with --config to every `tauri build` and `tauri bundle`. A JSON
// merge patch replaces arrays, so this one list is the target's whole externalBin. Both outputs
// are gitignored. The base tauri.conf.json has no externalBin: tauri-build checks that every
// listed file exists, which would break every dev, CI and e2e build.
//
// The Safe workspace is off for launch (ADR 0035, "Safe workspace off for launch"): its sidecars
// (the elevated set-up tool, clauseo-sandbox-setup) carry `safeWorkspace: true` and are built
// and listed only with --safe-workspace=true, which release.yml passes from its SAFE_WORKSPACE
// switch. Without it (the default) no sandbox exe is built, placed or bundled. --setup-exe builds
// the exe on demand either way (setup-exe-repro.yml).
//
// Cargo runs with the job's own environment (RUSTFLAGS and the CARGO_PROFILE_RELEASE_* flags),
// so a sidecar gets the same release flags as the app, and its .pdb, .dSYM or .dwp lands in
// target/<triple>/release beside the app's. The setup exe adds only path remaps to those flags,
// and SOURCE_DATE_EPOCH, for a reproducible build (reproducibleEnv); it has no .pdb
// (crates/sandbox/build.rs).
//
// Node built-ins only. Tests: sidecars.test.mjs next to this file (ci.yml's fast lane).

import { spawnSync } from "node:child_process";
import { chmod, copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { fetchAgentBrowser } from "../../crates/desktop/sidecars/agent-browser.mjs";
import { fetchStetdoc } from "../../crates/desktop/sidecars/stetdoc.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** release.yml's build matrix. */
export const RELEASE_TARGETS = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
];

const isWindows = (triple) => triple.includes("-windows-");
const isDarwin = (triple) => triple.endsWith("-apple-darwin");
const exe = (triple) => (isWindows(triple) ? ".exe" : "");

/**
 * Builds a workspace package's binary for the target and returns where cargo left it.
 * @param {string} pkg the cargo package, whose binary has the same name
 */
const cargoBin = (pkg) => async (triple, ctx) => {
  ctx.cargo(["build", "--release", "--locked", "-p", pkg, "--target", triple]);
  return join(ctx.targetDir, triple, "release", `${pkg}${exe(triple)}`);
};

/**
 * Builds one named [[bin]] of a workspace package for the target and returns where cargo left it.
 * @param {string} pkg the cargo package
 * @param {string} bin the binary, named in the package's Cargo.toml
 */
const cargoPackageBin = (pkg, bin) => async (triple, ctx) => {
  ctx.cargo(["build", "--release", "--locked", "-p", pkg, "--bin", bin, "--target", triple]);
  return join(ctx.targetDir, triple, "release", `${bin}${exe(triple)}`);
};

/**
 * The extra environment of a reproducible build: the job's rustflags (CARGO_ENCODED_RUSTFLAGS,
 * else RUSTFLAGS) and --remap-path-prefix for the repo root, cargo's home and the target dir, so
 * the target's crates carry no absolute path from the runner (a dependency's panic location, an
 * OUT_DIR file). Under --target, rustflags never reach the host's proc macros and build scripts,
 * so the exe is reproducible at one build path only (crates/sandbox/build.rs). rustc applies the last prefix that matches, so the target dir,
 * inside the root, comes after it. CARGO_ENCODED_RUSTFLAGS (0x1f between flags) keeps a path
 * with spaces whole. Cargo hashes rustflags into its artifacts' names, so this build sits beside
 * the app's, not over it.
 * SOURCE_DATE_EPOCH is the commit's time (the job's own, when it sets one), for any build
 * script or tool that would stamp a date. The linker's half (/Brepro, /DEBUG:NONE) is in
 * crates/sandbox/build.rs; the toolchain is rust-toolchain.toml's, which rustup and cargo honour
 * in the repo.
 *
 * @param {{ root: string, targetDir: string, commitTime: string, env?: NodeJS.ProcessEnv }} where
 * @returns {{ CARGO_ENCODED_RUSTFLAGS: string, SOURCE_DATE_EPOCH: string }}
 */
export function reproducibleEnv({ root, targetDir, commitTime, env = process.env }) {
  const inherited = env.CARGO_ENCODED_RUSTFLAGS
    ? env.CARGO_ENCODED_RUSTFLAGS.split("\x1f")
    : (env.RUSTFLAGS ?? "").split(/\s+/).filter((flag) => flag !== "");
  const cargoHome = env.CARGO_HOME ?? join(homedir(), ".cargo");
  const remaps = [
    [root, "/clauseo"],
    [cargoHome, "/cargo"],
    [targetDir, "/target"],
  ].map(([from, to]) => `--remap-path-prefix=${from}=${to}`);
  return {
    CARGO_ENCODED_RUSTFLAGS: [...inherited, ...remaps].join("\x1f"),
    SOURCE_DATE_EPOCH: env.SOURCE_DATE_EPOCH ?? commitTime,
  };
}

/** HEAD's commit time in Unix seconds, from git in `root`; throws when git fails. */
function gitCommitTime(root) {
  const result = spawnSync("git", ["log", "-1", "--format=%ct"], { cwd: root, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`git log failed in ${root}: ${result.stderr.trim()}`);
  return result.stdout.trim();
}

/**
 * The elevated setup exe: built as cargoPackageBin builds it, reproducibly (reproducibleEnv).
 * setup-exe-repro.yml builds it twice from clean and compares the two.
 */
const setupExe = async (triple, ctx) => {
  const bin = "clauseo-sandbox-setup";
  const args = ["build", "--release", "--locked", "-p", "clauseo-sandbox", "--bin", bin, "--target", triple];
  const commitTime = (ctx.commitTime ?? gitCommitTime)(ctx.root);
  ctx.cargo(args, reproducibleEnv({ root: ctx.root, targetDir: ctx.targetDir, commitTime }));
  return join(ctx.targetDir, triple, "release", `${bin}${exe(triple)}`);
};

/**
 * Fetches the pinned agent-browser straight to its destination; null when the pin says none.
 * The pin and the download keep upstream's asset names; only the installed name has the prefix.
 */
const pinnedAgentBrowser = async (triple, ctx, dest) =>
  ctx.fetchAgentBrowser(triple, dest, ctx.agentBrowserOptions);

/**
 * Fetches the pinned stetdoc straight to its destination (crates/desktop/stetdoc.toml); throws on
 * a triple the pin marks pending, so no release of it builds without stetdoc.
 */
const pinnedStetdoc = async (triple, ctx, dest) => ctx.fetchStetdoc(triple, dest, ctx.stetdocOptions);

/**
 * Every sidecar, in bundle order. `targets` is the triples it ships on; `macosHelper`, that on a
 * darwin target it goes to Contents/Helpers/<name> (bundle.macOS.files) instead of
 * bundle.externalBin (Contents/MacOS). `place` returns the
 * path of the built binary (copied to the destination), the destination itself (already
 * written), or null (no build for this target: left out of the list, not an error). A throw
 * fails the job.
 */
export const SIDECARS = [
  { name: "clauseo-mcp", targets: RELEASE_TARGETS, place: cargoBin("clauseo-mcp") },
  // Throws on a triple missing from crates/desktop/agent-browser.toml; null for `none = true`.
  // On macOS in Contents/Helpers, out of the agent's reach (MACOS_HELPER).
  { name: "clauseo-agent-browser", targets: RELEASE_TARGETS, macosHelper: true, place: pinnedAgentBrowser },
  // Parked (host thread, 2026-11-03): the guard's start time was not the cost. If it returns:
  // { name: "clauseo-guard", targets: RELEASE_TARGETS, place: cargoBin("clauseo-guard") },
  // which gets -C target-feature=+crt-static on Windows from the job's flags, as every row.
  // S15's set-up tool, a [[bin]] of the clauseo-sandbox package, only with the Safe workspace on
  // (safeWorkspace). Tauri bundles it beside Clauseo.exe as clauseo-sandbox-setup.exe;
  // hooks.nsh (CLAUSEO_SAFE_WORKSPACE) writes it to Program Files and runs it, and the NSIS
  // template deletes it on uninstall.
  {
    name: "clauseo-sandbox-setup",
    targets: RELEASE_TARGETS.filter(isWindows),
    place: setupExe,
    safeWorkspace: true,
  },
  // clauseo-sandbox-run comes later (S15 PR5), Windows targets only, with safeWorkspace: true.
  // stetdoc, in its own folder (STETDOC): never in externalBin, so never beside the app.
  { name: "clauseo-stetdoc", targets: RELEASE_TARGETS, ownFolder: true, place: pinnedStetdoc },
];

/** Every sidecar's name starts with this (ADR 0035, "Sidecars"). */
export const PREFIX = "clauseo-";

/**
 * The one sidecar a darwin overlay puts in bundle.macOS.files, at Contents/Helpers/<name>, beside
 * stetdoc's own folder (STETDOC). A second helper means changing this, overlayProblem and
 * release.yml's .app checks together.
 */
export const MACOS_HELPER = "clauseo-agent-browser";

/**
 * stetdoc, staged as binaries/clauseo-stetdoc-<triple>[.exe] and bundled in a folder of its own:
 * Contents/Helpers/stetdoc/stetdoc on macOS (bundle.macOS.files), <resources>/stetdoc/stetdoc
 * elsewhere (bundle.resources; on Windows the resource folder is the install folder).
 * crates/desktop/src/profile.rs looks for it at the same paths.
 */
export const STETDOC = "clauseo-stetdoc";
const stetdocHelper = "Helpers/stetdoc/stetdoc";
const stetdocResource = (triple) => `stetdoc/stetdoc${exe(triple)}`;
const staged = (name, triple) => `binaries/${name}-${triple}${exe(triple)}`;

/** The bundle.macOS.files entries a darwin overlay may hold for `triple`. */
const macosFiles = (triple) => ({
  [`Helpers/${MACOS_HELPER}`]: staged(MACOS_HELPER, triple),
  [stetdocHelper]: staged(STETDOC, triple),
});

/** The bundle.resources entries any other overlay may hold for `triple`. */
const resourceFiles = (triple) => ({ [staged(STETDOC, triple)]: stetdocResource(triple) });

const OVERLAY = join(ROOT, "crates", "desktop", "release", "sidecars.generated.json");

/**
 * Why a parsed overlay is not one this script writes for `triple`, or null when it is: only
 * bundle.externalBin, a list of binaries/clauseo-* strings, and, optionally, on a darwin target
 * bundle.macOS.files, elsewhere bundle.resources, each a non-empty object whose every entry is
 * one this script writes for the triple (macosFiles, resourceFiles); a sidecar there is then
 * absent from externalBin. release.yml checks it with --check after the seam and again in
 * sign-windows, before any bundle reads it.
 */
export function overlayProblem(overlay, triple) {
  if (!RELEASE_TARGETS.includes(triple)) return `${triple} is not a release target`;
  const keys = (v) => (v !== null && typeof v === "object" && !Array.isArray(v) ? Object.keys(v).sort().join(" ") : null);
  const extra = isDarwin(triple) ? "macOS" : "resources";
  const shape = keys(overlay?.bundle);
  const withExtra = shape === `externalBin ${extra}`;
  if (keys(overlay) !== "bundle" || (shape !== "externalBin" && !withExtra) || !Array.isArray(overlay.bundle.externalBin)) {
    return isDarwin(triple)
      ? "it may hold only bundle.externalBin, a list, and bundle.macOS.files"
      : "it may hold only bundle.externalBin, a list, and bundle.resources";
  }
  const bad = overlay.bundle.externalBin.filter((entry) => typeof entry !== "string" || !entry.startsWith(`binaries/${PREFIX}`));
  if (bad.length > 0) return `each entry must start with binaries/${PREFIX}: ${JSON.stringify(bad)}`;
  if (!withExtra) return null;
  const allowed = isDarwin(triple) ? macosFiles(triple) : resourceFiles(triple);
  const held = isDarwin(triple) ? overlay.bundle.macOS : overlay.bundle.resources;
  const entries = isDarwin(triple) ? (keys(held) === "files" ? held.files : null) : held;
  const what = isDarwin(triple) ? "bundle.macOS" : "bundle.resources";
  const ok =
    keys(entries) !== null &&
    Object.keys(entries).length > 0 &&
    Object.entries(entries).every(([key, value]) => Object.hasOwn(allowed, key) && allowed[key] === value);
  if (!ok) return `${what} may hold only entries of ${JSON.stringify(allowed)}: ${JSON.stringify(held)}`;
  const paths = new Set([...Object.keys(entries), ...Object.values(entries)]);
  const moved = [MACOS_HELPER, STETDOC].filter(
    (name) => paths.has(staged(name, triple)) && overlay.bundle.externalBin.includes(`binaries/${name}`),
  );
  if (moved.length > 0) return `binaries/${moved[0]} is in ${what}, so it may not be in externalBin`;
  return null;
}

/** Runs cargo in the repo root with the inherited environment and `extra`; throws on failure. */
function runCargo(args, extra = {}) {
  console.log(`$ cargo ${args.join(" ")}`);
  const result = spawnSync("cargo", args, { cwd: ROOT, stdio: "inherit", env: { ...process.env, ...extra } });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`cargo ${args.join(" ")} failed (exit ${result.status})`);
}

/**
 * Places every sidecar for `triple` and writes the overlay. Returns the overlay's externalBin
 * (without a darwin target's helper, which is in bundle.macOS.files, or stetdoc, in its own folder).
 * The Safe workspace's sidecars (`safeWorkspace: true`) only with `safeWorkspace` true.
 *
 * @param {string} triple a target in RELEASE_TARGETS
 * @param {{ root?: string, cargo?: (args: string[], extra?: object) => void, targetDir?: string,
 *   fetchAgentBrowser?: typeof fetchAgentBrowser, agentBrowserOptions?: object,
 *   fetchStetdoc?: typeof fetchStetdoc, stetdocOptions?: object,
 *   sidecars?: typeof SIDECARS, commitTime?: (root: string) => string,
 *   safeWorkspace?: boolean }} [options] the tests replace cargo, the fetch, the root and git's
 *   commit time; safeWorkspace is release.yml's SAFE_WORKSPACE switch, off when left out
 * @returns {Promise<string[]>}
 */
export async function placeSidecars(triple, options = {}) {
  if (!RELEASE_TARGETS.includes(triple)) {
    throw new Error(`sidecars: ${triple} is not a release target (${RELEASE_TARGETS.join(", ")})`);
  }
  const sidecars = options.sidecars ?? SIDECARS;
  for (const { name, macosHelper } of sidecars) {
    if (!name.startsWith(PREFIX)) throw new Error(`sidecars: ${name} does not start with ${PREFIX}`);
    if (macosHelper && name !== MACOS_HELPER) throw new Error(`sidecars: only ${MACOS_HELPER} may be a macOS helper, not ${name}`);
  }
  for (const { name, ownFolder } of sidecars) {
    if (ownFolder && name !== STETDOC) throw new Error(`sidecars: only ${STETDOC} has a folder of its own, not ${name}`);
  }
  const root = options.root ?? ROOT;
  const ctx = {
    root,
    commitTime: options.commitTime,
    cargo: options.cargo ?? runCargo,
    targetDir: resolve(root, options.targetDir ?? process.env.CARGO_TARGET_DIR ?? "target"),
    fetchAgentBrowser: options.fetchAgentBrowser ?? fetchAgentBrowser,
    agentBrowserOptions: options.agentBrowserOptions,
    fetchStetdoc: options.fetchStetdoc ?? fetchStetdoc,
    stetdocOptions: options.stetdocOptions,
  };
  const desktop = join(root, "crates", "desktop");
  const binaries = join(desktop, "binaries");
  const overlay = join(desktop, "release", "sidecars.generated.json");

  // Nothing left from an earlier run for this target can reach a bundle or the size table.
  await rm(overlay, { force: true });
  await mkdir(binaries, { recursive: true });
  await mkdir(dirname(overlay), { recursive: true });
  for (const file of await readdir(binaries)) {
    const ours = [`-${triple}`, `-${triple}.exe`].some((end) => file.endsWith(end) || file.endsWith(`${end}.partial`));
    if (ours) await rm(join(binaries, file), { force: true });
  }

  const externalBin = [];
  const files = {};
  const resources = {};
  for (const sidecar of sidecars) {
    if (!sidecar.targets.includes(triple)) continue;
    if (sidecar.safeWorkspace && options.safeWorkspace !== true) {
      console.log(`${sidecar.name}: the Safe workspace is off, left out`);
      continue;
    }
    const dest = join(binaries, `${sidecar.name}-${triple}${exe(triple)}`);
    const placed = await sidecar.place(triple, ctx, dest);
    if (placed === null) {
      console.log(`${sidecar.name}: no build for ${triple}, left out`);
      continue;
    }
    if (resolve(placed) !== resolve(dest)) {
      await copyFile(placed, dest);
      if (!isWindows(triple)) await chmod(dest, 0o755);
    }
    console.log(`${sidecar.name}: ${dest} (${(await stat(dest)).size} bytes)`);
    if (sidecar.ownFolder && isDarwin(triple)) {
      files[stetdocHelper] = staged(sidecar.name, triple);
    } else if (sidecar.ownFolder) {
      resources[staged(sidecar.name, triple)] = stetdocResource(triple);
    } else if (sidecar.macosHelper && isDarwin(triple)) {
      files[`Helpers/${sidecar.name}`] = `binaries/${sidecar.name}-${triple}`;
    } else {
      externalBin.push(`binaries/${sidecar.name}`);
    }
  }

  const bundle = { externalBin };
  if (Object.keys(files).length > 0) bundle.macOS = { files };
  if (Object.keys(resources).length > 0) bundle.resources = resources;
  await writeFile(overlay, `${JSON.stringify({ bundle })}\n`);
  console.log(`${overlay}: ${JSON.stringify(bundle)}`);
  return externalBin;
}

/**
 * Builds only the setup exe for a Windows `triple`, reproducibly, and returns its path; nothing
 * is placed and no overlay is written. setup-exe-repro.yml runs it twice, in target/ wiped
 * between the two.
 *
 * @param {string} triple a Windows target in RELEASE_TARGETS
 * @param {{ root?: string, cargo?: (args: string[], extra?: object) => void, targetDir?: string,
 *   commitTime?: (root: string) => string }} [options] the tests replace cargo, the root and git
 * @returns {Promise<string>}
 */
export async function buildSetupExe(triple, options = {}) {
  if (!RELEASE_TARGETS.filter(isWindows).includes(triple)) {
    throw new Error(`sidecars: the setup exe ships on the Windows release targets only, not ${triple}`);
  }
  const root = options.root ?? ROOT;
  const targetDir = resolve(root, options.targetDir ?? process.env.CARGO_TARGET_DIR ?? "target");
  return setupExe(triple, { root, targetDir, commitTime: options.commitTime, cargo: options.cargo ?? runCargo });
}

const USAGE =
  "usage: node .github/scripts/sidecars.mjs [--safe-workspace=true|false] <target-triple> | --check <target-triple> | --setup-exe <target-triple>";

/**
 * The Safe workspace switch's value from its option, or null for anything but
 * --safe-workspace=true or --safe-workspace=false, so a typo can't turn it off unseen.
 * @param {string} arg
 * @returns {boolean | null}
 */
export function safeWorkspaceOption(arg) {
  return { "--safe-workspace=true": true, "--safe-workspace=false": false }[arg] ?? null;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  let safeWorkspace = false;
  if (args[0]?.startsWith("--safe-workspace")) {
    safeWorkspace = safeWorkspaceOption(args.shift());
  }
  // The switch goes with the plain form only: --check and --setup-exe take none.
  const switched = args.length < process.argv.length - 2;
  const [first, ...rest] = args;
  const flag = first === "--setup-exe" || first === "--check";
  const setup = first === "--setup-exe";
  if (safeWorkspace === null || !first || rest.length !== (flag ? 1 : 0) || (switched && flag)) {
    console.error(USAGE);
    process.exit(2);
  }
  try {
    if (setup) {
      console.log(await buildSetupExe(rest[0]));
    } else if (first === "--check") {
      const problem = overlayProblem(JSON.parse(await readFile(OVERLAY, "utf8")), rest[0]);
      if (problem !== null) throw new Error(`crates/desktop/release/sidecars.generated.json: ${problem}`);
      console.log(`crates/desktop/release/sidecars.generated.json holds only its allowed shape for ${rest[0]}`);
    } else {
      await placeSidecars(first, { safeWorkspace });
    }
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exit(1);
  }
}
