// A scripted OpenAI Responses server on loopback, for the S15 PR5 harness's codex row.
//
// Copied from branch exp-mxc-spike, spikes/mxc-spike/proxy-mock.mjs (its model-server part, the
// OpenAI Responses half), with the CONNECT proxy, the absolute-form forwarding and the Anthropic
// half left out: this copy proxies nothing. Requests in absolute form get 403.
//
//   node mock-responses.mjs port=N matter=DIR log=FILE [probe=EXE]
//
// The first request that offers a shell tool gets one shell call that writes
// s15-codex.txt (holding s15-codex-ok) in DIR, and with probe=EXE (a path with no space) also runs
// `EXE probe job-ui` into s15-codex-job.txt; once a tool's output comes back, the turn ends.
// It keeps no state: it reads the history in each request. Each request is one JSON line in FILE;
// `outputs` holds the tool results the request carried. The first line is `listening`.
import fs from 'node:fs';
import http from 'node:http';

const opts = Object.fromEntries(process.argv.slice(2).map((a) => {
  const i = a.indexOf('=');
  return [a.slice(0, i), a.slice(i + 1)];
}));
const port = Number(opts.port || 0);
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

function responses(body, res) {
  const tools = body.tools || [];
  const names = tools.map((t) => t.name || t.type);
  const input = Array.isArray(body.input) ? body.input : [];
  const outputs = input.filter((i) => /_output$/.test(i.type || ''));
  const job = opts.probe ? ` & ${opts.probe} probe job-ui> s15-codex-job.txt` : '';
  const inner = `echo s15-codex-ok> s15-codex.txt${job}`;
  const argv = ['cmd.exe', '/d', '/c', inner];
  const cmd = `cmd.exe /d /c "${inner}"`;
  let item;
  if (outputs.length === 0) {
    if (names.includes('shell_command')) {
      item = { type: 'function_call', call_id: 'call_mock_1', name: 'shell_command', arguments: JSON.stringify({ command: cmd, workdir: matter }) };
    } else if (names.includes('shell')) {
      item = { type: 'function_call', call_id: 'call_mock_1', name: 'shell', arguments: JSON.stringify({ command: argv, workdir: matter }) };
    } else if (names.includes('exec_command')) {
      item = { type: 'function_call', call_id: 'call_mock_1', name: 'exec_command', arguments: JSON.stringify({ cmd, workdir: matter }) };
    } else if (names.includes('local_shell')) {
      item = { type: 'local_shell_call', call_id: 'call_mock_1', status: 'completed', action: { type: 'exec', command: argv, working_directory: matter } };
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

const server = http.createServer((req, res) => {
  const absolute = /^https?:\/\//i.test(req.url);
  const url = new URL(req.url, `http://127.0.0.1:${server.address().port}`);
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    log({ kind: 'http', method: req.method, url: req.url, verdict: absolute ? 'refused' : 'mock' });
    if (absolute) return json(res, 403, { error: 'this mock proxies nothing' });
    let body = {};
    try { body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {}; } catch { body = {}; }
    const p = url.pathname.replace(/\/+$/, '');
    if (p.endsWith('/responses')) return responses(body, res);
    if (p.endsWith('/models')) return json(res, 200, { object: 'list', data: [{ id: 'gpt-mock', object: 'model' }] });
    return json(res, 404, { error: 'not mocked', path: p });
  });
});

// CONNECT is refused outright: no tunnel.
server.on('connect', (req, sock) => {
  log({ kind: 'connect', target: req.url, verdict: 'refused' });
  sock.end('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n');
});

server.listen(port, '127.0.0.1', () => {
  log({ kind: 'listening', port: server.address().port, matter });
});
