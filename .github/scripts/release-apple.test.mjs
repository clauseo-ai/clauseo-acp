// Tests for release.yml's Apple Developer ID path (decision 0035, "Apple Developer ID") and for
// apple-notary.mjs. Plain Node, no install, no network:
//
//   node --test .github/scripts/release-apple.test.mjs
//
// release.yml is read as text, as latest-json.test.mjs reads it. The rules: an Apple secret
// appears only on the steps that use it (custody's decision and its check, build's bundle step
// and its Developer ID check), always behind the mode or DEVELOPER_ID, never in a run: script;
// every Developer ID codesign has --timestamp, and every one on code --options runtime;
// notarisation, stapling and the keychain happen only on a Developer ID leg; the Developer ID
// check holds its spctl, stapler and codesign checks; custody refuses a partial set of Apple
// secrets (its script is run here, with a throwaway certificate when openssl is there); and the
// entitlements file holds none of the entitlements the decision leaves out. Each rule is a
// function that returns what is wrong, and each test breaks the workflow on purpose to show the
// rule catches it.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createPublicKey, generateKeyPairSync, verify } from "node:crypto";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { SUBMISSIONS_URL, credentialProblems, notaryProblem, notaryToken } from "./apple-notary.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const ROOT = join(here, "..", "..");
const RELEASE_YML = join(ROOT, ".github", "workflows", "release.yml");
const TAURI_CONF = join(ROOT, "crates", "desktop", "tauri.conf.json");
const workflow = () => readFileSync(RELEASE_YML, "utf8");

const APPLE_SIX = [
  "APPLE_CERTIFICATE",
  "APPLE_CERTIFICATE_PASSWORD",
  "APPLE_SIGNING_IDENTITY",
  "APPLE_API_KEY",
  "APPLE_API_ISSUER",
  "APPLE_API_KEY_P8",
];
const APPLE_ID_NAMES = ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];
const BUNDLE_STEP = "Bundle the .app and the DMG, and sign the updater archive under its arch name";
const CHECK_STEP = "Every Mach-O is signed with the Developer ID, and the .app and the DMG are notarised and stapled";
const CUSTODY_STEP = "Every Apple secret is set or none is, and the certificate and the API key work";
const LEFT_OUT = [
  "com.apple.security.get-task-allow",
  "com.apple.security.cs.allow-jit",
  "com.apple.security.cs.allow-unsigned-executable-memory",
  "com.apple.security.cs.disable-library-validation",
  "com.apple.security.cs.allow-dyld-environment-variables",
  "com.apple.security.cs.disable-executable-page-protection",
  "com.apple.security.cs.debugger",
];

/** <line> without a trailing YAML comment (a # after a space, outside quotes). */
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

/** The lines of job <job>, its header included, up to the next job. */
function jobLines(text, job) {
  const lines = text.split("\n");
  const start = lines.indexOf(`  ${job}:`);
  if (start === -1) {
    return [];
  }
  const end = lines.findIndex((line, i) => i > start && /^  [a-z0-9-]+:\s*$/.test(line));
  return lines.slice(start, end === -1 ? undefined : end);
}

/**
 * Each step of <job>: its name, `if:`, env lines (KEY: value), timeout and run script (dedented,
 * comments kept, as bash would read it).
 */
function stepsOf(text, job) {
  const steps = [];
  let step = null;
  for (const raw of jobLines(text, job)) {
    if (/^ {6}- /.test(raw)) {
      step = { lines: [] };
      steps.push(step);
    } else if (/^ {0,5}\S/.test(raw)) {
      step = null;
    }
    step?.lines.push(raw);
  }
  return steps.map(({ lines }) => {
    const top = (key) => {
      const line = lines.find((one) => new RegExp(`^ {6}(- | {2})${key}: `).test(one));
      return line === undefined ? null : stripComment(line).replace(new RegExp(`^ {6}(- | {2})${key}: `), "").trim();
    };
    const env = [];
    const envAt = lines.findIndex((line) => /^ {8}env:\s*$/.test(line));
    if (envAt !== -1) {
      for (const line of lines.slice(envAt + 1)) {
        if (!/^ {10}\S/.test(line)) {
          break;
        }
        env.push(stripComment(line).trim());
      }
    }
    let run = "";
    const runAt = lines.findIndex((line) => /^ {8}run: \|\s*$/.test(line));
    if (runAt !== -1) {
      const body = [];
      for (const line of lines.slice(runAt + 1)) {
        if (line.trim() !== "" && !line.startsWith(" ".repeat(10))) {
          break;
        }
        body.push(line.slice(10));
      }
      run = body.join("\n");
    } else if (top("run") !== null) {
      run = top("run");
    }
    return { name: top("name"), if: top("if"), timeout: top("timeout-minutes"), env, run, lines };
  });
}

const stepNamed = (text, job, name) => stepsOf(text, job).find((step) => step.name === name);

// ---- Apple secrets: only on the named steps ----

const custodyGate = (name) => `${name}: \${{ needs.check.outputs.mode == 'release' && secrets.${name} || '' }}`;
const buildGate = (name, otherwise = "''") => `${name}: \${{ env.DEVELOPER_ID == 'true' && secrets.${name} || ${otherwise} }}`;

/** Where release.yml's Apple secrets are wrong: every mention, by job and step, against the list. */
function appleSecretProblems(text) {
  const problems = [];
  const hasApple =
    "HAS_APPLE: ${{ " + [...APPLE_SIX, ...APPLE_ID_NAMES].map((name) => `secrets.${name} != ''`).join(" || ") + " }}";
  const allowed = new Map([
    ["custody/Decide the unsigned beta", [hasApple]],
    [
      `custody/${CUSTODY_STEP}`,
      [
        ...APPLE_SIX.map(custodyGate),
        "HAS_APPLE_ID: ${{ " + APPLE_ID_NAMES.map((name) => `secrets.${name} != ''`).join(" || ") + " }}",
      ],
    ],
    [
      `build/${BUNDLE_STEP}`,
      APPLE_SIX.map((name) => buildGate(name, name === "APPLE_SIGNING_IDENTITY" ? "'-'" : "''")),
    ],
    [`build/${CHECK_STEP}`, [buildGate("APPLE_SIGNING_IDENTITY")]],
  ]);
  const seen = new Map();
  const jobs = [...text.matchAll(/^ {2}([a-z0-9-]+):\s*$/gm)].map((match) => match[1]);
  for (const job of jobs) {
    // Outside any step: the job's header (env, outputs) must hold none.
    const header = jobLines(text, job).slice(0, jobLines(text, job).findIndex((line) => /^ {4}steps:/.test(line)));
    if (header.some((line) => /secrets\.APPLE_/.test(stripComment(line)))) {
      problems.push(`job ${job} holds an Apple secret outside a step`);
    }
    for (const step of stepsOf(text, job)) {
      const mentions = step.lines.map(stripComment).filter((line) => /secrets\.APPLE_/.test(line));
      if (mentions.length === 0) {
        continue;
      }
      const where = `${job}/${step.name}`;
      if (/secrets\./.test(step.run)) {
        problems.push(`${where} puts a secret in its run: script`);
      }
      const want = allowed.get(where);
      if (want === undefined) {
        problems.push(`${where} holds an Apple secret, and is not one of the steps that may`);
        continue;
      }
      seen.set(where, true);
      const have = step.env.filter((line) => /secrets\.APPLE_/.test(line));
      if (mentions.length !== have.length) {
        problems.push(`${where} mentions an Apple secret outside its env`);
      }
      if (JSON.stringify([...have].sort()) !== JSON.stringify([...want].sort())) {
        problems.push(`${where}'s Apple env is ${JSON.stringify(have)}, not ${JSON.stringify(want)}`);
      }
    }
  }
  for (const where of allowed.keys()) {
    if (!seen.has(where)) {
      problems.push(`${where} is missing, or holds no Apple secret`);
    }
  }
  // Build's switch: a macOS leg of a v* release with custody's has_apple.
  const developerId =
    "DEVELOPER_ID: ${{ needs.check.outputs.mode == 'release' && needs.custody.outputs.has_apple == 'true' && matrix.signs }}";
  if (!jobLines(text, "build").map((line) => stripComment(line).trim()).includes(developerId)) {
    problems.push("build's env has no DEVELOPER_ID, or another one");
  }
  return problems;
}

test("an Apple secret appears only on the steps that use it, behind the mode or DEVELOPER_ID", () => {
  const text = workflow();
  assert.deepEqual(appleSecretProblems(text), []);
  const breaks = [
    // On another step.
    text.replace(
      "      - name: Collect this leg's assets\n",
      "      - name: Collect this leg's assets\n        env:\n          LEAK: ${{ secrets.APPLE_API_KEY_P8 }}\n",
    ),
    // In a run: script.
    text.replace('          node .github/scripts/apple-notary.mjs --check\n', '          echo "${{ secrets.APPLE_API_KEY }}"\n'),
    // Not behind DEVELOPER_ID.
    text.replace(buildGate("APPLE_CERTIFICATE"), "APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}"),
    // A test run would sign: the switch no longer asks for release mode.
    text.replace("DEVELOPER_ID: ${{ needs.check.outputs.mode == 'release' && ", "DEVELOPER_ID: ${{ "),
    // On a job.
    text.replace("      DEVELOPER_ID: ${{", "      CERT: ${{ secrets.APPLE_CERTIFICATE }}\n      DEVELOPER_ID: ${{"),
    // has_apple forgets a name.
    text.replace(" || secrets.APPLE_API_KEY_P8 != ''", ""),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(appleSecretProblems(broken), [], `break ${i} is caught`);
  }
});

// ---- codesign: --timestamp and --options runtime ----

/** Every signing codesign in build: helpers with runtime, the Developer ID's timestamped. */
function codesignProblems(text, conf) {
  const problems = [];
  const bundle = stepNamed(text, "build", BUNDLE_STEP);
  if (bundle === undefined) {
    return ["build has no bundle step"];
  }
  const signing = jobLines(text, "build")
    .map((line) => stripComment(line).trim())
    .filter((line) => /^codesign\b.*\s(-s|--sign)\s/.test(line));
  if (signing.length === 0) {
    problems.push("build signs nothing with codesign");
  }
  for (const line of signing) {
    const developerId = line.includes("--keychain");
    if (developerId && !/\s--timestamp(\s|$)/.test(line)) {
      problems.push(`a Developer ID codesign without --timestamp: ${line}`);
    }
    if (!developerId && line.includes("--timestamp")) {
      problems.push(`an ad hoc codesign asks for a timestamp: ${line}`);
    }
    if (line.endsWith('"$helper"') && !/\s--options runtime\s/.test(line)) {
      problems.push(`a helper signed without --options runtime: ${line}`);
    }
    if (!line.endsWith('"$helper"') && !line.endsWith('"$dmg"')) {
      problems.push(`a codesign of something other than a helper or the DMG: ${line}`);
    }
  }
  const lines = bundle.run.split("\n").map((line) => line.trim());
  const developerIdHelper =
    'codesign --force -s "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" --options runtime --timestamp "$helper"';
  if (!lines.includes(developerIdHelper)) {
    problems.push("the bundle step does not sign the helpers with the Developer ID, runtime and timestamp");
  }
  // The bundler's own codesign of the sidecars, main binary and .app: --options runtime comes
  // from hardenedRuntime, and the entitlements from this file.
  if (conf.bundle?.macOS?.hardenedRuntime !== true) {
    problems.push("tauri.conf.json's bundle.macOS.hardenedRuntime is not true");
  }
  return problems;
}

test("every Developer ID codesign has --timestamp, and every one on code --options runtime", () => {
  const text = workflow();
  const conf = JSON.parse(readFileSync(TAURI_CONF, "utf8"));
  assert.deepEqual(codesignProblems(text, conf), []);
  const helper = 'codesign --force -s "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" --options runtime --timestamp "$helper"';
  const dmg = 'codesign --force -s "$APPLE_SIGNING_IDENTITY" --keychain "$keychain" --timestamp "$dmg"';
  const breaks = [
    text.replace(helper, helper.replace(" --timestamp", "")),
    text.replace(helper, helper.replace(" --options runtime", "")),
    text.replace(dmg, dmg.replace(" --timestamp", "")),
    text.replace(
      'codesign --force -s "$APPLE_SIGNING_IDENTITY" --options runtime "$helper"',
      'codesign --force -s "$APPLE_SIGNING_IDENTITY" --options runtime --timestamp "$helper"',
    ),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(codesignProblems(broken, conf), [], `break ${i} is caught`);
  }
  assert.notDeepEqual(codesignProblems(text, { bundle: { macOS: { hardenedRuntime: false } } }), []);
});

// ---- Notarisation, stapling and the keychain only on a Developer ID leg ----

/** For each line of <script>, the if-conditions it runs under ("!" before an else branch's). */
function conditions(script) {
  const stack = [];
  return script.split("\n").map((raw) => {
    const line = raw.trim();
    const opened = line.match(/^if (.*); then$/);
    if (opened) {
      stack.push(opened[1]);
      return [...stack.slice(0, -1)];
    }
    if (line === "else") {
      stack.push(`!${stack.pop()}`);
      return [...stack.slice(0, -1)];
    }
    if (line === "fi") {
      stack.pop();
      return [...stack];
    }
    return [...stack];
  });
}

const ON_DEVELOPER_ID = '[ "$DEVELOPER_ID" = true ]';

/** What runs outside a Developer ID leg that must not, and what the Developer ID path lacks. */
function developerIdProblems(text) {
  const problems = [];
  const bundle = stepNamed(text, "build", BUNDLE_STEP);
  const check = stepNamed(text, "build", CHECK_STEP);
  if (bundle === undefined || check === undefined) {
    return ["build's bundle step or Developer ID check is missing"];
  }
  if (check.if !== "env.DEVELOPER_ID == 'true'") {
    problems.push(`the Developer ID check runs under ${check.if}, not env.DEVELOPER_ID == 'true'`);
  }
  if (!(Number(bundle.timeout) > 0)) {
    problems.push("the bundle step, which waits on the notary service, has no timeout-minutes");
  }
  const lines = bundle.run.split("\n").map((line) => line.trim());
  const under = conditions(bundle.run);
  const onlyDeveloperId = [
    /^security (create-keychain|import|set-key-partition-list|list-keychains)\b/,
    /\bxcrun notarytool submit\b/,
    /\bxcrun notarytool log\b/,
    /^xcrun stapler\b/,
    /^APPLE_API_KEY_PATH=/,
    /^printf .*APPLE_API_KEY_P8/,
  ];
  const seen = new Set();
  lines.forEach((line, i) => {
    for (const pattern of onlyDeveloperId) {
      if (pattern.test(line)) {
        seen.add(String(pattern));
        if (!under[i].includes(ON_DEVELOPER_ID)) {
          problems.push(`"${line}" runs outside a Developer ID leg`);
        }
      }
    }
  });
  for (const pattern of onlyDeveloperId) {
    if (!seen.has(String(pattern))) {
      problems.push(`the bundle step has no ${pattern}`);
    }
  }
  const bundled = lines.findIndex((line) => /\btauri bundle\b/.test(line));
  const unset = lines.indexOf("unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_API_KEY_P8");
  if (unset === -1 || unset > bundled || under[unset].length > 0) {
    problems.push("the certificate and the .p8's text are not unset, on every leg, before tauri bundle");
  }
  const submit = lines.findIndex((line) => line.includes("xcrun notarytool submit"));
  if (submit !== -1) {
    if (!/--wait --timeout \d+m/.test(lines[submit]) || submit < bundled) {
      problems.push("the DMG's notarytool submit does not wait, bounded, after the bundle");
    }
    const after = lines.slice(submit);
    const refused = after.findIndex((line) => line === 'if [ "$verdict" != Accepted ]; then');
    const log = after.findIndex((line) => line.includes("xcrun notarytool log"));
    const staple = after.findIndex((line) => line === 'xcrun stapler staple "$dmg"');
    const validate = after.findIndex((line) => line.startsWith('xcrun stapler validate "$dmg"'));
    if (!(refused !== -1 && refused < log && log < staple && staple < validate)) {
      problems.push("a refused DMG does not print its notary log and fail before stapling, then validate");
    }
    if (!after.slice(log, staple).includes("exit 1")) {
      problems.push("a refused DMG does not fail the step");
    }
  }
  if (!lines.some((line) => line.startsWith("trap ") && line.includes("delete-keychain") && line.includes("rm -rf"))) {
    problems.push("the keychain and the .p8 are not removed when the step ends");
  }
  return problems;
}

test("notarisation, stapling and the keychain happen only on a Developer ID leg, bounded in time", () => {
  const text = workflow();
  assert.deepEqual(developerIdProblems(text), []);
  const breaks = [
    text.replace("        if: env.DEVELOPER_ID == 'true'\n", "        if: runner.os == 'macOS'\n"),
    text.replace("        timeout-minutes: 50\n", ""),
    // The DMG's notarisation moved outside the Developer ID branch.
    text.replace(
      '          if [ "$DEVELOPER_ID" = true ]; then\n            dmg=$RELEASE_DIR/bundle/dmg/',
      '          if true; then\n            dmg=$RELEASE_DIR/bundle/dmg/',
    ),
    text.replace("--wait --timeout 20m --output-format json", "--output-format json"),
    text.replace('              [ -z "$id" ] || xcrun notarytool log "$id" "${notary[@]}" || true\n', ""),
    text.replace("          unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_API_KEY_P8\n", ""),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(developerIdProblems(broken), [], `break ${i} is caught`);
  }
});

// ---- The Developer ID check ----

/** The checks the Developer ID step must hold, in order where order matters. */
function checkStepProblems(text) {
  const problems = [];
  const steps = stepsOf(text, "build");
  const at = (name) => steps.findIndex((step) => step.name === name);
  const check = steps[at(CHECK_STEP)];
  if (check === undefined) {
    return ["build has no Developer ID check"];
  }
  if (!(at(BUNDLE_STEP) < at(CHECK_STEP) && at(CHECK_STEP) < at("Collect this leg's assets"))) {
    problems.push("the Developer ID check is not after the bundle and before the assets are collected");
  }
  const script = check.run;
  const wanted = [
    'grep -qxF "Authority=$APPLE_SIGNING_IDENTITY" <<< "$info"',
    'grep -qx "TeamIdentifier=$team" <<< "$info"',
    "grep -q '^Timestamp=' <<< \"$info\"",
    "grep -q ' flags=0x10000(runtime) ' <<< \"$info\"",
    'codesign -d --entitlements - --xml "$1"',
    "for left_out in get-task-allow ",
    'spctl -a -vv -t exec "$app"',
    'spctl -a -vv -t open --context context:primary-signature "$dmg"',
    "grep -qx 'source=Notarized Developer ID' <<< \"$assessed\"",
    'xcrun stapler validate "$app"',
    'xcrun stapler validate "$dmg"',
    'xcrun stapler validate "$copy"',
    'codesign --verify --deep --strict --verbose=2 "$copy"',
    'while IFS= read -r macho; do code "$app/$macho"; done <<< "$have"',
    'code "$app"',
    'signed "$dmg"',
  ];
  for (const words of wanted) {
    if (!script.includes(words)) {
      problems.push(`the Developer ID check has no ${words}`);
    }
  }
  // Gatekeeper's verdict is read for the .app and for the DMG.
  const notarized = script.split("grep -qx 'source=Notarized Developer ID' <<< \"$assessed\"").length - 1;
  if (notarized !== 2) {
    problems.push(`Gatekeeper's "Notarized Developer ID" is checked ${notarized} times, not twice (the .app, the DMG)`);
  }
  // The left-out entitlements the check refuses: every one the decision leaves out.
  const loop = script.match(/for left_out in ([^;]*?); do/s);
  const listed = loop ? loop[1].replace(/\\\n/g, " ").split(/\s+/).filter(Boolean) : [];
  for (const name of LEFT_OUT) {
    if (!listed.includes(name.replace("com.apple.security.", ""))) {
      problems.push(`the Developer ID check does not refuse ${name}`);
    }
  }
  // The Mach-O list is found, then compared with exactly the expected one.
  if (!/file -N -F '\|'/.test(script) || !script.includes('[ "$have" = "$want" ] ||')) {
    problems.push("the Developer ID check does not compare every Mach-O file found with the expected list");
  }
  if (!script.includes('"$archive"') || !script.includes('hdiutil attach')) {
    problems.push("the Developer ID check does not open the DMG and the updater archive");
  }
  return problems;
}

test("the Developer ID check holds its codesign, spctl and stapler checks", () => {
  const text = workflow();
  assert.deepEqual(checkStepProblems(text), []);
  const breaks = [
    text.replace("source=Notarized Developer ID' <<< \"$assessed\" || fail \"Gatekeeper does not see $app", "source=Developer ID' <<< \"$assessed\" || fail \"Gatekeeper does not see $app"),
    text.replace('          xcrun stapler validate "$dmg" || fail "$dmg has no valid', '          true || fail "$dmg has no valid'),
    text.replace("for left_out in get-task-allow ", "for left_out in "),
    text.replace("for left_out in get-task-allow cs.allow-jit ", "for left_out in get-task-allow "),
    text.replace("grep -q ' flags=0x10000(runtime) ' <<< \"$info\"", "true"),
    text.replace('spctl -a -vv -t open --context context:primary-signature "$dmg"', 'spctl -a -vv -t open "$dmg"'),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(checkStepProblems(broken), [], `break ${i} is caught`);
  }
  // Moved after the upload.
  const block = stepNamed(text, "build", CHECK_STEP).lines.join("\n");
  const moved = text.replace(`${block}\n`, "").replace("      # A dry run's throwaway public key", `${block}\n      # A dry run's throwaway public key`);
  assert.notEqual(moved, text);
  assert.notDeepEqual(checkStepProblems(moved), []);
});

// ---- custody refuses a partial set: its script, run ----

const haveBash = spawnSync("bash", ["--version"]).status === 0;
const haveOpenssl = spawnSync("openssl", ["version"]).status === 0;

/** Runs custody's Apple step's script with <env> (names to values); node's notary call faked. */
function runCustodyCheck(script, env) {
  const dir = mkdtempSync(join(tmpdir(), "custody-apple-"));
  try {
    const bin = join(dir, "bin");
    spawnSync("mkdir", ["-p", bin]);
    // `node .github/scripts/apple-notary.mjs --check` answers "accepted" without the network.
    writeFileSync(
      join(bin, "node"),
      `#!/bin/sh\nif [ "$1" = .github/scripts/apple-notary.mjs ]; then echo "notary: faked"; exit 0; fi\nexec "${process.execPath}" "$@"\n`,
    );
    chmodSync(join(bin, "node"), 0o755);
    const summary = join(dir, "summary.md");
    writeFileSync(summary, "");
    const full = {
      PATH: `${bin}:${process.env.PATH}`,
      RUNNER_TEMP: dir,
      GITHUB_STEP_SUMMARY: summary,
      HAS_APPLE_ID: "false",
      ...Object.fromEntries(APPLE_SIX.map((name) => [name, ""])),
      ...env,
    };
    const result = spawnSync("bash", ["-c", script], { cwd: ROOT, env: full, encoding: "utf8" });
    return { status: result.status, out: `${result.stdout}${result.stderr}`, summary: readFileSync(summary, "utf8") };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** A Developer ID-like .p12 (base64) from a throwaway CA, and its password. */
function throwawayIdentity(dir, { team = "ABCDE12345", name = "Test Person", issuer = "Developer ID Certification Authority", days = 30 } = {}) {
  const run = (...args) => {
    const result = spawnSync("openssl", args, { cwd: dir, encoding: "utf8" });
    assert.equal(result.status, 0, `openssl ${args.join(" ")}: ${result.stderr}`);
  };
  const ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes"];
  run("req", "-x509", ...ec, "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", `/CN=${issuer}`);
  const cn = `Developer ID Application: ${name} (${team})`;
  run("req", "-new", ...ec, "-keyout", "leaf.key", "-out", "leaf.csr", "-subj", `/UID=${team}/CN=${cn}/OU=${team}/O=${name}/C=IN`);
  run("x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-days", String(days), "-out", "leaf.pem");
  const password = "throwaway-p12-password";
  run("pkcs12", "-export", "-inkey", "leaf.key", "-in", "leaf.pem", "-passout", `pass:${password}`, "-out", "id.p12");
  return { p12: readFileSync(join(dir, "id.p12")).toString("base64"), password, identity: cn };
}

/** What custody's script gets wrong, run against each set of secrets it must refuse or pass. */
function custodyProblems(script) {
  const problems = [];
  const none = runCustodyCheck(script, {});
  if (none.status !== 0 || !none.out.includes("no Apple secret")) {
    problems.push(`with no Apple secret it does not pass and say so (${none.status})`);
  }
  const partial = runCustodyCheck(script, {
    APPLE_CERTIFICATE: "c2VjcmV0LWNlcnQ=",
    APPLE_CERTIFICATE_PASSWORD: "secret-password-value",
  });
  const missing = ["APPLE_SIGNING_IDENTITY", "APPLE_API_KEY", "APPLE_API_ISSUER", "APPLE_API_KEY_P8"];
  if (partial.status === 0 || !missing.every((name) => partial.out.includes(name))) {
    problems.push(`a partial set is not refused with the missing names (${partial.status}: ${partial.out.trim()})`);
  }
  if (/secret-password-value|c2VjcmV0LWNlcnQ=/.test(partial.out)) {
    problems.push("the refusal prints a secret's value");
  }
  const appleId = runCustodyCheck(script, { HAS_APPLE_ID: "true" });
  if (appleId.status === 0 || !appleId.out.includes("APPLE_ID")) {
    problems.push("Tauri's Apple ID names are not refused");
  }
  return problems;
}

test("custody refuses a partial set of Apple secrets, naming only the missing names", { skip: !haveBash || process.platform === "win32" }, () => {
  const step = stepNamed(workflow(), "custody", CUSTODY_STEP);
  assert.ok(step, "custody's Apple step");
  assert.equal(step.if, "needs.check.outputs.mode == 'release'");
  const custody = stepsOf(workflow(), "custody").map((one) => one.name);
  assert.ok(custody.indexOf(CUSTODY_STEP) < custody.indexOf("Install the ui workspace"), "before the ui install");
  assert.deepEqual(custodyProblems(step.run), []);
  // Broken on purpose: the missing names are not checked, so a partial set gets further.
  const broken = step.run.replace('[ "${#missing[@]}" = 0 ] ||', "true ||");
  assert.notEqual(broken, step.run);
  assert.notDeepEqual(custodyProblems(broken), []);
  const noAppleId = step.run.replace('[ "$HAS_APPLE_ID" = false ] ||', "true ||");
  assert.notEqual(noAppleId, step.run);
  assert.notDeepEqual(custodyProblems(noAppleId), []);
});

test("custody opens the certificate and checks its identity, team, issuer and dates", { skip: !haveBash || !haveOpenssl || process.platform === "win32" }, () => {
  const script = stepNamed(workflow(), "custody", CUSTODY_STEP).run;
  const dir = mkdtempSync(join(tmpdir(), "custody-cert-"));
  try {
    const good = throwawayIdentity(dir);
    const secrets = {
      APPLE_CERTIFICATE: good.p12,
      APPLE_CERTIFICATE_PASSWORD: good.password,
      APPLE_SIGNING_IDENTITY: good.identity,
      APPLE_API_KEY: "ABCDEFGHIJ",
      APPLE_API_ISSUER: "00000000-0000-0000-0000-000000000000",
      APPLE_API_KEY_P8: "not read: node is faked",
    };
    const passed = runCustodyCheck(script, secrets);
    assert.equal(passed.status, 0, passed.out);
    assert.match(passed.summary, /Developer ID of team ABCDE12345, valid until/);
    assert.ok(!passed.out.includes(good.password) && !passed.out.includes(good.p12.slice(0, 40)), "no secret printed");
    const refused = (env, words) => {
      const result = runCustodyCheck(script, { ...secrets, ...env });
      assert.notEqual(result.status, 0, words);
      assert.ok(result.out.includes(words), `${words}: ${result.out}`);
      assert.ok(!result.out.includes(good.password), "no secret printed");
    };
    refused({ APPLE_CERTIFICATE_PASSWORD: "wrong" }, "does not open with APPLE_CERTIFICATE_PASSWORD");
    refused({ APPLE_SIGNING_IDENTITY: "Developer ID Application: Someone Else (ABCDE12345)" }, "is not APPLE_SIGNING_IDENTITY's");
    refused({ APPLE_SIGNING_IDENTITY: "Apple Development: Test Person (ABCDE12345)" }, "is not \"Developer ID Application");
    refused({ APPLE_CERTIFICATE: "%%%" }, "is not base64");
    const other = mkdtempSync(join(tmpdir(), "custody-cert-"));
    try {
      const selfIssued = throwawayIdentity(other, { issuer: "Clauseo Code Signing" });
      refused({ APPLE_CERTIFICATE: selfIssued.p12 }, "not issued by Developer ID Certification Authority");
      const expired = throwawayIdentity(other, { days: 0 });
      refused({ APPLE_CERTIFICATE: expired.p12 }, "the Developer ID certificate expired");
      const otherTeam = throwawayIdentity(other, { team: "ZZZZZ99999" });
      refused({ APPLE_CERTIFICATE: otherTeam.p12, APPLE_SIGNING_IDENTITY: good.identity }, "is not APPLE_SIGNING_IDENTITY's");
    } finally {
      rmSync(other, { recursive: true, force: true });
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ---- The entitlements file ----

/** What is wrong with the entitlements tauri.conf.json names. */
function entitlementProblems(conf, plist) {
  const problems = [];
  if (conf.bundle?.macOS?.entitlements !== "macos/app.entitlements.plist") {
    problems.push("tauri.conf.json's bundle.macOS.entitlements is not macos/app.entitlements.plist");
  }
  if (plist === null) {
    return [...problems, "the entitlements file is missing"];
  }
  const body = plist.replace(/<!--[\s\S]*?-->/g, "");
  if (!/<plist version="1\.0">\s*<dict(\/>|>[\s\S]*<\/dict>)\s*<\/plist>\s*$/.test(body)) {
    problems.push("the entitlements file is not one plist dict");
  }
  const keys = [...body.matchAll(/<key>\s*([^<]*?)\s*<\/key>/g)].map((match) => match[1]);
  for (const key of keys) {
    if (LEFT_OUT.includes(key)) {
      problems.push(`the entitlements file holds ${key}`);
    }
  }
  // Decision 0035 keeps none; one added is a decision, made there and here.
  if (keys.length > 0) {
    problems.push(`the entitlements file holds ${keys.join(", ")}, which decision 0035 does not keep`);
  }
  if (/get-task-allow/.test(body)) {
    problems.push("the entitlements file mentions get-task-allow outside a comment");
  }
  return problems;
}

test("the entitlements file holds none of the entitlements decision 0035 leaves out, get-task-allow first", () => {
  const conf = JSON.parse(readFileSync(TAURI_CONF, "utf8"));
  const plist = readFileSync(join(ROOT, "crates", "desktop", conf.bundle.macOS.entitlements ?? "missing"), "utf8");
  assert.deepEqual(entitlementProblems(conf, plist), []);
  const withKey = (key) => plist.replace("<dict/>", `<dict>\n\t<key>${key}</key>\n\t<true/>\n</dict>`);
  assert.notEqual(withKey("x"), plist);
  assert.ok(entitlementProblems(conf, withKey("com.apple.security.get-task-allow")).includes("the entitlements file holds com.apple.security.get-task-allow"));
  assert.notDeepEqual(entitlementProblems(conf, withKey("com.apple.security.cs.allow-jit")), []);
  assert.notDeepEqual(entitlementProblems({ bundle: { macOS: {} } }, plist), []);
  assert.notDeepEqual(entitlementProblems(conf, null), []);
});

// ---- apple-notary.mjs ----

function throwawayP8() {
  const { privateKey } = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
  return privateKey.export({ type: "pkcs8", format: "pem" });
}

const goodEnv = (p8) => ({
  APPLE_API_KEY: "ABCDEFGHIJ",
  APPLE_API_ISSUER: "00000000-0000-0000-0000-000000000000",
  APPLE_API_KEY_P8: p8,
});

test("notaryToken is an ES256 token for the App Store Connect API that the key's public half verifies", () => {
  const p8 = throwawayP8();
  const token = notaryToken({ keyId: "ABCDEFGHIJ", issuer: "issuer-id", privateKeyPem: p8, now: 1_000 });
  const [header, claims, signature] = token.split(".");
  assert.deepEqual(JSON.parse(Buffer.from(header, "base64url")), { alg: "ES256", kid: "ABCDEFGHIJ", typ: "JWT" });
  assert.deepEqual(JSON.parse(Buffer.from(claims, "base64url")), { iss: "issuer-id", iat: 1_000, exp: 1_600, aud: "appstoreconnect-v1" });
  const raw = Buffer.from(signature, "base64url");
  assert.equal(raw.length, 64, "JOSE's r||s, not DER");
  const ok = verify("sha256", Buffer.from(`${header}.${claims}`), { key: createPublicKey(p8), dsaEncoding: "ieee-p1363" }, raw);
  assert.ok(ok);
});

test("credentialProblems names each wrong value by name, never its value", () => {
  assert.deepEqual(credentialProblems(goodEnv(throwawayP8())), []);
  const problems = credentialProblems({ APPLE_API_KEY: "key-value-1", APPLE_API_ISSUER: "issuer-value", APPLE_API_KEY_P8: "p8-value" });
  assert.equal(problems.length, 3);
  assert.ok(!problems.join(" ").match(/key-value-1|issuer-value|p8-value/));
  const { privateKey } = generateKeyPairSync("rsa", { modulusLength: 1024 });
  assert.equal(credentialProblems({ ...goodEnv(""), APPLE_API_KEY_P8: privateKey.export({ type: "pkcs8", format: "pem" }) }).length, 1);
});

test("notaryProblem passes a 200, and names the status of a refusal without the key or the token", async () => {
  const p8 = throwawayP8();
  let asked = null;
  const ok = await notaryProblem(goodEnv(p8), async (url, init) => {
    asked = { url, auth: init.headers.Authorization };
    return { status: 200, json: async () => ({ data: [] }) };
  });
  assert.equal(ok, null);
  assert.equal(asked.url, SUBMISSIONS_URL);
  assert.match(asked.auth, /^Bearer [\w-]+\.[\w-]+\.[\w-]+$/);
  const refused = await notaryProblem(goodEnv(p8), async () => ({
    status: 401,
    json: async () => ({ errors: [{ status: "401", title: "Authentication credentials are missing or invalid." }] }),
  }));
  assert.match(refused, /HTTP 401 \(Authentication credentials are missing or invalid\.\)/);
  assert.ok(!refused.includes(p8.split("\n")[1]) && !refused.includes(asked.auth.slice(7)), "no key, no token");
  const down = await notaryProblem(goodEnv(p8), async () => {
    throw Object.assign(new Error("x"), { name: "TimeoutError" });
  });
  assert.match(down, /did not answer \(TimeoutError\)/);
  // A wrong shape is refused before any request.
  let called = false;
  const shape = await notaryProblem({ ...goodEnv(p8), APPLE_API_KEY: "short" }, async () => {
    called = true;
  });
  assert.match(shape, /APPLE_API_KEY is not a Key ID/);
  assert.equal(called, false);
});
