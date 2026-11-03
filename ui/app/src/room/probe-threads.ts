// The threads the probe build's host lists for its matter, for the room's next_query (fake-next.ts's `threads`): with
// ?thread=permission the turn's thread, waiting on the lawyer, so the room subscribes to it and shows its prompts; with
// ?thread=commands the idle thread, so the room shows its commands; with ?thread=shaping the same idle thread, so the
// room shows the shaping answer; otherwise the page's own list (the ?word= pages').
// probe-main.tsx answers with this; probe-threads.test.tsx checks it. Not shipped: only the probe build imports it.
import { MATTER, THREAD } from "../testing/stream-script.ts";
import type { ThreadInfo } from "./host-next.ts";

/** The page's flags: `asking` for ?thread=permission, `commanding` for ?thread=commands, `shaping` for
 * ?thread=shaping. */
export function probeThreads(
  {
    asking,
    commanding,
    shaping = false,
  }: { asking: boolean; commanding: boolean; shaping?: boolean },
  fallback: readonly ThreadInfo[],
): readonly ThreadInfo[] {
  if (commanding || shaping)
    return [{ thread: THREAD, matter: MATTER, agent: "claude-acp", status: "idle", seen: true }];
  if (asking)
    return [
      { thread: THREAD, matter: MATTER, agent: "claude-acp", status: "needs_you", seen: false },
    ];
  return fallback;
}
