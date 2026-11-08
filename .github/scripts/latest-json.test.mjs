// Tests for latest-json.mjs, and the rules release.yml must keep. Plain Node, no install:
//
//   node --test .github/scripts/latest-json.test.mjs
//
// ci.yml's fast lane runs it on every pull request; nothing in it reaches the network. With
// LATEST_JSON_RELEASE=<owner>/<repo>@<tag> set, one more test checks that published release:
// its latest.json is the plugin's format with every signature for the tag's version, every
// `platforms` URL downloads, and each download matches SHA256SUMS.txt. It is skipped otherwise.
// A draft's assets need a signed-in download, so run it on a test pre-release or a published
// release.
//
// release.yml is read as text, line by line with its indentation, not with a YAML parser (none
// ships with Node). It fails on a pull_request or pull_request_target trigger, a `uses:` not
// pinned by a 40-character commit SHA (a local `./` action passes), a job with no `permissions:`,
// a mention of `secrets` anywhere but inside a step, a job that downloads an artifact name it
// also uploads (a re-run of that job would download its own earlier upload), and a step that runs
// sidecars.mjs (third-party build scripts) with a secret in it or in any step before it.
// release.yml's own test also holds every `tauri build` and `tauri bundle` of the shipped legs
// to passing the generated sidecar overlay last, and checks the sandbox tool's path: its one name
// (SANDBOX_EXE), its updater signature before NSIS, its .sig in both installers, assemble's checks
// of the published exe and .sig against both installers and the updater key, and the build env
// its build.rs reads; and every job that passes release.conf.json to passing blocklist.conf.json
// with it, needing `blocklist` and downloading blocklist-snapshot before it builds or bundles.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  PLACEHOLDER_KEY_ID,
  assets,
  buildManifest,
  expectedKeyId,
  manifestProblems,
  publicKeyId,
  sandboxAsset,
  signatureCheck,
  signedVersion,
  tier1Platforms,
  versionString,
} from "./latest-json.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const RELEASE_YML = join(here, "..", "workflows", "release.yml");

/** A `.sig` as tauri signer writes it, with <comment> as the trusted comment. */
function fakeSig(comment) {
  const text = [
    "untrusted comment: signature from tauri secret key",
    "RUQT+H25DBA+wygq7VwnoKwPRikYGrMvKZnAPYdUk/J3TvEI+kcHBD2RebKmrW4zrD7EMzTv7UUqb/+bSp06jKwoqfy1QWTPiwM=",
    `trusted comment: ${comment}`,
    "4OaycKJG3qgoEjjj/8KnSGE2zAVr7XCoyMDrXIciSuuSHlROuQDAOJnL5d/Pm8vT7vQgtrgslApq8p3JHvvdBg==",
    "",
  ].join("\n");
  return Buffer.from(text).toString("base64");
}

/** A folder holding release <version>'s assets but <leave>, each updater asset signed for <signedFor>. */
function release(version, { leave = [], signedFor = version, comment } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-"));
  for (const asset of assets(version)) {
    if (leave.includes(asset.name)) {
      continue;
    }
    writeFileSync(join(dir, asset.name), asset.name);
    if (asset.platforms !== undefined) {
      const trusted = comment ?? `timestamp:1793641889\tfile:${asset.name}\tversion:${signedFor}`;
      writeFileSync(join(dir, `${asset.name}.sig`), fakeSig(trusted));
    }
  }
  return dir;
}

const BASE = "https://github.com/clauseo-ai/clauseo-acp/releases/download/v1.2.3";
const DATE = "2026-11-02T10:00:00Z";

/** buildManifest over a scratch release, removed afterwards. */
function build(version, options) {
  const dir = release(version, options);
  try {
    return buildManifest(dir, version, BASE, DATE);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a full release gives the plugin's static format, Windows under both keys", () => {
  const { manifest, missing, problems } = build("1.2.3");
  assert.deepEqual(problems, []);
  assert.deepEqual(missing, []);
  assert.deepEqual(Object.keys(manifest), ["version", "notes", "pub_date", "platforms"]);
  assert.equal(manifest.version, "1.2.3");
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    "darwin-aarch64-app",
    "darwin-x86_64-app",
    "windows-aarch64",
    "windows-aarch64-nsis",
    "windows-x86_64",
    "windows-x86_64-nsis",
  ]);
  assert.equal(manifest.platforms["windows-x86_64"].url, `${BASE}/Clauseo_1.2.3_x64-setup.exe`);
  assert.deepEqual(manifest.platforms["windows-x86_64"], manifest.platforms["windows-x86_64-nsis"]);
  assert.equal(manifest.platforms["darwin-aarch64-app"].url, `${BASE}/Clauseo_1.2.3_aarch64.app.tar.gz`);
  assert.equal(signedVersion(manifest.platforms["darwin-aarch64-app"].signature), "1.2.3");
  assert.deepEqual(manifestProblems(manifest, "1.2.3"), []);
});

test("a missing tier 2 asset is named, and its platform left out", () => {
  const { manifest, missing, problems } = build("1.2.3", {
    leave: ["Clauseo_1.2.3_x64.app.tar.gz", "Clauseo_1.2.3_amd64.deb"],
  });
  assert.deepEqual(problems, []);
  assert.deepEqual(missing, ["Clauseo_1.2.3_x64.app.tar.gz", "Clauseo_1.2.3_amd64.deb"]);
  assert.equal("darwin-x86_64-app" in manifest.platforms, false);
});

test("Windows ARM64 is tier 1: a missing arm64 installer, or its keys in latest.json, is refused", () => {
  assert.deepEqual(build("1.2.3", { leave: ["Clauseo_1.2.3_arm64-setup.exe"] }).problems, [
    "Clauseo_1.2.3_arm64-setup.exe is missing (tier 1)",
  ]);
  assert.deepEqual(build("1.2.3", { leave: ["Clauseo_1.2.3_arm64-machine-setup.exe"] }).problems, [
    "Clauseo_1.2.3_arm64-machine-setup.exe is missing (tier 1)",
  ]);
  assert.ok(tier1Platforms("1.2.3").includes("windows-aarch64"));
  assert.ok(tier1Platforms("1.2.3").includes("windows-aarch64-nsis"));
  const { manifest } = build("1.2.3");
  const broken = structuredClone(manifest);
  delete broken.platforms["windows-aarch64"];
  delete broken.platforms["windows-aarch64-nsis"];
  assert.deepEqual(manifestProblems(broken, "1.2.3"), [
    "platforms has no windows-aarch64-nsis (tier 1)",
    "platforms has no windows-aarch64 (tier 1)",
  ]);
});

test("a missing tier 1 asset or signature is refused", () => {
  assert.deepEqual(build("1.2.3", { leave: ["Clauseo_1.2.3_x64-machine-setup.exe"] }).problems, [
    "Clauseo_1.2.3_x64-machine-setup.exe is missing (tier 1)",
  ]);
  const dir = release("1.2.3");
  try {
    rmSync(join(dir, "Clauseo_1.2.3_aarch64.app.tar.gz.sig"));
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE).problems, [
      "Clauseo_1.2.3_aarch64.app.tar.gz.sig is missing",
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a signature for another version, or for none, is refused", () => {
  const other = build("1.2.3", { signedFor: "1.2.2" }).problems;
  assert.ok(other.includes("Clauseo_1.2.3_x64-setup.exe.sig: the signature is for version 1.2.2, not 1.2.3"), other);
  const none = build("1.2.3", { comment: "timestamp:1793641889\tfile:x" }).problems;
  assert.ok(none.includes("Clauseo_1.2.3_x64-setup.exe.sig: the signature's trusted comment holds no version:<v>"), none);
});

test("an updater asset signed under another name, as the bundler's Clauseo.app.tar.gz, is refused", () => {
  const dir = release("1.2.3");
  try {
    for (const arch of ["aarch64", "x64"]) {
      writeFileSync(join(dir, `Clauseo_1.2.3_${arch}.app.tar.gz.sig`), fakeSig("timestamp:1794112225\tfile:Clauseo.app.tar.gz\tversion:1.2.3"));
    }
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE).problems, [
      "Clauseo_1.2.3_aarch64.app.tar.gz.sig: the signature's trusted comment holds no file:Clauseo_1.2.3_aarch64.app.tar.gz",
      "Clauseo_1.2.3_x64.app.tar.gz.sig: the signature's trusted comment holds no file:Clauseo_1.2.3_x64.app.tar.gz",
    ]);
    // The other arch's name is no better: the name binds the arch.
    writeFileSync(join(dir, "Clauseo_1.2.3_aarch64.app.tar.gz.sig"), fakeSig("timestamp:1\tfile:Clauseo_1.2.3_x64.app.tar.gz\tversion:1.2.3"));
    rmSync(join(dir, "Clauseo_1.2.3_x64.app.tar.gz"));
    rmSync(join(dir, "Clauseo_1.2.3_x64.app.tar.gz.sig"));
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE).problems, [
      "Clauseo_1.2.3_aarch64.app.tar.gz.sig: the signature's trusted comment holds no file:Clauseo_1.2.3_aarch64.app.tar.gz",
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a version that is not plain X.Y.Z is refused", () => {
  assert.ok(build("1.2.3-beta.1").problems.includes("version 1.2.3-beta.1 is not plain X.Y.Z"));
});

test("manifestProblems names each way a manifest is wrong", () => {
  const { manifest } = build("1.2.3");
  const broken = structuredClone(manifest);
  broken.pub_date = "2 November 2026";
  broken.platforms["linux-x86_64"] = broken.platforms["darwin-aarch64-app"];
  broken.platforms["windows-x86_64"] = { ...broken.platforms["windows-x86_64"], url: "http://example.com/a" };
  delete broken.platforms["darwin-aarch64-app"];
  assert.deepEqual(manifestProblems(broken, "1.2.3"), [
    'pub_date "2 November 2026" is not RFC 3339',
    "platforms.windows-x86_64.url is not https",
    "platforms has the unknown key linux-x86_64",
    "platforms has no darwin-aarch64-app (tier 1)",
    "platforms.windows-x86_64 differs from platforms.windows-x86_64-nsis",
  ]);
  assert.deepEqual(manifestProblems({ ...manifest, extra: 1 }, "1.2.3"), [
    "latest.json has the keys extra,notes,platforms,pub_date,version, not notes, platforms, pub_date and version",
  ]);
});

// ---- release.yml ----

/** The line's text before a comment (a `#` at its start or after a space, outside quotes). */
function stripComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    if (quote !== null) {
      if (c === quote) {
        quote = null;
      }
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === "#" && (i === 0 || line[i - 1] === " ")) {
      return line.slice(0, i);
    }
  }
  return line;
}

const indentOf = (line) => line.length - line.trimStart().length;

/**
 * Every rule <text> (a workflow) breaks, as `line N: what`. Each line gets its path of keys from
 * the indentation (`-` for a list item); a block scalar's lines belong to its key.
 */
function lintWorkflow(text) {
  const problems = [];
  const stack = [];
  const jobs = new Map();
  let block = null;
  const inStep = (path) => path[0] === "jobs" && path[2] === "steps" && path.length > 3;
  const checkSecrets = (n, path, line) => {
    // Any mention: `secrets.X`, `secrets[...]`, `secrets: inherit`, `toJSON(secrets)`.
    const secret = /\bsecrets\b/.test(line);
    if (secret && !inStep(path)) {
      problems.push(`line ${n}: a secret outside a step (${path.join(".") || "top level"})`);
    }
    // What the step holds, for the sidecar rule below.
    const step = inStep(path) ? jobs.get(path[1])?.steps.at(-1) : undefined;
    if (step !== undefined) {
      step.secret ||= secret;
      if (/\bsidecars\.mjs\b/.test(line)) {
        step.sidecars ??= n;
      }
    }
  };
  text.split(/\r?\n/).forEach((raw, index) => {
    const n = index + 1;
    if (block !== null) {
      if (raw.trim() === "" || indentOf(raw) > block.indent) {
        checkSecrets(n, block.path, raw);
        return;
      }
      block = null;
    }
    const line = stripComment(raw);
    if (line.trim() === "") {
      return;
    }
    let indent = indentOf(line);
    let rest = line.slice(indent);
    while (stack.length > 0 && stack.at(-1).indent >= indent) {
      stack.pop();
    }
    const startsStep =
      stack.length === 3 && stack[0].key === "jobs" && stack[2].key === "steps" && rest.startsWith("- ");
    while (rest.startsWith("- ")) {
      stack.push({ indent, key: "-" });
      indent += 2;
      rest = rest.slice(2);
    }
    const match = rest.match(/^("[^"]*"|'[^']*'|[^\s:]+)\s*:(?:\s+(.*))?$/);
    const key = match ? match[1].replace(/^["']|["']$/g, "") : null;
    const value = match ? (match[2] ?? "").trim() : rest.trim();
    const parents = stack.map((entry) => entry.key);
    const path = key === null ? parents : [...parents, key];
    if (key !== null) {
      stack.push({ indent, key });
    }
    if (path[0] === "on" && /\bpull_request(_target)?\b/.test(rest)) {
      problems.push(`line ${n}: a pull_request or pull_request_target trigger`);
    }
    // `uses:` as a key or inside a flow mapping (`- { uses: x@v4 }`).
    for (const [, quoted] of rest.matchAll(/(?:^|[\s{,])uses\s*:\s*("[^"]*"|'[^']*'|[^\s,}]+)/g)) {
      const ref = quoted.replace(/^["']|["']$/g, "");
      if (!ref.startsWith("./") && !/^[^@\s]+@[0-9a-f]{40}$/.test(ref)) {
        problems.push(`line ${n}: ${ref} is not pinned by a 40-character commit SHA`);
      }
    }
    // Each step's action and its `with:` name or pattern, for the artifact rule below.
    if (path[0] === "jobs" && path[2] === "steps" && jobs.has(path[1])) {
      const job = jobs.get(path[1]);
      if (startsStep) {
        job.steps.push({ uses: "", name: null, pattern: null, secret: false, sidecars: null });
      }
      const step = job.steps.at(-1);
      if (step !== undefined && path.length === 5 && key === "uses") {
        step.uses = value;
      }
      if (step !== undefined && path.length === 6 && path[4] === "with" && (key === "name" || key === "pattern")) {
        step[key] = { value: value.replace(/^["']|["']$/g, ""), line: n };
      }
    }
    if (path[0] === "jobs" && path.length === 2 && key !== null) {
      jobs.set(key, { line: n, permissions: false, steps: [] });
    }
    if (path[0] === "jobs" && path.length === 3 && key === "permissions") {
      jobs.get(path[1]).permissions = true;
    }
    checkSecrets(n, path, rest);
    if (key !== null && /^[|>][-+0-9]*$/.test(value)) {
      block = { indent, path };
    }
  });
  for (const [name, job] of jobs) {
    if (!job.permissions) {
      problems.push(`line ${job.line}: job ${name} has no permissions block`);
    }
    // The sidecar script runs cargo build scripts and fetches: no secret may be present yet.
    job.steps.forEach((step, i) => {
      if (step.sidecars !== null && job.steps.slice(0, i + 1).some((one) => one.secret)) {
        problems.push(`line ${step.sidecars}: job ${name} runs sidecars.mjs with a secret in that step or before it`);
      }
    });
    // A job's download must not pick up what the same job uploads: a re-run of the job would
    // download its own earlier upload. An expression in a name or pattern stands for anything.
    const action = (step, what) => step.uses.startsWith(`actions/${what}-artifact@`);
    const uploads = job.steps.filter((step) => action(step, "upload") && step.name !== null);
    for (const step of job.steps.filter((one) => action(one, "download"))) {
      const wanted = step.pattern ?? step.name;
      const glob = new RegExp(
        `^${(wanted?.value ?? "*")
          .split(/(\$\{\{.*?\}\}|\*)/)
          .map((part, i) => (i % 2 === 1 ? ".*" : part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")))
          .join("")}$`,
      );
      for (const upload of uploads) {
        if (glob.test(upload.name.value.replace(/\$\{\{.*?\}\}/g, "x"))) {
          const what = wanted?.value ?? "every artifact";
          problems.push(
            `line ${wanted?.line ?? upload.name.line}: job ${name} downloads ${what}, ` +
              `which matches its own upload ${upload.name.value} (line ${upload.name.line})`,
          );
        }
      }
    }
  }
  return problems;
}

test("release.yml keeps the workflow rules", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  assert.deepEqual(lintWorkflow(text), []);
  // The lint read the jobs: each name the release depends on is there.
  for (const job of ["check", "blocklist", "build", "sign-windows", "assemble", "publish", "measure-builds"]) {
    assert.match(text, new RegExp(`^  ${job}:`, "m"), job);
  }
});

test("release.yml's arm64 install smoke runs after sign-windows on windows-11-arm, and blocks nothing yet", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const smoke = jobCommands(text, "arm64-install-smoke");
  assert.ok(smoke.includes("needs: [check, sign-windows]"), smoke.join("\n"));
  assert.ok(smoke.includes("runs-on: windows-11-arm"));
  // Non-blocking until three passing runs (the orchestrator's call): it may fail, and no job,
  // assemble included, needs it.
  assert.ok(smoke.includes("continue-on-error: true"));
  assert.ok(jobCommands(text, "assemble").includes("needs: [check, build, sign-windows]"));
  const needers = text.split("\n").filter((line) => /^\s+needs:.*\barm64-install-smoke\b/.test(stripComment(line)));
  assert.deepEqual(needers, []);
  // x64 first, then arm64 over it, then the registered silent uninstall; ARM64 is checked.
  const script = smoke.join("\n");
  const order = ["_x64-machine-setup.exe", "_arm64-machine-setup.exe", "0xAA64", "QuietUninstallString"];
  const at = order.map((part) => script.indexOf(part));
  assert.ok(at.every((i, n) => i !== -1 && (n === 0 || at[n - 1] < i)), JSON.stringify(at));
});

/** A job's lines without comments, each `\`-continued command joined into one line. */
function jobCommands(text, job) {
  const body = text.split(/^  ([a-z0-9-]+):\s*$/m);
  const at = body.indexOf(job);
  assert.notEqual(at, -1, job);
  return body[at + 1]
    .replace(/\\\n\s*/g, " ")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => !line.startsWith("#"));
}

/** Every `tauri build` or `tauri bundle` in <job> that does not pass the sidecar overlay last. */
function overlayNotLast(text, job) {
  return jobCommands(text, job)
    .filter((line) => /\btauri (build|bundle)\b/.test(line))
    .filter((line) => !/--config release\/sidecars\.generated\.json( -- --locked)?$/.test(line));
}

test("release.yml's shipped builds run sidecars.mjs first and pass its overlay last", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  for (const job of ["build", "sign-windows"]) {
    const commands = jobCommands(text, job).filter((line) => /\btauri (build|bundle)\b/.test(line));
    assert.ok(commands.length > 0, job);
    assert.deepEqual(overlayNotLast(text, job), [], job);
  }
  const build = jobCommands(text, "build");
  const seam = build.findIndex((line) => line.startsWith("node .github/scripts/sidecars.mjs"));
  assert.ok(seam !== -1 && seam < build.findIndex((line) => /\btauri build\b/.test(line)), "the seam runs before tauri build");
  const broken = text.replace(/ --config release\/sidecars\.generated\.json -- --locked/, " -- --locked");
  assert.equal(overlayNotLast(broken, "build").length, 1);
  const early = text.replace(
    '--config "$RUNNER_TEMP/updater-key.conf.json" --config release/sidecars.generated.json',
    '--config release/sidecars.generated.json --config "$RUNNER_TEMP/updater-key.conf.json"',
  );
  assert.notEqual(early, text);
  assert.equal(overlayNotLast(early, "build").length, 1);
});

/** release.yml's SANDBOX_EXE, the sandbox tool's name in the install folder. */
function sandboxExe(text) {
  const match = text.match(/^env:\n(?:  .*\n)*?  SANDBOX_EXE: (\S+)$/m);
  assert.ok(match, "release.yml's top-level env sets SANDBOX_EXE");
  return match[1];
}

test("release.yml names the sandbox tool once, and latest-json.mjs publishes it per architecture, signed", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const exe = sandboxExe(text);
  assert.match(exe, /^clauseo-sandbox-[a-z]+\.exe$/);
  const code = text
    .split("\n")
    .map(stripComment)
    .filter((line) => !/^ {2}SANDBOX_EXE: /.test(line))
    .join("\n");
  assert.equal(code.match(/clauseo-sandbox-[a-z]+/g), null, "the name is written only in SANDBOX_EXE");
  const base = exe.replace(/\.exe$/, "");
  const rows = assets("1.2.3", exe).filter((asset) => asset.signed);
  assert.deepEqual(rows, [
    { name: `${base}-x64.exe`, tier: 1, signed: true },
    { name: `${base}-arm64.exe`, tier: 1, signed: true },
  ]);
  assert.equal(sandboxAsset(exe, "x64"), `${base}-x64.exe`);
  // No two assets (or their .sig) share a name.
  const names = assets("1.2.3", exe).flatMap((a) => (a.platforms || a.signed ? [a.name, `${a.name}.sig`] : [a.name]));
  assert.equal(new Set(names).size, names.length);
  assert.throws(() => sandboxAsset("Setup.EXE", "x64"));

  const dir = release("1.2.3");
  try {
    const sig = fakeSig("timestamp:1793641889\tfile:x\tversion:1.2.3");
    // Both architectures are tier 1.
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE, exe).problems, [
      `${base}-x64.exe is missing (tier 1)`,
      `${base}-arm64.exe is missing (tier 1)`,
    ]);
    writeFileSync(join(dir, `${base}-arm64.exe`), "exe");
    writeFileSync(join(dir, `${base}-arm64.exe.sig`), sig);
    writeFileSync(join(dir, `${base}-x64.exe`), "exe");
    // Each needs its .sig.
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE, exe).problems, [`${base}-x64.exe.sig is missing`]);
    writeFileSync(join(dir, `${base}-x64.exe.sig`), fakeSig("timestamp:1793641889\tfile:x\tversion:1.2.2"));
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE, exe).problems, [
      `${base}-x64.exe.sig: the signature is for version 1.2.2, not 1.2.3`,
    ]);
    writeFileSync(join(dir, `${base}-x64.exe.sig`), sig);
    const { manifest, missing, problems } = buildManifest(dir, "1.2.3", BASE, DATE, exe);
    assert.deepEqual(problems, []);
    assert.deepEqual(missing, []);
    // The updater never sees it.
    assert.deepEqual(manifest, buildManifest(dir, "1.2.3", BASE, DATE).manifest);
    rmSync(join(dir, `${base}-x64.exe`));
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE, exe).problems, [`${base}-x64.exe is missing (tier 1)`]);
    writeFileSync(join(dir, `${base}-x64.exe`), "exe");
    rmSync(join(dir, `${base}-arm64.exe`));
    assert.deepEqual(buildManifest(dir, "1.2.3", BASE, DATE, exe).problems, [`${base}-arm64.exe is missing (tier 1)`]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// A real pair from `tauri signer generate` and `tauri signer sign --app-version 1.2.3` (CLI 2.12.2)
// over REAL_DATA; the private key was thrown away.
const REAL_DATA = "clauseo fixture bytes\n";
const REAL_PUB =
  "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENGN0M3MjJFQUY3M0ZDOTEKUldTUi9IT3ZMbko4ejhKUlRyNHdzZGovQm14azliMDRJVUJ6cGRlNURyK2dhZ21uZi9kVFdsSHYK";
const REAL_SIG =
  "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTUi9IT3ZMbko4ejk0bWlyMitUY1NFUHgwOE1pN044cjRWcVVXVGdVYktCbVZoTVZYSi9zWGZQUE1RdnF6ZU1NaGZ1c0tGRVVlL0hjVW9MVkk5N21wSTI4NDkzbXVIWWdjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0MDQ2Nzc2CWZpbGU6ZGF0YS5leGUJdmVyc2lvbjoxLjIuMwpEZ29GYXJMbkMvYzJEWllPR3BUei9tUVl3cWx3S085OUlCY2Nlak92YkJWYmN6UmdUbmxXVmpBUkhTcFZGdlluSGZhRXJzZUY0V2NWbm9yQ0tza0REQT09Cg==";
const b64 = (text) => Buffer.from(text).toString("base64");
const reSig = (sig, edit) => b64(edit(Buffer.from(sig, "base64").toString("utf8").split("\n")).join("\n"));

test("signatureCheck holds for tauri signer's signature and fails each way it can be wrong", () => {
  const data = Buffer.from(REAL_DATA);
  assert.deepEqual(signatureCheck(data, REAL_SIG, REAL_PUB), []);
  assert.equal(signedVersion(REAL_SIG), "1.2.3");
  assert.deepEqual(signatureCheck(Buffer.from("clauseo fixture bytez\n"), REAL_SIG, REAL_PUB), [
    "the signature does not match the file",
  ]);
  const forged = reSig(REAL_SIG, (l) => [l[0], l[1], l[2].replace("version:1.2.3", "version:9.9.9"), l[3]]);
  assert.deepEqual(signatureCheck(data, forged, REAL_PUB), ["the trusted comment is not signed by the key"]);
  // Another key with the same key id, and another key id.
  const pub = Buffer.from(REAL_PUB, "base64").toString("utf8").split("\n");
  const key = Buffer.from(pub[1], "base64");
  const otherKey = Buffer.concat([key.subarray(0, 10), Buffer.alloc(32, 7)]);
  assert.notDeepEqual(signatureCheck(data, REAL_SIG, b64(`${pub[0]}\n${otherKey.toString("base64")}\n`)), []);
  const otherId = Buffer.concat([key.subarray(0, 2), Buffer.alloc(8, 1), key.subarray(10)]);
  assert.deepEqual(signatureCheck(data, REAL_SIG, b64(`${pub[0]}\n${otherId.toString("base64")}\n`)), [
    "the signature is by another key than the public key",
  ]);
  assert.deepEqual(signatureCheck(data, REAL_SIG, ""), ["the public key is not a minisign Ed25519 key"]);
  assert.deepEqual(signatureCheck(data, fakeSig("x"), REAL_PUB), ["the signature is by another key than the public key"]);
  assert.deepEqual(signatureCheck(data, b64("nothing"), REAL_PUB), ["the signature is not minisign's text"]);
});

test("signatureCheck holds the trusted comment to the signed name and version when asked", () => {
  const data = Buffer.from(REAL_DATA);
  // REAL_SIG's comment: timestamp:1794046776\tfile:data.exe\tversion:1.2.3
  assert.deepEqual(signatureCheck(data, REAL_SIG, REAL_PUB, { file: "data.exe", version: "1.2.3" }), []);
  assert.deepEqual(signatureCheck(data, REAL_SIG, REAL_PUB, { file: "clauseo-sandbox-setup.exe", version: "1.2.3" }), [
    "the trusted comment holds no file:clauseo-sandbox-setup.exe",
  ]);
  assert.deepEqual(signatureCheck(data, REAL_SIG, REAL_PUB, { file: "data", version: "1.2" }), [
    "the trusted comment holds no file:data",
    "the trusted comment holds no version:1.2",
  ]);
  // The CLI: the sandbox tool's .sig, signed as SANDBOX_EXE, as assemble checks it.
  const dir = mkdtempSync(join(tmpdir(), "latest-json-verify-"));
  try {
    writeFileSync(join(dir, "tool-x64.exe"), REAL_DATA);
    writeFileSync(join(dir, "tool-x64.exe.sig"), REAL_SIG);
    writeFileSync(join(dir, "key.pub"), REAL_PUB);
    const run = (...args) =>
      spawnSync(process.execPath, [join(here, "latest-json.mjs"), "--verify", join(dir, "tool-x64.exe"), join(dir, "key.pub"), ...args], {
        encoding: "utf8",
      });
    assert.equal(run().status, 0);
    assert.equal(run("data.exe", "1.2.3").status, 0);
    const wrong = run("tool-x64.exe", "1.2.3");
    assert.equal(wrong.status, 1);
    assert.match(wrong.stderr, /the trusted comment holds no file:tool-x64\.exe/);
    assert.equal(run("data.exe").status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ---- the sandbox tool's UpdaterKeyId ----

const TEST_PUB = readFileSync(join(here, "..", "..", "crates", "desktop", "release", "updater-test.pub"), "utf8");
const CONFIG_PUB = JSON.parse(readFileSync(join(here, "..", "..", "crates", "desktop", "tauri.conf.json"), "utf8")).plugins
  .updater.pubkey;

/** One VERSIONINFO node: header, NUL-ended UTF-16 key, padding, value (text or bytes), padding, children. */
function versionNode(key, { text = null, bytes = Buffer.alloc(0), children = [] } = {}) {
  const pad = (b) => Buffer.concat([b, Buffer.alloc((4 - (b.length % 4)) % 4)]);
  const value = text === null ? bytes : Buffer.from(`${text}\0`, "utf16le");
  const head = Buffer.alloc(6);
  head.writeUInt16LE(text === null ? bytes.length : text.length + 1, 2);
  head.writeUInt16LE(text === null ? 0 : 1, 4);
  const body = Buffer.concat([pad(Buffer.concat([head, Buffer.from(`${key}\0`, "utf16le")])), pad(value), ...children.map(pad)]);
  body.writeUInt16LE(body.length, 0);
  return body;
}

/** A PE32+ file with one .rsrc section holding RT_VERSION/1/0x409 with <strings>, or no resources. */
function fakePe(strings) {
  const VA = 0x1000;
  const RAW = 0x200;
  let rsrc = Buffer.alloc(0);
  if (strings !== null) {
    const fixed = Buffer.alloc(52);
    fixed.writeUInt32LE(0xfeef04bd, 0);
    const info = versionNode("VS_VERSION_INFO", {
      bytes: fixed,
      children: [
        versionNode("StringFileInfo", {
          children: [
            versionNode("040904B0", {
              children: Object.entries(strings).map(([key, text]) => versionNode(key, { text })),
            }),
          ],
        }),
      ],
    });
    // Three directories of one entry each (16 + 8 bytes), a data entry (16), then the data.
    const dir = (id, target, subdirectory) => {
      const b = Buffer.alloc(24);
      b.writeUInt16LE(1, 14);
      b.writeUInt32LE(id, 16);
      b.writeUInt32LE((target | (subdirectory ? 0x80000000 : 0)) >>> 0, 20);
      return b;
    };
    const entry = Buffer.alloc(16);
    entry.writeUInt32LE(VA + 88, 0);
    entry.writeUInt32LE(info.length, 4);
    rsrc = Buffer.concat([dir(16, 24, true), dir(1, 48, true), dir(0x409, 72, false), entry, info]);
  }
  const header = Buffer.alloc(RAW);
  header.write("MZ", 0, "latin1");
  header.writeUInt32LE(0x40, 0x3c);
  header.write("PE\0\0", 0x40, "latin1");
  header.writeUInt16LE(0x8664, 0x44);
  header.writeUInt16LE(1, 0x46);
  header.writeUInt16LE(240, 0x54);
  const optional = 0x58;
  header.writeUInt16LE(0x20b, optional);
  header.writeUInt32LE(16, optional + 108);
  header.writeUInt32LE(strings === null ? 0 : VA, optional + 112 + 16);
  header.writeUInt32LE(rsrc.length, optional + 112 + 20);
  const section = optional + 240;
  header.write(".rsrc", section, "latin1");
  header.writeUInt32LE(rsrc.length, section + 8);
  header.writeUInt32LE(VA, section + 12);
  header.writeUInt32LE(rsrc.length, section + 16);
  header.writeUInt32LE(RAW, section + 20);
  return Buffer.concat([header, rsrc]);
}

test("publicKeyId prints a key's ID as minisign does, and expectedKeyId picks each mode's", () => {
  // Each against the ID minisign wrote in the key's own untrusted comment.
  for (const pub of [REAL_PUB, TEST_PUB]) {
    const comment = Buffer.from(pub.trim(), "base64").toString("utf8").split("\n")[0];
    assert.equal(publicKeyId(pub), comment.split(" ").at(-1));
  }
  assert.equal(publicKeyId(TEST_PUB), "B142942ACEC5F521");
  assert.equal(publicKeyId(REAL_PUB), "CF7C722EAF73FC91");
  assert.equal(publicKeyId(""), null);
  assert.equal(expectedKeyId("test", "", TEST_PUB), "B142942ACEC5F521");
  assert.equal(expectedKeyId("release", REAL_PUB, TEST_PUB), "CF7C722EAF73FC91");
  assert.throws(() => expectedKeyId("release", "", TEST_PUB), /plugins\.updater\.pubkey/);
  // dry: tauri.conf.json's key when it has one (build.rs's fallback), else build.rs's placeholder.
  assert.equal(expectedKeyId("dry", REAL_PUB, TEST_PUB), "CF7C722EAF73FC91");
  assert.equal(expectedKeyId("dry", "", TEST_PUB), PLACEHOLDER_KEY_ID);
  assert.equal(PLACEHOLDER_KEY_ID, "AC986E7C6BA67AB9");
  assert.throws(() => expectedKeyId("nightly", "", TEST_PUB));
});

test("versionString reads a VERSIONINFO string through the PE's resources", () => {
  const pe = fakePe({ ProductName: "Clauseo", UpdaterKeyId: "B142942ACEC5F521" });
  assert.equal(versionString(pe, "UpdaterKeyId"), "B142942ACEC5F521");
  assert.equal(versionString(pe, "ProductName"), "Clauseo");
  assert.equal(versionString(pe, "FileVersion"), null);
  assert.equal(versionString(fakePe(null), "UpdaterKeyId"), null);
  // The bytes alone are no UpdaterKeyId: a string outside the resources is not read.
  assert.equal(versionString(Buffer.concat([fakePe(null), Buffer.from("UpdaterKeyId B142942ACEC5F521", "utf16le")]), "UpdaterKeyId"), null);
  assert.throws(() => versionString(Buffer.from("not a PE file"), "UpdaterKeyId"), /not a readable PE file/);
  assert.throws(() => versionString(pe.subarray(0, pe.length - 40), "UpdaterKeyId"), /not a readable PE file/);
});

test("--key-id passes a matching UpdaterKeyId, fails another and exits 3 on none, which assemble fails", () => {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-key-id-"));
  const run = (strings, mode) => {
    const exe = join(dir, "tool.exe");
    writeFileSync(exe, fakePe(strings));
    return spawnSync(process.execPath, [join(here, "latest-json.mjs"), "--key-id", exe, mode], { encoding: "utf8" });
  };
  try {
    assert.equal(run({ UpdaterKeyId: "B142942ACEC5F521" }, "test").status, 0);
    const other = run({ UpdaterKeyId: "CF7C722EAF73FC91" }, "test");
    assert.equal(other.status, 1);
    assert.match(other.stderr, /UpdaterKeyId CF7C722EAF73FC91, not the test key B142942ACEC5F521/);
    const none = run({ ProductName: "Clauseo" }, "test");
    assert.equal(none.status, 3);
    assert.match(none.stderr, /no UpdaterKeyId/);
    assert.equal(run(null, "test").status, 3);
    // dry, against whichever key tauri.conf.json holds today (empty: the placeholder).
    const dry = expectedKeyId("dry", CONFIG_PUB, TEST_PUB);
    assert.equal(run({ UpdaterKeyId: dry }, "dry").status, 0);
    assert.equal(run({ UpdaterKeyId: "B142942ACEC5F521" }, "dry").status, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("release.yml signs the sandbox tool, ships its .sig in both installers and checks both in assemble", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const at = (commands, line) => {
    const index = commands.indexOf(line);
    assert.notEqual(index, -1, line);
    return index;
  };
  const sign = jobCommands(text, "sign-windows");
  // Signed as SANDBOX_EXE (the trusted comment's file:), its .sig moved where the overlay reads it.
  const signed = at(sign, 'pnpm -C ui tauri signer sign --app-version "$VERSION" "$dir/$SANDBOX_EXE"');
  assert.ok(at(sign, 'cp "$exe" "$dir/$SANDBOX_EXE"') < signed);
  assert.ok(signed < at(sign, 'mv "$dir/$SANDBOX_EXE.sig" "$exe.sig"'));
  const bundled = sign.findIndex((line) => /\btauri bundle\b/.test(line));
  assert.ok(signed < bundled, "the sandbox tool is signed before NSIS takes it");
  assert.ok(sign[bundled].includes('--config "$RUNNER_TEMP/sandbox-sig.conf.json" --config release/sidecars.generated.json'));
  at(sign, 'cp "crates/desktop/$sig" "$OUT/$asset.sig"');
  const assemble = jobCommands(text, "assemble");
  const verified = at(assemble, 'node .github/scripts/latest-json.mjs --verify "$exe" "$pub" "$SANDBOX_EXE" "$VERSION" ||');
  at(assemble, 'node .github/scripts/latest-json.mjs --verify "${files[0]}" "$pub" "$(basename "${files[0]}")" "$VERSION" ||');
  const listed = at(assemble, 'grep -qxF "$inner" <<< "$listing" ||');
  const same = at(assemble, '7z e -so -spd "$installer" "$inner" | cmp - "$published" ||');
  const keyId = at(assemble, 'node .github/scripts/latest-json.mjs --key-id "$exe" "$MODE" || status=$?');
  assert.ok(verified < keyId && keyId < listed, "the key ID is checked with the signatures");
  // An exe with no UpdaterKeyId (--key-id's 3) fails the job like a wrong one: no warning case.
  assert.doesNotMatch(text, /TODO\(sandbox-admin-refresh\)/);
  assert.ok(!assemble.some((line) => line.includes("has no UpdaterKeyId yet")), "no step-summary warning for a missing ID");
  const missing = assemble.find((line) => line.startsWith("3)"));
  assert.ok(missing, "assemble handles --key-id's 3");
  assert.match(missing, /^3\) echo "::error::.* has no UpdaterKeyId, .*"; exit 1 ;;$/);
  assert.ok(keyId < assemble.indexOf(missing) && assemble.indexOf(missing) < listed);
  const notes = at(assemble, 'echo "## Inside the Windows installers"');
  at(assemble, "printf '%s\\n' \"${inside[@]}\"");
  assert.ok(verified < listed && listed < same && same < notes);
  assert.ok(assemble.some((line) => line.includes('"$SANDBOX_EXE" > missing.txt')), "latest-json.mjs gets SANDBOX_EXE");
  // apt-get in assemble and blocklist (minisign) only.
  for (const job of ["check", "ui", "build", "sign-windows", "publish", "measure-builds"]) {
    assert.deepEqual(jobCommands(text, job).filter((line) => /\bapt-get\b/.test(line)), [], job);
  }
});

test("release.yml signs each macOS archive under its arch name in the build and checks it in assemble", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const at = (commands, line, from = 0) => {
    const index = commands.indexOf(line, from);
    assert.notEqual(index, -1, line);
    return index;
  };
  const build = jobCommands(text, "build");
  const bundled = build.findIndex((line) => /\btauri bundle\b.*--bundles app,dmg/.test(line));
  assert.notEqual(bundled, -1, "the macOS tauri bundle");
  // In the bundle's step (the one that holds the key), after the bundle: rename, drop the
  // bundler's .sig (file:Clauseo.app.tar.gz), sign again; then collect the re-signed pair.
  const named = at(build, "tarball=Clauseo_${VERSION}_${{ matrix.arch }}.app.tar.gz", bundled);
  const moved = at(build, 'mv "$macos/Clauseo.app.tar.gz" "$macos/$tarball"', named);
  const dropped = at(build, 'rm "$macos/Clauseo.app.tar.gz.sig"', moved);
  // By its absolute path: pnpm -C ui runs the signer in ui/, where "$macos/..." doesn't resolve.
  const signed = at(build, 'pnpm -C ui tauri signer sign --app-version "$VERSION" "$PWD/$macos/$tarball"', dropped);
  const guard = build.lastIndexOf('[ -n "$TAURI_SIGNING_PRIVATE_KEY" ] || { echo "::error::no updater key for a $MODE run"; exit 1; }', bundled);
  assert.ok(guard !== -1 && build.slice(guard, signed).every((line) => !line.startsWith("- name:")), "the key guard is in the signing step");
  const collected = at(build, 'cp "$bundle/macos/$tarball" "$bundle/macos/$tarball.sig" "$OUT/"', signed);
  assert.ok(build.slice(signed, collected).some((line) => line.startsWith("- name: Collect")));
  assert.ok(!build.some((line) => line.includes('cp "$bundle/macos/Clauseo.app.tar.gz')), "no bundler-named archive is collected");
  // A dry run's macOS legs publish their throwaway public key for assemble.
  assert.ok(build.includes("name: dry-updater-pub-${{ matrix.target }}"));
  const assemble = jobCommands(text, "assemble");
  assert.ok(!assemble.some((line) => /^mv .*Clauseo\.app\.tar\.gz/.test(line)), "assemble renames no archive");
  at(assemble, "for old in Clauseo.app.tar.gz Clauseo.app.tar.gz.sig; do");
  at(assemble, 'node .github/scripts/latest-json.mjs --verify "$tarball" "$pub" "$(basename "$tarball")" "$VERSION" ||');
  at(assemble, "tarball=dist/Clauseo_${VERSION}_${arch}.app.tar.gz");
});

test("release.yml's build tells the sandbox's build.rs the mode and, for release and test, the updater key", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const build = jobCommands(text, "build");
  assert.ok(build.includes("CLAUSEO_RELEASE_MODE: ${{ needs.check.outputs.mode }}"), "CLAUSEO_RELEASE_MODE in build's env");
  const written = build.indexOf('echo "CLAUSEO_UPDATER_PUBKEY=$key" >> "$GITHUB_ENV"');
  const sidecars = build.findIndex((line) => line.startsWith("node .github/scripts/sidecars.mjs"));
  assert.ok(written !== -1 && written < sidecars, "CLAUSEO_UPDATER_PUBKEY is set before sidecars.mjs builds the sandbox tool");
  assert.ok(build.includes("release) key=$(jq -r .plugins.updater.pubkey crates/desktop/tauri.conf.json) ;;"));
  // Both branches give plugins.updater.pubkey's format: release reads that field, test the file
  // as stored, which already is that format (one base64 line of the whole .pub text).
  assert.ok(build.includes("test) key=$(tr -d '\\r\\n' < crates/desktop/release/updater-test.pub) ;;"));
  const stored = readFileSync(join(here, "..", "..", "crates", "desktop", "release", "updater-test.pub"), "utf8").trim();
  assert.match(stored, /^[A-Za-z0-9+/]+=*$/, "updater-test.pub is one base64 line");
  const pubFormat = (key) => {
    const lines = Buffer.from(key, "base64").toString("utf8").split("\n");
    return /^untrusted comment: minisign public key: [0-9A-F]{16}$/.test(lines[0]) && /^RW[A-Za-z0-9+/]{54}$/.test(lines[1]);
  };
  assert.ok(pubFormat(stored), "updater-test.pub decodes to a minisign public key file");
  // The same format tauri signer writes and tauri.conf.json holds (REAL_PUB is a .pub as written).
  assert.ok(pubFormat(REAL_PUB));
  const conf = JSON.parse(readFileSync(join(here, "..", "..", "crates", "desktop", "tauri.conf.json"), "utf8"));
  const release = conf.plugins.updater.pubkey;
  assert.ok(release === "" || pubFormat(release), "tauri.conf.json's pubkey, once set, is that format");
});

/**
 * The jobs that pass release.conf.json, and every way one of them leaves out the block-list
 * snapshot: a line passing release.conf.json without blocklist.conf.json, no `blocklist` in its
 * `needs:`, or no blocklist-snapshot download before its first `tauri build` or `tauri bundle`.
 */
function snapshotProblems(text) {
  const checked = [];
  const problems = [];
  for (const [, job] of text.matchAll(/^  ([a-z-]+):\s*$/gm)) {
    const lines = jobCommands(text, job);
    const release = lines.filter((line) => line.includes("--config release/release.conf.json"));
    if (release.length === 0) {
      continue;
    }
    checked.push(job);
    for (const line of release.filter((one) => !one.includes("--config release/blocklist.conf.json"))) {
      problems.push(`${job}: ${line}`);
    }
    if (!/^needs:.*\bblocklist\b/.test(lines.find((line) => line.startsWith("needs:")) ?? "")) {
      problems.push(`${job}: no blocklist in needs`);
    }
    const download = lines.indexOf("name: blocklist-snapshot");
    const first = lines.findIndex((line) => /\btauri (build|bundle)\b/.test(line));
    if (download === -1 || download > first) {
      problems.push(`${job}: no blocklist-snapshot download before its first tauri build or bundle`);
    }
  }
  return { checked, problems };
}

test("release.yml's builds with release.conf.json take the verified block-list snapshot", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  assert.deepEqual(snapshotProblems(text), { checked: ["build", "sign-windows", "measure-builds"], problems: [] });
  const broken = (from, to) => {
    const changed = text.replace(from, to);
    assert.notEqual(changed, text, from);
    return snapshotProblems(changed).problems;
  };
  assert.equal(broken(/(--config release\/release\.conf\.json) --config release\/blocklist\.conf\.json --no-bundle/, "$1 --no-bundle").length, 1);
  assert.deepEqual(broken("needs: [check, build, blocklist]", "needs: [check, build]"), ["sign-windows: no blocklist in needs"]);
  assert.deepEqual(broken(/( {6}# The same verified snapshot)[^]*?path: crates\/desktop\/blocklist\n/, ""), [
    "sign-windows: no blocklist-snapshot download before its first tauri build or bundle",
  ]);
});

test("the workflow lint fails each broken rule", () => {
  const good = [
    "on:",
    "  push:",
    '    tags: ["v*"]',
    "permissions: {}",
    "jobs:",
    "  one:",
    "    permissions:",
    "      contents: read",
    "    runs-on: ubuntu-24.04",
    "    steps:",
    "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1",
    "      - uses: ./.github/actions/install-tauri-linux-deps",
    "      - name: Sign",
    "        env:",
    "          KEY: ${{ secrets.KEY }}",
    "        run: |",
    "          echo \"${{ secrets.OTHER }}\"",
    "",
  ].join("\n");
  assert.deepEqual(lintWorkflow(good), []);
  const broken = (from, to) => lintWorkflow(good.replace(from, to));
  assert.deepEqual(broken("  push:", "  pull_request:\n  push:"), [
    "line 2: a pull_request or pull_request_target trigger",
  ]);
  assert.deepEqual(broken("on:\n", "on: [push, pull_request_target]\nx:\n"), [
    "line 1: a pull_request or pull_request_target trigger",
  ]);
  assert.deepEqual(broken("@3d3c42e5aac5ba805825da76410c181273ba90b1", "@v7"), [
    "line 11: actions/checkout@v7 is not pinned by a 40-character commit SHA",
  ]);
  assert.deepEqual(broken("    permissions:\n      contents: read\n", ""), ["line 6: job one has no permissions block"]);
  assert.deepEqual(broken("    runs-on:", "    env:\n      KEY: ${{ secrets.KEY }}\n    runs-on:"), [
    "line 10: a secret outside a step (jobs.one.env.KEY)",
  ]);
  assert.deepEqual(broken("permissions: {}", "env:\n  KEY: ${{ secrets.KEY }}"), [
    "line 5: a secret outside a step (env.KEY)",
  ]);
  assert.deepEqual(broken("    runs-on:", "    secrets: inherit\n    runs-on:"), [
    "line 9: a secret outside a step (jobs.one.secrets)",
  ]);
  assert.deepEqual(broken("    runs-on:", "    env:\n      ALL: ${{ toJSON(secrets) }}\n    runs-on:"), [
    "line 10: a secret outside a step (jobs.one.env.ALL)",
  ]);
  assert.deepEqual(broken("      - name: Sign", "      - { uses: actions/checkout@v4 }\n      - name: Sign"), [
    "line 13: actions/checkout@v4 is not pinned by a 40-character commit SHA",
  ]);
  // A job that downloads release-* and uploads under a name the pattern does not match passes;
  // an upload the pattern matches (a re-run would download it) fails, expressions included.
  const artifacts = [
    "      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2",
    "        with:",
    "          pattern: release-*",
    "      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2",
    "        with:",
    "          name: dist-assets",
    "      - name: Sign",
  ].join("\n");
  assert.deepEqual(broken("      - name: Sign", artifacts), []);
  assert.deepEqual(broken("      - name: Sign", artifacts.replace("dist-assets", "release-assets")), [
    "line 15: job one downloads release-*, which matches its own upload release-assets (line 18)",
  ]);
  assert.deepEqual(broken("      - name: Sign", artifacts.replace("dist-assets", "release-${{ matrix.target }}")), [
    "line 15: job one downloads release-*, which matches its own upload release-${{ matrix.target }} (line 18)",
  ]);
  // sidecars.mjs runs before any secret: after the signing step, or with a secret of its own, fails.
  const seam = "      - name: Sidecars\n        run: node .github/scripts/sidecars.mjs \"$TARGET\"\n";
  assert.deepEqual(broken("      - name: Sign", `${seam}      - name: Sign`), []);
  assert.deepEqual(lintWorkflow(`${good}${seam}`), [
    "line 19: job one runs sidecars.mjs with a secret in that step or before it",
  ]);
  assert.deepEqual(
    broken("      - name: Sign", `${seam}        env:\n          GH_TOKEN: \${{ secrets.GH }}\n      - name: Sign`),
    ["line 14: job one runs sidecars.mjs with a secret in that step or before it"],
  );
});

// ---- a published release (network; only with LATEST_JSON_RELEASE) ----

const target = process.env.LATEST_JSON_RELEASE ?? "";

test("the release's latest.json is right and every platforms URL downloads", { skip: target === "" }, async () => {
  const match = target.match(/^([\w.-]+\/[\w.-]+)@((?:test-)?v(\d+\.\d+\.\d+))$/);
  assert.ok(match, "LATEST_JSON_RELEASE is <owner>/<repo>@v<X.Y.Z> or @test-v<X.Y.Z>");
  const [, repo, tag, version] = match;
  const base = `https://github.com/${repo}/releases/download/${tag}`;
  const get = async (url) => {
    const response = await fetch(url, { redirect: "follow" });
    assert.ok(response.ok, `${url}: HTTP ${response.status}`);
    return Buffer.from(await response.arrayBuffer());
  };
  const manifest = JSON.parse((await get(`${base}/latest.json`)).toString("utf8"));
  assert.deepEqual(manifestProblems(manifest, version), []);
  const sums = new Map(
    (await get(`${base}/SHA256SUMS.txt`))
      .toString("utf8")
      .trim()
      .split("\n")
      .map((line) => line.match(/^([0-9a-f]{64}) [ *](.+)$/).slice(1, 3).reverse()),
  );
  for (const [key, entry] of Object.entries(manifest.platforms)) {
    assert.ok(entry.url.startsWith(`${base}/`), `${key}: ${entry.url} is not on ${tag}`);
    const name = entry.url.slice(base.length + 1);
    const body = await get(entry.url);
    assert.equal(createHash("sha256").update(body).digest("hex"), sums.get(name), `${name} against SHA256SUMS.txt`);
    const sig = (await get(`${entry.url}.sig`)).toString("utf8").trim();
    assert.equal(sig, entry.signature, `${name}.sig against latest.json`);
  }
});

// release.yml's assemble imports this module from `node --input-type=module -e`, where argv[1] is
// the version, not a path; the run-as-script guard must not throw on it (dry run 42371003314).
test("importing latest-json.mjs with a non-path argv[1] runs nothing and does not throw", () => {
  const run = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `import { assets } from ${JSON.stringify(pathToFileURL(join(here, "latest-json.mjs")).href)}; console.log(assets(process.argv[1], process.argv[2]).length);`,
      "0.0.0",
      "clauseo-sandbox-setup.exe",
    ],
    { encoding: "utf8" },
  );
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /^\d+\n$/);
});
