// Runs INSIDE the MXC container (node inside.js <mode> ...). Prints one line: RESULT <json>.
//   net <allowHost> <refusedHost>   DNS, direct TCP, HTTPS through the proxy, proxy env as seen
//   read <path>...                  try to read each file or list each folder
//   tree                            spawn PING.EXE -t and print TREE_READY (the host then kills)
'use strict';
const fs = require('fs');
const net = require('net');
const tls = require('tls');
const dns = require('dns');
const path = require('path');
const cp = require('child_process');

const [mode, ...args] = process.argv.slice(2);
const err = (e) => (e && (e.code || e.message)) || String(e);
const done = (obj) => { console.log('RESULT ' + JSON.stringify(obj)); };
const timeout = (p, ms, label) => Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error(`${label}: timeout ${ms} ms`)), ms))]);

function connectVia(proxyUrl, host) {
  return new Promise((resolve, reject) => {
    const u = new URL(proxyUrl);
    const s = net.connect({ host: u.hostname, port: Number(u.port || 80) });
    s.once('error', reject);
    s.once('connect', () => s.write(`CONNECT ${host}:443 HTTP/1.1\r\nHost: ${host}:443\r\n\r\n`));
    let buf = '';
    const onData = (d) => {
      buf += d.toString('latin1');
      const i = buf.indexOf('\r\n\r\n');
      if (i >= 0) { s.removeListener('data', onData); resolve({ socket: s, status: buf.split('\r\n')[0] }); }
    };
    s.on('data', onData);
  });
}

async function netMode(allowHost, refusedHost) {
  const r = {
    env: {
      HTTPS_PROXY: process.env.HTTPS_PROXY ?? null,
      HTTP_PROXY: process.env.HTTP_PROXY ?? null,
      https_proxy: process.env.https_proxy ?? null,
      NO_PROXY: process.env.NO_PROXY ?? null,
      ALL_PROXY: process.env.ALL_PROXY ?? null,
      CLAUSEO_TOKEN_PROBE: process.env.CLAUSEO_TOKEN_PROBE ?? null,
    },
  };
  // System resolver (getaddrinfo through the host's DNS Client service).
  r.dns_lookup = await timeout(dns.promises.lookup('example.com'), 10000, 'lookup').then((a) => `resolved ${a.address}`, (e) => `failed ${err(e)}`);
  const rand = `mxc-spike-${Date.now().toString(36)}.example.com`;
  r.dns_lookup_random = await timeout(dns.promises.lookup(rand), 10000, 'lookup').then((a) => `resolved ${a.address}`, (e) => `failed ${err(e)} (${rand})`);
  // c-ares straight to the configured DNS server over UDP 53.
  r.dns_resolve4_direct = await timeout(new dns.promises.Resolver({ timeout: 4000, tries: 1 }).resolve4('example.com'), 10000, 'resolve4').then((a) => `resolved ${a.join(',')}`, (e) => `failed ${err(e)}`);
  r.direct_tcp = await new Promise((resolve) => {
    const s = net.connect({ host: '1.1.1.1', port: 443 });
    const t = setTimeout(() => { s.destroy(); resolve('timeout'); }, 5000);
    s.on('connect', () => { clearTimeout(t); s.destroy(); resolve('CONNECTED'); });
    s.on('error', (e) => { clearTimeout(t); resolve(`error ${err(e)}`); });
  });
  const proxyUrl = process.env.HTTPS_PROXY || process.env.https_proxy;
  if (!proxyUrl) {
    r.proxy_https = 'no HTTPS_PROXY in env';
  } else {
    try {
      const { socket, status } = await timeout(connectVia(proxyUrl, allowHost), 15000, 'CONNECT');
      r.proxy_connect = status;
      r.proxy_https = await timeout(new Promise((resolve, reject) => {
        const t = tls.connect({ socket, servername: allowHost });
        let b = '';
        t.once('secureConnect', () => t.write(`GET / HTTP/1.1\r\nHost: ${allowHost}\r\nConnection: close\r\nUser-Agent: mxc-spike\r\n\r\n`));
        t.on('data', (d) => { b += d.toString('latin1'); if (b.includes('\r\n')) { resolve(b.split('\r\n')[0]); t.destroy(); } });
        t.once('error', reject);
      }), 15000, 'TLS GET');
    } catch (e) { r.proxy_https = `failed ${err(e)}`; }
    r.proxy_refused = await timeout(connectVia(proxyUrl, refusedHost), 15000, 'CONNECT').then((x) => { x.socket.destroy(); return x.status; }, (e) => `failed ${err(e)}`);
  }
  done(r);
}

function readMode(paths) {
  const r = {};
  for (const p of paths) {
    try {
      const st = fs.statSync(p);
      if (st.isDirectory()) {
        const names = fs.readdirSync(p);
        r[p] = `LISTED ${names.length}: ${names.slice(0, 8).join(', ')}`;
      } else {
        r[p] = `READ ${JSON.stringify(fs.readFileSync(p, 'utf8').slice(0, 40))}`;
      }
    } catch (e) { r[p] = `refused ${err(e)}`; }
  }
  done(r);
}

function treeMode() {
  const ping = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'PING.EXE');
  const c = cp.spawn(ping, ['-t', '127.0.0.1'], { stdio: 'ignore' });
  c.on('error', (e) => console.log(`TREE_ERROR ${err(e)}`));
  c.on('spawn', () => console.log(`TREE_READY node=${process.pid} ping=${c.pid}`));
  setInterval(() => {}, 1000);
}

(async () => {
  if (mode === 'net') return netMode(args[0], args[1]);
  if (mode === 'read') return readMode(args);
  if (mode === 'tree') return treeMode();
  done({ error: `unknown mode ${mode}` });
})().catch((e) => done({ error: err(e) }));
