// sidecars.mjs: every sidecar a release ships, built or fetched for one target, and the
// release-only overlay that bundles them (ADR 0035, "Sidecars").
//
//   node .github/scripts/sidecars.mjs <target-triple>
//   node .github/scripts/sidecars.mjs --check   (the overlay is only bundle.externalBin, clauseo- names)
//
// release.yml's sidecar seam runs it in each `build` leg, after the installs and before any
// secret. For the target it:
//   - builds or fetches each sidecar in SIDECARS below, and copies it to
//     crates/desktop/binaries/<name>-<triple> (.exe on Windows), Tauri's externalBin naming.
//     Every name starts with clauseo-: the .deb and .rpm install sidecars into /usr/bin, beside
//     other programs (agent-browser itself, from a global npm install, for one);
//   - writes crates/desktop/release/sidecars.generated.json, holding only
//     {"bundle":{"externalBin":[...]}} for the sidecars it placed.
// The legs pass that file last with --config to every `tauri build` and `tauri bundle`. A JSON
// merge patch replaces arrays, so this one list is the target's whole externalBin. Both outputs
// are gitignored. The base tauri.conf.json has no externalBin: tauri-build checks that every
// listed file exists, which would break every dev, CI and e2e build.
//
// Cargo runs with the job's own environment (RUSTFLAGS and the CARGO_PROFILE_RELEASE_* flags),
// so a sidecar gets the same release flags as the app, and its .pdb, .dSYM or .dwp lands in
// target/<triple>/release beside the app's.
//
// Node built-ins only. Tests: sidecars.test.mjs next to this file (ci.yml's fast lane).

import { spawnSync } from "node:child_process";
import { chmod, copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { fetchAgentBrowser } from "../../crates/desktop/sidecars/agent-browser.mjs";

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
 * Fetches the pinned agent-browser straight to its destination; null when the pin says none.
 * The pin and the download keep upstream's asset names; only the installed name has the prefix.
 */
const pinnedAgentBrowser = async (triple, ctx, dest) =>
  ctx.fetchAgentBrowser(triple, dest, ctx.agentBrowserOptions);

/**
 * Every sidecar, in bundle order. `targets` is the triples it ships on; `place` returns the
 * path of the built binary (copied to the destination), the destination itself (already
 * written), or null (no build for this target: left out of the list, not an error). A throw
 * fails the job.
 */
export const SIDECARS = [
  { name: "clauseo-mcp", targets: RELEASE_TARGETS, place: cargoBin("clauseo-mcp") },
  // Throws on a triple missing from crates/desktop/agent-browser.toml; null for `none = true`.
  { name: "clauseo-agent-browser", targets: RELEASE_TARGETS, place: pinnedAgentBrowser },
  // Parked (host thread, 2026-11-03): the guard's start time was not the cost. If it returns:
  // { name: "clauseo-guard", targets: RELEASE_TARGETS, place: cargoBin("clauseo-guard") },
  // with -C target-feature=+crt-static on Windows (S20-sidecars.md, "Release flags").
  // S15 adds its rows here: clauseo-sandbox-setup and clauseo-sandbox-run, Windows targets only.
];

/** Every sidecar's name starts with this (ADR 0035, "Sidecars"). */
export const PREFIX = "clauseo-";

const OVERLAY = join(ROOT, "crates", "desktop", "release", "sidecars.generated.json");

/**
 * Why a parsed overlay is not one this script writes, or null when it is: only
 * bundle.externalBin, a list of binaries/clauseo-* strings. release.yml checks it with --check
 * after the seam and again in sign-windows, before any bundle reads it.
 */
export function overlayProblem(overlay) {
  const keys = (v) => (v !== null && typeof v === "object" && !Array.isArray(v) ? Object.keys(v).join(" ") : null);
  if (keys(overlay) !== "bundle" || keys(overlay.bundle) !== "externalBin" || !Array.isArray(overlay.bundle.externalBin)) {
    return "it may hold only bundle.externalBin, a list";
  }
  const bad = overlay.bundle.externalBin.filter((entry) => typeof entry !== "string" || !entry.startsWith(`binaries/${PREFIX}`));
  return bad.length > 0 ? `each entry must start with binaries/${PREFIX}: ${JSON.stringify(bad)}` : null;
}

/** Runs cargo in the repo root with the inherited environment; throws on failure. */
function runCargo(args) {
  console.log(`$ cargo ${args.join(" ")}`);
  const result = spawnSync("cargo", args, { cwd: ROOT, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`cargo ${args.join(" ")} failed (exit ${result.status})`);
}

/**
 * Places every sidecar for `triple` and writes the overlay. Returns the overlay's externalBin.
 *
 * @param {string} triple a target in RELEASE_TARGETS
 * @param {{ root?: string, cargo?: (args: string[]) => void, targetDir?: string,
 *   fetchAgentBrowser?: typeof fetchAgentBrowser, agentBrowserOptions?: object,
 *   sidecars?: typeof SIDECARS }} [options] the tests replace cargo, the fetch and the root
 * @returns {Promise<string[]>}
 */
export async function placeSidecars(triple, options = {}) {
  if (!RELEASE_TARGETS.includes(triple)) {
    throw new Error(`sidecars: ${triple} is not a release target (${RELEASE_TARGETS.join(", ")})`);
  }
  const sidecars = options.sidecars ?? SIDECARS;
  for (const { name } of sidecars) {
    if (!name.startsWith(PREFIX)) throw new Error(`sidecars: ${name} does not start with ${PREFIX}`);
  }
  const root = options.root ?? ROOT;
  const ctx = {
    cargo: options.cargo ?? runCargo,
    targetDir: resolve(root, options.targetDir ?? process.env.CARGO_TARGET_DIR ?? "target"),
    fetchAgentBrowser: options.fetchAgentBrowser ?? fetchAgentBrowser,
    agentBrowserOptions: options.agentBrowserOptions,
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
  for (const sidecar of sidecars) {
    if (!sidecar.targets.includes(triple)) continue;
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
    externalBin.push(`binaries/${sidecar.name}`);
  }

  await writeFile(overlay, `${JSON.stringify({ bundle: { externalBin } })}\n`);
  console.log(`${overlay}: ${JSON.stringify(externalBin)}`);
  return externalBin;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [triple, ...extra] = process.argv.slice(2);
  if (!triple || extra.length > 0) {
    console.error("usage: node .github/scripts/sidecars.mjs <target-triple> | --check");
    process.exit(2);
  }
  try {
    if (triple === "--check") {
      const problem = overlayProblem(JSON.parse(await readFile(OVERLAY, "utf8")));
      if (problem !== null) throw new Error(`crates/desktop/release/sidecars.generated.json: ${problem}`);
      console.log("crates/desktop/release/sidecars.generated.json holds only bundle.externalBin, clauseo- names");
    } else {
      await placeSidecars(triple);
    }
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exit(1);
  }
}
