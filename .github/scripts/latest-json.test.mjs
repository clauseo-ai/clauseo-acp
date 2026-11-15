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
// (SANDBOX_EXE), the Safe workspace switch (SAFE_WORKSPACE, "false" for launch) every step of that
// path is behind, its updater signature before NSIS, its .sig in both installers, assemble's checks
// of the published exe and .sig against both installers and the updater key, assemble's check
// that with the switch off neither installer holds a sandbox file, and the build env its build.rs
// reads; and the block-list switch (check's output bundle_blocklist, 'true' for launch): the
// `blocklist` job behind it, and every job that passes release.conf.json passing
// blocklist.conf.json only through $BLOCKLIST_OVERLAY, which the same switch sets, needing
// `blocklist` and downloading blocklist-snapshot, behind the same switch, before it builds or
// bundles; and every step of `blocklist` but its off-notice behind the switch, with no job-level
// `if:`, so with the switch off it runs as a no-op and every job below it runs as with it on.
// The release notes' "Opening Clauseo the first time" block must say, line by line as GitHub
// shows it, the strings the copy review registers for it (registry-release-notes.tsx) and
// copy-approvals holds, so the published words are the reviewed ones.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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
  for (const job of ["check", "blocklist", "build", "sign-windows", "assemble", "publish", "latest", "measure-builds"]) {
    assert.match(text, new RegExp(`^  ${job}:`, "m"), job);
  }
});

test("release.yml's arm64 install smoke runs after sign-windows on windows-11-arm, and blocks the release", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const smoke = jobCommands(text, "arm64-install-smoke");
  assert.ok(smoke.includes("needs: [check, sign-windows]"), smoke.join("\n"));
  assert.ok(smoke.includes("runs-on: windows-11-arm"));
  // Blocking: it may not fail (no continue-on-error), and assemble, so the release, needs it.
  assert.ok(!smoke.some((line) => line.startsWith("continue-on-error:")), smoke.join("\n"));
  assert.ok(jobCommands(text, "assemble").includes("needs: [check, custody, build, sign-windows, arm64-install-smoke]"));
  const needers = text.split("\n").filter((line) => /^\s+needs:.*\barm64-install-smoke\b/.test(stripComment(line)));
  assert.deepEqual(needers, ["    needs: [check, custody, build, sign-windows, arm64-install-smoke]"]);
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

/** release.yml's top-level SAFE_WORKSPACE, the Safe workspace switch. */
function safeWorkspace(text) {
  const match = text.match(/^env:\n(?:  .*\n)*?  SAFE_WORKSPACE: "(\S+)"$/m);
  assert.ok(match, "release.yml's top-level env sets SAFE_WORKSPACE, quoted");
  return match[1];
}

/** The index of the exact command <line> in <commands>; fails when it is not there. */
function commandAt(commands, line) {
  const index = commands.indexOf(line);
  assert.notEqual(index, -1, line);
  return index;
}

/** Whether <commands>[<index>] is inside an `if [ "$SAFE_WORKSPACE" = true ]; then` block. */
function underSwitchOn(commands, index) {
  let depth = 0;
  for (let i = index - 1; i >= 0; i -= 1) {
    const line = commands[i];
    if (line === "fi") depth += 1;
    else if (/^if .*; then$/.test(line)) {
      if (depth === 0) return line === 'if [ "$SAFE_WORKSPACE" = true ]; then';
      depth -= 1;
    }
  }
  return false;
}

test("release.yml's Safe workspace switch is off for launch and every sandbox tool step is behind it", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  assert.equal(safeWorkspace(text), "false");
  // The seam passes the switch to sidecars.mjs, which builds the sandbox tool only for "true".
  const build = jobCommands(text, "build");
  commandAt(build, 'node .github/scripts/sidecars.mjs --safe-workspace="$SAFE_WORKSPACE" "$TARGET"');
  assert.deepEqual(
    build.filter((line) => line.startsWith("node .github/scripts/sidecars.mjs") && line !== 'node .github/scripts/sidecars.mjs --check "$TARGET"'),
    ['node .github/scripts/sidecars.mjs --safe-workspace="$SAFE_WORKSPACE" "$TARGET"'],
  );
  const sign = jobCommands(text, "sign-windows");
  // The signing step runs only with the switch on.
  const step = sign.indexOf("- name: Sign the sandbox tool for the updater");
  assert.notEqual(step, -1);
  assert.equal(sign[step + 1], "if: env.SAFE_WORKSPACE == 'true'");
  // The bundle overlay with the .sig and hooks_safe_workspace.nsh only with it on; off, empty.
  const hooks = sign.findIndex((line) => line.includes("windows/hooks_safe_workspace.nsh"));
  assert.ok(hooks !== -1 && underSwitchOn(sign, hooks), "hooks_safe_workspace.nsh only with the switch on");
  const empty = commandAt(sign, `echo '{}' > "$RUNNER_TEMP/safe-workspace.conf.json"`);
  assert.equal(sign[empty - 1], "else");
  assert.ok(underSwitchOn(sign, commandAt(sign, 'cp "crates/desktop/$sig" "$OUT/$asset.sig"')));
  // Assemble: the sandbox tool's rows, signature, key ID, installer contents and notes with it on.
  const assemble = jobCommands(text, "assemble");
  const manifest = assemble.filter((line) => line.startsWith("node .github/scripts/latest-json.mjs dist "));
  assert.equal(manifest.length, 2, manifest.join("\n"));
  const withExe = assemble.findIndex((line) => line.endsWith('"$SANDBOX_EXE" > missing.txt'));
  assert.ok(underSwitchOn(assemble, withExe));
  const without = commandAt(assemble, 'node .github/scripts/latest-json.mjs dist "$VERSION" "$base" "$pub_date" > missing.txt');
  assert.equal(assemble[without - 1], "else");
  assert.ok(underSwitchOn(assemble, commandAt(assemble, 'echo "## Inside the Windows installers"')));
  // Off: neither installer may hold a file named like the sandbox's (clauseo-sandbox), then the
  // loop moves on before any check of the sandbox tool.
  const off = commandAt(assemble, 'if [ "$SAFE_WORKSPACE" != true ]; then');
  const refused = commandAt(assemble, '! grep -qiF "${SANDBOX_EXE%-*}" <<< "$listing" ||');
  const next = assemble.indexOf("continue", refused);
  const installerSig = commandAt(
    assemble,
    'node .github/scripts/latest-json.mjs --verify "${files[0]}" "$pub" "$(basename "${files[0]}")" "$VERSION" ||',
  );
  const exeSig = commandAt(assemble, 'node .github/scripts/latest-json.mjs --verify "$exe" "$pub" "$SANDBOX_EXE" "$VERSION" ||');
  assert.ok(installerSig < off && off < refused && refused < next && next < exeSig, "the installer's .sig either way");
  assert.equal(sandboxExe(text).replace(/-[^-]*$/, ""), "clauseo-sandbox");
});

test("release.yml signs the sandbox tool, ships its .sig in both installers and checks both in assemble", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const at = commandAt;
  const sign = jobCommands(text, "sign-windows");
  // Signed as SANDBOX_EXE (the trusted comment's file:), its .sig moved where the overlay reads it.
  const signed = at(sign, 'pnpm -C ui tauri signer sign --app-version "$VERSION" "$dir/$SANDBOX_EXE"');
  assert.ok(at(sign, 'cp "$exe" "$dir/$SANDBOX_EXE"') < signed);
  assert.ok(signed < at(sign, 'mv "$dir/$SANDBOX_EXE.sig" "$exe.sig"'));
  const bundled = sign.findIndex((line) => /\btauri bundle\b/.test(line));
  assert.ok(signed < bundled, "the sandbox tool is signed before NSIS takes it");
  assert.ok(sign[bundled].includes('--config "$RUNNER_TEMP/safe-workspace.conf.json" --config release/sidecars.generated.json'));
  // The overlay names the .sig as a resource and hooks_safe_workspace.nsh as the hooks.
  const overlay = sign.find((line) => line.includes("windows/hooks_safe_workspace.nsh"));
  assert.ok(overlay.includes('"$RUNNER_TEMP/safe-workspace.conf.json" "$sig" "$SANDBOX_EXE.sig"'), overlay);
  assert.ok(overlay.includes("installerHooks: process.argv[4]"), overlay);
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
  // assets() gets the switch's exe name, or null with it off.
  assert.ok(assemble.some((line) => line.includes("assets(process.argv[1], process.argv[2] || null)")));
  assert.ok(assemble.includes('" "$VERSION" "$sandbox_exe")'));
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

// The block-list switch: check's output, set from one literal. Each piece of the snapshot's path
// is behind it, spelled the same way everywhere.
const BLOCKLIST_SWITCH = "needs.check.outputs.bundle_blocklist == 'true'";
const BLOCKLIST_ARG = '${BLOCKLIST_OVERLAY:+--config "$BLOCKLIST_OVERLAY"}';
const BLOCKLIST_ENV = `BLOCKLIST_OVERLAY: \${{ ${BLOCKLIST_SWITCH} && 'release/blocklist.conf.json' || '' }}`;

/** A job's own keys: its lines up to `runs-on:`, comments dropped. */
function jobHead(text, job) {
  const lines = jobCommands(text, job);
  return lines.slice(0, lines.findIndex((line) => line.startsWith("runs-on:")));
}

/** The jobs named in <job>'s `needs:`. */
function jobNeeds(text, job) {
  const line = jobHead(text, job).find((one) => one.startsWith("needs:")) ?? "needs: []";
  return line
    .replace(/^needs:\s*/, "")
    .replace(/^\[|\]$/g, "")
    .split(",")
    .map((one) => one.trim())
    .filter(Boolean);
}

/**
 * Every way release.yml breaks the block-list switch: check's bundle_blocklist not the literal
 * 'true'; the `blocklist` job skipped by a job-level `if:` (it runs and succeeds either way, so
 * every job below it runs as with the switch on) or a step of it, other than the off-notice, not
 * behind the switch; and, in each job that passes release.conf.json, blocklist.conf.json passed
 * other than through $BLOCKLIST_OVERLAY, $BLOCKLIST_OVERLAY not set from the switch, no
 * `blocklist` in its `needs:`, a blocklist-snapshot download not behind the switch, or none
 * before its first `tauri build` or `tauri bundle` (with the switch on, it must be there as before).
 */
function snapshotProblems(text) {
  const checked = [];
  const problems = [];
  const outputs = jobCommands(text, "check").filter((line) => line.startsWith("bundle_blocklist:"));
  const anywhere = text.split("\n").filter((line) => /^\s*bundle_blocklist:/.test(line));
  if (outputs.length !== 1 || anywhere.length !== 1 || outputs[0] !== "bundle_blocklist: 'true'") {
    problems.push(`check: bundle_blocklist is not the one literal 'true' (${outputs.join(" | ")})`);
  }
  if (jobHead(text, "blocklist").some((line) => line.startsWith("if:"))) {
    problems.push("blocklist: skipped by a job-level if:");
  }
  // Its steps, from its raw lines: each starts with `      - `; a step's own `if:` is at 8 spaces.
  const body = text.split(/^  blocklist:\s*$/m)[1].split(/^  \S/m)[0];
  for (const step of body.split(/^ {6}- /m).slice(1)) {
    const name = step.split("\n")[0];
    const gate = step.match(/^ {8}if: (.*)$/m)?.[1] ?? "";
    const off = name === "name: The block-list snapshot is not bundled";
    if (off ? gate !== "needs.check.outputs.bundle_blocklist != 'true'" : gate !== BLOCKLIST_SWITCH) {
      problems.push(`blocklist: step ${name} is not behind the switch`);
    }
  }
  for (const [, job] of text.matchAll(/^  ([a-z0-9-]+):\s*$/gm)) {
    const lines = jobCommands(text, job);
    const release = lines.filter((line) => line.includes("--config release/release.conf.json"));
    if (release.length === 0) {
      continue;
    }
    checked.push(job);
    for (const line of release.filter((one) => !one.includes(BLOCKLIST_ARG))) {
      problems.push(`${job}: no $BLOCKLIST_OVERLAY in ${line}`);
    }
    for (const line of lines.filter((one) => one.includes("release/blocklist.conf.json") && one !== BLOCKLIST_ENV)) {
      problems.push(`${job}: blocklist.conf.json outside the switch: ${line}`);
    }
    if (!lines.includes(BLOCKLIST_ENV)) {
      problems.push(`${job}: BLOCKLIST_OVERLAY is not set from the switch`);
    }
    if (!jobNeeds(text, job).includes("blocklist")) {
      problems.push(`${job}: no blocklist in needs`);
    }
    const download = lines.indexOf("name: blocklist-snapshot");
    const first = lines.findIndex((line) => /\btauri (build|bundle)\b/.test(line));
    if (download === -1 || download > first) {
      problems.push(`${job}: no blocklist-snapshot download before its first tauri build or bundle`);
    } else {
      // The download step's own keys: from its `- uses:` line to the artifact's name.
      const step = lines.slice(0, download + 1);
      const own = step.slice(step.findLastIndex((line) => line.startsWith("- ")));
      if (!own.includes(`if: ${BLOCKLIST_SWITCH}`)) {
        problems.push(`${job}: the blocklist-snapshot download is not behind the switch`);
      }
    }
  }
  return { checked, problems };
}

test("release.yml bundles the block-list snapshot only behind its one switch, on for launch", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  assert.deepEqual(snapshotProblems(text), { checked: ["build", "sign-windows", "measure-builds"], problems: [] });
  const broken = (from, to) => {
    const changed = text.replace(from, to);
    assert.notEqual(changed, text, from);
    return snapshotProblems(changed).problems;
  };
  // The switch: one literal, 'true'.
  assert.deepEqual(broken("bundle_blocklist: 'true'", "bundle_blocklist: 'false'"), [
    "check: bundle_blocklist is not the one literal 'true' (bundle_blocklist: 'false')",
  ]);
  // The blocklist job: never skipped, each step but the off-notice behind the switch.
  assert.deepEqual(broken("    name: blocklist\n    needs: check\n", `    name: blocklist\n    needs: check\n    if: ${BLOCKLIST_SWITCH}\n`), [
    "blocklist: skipped by a job-level if:",
  ]);
  assert.deepEqual(broken(/(- name: Install minisign\n) {8}if: [^\n]*\n/, "$1"), [
    "blocklist: step name: Install minisign is not behind the switch",
  ]);
  assert.deepEqual(broken(/(- uses: actions\/upload-artifact@[^\n]*\n) {8}if: [^\n]*\n( {8}with:\n {10}name: blocklist-snapshot)/, "$1$2"), [
    "blocklist: step uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2 is not behind the switch",
  ]);
  assert.deepEqual(broken("if: needs.check.outputs.bundle_blocklist != 'true'", `if: ${BLOCKLIST_SWITCH}`), [
    "blocklist: step name: The block-list snapshot is not bundled is not behind the switch",
  ]);
  // The overlay only through $BLOCKLIST_OVERLAY, which the switch sets.
  const measureBuild = `--config release/release.conf.json ${BLOCKLIST_ARG} --no-bundle`;
  const dropped = broken(measureBuild, "--config release/release.conf.json --no-bundle");
  assert.equal(dropped.length, 1, dropped.join("\n"));
  assert.match(dropped[0], /^measure-builds: no \$BLOCKLIST_OVERLAY in pnpm -C ui tauri build /);
  const ungated = broken(measureBuild, `--config release/release.conf.json ${BLOCKLIST_ARG} --config release/blocklist.conf.json --no-bundle`);
  assert.equal(ungated.length, 1, ungated.join("\n"));
  assert.match(ungated[0], /^measure-builds: blocklist\.conf\.json outside the switch: pnpm -C ui tauri build /);
  assert.deepEqual(broken(/( {6}# As in build: blocklist\.conf\.json[^\n]*\n)[^\n]*\n/, "$1"), [
    "sign-windows: BLOCKLIST_OVERLAY is not set from the switch",
  ]);
  // Needing blocklist.
  assert.deepEqual(broken("needs: [check, custody, build, blocklist]", "needs: [check, custody, build]"), ["sign-windows: no blocklist in needs"]);
  // The download: behind the switch, before the first build or bundle.
  assert.deepEqual(broken(/( {6}# The same verified snapshot[^]*?)\n {8}if: [^\n]*/, "$1"), [
    "sign-windows: the blocklist-snapshot download is not behind the switch",
  ]);
  assert.deepEqual(broken(/( {6}# The same verified snapshot)[^]*?path: crates\/desktop\/blocklist\n/, ""), [
    "sign-windows: no blocklist-snapshot download before its first tauri build or bundle",
  ]);
  const snapshotStep = /\n( {6}# The same snapshot as the shipped builds[^]*?path: crates\/desktop\/blocklist\n)/;
  const step = text.match(snapshotStep)[1];
  const moved = text.replace(snapshotStep, "\n").replace(/(fake_agent --target "\$TARGET"\n)/, `$1\n${step}`);
  assert.notEqual(moved, text);
  assert.deepEqual(snapshotProblems(moved).problems, [
    "measure-builds: no blocklist-snapshot download before its first tauri build or bundle",
  ]);
});

const LATEST_IF = "if: always() && needs.check.outputs.mode == 'release' && needs.publish.outputs.public == 'true'";
const PUBLISH_LINE = 'gh release edit "$TAG" --repo "$GITHUB_REPOSITORY" --draft=false --latest';

test("sign-windows runs without the macOS and Linux legs; assemble and publish need every job", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  // The job's own keys: its lines up to `runs-on:`, comments dropped.
  const head = (job) => {
    const lines = jobCommands(text, job);
    return lines.slice(0, lines.findIndex((line) => line.startsWith("runs-on:")));
  };
  const sign = head("sign-windows");
  assert.ok(sign.includes("needs: [check, custody, build, blocklist]"), sign.join("\n"));
  assert.ok(
    sign.includes(
      "if: ${{ !cancelled() && needs.check.result == 'success' && needs.custody.result == 'success' && needs.blocklist.result == 'success' }}",
    ),
    sign.join("\n"),
  );
  // No always(), no !cancelled() and no failure() in assemble or publish: the default success()
  // keeps a release, or dist-assets, from going out with a leg missing. latest's one always()
  // goes with publish's output public, so it checks only a release publish made public.
  for (const job of ["assemble", "publish", "latest"]) {
    const ifs = jobCommands(text, job).filter((line) => /^if:/.test(line));
    const allowed = job === "latest" ? [LATEST_IF] : [];
    assert.deepEqual(ifs.filter((line) => /always\(\)|cancelled\(\)|failure\(\)/.test(line)), allowed, job);
  }
});

test("a dry run's About shows its version with dev.<short sha>; a tag's shows the version alone", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const check = jobCommands(text, "check");
  assert.ok(check.includes("version_suffix: ${{ steps.mode.outputs.version_suffix }}"), "the check job's output");
  assert.ok(
    check.includes('if [ "$mode" = dry ]; then echo "version_suffix=dev.${GITHUB_SHA:0:9}"; else echo "version_suffix="; fi'),
    "dry only",
  );
  // The ui job's vite build reads it (ui/app/vite.config.ts, appVersion).
  const ui = jobCommands(text, "ui");
  assert.ok(ui.includes("CLAUSEO_VERSION_SUFFIX: ${{ needs.check.outputs.version_suffix }}"), "the ui build's env");
});

test("release.yml makes a published v* release the latest explicitly, and checks it as the app reads it", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const lines = text.split("\n").map((line) => stripComment(line).trim());
  // --latest only where it is meant: publish makes the release the latest, a test pre-release
  // says it is not; nowhere else.
  const publish = jobCommands(text, "publish");
  const edit = publish.indexOf(PUBLISH_LINE);
  assert.notEqual(edit, -1, "publish");
  const assemble = jobCommands(text, "assemble");
  assert.ok(
    assemble.includes('if [ "$MODE" = release ]; then kind=(--draft); else kind=(--prerelease); fi'),
    "assemble's create",
  );
  assert.ok(
    assemble.some((line) =>
      line.startsWith('gh release create "$TAG" dist/* --repo "$GITHUB_REPOSITORY" --verify-tag "${kind[@]}" --latest=false '),
    ),
    "assemble's create says --latest=false",
  );
  assert.equal(lines.filter((line) => /--latest\b/.test(line)).length, 2, "--latest elsewhere");
  // publish's output public is set right after the edit succeeds, in the same step, nowhere else.
  assert.ok(jobHead(text, "publish").length > 0);
  const outputs = publish.indexOf("outputs:");
  assert.ok(outputs !== -1 && outputs < publish.indexOf("steps:"), "publish's outputs");
  assert.equal(publish[outputs + 1], "public: ${{ steps.public.outputs.public }}");
  assert.equal(publish[edit + 1], 'echo "public=true" >> "$GITHUB_OUTPUT"');
  assert.deepEqual(publish.slice(edit - 6, edit), [
    "- name: Publish the draft as the latest release",
    "id: public",
    "env:",
    "GH_TOKEN: ${{ github.token }}",
    "run: |",
    "set -euo pipefail",
  ]);
  assert.equal(publish.filter((line) => line.includes("public=")).length, 1, "public set once");
  // `latest` runs after publish, in release mode, whenever publish made the release public (a
  // later step of publish may fail), with a read token, no environment and no secret.
  const head = jobHead(text, "latest");
  assert.deepEqual(jobNeeds(text, "latest"), ["check", "publish"]);
  assert.deepEqual(head.filter((line) => line.startsWith("if:")), [LATEST_IF], head.join("\n"));
  const latest = jobCommands(text, "latest");
  const permissions = latest.indexOf("permissions:");
  assert.deepEqual(latest.slice(permissions + 1, permissions + 3), ["contents: read", "env:"]);
  assert.ok(!latest.some((line) => /^environment:|\bsecrets\b/.test(line)), "no environment, no secret");
  // The blocklist job's minisign pin, exactly.
  const pin = (job) => jobCommands(text, job).filter((line) => /^(tarball|sha)=/.test(line));
  assert.equal(pin("blocklist").length, 2);
  assert.deepEqual(pin("latest"), pin("blocklist"));
  // The tier 1 platforms latest-json.mjs requires, each of which must be in the served latest.json.
  assert.ok(latest.includes(`TIER1="${tier1Platforms("1.2.3").join(" ")}"`), "TIER1 is tier1Platforms");
  assert.equal(latest.filter((line) => line.startsWith("TIER1=")).length, 1, "TIER1 once");
  const tierLoop = latest.indexOf("for want in $TIER1; do");
  assert.notEqual(tierLoop, -1, "every tier 1 platform is looked for");
  assert.equal(
    latest[tierLoop + 1],
    'grep -qxF "$want" <<< "$platforms" || fail "latest.json has no $want (tier 1)"',
  );
  assert.equal(latest[tierLoop - 1], "platforms=$(jq -r '.platforms // {} | keys[]' \"$manifest\")");
  // Each platform's signature verifies, and its trusted comment names the file and the version.
  const verify = latest.indexOf(
    'out=$(minisign -V -p "$work/updater.pub" -m "$work/assets/$name" -x "$work/$platform.minisig") ||',
  );
  assert.deepEqual(latest.slice(verify + 1, verify + 6), [
    'fail "$platform: $name does not verify with plugins.updater.pubkey"',
    "comment=$(sed -n 's/^Trusted comment: //p' <<< \"$out\" | tr '\\t' '\\n')",
    'grep -qxF "file:$name" <<< "$comment" && grep -qxF "version:$VERSION" <<< "$comment" ||',
    'fail "$platform: $name\'s signed comment does not name file:$name and version:$VERSION"',
    'summary+=("- $platform: \\`$name\\` verifies (file:$name, version:$VERSION)")',
  ]);
  assert.ok(latest.indexOf("for platform in $platforms; do") < verify, "per platform");
  // As the app reads it: releases/latest, over HTTPS only, each platform's bytes against the key.
  for (const line of [
    'latest=$(gh api "repos/$GITHUB_REPOSITORY/releases/latest" --jq .tag_name) || latest=""',
    '[ "$latest" = "$TAG" ] || fail "GitHub\'s latest release is ${latest:-none}, not $TAG"',
    "endpoint=$(jq -r '.plugins.updater.endpoints[0] // \"\"' crates/desktop/tauri.conf.json)",
    'if get "$endpoint" "$manifest"; then',
    'key=$(jq -r \'.plugins.updater.pubkey // ""\' crates/desktop/tauri.conf.json)',
    '[ -n "$key" ] || fail "plugins.updater.pubkey in tauri.conf.json is empty, so latest.json can\'t be checked"',
    'out=$(minisign -V -p "$work/updater.pub" -m "$work/assets/$name" -x "$work/$platform.minisig") ||',
  ]) {
    assert.ok(latest.includes(line), line);
  }
  // latest.json from the app's own endpoint, never a URL typed into the job.
  assert.ok(!latest.some((line) => line.includes("releases/latest/download")), "a typed-in URL in latest");
  const get = latest.find((line) => line.startsWith("get() {"));
  assert.match(get ?? "", / --location --proto '=https' --proto-redir '=https' /);
});

test("an unsigned beta goes out only with CLAUSEO_UNSIGNED_BETA 'true' and no Apple secret, signed ad hoc", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const lines = text.split("\n").map((line) => stripComment(line).trim());
  const at = (commands, line, from = 0) => {
    const index = commands.indexOf(line, from);
    assert.notEqual(index, -1, line);
    return index;
  };
  // custody decides it once: the variable and the Apple secrets are read there and nowhere else,
  // as booleans, into its outputs has_apple and unsigned_beta, in every mode.
  const variable = "BETA_VARIABLE: ${{ vars.CLAUSEO_UNSIGNED_BETA == 'true' }}";
  const appleSecrets =
    "HAS_APPLE: ${{ secrets.APPLE_CERTIFICATE != '' || secrets.APPLE_ID != '' || secrets.APPLE_API_KEY != '' }}";
  assert.deepEqual(lines.filter((line) => line.includes("CLAUSEO_UNSIGNED_BETA") && line.includes("vars.")), [variable]);
  assert.deepEqual(lines.filter((line) => /secrets\.APPLE_/.test(line)), [appleSecrets]);
  const custody = jobCommands(text, "custody");
  const decide = at(custody, "- name: Decide the unsigned beta");
  assert.deepEqual(custody.slice(decide + 1, decide + 5), ["id: beta", "env:", variable, appleSecrets]);
  const decideEnd = custody.findIndex((line, i) => i > decide && line.startsWith("- "));
  const decideStep = custody.slice(decide, decideEnd);
  assert.ok(!decideStep.some((line) => line.startsWith("if:")), "decided in every mode");
  for (const line of [
    "true/false) beta=true ;;",
    "true/true | false/true | false/false) beta=false ;;",
    '{ echo "has_apple=$HAS_APPLE"; echo "unsigned_beta=$beta"; } >> "$GITHUB_OUTPUT"',
  ]) {
    at(decideStep, line);
  }
  const outputs = at(custody, "outputs:");
  assert.ok(outputs < custody.indexOf("steps:"));
  assert.deepEqual(custody.slice(outputs + 1, outputs + 3), [
    "has_apple: ${{ steps.beta.outputs.has_apple }}",
    "unsigned_beta: ${{ steps.beta.outputs.unsigned_beta }}",
  ]);
  // publish and assemble read custody's outputs, which they need.
  const beta = "UNSIGNED_BETA: ${{ needs.custody.outputs.unsigned_beta }}";
  const apple = "HAS_APPLE: ${{ needs.custody.outputs.has_apple }}";
  for (const job of ["publish", "assemble"]) {
    assert.ok(jobNeeds(text, job).includes("custody"), `${job} needs custody`);
  }
  assert.equal(lines.filter((line) => line === beta).length, 2);
  assert.equal(lines.filter((line) => line === apple).length, 2);
  // publish: without the variable, or with an Apple secret, today's stapler check; else every
  // DMG's .app verifies and is signed ad hoc.
  const publish = jobCommands(text, "publish");
  at(publish, beta);
  at(publish, apple);
  const signed = at(publish, 'if [ "$UNSIGNED_BETA" != true ] || [ "$HAS_APPLE" = true ]; then');
  const stapler = at(
    publish,
    'xcrun stapler validate "$dmg" || { echo "::error::$(basename "$dmg") is not notarised and stapled"; exit 1; }',
    signed,
  );
  const leave = at(publish, "exit 0", stapler);
  const adhoc = publish.findIndex((line) => line.includes("grep -qx 'Signature=adhoc'"));
  assert.ok(adhoc > leave, "the ad hoc check is the beta's");
  at(publish, 'elif ! codesign --verify --deep --strict --verbose=2 "$app"; then', leave);
  assert.ok(publish.slice(leave).some((line) => line.includes("**Unsigned beta:**")), "the summary says so");
  assert.ok(at(publish, "- name: Publish the draft as the latest release") > adhoc, "checked before publishing");
  // assemble: the first-open steps only under the same rule, linking the install guides.
  const assemble = jobCommands(text, "assemble");
  at(assemble, beta);
  at(assemble, apple);
  const block = at(assemble, 'if [ "$UNSIGNED_BETA" = true ] && [ "$HAS_APPLE" != true ]; then');
  const end = at(assemble, "fi", at(assemble, 'echo "- **Windows:** a blue box says **Windows protected your PC**. Click **More info**, then **Run anyway**. [Installing Clauseo on Windows]($guide/windows.md) shows each click."', block) + 1) + 1;
  const inside = assemble.slice(block, end).join("\n");
  for (const words of ["**Open Anyway**", "xattr -dr com.apple.quarantine /Applications/Clauseo.app", "($guide/macos.md)"]) {
    assert.ok(inside.includes(words), words);
  }
  assert.ok(inside.includes("guide=https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/install"));
  assert.ok(!assemble.slice(0, block).concat(assemble.slice(end)).some((line) => line.includes("Open Anyway")));
});

/** The script of the step named <name> in release.yml, dedented. */
function stepScript(text, name) {
  const lines = text.split("\n");
  const at = lines.findIndex((line) => line.trim() === `- name: ${name}`);
  assert.notEqual(at, -1, name);
  const run = lines.findIndex((line, i) => i > at && /^\s+run: \|$/.test(line));
  const indent = indentOf(lines[run]);
  const body = [];
  for (const line of lines.slice(run + 1)) {
    if (line.trim() !== "" && indentOf(line) <= indent) {
      break;
    }
    body.push(line);
  }
  const strip = Math.min(...body.filter((line) => line.trim() !== "").map(indentOf));
  return body.map((line) => line.slice(strip)).join("\n");
}

test("check refuses a release whose endpoint is not releases/latest, or whose version is not above the latest", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const check = jobCommands(text, "check");
  // The endpoint: exactly the one list, in release mode only.
  assert.ok(
    check.includes(
      "want_endpoints='[\"https://github.com/clauseo-ai/clauseo-acp/releases/latest/download/latest.json\"]'",
    ),
  );
  assert.ok(
    check.includes(
      'if [ "$mode" = release ] && [ "$(jq -c \'.plugins.updater.endpoints // null\' crates/desktop/tauri.conf.json)" != "$want_endpoints" ]; then',
    ),
  );
  // check stays read-only.
  const permissions = check.indexOf("permissions:");
  assert.deepEqual(check.slice(permissions + 1, permissions + 3), ["contents: read", "outputs:"]);
  const name = "A release's version is above every published app release's";
  const step = check.indexOf(`- name: ${name}`);
  assert.equal(check[step + 1], "if: steps.mode.outputs.mode == 'release'");
});

test("the version check passes a first app release or a higher version and refuses the rest", { skip: process.platform === "win32" }, () => {
  const script = stepScript(readFileSync(RELEASE_YML, "utf8"), "A release's version is above every published app release's");
  const dir = mkdtempSync(join(tmpdir(), "version-check-"));
  try {
    // gh api --paginate prints each page's array, one after another; FAIL makes it fail.
    writeFileSync(
      join(dir, "gh"),
      ["#!/bin/bash", '[ -z "$FAIL" ] || { echo "gh: Server Error (HTTP 500)" >&2; exit 1; }', 'printf "%s" "$PAGES"', ""].join("\n"),
      { mode: 0o755 },
    );
    const release = (tag, { draft = false, prerelease = false } = {}) => ({ tag_name: tag, draft, prerelease });
    // releases/latest today is python-runtime's; drafts, pre-releases and test tags never count.
    const others = [
      release("python-runtime-3.13.18-c2"),
      release("v9.0.0", { draft: true }),
      release("v8.0.0", { prerelease: true }),
      release("test-v7.0.0", { prerelease: true }),
      release("v7.0"),
      release("v07.0.0"),
    ];
    const pages = (...lists) => lists.map((list) => JSON.stringify(list)).join("\n");
    const run = (version, PAGES, FAIL = "") =>
      spawnSync("bash", ["-c", script], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${dir}:${process.env.PATH}`,
          RUNNER_TEMP: dir,
          GITHUB_REPOSITORY: "clauseo-ai/clauseo-acp",
          VERSION: version,
          PAGES,
          FAIL,
        },
      });
    for (const [version, listed] of [
      ["0.1.0", pages([])],
      ["0.1.0", pages(others)],
      ["1.2.4", pages(others, [release("v1.2.3")])],
      ["1.3.0", pages([release("v1.2.9"), release("v0.9.0")], others)],
      ["2.0.0", pages([release("v1.99.99")])],
      ["1.10.0", pages([release("v1.9.0"), release("v1.2.0")])],
    ]) {
      const result = run(version, listed);
      assert.equal(result.status, 0, `${version} over ${listed}: ${result.stdout}${result.stderr}`);
    }
    for (const [version, listed, error, fail = ""] of [
      ["1.2.3", pages(others, [release("v1.2.3")]), "::error::1.2.3 is not above the highest published app release, v1.2.3"],
      ["1.2.2", pages([release("v1.2.3")]), "::error::1.2.2 is not above the highest published app release, v1.2.3"],
      ["1.9.0", pages([release("v1.10.0"), release("v1.2.0")]), "::error::1.9.0 is not above the highest published app release, v1.10.0"],
      ["1.0.5", pages([release("v1.0.4")], [release("v1.1.0")]), "::error::1.0.5 is not above the highest published app release, v1.1.0"],
      ["1.2.4", pages([]), "::error::could not list the releases of clauseo-ai/clauseo-acp", "yes"],
    ]) {
      const result = run(version, listed, fail);
      assert.equal(result.status, 1, `${version} over ${listed}`);
      assert.ok(result.stdout.includes(error), result.stdout + result.stderr);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

const DESKTOP = join(here, "..", "..", "crates", "desktop");
const KEYS_STEP = "The updater's keys and endpoint are the release's";

/** Tauri's form of a minisign public key with key id `id` (16 hex digits) and a comment naming `named`. */
function madePub(id, named = id) {
  const key = Buffer.concat([Buffer.from("Ed"), Buffer.from(id, "hex").reverse(), Buffer.alloc(32, 0x11)]);
  return b64(`untrusted comment: minisign public key: ${named}\n${key.toString("base64")}\n`);
}

test("check's key step requires the release key's id, refuses the placeholder and the test key, and needs updater-test.pub", { skip: process.platform === "win32" }, () => {
  const script = stepScript(readFileSync(RELEASE_YML, "utf8"), KEYS_STEP);
  const real = {
    conf: readFileSync(join(DESKTOP, "tauri.conf.json"), "utf8"),
    testPub: readFileSync(join(DESKTOP, "release", "updater-test.pub"), "utf8"),
    keyid: readFileSync(join(DESKTOP, "release", "updater-release.keyid"), "utf8"),
  };
  const placeholder = readFileSync(join(DESKTOP, "release", "updater-placeholder.pub"), "utf8").trim();
  const withPub = (pubkey) => {
    const conf = JSON.parse(real.conf);
    conf.plugins.updater.pubkey = pubkey;
    return JSON.stringify(conf);
  };
  const dir = mkdtempSync(join(tmpdir(), "keys-step-"));
  // One run of the step's own script in a copy of the files it reads; null leaves a file out.
  const run = (mode, files = {}) => {
    rmSync(join(dir, "crates"), { recursive: true, force: true });
    mkdirSync(join(dir, "crates", "desktop", "release"), { recursive: true });
    const { conf, testPub, keyid } = { ...real, ...files };
    for (const [path, text] of [["tauri.conf.json", conf], ["release/updater-test.pub", testPub], ["release/updater-release.keyid", keyid]]) {
      if (text !== null) writeFileSync(join(dir, "crates", "desktop", path), text);
    }
    const result = spawnSync("bash", ["-c", script], { cwd: dir, encoding: "utf8", env: { ...process.env, MODE: mode } });
    return { status: result.status, out: result.stdout + result.stderr };
  };
  const passes = (mode, files, why) => {
    const result = run(mode, files);
    assert.equal(result.status, 0, `${why} in ${mode}: ${result.out}`);
    return result.out;
  };
  const fails = (mode, files, error, why) => {
    const result = run(mode, files);
    assert.equal(result.status, 1, `${why} in ${mode}: ${result.out}`);
    assert.ok(result.out.includes(`::error::${error}`), `${why} in ${mode}: ${result.out}`);
  };
  try {
    // The committed files pass in every mode, the release key being the one updater-release.keyid names.
    assert.equal(real.keyid, "F6A25D1511B92EBE\n");
    for (const mode of ["release", "test", "dry"]) {
      assert.ok(passes(mode, {}, "the committed files").includes("plugins.updater.pubkey is key F6A25D1511B92EBE; the test key is B142942ACEC5F521"));
    }
    // updater-test.pub: missing, empty or no key fails in every mode, before anything is decoded.
    for (const mode of ["release", "test", "dry"]) {
      for (const [testPub, error] of [
        [null, "crates/desktop/release/updater-test.pub (the test key's public key) is missing"],
        ["", "crates/desktop/release/updater-test.pub (the test key's public key) is missing"],
        ["bm90IGEga2V5Cg==\n", "crates/desktop/release/updater-test.pub is not a minisign public key:"],
      ]) {
        fails(mode, { testPub }, error, `updater-test.pub ${JSON.stringify(testPub)}`);
      }
    }
    // updater-release.keyid: missing or not one upper-case key id fails in every mode.
    for (const keyid of [null, "", "f6a25d1511b92ebe\n", "F6A25D1511B92EB\n"]) {
      fails("dry", { keyid }, "crates/desktop/release/updater-release.keyid does not hold a key id", `keyid ${JSON.stringify(keyid)}`);
    }
    // A release refuses the placeholder, the test key and any key but the one the keyid names.
    fails("release", { conf: withPub(placeholder) }, "plugins.updater.pubkey is the placeholder (AC986E7C6BA67AB9)", "the placeholder");
    fails("release", { conf: withPub(real.testPub.trim()) }, "plugins.updater.pubkey is the test key (B142942ACEC5F521)", "the test key");
    const other = withPub(madePub("0123456789ABCDEF"));
    fails("release", { conf: other }, "plugins.updater.pubkey is key 0123456789ABCDEF, not the release key F6A25D1511B92EBE", "another key");
    fails("release", { conf: withPub(madePub("0123456789ABCDEF", "F6A25D1511B92EBE")) }, "plugins.updater.pubkey in tauri.conf.json is not a minisign public key: its comment does not name its own key id 0123456789ABCDEF", "a renamed comment");
    fails("release", { conf: withPub("") }, "plugins.updater.pubkey in tauri.conf.json is empty", "no key");
    // Rotating the key: a new pubkey passes once the keyid names it.
    passes("release", { conf: other, keyid: "0123456789ABCDEF\n" }, "a rotated key and keyid");
    // A dry or test run only warns on the placeholder and takes any well-formed key.
    for (const mode of ["test", "dry"]) {
      assert.ok(passes(mode, { conf: withPub(placeholder) }, "the placeholder").includes("::warning::plugins.updater.pubkey is still the placeholder"));
      passes(mode, { conf: other }, "another key");
    }
    // The endpoint: exactly releases/latest in a release.
    const conf = JSON.parse(real.conf);
    conf.plugins.updater.endpoints = ["https://example.com/latest.json"];
    fails("release", { conf: JSON.stringify(conf) }, "plugins.updater.endpoints in tauri.conf.json is", "another endpoint");
    passes("dry", { conf: JSON.stringify(conf) }, "another endpoint");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("no overlay a release build passes sets plugins.updater, so check's read of tauri.conf.json is the app's", () => {
  const committed = readdirSync(join(DESKTOP, "release")).filter((name) => name.endsWith(".json") && name !== "sidecars.generated.json");
  assert.ok(committed.includes("release.conf.json") && committed.includes("machine.conf.json"), committed.join(" "));
  const inline = [...readFileSync(RELEASE_YML, "utf8").matchAll(/--config '(\{[^']*\})'/g)].map((match) => match[1]);
  assert.ok(inline.length > 0);
  for (const [name, json] of [...committed.map((name) => [name, readFileSync(join(DESKTOP, "release", name), "utf8")]), ...inline.map((json) => [json, json])]) {
    assert.equal(JSON.parse(json).plugins?.updater, undefined, `${name} sets plugins.updater`);
  }
});

/**
 * Every way a workflow in .github/workflows (each *.yml and *.yaml, found by listing the folder,
 * so a new workflow is covered) could take releases/latest from a published v* app release: a
 * `gh release create` or `gh release edit` without --latest=false, but release.yml's publish
 * (PUBLISH_LINE); --latest anywhere else; a write to the releases API (gh api or curl with a
 * method or fields) without make_latest=false; or an action that makes releases.
 */
function latestProblems(workflows) {
  const problems = [];
  for (const [name, text] of workflows) {
    const lines = text
      .replace(/\\\n\s*/g, " ")
      .split("\n")
      .map((line) => stripComment(line).trim());
    for (const line of lines) {
      const command = line.replace(/^run:\s*/, "");
      if (name === "release.yml" && command === PUBLISH_LINE) continue;
      if (/\bgh release (create|edit)\b/.test(command) && !/ --latest=false\b/.test(command)) {
        problems.push(`${name}: no --latest=false: ${command}`);
      }
      if (/--latest\b(?!=false)/.test(command)) problems.push(`${name}: --latest: ${command}`);
      const api = /\bgh api\b|\bcurl\b/.test(command) && /\/releases\b/.test(command);
      const writes = / (-X|--method|--request) *(POST|PATCH|PUT)\b| (-f|-F|--field|--raw-field|--input|-d|--data)\b/.test(command);
      if (api && writes && !/make_latest=false/.test(command)) {
        problems.push(`${name}: a releases API write without make_latest=false: ${command}`);
      }
      if (/^-? *uses: *[^ ]*(action-gh-release|release-action|create-release)\b/i.test(line)) {
        problems.push(`${name}: a release action: ${line}`);
      }
    }
  }
  return problems;
}

test("no workflow's release takes releases/latest but a published v* app release", () => {
  // Every installed app reads latest.json through releases/latest, so every other release any
  // workflow makes or edits (agent-browser's, python-runtime's, a test run's, release.yml's
  // draft) says --latest=false, and only release.yml's publish says --latest.
  const dir = join(here, "..", "workflows");
  const workflows = readdirSync(dir)
    .filter((name) => /\.ya?ml$/.test(name))
    .sort()
    .map((name) => [name, readFileSync(join(dir, name), "utf8")]);
  assert.ok(workflows.length > 10, "the workflows folder");
  assert.deepEqual(latestProblems(workflows), []);
  // The ones that make a release today, so the rule above is known to have looked at them.
  const making = workflows.filter(([, text]) => /\bgh release create\b/.test(text)).map(([name]) => name);
  assert.deepEqual(making, ["agent-browser-build.yml", "python-runtime.yml", "release.yml"]);
  assert.equal(
    workflows.flatMap(([, text]) => text.split("\n")).filter((line) => line.trim() === PUBLISH_LINE).length,
    1,
  );
  // Each way it can break is found.
  const one = (body) => latestProblems([["new.yml", body]]);
  assert.equal(one('run: gh release create "$TAG" --verify-tag').length, 1);
  assert.equal(one('run: gh release create "$TAG" \\\n  --prerelease').length, 1);
  assert.equal(one('gh release edit "$TAG" --draft=false').length, 1);
  assert.equal(one('gh release edit "$TAG" --draft=false --latest').length, 2);
  assert.equal(latestProblems([["release.yml", `run: ${PUBLISH_LINE}`]]).length, 0);
  assert.equal(one('gh api -X POST "repos/$R/releases" -f tag_name=v1').length, 1);
  assert.equal(one('gh api "repos/$R/releases/1" --method PATCH -F draft=false').length, 1);
  assert.equal(one('curl -X POST https://api.github.com/repos/o/r/releases -d @body.json').length, 1);
  assert.equal(one('gh api -X POST "repos/$R/releases" -f make_latest=false').length, 0);
  assert.equal(one('gh api --paginate "repos/$R/releases"').length, 0);
  assert.equal(one("- uses: softprops/action-gh-release@0123456789012345678901234567890123456789").length, 1);
  assert.equal(one('gh release create "$TAG" --latest=false').length, 0);
});

test("custody checks the release key against tauri.conf.json's pubkey before any leg, the key in one step", () => {
  const text = readFileSync(RELEASE_YML, "utf8");
  const head = jobHead(text, "custody");
  assert.deepEqual(jobNeeds(text, "custody"), ["check"]);
  assert.ok(!head.some((line) => line.startsWith("if:")), "no job-level if: build and sign-windows need it to succeed");
  // Every leg that builds, and sign-windows, waits for it.
  assert.ok(jobNeeds(text, "build").includes("custody"), "build needs custody");
  assert.ok(jobNeeds(text, "sign-windows").includes("custody"), "sign-windows needs custody");
  const custody = jobCommands(text, "custody");
  // The release environment in release mode only, before `steps:`.
  const environment = custody.indexOf("environment: ${{ needs.check.outputs.mode == 'release' && 'release' || '' }}");
  assert.ok(environment !== -1 && environment < custody.indexOf("steps:"), "custody's environment");
  const secrets = custody.filter((line) => /\bsecrets\b/.test(line));
  assert.deepEqual(secrets, [
    "HAS_APPLE: ${{ secrets.APPLE_CERTIFICATE != '' || secrets.APPLE_ID != '' || secrets.APPLE_API_KEY != '' }}",
    "TAURI_SIGNING_PRIVATE_KEY: ${{ needs.check.outputs.mode == 'release' && secrets.TAURI_SIGNING_PRIVATE_KEY || '' }}",
    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ needs.check.outputs.mode == 'release' && secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD || '' }}",
  ]);
  const step = custody.indexOf("- name: The release key signs, and tauri.conf.json's pubkey verifies it");
  assert.ok(step > custody.indexOf("run: pnpm -C ui install --frozen-lockfile"), "install before the key");
  const body = custody.slice(step, custody.findIndex((line, i) => i > step && line.startsWith("- ")) >>> 0);
  assert.equal(body[1], "if: needs.check.outputs.mode == 'release'");
  for (const line of [
    'pnpm -s -C ui tauri signer sign --app-version "$VERSION" "$dir/$name" > /dev/null ||',
    "jq -r '.plugins.updater.pubkey // \"\"' crates/desktop/tauri.conf.json > \"$dir/updater.pub\"",
    'node .github/scripts/latest-json.mjs --verify "$dir/$name" "$dir/updater.pub" "$name" "$VERSION" ||',
  ]) {
    assert.ok(body.includes(line), line);
  }
  // Every step but the beta's decision (in every mode, its own test) and the off-notice only in
  // release mode.
  const steps = custody.filter((line) => line.startsWith("- "));
  const ifs = custody.filter((line) => line.startsWith("if: needs.check.outputs.mode"));
  assert.equal(steps[0], "- name: Decide the unsigned beta");
  assert.equal(ifs.length, steps.length - 1);
  assert.equal(ifs.filter((line) => line === "if: needs.check.outputs.mode == 'release'").length, steps.length - 2);
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

test("the CLI writes latest.json with the sandbox tool's rows only when given its name", () => {
  const dir = release("1.2.3");
  try {
    const cli = (...args) =>
      spawnSync(process.execPath, [join(here, "latest-json.mjs"), dir, "1.2.3", BASE, DATE, ...args], { encoding: "utf8" });
    // The Safe workspace off: no sandbox exe in the release, and none asked for.
    const off = cli();
    assert.equal(off.status, 0, off.stderr);
    assert.equal(off.stdout, "");
    const manifest = readFileSync(join(dir, "latest.json"), "utf8");
    assert.doesNotMatch(manifest, /sandbox/);
    // On: the tier 1 exe is required.
    const on = cli("clauseo-sandbox-setup.exe");
    assert.equal(on.status, 1);
    assert.match(on.stderr, /clauseo-sandbox-setup-x64\.exe is missing \(tier 1\)/);
    // A bad name, or an option where the folder goes, is a usage error.
    assert.equal(cli("Setup.EXE").status, 2);
    assert.equal(spawnSync(process.execPath, [join(here, "latest-json.mjs"), "--verify", "a", "b", "c"]).status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
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

// The first-open block's lines as GitHub shows them: each `echo "..."` from its heading to the next
// heading, the shell's escapes undone, then the Markdown (heading, bullet, bold, link, code) read
// as plain text, as the copy review registers them.
const firstOpenLines = (text) => {
  const lines = text.split("\n");
  const start = lines.findIndex((l) => l.trim() === 'echo "## Opening Clauseo the first time"');
  assert.ok(start >= 0, "release.yml has no first-open heading");
  const end = lines.findIndex((l, i) => i > start && /^\s*echo "## /.test(l));
  assert.ok(end > start, "release.yml's first-open block has no heading after it");
  return lines
    .slice(start, end)
    .map((l) => /^\s*echo "(.+)"$/.exec(l)?.[1])
    .filter((l) => l !== undefined)
    .map((l) =>
      l
        .replace(/\\(["`$\\])/g, "$1")
        .replace(/^## /, "")
        .replace(/^- /, "")
        .replace(/\*\*(.+?)\*\*/g, "$1")
        .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
        .replace(/`([^`]+)`/g, "$1"),
    );
};

test("the first-open block says what the copy review registers and copy-approvals holds", () => {
  const ui = join(here, "..", "..", "ui", "app");
  const shown = firstOpenLines(readFileSync(RELEASE_YML, "utf8"));
  assert.equal(shown.length, 5);
  for (const line of shown)
    assert.doesNotMatch(line, /[*[\]`$]/, `Markdown or a slot left in: ${line}`);
  const registry = readFileSync(join(ui, "src", "review", "registry-release-notes.tsx"), "utf8");
  const call = /notesWords\(\s*(?:"((?:[^"\\]|\\.)*)"|'([^'\\]*)')\s*,?\s*\)/g;
  const registered = [...registry.matchAll(call)].map(([, double, single]) =>
    double === undefined ? single : JSON.parse(`"${double}"`),
  );
  assert.deepEqual(shown, registered);
  const approvals = JSON.parse(
    readFileSync(join(ui, "copy-approvals", "UC-release-latest-check.json"), "utf8"),
  );
  assert.deepEqual(
    Object.keys(approvals).sort().map((id) => approvals[id].en),
    shown,
  );
});
