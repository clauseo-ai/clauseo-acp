// Writes the signed fixtures in this folder with Tauri's own signer, so the tests prove the format
// `tauri signer sign` writes. Two throwaway keys, made with `tauri signer generate --ci -p <pw>`
// outside the repo; only their public halves are committed (key-a.pub, key-b.pub). A third key, C,
// is made here for the comments Tauri never writes, and its secret half is never written.
//
//   TAURI=/tmp/x/node_modules/.bin/tauri KEY_A=/tmp/x/a.key KEY_B=/tmp/x/b.key \
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<pw> node make_fixtures.mjs
//
// Run it from anywhere; it writes beside itself. `@tauri-apps/cli` 2.12.2 made the committed files.
//
// The pins come from pins/package.json and pins/package-lock.json, copies of
// crates/limits/tests/live's two files made when the fixtures were last regenerated, so the tests
// (which read the same copies as the bundle) don't break when the live pins move. The sizes come
// from SIZES in crates/agents/src/snapshot/embedded.rs as it is when this runs. To follow a bump:
// copy the two live files into pins/, then run this.

import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes, sign as edSign } from "node:crypto";
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pins = join(here, "pins");
const { TAURI, KEY_A, KEY_B } = process.env;
if (!TAURI || !KEY_A || !KEY_B || !process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
  throw new Error("set TAURI, KEY_A, KEY_B and TAURI_SIGNING_PRIVATE_KEY_PASSWORD");
}
copyFileSync(`${KEY_A}.pub`, join(here, "key-a.pub"));
copyFileSync(`${KEY_B}.pub`, join(here, "key-b.pub"));

const packageJson = JSON.parse(readFileSync(join(pins, "package.json"), "utf8"));
const packageLock = JSON.parse(readFileSync(join(pins, "package-lock.json"), "utf8"));

// SIZES in crates/agents/src/snapshot/embedded.rs: (agent, Os::…, Arch::…, download, unpacked).
const embedded = readFileSync(join(here, "../../../agents/src/snapshot/embedded.rs"), "utf8");
const table = embedded.slice(embedded.indexOf("const SIZES"));
const sizes = [
  ...table
    .slice(0, table.indexOf("];"))
    .matchAll(/\(\s*"([a-z-]+)",\s*Os::(\w+),\s*Arch::(\w+),\s*([\d_]+),\s*([\d_]+),?\s*\)/g),
].map(([, agent, os, arch, download, unpacked]) => ({
  agent,
  os: os.toLowerCase(),
  arch: arch.toLowerCase(),
  download: Number(download.replaceAll("_", "")),
  unpacked: Number(unpacked.replaceAll("_", "")),
}));
if (sizes.length !== 12) throw new Error(`SIZES has ${sizes.length} rows, not 12`);

// The pins, and versions just below and above one: `<v>-0` sorts below `<v>`.
const pinOf = (name) => packageJson.dependencies[name] ?? packageJson.overrides[name];
const below = (version) => `${version}-0`;
const above = (version) => {
  const [major, minor, patch] = version.split(".").map(Number);
  return `${major}.${minor}.${patch + 1}`;
};

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
// Adds the full lock's codex alias for `platform`, changed by `change`, at `path`'s platform.
const platformAlias = (list, platform, change, path = platform) => {
  const entry = structuredClone(packageLock.packages[`node_modules/@openai/codex-${platform}`]);
  change(entry);
  list.package_lock.packages[`node_modules/@openai/codex-${path}`] = entry;
};
// Adds a copy of the first package's entry under `key`, changed by `change`.
const extraEntry = (list, key, change = () => {}) => {
  const entry = structuredClone(list.package_lock.packages[firstPackage(list)]);
  change(entry);
  list.package_lock.packages[key] = entry;
};
const CLAUDE = "@agentclientprotocol/claude-agent-acp";
const CODEX = "@agentclientprotocol/codex-acp";
const OPENAI = "@openai/codex";

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
  "not-semver": (l) => { l.package_json.dependencies[CODEX] = `^${pinOf(CODEX)}`; },
  "older": (l) => {
    l.package_json.overrides[OPENAI] = below(pinOf(OPENAI));
    lockAt(l, OPENAI, below(pinOf(OPENAI)));
  },
  "newer": (l) => {
    l.seq = 3;
    pin(l, CLAUDE, above(pinOf(CLAUDE)));
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
  "root-mismatch": (l) => {
    l.package_lock.packages[""].dependencies[CLAUDE] = below(pinOf(CLAUDE));
  },
  "installed-mismatch": (l) => { lockAt(l, CODEX, below(pinOf(CODEX))); },
  "package-json-array": (l) => { l.package_json = [l.package_json]; },
  "workspaces": (l) => { l.package_json.workspaces = ["evil"]; },
  "override-object": (l) => { l.package_json.overrides[OPENAI] = { ".": pinOf(OPENAI) }; },
  "override-dropped": (l) => { delete l.package_json.overrides; },
  "override-off-pin": (l) => { l.package_json.overrides[OPENAI] = above(pinOf(OPENAI)); },
  "bad-name": (l) => { l.package_lock.packages[firstPackage(l)].name = "../evil"; },
  "alias-at-engine": (l) => {
    // An alias of another package installed where claude-agent-acp goes.
    const entry = l.package_lock.packages[firstPackage(l)];
    entry.name = "@openai/codex";
    entry.version = pinOf(OPENAI);
    entry.resolved = `https://registry.npmjs.org/@openai/codex/-/codex-${pinOf(OPENAI)}.tgz`;
  },
  "short-integrity": (l) => {
    const entry = l.package_lock.packages[firstPackage(l)];
    entry.integrity = `${entry.integrity.slice(0, -4)}==`;
  },
  "empty-app": (l) => { l.apps = ["0.1.0", ""]; },
  "zero-size": (l) => { l.sizes[0].download = 0; },
  "missing-size": (l) => { l.sizes.pop(); },
  "dep-npm-alias": (l) => { l.package_json.dependencies[CLAUDE] = `npm:evil@${pinOf(CLAUDE)}`; },
  "dep-git": (l) => { l.package_json.dependencies[CLAUDE] = "git+https://evil.example/c.git"; },
  "dep-build": (l) => { pin(l, CLAUDE, `${pinOf(CLAUDE)}+evil`); },
  "seq-jump": (l) => { l.seq = 1 + 2 ** 16 + 1; },
  "seq-step": (l) => { l.seq = 1 + 2 ** 16; },
  "install-script": (l) => { l.package_lock.packages[firstPackage(l)].hasInstallScript = true; },
  "nested-codex": (l) => {
    // An older codex nested under codex-acp, beside the pinned one at the top.
    const key = `node_modules/${CODEX}/node_modules/@openai/codex`;
    l.package_lock.packages[key] = structuredClone(l.package_lock.packages["node_modules/@openai/codex"]);
    l.package_lock.packages[key].version = below(pinOf(OPENAI));
    l.package_lock.packages[key].resolved =
      `https://registry.npmjs.org/@openai/codex/-/codex-${below(pinOf(OPENAI))}.tgz`;
  },
  "upper-key": (l) => extraEntry(l, "node_modules/@agentclientprotocol/Claude-agent-acp"),
  "dot-key": (l) => extraEntry(l, "node_modules/@openai/codex."),
  "unnamed-platform": (l) => extraEntry(l, "node_modules/@openai/codex-darwin-arm64", (e) => {
    e.version = "0.1.0";
    e.resolved =
      "https://registry.npmjs.org/@openai/codex-darwin-arm64/-/codex-darwin-arm64-0.1.0.tgz";
    delete e.dependencies;
  }),
  "alias-ok": (l) => { platformAlias(l, "darwin-arm64", (e) => e); },
  "alias-wrong-version": (l) => {
    platformAlias(l, "darwin-arm64", (e) => {
      e.version = `${pinOf(OPENAI)}-linux-x64`;
      e.resolved = `https://registry.npmjs.org/@openai/codex/-/codex-${e.version}.tgz`;
    });
  },
  "alias-wrong-path": (l) => { platformAlias(l, "darwin-arm64", (e) => e, "freebsd-x64"); },
  "unknown-entry-field": (l) => { l.package_lock.packages[firstPackage(l)].future = true; },
  "extra-size": (l) => {
    l.sizes.push({ agent: "claude-acp", os: "linux", arch: "riscv64", download: 1, unpacked: 2 });
  },
  ...Object.fromEntries(
    [
      ["git", "dependencies", "git+https://evil.example/x.git"],
      ["github", "optionalDependencies", "github:evil/x"],
      ["user-repo", "dependencies", "evil/x"],
      ["url", "dependencies", "https://evil.example/x.tgz"],
      ["file", "peerDependencies", "file:../x"],
      ["link", "dependencies", "link:../x"],
      ["workspace", "dependencies", "workspace:*"],
      ["tag", "dependencies", "git"],
      ["alias-git", "dependencies", "npm:left-pad@git+https://evil.example/x.git"],
      ["alias-name", "dependencies", "npm:Left-Pad@1.3.0"],
      ["dot", "dependencies", ".1"],
      ["tgz", "dependencies", "1.tgz"],
      ["tar", "dependencies", "1.TAR"],
      ["alias-ok", "dependencies", "npm:left-pad@^1.3.0"],
      ["beta1", "dependencies", "beta1"],
    ].map(([stem, field, spec]) => [
      `spec-${stem}`,
      (l) => {
        const entry = l.package_lock.packages[firstPackage(l)];
        entry[field] = { ...(entry[field] ?? {}), "evil-x": spec };
      },
    ]),
  ),
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
// The trusted comment's file name as Tauri writes it: the signed file's base name.
writeFileSync(join(here, "engines.bak-name.sig"), sign(goodText, { name: "engines.json.bak" }));
// Comments Tauri never writes (no digits, a trailing tab: it refuses that file name), from a minisign-compatible signer here with key C;
// `engines.by-c.sig` is the right comment by the same signer, so the tests prove the signer.
const keyC = generateKeyPairSync("ed25519");
const keyId = randomBytes(8);
const raw = Buffer.from(keyC.publicKey.export({ format: "jwk" }).x, "base64url");
const idHex = Buffer.from(keyId).reverse().toString("hex").toUpperCase();
const pubLine = Buffer.concat([Buffer.from("Ed"), keyId, raw]).toString("base64");
writeFileSync(
  join(here, "key-c.pub"),
  Buffer.from(`untrusted comment: minisign public key: ${idHex}\n${pubLine}\n`).toString("base64"),
);
const minisign = (text, comment) => {
  const hash = createHash("blake2b512").update(text).digest();
  const signature = edSign(null, hash, keyC.privateKey);
  const global = edSign(null, Buffer.concat([signature, Buffer.from(comment)]), keyC.privateKey);
  const line = Buffer.concat([Buffer.from("ED"), keyId, signature]).toString("base64");
  const file = `untrusted comment: signature from tauri secret key\n${line}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`;
  return Buffer.from(file).toString("base64");
};
writeFileSync(join(here, "engines.by-c.sig"), minisign(goodText, "timestamp:1795500000\tfile:engines.json"));
writeFileSync(join(here, "engines.no-digits.sig"), minisign(goodText, "timestamp:\tfile:engines.json"));
writeFileSync(
  join(here, "engines.trailing-tab.sig"),
  minisign(goodText, "timestamp:1795500000\tfile:engines.json\t"),
);
for (const [stem, change] of Object.entries(variants)) {
  const list = small();
  change(list);
  const text = `${JSON.stringify(list, null, 2)}\n`;
  write(stem, text, sign(text));
}
rmSync(scratch, { recursive: true });
