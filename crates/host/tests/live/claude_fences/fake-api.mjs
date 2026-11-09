// A fake Anthropic Messages API on 127.0.0.1 for the D2d fence proof. No dependencies.
//
// - Main-loop turns are scripted per scenario. A request is a scenario's step 0 when its last
//   user message holds the text `[[SCN:<name>]]`, and step k+1 when it holds the tool_result of
//   step k's tool_use (ids `toolu_fp_<name>_<k>_<i>`). The step's tool_use blocks are sent back;
//   past the last step the reply is a plain "done" (end_turn).
// - Auto mode's classifier calls (system prompt "You are a security monitor ...", or a
//   `</block>`/`</severity>` stop sequence) get a scripted verdict: `allow` gives
//   `<block>no</block><severity>0</severity>`, `deny` gives
//   `<block>yes</block><reason>scripted deny</reason><severity>100</severity>`.
// - Any other call (titles, quota probes, ...) gets a short text reply.
// - `rejectAfkBeta`: answer HTTP 400 naming the beta to any request carrying
//   `afk-mode-2026-01-31` (the "plan" circuit breaker, auto-gate.md §1.5).
// Streams SSE when the request has `stream: true`, else plain JSON. Logs every request as one
// JSON line.

import http from "node:http";
import fs from "node:fs";

export const AFK_BETA = "afk-mode-2026-01-31";
const CLASSIFIER_SYSTEM = "You are a security monitor for autonomous AI coding agents";

function systemText(body) {
  const s = body.system;
  if (typeof s === "string") return s;
  if (Array.isArray(s)) return s.map((b) => (typeof b === "string" ? b : b.text ?? "")).join("\n");
  return "";
}

function blocks(content) {
  if (typeof content === "string") return [{ type: "text", text: content }];
  return Array.isArray(content) ? content : [];
}

function resultText(content) {
  return blocks(content)
    .map((b) => (b.type === "text" ? b.text : JSON.stringify(b)))
    .join("\n");
}

export function isClassifier(body) {
  const stops = body.stop_sequences ?? [];
  return (
    systemText(body).includes(CLASSIFIER_SYSTEM) ||
    stops.includes("</block>") ||
    stops.includes("</severity>")
  );
}

/** The action a classifier request asks about: the transcript's last entry, the text block
 *  just before the `</transcript>` block (CLI 2.1.293's layout), else the whole text. */
export function classifierAction(body) {
  const texts = (body.messages ?? []).flatMap((m) => blocks(m.content)).map((b) => b.text ?? "");
  const end = texts.findLastIndex((t) => t.startsWith("</transcript>"));
  return end > 0 ? texts[end - 1] : texts.join("\n");
}

export class FakeApi {
  constructor({ log }) {
    this.logPath = log;
    this.scenarios = new Map(); // name -> [{ tools: [{name, input}], text? }]
    this.classifier = "allow"; // "allow" | "deny" | (actionText) => "allow" | "deny"
    this.rejectAfkBeta = false;
    this.seq = 0;
    this.label = "";
  }

  script(name, steps) {
    this.scenarios.set(name, steps);
  }

  log(entry) {
    fs.appendFileSync(this.logPath, JSON.stringify({ t: new Date().toISOString(), label: this.label, ...entry }) + "\n");
  }

  async start() {
    this.server = http.createServer((req, res) => this.handle(req, res));
    await new Promise((ok) => this.server.listen(0, "127.0.0.1", ok));
    this.port = this.server.address().port;
    this.url = `http://127.0.0.1:${this.port}`;
    return this;
  }

  stop() {
    this.server?.closeAllConnections?.();
    return new Promise((ok) => (this.server ? this.server.close(() => ok()) : ok()));
  }

  handle(req, res) {
    let raw = "";
    req.on("data", (c) => (raw += c));
    req.on("end", () => {
      try {
        this.route(req, res, raw);
      } catch (error) {
        this.log({ path: req.url, error: String(error?.stack ?? error) });
        res.writeHead(500, { "content-type": "application/json" });
        res.end(JSON.stringify({ type: "error", error: { type: "api_error", message: "fake api bug" } }));
      }
    });
  }

  route(req, res, raw) {
    const path = req.url.split("?")[0];
    const betas = String(req.headers["anthropic-beta"] ?? "");
    const via = req.headers["x-fence-proxy"] ? "proxy" : "direct";
    if (req.method === "POST" && path === "/v1/messages/count_tokens") {
      this.log({ path, kind: "count_tokens", via });
      return json(res, 200, { input_tokens: 100 });
    }
    if (req.method === "GET" && path === "/v1/models") {
      this.log({ path, kind: "models", via });
      return json(res, 200, { data: [], has_more: false, first_id: null, last_id: null });
    }
    if (req.method !== "POST" || path !== "/v1/messages") {
      this.log({ path, method: req.method, kind: "unknown", via });
      return json(res, 404, { type: "error", error: { type: "not_found_error", message: "not found" } });
    }
    const body = JSON.parse(raw || "{}");
    const n = ++this.seq;
    const common = {
      n,
      path,
      via,
      model: body.model,
      stream: !!body.stream,
      max_tokens: body.max_tokens,
      stop_sequences: body.stop_sequences,
      betas,
      afk_beta: betas.includes(AFK_BETA) || JSON.stringify(body.anthropic_beta ?? "").includes(AFK_BETA),
    };
    if (this.rejectAfkBeta && common.afk_beta) {
      const message = `Unexpected value(s) \`${AFK_BETA}\` for the \`anthropic-beta\` header. Please consult our documentation at docs.anthropic.com or try again without the header.`;
      this.log({ ...common, kind: "afk-rejected", status: 400 });
      return json(res, 400, { type: "error", error: { type: "invalid_request_error", message } });
    }
    if (isClassifier(body)) {
      const action = classifierAction(body);
      const policy = typeof this.classifier === "function" ? this.classifier(action) : this.classifier;
      const text =
        policy === "deny"
          ? "<block>yes</block><reason>scripted deny</reason><severity>100</severity>"
          : "<block>no</block><severity>0</severity>";
      this.log({ ...common, kind: "classifier", verdict: policy, action: action.slice(-600), ...(this.debug ? { body } : {}) });
      return this.reply(res, body, [{ type: "text", text }], "end_turn");
    }
    const last = [...(body.messages ?? [])].reverse().find((m) => m.role === "user");
    const content = blocks(last?.content);
    const toolResults = content
      .filter((b) => b.type === "tool_result")
      .map((b) => ({ tool_use_id: b.tool_use_id, is_error: !!b.is_error, content: resultText(b.content).slice(0, 2000) }));
    const userText = content.filter((b) => b.type === "text").map((b) => b.text).join("\n");
    const tools = (body.tools ?? []).map((t) => t.name);
    let scenario = null;
    let step = 0;
    const marker = userText.match(/\[\[SCN:([A-Za-z0-9_-]+)\]\]/);
    if (marker) scenario = marker[1];
    for (const r of toolResults) {
      const m = String(r.tool_use_id).match(/^toolu_fp_([A-Za-z0-9-]+)_(\d+)_\d+$/);
      if (m) {
        scenario = m[1];
        step = Number(m[2]) + 1;
      }
    }
    const steps = scenario ? this.scenarios.get(scenario) : undefined;
    if (!steps) {
      this.log({ ...common, kind: "other", system: systemText(body).slice(0, 120), toolCount: tools.length, toolResults, userText: userText.slice(0, 300) });
      return this.reply(res, body, [{ type: "text", text: "ok" }], "end_turn");
    }
    const entry = steps[step];
    this.log({ ...common, kind: "main", scenario, step, tools, toolResults, userText: userText.slice(0, 300) });
    if (!entry) return this.reply(res, body, [{ type: "text", text: "done" }], "end_turn");
    const out = [];
    if (entry.text) out.push({ type: "text", text: entry.text });
    (entry.tools ?? []).forEach((t, i) =>
      out.push({ type: "tool_use", id: `toolu_fp_${scenario}_${step}_${i}`, name: t.name, input: t.input }),
    );
    return this.reply(res, body, out, out.some((b) => b.type === "tool_use") ? "tool_use" : "end_turn");
  }

  reply(res, body, content, stopReason) {
    let stopSequence = null;
    // Emulate stop sequences on text replies (the classifier's stage 1).
    for (const block of content) {
      if (block.type !== "text") continue;
      for (const stop of body.stop_sequences ?? []) {
        const at = block.text.indexOf(stop);
        if (at >= 0) {
          block.text = block.text.slice(0, at);
          stopReason = "stop_sequence";
          stopSequence = stop;
        }
      }
    }
    const id = `msg_fp_${this.seq}_${Date.now()}`;
    const usage = { input_tokens: 100, output_tokens: 20, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 };
    const message = { id, type: "message", role: "assistant", model: body.model ?? "claude-sonnet-5", content, stop_reason: stopReason, stop_sequence: stopSequence, usage };
    if (!body.stream) return json(res, 200, message);
    res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache", connection: "keep-alive", "request-id": id });
    const send = (event, data) => res.write(`event: ${event}\ndata: ${JSON.stringify({ type: event, ...data })}\n\n`);
    send("message_start", { message: { ...message, content: [], stop_reason: null, stop_sequence: null, usage: { ...usage, output_tokens: 1 } } });
    content.forEach((block, index) => {
      if (block.type === "text") {
        send("content_block_start", { index, content_block: { type: "text", text: "" } });
        send("content_block_delta", { index, delta: { type: "text_delta", text: block.text } });
      } else {
        send("content_block_start", { index, content_block: { type: "tool_use", id: block.id, name: block.name, input: {} } });
        send("content_block_delta", { index, delta: { type: "input_json_delta", partial_json: JSON.stringify(block.input) } });
      }
      send("content_block_stop", { index });
    });
    send("message_delta", { delta: { stop_reason: stopReason, stop_sequence: stopSequence }, usage: { output_tokens: 20 } });
    send("message_stop", {});
    res.end();
  }
}

function json(res, status, value) {
  res.writeHead(status, { "content-type": "application/json", "request-id": `req_fp_${Date.now()}` });
  res.end(JSON.stringify(value));
}
