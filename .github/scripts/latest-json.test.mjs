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
// to passing the generated sidecar overlay last.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { assets, buildManifest, manifestProblems, signedVersion } from "./latest-json.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(here, "latest-json.mjs");
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

/** buildManifest over a scratch release, removed afterwards; <macos> false is a release without macOS. */
function build(version, options, { macos = true } = {}) {
  const dir = release(version, options);
  try {
    return buildManifest(dir, version, BASE, DATE, { macos });
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
    leave: ["Clauseo_1.2.3_arm64-setup.exe", "Clauseo_1.2.3_amd64.deb"],
  });
  assert.deepEqual(problems, []);
  assert.deepEqual(missing, ["Clauseo_1.2.3_arm64-setup.exe", "Clauseo_1.2.3_amd64.deb"]);
  assert.equal("windows-aarch64" in manifest.platforms, false);
  assert.equal("windows-aarch64-nsis" in manifest.platforms, false);
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

/** The macOS assets of release <version>: what a release without macOS leaves out. */
const macosAssets = (version) => assets(version).filter((asset) => asset.macos).map((asset) => asset.name);

test("a release without macOS lists only what was built, and names no macOS asset missing", () => {
  assert.deepEqual(macosAssets("1.2.3"), [
    "Clauseo_1.2.3_aarch64.dmg",
    "Clauseo_1.2.3_aarch64.app.tar.gz",
    "Clauseo_1.2.3_x64.dmg",
    "Clauseo_1.2.3_x64.app.tar.gz",
  ]);
  assert.deepEqual(
    assets("1.2.3", { macos: false }).map((asset) => asset.name),
    assets("1.2.3").map((asset) => asset.name).filter((name) => !macosAssets("1.2.3").includes(name)),
  );
  const leave = macosAssets("1.2.3");
  const { manifest, missing, problems } = build("1.2.3", { leave }, { macos: false });
  assert.deepEqual(problems, []);
  assert.deepEqual(missing, []);
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    "windows-aarch64",
    "windows-aarch64-nsis",
    "windows-x86_64",
    "windows-x86_64-nsis",
  ]);
  assert.deepEqual(manifestProblems(manifest, "1.2.3", { macos: false }), []);
  // The same manifest in a release with macOS lacks its tier 1 platform...
  assert.deepEqual(manifestProblems(manifest, "1.2.3"), ["platforms has no darwin-aarch64-app (tier 1)"]);
  // ...and the same folder is refused there.
  assert.ok(build("1.2.3", { leave }).problems.includes("Clauseo_1.2.3_aarch64.dmg is missing (tier 1)"));
  // A tier 2 asset missing beside it is still named, and still only it.
  const partial = build("1.2.3", { leave: [...leave, "Clauseo_1.2.3_amd64.deb"] }, { macos: false });
  assert.deepEqual(partial.problems, []);
  assert.deepEqual(partial.missing, ["Clauseo_1.2.3_amd64.deb"]);
  // A tier 1 Windows asset is still required.
  assert.deepEqual(
    build("1.2.3", { leave: [...leave, "Clauseo_1.2.3_x64-machine-setup.exe"] }, { macos: false }).problems,
    ["Clauseo_1.2.3_x64-machine-setup.exe is missing (tier 1)"],
  );
});

test("a release without macOS refuses a macOS asset or a darwin key", () => {
  const leave = macosAssets("1.2.3").filter((name) => name !== "Clauseo_1.2.3_x64.app.tar.gz");
  assert.deepEqual(build("1.2.3", { leave }, { macos: false }).problems, [
    "Clauseo_1.2.3_x64.app.tar.gz is a macOS asset, and this release has no macOS",
    "Clauseo_1.2.3_x64.app.tar.gz.sig is a macOS asset, and this release has no macOS",
  ]);
  const { manifest } = build("1.2.3");
  assert.deepEqual(manifestProblems(manifest, "1.2.3", { macos: false }), [
    "platforms has the unknown key darwin-aarch64-app",
    "platforms has the unknown key darwin-x86_64-app",
  ]);
});

test("the command line's --no-macos writes latest.json for a Windows release, as assemble runs it", () => {
  const dir = release("1.2.3", { leave: macosAssets("1.2.3") });
  try {
    const run = (...args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
    const without = run("--no-macos", dir, "1.2.3", BASE, DATE);
    assert.equal(without.status, 0, without.stderr);
    assert.equal(without.stdout, "");
    const manifest = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
    assert.deepEqual(manifestProblems(manifest, "1.2.3", { macos: false }), []);
    assert.equal(Object.keys(manifest.platforms).some((key) => key.startsWith("darwin-")), false);
    rmSync(join(dir, "latest.json"));
    const withMacos = run(dir, "1.2.3", BASE, DATE);
    assert.equal(withMacos.status, 1);
    assert.match(withMacos.stderr, /Clauseo_1\.2\.3_aarch64\.dmg is missing \(tier 1\)/);
    assert.equal(existsSync(join(dir, "latest.json")), false);
    assert.equal(run("--no-macos", dir, "1.2.3", BASE).status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
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
  for (const job of ["check", "build", "sign-windows", "assemble", "publish", "measure-builds"]) {
    assert.match(text, new RegExp(`^  ${job}:`, "m"), job);
  }
});

/** A job's lines without comments, each `\`-continued command joined into one line. */
function jobCommands(text, job) {
  const body = text.split(/^  ([a-z-]+):\s*$/m);
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
  // A release without macOS (release.yml's MACOS_IN_RELEASE "false") has no darwin key at all.
  const macos = Object.keys(manifest.platforms ?? {}).some((key) => key.startsWith("darwin-"));
  assert.deepEqual(manifestProblems(manifest, version, { macos }), []);
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
