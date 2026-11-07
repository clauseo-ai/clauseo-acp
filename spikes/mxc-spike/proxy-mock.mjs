// Loopback CONNECT proxy plus a scripted model server, on one port, for the MXC spike's rows.
// Runs on the host, outside the container: node proxy-mock.mjs port=N allow=example.com matter=DIR log=FILE
//
// - CONNECT host:443 for an allowed host is tunnelled; anything else gets 403. Each decision is
//   logged with whether a Proxy-Authorization header came with it (MXC strips HTTPS_PROXY's
//   userinfo, so Clauseo's token should never arrive).
// - Any other request, in origin form or absolute form for this port, is answered by the mock:
//   Anthropic Messages (/v1/messages, streaming or not) and OpenAI Responses (/v1/responses).
//   The mock asks for one Bash call then one Write call (Claude Code), or one shell call (Codex),
//   then ends the turn. It keeps no state: it reads the history in each request.
// - Absolute-form requests for other hosts get 403 (no plain-HTTP egress).
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import path from 'node:path';

const opts = Object.fromEntries(process.argv.slice(2).map((a) => {
  const i = a.indexOf('=');
  return [a.slice(0, i), a.slice(i + 1)];
}));
const port = Number(opts.port || 0);
const allow = (opts.allow || '').split(',').map((h) => h.trim().toLowerCase()).filter(Boolean);
const matter = opts.matter || process.cwd();
const logFile = opts.log;

function log(event) {
  const line = JSON.stringify({ t: new Date().toISOString(), ...event });
  console.log(line);
  if (logFile) fs.appendFileSync(logFile, line + '\n');
}

function sse(res, events) {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'close' });
  for (const [name, data] of events) res.write(`event: ${name}\ndata: ${JSON.stringify(data)}\n\n`);
  res.end();
}

function json(res, status, body) {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(text) });
  res.end(text);
}

function textOf(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) return content.map((c) => c.text ?? (typeof c.content === 'string' ? c.content : JSON.stringify(c.content ?? c))).join(' ');
  return JSON.stringify(content);
}

// ---------------------------------------------------------------- Anthropic Messages
function anthropic(body, res) {
  const tools = (body.tools || []).map((t) => t.name);
  const used = [];
  const results = [];
  for (const m of body.messages || []) {
    if (!Array.isArray(m.content)) continue;
    for (const c of m.content) {
      if (m.role === 'assistant' && c.type === 'tool_use') used.push(c.name);
      if (m.role === 'user' && c.type === 'tool_result') results.push({ id: c.tool_use_id, is_error: !!c.is_error, text: textOf(c.content).slice(0, 400) });
    }
  }
  let block;
  if (tools.includes('Bash') && !used.includes('Bash')) {
    block = { type: 'tool_use', id: 'toolu_mock_bash', name: 'Bash', input: { command: 'echo hi > claude-bash.txt && pwd', description: 'Write a test file' } };
  } else if (tools.includes('Write') && !used.includes('Write')) {
    block = { type: 'tool_use', id: 'toolu_mock_write', name: 'Write', input: { file_path: path.join(matter, 'claude-write.txt'), content: 'written by the Write tool\n' } };
  } else {
    block = { type: 'text', text: tools.length ? 'done' : 'ok' };
  }
  log({ kind: 'mock', api: 'anthropic', model: body.model, stream: !!body.stream, tools: tools.length, has_bash: tools.includes('Bash'), has_write: tools.includes('Write'), used, results, reply: block.type === 'tool_use' ? block.name : block.text });
  const stop = block.type === 'tool_use' ? 'tool_use' : 'end_turn';
  const usage = { input_tokens: 10, output_tokens: 5 };
  if (!body.stream) {
    return json(res, 200, { id: 'msg_mock', type: 'message', role: 'assistant', model: body.model || 'mock', content: [block], stop_reason: stop, stop_sequence: null, usage });
  }
  const start = block.type === 'tool_use' ? { ...block, input: {} } : { type: 'text', text: '' };
  const delta = block.type === 'tool_use'
    ? { type: 'input_json_delta', partial_json: JSON.stringify(block.input) }
    : { type: 'text_delta', text: block.text };
  sse(res, [
    ['message_start', { type: 'message_start', message: { id: 'msg_mock', type: 'message', role: 'assistant', model: body.model || 'mock', content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 1 } } }],
    ['content_block_start', { type: 'content_block_start', index: 0, content_block: start }],
    ['content_block_delta', { type: 'content_block_delta', index: 0, delta }],
    ['content_block_stop', { type: 'content_block_stop', index: 0 }],
    ['message_delta', { type: 'message_delta', delta: { stop_reason: stop, stop_sequence: null }, usage: { output_tokens: 5 } }],
    ['message_stop', { type: 'message_stop' }],
  ]);
}

// ---------------------------------------------------------------- OpenAI Responses
function responses(body, res) {
  const tools = body.tools || [];
  const names = tools.map((t) => t.name || t.type);
  const input = Array.isArray(body.input) ? body.input : [];
  const outputs = input.filter((i) => /_output$/.test(i.type || ''));
  const cmd = 'cmd.exe /d /c "echo hi> codex-shell.txt"';
  let item;
  if (outputs.length === 0) {
    if (names.includes('shell_command')) {
      item = { type: 'function_call', call_id: 'call_mock_1', name: 'shell_command', arguments: JSON.stringify({ command: cmd, workdir: matter }) };
    } else if (names.includes('shell')) {
      item = { type: 'function_call', call_id: 'call_mock_1', name: 'shell', arguments: JSON.stringify({ command: ['cmd.exe', '/d', '/c', 'echo hi> codex-shell.txt'], workdir: matter }) };
    } else if (names.includes('exec_command')) {
      item = { type: 'function_call', call_id: 'call_mock_1', name: 'exec_command', arguments: JSON.stringify({ cmd, workdir: matter }) };
    } else if (names.includes('local_shell')) {
      item = { type: 'local_shell_call', call_id: 'call_mock_1', status: 'completed', action: { type: 'exec', command: ['cmd.exe', '/d', '/c', 'echo hi> codex-shell.txt'], working_directory: matter } };
    }
  }
  if (!item) item = { type: 'message', role: 'assistant', id: 'msg_mock', content: [{ type: 'output_text', text: names.length ? 'done' : 'ok' }] };
  log({ kind: 'mock', api: 'responses', model: body.model, tools: names, outputs: outputs.map((o) => textOf(o.output ?? o).slice(0, 400)), reply: item.name || item.type });
  sse(res, [
    ['response.created', { type: 'response.created', response: { id: 'resp_mock' } }],
    ['response.output_item.done', { type: 'response.output_item.done', item }],
    ['response.completed', { type: 'response.completed', response: { id: 'resp_mock', usage: { input_tokens: 10, input_tokens_details: { cached_tokens: 0 }, output_tokens: 5, output_tokens_details: { reasoning_tokens: 0 }, total_tokens: 15 } } }],
  ]);
}

// ---------------------------------------------------------------- server
const server = http.createServer((req, res) => {
  const absolute = /^https?:\/\//i.test(req.url);
  const url = new URL(req.url, `http://127.0.0.1:${server.address().port}`);
  const local = !absolute || ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname);
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    log({ kind: 'http', method: req.method, url: req.url, form: absolute ? 'absolute' : 'origin', proxy_auth: req.headers['proxy-authorization'] ? 'present' : 'absent', verdict: local ? 'mock' : 'refused' });
    if (!local) return json(res, 403, { error: 'plain HTTP egress is refused' });
    let body = {};
    try { body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {}; } catch { body = {}; }
    const p = url.pathname.replace(/\/+$/, '');
    if (p.endsWith('/messages/count_tokens')) return json(res, 200, { input_tokens: 10 });
    if (p.endsWith('/messages')) return anthropic(body, res);
    if (p.endsWith('/responses')) return responses(body, res);
    if (p.endsWith('/models')) return json(res, 200, { object: 'list', data: [{ id: 'gpt-mock', object: 'model' }, { id: 'claude-mock', type: 'model' }] });
    return json(res, 404, { error: 'not mocked', path: p });
  });
});

server.on('connect', (req, sock, head) => {
  const target = req.url || '';
  const i = target.lastIndexOf(':');
  const host = target.slice(0, i).replace(/^\[|\]$/g, '').toLowerCase();
  const tport = Number(target.slice(i + 1));
  const ok = tport === 443 && allow.includes(host);
  const auth = req.headers['proxy-authorization'] ? 'present' : 'absent';
  sock.on('error', () => {});
  if (!ok) {
    log({ kind: 'connect', host, port: tport, proxy_auth: auth, verdict: 'refused' });
    sock.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n');
    return;
  }
  const up = net.connect(tport, host, () => {
    log({ kind: 'connect', host, port: tport, proxy_auth: auth, verdict: 'allowed' });
    sock.write('HTTP/1.1 200 Connection Established\r\n\r\n');
    if (head && head.length) up.write(head);
    up.pipe(sock);
    sock.pipe(up);
  });
  up.on('error', (e) => {
    log({ kind: 'connect', host, port: tport, proxy_auth: auth, verdict: 'upstream_error', error: e.code || e.message });
    sock.end('HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n');
  });
});

server.listen(port, '127.0.0.1', () => {
  log({ kind: 'listening', port: server.address().port, allow, matter });
});
