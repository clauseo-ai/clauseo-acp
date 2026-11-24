// Writes the signed fixtures in this folder with Tauri's own signer, so the tests prove the format
// `tauri signer sign` writes. Two throwaway keys, made with `tauri signer generate --ci -p <pw>`
// outside the repo; only their public halves are committed (key-a.pub, key-b.pub).
//
//   TAURI=/tmp/x/node_modules/.bin/tauri KEY_A=/tmp/x/a.key KEY_B=/tmp/x/b.key \
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<pw> node make_fixtures.mjs
//
// Run it from anywhere; it writes beside itself. `@tauri-apps/cli` 2.12.2 made the committed files.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const live = join(here, "../../../limits/tests/live");
const { TAURI, KEY_A, KEY_B } = process.env;
if (!TAURI || !KEY_A || !KEY_B || !process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  throw new Error("set TAURI, KEY_A, KEY_B and TAURI_SIGNING_PRIVATE_KEY_PASSWORD");
}
copyFileSync(`${KEY_A}.pub`, join(here, "key-a.pub"));
copyFileSync(`${KEY_B}.pub`, join(here, "key-b.pub"));

const packageJson = JSON.parse(readFileSync(join(live, "package.json"), "utf8"));
const packageLock = JSON.parse(readFileSync(join(live, "package-lock.json"), "utf8"));

// SIZES in crates/agents/src/snapshot/embedded.rs.
const sizes = [
  ["claude-acp", "darwin", "arm64", 112272099, 284447720],
  ["claude-acp", "darwin", "x64", 116398379, 292936176],
  ["claude-acp", "win32", "x64", 124933588, 304272912],
  ["claude-acp", "win32", "arm64", 120819899, 291797016],
  ["claude-acp", "linux", "x64", 122081905, 300872245],
  ["claude-acp", "linux", "arm64", 122340028, 300225917],
  ["codex-acp", "darwin", "arm64", 138852372, 353173843],
  ["codex-acp", "darwin", "x64", 148075606, 377267611],
  ["codex-acp", "win32", "x64", 167303482, 472371597],
  ["codex-acp", "win32", "arm64", 156863952, 412140965],
  ["codex-acp", "linux", "x64", 167703509, 467510455],
  ["codex-acp", "linux", "arm64", 160119520, 423188897],
].map(([agent, os, arch, download, unpacked]) => ({ agent, os, arch, download, unpacked }));

const good = () => ({
  format: "clauseo-engines/1",
  seq: 2,
  generated: "2026-11-24T06:00:00Z",
  apps: ["0.1.0", "0.0.0-dev.1a2b3c4"],
  package_json: structuredClone(packageJson),
  package_lock: structuredClone(packageLock),
  sizes: structuredClone(sizes),
});

// The variants carry only the lock's root and the three pinned packages, to stay small.
const KEPT = [
  "",
  "node_modules/@agentclientprotocol/claude-agent-acp",
  "node_modules/@agentclientprotocol/codex-acp",
  "node_modules/@openai/codex",
];
const small = () => {
  const list = good();
  const all = list.package_lock.packages;
  list.package_lock.packages = Object.fromEntries(KEPT.map((key) => [key, all[key]]));
  return list;
};
const firstPackage = (list) => Object.keys(list.package_lock.packages)[1];
// Moves the locked package at node_modules/<name> to `version`, its tarball with it.
const lockAt = (list, name, version) => {
  const entry = list.package_lock.packages[`node_modules/${name}`];
  entry.version = version;
  entry.resolved = `https://registry.npmjs.org/${name}/-/${name.split("/").pop()}-${version}.tgz`;
};
// Pins dependency `name` at `version` everywhere: package.json, the lock's root and its entry.
const pin = (list, name, version) => {
  list.package_json.dependencies[name] = version;
  list.package_lock.packages[""].dependencies[name] = version;
  lockAt(list, name, version);
};
// Adds a copy of the first package's entry under `key`, changed by `change`.
const extraEntry = (list, key, change = () => {}) => {
  const entry = structuredClone(list.package_lock.packages[firstPackage(list)]);
  change(entry);
  list.package_lock.packages[key] = entry;
};
const CLAUDE = "@agentclientprotocol/claude-agent-acp";
const CODEX = "@agentclientprotocol/codex-acp";

const variants = {
  "unknown-field": (l) => { l.extra = true; },
  "wrong-format": (l) => { l.format = "clauseo-engines/2"; },
  "bad-generated": (l) => { l.generated = "2026-11-24 06:00:00Z"; },
  "no-apps": (l) => { l.apps = []; },
  "seq-zero": (l) => { l.seq = 0; },
  "bad-size": (l) => { l.sizes[0].os = "freebsd"; },
  "dup-size": (l) => { l.sizes.push(structuredClone(l.sizes[0])); },
  "extra-dep": (l) => { l.package_json.dependencies["left-pad"] = "1.3.0"; },
  "missing-dep": (l) => { delete l.package_json.dependencies["@agentclientprotocol/codex-acp"]; },
  "extra-override": (l) => { l.package_json.overrides["left-pad"] = "1.3.0"; },
  "scripts": (l) => { l.package_json.scripts = { postinstall: "node evil.js" }; },
  "lock-v2": (l) => { l.package_lock.lockfileVersion = 2; },
  "non-npm-resolved": (l) => {
    l.package_lock.packages[firstPackage(l)].resolved = "https://evil.example/pkg.tgz";
  },
  "missing-integrity": (l) => { delete l.package_lock.packages[firstPackage(l)].integrity; },
  "sha1-integrity": (l) => {
    l.package_lock.packages[firstPackage(l)].integrity = "sha1-2jmj7l5rSw0yVb/vlWAYkK/YBwk=";
  },
  "not-semver": (l) => { l.package_json.dependencies[CODEX] = "^2.2.2"; },
  "older": (l) => {
    l.package_json.overrides["@openai/codex"] = "0.162.0";
    lockAt(l, "@openai/codex", "0.162.0");
  },
  "newer": (l) => {
    l.seq = 3;
    pin(l, CLAUDE, "0.90.0");
  },
  "seq-huge": (l) => { l.seq = 2 ** 32 + 1; },
  "dotdot-key": (l) => extraEntry(l, "node_modules/../../evil"),
  "absolute-key": (l) => extraEntry(l, "/etc/evil"),
  "backslash-key": (l) => extraEntry(l, "node_modules/evil\\..\\x"),
  "missing-resolved": (l) => { delete l.package_lock.packages[firstPackage(l)].resolved; },
  "link": (l) => extraEntry(l, "node_modules/evil", (e) => {
    e.link = true;
    e.resolved = "../evil";
  }),
  "in-bundle": (l) => { l.package_lock.packages[firstPackage(l)].inBundle = true; },
  "other-resolved": (l) => {
    l.package_lock.packages[firstPackage(l)].resolved =
      "https://registry.npmjs.org/left-pad/-/left-pad-1.3.0.tgz";
  },
  "two-integrity": (l) => {
    const entry = l.package_lock.packages[firstPackage(l)];
    entry.integrity = `${entry.integrity} ${entry.integrity}`;
  },
  "override-alias": (l) => { l.package_json.overrides["@openai/codex"] = "npm:evil@1.0.0"; },
  "override-git": (l) => {
    l.package_json.overrides["@openai/codex"] = "git+https://evil.example/codex.git";
  },
  "override-ref": (l) => { l.package_json.overrides["@openai/codex"] = `$${CODEX}`; },
  "root-mismatch": (l) => { l.package_lock.packages[""].dependencies[CLAUDE] = "0.89.0"; },
  "installed-mismatch": (l) => { lockAt(l, CODEX, "2.2.1"); },
};

const scratch = mkdtempSync(join(tmpdir(), "engines-fixtures-"));
// Signs `text` as a file named `name` and returns the .sig file's bytes.
const sign = (text, { key = KEY_A, name = "engines.json", args = [] } = {}) => {
  const file = join(scratch, name);
  writeFileSync(file, text);
  execFileSync(TAURI, ["signer", "sign", "-f", key, ...args, file], { stdio: "ignore" });
  return readFileSync(`${file}.sig`);
};
const write = (stem, text, sig) => {
  writeFileSync(join(here, `${stem}.json`), text);
  writeFileSync(join(here, `${stem}.json.sig`), sig);
};

const goodText = `${JSON.stringify(good(), null, 2)}\n`;
write("engines", goodText, sign(goodText));
writeFileSync(join(here, "engines.by-b.sig"), sign(goodText, { key: KEY_B }));
writeFileSync(join(here, "engines.other-name.sig"), sign(goodText, { name: "update.tar.gz" }));
writeFileSync(
  join(here, "engines.app-version.sig"),
  sign(goodText, { args: ["--app-version", "0.1.0"] }),
);
for (const [stem, change] of Object.entries(variants)) {
  const list = small();
  change(list);
  const text = `${JSON.stringify(list, null, 2)}\n`;
  write(stem, text, sign(text));
}
rmSync(scratch, { recursive: true });
