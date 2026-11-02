// The updater's manifest, latest.json, and the table of a release's assets. release.yml's
// `assemble` job runs this file; latest-json.test.mjs tests it and lints release.yml.
//
//   node .github/scripts/latest-json.mjs <dir> <version> <base-url> <pub-date>
//
// <dir> holds the release's assets with their final names (the Mac archives already renamed to
// Clauseo_<version>_<arch>.app.tar.gz). <base-url> is where the release serves them,
// https://github.com/<owner>/<repo>/releases/download/<tag>; <pub-date> is RFC 3339.
// It checks that every tier 1 asset and every `.sig` an updater asset needs is there, and that
// each `.sig`'s trusted comment holds `version:<version>` (tauri-plugin-updater refuses a
// signature without it when `requireSignedVersion` is on), then writes <dir>/latest.json and
// prints one line per missing tier 2 asset, `missing <name>`, for the release notes.
// Exit status: 0 when latest.json is written, 1 when an asset or a signature is wrong, 2 on a
// usage error. Plain Node: no package to install.
//
// The manifest is the plugin's static format: { version, notes, pub_date, platforms }, each
// platform keyed `<os>-<arch>-<bundle>` (the plugin tries that key, then `<os>-<arch>`). The
// Windows installers are bundled with --no-binary-patching, so the app carries no bundle type and
// looks up the plain `windows-<arch>` key: each Windows entry is written under both. Linux never
// updates itself, so .deb and .rpm have no entry and no `.sig`.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

/** A plain `X.Y.Z` version, the only form a Clauseo release takes. */
export const PLAIN_VERSION = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;

/** RFC 3339, as the plugin parses `pub_date`. */
const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/;

/**
 * Every asset of release <version>, by tier (1 must be there, 2 may be missing), with the
 * latest.json keys of the ones the updater installs. The one home of the asset names: release.yml
 * builds them under these names, and the test checks a release against this list.
 */
export function assets(version) {
  const v = version;
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
  ];
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
export function buildManifest(dir, version, baseUrl, pubDate) {
  const problems = [];
  const missing = [];
  const platforms = {};
  if (!PLAIN_VERSION.test(version)) {
    problems.push(`version ${version} is not plain X.Y.Z`);
  }
  for (const asset of assets(version)) {
    if (!existsSync(join(dir, asset.name))) {
      if (asset.tier === 1) {
        problems.push(`${asset.name} is missing (tier 1)`);
      } else {
        missing.push(asset.name);
      }
      continue;
    }
    if (asset.platforms === undefined) {
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
    for (const key of asset.platforms) {
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
  if (argv.length !== 4) {
    process.stderr.write("usage: node latest-json.mjs <dir> <version> <base-url> <pub-date>\n");
    return 2;
  }
  const [dir, version, baseUrl, pubDate] = argv;
  const { manifest, missing, problems } = buildManifest(dir, version, baseUrl, pubDate);
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

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main(process.argv.slice(2));
}
