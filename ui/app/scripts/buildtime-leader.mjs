// The leader buildtime-proc.mjs puts between itself and each tool on POSIX, so the tool dies with
// buildtime.mjs even when buildtime.mjs gets SIGKILL and none of its handlers run. Why it exists
// is in the module doc of buildtime-proc.mjs.
//
//   node buildtime-leader.mjs <the tool's node argv...>
//
// Started detached, with an IPC channel to buildtime-proc.mjs and the tool's cwd and env. It starts
// `process.execPath <argv...>` in a process group of its own, with the leader's stdout and stderr
// (buildtime-proc.mjs's pipes), and sends `{ toolPid }` over the channel. Then:
// - the channel closes (buildtime-proc.mjs died, by SIGKILL too, or let go of it): SIGKILL to the
//   tool's group, and the leader exits;
// - SIGTERM: passed on to the tool's group, for a stop that comes before `{ toolPid }` arrives;
// - the tool exits: the leader exits with its code, or with 128 + the signal's number when a
//   signal ended it, as a shell does. Whatever is left of the tool's group is buildtime-proc.mjs's
//   to stop.

import { spawn } from "node:child_process";
import { constants } from "node:os";

const tool = spawn(process.execPath, process.argv.slice(2), {
  stdio: ["ignore", "inherit", "inherit"],
  detached: true,
});

function signalTool(signal) {
  try {
    process.kill(-tool.pid, signal);
  } catch {
    // The group is already gone.
  }
}

tool.on("error", (error) => {
  process.stderr.write(`buildtime-leader.mjs: could not start the tool: ${error.message}\n`);
  process.exit(127);
});
tool.on("exit", (code, signal) => {
  process.exit(code ?? 128 + (constants.signals[signal] ?? 0));
});
if (tool.pid !== undefined) {
  process.on("disconnect", () => {
    signalTool("SIGKILL");
    process.exit(1);
  });
  process.on("SIGTERM", () => signalTool("SIGTERM"));
  process.send?.({ toolPid: tool.pid });
}
