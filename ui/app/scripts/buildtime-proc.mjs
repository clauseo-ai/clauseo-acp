// The children of buildtime.mjs (the tools and Vite): how each starts, runs and stops, kept apart
// so buildtime-proc.test.mjs can test the stop on a real process tree.
//
// A tool can start processes of its own. `tsc` is a node wrapper that runs the native binary with
// `execFileSync` and `stdio: "inherit"`, so a signal to the wrapper alone leaves the binary running
// and holding the pipes. So on POSIX every child starts in its own process group (`detached`) and
// every signal goes to the group: SIGTERM, then SIGKILL after KILL_GRACE_MS while
// `process.kill(-pid, 0)` still finds a member. On Windows, `taskkill /pid <pid> /T /F` takes the
// tree. A child in its own group gets no Ctrl-C from the terminal, so buildtime.mjs stops every
// live child from its SIGINT, SIGTERM and SIGHUP handlers (`stopAll`), and `process.on("exit")`
// sends SIGKILL to any group still alive as a last resort.

import { spawn } from "node:child_process";

/** How long a child's group has between SIGTERM and SIGKILL. */
export const KILL_GRACE_MS = 5_000;
/** How long a run waits after `exit` for stdout and stderr to drain before it drops them. */
export const DRAIN_MS = 1_000;
/** How often a stop checks whether the group is gone. */
const POLL_MS = 50;

const WINDOWS = process.platform === "win32";

/** Every child started here and not yet stopped. */
const live = new Set();
/** Set by `stopAll`: nothing starts after it. */
let closed = false;

function note(text) {
  process.stderr.write(`buildtime.mjs: ${text}\n`);
}

/** Whether any process of the group `pid` is still alive. */
export function groupAlive(pid) {
  try {
    process.kill(-pid, 0);
    return true;
  } catch (error) {
    // EPERM: a member exists but belongs to someone else; ESRCH: the group is gone.
    return error.code === "EPERM";
  }
}

function signalGroup(pid, signal) {
  try {
    process.kill(-pid, signal);
  } catch {
    // The group is already gone.
  }
}

function exited(child) {
  return child.exitCode !== null || child.signalCode !== null;
}

/** Resolves once `test()` is true, or with false after `ms`. */
function poll(test, ms) {
  return new Promise((done) => {
    const until = performance.now() + ms;
    const tick = () => {
      if (test()) {
        done(true);
      } else if (performance.now() >= until) {
        done(false);
      } else {
        setTimeout(tick, POLL_MS);
      }
    };
    tick();
  });
}

/** Starts `node ...argv` in `cwd` with piped output, in its own process group on POSIX. */
export function start(argv, { cwd, env = process.env }) {
  if (closed) {
    throw new Error("buildtime.mjs is stopping; started nothing");
  }
  const child = spawn(process.execPath, argv, {
    cwd,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    detached: !WINDOWS,
  });
  if (child.pid !== undefined) {
    live.add(child);
  }
  return child;
}

async function stopPosix(child) {
  if (!groupAlive(child.pid)) {
    return true;
  }
  signalGroup(child.pid, "SIGTERM");
  if (await poll(() => !groupAlive(child.pid), KILL_GRACE_MS)) {
    return true;
  }
  note(`process group ${child.pid} outlived SIGTERM by ${KILL_GRACE_MS} ms; sent SIGKILL`);
  signalGroup(child.pid, "SIGKILL");
  return poll(() => !groupAlive(child.pid), DRAIN_MS);
}

async function stopWindows(child) {
  if (exited(child)) {
    return true;
  }
  const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  killer.on("error", (error) => {
    note(`could not run taskkill (${error.message}); stopping pid ${child.pid} alone`);
    child.kill();
  });
  return poll(() => exited(child), KILL_GRACE_MS);
}

/**
 * Stops `child` and everything it started: its process group on POSIX, its tree on Windows.
 * Resolves with whether they are gone. Safe to repeat and to call on a child that has exited.
 */
export function stop(child) {
  if (child?.pid === undefined) {
    return Promise.resolve(true);
  }
  child.stopping ??= (WINDOWS ? stopWindows(child) : stopPosix(child)).then((gone) => {
    live.delete(child);
    return gone;
  });
  return child.stopping;
}

/** Stops every live child, and anything started after it throws. Resolves once each stop has. */
export function stopAll() {
  closed = true;
  return Promise.all([...live].map((child) => stop(child)));
}

process.on("exit", () => {
  if (!WINDOWS) {
    for (const child of live) {
      signalGroup(child.pid, "SIGKILL");
    }
  }
});

/**
 * Runs `node ...argv` in `cwd`. Resolves with its exit code, wall time and output, and `timedOut`
 * when it ran past `timeoutMs` and was stopped. The run ends at the child's `exit`, plus at most
 * DRAIN_MS for its output to drain, so a grandchild holding the pipes cannot hold the run; then
 * whatever is left of its group is stopped.
 */
export function runTool(argv, { cwd, timeoutMs }) {
  return new Promise((done) => {
    const started = performance.now();
    const child = start(argv, { cwd });
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      void stop(child);
    }, timeoutMs);
    const chunks = [];
    child.stdout.on("data", (chunk) => chunks.push(chunk));
    child.stderr.on("data", (chunk) => chunks.push(chunk));
    let settled = false;
    const finish = async (code, ms, output) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      await stop(child);
      done({ code, ms, output, timedOut, pid: child.pid });
    };
    child.on("error", (error) => {
      void finish(-1, performance.now() - started, String(error));
    });
    child.on("exit", (code) => {
      const ms = performance.now() - started;
      let drain;
      const drained = () => {
        clearTimeout(drain);
        child.stdout.destroy();
        child.stderr.destroy();
        void finish(code, ms, Buffer.concat(chunks).toString("utf8"));
      };
      child.once("close", drained);
      drain = setTimeout(drained, DRAIN_MS);
    });
  });
}
