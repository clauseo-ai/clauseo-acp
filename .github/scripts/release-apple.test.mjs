// Tests for release.yml's Apple Developer ID path (decision 0035, "Apple Developer ID") and for
// apple-notary.mjs. Plain Node, no install, no network:
//
//   node --test .github/scripts/release-apple.test.mjs
//
// release.yml is read as text, as latest-json.test.mjs reads it. The rules: every mention of
// `secrets` in the whole workflow, in any syntax, is on a step and env key SECRET_TABLE lists;
// no step turns on shell tracing; no upload, and no copy into an uploaded folder, reaches
// RUNNER_TEMP; an Apple secret appears only on the steps that use it (custody's decision and its
// check, build's bundle step and its Developer ID check), always behind the mode or DEVELOPER_ID,
// never in a run: script; the Team ID is the one committed in crates/desktop/release/apple-team-id;
// every Developer ID codesign has --timestamp, and every one on code --options runtime;
// notarisation, stapling and the keychain happen only on a Developer ID leg, and only Accepted
// staples; the Developer ID check holds its spctl, stapler and codesign checks; custody refuses a
// partial set of Apple secrets (its script is run here, with a throwaway certificate when openssl
// is there); the
// entitlements file holds none of the entitlements the decision leaves out; and apple-setup.sh
// runs by hand only. Each rule is a function that returns what is wrong, and each test breaks the
// workflow on purpose to show the rule catches it.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createPublicKey, generateKeyPairSync, verify } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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
const TEAM_ID_FILE = "crates/desktop/release/apple-team-id";
// How every reader of the committed Team ID reads it: comments and white space dropped.
const TEAM_ID_READ = "sed 's/#.*//' crates/desktop/release/apple-team-id | tr -d '[:space:]'";

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
      [...APPLE_SIX, ...APPLE_ID_NAMES].map(custodyGate),
    ],
    [
      `build/${BUNDLE_STEP}`,
      [...APPLE_SIX, ...APPLE_ID_NAMES].map((name) => buildGate(name, name === "APPLE_SIGNING_IDENTITY" ? "'-'" : "''")),
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

// ---- Every secret, in any syntax, in the whole workflow: only where the table allows ----

const APPLE_ALL = [...APPLE_SIX, ...APPLE_ID_NAMES];
const RELEASE_KEY_STEP = "The release key signs, and tauri.conf.json's pubkey verifies it";
const TAURI_RELEASE = {
  TAURI_SIGNING_PRIVATE_KEY: ["TAURI_SIGNING_PRIVATE_KEY"],
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"],
};
const TAURI_RELEASE_OR_TEST = {
  TAURI_SIGNING_PRIVATE_KEY: ["TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_TEST"],
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ["TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "TAURI_SIGNING_PRIVATE_KEY_TEST_PASSWORD"],
};
const eachItself = (names) => Object.fromEntries(names.map((name) => [name, [name]]));

/**
 * Every step of release.yml that may mention `secrets`, and on which env key, which secrets.
 * A mention anywhere else (another step, a job's or the workflow's env, `with:`, `if:`, a run:
 * script, `secrets: inherit`), or in any other syntax (`secrets['X']`, `toJSON(secrets)`, a bare
 * `secrets`), fails. A new secret is a decision, made here and in the workflow together.
 */
const SECRET_TABLE = [
  { job: "custody", step: "Decide the unsigned beta", env: { HAS_APPLE: APPLE_ALL } },
  { job: "custody", step: CUSTODY_STEP, env: eachItself(APPLE_ALL) },
  { job: "custody", step: RELEASE_KEY_STEP, env: TAURI_RELEASE },
  { job: "build", step: BUNDLE_STEP, env: { ...eachItself(APPLE_ALL), ...TAURI_RELEASE_OR_TEST } },
  { job: "build", step: CHECK_STEP, env: eachItself(["APPLE_SIGNING_IDENTITY"]) },
  { job: "sign-windows", step: "Sign the sandbox tool for the updater", env: TAURI_RELEASE_OR_TEST },
  { job: "sign-windows", step: "Sign the currentUser installer for the updater", env: TAURI_RELEASE_OR_TEST },
  { job: "publish", step: "Record the release (S23)", env: eachItself(["CLAUSEO_RELEASE_ACCESS_CLIENT_ID", "CLAUSEO_RELEASE_ACCESS_CLIENT_SECRET"]) },
];

/**
 * Each line of <text> with where it sits: its job, its step (index and name, or null), whether
 * it is the body of a block scalar (a run: script: no YAML comments there, and GitHub expands
 * `${{ }}` even in a bash comment), and the env key it sets when it is a step's env entry.
 */
function placedLines(text) {
  const lines = text.split("\n");
  const placed = [];
  let inJobs = false;
  let job = null;
  let step = null;
  let steps = 0;
  let block = null;
  let inEnv = false;
  lines.forEach((raw, i) => {
    const indent = raw.length - raw.trimStart().length;
    if (block !== null && (raw.trim() === "" || indent > block)) {
      placed.push({ no: i + 1, raw, code: raw, job, step, scalar: true, envKey: null });
      return;
    }
    block = null;
    const code = stripComment(raw);
    if (/^jobs:\s*$/.test(code)) {
      inJobs = true;
    } else if (/^\S/.test(code)) {
      inJobs = false;
      job = null;
    }
    const jobHead = inJobs ? code.match(/^ {2}([A-Za-z0-9_-]+):\s*$/) : null;
    if (jobHead) {
      job = jobHead[1];
      step = null;
      inEnv = false;
    } else if (job !== null && /^ {6}- /.test(raw)) {
      steps += 1;
      step = { id: steps, name: null, at: i + 1 };
      inEnv = false;
    } else if (/^ {0,5}\S/.test(code)) {
      step = null;
      inEnv = false;
    }
    if (step !== null) {
      const named = code.match(/^ {6}(- | {2})name: (.*)$/);
      if (named) {
        step.name = named[2].trim();
      }
      if (/^ {8}\S/.test(code)) {
        inEnv = /^ {8}env:\s*$/.test(code);
      }
    }
    const entry = step !== null && inEnv ? code.match(/^ {10}([A-Za-z_][A-Za-z0-9_]*): (.*)$/) : null;
    placed.push({ no: i + 1, raw, code, job, step, scalar: false, envKey: entry ? entry[1] : null, envValue: entry ? entry[2].trim() : null });
    if (/:\s*[|>][-+0-9]*\s*$/.test(code)) {
      block = indent;
    }
  });
  return placed;
}

const where = (line) =>
  `${line.job ?? "(no job)"}/${line.step === null ? "(no step)" : line.step.name ?? `(unnamed step at line ${line.step.at})`}, line ${line.no}`;

/** Every mention of `secrets` in <text> that the table does not allow, by job, step and line. */
function secretMentionProblems(text) {
  const problems = [];
  const placed = placedLines(text);
  // Offsets of each line's start, and of every ${{ }} span (which may run over lines).
  const starts = [];
  let offset = 0;
  for (const line of placed) {
    starts.push(offset);
    offset += line.raw.length + 1;
  }
  const spans = [...text.matchAll(/\$\{\{[\s\S]*?\}\}/g)].map((match) => [match.index, match.index + match[0].length]);
  const inSpan = (at) => spans.some(([from, to]) => at >= from && at < to);
  const lineAt = (at) => {
    let low = 0;
    while (low + 1 < starts.length && starts[low + 1] <= at) {
      low += 1;
    }
    return placed[low];
  };
  const mentioned = new Map();
  for (const match of text.matchAll(/\bsecrets\b/gi)) {
    const line = lineAt(match.index);
    const column = match.index - starts[line.no - 1];
    // Outside an expression: in YAML, a mention unless in a comment (`secrets: inherit`, an if:
    // condition); in a block scalar, plain text that GitHub does not expand.
    if (!inSpan(match.index) && (line.scalar || column >= line.code.length)) {
      continue;
    }
    if (!mentioned.has(line.no)) {
      mentioned.set(line.no, line);
    }
  }
  const seen = new Set();
  for (const line of mentioned.values()) {
    const entry = SECRET_TABLE.find((one) => one.job === line.job && line.step !== null && one.step === line.step.name);
    if (line.scalar) {
      problems.push(`${where(line)}: a secret in a run: script or another block (only a step's env may hold one)`);
      continue;
    }
    if (entry === undefined) {
      problems.push(`${where(line)}: mentions secrets, and the table lets no secret there`);
      continue;
    }
    if (line.envKey === null) {
      problems.push(`${where(line)}: mentions secrets outside the step's env`);
      continue;
    }
    // Every mention is secrets.NAME: never secrets[...], toJSON(secrets) or a bare secrets.
    const all = [...line.code.matchAll(/\bsecrets\b/gi)].length;
    const named = [...line.code.matchAll(/\bsecrets\.([A-Za-z_][A-Za-z0-9_]*)\b(?!\s*[[(])/gi)].map((match) => match[1]);
    if (named.length !== all) {
      problems.push(`${where(line)}: ${line.envKey} names secrets other than as secrets.NAME`);
      continue;
    }
    const want = entry.env[line.envKey];
    if (want === undefined) {
      problems.push(`${where(line)}: ${line.envKey} is not an env key the table lets hold a secret`);
      continue;
    }
    const have = [...new Set(named)].sort();
    if (JSON.stringify(have) !== JSON.stringify([...want].sort())) {
      problems.push(`${where(line)}: ${line.envKey} holds ${have.join(" ")}, the table ${[...want].sort().join(" ")}`);
      continue;
    }
    seen.add(`${entry.job}/${entry.step}/${line.envKey}`);
  }
  for (const entry of SECRET_TABLE) {
    for (const key of Object.keys(entry.env)) {
      if (!seen.has(`${entry.job}/${entry.step}/${key}`)) {
        problems.push(`${entry.job}/${entry.step}: ${key} is in the table but not in the workflow, or not as the table says`);
      }
    }
  }
  return problems;
}

const COLLECT = "      - name: Collect this leg's assets\n        if: runner.os != 'Windows'\n        run: |\n          set -euo pipefail\n";

test("every mention of secrets, in any syntax, is on a step and env key the table lists", () => {
  const text = workflow();
  assert.deepEqual(secretMentionProblems(text), []);
  const withEnv = (env) =>
    text.replace(COLLECT, `      - name: Collect this leg's assets\n        if: runner.os != 'Windows'\n        env:\n          ${env}\n        run: |\n          set -euo pipefail\n`);
  const breaks = [
    // The review's: every secret in one value, and one by index.
    [withEnv("ALL: ${{ toJSON(secrets) }}"), "build/Collect this leg's assets"],
    [withEnv("KEY: ${{ secrets['APPLE_API_KEY_P8'] }}"), "build/Collect this leg's assets"],
    [withEnv('KEY: ${{ secrets["TAURI_SIGNING_PRIVATE_KEY"] }}'), "build/Collect this leg's assets"],
    [withEnv("KEY: ${{ secrets[format('{0}_P8', 'APPLE_API_KEY')] }}"), "build/Collect this leg's assets"],
    [withEnv("KEY: ${{ SECRETS.APPLE_API_KEY_P8 }}"), "build/Collect this leg's assets"],
    // On a listed step, in another syntax or another key.
    [text.replace("          APPLE_TEAM_ID: ${{ env.DEVELOPER_ID == 'true' && secrets.APPLE_TEAM_ID || '' }}\n",
      "          APPLE_TEAM_ID: ${{ env.DEVELOPER_ID == 'true' && secrets.APPLE_TEAM_ID || '' }}\n          ALL: ${{ toJSON(secrets) }}\n"), BUNDLE_STEP],
    [text.replace("secrets.CLAUSEO_RELEASE_ACCESS_CLIENT_SECRET }}", "secrets.TAURI_SIGNING_PRIVATE_KEY }}"), "Record the release"],
    // In a run: script, even in a bash comment (GitHub expands it there too).
    [text.replace(COLLECT, `${COLLECT}          # \${{ secrets.APPLE_PASSWORD }}\n`), "a run: script"],
    // A job's env, an action's with:, and secrets: inherit.
    [text.replace("    env:\n      MODE: ${{ needs.check.outputs.mode }}\n      VERSION: ${{ needs.check.outputs.version }}\n    steps:\n",
      "    env:\n      MODE: ${{ needs.check.outputs.mode }}\n      VERSION: ${{ needs.check.outputs.version }}\n      ALL: ${{ toJSON(secrets) }}\n    steps:\n"), "custody/(no step)"],
    [text.replace("        with:\n          name: ui-dist\n", "        with:\n          name: ui-dist\n          token: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}\n"), "ui/"],
    [text.replace("    name: publish\n", "    name: publish\n    secrets: inherit\n"), "publish/(no step)"],
    // A table entry the workflow lost.
    [text.replace("          CLAUSEO_RELEASE_ACCESS_CLIENT_ID: ${{ secrets.CLAUSEO_RELEASE_ACCESS_CLIENT_ID }}\n", ""), "CLAUSEO_RELEASE_ACCESS_CLIENT_ID is in the table"],
  ];
  for (const [i, [broken, words]] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    const problems = secretMentionProblems(broken);
    assert.ok(problems.some((problem) => problem.includes(words)), `break ${i} is caught, naming ${words}: ${JSON.stringify(problems)}`);
  }
  // A problem names the line.
  assert.match(secretMentionProblems(withEnv("ALL: ${{ toJSON(secrets) }}"))[0], /, line \d+: /);
});

// ---- No shell tracing anywhere, so no step that holds a secret can print it ----

/** Every line of <text> that turns on shell tracing (set -x in any form, bash -x, PS4, ...). */
function tracingProblems(text) {
  const flags = String.raw`(?:[-+][A-Za-z]+\s+|-o\s+\S+\s+)*`;
  const patterns = [
    [new RegExp(String.raw`(^|[\s;&|({\x60])set\s+${flags}-[A-Za-z]*x`), "set -x"],
    [/\bxtrace\b/i, "xtrace"],
    [new RegExp(String.raw`\b(ba|da|z|k)?sh\s+${flags}-[A-Za-z]*x`), "a shell run with -x"],
    [/\bBASH_XTRACEFD\b/, "BASH_XTRACEFD"],
    [/\bPS4\b/, "PS4"],
    [/\bSHELLOPTS\b/, "SHELLOPTS"],
  ];
  const problems = [];
  for (const line of placedLines(text)) {
    // In a run: script, bash's own comments go; elsewhere, YAML's already have.
    const code = line.scalar ? stripComment(line.raw) : line.code;
    for (const [pattern, what] of patterns) {
      if (pattern.test(code)) {
        problems.push(`${where(line)}: ${what} (${code.trim()})`);
      }
    }
  }
  return problems;
}

test("no step turns on shell tracing, in any form", () => {
  const text = workflow();
  assert.deepEqual(tracingProblems(text), []);
  const bundleScript = `        run: |\n          set -euo pipefail\n          fail() { echo "::error::$1"; exit 1; }\n          if [ "$MODE" = dry ]; then\n`;
  assert.ok(text.includes(bundleScript));
  const breaks = [
    // The review's: in the bundle step, which holds every Apple secret.
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set -euxo pipefail")),
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set -euo pipefail; set -x")),
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set -e -x -o pipefail")),
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set -o xtrace")),
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set -euo pipefail && exec {fd}>&2 && BASH_XTRACEFD=$fd")),
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set -euo pipefail; PS4='$(env)'")),
    text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "bash -x -c true")),
    text.replace(`        timeout-minutes: 50\n`, `        timeout-minutes: 50\n        shell: bash -euxo pipefail {0}\n`),
    text.replace("    defaults:\n      run:\n        shell: bash\n    steps:\n      - uses: actions/checkout", "    defaults:\n      run:\n        shell: bash -x {0}\n    steps:\n      - uses: actions/checkout"),
    text.replace("          BETA_VARIABLE: ${{ vars.CLAUSEO_UNSIGNED_BETA == 'true' }}\n", "          BETA_VARIABLE: ${{ vars.CLAUSEO_UNSIGNED_BETA == 'true' }}\n          SHELLOPTS: xtrace\n"),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(tracingProblems(broken), [], `break ${i} is caught`);
  }
  // set +x (tracing off) and the -o options without x are fine.
  assert.deepEqual(tracingProblems(text.replace(bundleScript, bundleScript.replace("set -euo pipefail", "set +x; set -euo pipefail"))), []);
});

// ---- No upload can reach RUNNER_TEMP, where the keychain, the .p8 and the key files are ----

/** The RUNNER_TEMP paths an upload may name, each a file or folder no secret is written to. */
const TEMP_UPLOADS = [
  "${{ runner.temp }}/ui-sourcemaps",
  "${{ runner.temp }}/dry-updater.key.pub",
  "${{ runner.temp }}/smoke",
];

const COPIES = /^(cp|mv|ln|rsync|ditto|install|Copy-Item|Move-Item)$/;

/**
 * Where the command <segment> (one simple command, its redirections included) writes: its
 * redirection targets; a copy's last argument; tar's archive when it creates one; tee's files;
 * dd's of=. Words keep their quotes off.
 */
function destinations(segment) {
  const out = [];
  for (const match of segment.matchAll(/(?:^|[^<>&0-9])[0-9]?>{1,2}\s*([^\s&][^\s]*)/g)) {
    out.push(match[1]);
  }
  const words = segment.replace(/[0-9]?>{1,2}\s*[^\s]+/g, " ").trim().split(/\s+/).filter(Boolean);
  const [verb, ...args] = words;
  if (verb === undefined) {
    return out;
  }
  if (COPIES.test(verb) && args.length > 0) {
    out.push(args[args.length - 1]);
  } else if (verb === "tar") {
    const creates = args.some((arg) => /^-?[A-Za-z]*c[A-Za-z]*$/.test(arg) && !arg.startsWith("--") && /^-|^[a-z]+$/.test(arg));
    const file = args.findIndex((arg) => /^-?[A-Za-z]*f$/.test(arg));
    if (creates && file !== -1 && args[file + 1] !== undefined) {
      out.push(args[file + 1]);
    }
    for (const arg of args) {
      if (arg.startsWith("--file=")) {
        out.push(arg.slice(7));
      }
    }
  } else if (verb === "tee" || verb === "zip" || verb === "7z") {
    out.push(...args.filter((arg) => !arg.startsWith("-")));
  } else if (verb === "dd") {
    out.push(...args.filter((arg) => arg.startsWith("of=")).map((arg) => arg.slice(3)));
  }
  return out.map((word) => word.replace(/["']/g, "")).filter((word) => word !== "/dev/null");
}

/** Where an upload can reach RUNNER_TEMP: an upload path, or a copy into an uploaded folder. */
function uploadProblems(text) {
  const problems = [];
  const placed = placedLines(text);
  const steps = new Map();
  for (const line of placed) {
    if (line.step !== null) {
      steps.set(line.step.id, [...(steps.get(line.step.id) ?? []), line]);
    }
  }
  // Each upload-artifact step's path entries.
  const uploads = [];
  for (const lines of steps.values()) {
    if (!lines.some((line) => /^ {6}(- | {2})uses: actions\/upload-artifact@/.test(line.code))) {
      continue;
    }
    const at = lines.findIndex((line) => /^ {10}path:/.test(line.code));
    if (at === -1) {
      problems.push(`${where(lines[0])}: an upload with no path`);
      continue;
    }
    const inline = lines[at].code.replace(/^ {10}path:\s*/, "").trim();
    const entries = /^[|>]/.test(inline)
      ? lines.slice(at + 1).filter((line) => line.scalar).map((line) => line.raw.trim()).filter(Boolean)
      : [inline];
    for (const entry of entries) {
      uploads.push({ line: lines[at], entry });
    }
  }
  if (uploads.length === 0) {
    problems.push("no upload-artifact step found");
  }
  // A name's value in the job's env (or the workflow's).
  const envValue = (job, name) => {
    let block = null;
    const values = [];
    for (const line of placed) {
      if (line.step !== null || line.scalar || (line.job !== job && line.job !== null)) {
        continue;
      }
      if (/^env:\s*$/.test(line.code) || /^ {4}env:\s*$/.test(line.code)) {
        block = line.code.indexOf("env:") + 2;
        continue;
      }
      if (block !== null && line.code.trim() !== "" && line.code.length - line.code.trimStart().length < block) {
        block = null;
      }
      const match = block !== null ? line.code.match(new RegExp(`^ {${block}}${name}: (.*)$`)) : null;
      if (match) {
        values.push(match[1].trim());
      }
    }
    return values.length > 0 ? values.join(" ") : null;
  };
  const temp = /runner\.temp|RUNNER_TEMP/i;
  // Per job: the env names and the literal paths its uploads take.
  const roots = new Map();
  const rootsOf = (job) => roots.get(job) ?? roots.set(job, { names: new Set(), literals: new Set() }).get(job);
  for (const { line, entry } of uploads) {
    if (/(^|\/)\.\.(\/|$)/.test(entry) || /^[/~]/.test(entry)) {
      problems.push(`${where(line)}: the upload path ${entry} leaves the workspace`);
    }
    if (temp.test(entry)) {
      if (!TEMP_UPLOADS.includes(entry)) {
        problems.push(`${where(line)}: the upload path ${entry} reaches RUNNER_TEMP, and is not one of ${TEMP_UPLOADS.join(", ")}`);
      }
      continue;
    }
    for (const match of entry.matchAll(/\$\{\{\s*env\.([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g)) {
      const value = envValue(line.job, match[1]);
      if (value === null || temp.test(value)) {
        problems.push(`${where(line)}: the upload path ${entry} names env.${match[1]}, which is ${value ?? "not set in the job's env"}`);
      }
      rootsOf(line.job).names.add(match[1]);
    }
    if (!entry.startsWith("${{")) {
      // The literal path up to its first glob or expression.
      rootsOf(line.job).literals.add(entry.split(/[*?[]|\$\{\{/)[0].replace(/\/[^/]*$/, (tail) => (entry.includes("*") ? "" : tail)));
    }
  }
  // Copies: in each run: script, the names that hold a RUNNER_TEMP path and those that hold an
  // uploaded folder's (assignments, followed through); a command that writes into an uploaded
  // folder while it names a RUNNER_TEMP path fails, whatever it copies.
  for (const lines of steps.values()) {
    const first = lines[0];
    const job = rootsOf(first.job);
    // The script's commands, a backslash-continued one joined, each with its first line's number.
    const commands = [];
    let open = null;
    for (const line of lines.filter((one) => one.scalar)) {
      const code = stripComment(line.raw);
      if (open !== null) {
        open.code += ` ${code.trim()}`;
      } else {
        open = { no: line.no, code };
        commands.push(open);
      }
      if (/\\$/.test(open.code)) {
        open.code = open.code.slice(0, -1);
      } else {
        open = null;
      }
    }
    const tainted = new Set(["RUNNER_TEMP"]);
    const uploaded = new Set(job.names);
    const mentions = (set, code) => set.size > 0 && new RegExp(String.raw`\$\{?(?:${[...set].join("|")})\b`).test(code);
    const holdsTemp = (code) => mentions(tainted, code) || /runner\.temp/.test(code);
    const holdsUploaded = (code) =>
      mentions(uploaded, code) ||
      [...uploaded].some((name) => code.includes(`env.${name}`)) ||
      [...job.literals].some((root) => root !== "" && (code === root || code.startsWith(`${root}/`) || code.includes(`/${root}`)));
    for (const { code } of commands) {
      const assigned = code.trim().match(/^(?:local\s+|export\s+)?([A-Za-z_][A-Za-z0-9_]*)=(.*)$/);
      if (assigned && holdsTemp(assigned[2])) {
        tainted.add(assigned[1]);
      } else if (assigned && holdsUploaded(assigned[2])) {
        uploaded.add(assigned[1]);
      }
    }
    for (const { no, code } of commands) {
      const at = `${where(first).replace(/, line \d+$/, "")}, line ${no}`;
      for (const segment of code.split(/\|\||&&|[|;]/)) {
        if (!holdsTemp(segment) && !holdsTemp(code)) {
          continue;
        }
        const into = destinations(segment.trim()).filter(holdsUploaded);
        if (into.length > 0) {
          problems.push(`${at}: writes into an uploaded folder (${into.join(" ")}) while naming RUNNER_TEMP: ${code.trim()}`);
        }
      }
      // An uploaded folder's name set through GITHUB_ENV could point it at RUNNER_TEMP.
      if (/GITHUB_ENV/.test(code) && [...job.names].some((name) => new RegExp(`\\b${name}=`).test(code))) {
        problems.push(`${at}: sets an uploaded folder's name through GITHUB_ENV: ${code.trim()}`);
      }
    }
  }
  return problems;
}

test("no upload path, and no copy into an uploaded folder, reaches RUNNER_TEMP", () => {
  const text = workflow();
  assert.deepEqual(uploadProblems(text), []);
  const breaks = [
    // The review's.
    text.replace(COLLECT, `${COLLECT}          cp -r "$RUNNER_TEMP" "$OUT/t"\n`),
    text.replace(COLLECT, `${COLLECT}          cp -r "\${RUNNER_TEMP}/apple" "$OUT/"\n`),
    text.replace(COLLECT, `${COLLECT}          keys=$RUNNER_TEMP/apple\n          bundle=$RELEASE_DIR/bundle\n          tar -czf "$bundle/x.tgz" "$keys"\n`),
    text.replace(COLLECT, `${COLLECT}          ln -s "$RUNNER_TEMP" "$OUT/t"\n`),
    text.replace(COLLECT, `${COLLECT}          cat "$RUNNER_TEMP/dry-updater.key" > "$OUT/key"\n`),
    text.replace(COLLECT, `${COLLECT}          echo "OUT=$RUNNER_TEMP" >> "$GITHUB_ENV"\n`),
    text.replace("          path: ${{ env.OUT }}\n", "          path: ${{ runner.temp }}\n"),
    text.replace("          path: ${{ env.OUT }}\n", "          path: |\n            ${{ env.OUT }}\n            ${{ runner.temp }}/apple\n"),
    text.replace("          path: ${{ runner.temp }}/dry-updater.key.pub\n", "          path: ${{ runner.temp }}/dry-updater.key*\n"),
    text.replace("          path: ${{ env.OUT }}\n", "          path: ${{ env.OUT }}/..\n"),
    text.replace("      OUT: ${{ github.workspace }}/release-out\n", "      OUT: ${{ runner.temp }}/release-out\n"),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(uploadProblems(broken), [], `break ${i} is caught`);
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
    /\bxcrun notarytool history\b/,
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
    "grep -q '^Timestamp=' <<< \"$info\" && ! grep -q '^Signed Time=' <<< \"$info\" ||",
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
    // An untimestamped signature (Signed Time=) let through.
    text.replace("grep -q '^Timestamp=' <<< \"$info\" && ! grep -q '^Signed Time=' <<< \"$info\" ||", "grep -q '^Timestamp=' <<< \"$info\" ||"),
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

/**
 * Runs custody's Apple step's script with <env> (names to values), in a scratch checkout whose
 * crates/desktop/release/apple-team-id pins <pin> (under a comment, as the committed file is);
 * node's notary call faked.
 */
function runCustodyCheck(script, env, { pin = "ABCDE12345" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "custody-apple-"));
  try {
    const checkout = join(dir, "checkout");
    mkdirSync(join(checkout, "crates", "desktop", "release"), { recursive: true });
    writeFileSync(join(checkout, TEAM_ID_FILE), `# The Team ID (a comment, as committed).\n${pin}\n`);
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
      ...Object.fromEntries([...APPLE_SIX, ...APPLE_ID_NAMES].map((name) => [name, ""])),
      ...env,
    };
    const result = spawnSync("bash", ["-c", script], { cwd: checkout, env: full, encoding: "utf8" });
    return { status: result.status, out: `${result.stdout}${result.stderr}`, summary: readFileSync(summary, "utf8") };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * A Developer ID-like .p12 (base64) from a throwaway CA, and its password. An expired one is
 * issued with `openssl ca -startdate/-enddate` (2020), which OpenSSL 3.0 (boxbox, the runners)
 * and 4.x both take: 3.0's `x509` has no -not_before/-not_after, and 4.x refuses -days 0.
 */
function throwawayIdentity(dir, { team = "ABCDE12345", name = "Test Person", issuer = "Developer ID Certification Authority", expired = false } = {}) {
  const run = (...args) => {
    const result = spawnSync("openssl", args, { cwd: dir, encoding: "utf8" });
    assert.equal(result.status, 0, `openssl ${args.join(" ")}: ${result.stderr}`);
  };
  const ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes"];
  run("req", "-x509", ...ec, "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", `/CN=${issuer}`);
  const cn = `Developer ID Application: ${name} (${team})`;
  run("req", "-new", ...ec, "-keyout", "leaf.key", "-out", "leaf.csr", "-subj", `/UID=${team}/CN=${cn}/OU=${team}/O=${name}/C=IN`);
  if (expired) {
    writeFileSync(join(dir, "index.txt"), "");
    writeFileSync(join(dir, "serial"), "01\n");
    writeFileSync(
      join(dir, "ca.cnf"),
      "[ca]\ndefault_ca = throwaway\n[throwaway]\ndatabase = index.txt\nnew_certs_dir = .\nserial = serial\n" +
        "default_md = sha256\npolicy = anything\nunique_subject = no\n[anything]\ncommonName = supplied\n",
    );
    run("ca", "-batch", "-config", "ca.cnf", "-preserveDN", "-notext", "-cert", "ca.pem", "-keyfile", "ca.key", "-in", "leaf.csr",
      "-out", "leaf.pem", "-startdate", "20200101000000Z", "-enddate", "20200102000000Z");
  } else {
    run("x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-days", "30", "-out", "leaf.pem");
  }
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
  if (partial.status === 0 || !partial.out.includes("the certificate set lacks APPLE_SIGNING_IDENTITY")) {
    problems.push(`a partial set is not refused with the missing names (${partial.status}: ${partial.out.trim()})`);
  }
  if (/secret-password-value|c2VjcmV0LWNlcnQ=/.test(partial.out)) {
    problems.push("the refusal prints a secret's value");
  }
  const certificate = { APPLE_CERTIFICATE: "Y2VydA==", APPLE_CERTIFICATE_PASSWORD: "pw", APPLE_SIGNING_IDENTITY: "x" };
  const apiKey = { APPLE_API_KEY: "ABCDEFGHIJ", APPLE_API_ISSUER: "i", APPLE_API_KEY_P8: "k" };
  const appleId = { APPLE_ID: "rohan@example.com", APPLE_PASSWORD: "abcd-efgh-ijkl-mnop", APPLE_TEAM_ID: "ABCDE12345" };
  const refusals = [
    ["both notary sets complete", { ...certificate, ...apiKey, ...appleId }, "both notary sets"],
    ["a mixture of the two notary sets", { ...certificate, APPLE_API_KEY: "ABCDEFGHIJ", APPLE_ID: "rohan@example.com" }, "both notary sets"],
    ["a partial API key set", { ...certificate, APPLE_API_KEY: "ABCDEFGHIJ" }, "the API key set lacks APPLE_API_ISSUER APPLE_API_KEY_P8"],
    ["a partial Apple ID set", { ...certificate, APPLE_ID: "rohan@example.com", APPLE_TEAM_ID: "ABCDE12345" }, "the Apple ID set lacks APPLE_PASSWORD"],
    ["a notary set without the certificate", { ...appleId }, "the certificate set lacks APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY"],
    ["the certificate without a notary set", { ...certificate }, "no notary set"],
  ];
  for (const [what, env, words] of refusals) {
    const result = runCustodyCheck(script, env);
    if (result.status === 0 || !result.out.includes(words)) {
      problems.push(`${what} is not refused with "${words}" (${result.status}: ${result.out.trim()})`);
    }
    if (result.out.includes("abcd-efgh-ijkl-mnop") || result.out.includes("Y2VydA==")) {
      problems.push(`refusing ${what} prints a secret`);
    }
  }
  return problems;
}

test("custody accepts exactly one complete set, refusing a mixture or a partial set by name", { skip: !haveBash || process.platform === "win32" }, () => {
  const step = stepNamed(workflow(), "custody", CUSTODY_STEP);
  assert.ok(step, "custody's Apple step");
  assert.equal(step.if, "needs.check.outputs.mode == 'release'");
  const custody = stepsOf(workflow(), "custody").map((one) => one.name);
  assert.ok(custody.indexOf(CUSTODY_STEP) < custody.indexOf("Install the ui workspace"), "before the ui install");
  assert.deepEqual(custodyProblems(step.run), []);
  // Broken on purpose, each refusal once.
  const breaks = [
    ['if [ -n "$(names set "$api_key")" ] && [ -n "$(names set "$apple_id")" ]; then', "if false; then"],
    ['[ -z "$(names missing "$api_key")" ] ||', "true ||"],
    ['[ -z "$(names missing "$apple_id")" ] ||', "true ||"],
    ['[ -z "$(names missing "$certificate")" ] ||', "true ||"],
    ['fail "no notary set: set', 'true "no notary set: set'],
  ];
  for (const [from, to] of breaks) {
    const broken = step.run.replace(from, to);
    assert.notEqual(broken, step.run, from);
    assert.notDeepEqual(custodyProblems(broken), [], from);
  }
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
    // The committed Team ID: empty, another team's, or not a Team ID is refused, naming both.
    const pinRefusals = (body) => {
      const said = [];
      const empty = runCustodyCheck(body, secrets, { pin: "" });
      if (empty.status === 0 || !empty.out.includes("apple-team-id is empty, and the certificate's Team ID is ABCDE12345")) {
        said.push(`an empty pin: ${empty.out}`);
      }
      const other = runCustodyCheck(body, secrets, { pin: "ZZZZZ99999" });
      if (other.status === 0 || !other.out.includes("the certificate's Team ID is ABCDE12345, but crates/desktop/release/apple-team-id pins ZZZZZ99999")) {
        said.push(`another team's pin: ${other.out}`);
      }
      const junk = runCustodyCheck(body, secrets, { pin: "abc" });
      if (junk.status === 0 || !junk.out.includes("not a Team ID")) {
        said.push(`a pin that is no Team ID: ${junk.out}`);
      }
      return said;
    };
    assert.deepEqual(pinRefusals(script), []);
    // Broken on purpose: each pin check skipped once.
    for (const from of ['[ "$team" = "$pinned" ] ||', '[ -n "$pinned" ] ||', '[[ "$pinned" =~ ^([A-Z0-9]{10})?$ ]] ||']) {
      const lax = script.replace(from, "true ||");
      assert.notEqual(lax, script, from);
      assert.notDeepEqual(pinRefusals(lax), [], from);
    }
    // The Apple ID set instead of the API key's: its Team ID must be the certificate's.
    const appleIdSet = {
      ...secrets,
      APPLE_API_KEY: "",
      APPLE_API_ISSUER: "",
      APPLE_API_KEY_P8: "",
      APPLE_ID: "rohan@example.com",
      APPLE_PASSWORD: "abcd-efgh-ijkl-mnop",
      APPLE_TEAM_ID: "ABCDE12345",
    };
    const viaAppleId = runCustodyCheck(script, appleIdSet);
    assert.equal(viaAppleId.status, 0, viaAppleId.out);
    assert.ok(!viaAppleId.out.includes("abcd-efgh-ijkl-mnop"), "no secret printed");
    const mismatch = runCustodyCheck(script, { ...appleIdSet, APPLE_TEAM_ID: "ZZZZZ99999" });
    assert.notEqual(mismatch.status, 0);
    assert.ok(mismatch.out.includes("APPLE_TEAM_ID is not the Team ID"), mismatch.out);
    const notAnAsp = runCustodyCheck(script, { ...appleIdSet, APPLE_PASSWORD: "my-real-password" });
    assert.ok(notAnAsp.status !== 0 && notAnAsp.out.includes("not an app-specific password"), notAnAsp.out);
    // Broken on purpose: the Team ID not compared, so the mismatch passes.
    const lax = script.replace('[ "$APPLE_TEAM_ID" = "$pinned" ] ||', "true ||");
    assert.notEqual(lax, script);
    assert.equal(runCustodyCheck(lax, { ...appleIdSet, APPLE_TEAM_ID: "ZZZZZ99999" }).status, 0);
    refused({ APPLE_SIGNING_IDENTITY: "Developer ID Application: Someone Else (ABCDE12345)" }, "is not APPLE_SIGNING_IDENTITY's");
    refused({ APPLE_SIGNING_IDENTITY: "Apple Development: Test Person (ABCDE12345)" }, "is not \"Developer ID Application");
    refused({ APPLE_CERTIFICATE: "%%%" }, "is not base64");
    const other = mkdtempSync(join(tmpdir(), "custody-cert-"));
    try {
      const selfIssued = throwawayIdentity(other, { issuer: "Clauseo Code Signing" });
      refused({ APPLE_CERTIFICATE: selfIssued.p12 }, "not issued by Developer ID Certification Authority");
      const expired = throwawayIdentity(other, { expired: true });
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

// ---- The codesign wrapper: the bundler's signatures are timestamped, or the step fails first ----

/** The wrapper's script, from the bundle step's here-document, or null. */
function codesignWrapper(text) {
  const run = stepNamed(text, "build", BUNDLE_STEP)?.run ?? "";
  const match = run.match(/<< 'CODESIGN'\n([\s\S]*?)\nCODESIGN\n/);
  return match ? match[1] : null;
}

/** Where the wrapper is placed wrong: outside a Developer ID leg, or not on PATH before the bundle. */
function wrapperPlacementProblems(text) {
  const run = stepNamed(text, "build", BUNDLE_STEP)?.run ?? "";
  const lines = run.split("\n").map((line) => line.trim());
  const under = conditions(run);
  const written = lines.indexOf(`cat > "$apple/bin/codesign" << 'CODESIGN'`);
  const onPath = lines.indexOf("export PATH=$apple/bin:$PATH");
  const bundled = lines.findIndex((line) => /\btauri bundle\b/.test(line));
  if (written === -1 || onPath === -1) {
    return ["the bundle step writes no codesign wrapper, or does not put it first in PATH"];
  }
  const problems = [];
  if (!under[written].includes(ON_DEVELOPER_ID) || !under[onPath].includes(ON_DEVELOPER_ID)) {
    problems.push("the codesign wrapper is outside the Developer ID branch");
  }
  if (!(written < onPath && onPath < bundled)) {
    problems.push("the codesign wrapper is not first in PATH before tauri bundle");
  }
  return problems;
}

/** What the wrapper gets wrong, run with a fake /usr/bin/codesign. */
function wrapperProblems(wrapper) {
  if (wrapper === null) {
    return ["no codesign wrapper"];
  }
  const dir = mkdtempSync(join(tmpdir(), "codesign-wrapper-"));
  try {
    const log = join(dir, "calls.log");
    // The fake logs each call's arguments; -dvv shows the line $STAMP names.
    writeFileSync(
      join(dir, "fake-codesign"),
      `#!/bin/bash\necho "$*" >> "${log}"\nif [ "$1" = -dvv ]; then echo "$STAMP=Nov 16, 2026 at 10:00:00" >&2; fi\nexit 0\n`,
    );
    chmodSync(join(dir, "fake-codesign"), 0o755);
    writeFileSync(join(dir, "codesign"), wrapper.replaceAll("/usr/bin/codesign", join(dir, "fake-codesign")));
    chmodSync(join(dir, "codesign"), 0o755);
    const call = (args, stamp = "Timestamp") => {
      writeFileSync(log, "");
      const result = spawnSync(join(dir, "codesign"), args, { env: { ...process.env, STAMP: stamp }, encoding: "utf8" });
      const calls = readFileSync(log, "utf8").trim().split("\n");
      return { status: result.status, out: `${result.stdout}${result.stderr}`, calls };
    };
    const problems = [];
    const sign = ["--force", "-s", "Developer ID Application: T (ABCDE12345)", "--options", "runtime", "Clauseo.app"];
    const signed = call(sign);
    if (signed.status !== 0 || signed.calls[0] !== `--timestamp ${sign.join(" ")}` || signed.calls[1] !== "-dvv Clauseo.app") {
      problems.push(`a signing is not given --timestamp and checked: ${JSON.stringify(signed)}`);
    }
    const already = call(["--force", "-s", "X", "--timestamp", "helper"]);
    if (already.calls[0] !== "--force -s X --timestamp helper") {
      problems.push(`a signing with --timestamp gets a second one: ${already.calls[0]}`);
    }
    const untimestamped = call(sign, "Signed Time");
    if (untimestamped.status === 0 || !untimestamped.out.includes("without a secure timestamp")) {
      problems.push("a signature showing Signed Time= (no secure timestamp) is not refused");
    }
    const looked = call(["-dvv", "Clauseo.app"]);
    if (looked.status !== 0 || looked.calls.join("|") !== "-dvv Clauseo.app") {
      problems.push(`a call that signs nothing is changed: ${looked.calls.join("|")}`);
    }
    return problems;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("the bundler's codesign gets --timestamp, and a signature without one fails before notarisation", { skip: !haveBash || process.platform === "win32" }, () => {
  const text = workflow();
  assert.deepEqual(wrapperPlacementProblems(text), []);
  const wrapper = codesignWrapper(text);
  assert.deepEqual(wrapperProblems(wrapper), []);
  // Broken on purpose: no --timestamp added; no check after signing; on PATH only after the bundle.
  const noStamp = wrapper.replace("stamp=(--timestamp)", "stamp=()");
  assert.notEqual(noStamp, wrapper);
  assert.notDeepEqual(wrapperProblems(noStamp), []);
  const lax = wrapper.replace(
    "if ! grep -q '^Timestamp=' <<< \"$info\" || grep -q '^Signed Time=' <<< \"$info\"; then",
    "if false; then",
  );
  assert.notEqual(lax, wrapper);
  assert.notDeepEqual(wrapperProblems(lax), []);
  const late = text
    .replace("            export PATH=$apple/bin:$PATH\n", "")
    .replace(
      '          if [ "$DEVELOPER_ID" = true ]; then\n            dmg=',
      '          if [ "$DEVELOPER_ID" = true ]; then\n            export PATH=$apple/bin:$PATH\n            dmg=',
    );
  assert.notEqual(late, text);
  assert.notDeepEqual(wrapperPlacementProblems(late), []);
});

// ---- Every mode: the hardened runtime and per-binary entitlements on every Mach-O ----

const RUNTIME_STEP = "Every Mach-O has the hardened runtime and its own entitlements";

/** What the every-mode runtime check lacks, or where it is skipped. */
function runtimeStepProblems(text) {
  const steps = stepsOf(text, "build");
  const at = (name) => steps.findIndex((step) => step.name === name);
  const step = steps[at(RUNTIME_STEP)];
  if (step === undefined) {
    return ["build has no every-mode hardened runtime check"];
  }
  const problems = [];
  if (step.if !== "runner.os == 'macOS'") {
    problems.push(`the hardened runtime check runs under ${step.if}, not on every macOS leg`);
  }
  if (!(at(BUNDLE_STEP) < at(RUNTIME_STEP) && at(RUNTIME_STEP) < at("Collect this leg's assets"))) {
    problems.push("the hardened runtime check is not after the bundle and before the assets are collected");
  }
  const wanted = [
    "grep -Eq ' flags=0x[0-9a-f]+\\(([a-z-]+,)*runtime(,[a-z-]+)*\\) ' <<< \"$info\" ||",
    '[ "$have" = "$want" ] ||',
    "Contents/MacOS/*) expected=$app_keys ;;",
    "Contents/Helpers/*) expected='[]' ;;",
    'held=$(codesign -d --entitlements - --xml "$file" 2> /dev/null | keys) ||',
    '[ "$held" = "$expected" ] ||',
    "printf '.\\t%s\\n' \"$app_keys\" >> \"$RUNNER_TEMP/entitlements.tsv\"",
  ];
  for (const words of wanted) {
    if (!step.run.includes(words)) {
      problems.push(`the hardened runtime check has no ${words}`);
    }
  }
  // The ad hoc helpers get the runtime too; the bundler's own signatures get it from hardenedRuntime.
  const bundle = stepNamed(text, "build", BUNDLE_STEP)?.run ?? "";
  if (!bundle.includes('codesign --force -s "$APPLE_SIGNING_IDENTITY" --options runtime "$helper"')) {
    problems.push("the ad hoc helpers are not signed with --options runtime");
  }
  return problems;
}

test("every mode's .app has the hardened runtime and per-binary entitlements on every Mach-O", () => {
  const text = workflow();
  assert.deepEqual(runtimeStepProblems(text), []);
  const head = `      - name: ${RUNTIME_STEP}\n        if: runner.os == 'macOS'\n`;
  const breaks = [
    // Developer ID only: the ad hoc build the VM tests would go unchecked.
    text.replace(head, `      - name: ${RUNTIME_STEP}\n        if: env.DEVELOPER_ID == 'true'\n`),
    // A Mach-O without the runtime let through.
    text.replace("grep -Eq ' flags=0x[0-9a-f]+\\(([a-z-]+,)*runtime(,[a-z-]+)*\\) ' <<< \"$info\" ||", "true ||"),
    // One blanket set: helpers given the app's entitlements.
    text.replace("Contents/Helpers/*) expected='[]' ;;", "Contents/Helpers/*) expected=$app_keys ;;"),
    // The .app itself skipped.
    text.replace("          printf '.\\t%s\\n' \"$app_keys\" >> \"$RUNNER_TEMP/entitlements.tsv\"\n", ""),
    // Ad hoc helpers without the runtime.
    text.replace(
      'codesign --force -s "$APPLE_SIGNING_IDENTITY" --options runtime "$helper"',
      'codesign --force -s "$APPLE_SIGNING_IDENTITY" "$helper"',
    ),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(runtimeStepProblems(broken), [], `break ${i} is caught`);
  }
});

// ---- The DMG's notary verdict: only the notary service's "Accepted" staples ----

/**
 * What the bundle step's DMG notarisation gets wrong, run from `submitted=` to the stapler's
 * validate with a fake xcrun that answers <status>: a refusal, an empty answer or a failed call
 * must fail before stapling; only Accepted staples.
 */
function verdictProblems(text) {
  const run = stepNamed(text, "build", BUNDLE_STEP)?.run ?? "";
  const lines = run.split("\n");
  const from = lines.findIndex((line) => line.trim().startsWith("submitted=$(xcrun notarytool submit"));
  const to = lines.findIndex((line) => line.trim().startsWith('xcrun stapler validate "$dmg"'));
  if (from === -1 || to < from) {
    return ["the bundle step has no notarytool submit followed by the DMG's stapler validate"];
  }
  const dir = mkdtempSync(join(tmpdir(), "notary-verdict-"));
  try {
    const log = join(dir, "calls.log");
    writeFileSync(
      join(dir, "xcrun"),
      `#!/bin/bash\necho "$*" >> "${log}"\ncase "$1 $2" in\n  "notarytool submit") [ "$ANSWER" = fail ] && exit 1; printf '%s' "$ANSWER" ;;\nesac\nexit 0\n`,
    );
    chmodSync(join(dir, "xcrun"), 0o755);
    const script = ['set -euo pipefail', 'fail() { echo "::error::$1"; exit 1; }', 'dmg=Clauseo_0.0.0_aarch64.dmg', "notary=(--key k)", ...lines.slice(from, to + 1)].join("\n");
    const problems = [];
    const answers = [
      ['{"id":"x","status":"Invalid"}', false],
      ['{"id":"x","status":"In Progress"}', false],
      ["", false],
      ["fail", false],
      ["not json", false],
      ['{"id":"x","status":"Accepted"}', true],
    ];
    for (const [answer, staples] of answers) {
      writeFileSync(log, "");
      const result = spawnSync("bash", ["-c", script], { cwd: dir, env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, ANSWER: answer }, encoding: "utf8" });
      const stapled = readFileSync(log, "utf8").includes("stapler staple");
      if (staples && (result.status !== 0 || !stapled)) {
        problems.push(`an Accepted DMG is not stapled (${result.status}: ${result.stdout}${result.stderr})`);
      }
      if (!staples && (result.status === 0 || stapled)) {
        problems.push(`the answer ${JSON.stringify(answer)} staples or passes`);
      }
    }
    return problems;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("only the notary service's Accepted lets the DMG be stapled; anything else fails the step", { skip: !haveBash || process.platform === "win32" || spawnSync("jq", ["--version"]).status !== 0 }, () => {
  const text = workflow();
  assert.deepEqual(verdictProblems(text), []);
  const verdictLine = "            verdict=$(jq -r '.status // \"\"' <<< \"$submitted\" 2> /dev/null) || verdict=\"\"\n";
  assert.ok(text.includes(verdictLine));
  const breaks = [
    // The verdict forced to Accepted, three ways.
    text.replace(verdictLine, `${verdictLine}            verdict=Accepted\n`),
    text.replace(verdictLine, verdictLine.replace('|| verdict=""', "|| verdict=Accepted").replace("'.status // \"\"'", "'.status // \"Accepted\"'")),
    text.replace('            if [ "$verdict" != Accepted ]; then\n', '            if false; then\n'),
    // A refusal that goes on to staple.
    text.replace("              [ -z \"$id\" ] || xcrun notarytool log \"$id\" \"${notary[@]}\" || true\n              exit 1\n", "              [ -z \"$id\" ] || xcrun notarytool log \"$id\" \"${notary[@]}\" || true\n"),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, text, `break ${i} applies`);
    assert.notDeepEqual(verdictProblems(broken), [], `break ${i} is caught`);
  }
});

// ---- The committed Team ID: one file, read the same way by custody, the check and publish ----

/** Where the committed Team ID is not the one every signature is compared with. */
function teamPinProblems(text, committed) {
  const problems = [];
  const value = committed.replace(/#.*/g, "").replace(/\s/g, "");
  if (!/^([A-Z0-9]{10})?$/.test(value)) {
    problems.push(`${TEAM_ID_FILE} holds "${value}", not a Team ID or nothing`);
  }
  const custody = stepNamed(text, "custody", CUSTODY_STEP)?.run ?? "";
  const check = stepNamed(text, "build", CHECK_STEP)?.run ?? "";
  const publish = stepNamed(text, "publish", "Every DMG is notarised and stapled, or this is an unsigned beta on purpose")?.run ?? "";
  for (const [step, script, read] of [
    ["custody", custody, `pinned=$(${TEAM_ID_READ}) ||`],
    ["the Developer ID check", check, `team=$(${TEAM_ID_READ})`],
    ["publish", publish, `team=$(${TEAM_ID_READ})`],
  ]) {
    if (!script.includes(read)) {
      problems.push(`${step} does not read the Team ID from ${TEAM_ID_FILE} (${read})`);
    }
  }
  // The check: the identity's team must be the pinned one, and every TeamIdentifier is compared
  // with the pinned one, never with the secret's own.
  if (/^\s*team=\$\{BASH_REMATCH/m.test(check) || !check.includes('[ "$identity_team" = "$team" ] ||')) {
    problems.push("the Developer ID check takes its team from APPLE_SIGNING_IDENTITY, not from the committed file");
  }
  if (!check.includes('grep -qx "TeamIdentifier=$team" <<< "$info"')) {
    problems.push("the Developer ID check does not compare each TeamIdentifier with the committed team");
  }
  // publish: each notarised DMG and its .app signed by the committed team.
  const signed = publish.indexOf('if [ "$UNSIGNED_BETA" != true ] || [ "$HAS_APPLE" = true ]; then');
  const team = publish.indexOf('grep -c "^TeamIdentifier=$team\\$"');
  const both = publish.indexOf('[ "$signers" = 2 ] ||');
  if (signed === -1 || !(signed < team && team < both) || !publish.includes('codesign -dv "$mnt/Clauseo.app"')) {
    problems.push("publish does not check that each notarised DMG and its .app are signed by the committed team");
  }
  return problems;
}

test("the Team ID is committed in one file, and custody, the check and publish compare with it", () => {
  const text = workflow();
  const committed = readFileSync(join(ROOT, TEAM_ID_FILE), "utf8");
  assert.deepEqual(teamPinProblems(text, committed), []);
  const breaks = [
    [text, `${committed}ABCDE1234\n`],
    [text.replace(`          team=$(${TEAM_ID_READ})\n          [[ "$team" =~`, `          team=$identity_team\n          [[ "$team" =~`), committed],
    [text.replace('          [ "$identity_team" = "$team" ] ||\n', "          true ||\n"), committed],
    [text.replace('[ "$signers" = 2 ] ||', "true ||"), committed],
    [text.replace("pinned=$(sed 's/#.*//' crates/desktop/release/apple-team-id", "pinned=$(sed 's/#.*//' crates/desktop/release/team"), committed],
  ];
  for (const [i, [broken, file]] of breaks.entries()) {
    assert.ok(broken !== text || file !== committed, `break ${i} applies`);
    assert.notDeepEqual(teamPinProblems(broken, file), [], `break ${i} is caught`);
  }
});

// ---- apple-setup.sh runs by hand only, and leaves no password on the USB ----

const SETUP = join(here, "apple-setup.sh");

/** What apple-setup.sh gets wrong: run under CI and without a terminal; read for the rest. */
function setupProblems(script) {
  const problems = [];
  const dir = mkdtempSync(join(tmpdir(), "apple-setup-"));
  try {
    const file = join(dir, "apple-setup.sh");
    writeFileSync(file, script);
    const run = (env) => spawnSync("bash", [file, "--check"], { env: { PATH: process.env.PATH, HOME: dir, ...env }, encoding: "utf8", stdio: ["pipe", "pipe", "pipe"] });
    for (const env of [{ CI: "true" }, { GITHUB_ACTIONS: "true" }]) {
      const result = run(env);
      if (result.status === 0 || !result.stderr.includes("never under CI")) {
        problems.push(`under ${Object.keys(env)[0]} it does not stop (${result.status}: ${result.stdout}${result.stderr})`);
      }
    }
    const piped = run({});
    if (piped.status === 0 || !piped.stderr.includes("needs a terminal")) {
      problems.push(`without a terminal it does not stop (${piped.status}: ${piped.stdout}${piped.stderr})`);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  const code = script.split("\n").filter((line) => !/^\s*#/.test(line)).join("\n");
  if (/p12_password[^\n]*[^0-9&]>{1,2}\s*(?!\/dev\/null|&)\S|password\.txt/.test(code)) {
    problems.push("the .p12's password is written to a file");
  }
  if (/Desktop\/DeveloperID\.p12/.test(code) || !code.includes('work=$(mktemp -d "$HOME/clauseo-apple-setup.XXXXXX")')) {
    problems.push("the .p12 is not exported into the script's own folder outside Desktop and Documents");
  }
  if (!code.includes("pbcopy < /dev/null")) {
    problems.push("the clipboard is not cleared");
  }
  // One paste of the app-specific password: notarytool checks the value GitHub gets.
  if ((code.match(/read -r -s -p "  App-specific password: " asp/g) ?? []).length !== 1 || !code.includes('--password "$asp"') || !code.includes("printf '%s' \"$asp\" | set_secret APPLE_PASSWORD")) {
    problems.push("the app-specific password notarytool checks is not the one set on GitHub");
  }
  if (!code.includes('[[ "$pick" =~ ^[0-9]{1,3}$ ]] && [ "$pick" -ge 1 ] && [ "$pick" -le "${#identities[@]}" ] ||')) {
    problems.push("the identity's number is not checked before arithmetic");
  }
  return problems;
}

test("apple-setup.sh stops under CI or without a terminal, and keeps the .p12's password off disk", { skip: !haveBash || process.platform === "win32" }, () => {
  const script = readFileSync(SETUP, "utf8");
  assert.deepEqual(setupProblems(script), []);
  const breaks = [
    script.replace('if [ -n "${CI:-}" ] || [ -n "${GITHUB_ACTIONS:-}" ]; then', "if false; then"),
    script.replace("if ! [ -t 0 ] || ! [ -t 1 ]; then", "if false; then"),
    script.replace("cp \"$p12\" \"$dest/DeveloperID.p12\"\n", "cp \"$p12\" \"$dest/DeveloperID.p12\"\n(umask 077 && printf '%s\\n' \"$p12_password\" > \"$dest/DeveloperID.p12.password.txt\")\n"),
    script.replace('"$work/DeveloperID.p12")', '"$HOME/Desktop/DeveloperID.p12")'),
    script.replace("  pbcopy < /dev/null", "  true"),
    script.replace(' --password "$asp"', ""),
    script.replace('[[ "$pick" =~ ^[0-9]{1,3}$ ]] && ', ""),
  ];
  for (const [i, broken] of breaks.entries()) {
    assert.notEqual(broken, script, `break ${i} applies`);
    assert.notDeepEqual(setupProblems(broken), [], `break ${i} is caught`);
  }
});
