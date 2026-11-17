#!/usr/bin/env node
// Runs codex-acp and Codex against a scripted stand-in for the Responses API and checks four
// rows of the Codex launch: the goal tools, `exec_command` with `tty`, `write_stdin` with input,
// and ordinary work. See README.md beside this file. No login, no network, no credentials.
//
//   node scripts/codex-launch-probe/probe.mjs <node_modules holding @agentclientprotocol/codex-acp>
//
// BASELINE=1 leaves the two feature keys out, to see the rows fail. KEYS_IN=config or
// KEYS_IN=home puts them in only CODEX_CONFIG or only the home's config.toml.

import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";

const modules = process.argv[2];
if (!modules) {
  console.error("usage: probe.mjs <node_modules folder with @agentclientprotocol/codex-acp>");
  process.exit(2);
}
const acp = resolve(modules, "@agentclientprotocol/codex-acp/dist/index.js");
const baseline = process.env.BASELINE === "1";
const keep = process.env.KEEP === "1";

// The launch keys, as `crates/host/src/agent/codex.rs` writes them: `clauseo_limits::
// CODEX_OVERRIDES` and `LAUNCH_FEATURES`, in the home's config.toml and in CODEX_CONFIG.
const OVERRIDES = [
  ["features.apps", "false"],
  ["features.plugins", "false"],
  ["features.skill_mcp_dependency_install", "false"],
  ["cloud.skills.enabled", "false"],
  ["skills.bundled.enabled", "false"],
  ["skills.include_instructions", "false"],
  ["cli_auth_credentials_store", '"file"'],
  ["shell_environment_policy.ignore_default_excludes", "false"],
];
const FEATURES = [
  ["features.goals", "false"],
  ["features.unified_exec_tty", "false"],
];
const keysIn = baseline ? "none" : process.env.KEYS_IN || "both";
const homeKeys = ["both", "home"].includes(keysIn) ? [...OVERRIDES, ...FEATURES] : OVERRIDES;
const configKeys = ["both", "config"].includes(keysIn) ? [...OVERRIDES, ...FEATURES] : OVERRIDES;

const root = mkdtempSync(join(tmpdir(), "codex-launch-probe-"));
const home = join(root, "home");
const work = join(root, "work");
mkdirSync(home);
mkdirSync(work);
writeFileSync(join(work, "note.txt"), "MARKER in matter\n");

// The model's steps on the main thread, one per request; `{{SID}}` is the last session id an
// `exec_command` output named.
const PATCH = "*** Begin Patch\n*** Add File: patched.txt\n+patched\n*** End Patch\n";
const STEPS = [
  { fn: "exec_command", args: { cmd: "echo shell-ok > shell.txt; cat shell.txt" } },
  { fn: "exec_command", args: { cmd: "bash --norc -i", tty: true, yield_time_ms: 800 } },
  { fn: "exec_command", args: { cmd: "sleep 4; echo long-done", yield_time_ms: 500 } },
  { fn: "write_stdin", args: { session_id: "{{SID}}", chars: "echo typed\n", yield_time_ms: 500 } },
  { fn: "write_stdin", args: { session_id: "{{SID}}", chars: "", yield_time_ms: 6000 } },
  { fn: "exec_command", args: { cmd: "rg -n MARKER ." } },
  { custom: "apply_patch", input: PATCH },
  { text: "Probe finished." },
];
const GOAL_TOOLS = ["create_goal", "get_goal", "update_goal"];

const requests = [];
let lastSid = null;
let step = 0;

function sse(res, events) {
  res.writeHead(200, { "content-type": "text/event-stream" });
  for (const ev of events) res.write(`event: ${ev.type}\ndata: ${JSON.stringify(ev)}\n\n`);
  res.end();
}
const done = (id) => ({
  type: "response.completed",
  response: { id, usage: { input_tokens: 0, input_tokens_details: null, output_tokens: 0, output_tokens_details: null, total_tokens: 0 } },
});
const message = (text) => ({
  type: "response.output_item.done",
  item: { type: "message", role: "assistant", id: "m", content: [{ type: "output_text", text }] },
});

// The text of every tool output in a request's input, in order.
function outputs(body) {
  return (body.input || [])
    .filter((item) => item.type === "function_call_output" || item.type === "custom_tool_call_output")
    .map((item) => (typeof item.output === "string" ? item.output : JSON.stringify(item.output)));
}

const server = createServer((req, res) => {
  let raw = "";
  req.on("data", (chunk) => (raw += chunk));
  req.on("end", () => {
    if (!req.url.endsWith("/responses")) {
      res.writeHead(req.url.includes("/models") ? 200 : 404, { "content-type": "application/json" });
      res.end(req.url.includes("/models") ? '{"models":[]}' : "{}");
      return;
    }
    const body = JSON.parse(raw);
    const names = (body.tools || []).map((t) => t.name || t.type);
    const main = names.includes("exec_command");
    const exec = (body.tools || []).find((t) => t.name === "exec_command");
    // The title thread gets no tools of its own: its input carries the main thread's tool
    // declarations as code-mode text (`declare const tools: { create_goal(...) }`, `tty?: boolean`),
    // so the whole request is searched, not only `tools`.
    const record = {
      thread: main ? "main" : "title",
      model: body.model,
      goalTools: GOAL_TOOLS.filter((name) => raw.includes(name)),
      ttyParam: exec ? JSON.stringify(exec.parameters || {}).includes('"tty"') : /tty\?: boolean/.test(raw),
      outputs: outputs(body),
    };
    requests.push(record);
    const id = `resp-${requests.length}`;
    if (!main) return sse(res, [{ type: "response.created", response: { id } }, message("Probe title"), done(id)]);
    const seen = record.outputs;
    for (const text of seen) {
      const sid = /session ID (\d+)/i.exec(text);
      if (sid) lastSid = Number(sid[1]);
    }
    step = seen.length;
    const s = STEPS[Math.min(step, STEPS.length - 1)];
    const call = `call-${step}`;
    let item;
    if (s.fn) {
      const args = JSON.stringify(s.args).replace('"{{SID}}"', String(lastSid));
      item = { type: "response.output_item.done", item: { type: "function_call", call_id: call, name: s.fn, arguments: args } };
    } else if (s.custom) {
      item = { type: "response.output_item.done", item: { type: "custom_tool_call", call_id: call, name: s.custom, input: s.input } };
    } else {
      item = message(s.text);
    }
    sse(res, [{ type: "response.created", response: { id } }, item, done(id)]);
  });
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const port = server.address().port;

const tomlLines = homeKeys.map(([k, v]) => `${k} = ${v}`);
writeFileSync(
  join(home, "config.toml"),
  [
    'model = "gpt-5.5"',
    'model_provider = "mock"',
    ...tomlLines,
    "",
    "[model_providers.mock]",
    'name = "mock"',
    `base_url = "http://127.0.0.1:${port}/v1"`,
    'wire_api = "responses"',
    "requires_openai_auth = false",
    "supports_websockets = false",
    "",
  ].join("\n"),
);
const codexConfig = {};
for (const [k, v] of configKeys) {
  const parts = k.split(".");
  let at = codexConfig;
  for (const part of parts.slice(0, -1)) at = at[part] ??= {};
  at[parts.at(-1)] = JSON.parse(v);
}

const child = spawn(process.execPath, [acp], {
  cwd: work,
  env: {
    PATH: process.env.PATH,
    HOME: root,
    CODEX_HOME: home,
    CODEX_CONFIG: JSON.stringify(codexConfig),
    INITIAL_AGENT_MODE: "workspace-write",
    DISABLE_MCP_CONFIG_FILTERING: "true",
  },
  stdio: ["pipe", "pipe", "pipe"],
});
let stderr = "";
child.stderr.on("data", (d) => (stderr += d));
// A reply written as the child exits must not crash the probe while cleanup waits for it.
child.stdin.on("error", () => {});
let nextId = 1;
const pending = new Map();
const updates = [];
function send(method, params) {
  const id = nextId++;
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  return new Promise((ok, fail) => pending.set(id, { ok, fail }));
}
createInterface({ input: child.stdout }).on("line", (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return;
  }
  if (msg.method === "session/update") updates.push(msg.params.update);
  if (msg.method === "session/request_permission") {
    const allow = msg.params.options.find((o) => o.kind?.startsWith("allow")) || msg.params.options[0];
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { outcome: { outcome: "selected", optionId: allow.optionId } } }) + "\n");
  } else if (msg.method && msg.id !== undefined) {
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "not supported" } }) + "\n");
  }
  if (msg.id !== undefined && pending.has(msg.id) && !msg.method) {
    const p = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? p.fail(new Error(JSON.stringify(msg.error))) : p.ok(msg.result);
  }
});

// Stops the child and removes the scratch folder. The normal end, a thrown error and the timeout
// all come here, and it runs once; later calls get the same promise. It waits for the child to
// exit, then for the Codex processes the child started (codex-acp starts Codex, which can outlive
// codex-acp by a moment), and only then removes the folder, so nothing writes into it while it
// shuts down. Each wait is capped: SIGTERM and 10 s to exit, then SIGKILL and 5 s more.
let cleaning = null;
const gone = () => child.exitCode !== null || child.signalCode !== null || child.pid === undefined;
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
};
function exitWithin(ms) {
  if (gone()) return Promise.resolve(true);
  return new Promise((ok) => {
    const timer = setTimeout(() => {
      child.off("exit", onExit);
      ok(false);
    }, ms);
    const onExit = () => {
      clearTimeout(timer);
      ok(true);
    };
    child.once("exit", onExit);
  });
}
// The pids below `parent` in the process table, read before anything is signalled.
function descendants(parent) {
  if (parent === undefined) return [];
  let rows;
  try {
    rows = execFileSync("ps", ["-axo", "pid=,ppid="], { encoding: "utf8" });
  } catch {
    return [];
  }
  const kids = new Map();
  for (const [pid, ppid] of rows.trim().split("\n").map((row) => row.trim().split(/\s+/).map(Number))) {
    kids.set(ppid, [...(kids.get(ppid) || []), pid]);
  }
  const found = [];
  for (let level = [parent]; level.length; ) {
    level = level.flatMap((pid) => kids.get(pid) || []);
    found.push(...level);
  }
  return found;
}
async function allGone(pids, ms) {
  const until = Date.now() + ms;
  while (pids.some(alive)) {
    if (Date.now() >= until) return false;
    await new Promise((ok) => setTimeout(ok, 100));
  }
  return true;
}
function cleanup() {
  cleaning ??= (async () => {
    clearTimeout(timeout);
    const started = descendants(child.pid);
    if (!gone()) child.kill("SIGTERM");
    if (!(await exitWithin(10000))) {
      child.kill("SIGKILL");
      await exitWithin(5000);
    }
    if (!(await allGone(started, 10000))) {
      for (const pid of started.filter(alive)) process.kill(pid, "SIGKILL");
      await allGone(started, 5000);
    }
    server.close();
    if (keep) console.log(`kept ${root}`);
    else await rm(root, { recursive: true, force: true });
  })();
  return cleaning;
}

const timeout = setTimeout(async () => {
  console.error("timed out\n" + stderr.slice(-4000));
  await cleanup();
  process.exit(3);
}, 120000);

let failed = 0;
const check = (row, ok, detail) => {
  if (!ok) failed++;
  console.log(`${ok ? "PASS" : "FAIL"} ${row}: ${detail}`);
};
try {
  await send("initialize", {
    protocolVersion: 1,
    clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
  });
  const session = await send("session/new", { cwd: work, mcpServers: [] });
  const result = await send("session/prompt", { sessionId: session.sessionId, prompt: [{ type: "text", text: "probe step one" }] });
  await new Promise((ok) => setTimeout(ok, 3000));
  console.log(`feature keys in: ${keysIn}; stop: ${result.stopReason}`);

  const mains = requests.filter((r) => r.thread === "main");
  const titles = requests.filter((r) => r.thread === "title");
  const final = mains.at(-1)?.outputs || [];
  const out = (i) => final[i] || "";
  for (const r of requests) console.log(`  request ${r.thread} ${r.model} goalTools=[${r.goalTools}] ttyParam=${r.ttyParam}`);
  final.forEach((text, i) => console.log(`  output ${i}: ${text.replace(/\s+/g, " ").slice(0, 160)}`));

  check("a goal tools and tty", titles.length > 0 && requests.every((r) => r.goalTools.length === 0 && !r.ttyParam),
    `${mains.length} main and ${titles.length} title requests; goal tools listed in ${requests.filter((r) => r.goalTools.length).length}, tty in ${requests.filter((r) => r.ttyParam).length}`);
  check("b exec_command tty:true", /TTY execution is disabled by config/.test(out(1)), out(1).replace(/\s+/g, " ").slice(0, 120));
  check("c write_stdin with input", /stdin is closed for this session/.test(out(3)), out(3).replace(/\s+/g, " ").slice(0, 120));
  check("d shell", existsSync(join(work, "shell.txt")) && readFileSync(join(work, "shell.txt"), "utf8").trim() === "shell-ok", "shell.txt holds shell-ok");
  check("d rg", /note\.txt:1:MARKER in matter/.test(out(5)), "rg found ./note.txt:1");
  check("d apply_patch", existsSync(join(work, "patched.txt")), "patched.txt made");
  check("d long command polled", /long-done/.test(out(4)) && /exit code:? 0|exited with code 0/i.test(out(4)), "empty poll returned long-done, exit 0");
  check("d turn ends", result.stopReason === "end_turn" && mains.length === STEPS.length, `${mains.length} main requests for ${STEPS.length} steps`);
} catch (error) {
  failed++;
  console.log(`FAIL run: ${error.message}\n${stderr.slice(-4000)}`);
} finally {
  await cleanup();
}
console.log(failed ? `${failed} failed` : "all rows passed");
process.exit(failed ? 1 : 0);
