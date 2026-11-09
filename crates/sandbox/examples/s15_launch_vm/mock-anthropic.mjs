// A scripted Anthropic Messages server on loopback, for the S15 harness's claude-mock row.
//
//   node mock-anthropic.mjs [port=N] [log=FILE]
//
// What it mimics: `POST /v1/messages` (query strings such as `?beta=true` are ignored), streamed
// as server-sent events (message_start, content_block_start, content_block_delta,
// content_block_stop, message_delta, message_stop) when the request says `stream: true`, else
// one JSON message; `POST /v1/messages/count_tokens`; `GET /v1/models`.
// - A request whose `tools` offer a shell tool (`Bash` first, else `PowerShell`) and that carries
//   no tool result yet gets one `tool_use` of that tool running `echo clauseo-mock-ok`.
// - A request carrying a tool result gets a short text that quotes the result, and `end_turn`;
//   so the CLI's printed answer holds clauseo-mock-ok only when the command really ran (an
//   error result's text has the marker masked).
// - Any other request (title, topic or summary calls, quota checks) gets a short text.
// What it doesn't: no model, no auth check (any key passes), no thinking blocks, no prompt
// caching, no rate limits or errors, no batches or files API. It keeps no state: it reads the
// history in each request. It proxies nothing: absolute-form requests and CONNECT get 403.
//
// Port 0 (the default) picks a free one. The first stdout line is `listening` with the port.
// Each request is one JSON line on stdout and in FILE: its path, model, the names in `tools`,
// the tool result it carried (cut to 300 characters) and what the mock replied. Never a body.
import fs from 'node:fs';
import http from 'node:http';

const opts = Object.fromEntries(process.argv.slice(2).map((a) => {
  const i = a.indexOf('=');
  return [a.slice(0, i), a.slice(i + 1)];
}));
const port = Number(opts.port || 0);
const logFile = opts.log;
const COMMAND = 'echo clauseo-mock-ok';
const SHELLS = ['Bash', 'PowerShell'];

function log(event) {
  const line = JSON.stringify({ t: new Date().toISOString(), ...event });
  console.log(line);
  if (logFile) fs.appendFileSync(logFile, line + '\n');
}

function json(res, status, body) {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(text) });
  res.end(text);
}

function textOf(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) return content.map((c) => (typeof c === 'string' ? c : c.text ?? textOf(c.content ?? ''))).join(' ');
  return content == null ? '' : JSON.stringify(content);
}

// The tool_result blocks of the last user message: the turn after a tool ran.
function toolResults(messages) {
  const last = [...messages].reverse().find((m) => m.role === 'user');
  if (!last || !Array.isArray(last.content)) return [];
  return last.content.filter((c) => c && c.type === 'tool_result');
}

// The tool's input: `command` set, every other required string filled from its schema.
function inputFor(tool) {
  const props = tool.input_schema?.properties || {};
  const input = { command: COMMAND };
  for (const key of tool.input_schema?.required || []) {
    if (key in input) continue;
    const type = props[key]?.type;
    if (type === 'string') input[key] = 'Print the mock marker';
    else if (type === 'boolean') input[key] = false;
  }
  if ('description' in props && !('description' in input)) input.description = 'Print the mock marker';
  return input;
}

function usage(output) {
  return { input_tokens: 10, output_tokens: output, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 };
}

function reply(res, body, block, stop) {
  const model = body.model || 'claude-mock';
  const id = `msg_mock_${Date.now()}`;
  if (!body.stream) {
    return json(res, 200, { id, type: 'message', role: 'assistant', model, content: [block], stop_reason: stop, stop_sequence: null, usage: usage(5) });
  }
  const events = [['message_start', { type: 'message_start', message: { id, type: 'message', role: 'assistant', model, content: [], stop_reason: null, stop_sequence: null, usage: usage(1) } }]];
  if (block.type === 'tool_use') {
    events.push(['content_block_start', { type: 'content_block_start', index: 0, content_block: { ...block, input: {} } }]);
    events.push(['content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(block.input) } }]);
  } else {
    events.push(['content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }]);
    events.push(['content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: block.text } }]);
  }
  events.push(['content_block_stop', { type: 'content_block_stop', index: 0 }]);
  events.push(['message_delta', { type: 'message_delta', delta: { stop_reason: stop, stop_sequence: null }, usage: { output_tokens: 5 } }]);
  events.push(['message_stop', { type: 'message_stop' }]);
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'close' });
  for (const [name, data] of events) res.write(`event: ${name}\ndata: ${JSON.stringify(data)}\n\n`);
  res.end();
}

function messages(path, body, res) {
  const tools = Array.isArray(body.tools) ? body.tools : [];
  const names = tools.map((t) => t.name || t.type);
  const results = toolResults(Array.isArray(body.messages) ? body.messages : []);
  const shell = SHELLS.map((name) => tools.find((t) => t.name === name)).find(Boolean);
  let block;
  let stop = 'end_turn';
  let replied;
  if (results.length) {
    const out = results.map((r) => textOf(r.content)).join(' ').trim();
    // An error result may quote the command, so its text hides the marker.
    const failed = results.some((r) => r.is_error);
    block = { type: 'text', text: failed ? `The command failed: ${out.replaceAll('clauseo-mock-ok', '<marker>')}` : `The command printed: ${out}` };
    replied = 'text-after-result';
  } else if (shell) {
    block = { type: 'tool_use', id: 'toolu_mock_1', name: shell.name, input: inputFor(shell) };
    stop = 'tool_use';
    replied = `tool_use:${shell.name}`;
  } else {
    // Side calls: titles and topic checks often ask for JSON, so the text is JSON too.
    block = { type: 'text', text: '{"isNewTopic": false, "title": "Mock run"}' };
    replied = 'text-side';
  }
  log({
    kind: 'messages', path, model: body.model, stream: !!body.stream, tools: names,
    shells: names.filter((n) => SHELLS.includes(n)),
    results: results.map((r) => ({ is_error: !!r.is_error, text: textOf(r.content).slice(0, 300) })),
    reply: replied,
  });
  reply(res, body, block, stop);
}

const server = http.createServer((req, res) => {
  const absolute = /^https?:\/\//i.test(req.url);
  const url = new URL(req.url, 'http://127.0.0.1');
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const p = url.pathname.replace(/\/+$/, '');
    if (absolute) {
      log({ kind: 'http', method: req.method, path: p, verdict: 'refused' });
      return json(res, 403, { error: 'this mock proxies nothing' });
    }
    let body = {};
    try { body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {}; } catch { body = {}; }
    if (req.method === 'POST' && p.endsWith('/v1/messages')) return messages(p, body, res);
    const tools = Array.isArray(body.tools) ? body.tools.map((t) => t.name || t.type) : [];
    log({ kind: 'http', method: req.method, path: p, tools });
    if (p.endsWith('/v1/messages/count_tokens')) return json(res, 200, { input_tokens: 10 });
    if (p.endsWith('/v1/models')) {
      return json(res, 200, { data: [{ type: 'model', id: 'claude-mock', display_name: 'Mock', created_at: '2026-01-01T00:00:00Z' }], has_more: false, first_id: 'claude-mock', last_id: 'claude-mock' });
    }
    return json(res, 404, { type: 'error', error: { type: 'not_found_error', message: `not mocked: ${p}` } });
  });
});

server.on('connect', (req, sock) => {
  log({ kind: 'connect', target: req.url, verdict: 'refused' });
  sock.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n');
});

server.listen(port, '127.0.0.1', () => {
  log({ kind: 'listening', port: server.address().port });
});
