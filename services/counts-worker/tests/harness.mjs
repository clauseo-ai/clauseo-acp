// The Worker tests' fixtures and harness: the crate's own example day and tally
// (crates/counts/src/schema/tests/strict.rs) dated from today's UTC clock, the Worker under
// `wrangler dev --local` with its own local D1, and a reader for that D1.

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { gzipSync } from "node:zlib";
import { getPlatformProxy } from "wrangler";

export const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const DAY_MS = 86_400_000;

/** wrangler's environment: no metrics, and its logs and dev registry kept in .wrangler/ here. */
const ENV = {
  ...process.env,
  CI: "true",
  WRANGLER_SEND_METRICS: "false",
  XDG_CONFIG_HOME: join(root, ".wrangler", "config"),
};

/** The UTC day `daysAgo` days before today, as YYYY-MM-DD. */
export function day(daysAgo) {
  return new Date(Date.now() - daysAgo * DAY_MS).toISOString().slice(0, 10);
}

/** The ISO week `weeksAgo` weeks before this one, as YYYY-Www. */
export function week(weeksAgo) {
  return weekOf(new Date(Date.now() - weeksAgo * 7 * DAY_MS));
}

/** How many days ago the Monday of the week `daysAgo` days back was. */
export function mondayAgo(daysAgo) {
  return daysAgo + ((new Date(Date.now() - daysAgo * DAY_MS).getUTCDay() + 6) % 7);
}

/** The ISO week of a date (a Date, or YYYY-MM-DD), as YYYY-Www. */
export function weekOf(date) {
  if (typeof date === "string") date = new Date(`${date}T00:00:00Z`);
  const thursday = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));
  thursday.setUTCDate(thursday.getUTCDate() + 3 - ((thursday.getUTCDay() + 6) % 7));
  const year = thursday.getUTCFullYear();
  const number = Math.floor((thursday - Date.UTC(year, 0, 1)) / (7 * DAY_MS)) + 1;
  return `${year}-W${String(number).padStart(2, "0")}`;
}

export const INSTALL = "8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40";

/** A fresh lowercase UUID v4. */
export function uuid() {
  return crypto.randomUUID();
}

/** strict.rs's `day_json()`, sent yesterday unless `daysAgo` says otherwise. */
export function dayJson({ daysAgo = 1, install = INSTALL } = {}) {
  return {
    schema: 1,
    install,
    day: day(daysAgo),
    version: "3.0.4",
    channel: "stable",
    os: "macos",
    os_major: 15,
    arch: "aarch64",
    language: "en",
    cells: {
      app_opened: [{ launch: "cold", n: 2 }],
      time_open: [{ bucket: "lt_3h", n: 1 }],
      question_sent: [{ agent: "claude", delivery: "now", n: 14 }],
      matter_opened: [{ kind: "folder", papers: "11_50", protection: "not_protected", n: 1 }],
      setting_changed: [
        { setting: "text_size", value: 18, n: 1 },
        { setting: "theme", value: "dark", n: 1 },
      ],
      error_shown: [{ code: "agent.exited", n: 1 }],
    },
  };
}

/** strict.rs's tally, for last week unless `weeksAgo` says otherwise. */
export function tallyJson({ weeksAgo = 1, tally = uuid() } = {}) {
  return {
    schema: 1,
    tally,
    week: week(weeksAgo),
    version: "3.0.4",
    channel: "stable",
    os: "windows",
    os_major: 11,
    arch: "x86_64",
    install_type: "per_machine",
    managed: true,
    cells: {
      crash: [{ process: "webview", kind: "renderer_gone", after_launch: false, n: 1 }],
      active_week: [{ n: 1 }],
    },
  };
}

/** crates/counts' example panic report (schema/tests/crash.rs), found `weeksAgo` weeks ago. */
export function crashJson({ weeksAgo = 0, report = uuid() } = {}) {
  return {
    schema: 1,
    report,
    week: week(weeksAgo),
    version: "3.0.4",
    channel: "stable",
    os: "windows",
    os_major: 11,
    arch: "x86_64",
    process: "host",
    kind: "panic",
    area: "store",
    after_launch: true,
    location: { file: "crates/store/src/writer.rs", line: 212 },
  };
}

function wrangler(args, persist, config) {
  const run = spawnSync("pnpm", ["exec", "wrangler", ...args, "--persist-to", persist, "--config", config], {
    cwd: root,
    encoding: "utf8",
    env: ENV,
  });
  if (run.status !== 0) throw new Error(`wrangler ${args.join(" ")} failed:\n${run.stdout}\n${run.stderr}`);
  return run.stdout;
}

async function freePort() {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

/**
 * The Worker under `wrangler dev --local --test-scheduled`, with a fresh local D1 that has the
 * migrations applied, from `config` (a wrangler config in this folder), and `vars` passed as
 * `--var` (the Access settings, for the read routes). `post`, `send`, `sql`, `object` (an R2
 * object's text) and `rowsRead` talk to it; `stop` ends it and deletes its D1 and R2.
 */
export async function startWorker({ config = "wrangler.toml", vars = {} } = {}) {
  mkdirSync(join(root, ".wrangler"), { recursive: true });
  const persist = mkdtempSync(join(root, ".wrangler", "test-"));
  wrangler(["d1", "migrations", "apply", "clauseo-counts", "--local"], persist, config);
  const port = await freePort();
  const dev = spawn(
    "pnpm",
    [
      "exec", "wrangler", "dev", "--local", "--test-scheduled", "--ip", "127.0.0.1", "--port", String(port), "--persist-to", persist, "--config", config,
      ...Object.entries(vars).flatMap(([name, value]) => ["--var", `${name}:${value}`]),
    ],
    { cwd: root, env: ENV, stdio: ["ignore", "pipe", "pipe"] },
  );
  let output = "";
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`wrangler dev did not start:\n${output}`)), 600_000);
    const read = (chunk) => {
      output += chunk;
      if (/Ready on/.test(output)) {
        clearTimeout(timer);
        resolve();
      }
    };
    dev.stdout.on("data", read);
    dev.stderr.on("data", read);
    dev.on("exit", (code) => reject(new Error(`wrangler dev exited ${code}:\n${output}`)));
  });
  const base = `http://127.0.0.1:${port}`;

  /**
   * Any request, as given, on a connection of its own. fetch keeps an idle socket for 4 s; a
   * request sent on one after 3-4 s, with the event loop held by `sql`'s spawnSync, failed in CI
   * with a bare "fetch failed" and no line from wrangler dev (rollup.test.mjs, 2026-11-16). No
   * retry: a fresh connection each time, so a failure is the Worker's or the server's.
   */
  const send = (path, init = {}) => {
    const headers = new Headers(init.headers);
    headers.set("connection", "close");
    return fetch(base + path, { ...init, headers });
  };

  /** A JSON value (or a string, sent as it is) gzipped with Content-Encoding: gzip. */
  const post = (path, body, { gzip = true, headers = {} } = {}) => {
    const text = typeof body === "string" ? body : JSON.stringify(body);
    return send(path, {
      method: "POST",
      body: gzip ? gzipSync(text) : text,
      headers: gzip ? { "content-encoding": "gzip", ...headers } : headers,
    });
  };

  /** The rows of one SQL statement on the local D1. */
  const sql = (statement) => JSON.parse(wrangler(["d1", "execute", "clauseo-counts", "--local", "--json", "--command", statement], persist, config))[0].results;

  // The R2 binding through wrangler's platform proxy over the same local files.
  let proxy;
  /** The text of one object in the local crash bucket, or null when there is none. */
  const object = async (key) => {
    proxy ??= await getPlatformProxy({ configPath: join(root, config), persist: { path: join(persist, "v3") } });
    const found = await proxy.env.CRASH.get(key);
    return found === null ? null : await found.text();
  };

  // `wrangler d1 execute --local` reports no rows_read; the D1 binding (the same proxy) does.
  /** D1's rows_read for one statement on the local D1, through the binding. */
  const rowsRead = async (statement) => {
    proxy ??= await getPlatformProxy({ configPath: join(root, config), persist: { path: join(persist, "v3") } });
    return (await proxy.env.DB.prepare(statement).all()).meta.rows_read;
  };

  const stop = async () => {
    await proxy?.dispose();
    dev.kill("SIGTERM");
    await new Promise((resolve) => (dev.exitCode === null ? dev.on("exit", resolve) : resolve()));
    rmSync(persist, { recursive: true, force: true });
  };
  /** The end of wrangler dev's output, for a failure message (it holds no id). */
  const tail = () => output.slice(-4_000);
  return { send, post, sql, object, rowsRead, stop, tail };
}
