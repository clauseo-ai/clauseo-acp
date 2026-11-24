#!/usr/bin/env node
// Starts the pinned claude-agent-acp and codex-acp with the machine's own sign-in, lists each
// one's models and sends each exactly one prompt. See README.md beside this file. It is run by
// hand on machines with real accounts before a new pair of engine pins is merged; CI never runs it.
//
//   node scripts/engine-smoke/smoke.mjs <node_modules> [--agent claude|codex|both]
//     [--expect-model [claude:|codex:]<id>]... [--timeout 180] [--mock-codex]
//
// SMOKE_CLAUDE_CONFIG_DIR and SMOKE_CODEX_HOME set CLAUDE_CONFIG_DIR and CODEX_HOME for the
// adapters; without them the adapters use their own defaults. KEEP=1 keeps the scratch folders.

import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";

const PROMPT = "Reply with the single word OK.";
const PACKAGES = {
  claude: { name: "@agentclientprotocol/claude-agent-acp", configEnv: "CLAUDE_CONFIG_DIR", smokeEnv: "SMOKE_CLAUDE_CONFIG_DIR" },
  codex: { name: "@agentclientprotocol/codex-acp", configEnv: "CODEX_HOME", smokeEnv: "SMOKE_CODEX_HOME" },
};
// ACP's `authRequired` error (the SDK's RequestError.authRequired()).
const AUTH_REQUIRED = -32000;

function usage(message) {
  if (message) console.error(message);
  console.error(
    "usage: smoke.mjs <node_modules> [--agent claude|codex|both] [--expect-model [claude:|codex:]<id>]... [--timeout 180] [--mock-codex]",
  );
  process.exit(2);
}

function parseArgs(argv) {
  const opts = { modules: null, agents: ["claude", "codex"], expect: [], timeout: 180, mockCodex: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => (i + 1 < argv.length ? argv[++i] : usage(`${arg} needs a value`));
    if (arg === "--agent") {
      const a = value();
      if (a === "both") opts.agents = ["claude", "codex"];
      else if (a in PACKAGES) opts.agents = [a];
      else usage(`unknown agent: ${a}`);
    } else if (arg === "--expect-model") opts.expect.push(value());
    else if (arg === "--timeout") {
      opts.timeout = Number(value());
      if (!(opts.timeout > 0)) usage("--timeout takes a number of seconds");
    } else if (arg === "--mock-codex") opts.mockCodex = true;
    else if (arg === "-h" || arg === "--help") usage();
    else if (arg.startsWith("--")) usage(`unknown option: ${arg}`);
    else if (opts.modules === null) opts.modules = arg;
    else usage(`unexpected argument: ${arg}`);
  }
  if (!opts.modules) usage();
  if (opts.mockCodex) opts.agents = ["codex"];
  return opts;
}

// The adapter's entry from its package.json: `bin` (a string, or the first entry), else `main`.
function adapterEntry(modules, name) {
  const dir = resolve(modules, name);
  const pkgPath = join(dir, "package.json");
  if (!existsSync(pkgPath)) throw new Error(`${name} is not in ${modules}`);
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  const bin = typeof pkg.bin === "string" ? pkg.bin : pkg.bin && Object.values(pkg.bin)[0];
  const entry = bin || pkg.main;
  if (!entry) throw new Error(`${name} has no bin or main`);
  return { path: join(dir, entry), version: pkg.version };
}

// The model and effort the adapter reports in session/new (and later config_option_update).
// Both adapters send `configOptions`: an option with category "model" (claude-agent-acp's id
// "model", values such as "default", "opus"; codex-acp's id "model", values such as "gpt-5.5")
// and one with category "thought_level" (claude-agent-acp's id "effort", codex-acp's id
// "reasoning_effort"). codex-acp also sends `models` (availableModels with ids "gpt-5.5[high]",
// currentModelId in the same form), which is the fallback when configOptions are missing.
function optionValues(option) {
  return (option.options || []).flatMap((o) => (Array.isArray(o.options) ? o.options : [o])).map((o) => o.value);
}
function readModels(result, configOptions) {
  const byCategory = (cat) => (configOptions || []).find((o) => o.category === cat);
  const model = byCategory("model");
  const effort = byCategory("thought_level");
  const split = (id) => /^(?<model>[^[]+?)(?:\[(?<effort>[^\]]+)\])?$/.exec(id || "")?.groups || {};
  const state = result?.models;
  const stateIds = (state?.availableModels || []).map((m) => m.modelId);
  const models = model ? optionValues(model) : [...new Set(stateIds.map((id) => split(id).model))];
  return {
    models,
    known: new Set([...models, ...stateIds, ...stateIds.map((id) => split(id).model)]),
    current_model: model ? model.currentValue : (split(state?.currentModelId).model ?? null),
    current_effort: effort ? effort.currentValue : (split(state?.currentModelId).effort ?? null),
  };
}

// A stand-in for the Responses API that answers every request with the message "OK", for
// --mock-codex. The Codex home it writes points Codex at it, as scripts/codex-launch-probe does.
async function startMockResponses(home) {
  let requests = 0;
  const server = createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      if (!req.url.endsWith("/responses")) {
        const models = req.url.includes("/models");
        res.writeHead(models ? 200 : 404, { "content-type": "application/json" });
        res.end(models ? '{"models":[]}' : "{}");
        return;
      }
      const id = `resp-${++requests}`;
      if (process.env.SMOKE_TRACE === "1") console.error(`mock request ${requests} ${req.url}`);
      const events = [
        { type: "response.created", response: { id } },
        { type: "response.output_item.added", item: { type: "message", role: "assistant", id: "m", content: [] } },
        { type: "response.output_text.delta", item_id: "m", output_index: 0, content_index: 0, delta: "OK" },
        { type: "response.output_item.done", item: { type: "message", role: "assistant", id: "m", content: [{ type: "output_text", text: "OK" }] } },
        { type: "response.completed", response: { id, usage: { input_tokens: 0, input_tokens_details: null, output_tokens: 0, output_tokens_details: null, total_tokens: 0 } } },
      ];
      res.writeHead(200, { "content-type": "text/event-stream" });
      for (const ev of events) res.write(`event: ${ev.type}\ndata: ${JSON.stringify(ev)}\n\n`);
      res.end();
    });
  });
  await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
  writeFileSync(
    join(home, "config.toml"),
    [
      'model = "gpt-5.5"',
      'model_provider = "mock"',
      "",
      "[model_providers.mock]",
      'name = "mock"',
      `base_url = "http://127.0.0.1:${server.address().port}/v1"`,
      'wire_api = "responses"',
      "requires_openai_auth = false",
      "supports_websockets = false",
      "",
    ].join("\n"),
  );
  return server;
}

// The pids below `parent`, read before anything is signalled: `ps` on macOS and Linux, the
// process table through PowerShell on Windows.
function descendants(parent) {
  if (parent === undefined) return [];
  let pairs;
  try {
    if (process.platform === "win32") {
      const rows = execFileSync(
        "powershell.exe",
        ["-NoProfile", "-NonInteractive", "-Command", "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId) $($_.ParentProcessId)\" }"],
        { encoding: "utf8", windowsHide: true, timeout: 20000 },
      );
      pairs = rows.trim().split(/\r?\n/);
    } else {
      pairs = execFileSync("ps", ["-axo", "pid=,ppid="], { encoding: "utf8" }).trim().split("\n");
    }
  } catch {
    return [];
  }
  const kids = new Map();
  for (const [pid, ppid] of pairs.map((row) => row.trim().split(/\s+/).map(Number))) {
    kids.set(ppid, [...(kids.get(ppid) || []), pid]);
  }
  const found = [];
  for (let level = [parent]; level.length; ) {
    level = level.flatMap((pid) => kids.get(pid) || []);
    found.push(...level);
  }
  return found;
}
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
};
const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));
async function allGone(pids, ms) {
  const until = Date.now() + ms;
  while (pids.some(alive)) {
    if (Date.now() >= until) return false;
    await sleep(100);
  }
  return true;
}
const killQuietly = (pid, signal) => {
  try {
    process.kill(pid, signal);
  } catch {}
};

class StepError extends Error {
  constructor(reason, message) {
    super(message);
    this.reason = reason;
  }
}

async function smokeAgent(agent, opts) {
  const started = Date.now();
  const deadline = started + opts.timeout * 1000;
  const mock = agent === "codex" && opts.mockCodex;
  // `--expect-model claude:opus` is checked only for Claude; an id with no agent prefix, for both.
  const expect = opts.expect.flatMap((id) => {
    const scoped = /^(claude|codex):(.+)$/.exec(id);
    return !scoped ? [id] : scoped[1] === agent ? [scoped[2]] : [];
  });
  const keep = process.env.KEEP === "1";
  const row = {
    agent,
    adapter_version: null,
    models: [],
    current_model: null,
    current_effort: null,
    expected_missing: [...expect],
    prompt_ok: false,
    stop_reason: null,
    seconds: 0,
  };
  const { name, configEnv, smokeEnv } = PACKAGES[agent];
  let entry;
  try {
    entry = adapterEntry(opts.modules, name);
  } catch (error) {
    return { ...row, ok: false, reason: `adapter_missing: ${error.message}` };
  }

  // The scratch folder: a work folder with one small text file, and for --mock-codex a Codex
  // home and HOME of its own.
  const root = mkdtempSync(join(tmpdir(), `engine-smoke-${agent}-`));
  const work = join(root, "work");
  mkdirSync(work);
  writeFileSync(join(work, "note.txt"), "A short note for the engine smoke.\n");
  const env = { ...process.env };
  let server = null;
  if (mock) {
    const home = join(root, "codex-home");
    mkdirSync(home);
    server = await startMockResponses(home);
    Object.assign(env, { HOME: root, USERPROFILE: root, CODEX_HOME: home });
  } else if (process.env[smokeEnv]) {
    env[configEnv] = resolve(process.env[smokeEnv]);
  }

  const child = spawn(process.execPath, [entry.path], {
    cwd: work,
    env,
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  child.stderr.on("data", (d) => (stderr = (stderr + d).slice(-8000)));
  child.stdin.on("error", () => {});
  let spawnError = null;
  child.on("error", (error) => (spawnError = error));

  let nextId = 1;
  const pending = new Map();
  const updates = [];
  let authStatus = null;
  let permissionsRefused = 0;
  const trace = process.env.SMOKE_TRACE === "1" ? (dir, line) => console.error(`${agent} ${dir} ${line.slice(0, 2000)}`) : () => {};
  const write = (msg) => {
    const line = JSON.stringify({ jsonrpc: "2.0", ...msg });
    trace(">", line);
    if (child.stdin.writable) child.stdin.write(line + "\n");
  };
  const failAll = (error) => {
    for (const p of pending.values()) p.fail(error);
    pending.clear();
  };
  child.on("exit", (code, signal) => failAll(new StepError("adapter_exited", `the adapter exited (${signal || code})`)));
  child.on("error", (error) => failAll(new StepError("adapter_failed", error.message)));

  // One request, bounded by what is left of the agent's time.
  function send(method, params, capMs = Infinity) {
    const id = nextId++;
    return new Promise((ok, fail) => {
      const left = Math.min(deadline - Date.now(), capMs);
      const timer = setTimeout(() => {
        pending.delete(id);
        fail(new StepError("timeout", `no answer to ${method} in time (--timeout ${opts.timeout})`));
      }, Math.max(left, 0));
      pending.set(id, {
        ok: (v) => (clearTimeout(timer), ok(v)),
        fail: (e) => (clearTimeout(timer), fail(e)),
      });
      write({ id, method, params });
    });
  }

  createInterface({ input: child.stdout }).on("line", (line) => {
    let msg;
    trace("<", line);
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.method === "session/update") updates.push(msg.params?.update || {});
    else if (msg.method === "_auth/status_update") authStatus = msg.params?.authStatus?.kind ?? null;
    if (msg.method === "session/request_permission" && msg.id !== undefined) {
      // The prompt needs no tools, so every request is refused.
      permissionsRefused++;
      const options = msg.params?.options || [];
      const reject = options.find((o) => o.kind === "reject_once") || options.find((o) => o.kind === "reject_always");
      const outcome = reject ? { outcome: "selected", optionId: reject.optionId } : { outcome: "cancelled" };
      write({ id: msg.id, result: { outcome } });
    } else if (msg.method && msg.id !== undefined) {
      write({ id: msg.id, error: { code: -32601, message: "not supported by the engine smoke" } });
    } else if (!msg.method && msg.id !== undefined && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) {
        const reason = msg.error.code === AUTH_REQUIRED ? "auth_required" : "acp_error";
        p.fail(new StepError(reason, `${msg.error.code} ${msg.error.message}`));
      } else p.ok(msg.result);
    }
  });

  // Stops the adapter and everything it started, then removes the scratch folder: the normal
  // end and every failure come here. The process tree is read before anything is signalled.
  // On macOS and Linux: SIGTERM and 10 s to exit, then SIGKILL and 5 s more, then the same for
  // the children left over. On Windows, where a signal is a hard kill, stdin is closed first
  // and the adapter gets the same 10 s to exit on its own.
  const gone = () => child.exitCode !== null || child.signalCode !== null || child.pid === undefined;
  const exitWithin = (ms) =>
    gone()
      ? Promise.resolve(true)
      : new Promise((ok) => {
          const timer = setTimeout(() => (child.off("exit", onExit), ok(false)), ms);
          const onExit = () => (clearTimeout(timer), ok(true));
          child.once("exit", onExit);
        });
  let cleaning = null;
  const cleanup = () => (cleaning ??= stop());
  interrupted = () => cleanup();
  async function stop() {
    const tree = descendants(child.pid);
    if (process.platform === "win32") {
      child.stdin.end();
      if (!(await exitWithin(10000))) {
        killQuietly(child.pid, "SIGKILL");
        await exitWithin(5000);
      }
      if (!(await allGone(tree, 10000))) {
        for (const pid of tree.filter(alive)) killQuietly(pid, "SIGKILL");
        await allGone(tree, 5000);
      }
    } else {
      if (!gone()) killQuietly(child.pid, "SIGTERM");
      if (!(await exitWithin(10000))) {
        killQuietly(child.pid, "SIGKILL");
        await exitWithin(5000);
      }
      if (!(await allGone(tree, 10000))) {
        for (const pid of tree.filter(alive)) killQuietly(pid, "SIGKILL");
        await allGone(tree, 5000);
      }
    }
    const left = [child.pid, ...tree].filter((pid) => pid !== undefined && alive(pid));
    server?.close();
    if (keep) console.error(`kept ${root}`);
    else await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
    return left;
  }

  let ok = false;
  let reason = null;
  let reply = "";
  let deleted = null;
  try {
    if (spawnError) throw new StepError("adapter_failed", spawnError.message);
    const init = await send("initialize", {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
      clientInfo: { name: "clauseo-engine-smoke", version: "1" },
    });
    row.adapter_version = init?.agentInfo ? `${init.agentInfo.name} ${init.agentInfo.version}` : `${name} ${entry.version} (package.json)`;

    const session = await send("session/new", { cwd: work, mcpServers: [] });
    // A config_option_update between session/new and the prompt replaces the options.
    let configOptions = session?.configOptions;
    for (const u of updates) if (u.sessionUpdate === "config_option_update" && u.configOptions) configOptions = u.configOptions;
    const seen = readModels(session, configOptions);
    Object.assign(row, { models: seen.models, current_model: seen.current_model, current_effort: seen.current_effort });
    row.expected_missing = expect.filter((id) => !seen.known.has(id));
    if (!seen.models.length) throw new StepError("no_models", "session/new listed no models");

    updates.length = 0;
    const result = await send("session/prompt", { sessionId: session.sessionId, prompt: [{ type: "text", text: PROMPT }] });
    row.stop_reason = result?.stopReason ?? null;
    reply = updates
      .filter((u) => u.sessionUpdate === "agent_message_chunk" && u.content?.type === "text")
      .map((u) => u.content.text)
      .join("")
      .trim();
    row.prompt_ok = row.stop_reason === "end_turn" && reply.includes("OK");
    // Best effort: drop the smoke's conversation from the account's session list, where the
    // adapter offers session/delete. Not a prompt, and its failure fails nothing.
    if (init?.agentCapabilities?.sessionCapabilities?.delete) {
      deleted = await send("session/delete", { sessionId: session.sessionId }, 15000).then(() => true, () => false);
    }
    if (!row.prompt_ok) reason = `prompt: stop_reason ${row.stop_reason}, reply ${JSON.stringify(reply.slice(0, 200))}`;
    else if (row.expected_missing.length) reason = `expected models missing: ${row.expected_missing.join(", ")}`;
    else ok = true;
  } catch (error) {
    reason = error instanceof StepError ? `${error.reason}: ${error.message}` : `error: ${error.message}`;
  }
  const left = await cleanup();
  if (left.length) {
    ok = false;
    reason = `${reason ? reason + "; " : ""}processes left running: ${left.join(", ")}`;
  }
  if (!ok && !reason?.startsWith("auth_required") && stderr.trim()) console.error(`--- ${agent} adapter stderr (last 2000 characters)\n${stderr.slice(-2000)}`);
  row.seconds = Math.round((Date.now() - started) / 100) / 10;
  const extra = { ok, reason };
  if (reply) extra.reply = reply.slice(0, 200);
  if (authStatus) extra.auth_status = authStatus;
  if (deleted !== null) extra.session_deleted = deleted;
  if (permissionsRefused) extra.permissions_refused = permissionsRefused;
  if (mock) extra.mock = true;
  return { ...row, ...extra };
}

// Ctrl-C or a SIGTERM to the smoke still stops the adapter and removes the folder.
let interrupted = null;
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, async () => {
    if (interrupted) await interrupted();
    console.log("FAIL");
    process.exit(130);
  });
}

const opts = parseArgs(process.argv.slice(2));
if (!existsSync(opts.modules)) usage(`no such folder: ${opts.modules}`);
let allPassed = true;
for (const agent of opts.agents) {
  const result = await smokeAgent(agent, opts);
  if (!result.ok) allPassed = false;
  console.log(JSON.stringify(result));
}
console.log(allPassed ? "PASS" : "FAIL");
process.exit(allPassed ? 0 : 1);
