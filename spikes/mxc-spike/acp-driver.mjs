// ACP client for the MXC spike: runs on the host, starts an ACP adapter inside the container
// through wxc-exec, and drives one turn: initialize, session/new, session/prompt. Permission
// requests are answered with the first allow option. Prints one line: RESULT <json>.
//   node acp-driver.mjs <wxc-exec.exe> <policy.json> <matter> [timeoutMs]
import { spawn } from 'node:child_process';
import readline from 'node:readline';

const [wxc, policy, matter, timeoutArg] = process.argv.slice(2);
const timeoutMs = Number(timeoutArg || 180000);
const t0 = Date.now();
const r = { steps: [], non_json_stdout: 0, notifications: {}, tool_calls: [], permissions: 0, stderr_tail: [] };

const child = spawn(wxc, [policy], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
let nextId = 1;
const pending = new Map();

function send(msg) { child.stdin.write(JSON.stringify(msg) + '\n'); }
function request(method, params) {
  const id = nextId++;
  send({ jsonrpc: '2.0', id, method, params });
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject, method }));
}

readline.createInterface({ input: child.stdout }).on('line', (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { r.non_json_stdout++; return; }
  if (msg.id !== undefined && !msg.method && pending.has(msg.id)) {
    const p = pending.get(msg.id);
    pending.delete(msg.id);
    if (msg.error) p.reject(new Error(`${p.method}: ${JSON.stringify(msg.error).slice(0, 300)}`));
    else p.resolve(msg.result);
    return;
  }
  if (msg.method === 'session/request_permission') {
    r.permissions++;
    const opts = msg.params?.options || [];
    const pick = opts.find((o) => o.kind === 'allow_always') || opts.find((o) => o.kind === 'allow_once') || opts[0];
    send({ jsonrpc: '2.0', id: msg.id, result: { outcome: pick ? { outcome: 'selected', optionId: pick.optionId } : { outcome: 'cancelled' } } });
    return;
  }
  if (msg.method === 'session/update') {
    const u = msg.params?.update || {};
    const kind = u.sessionUpdate || 'unknown';
    r.notifications[kind] = (r.notifications[kind] || 0) + 1;
    if (kind === 'tool_call' || kind === 'tool_call_update') r.tool_calls.push(`${u.title || u.toolCallId}:${u.status || ''}`);
    return;
  }
  if (msg.id !== undefined && msg.method) {
    // A request we don't serve (fs/*, terminal/*): refuse it.
    send({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'not supported by the spike driver' } });
  }
});
readline.createInterface({ input: child.stderr }).on('line', (line) => {
  r.stderr_tail.push(line.slice(0, 300));
  if (r.stderr_tail.length > 15) r.stderr_tail.shift();
});

const timer = setTimeout(() => finish('timeout'), timeoutMs);
let finished = false;
function finish(outcome) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  r.outcome = outcome;
  r.ms = Date.now() - t0;
  try { child.kill(); } catch {}
  console.log('RESULT ' + JSON.stringify(r));
  setTimeout(() => process.exit(0), 500);
}
child.on('exit', (code) => { r.exit = code; setTimeout(() => finish(`adapter exited ${code}`), 1000); });

(async () => {
  const init = await request('initialize', { protocolVersion: 1, clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false } });
  r.steps.push(`initialize ok ${Date.now() - t0} ms (protocol ${init?.protocolVersion}, auth methods ${(init?.authMethods || []).map((m) => m.id).join('|') || 'none'})`);
  const session = await request('session/new', { cwd: matter, mcpServers: [] });
  r.steps.push(`session/new ok ${Date.now() - t0} ms`);
  const prompt = await request('session/prompt', { sessionId: session.sessionId, prompt: [{ type: 'text', text: 'Run the spike test.' }] });
  r.steps.push(`session/prompt ${prompt?.stopReason} ${Date.now() - t0} ms`);
  r.stop_reason = prompt?.stopReason;
  finish('done');
})().catch((e) => { r.error = e.message; finish('error'); });
