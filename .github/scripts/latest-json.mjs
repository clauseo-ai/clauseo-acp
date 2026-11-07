// The updater's manifest, latest.json, and the table of a release's assets. release.yml's
// `assemble` job runs this file; latest-json.test.mjs tests it and lints release.yml.
//
//   node .github/scripts/latest-json.mjs <dir> <version> <base-url> <pub-date> <sandbox-exe>
//   node .github/scripts/latest-json.mjs --verify <file> <public-key-file> [<signed-name> <version>]
//   node .github/scripts/latest-json.mjs --key-id <exe> <mode>
//
// <dir> holds the release's assets with their final names (the Mac archives already renamed to
// Clauseo_<version>_<arch>.app.tar.gz). <base-url> is where the release serves them,
// https://github.com/<owner>/<repo>/releases/download/<tag>; <pub-date> is RFC 3339.
// It checks that every tier 1 asset and every `.sig` an updater asset needs is there, and that
// each `.sig`'s trusted comment holds `version:<version>` (tauri-plugin-updater refuses a
// signature without it when `requireSignedVersion` is on), then writes <dir>/latest.json and
// prints one line per missing tier 2 asset, `missing <name>`, for the release notes.
// <sandbox-exe> is release.yml's SANDBOX_EXE, the name S15's set-up tool has in the install folder;
// it is published per architecture as <name>-<arch>.exe with its updater-key `.sig`, so that IT can
// check a copy against the release. Exit status: 0 when latest.json is written, 1 when an asset or
// a signature is wrong, 2 on a usage error.
// --verify checks <file>.sig (tauri signer's format: base64 of minisign's text) against <file>
// and the base64 public key in <public-key-file>, trusted comment included, and with <signed-name>
// and <version> that the trusted comment holds the fields file:<signed-name> and
// version:<version>: 0 when it holds, 1 when not.
// --key-id reads the VERSIONINFO string UpdaterKeyId of the Windows exe <exe> (stage C's sandbox
// tool carries the updater key ID it was built with) by parsing its resources, and compares it with
// the key ID <mode> (release, test or dry) builds in: 0 when it matches, 1 when it differs or the
// exe can't be read, 3 when the exe has no UpdaterKeyId. Plain Node: no package to install.
//
// The manifest is the plugin's static format: { version, notes, pub_date, platforms }, each
// platform keyed `<os>-<arch>-<bundle>` (the plugin tries that key, then `<os>-<arch>`). The
// Windows installers are bundled with --no-binary-patching, so the app carries no bundle type and
// looks up the plain `windows-<arch>` key: each Windows entry is written under both. Linux never
// updates itself, so .deb and .rpm have no entry and no `.sig`.

import { createHash, createPublicKey, verify } from "node:crypto";
import { existsSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** A plain `X.Y.Z` version, the only form a Clauseo release takes. */
export const PLAIN_VERSION = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;

/** RFC 3339, as the plugin parses `pub_date`. */
const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;

/**
 * The key ID the sandbox's build.rs builds in when it has no key to embed (a dry run, with
 * tauri.conf.json's updater pubkey empty): stage C's PLACEHOLDER_KEY_ID in
 * crates/sandbox/src/windows/refresh/key.rs (arriving with its PR 3).
 */
export const PLACEHOLDER_KEY_ID = "AC986E7C6BA67AB9";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * The key ID of <publicKey> (Tauri's form: base64 of minisign's public key text), as minisign
 * prints it: the 8 key-ID bytes after "Ed", read as a little-endian number, 16 upper-case hex
 * digits. null when it is not such a key.
 */
export function publicKeyId(publicKey) {
  const lines = Buffer.from(String(publicKey).trim(), "base64").toString("utf8").split("\n");
  const key = Buffer.from(lines[1] ?? "", "base64");
  if (key.length !== 42 || key.subarray(0, 2).toString("latin1") !== "Ed") {
    return null;
  }
  return key.readBigUInt64LE(2).toString(16).toUpperCase().padStart(16, "0");
}

/**
 * The updater key ID a <mode> build's sandbox tool must carry: test's from <testPublicKey>
 * (updater-test.pub), release's from <configPublicKey> (tauri.conf.json's plugins.updater.pubkey),
 * dry's from <configPublicKey> too when it is set, else PLACEHOLDER_KEY_ID (build.rs's fallback).
 * Throws when the mode needs a key it does not have.
 */
export function expectedKeyId(mode, configPublicKey, testPublicKey) {
  const id = (key, what) => {
    const found = publicKeyId(key);
    if (found === null) {
      throw new Error(`${what} is not a minisign public key`);
    }
    return found;
  };
  switch (mode) {
    case "test":
      return id(testPublicKey, "updater-test.pub");
    case "release":
      return id(configPublicKey, "tauri.conf.json's plugins.updater.pubkey");
    case "dry":
      return String(configPublicKey ?? "").trim() === ""
        ? PLACEHOLDER_KEY_ID
        : id(configPublicKey, "tauri.conf.json's plugins.updater.pubkey");
    default:
      throw new Error(`unknown mode ${mode}`);
  }
}

/**
 * The string <name> in the VERSIONINFO resource of PE file <pe> (a Buffer): the section table maps
 * the resource directory (data directory 2) to the file, its tree goes type RT_VERSION (16), then any
 * name, then any language, to VS_VERSIONINFO, whose StringFileInfo string tables hold the strings.
 * null when the exe has no resources, no version resource or no such string; throws when <pe> is not
 * a PE file or its resources do not parse.
 */
export function versionString(pe, name) {
  const fail = (what) => {
    throw new Error(`not a readable PE file: ${what}`);
  };
  try {
    if (pe.toString("latin1", 0, 2) !== "MZ") {
      fail("no MZ header");
    }
    const nt = pe.readUInt32LE(0x3c);
    if (pe.toString("latin1", nt, nt + 4) !== "PE\0\0") {
      fail("no PE signature");
    }
    const sections = pe.readUInt16LE(nt + 6);
    const optionalSize = pe.readUInt16LE(nt + 20);
    const optional = nt + 24;
    const magic = pe.readUInt16LE(optional);
    if (magic !== 0x10b && magic !== 0x20b) {
      fail(`optional header magic 0x${magic.toString(16)}`);
    }
    const rvaCount = pe.readUInt32LE(optional + (magic === 0x20b ? 108 : 92));
    if (rvaCount < 3) {
      return null;
    }
    const directories = optional + (magic === 0x20b ? 112 : 96);
    const resourceRva = pe.readUInt32LE(directories + 2 * 8);
    if (resourceRva === 0) {
      return null;
    }
    const table = optional + optionalSize;
    const offsetOf = (rva) => {
      for (let i = 0; i < sections; i += 1) {
        const at = table + i * 40;
        const virtualAddress = pe.readUInt32LE(at + 12);
        const size = Math.max(pe.readUInt32LE(at + 8), pe.readUInt32LE(at + 16));
        if (rva >= virtualAddress && rva < virtualAddress + size) {
          return rva - virtualAddress + pe.readUInt32LE(at + 20);
        }
      }
      return fail(`RVA 0x${rva.toString(16)} is in no section`);
    };
    const root = offsetOf(resourceRva);
    // The entries of the resource directory at <dir> (offset from root): { id, offset, subdirectory }.
    const entries = (dir) => {
      const at = root + dir;
      const count = pe.readUInt16LE(at + 12) + pe.readUInt16LE(at + 14);
      return Array.from({ length: count }, (_, i) => {
        const nameField = pe.readUInt32LE(at + 16 + i * 8);
        const data = pe.readUInt32LE(at + 20 + i * 8);
        return {
          id: nameField & 0x80000000 ? null : nameField,
          offset: data & 0x7fffffff,
          subdirectory: (data & 0x80000000) !== 0,
        };
      });
    };
    const version = entries(0).find((entry) => entry.id === 16 && entry.subdirectory);
    if (version === undefined) {
      return null;
    }
    for (const named of entries(version.offset).filter((entry) => entry.subdirectory)) {
      for (const language of entries(named.offset).filter((entry) => !entry.subdirectory)) {
        const leaf = root + language.offset;
        const start = offsetOf(pe.readUInt32LE(leaf));
        const size = pe.readUInt32LE(leaf + 4);
        if (start + size > pe.length) {
          fail("the version resource runs past the end of the file");
        }
        const found = versionInfoString(pe.subarray(start, start + size), name);
        if (found !== null) {
          return found;
        }
      }
    }
    return null;
  } catch (error) {
    if (error instanceof RangeError) {
      return fail("a structure runs past the end of the file");
    }
    throw error;
  }
}

/**
 * The string <name> in VS_VERSIONINFO <info>, or null. Each node is { wLength, wValueLength, wType,
 * szKey (UTF-16, NUL-ended), padding to 4, Value, padding to 4, Children }. A String's value is read
 * up to its NUL or its node's end, whatever wValueLength says (compilers count it in bytes or in
 * characters).
 */
function versionInfoString(info, name) {
  const align = (n) => (n + 3) & ~3;
  const keyEnd = (at, end) => {
    let i = at;
    while (i + 1 < end && info.readUInt16LE(i) !== 0) {
      i += 2;
    }
    return i;
  };
  // [key, valueStart, childrenStart, end] of the node at <at>.
  const node = (at, limit) => {
    const length = info.readUInt16LE(at);
    const end = at + length;
    if (length < 6 || end > limit) {
      throw new RangeError("bad VERSIONINFO node");
    }
    const valueLength = info.readUInt16LE(at + 2);
    const text = info.readUInt16LE(at + 4) === 1;
    const k = keyEnd(at + 6, end);
    const key = info.toString("utf16le", at + 6, k);
    const valueStart = align(k + 2);
    const childrenStart = align(valueStart + (text ? valueLength * 2 : valueLength));
    if (valueStart > end) {
      throw new RangeError("bad VERSIONINFO node");
    }
    return { key, valueStart, childrenStart: Math.min(childrenStart, end), end };
  };
  const children = (parent) => {
    const list = [];
    for (let at = parent.childrenStart; at + 6 <= parent.end; ) {
      const child = node(at, parent.end);
      list.push(child);
      at = align(child.end);
    }
    return list;
  };
  const top = node(0, info.length);
  if (top.key !== "VS_VERSION_INFO") {
    throw new RangeError("not VS_VERSIONINFO");
  }
  for (const block of children(top).filter((child) => child.key === "StringFileInfo")) {
    for (const strings of children(block)) {
      for (const entry of children(strings)) {
        if (entry.key === name) {
          return info.toString("utf16le", entry.valueStart, keyEnd(entry.valueStart, entry.end));
        }
      }
    }
  }
  return null;
}

/** A file name S15's set-up tool may have in the install folder, such as clauseo-sandbox-setup.exe. */
const SANDBOX_EXE = /^[a-z0-9][a-z0-9-]*\.exe$/;

/** The release asset that publishes <sandboxExe> for Windows <arch> (x64 or arm64). */
export function sandboxAsset(sandboxExe, arch) {
  if (!SANDBOX_EXE.test(sandboxExe)) {
    throw new Error(`${sandboxExe} is not a plain lower-case .exe name`);
  }
  return sandboxExe.replace(/\.exe$/, `-${arch}.exe`);
}

/**
 * Every asset of release <version>, by tier (1 must be there, 2 may be missing), with the
 * latest.json keys of the ones the updater installs, and `signed` on the ones that carry an
 * updater-key `.sig` the updater does not read. The one home of the asset names: release.yml
 * builds them under these names, and the test checks a release against this list. <sandboxExe>
 * (release.yml's SANDBOX_EXE) adds S15's set-up tool per architecture; without it those rows are
 * left out, as only the updater's rows matter to the manifest.
 */
export function assets(version, sandboxExe = null) {
  const v = version;
  const sandbox =
    sandboxExe === null
      ? []
      : [
          { name: sandboxAsset(sandboxExe, "x64"), tier: 1, signed: true },
          { name: sandboxAsset(sandboxExe, "arm64"), tier: 2, signed: true },
        ];
  return [
    { name: `Clauseo_${v}_aarch64.dmg`, tier: 1 },
    { name: `Clauseo_${v}_aarch64.app.tar.gz`, tier: 1, platforms: ["darwin-aarch64-app"] },
    { name: `Clauseo_${v}_x64.dmg`, tier: 2 },
    { name: `Clauseo_${v}_x64.app.tar.gz`, tier: 2, platforms: ["darwin-x86_64-app"] },
    { name: `Clauseo_${v}_x64-setup.exe`, tier: 1, platforms: ["windows-x86_64-nsis", "windows-x86_64"] },
    { name: `Clauseo_${v}_x64-machine-setup.exe`, tier: 1 },
    { name: `Clauseo_${v}_arm64-setup.exe`, tier: 2, platforms: ["windows-aarch64-nsis", "windows-aarch64"] },
    { name: `Clauseo_${v}_arm64-machine-setup.exe`, tier: 2 },
    { name: `Clauseo_${v}_amd64.deb`, tier: 2 },
    { name: `Clauseo-${v}-1.x86_64.rpm`, tier: 2 },
    ...sandbox,
  ];
}

/**
 * Every way <signature> (a `.sig` file's text: base64 of minisign's signature text) fails to sign
 * <data> (a Buffer) under <publicKey> (a `.pub` file's text, as tauri signer writes both): the
 * file's signature, prehashed with BLAKE2b-512 (`ED`) or not (`Ed`), and the global signature
 * over it and the trusted comment. With <expect> ({ file, version }), the trusted comment must also
 * hold the tab-separated fields file:<file> and version:<version>, as tauri signer writes them
 * (`timestamp:<unix>\tfile:<base name of the signed file>\tversion:<v>`). Empty when it holds.
 */
export function signatureCheck(data, signature, publicKey, expect = null) {
  const decode = (text) => Buffer.from(String(text).trim(), "base64").toString("utf8").split("\n");
  const sig = decode(signature);
  const pub = decode(publicKey);
  const prefix = "trusted comment: ";
  if (sig.length < 4 || !sig[2].startsWith(prefix)) {
    return ["the signature is not minisign's text"];
  }
  const raw = Buffer.from(sig[1], "base64");
  const global = Buffer.from(sig[3], "base64");
  const key = Buffer.from(pub[1] ?? "", "base64");
  if (key.length !== 42 || key.subarray(0, 2).toString("latin1") !== "Ed") {
    return ["the public key is not a minisign Ed25519 key"];
  }
  const alg = raw.subarray(0, 2).toString("latin1");
  if (raw.length !== 74 || global.length !== 64 || (alg !== "ED" && alg !== "Ed")) {
    return ["the signature is not a minisign Ed25519 signature"];
  }
  if (!raw.subarray(2, 10).equals(key.subarray(2, 10))) {
    return ["the signature is by another key than the public key"];
  }
  const pk = createPublicKey({
    key: { kty: "OKP", crv: "Ed25519", x: key.subarray(10).toString("base64url") },
    format: "jwk",
  });
  const problems = [];
  const message = alg === "ED" ? createHash("blake2b512").update(data).digest() : data;
  if (!verify(null, message, pk, raw.subarray(10))) {
    problems.push("the signature does not match the file");
  }
  const comment = Buffer.from(sig[2].slice(prefix.length), "utf8");
  if (!verify(null, Buffer.concat([raw.subarray(10), comment]), pk, global)) {
    problems.push("the trusted comment is not signed by the key");
  }
  if (expect !== null) {
    const fields = sig[2].slice(prefix.length).split("\t");
    for (const [key, want] of [["file", expect.file], ["version", expect.version]]) {
      if (!fields.includes(`${key}:${want}`)) {
        problems.push(`the trusted comment holds no ${key}:${want}`);
      }
    }
  }
  return problems;
}

/** The latest.json keys of tier 1 assets: a manifest without one of them is refused. */
export function tier1Platforms(version) {
  return assets(version)
    .filter((asset) => asset.tier === 1)
    .flatMap((asset) => asset.platforms ?? []);
}

/**
 * The version in a `.sig`'s trusted comment, as the plugin reads it (`updater.rs`,
 * `signed_version`): the `.sig` is base64 of minisign's text, whose `trusted comment: ` line holds
 * tab-separated fields. null when there is no trusted comment or no `version:` field.
 */
export function signedVersion(signature) {
  const text = Buffer.from(signature.trim(), "base64").toString("utf8");
  const line = text.split("\n").find((l) => l.startsWith("trusted comment: "));
  if (line === undefined) {
    return null;
  }
  const field = line
    .slice("trusted comment: ".length)
    .split("\t")
    .find((f) => f.startsWith("version:"));
  return field === undefined ? null : field.slice("version:".length).trim();
}

/** The problems with signature <signature> for <name>, which must be signed for <version>. */
function signatureProblems(name, signature, version) {
  if (typeof signature !== "string" || signature.trim() === "") {
    return [`${name}: the signature is empty`];
  }
  const signed = signedVersion(signature);
  if (signed === null) {
    return [`${name}: the signature's trusted comment holds no version:<v>`];
  }
  if (signed !== version) {
    return [`${name}: the signature is for version ${signed}, not ${version}`];
  }
  return [];
}

/**
 * Every way <manifest> (parsed latest.json) is not the plugin's static format for <version>, or
 * leaves out a tier 1 platform; empty when it is fine.
 */
export function manifestProblems(manifest, version) {
  const problems = [];
  if (manifest === null || typeof manifest !== "object" || Array.isArray(manifest)) {
    return ["latest.json is not an object"];
  }
  const keys = Object.keys(manifest).sort().join(",");
  if (keys !== "notes,platforms,pub_date,version") {
    problems.push(`latest.json has the keys ${keys}, not notes, platforms, pub_date and version`);
  }
  if (manifest.version !== version || !PLAIN_VERSION.test(String(manifest.version))) {
    problems.push(`version is ${JSON.stringify(manifest.version)}, not ${version}`);
  }
  if (typeof manifest.notes !== "string") {
    problems.push("notes is not a string");
  }
  if (typeof manifest.pub_date !== "string" || !RFC3339.test(manifest.pub_date)) {
    problems.push(`pub_date ${JSON.stringify(manifest.pub_date)} is not RFC 3339`);
  }
  const platforms = manifest.platforms;
  if (platforms === null || typeof platforms !== "object" || Array.isArray(platforms)) {
    return [...problems, "platforms is not an object"];
  }
  const known = new Set(assets(version).flatMap((asset) => asset.platforms ?? []));
  for (const [key, entry] of Object.entries(platforms)) {
    if (!known.has(key)) {
      problems.push(`platforms has the unknown key ${key}`);
      continue;
    }
    if (entry === null || typeof entry !== "object" || Object.keys(entry).sort().join(",") !== "signature,url") {
      problems.push(`platforms.${key} is not { signature, url }`);
      continue;
    }
    let url = null;
    try {
      url = new URL(entry.url);
    } catch {
      problems.push(`platforms.${key}.url is not a URL`);
    }
    if (url !== null && url.protocol !== "https:") {
      problems.push(`platforms.${key}.url is not https`);
    }
    problems.push(...signatureProblems(`platforms.${key}`, entry.signature, version));
  }
  for (const key of tier1Platforms(version)) {
    if (!(key in platforms)) {
      problems.push(`platforms has no ${key} (tier 1)`);
    }
  }
  for (const arch of ["x86_64", "aarch64"]) {
    const nsis = platforms[`windows-${arch}-nsis`];
    const plain = platforms[`windows-${arch}`];
    if (JSON.stringify(nsis) !== JSON.stringify(plain)) {
      problems.push(`platforms.windows-${arch} differs from platforms.windows-${arch}-nsis`);
    }
  }
  return problems;
}

/**
 * Reads the assets in <dir> and builds latest.json for <version>. Returns { manifest, missing,
 * problems }: missing names the absent tier 2 assets; problems is empty when the manifest may be
 * written.
 */
export function buildManifest(dir, version, baseUrl, pubDate, sandboxExe = null) {
  const problems = [];
  const missing = [];
  const platforms = {};
  if (!PLAIN_VERSION.test(version)) {
    problems.push(`version ${version} is not plain X.Y.Z`);
  }
  for (const asset of assets(version, sandboxExe)) {
    if (!existsSync(join(dir, asset.name))) {
      if (asset.tier === 1) {
        problems.push(`${asset.name} is missing (tier 1)`);
      } else {
        missing.push(asset.name);
      }
      continue;
    }
    if (asset.platforms === undefined && !asset.signed) {
      continue;
    }
    const sigPath = join(dir, `${asset.name}.sig`);
    if (!existsSync(sigPath)) {
      problems.push(`${asset.name}.sig is missing`);
      continue;
    }
    const signature = readFileSync(sigPath, "utf8").trim();
    const wrong = signatureProblems(`${asset.name}.sig`, signature, version);
    if (wrong.length > 0) {
      problems.push(...wrong);
      continue;
    }
    for (const key of asset.platforms ?? []) {
      platforms[key] = { signature, url: `${baseUrl}/${asset.name}` };
    }
  }
  const manifest = { version, notes: "", pub_date: pubDate, platforms };
  if (problems.length === 0) {
    problems.push(...manifestProblems(manifest, version));
  }
  return { manifest, missing, problems };
}

function main(argv) {
  if (argv[0] === "--key-id" && argv.length === 3) {
    const [, exe, mode] = argv;
    let expected;
    let found;
    try {
      const config = JSON.parse(readFileSync(join(REPO, "crates", "desktop", "tauri.conf.json"), "utf8"));
      const testKey = readFileSync(join(REPO, "crates", "desktop", "release", "updater-test.pub"), "utf8");
      expected = expectedKeyId(mode, config.plugins?.updater?.pubkey ?? "", testKey);
      found = versionString(readFileSync(exe), "UpdaterKeyId");
    } catch (error) {
      process.stderr.write(`${exe}: ${error.message}\n`);
      return 1;
    }
    if (found === null) {
      process.stderr.write(`${exe}: no UpdaterKeyId in its VERSIONINFO (expected ${expected})\n`);
      return 3;
    }
    if (found !== expected) {
      process.stderr.write(`${exe}: UpdaterKeyId ${found}, not the ${mode} key ${expected}\n`);
      return 1;
    }
    process.stdout.write(`${exe}: UpdaterKeyId ${found}, the ${mode} key\n`);
    return 0;
  }
  if (argv[0] === "--verify" && (argv.length === 3 || argv.length === 5)) {
    const [, file, publicKeyFile, signedName, version] = argv;
    const expect = argv.length === 5 ? { file: signedName, version } : null;
    const problems = signatureCheck(
      readFileSync(file),
      readFileSync(`${file}.sig`, "utf8"),
      readFileSync(publicKeyFile, "utf8"),
      expect,
    );
    for (const problem of problems) {
      process.stderr.write(`${file}.sig: ${problem}\n`);
    }
    return problems.length === 0 ? 0 : 1;
  }
  if (argv.length !== 5 || !SANDBOX_EXE.test(argv[4])) {
    process.stderr.write(
      "usage: node latest-json.mjs <dir> <version> <base-url> <pub-date> <sandbox-exe>\n" +
        "       node latest-json.mjs --verify <file> <public-key-file> [<signed-name> <version>]\n" +
        "       node latest-json.mjs --key-id <exe> <mode>\n",
    );
    return 2;
  }
  const [dir, version, baseUrl, pubDate, sandboxExe] = argv;
  const { manifest, missing, problems } = buildManifest(dir, version, baseUrl, pubDate, sandboxExe);
  if (problems.length > 0) {
    for (const problem of problems) {
      process.stderr.write(`latest.json: ${problem}\n`);
    }
    return 1;
  }
  writeFileSync(join(dir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  for (const name of missing) {
    process.stdout.write(`missing ${name}\n`);
  }
  return 0;
}

// Real paths on both sides: run through a symbolic link, the script still runs (a --verify that
// silently did nothing would pass).
const entry = process.argv[1] === undefined ? "" : pathToFileURL(realpathSync(process.argv[1])).href;
if (pathToFileURL(realpathSync(fileURLToPath(import.meta.url))).href === entry) {
  process.exitCode = main(process.argv.slice(2));
}
