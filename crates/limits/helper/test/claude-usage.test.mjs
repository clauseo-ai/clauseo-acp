// Tests for claude-usage.mjs, run against the stub SDK in ./stub-sdk (never the real SDK or a real claude).
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const helperSource = join(here, "..", "claude-usage.mjs");
const stubDir = join(here, "stub-sdk");
const USAGE_METHOD = "usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET";

const roots = [];
after(() => {
  for (const root of roots) rmSync(root, { recursive: true, force: true });
});

// One install folder (helper + stub SDK as the adapter's own SDK) and one empty working folder.
function setup() {
  const root = mkdtempSync(join(tmpdir(), "s13-claude-usage-"));
  roots.push(root);
  const install = join(root, "install");
  const sdkDir = join(install, "node_modules", "@anthropic-ai", "claude-agent-sdk");
  const work = join(root, "work");
  mkdirSync(sdkDir, { recursive: true });
  mkdirSync(work);
  copyFileSync(helperSource, join(install, "clauseo-claude-usage.mjs"));
  for (const file of readdirSync(stubDir)) copyFileSync(join(stubDir, file), join(sdkDir, file));
  return {
    install,
    work,
    optionsOut: join(root, "options.json"),
    callsOut: join(root, "calls.json"),
  };
}

// Runs the helper with a scratch environment: PATH plus the given variables only. `before(fx)` runs
// after the folders are made and before the helper starts.
function run(env = {}, before = () => {}) {
  const fx = setup();
  before(fx);
  const result = spawnSync(process.execPath, [join(fx.install, "clauseo-claude-usage.mjs")], {
    cwd: fx.work,
    timeout: 10_000,
    encoding: "utf8",
    env: { PATH: process.env.PATH, STUB_OPTIONS_OUT: fx.optionsOut, STUB_CALLS_OUT: fx.callsOut, ...env },
  });
  return { ...result, fx };
}

// Asserts the contract (one stdout line, exit 0) and returns the parsed line.
function oneLine(result) {
  assert.equal(result.error, undefined, `helper did not run: ${result.error}`);
  assert.equal(result.status, 0, `exit code; stderr: ${result.stderr}`);
  assert.ok(result.stdout.endsWith("\n"), `stdout has no trailing newline: ${JSON.stringify(result.stdout)}`);
  const body = result.stdout.slice(0, -1);
  assert.ok(!body.includes("\n"), `more than one stdout line: ${JSON.stringify(result.stdout)}`);
  return JSON.parse(body);
}

function expectedOptions(cwd) {
  return {
    abortController: "<AbortController>",
    persistSession: false,
    settingSources: [],
    allowedTools: [],
    mcpServers: {},
    strictMcpConfig: true,
    hooks: {},
    settings: { disableAllHooks: true },
    cwd,
  };
}

test("query() gets exactly the locked-down options; the usage call skips behaviors", () => {
  const result = run();
  oneLine(result);
  const recorded = JSON.parse(readFileSync(result.fx.optionsOut, "utf8"));
  assert.equal(recorded.prompt, "<async-iterable>");
  assert.deepEqual(recorded.options, expectedOptions(realpathSync(result.fx.work)));
  const calls = JSON.parse(readFileSync(result.fx.callsOut, "utf8"));
  assert.deepEqual(
    calls.map((call) => call.method),
    ["initializationResult", USAGE_METHOD, "close"],
  );
  assert.deepEqual(calls[1].args, [{ skipBehaviors: true }]);
});

test("a .claude/settings.json in the run folder (Vertex env) is not read: query() gets the same options, cwd is the run folder", () => {
  const vertex = {
    env: {
      CLAUDE_CODE_USE_VERTEX: "1",
      ANTHROPIC_VERTEX_PROJECT_ID: "clauseo-g16-not-a-project",
      CLOUD_ML_REGION: "us-east5",
    },
  };
  const result = run({ STUB_MODE: "ok" }, (fx) => {
    mkdirSync(join(fx.work, ".claude"));
    writeFileSync(join(fx.work, ".claude", "settings.json"), JSON.stringify(vertex));
  });
  assert.equal(oneLine(result).ok, true);
  const recorded = JSON.parse(readFileSync(result.fx.optionsOut, "utf8"));
  assert.deepEqual(recorded.options, expectedOptions(realpathSync(result.fx.work)));
  assert.deepEqual(recorded.options.settingSources, []);
});

test("CLAUDE_CODE_EXECUTABLE set: pathToClaudeCodeExecutable is that path", () => {
  const result = run({ CLAUDE_CODE_EXECUTABLE: "/opt/claude/bin/claude" });
  oneLine(result);
  const recorded = JSON.parse(readFileSync(result.fx.optionsOut, "utf8"));
  assert.deepEqual(recorded.options, {
    ...expectedOptions(realpathSync(result.fx.work)),
    pathToClaudeCodeExecutable: "/opt/claude/bin/claude",
  });
});

test("CLAUDE_CODE_EXECUTABLE set but empty: the option is absent", () => {
  const result = run({ CLAUDE_CODE_EXECUTABLE: "" });
  oneLine(result);
  const recorded = JSON.parse(readFileSync(result.fx.optionsOut, "utf8"));
  assert.equal("pathToClaudeCodeExecutable" in recorded.options, false);
  assert.deepEqual(recorded.options, expectedOptions(realpathSync(result.fx.work)));
});

test("ok: one line with the account keys and the usage keys only", () => {
  const result = run({ STUB_MODE: "ok" });
  assert.deepEqual(oneLine(result), {
    ok: true,
    account: {
      email: "pat@example.com",
      subscriptionType: "max",
      apiProvider: "firstParty",
    },
    usage: {
      subscription_type: "max",
      rate_limits_available: true,
      rate_limits: {
        five_hour: { utilization: 42, resets_at: "2026-11-02T18:00:00Z" },
        seven_day: { utilization: 13, resets_at: "2026-11-07T00:00:00Z" },
      },
    },
  });
});

test("lone surrogates in the SDK's strings print as U+FFFD, so the line stays valid JSON", () => {
  const result = run({ STUB_MODE: "lone-surrogate" });
  const line = oneLine(result);
  assert.ok(!/\\ud[89a-f]/i.test(result.stdout), `escaped lone surrogate: ${result.stdout}`);
  assert.equal(line.account.email, "pat\uFFFD@example.com");
  assert.equal(line.usage.rate_limits.model_scoped[0].display_name, "Fa\uFFFDble");
});

const errorCases = [
  // [mode, stage, missing, exact message or null, stderr must be non-empty]
  ["import-fails", "init", false, "stub import failure", true],
  ["no-init", "init", true, "initializationResult method missing", false],
  ["init-throws", "init", false, "not logged in line two", true],
  ["no-usage", "usage", true, "usage_EXPERIMENTAL method missing", false],
  ["usage-unknown", "usage", true, "Unsupported control request subtype: get_usage", true],
];

for (const [mode, stage, missing, message, stderrFilled] of errorCases) {
  test(`${mode}: one error line, stage ${stage}, missing ${missing}`, () => {
    const result = run({ STUB_MODE: mode });
    assert.deepEqual(oneLine(result), { ok: false, stage, missing, message });
    if (stderrFilled) assert.notEqual(result.stderr, "");
  });
}

test("usage-throws: message is cut to 200 characters, missing false", () => {
  const result = run({ STUB_MODE: "usage-throws" });
  const line = oneLine(result);
  assert.deepEqual(line, { ok: false, stage: "usage", missing: false, message: "x".repeat(200) });
  assert.notEqual(result.stderr, "");
});
