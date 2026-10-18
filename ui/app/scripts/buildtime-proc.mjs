// The children of buildtime.mjs (the tools and Vite): how each starts, runs and stops, kept apart
// so buildtime-proc.test.mjs can test the stop on a real process tree.
//
// A tool can start processes of its own. `tsc` is a node wrapper that runs the native binary with
// `execFileSync` and `stdio: "inherit"`, so a signal to the wrapper alone leaves the binary running
// and holding the pipes. So on POSIX every tool runs in its own process group and every signal goes
// to the group: SIGTERM, then SIGKILL after KILL_GRACE_MS while `process.kill(-pid, 0)` still finds
// a member.
//
// A tool in its own group is out of reach of a signal to buildtime.mjs's group. It gets no Ctrl-C
// from the terminal, so buildtime.mjs stops every live child from its SIGINT, SIGTERM and SIGHUP
// handlers (`stopAll`), and `process.on("exit")` sends SIGKILL to any group still alive as a last
// resort. But SIGKILL runs no handler, and SIGKILL to the whole group is what agent harnesses, CI
// timeouts and `timeout -s KILL` send to `cargo xtask buildtime web`. So each tool has a leader,
// buildtime-leader.mjs. Started detached (out of the group the SIGKILL hits) with an IPC channel,
// it starts the tool in a group of its own, and when the channel closes, which it does however
// buildtime.mjs dies, it sends SIGKILL to the tool's group. It exits when the tool does, with the
// tool's code, so the child here is the leader: its pipes carry the tool's output and its `exit` is
// the tool's. The leader reports the tool's pid (`toolPid`), and a stop signals the tool's group.
//
// On Windows there is no leader and no group: `taskkill /pid <pid> /T /F` takes the tree, and a
// terminated xtask leaves the children running.

import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

/** How long a child's group has between SIGTERM and SIGKILL. */
export const KILL_GRACE_MS = 5_000;
/** How long a run waits after `exit` for stdout and stderr to drain before it drops them. */
export const DRAIN_MS = 1_000;
/** How often a stop checks whether the group is gone. */
const POLL_MS = 50;

const WINDOWS = process.platform === "win32";
const LEADER = fileURLToPath(new URL("buildtime-leader.mjs", import.meta.url));

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

/**
 * Starts `node ...argv` in `cwd` with piped output. On POSIX it runs under its leader, in its own
 * process group, and the returned child is the leader. `child.startedAt` is the `performance.now()`
 * at which the tool started, as near as this process knows it: when the leader reports the tool's
 * pid, so the leader's own start stays out of the timings.
 */
export function start(argv, { cwd, env = process.env }) {
  if (closed) {
    throw new Error("buildtime.mjs is stopping; started nothing");
  }
  const startedAt = performance.now();
  const child = WINDOWS
    ? spawn(process.execPath, argv, { cwd, env, stdio: ["ignore", "pipe", "pipe"] })
    : spawn(process.execPath, [LEADER, ...argv], {
        cwd,
        env,
        stdio: ["ignore", "pipe", "pipe", "ipc"],
        detached: true,
      });
  child.startedAt = startedAt;
  child.on("message", (message) => {
    if (typeof message?.toolPid === "number") {
      child.toolPid = message.toolPid;
      child.startedAt = performance.now();
    }
  });
  if (child.pid !== undefined) {
    live.add(child);
  }
  return child;
}

/** The process groups of `child` on POSIX: the leader's, and the tool's once the leader sent it. */
function groups(child) {
  return child.toolPid === undefined ? [child.pid] : [child.pid, child.toolPid];
}

function alive(child) {
  return groups(child).some((pid) => groupAlive(pid));
}

/**
 * SIGTERM to the tool's group, or to the leader, which passes it on, while the tool's pid is not
 * known yet. Then SIGKILL to both groups; with the tool's pid still unknown, the leader is let go
 * of instead, and it sends SIGKILL to the tool's group itself as its channel closes.
 */
async function stopPosix(child) {
  if (!alive(child)) {
    return true;
  }
  const target = child.toolPid ?? child.pid;
  signalGroup(target, "SIGTERM");
  if (await poll(() => !alive(child), KILL_GRACE_MS)) {
    return true;
  }
  note(`process group ${target} outlived SIGTERM by ${KILL_GRACE_MS} ms; sent SIGKILL`);
  if (child.toolPid === undefined) {
    if (child.connected) {
      child.disconnect();
    }
  } else {
    signalGroup(child.toolPid, "SIGKILL");
    signalGroup(child.pid, "SIGKILL");
  }
  return poll(() => !alive(child), DRAIN_MS);
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
 * Stops `child` and everything it started: its process groups on POSIX, its tree on Windows.
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

// The leaders also see their channels close as this process exits, and kill their tools' groups.
process.on("exit", () => {
  if (!WINDOWS) {
    for (const child of live) {
      for (const pid of groups(child)) {
        signalGroup(pid, "SIGKILL");
      }
    }
  }
});

/**
 * Runs `node ...argv` in `cwd`. Resolves with its exit code, wall time and output, and `timedOut`
 * when it ran past `timeoutMs` and was stopped. The run ends at the child's `exit`, plus at most
 * DRAIN_MS for its output to drain, so a grandchild holding the pipes cannot hold the run; then
 * whatever is left of its group is stopped. `pid` is the child's (the leader's on POSIX) and
 * `toolPid` the tool's, which is also its process group.
 */
export function runTool(argv, { cwd, timeoutMs }) {
  return new Promise((done) => {
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
      done({ code, ms, output, timedOut, pid: child.pid, toolPid: child.toolPid });
    };
    child.on("error", (error) => {
      void finish(-1, performance.now() - child.startedAt, String(error));
    });
    child.on("exit", (code) => {
      const ms = performance.now() - child.startedAt;
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
