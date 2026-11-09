// An ACP client for the fence proof: spawns the pinned claude-agent-acp, records every
// message it receives, answers permission requests by a policy, and drives sessions.

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { Readable, Writable } from "node:stream";
import { pathToFileURL } from "node:url";

/** Clauseo's own `clientCapabilities` (crates/acp/src/driver.rs `client_capabilities`). */
export const CLAUSEO_CLIENT_CAPABILITIES = {
  auth: { terminal: true },
  elicitation: { form: {}, url: {} },
  session: { notices: {}, compaction: {}, configOptions: { boolean: {} } },
  plan: {},
  _meta: { jetbrains: { air: { version: 1, capabilities: ["sessionFailure"] } } },
};

export class AcpRun {
  /**
   * @param {object} o
   * @param {string} o.pkg node_modules folder holding the pinned packages
   * @param {Record<string,string>} o.env the adapter's whole environment
   * @param {string} o.log JSONL file for every ACP message
   * @param {string} o.stderrLog file for the adapter's stderr
   */
  constructor({ pkg, env, log, stderrLog, node = process.execPath }) {
    this.pkg = pkg;
    this.env = env;
    this.logPath = log;
    this.stderrLog = stderrLog;
    this.node = node;
    this.events = []; // { t, label, dir, method, params | result | error }
    this.label = "";
    this.permission = () => "reject"; // (request) => "reject" | "allow"
  }

  record(entry) {
    const full = { t: new Date().toISOString(), label: this.label, ...entry };
    this.events.push(full);
    fs.appendFileSync(this.logPath, JSON.stringify(full) + "\n");
  }

  async start() {
    const acp = await import(pathToFileURL(path.join(this.pkg, "@agentclientprotocol/sdk/dist/acp.js")).href);
    const entry = path.join(this.pkg, "@agentclientprotocol/claude-agent-acp/dist/index.js");
    this.child = spawn(this.node, [entry], { env: this.env, stdio: ["pipe", "pipe", "pipe"], detached: true });
    const err = fs.createWriteStream(this.stderrLog, { flags: "a" });
    this.child.stderr.pipe(err);
    this.child.on("exit", (code, signal) => this.record({ dir: "process", method: "exit", params: { code, signal } }));
    const input = Writable.toWeb(this.child.stdin);
    const output = Readable.toWeb(this.child.stdout);
    const stream = acp.ndJsonStream(input, output);
    const run = this;
    this.conn = new acp.ClientSideConnection(
      () => ({
        async sessionUpdate(params) {
          run.record({ dir: "in", method: "session/update", params });
        },
        async requestPermission(params) {
          const decision = run.permission(params);
          const kinds = decision === "allow" ? ["allow_once", "allow_always"] : ["reject_once", "reject_always"];
          const option = params.options.find((o) => o.kind === kinds[0]) ?? params.options.find((o) => kinds.includes(o.kind));
          const result = option ? { outcome: { outcome: "selected", optionId: option.optionId } } : { outcome: { outcome: "cancelled" } };
          run.record({ dir: "in", method: "session/request_permission", params, answer: result });
          return result;
        },
        async extMethod(method, params) {
          run.record({ dir: "in", method, params });
          return {};
        },
        async extNotification(method, params) {
          run.record({ dir: "in", method, params });
        },
      }),
      stream,
    );
    return this;
  }

  /** Sends one request through `fn(conn)`, recording the request and its reply or error. */
  async call(method, params, fn) {
    this.record({ dir: "out", method, params: summarizeParams(method, params) });
    try {
      const result = await fn(this.conn);
      this.record({ dir: "reply", method, result });
      return { ok: true, result };
    } catch (error) {
      const e = { code: error?.code, message: error?.message, data: error?.data };
      this.record({ dir: "reply", method, error: e });
      return { ok: false, error: e };
    }
  }

  initialize() {
    const params = { protocolVersion: 1, clientCapabilities: CLAUSEO_CLIENT_CAPABILITIES, clientInfo: { name: "clauseo", version: "fence-proof" } };
    return this.call("initialize", params, (c) => c.initialize(params));
  }

  newSession(cwd, meta) {
    const params = { cwd, mcpServers: [], _meta: meta };
    return this.call("session/new", params, (c) => c.newSession(params));
  }

  setMode(sessionId, modeId) {
    const params = { sessionId, modeId };
    return this.call("session/set_mode", params, (c) => c.setSessionMode(params));
  }

  prompt(sessionId, text, timeoutMs = 180000) {
    const params = { sessionId, prompt: [{ type: "text", text }] };
    return this.call("session/prompt", params, (c) =>
      Promise.race([
        c.prompt(params),
        new Promise((_, no) => setTimeout(() => no(Object.assign(new Error("harness timeout"), { code: "TIMEOUT" })), timeoutMs)),
      ]),
    );
  }

  /** Every event recorded under `label`. */
  of(label) {
    return this.events.filter((e) => e.label === label);
  }

  async stop() {
    if (!this.child || this.child.exitCode !== null) return;
    const gone = new Promise((ok) => this.child.once("exit", ok));
    try {
      process.kill(-this.child.pid, "SIGTERM");
    } catch {}
    const timer = setTimeout(() => {
      try {
        process.kill(-this.child.pid, "SIGKILL");
      } catch {}
    }, 3000);
    await gone;
    clearTimeout(timer);
    // The adapter's process group may hold Claude Code children still: make sure.
    try {
      process.kill(-this.child.pid, "SIGKILL");
    } catch {}
  }
}

function summarizeParams(method, params) {
  if (method === "session/new") return { ...params, _meta: "(the dumped Clauseo meta)" };
  return params;
}
