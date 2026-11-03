// The usage limit's words that the thread's stop and the composer share (U12): the plan's name, the window's line
// under the stop's title, the toolbar's at-limit words, and the composer's paused line. Each takes `now` as an input:
// a render formats against the day it renders on, and nothing here runs a clock.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { AgentName } from "@clauseo/components/lib/words-errors.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import type { StreamView } from "../../store/view.ts";
import type { LimitStopEntry, Timestamp } from "../../transport/protocol.ts";
import { whenOf } from "./clock.ts";
import { resetPassed } from "./stops.ts";

/** The plan's name the limit's words take: ChatGPT for Codex, Claude for Claude or an agent not known, else the
 * agent's own name. */
export function planNameOf(agent: AgentName): string {
  if (agent === undefined || agent.kind === "claude") return "Claude";
  if (agent.kind === "codex") return "ChatGPT";
  const name = agent.name?.trim() ?? "";
  return name === "" ? "Claude" : name;
}

/** The stop's time: the `TurnStoppedAtLimit` envelope's ts (H17, reducer commit 9377bb80); null where an entry has
 * none, as one from before it may not. */
export const stopTimeOf = (entry: LimitStopEntry): Timestamp | null =>
  typeof entry.ts === "number" && Number.isFinite(entry.ts) ? entry.ts : null;

/** The window's line under a plan stop's title, "The 5-hour limit. It resets at 6:10 pm.", for the two windows it
 * names (Claude's `five_hour` and `seven_day`); null for another window, or no reset, or one gone by. */
export function windowLine(
  lang: Lang,
  window: string | undefined,
  resets: Timestamp | undefined,
  now: number,
): string | null {
  if (resets === undefined || resetPassed(resets, now)) return null;
  const { day, time } = whenOf(lang, resets, now);
  if (window === "five_hour")
    return day === null
      ? lim(lang, "The 5-hour limit. It resets at {time}.", { time })
      : lim(lang, "The 5-hour limit. It resets on {day} at {time}.", { day, time });
  if (window === "seven_day")
    return day === null
      ? lim(lang, "The weekly limit. It resets at {time}.", { time })
      : lim(lang, "The weekly limit. It resets on {day} at {time}.", { day, time });
  return null;
}

/** The toolbar's at-limit words: "Limit reached until 6:10 pm", its day form, or that the reset wasn't given. */
export function atLimitWords(lang: Lang, resets: Timestamp | undefined, now: number): string {
  if (resets === undefined) return lim(lang, "Limit reached. Reset not given.");
  const { day, time } = whenOf(lang, resets, now);
  return day === null
    ? lim(lang, "Limit reached until {time}", { time })
    : lim(lang, "Limit reached until {day} {time}", { day, time });
}

/** The open thread paused at the limit: the reset the header gives, the stop's time where known, and the moment the
 * pause was read, as the day its words format against (a new view of the header reads it again). */
export type Paused = { resets: Timestamp | undefined; stopped: Timestamp | null; now: number };

/** The composer's line while paused: "Stopped at 4:52 pm, at the plan's limit" with the stop's time, the toolbar's
 * at-limit words without it. */
export function pausedLine(lang: Lang, paused: Paused): string {
  const { now } = paused;
  if (paused.stopped === null) return atLimitWords(lang, paused.resets, now);
  const { day, time } = whenOf(lang, paused.stopped, now);
  return day === null
    ? lim(lang, "Stopped at {time}, at the plan's limit", { time })
    : lim(lang, "Stopped on {day} at {time}, at the plan's limit", { day, time });
}

/** The thread's stop for its pause: the last limit_stop, looked for back to the latest question, which a newer turn
 * would follow. */
function lastStop(view: StreamView): LimitStopEntry | undefined {
  for (let i = view.ids.length - 1; i >= 0; i -= 1) {
    const id = view.ids[i]!;
    if (id.startsWith("limit_stop:")) return view.entries.get(id)?.limit_stop;
    if (id.startsWith("user_message:")) return undefined;
  }
  return undefined;
}

/** A pick for useStreamPart: the open thread's pause, or null; the same object while the header's status and the
 * stop stay the same, so a streaming flush renders nothing for it. The moment is read here, in the store's snapshot,
 * as a new pause arrives, never in a render. */
export function pausedPick(): (view: StreamView | undefined) => Paused | null {
  let last: { status: object; stop: LimitStopEntry | undefined; paused: Paused } | null = null;
  return (view) => {
    const status = view?.header?.header?.status;
    if (view === undefined || typeof status !== "object" || !("paused_at_limit" in status)) {
      last = null;
      return null;
    }
    const stop = lastStop(view);
    if (last !== null && last.status === status && last.stop === stop) return last.paused;
    const paused: Paused = {
      resets: status.paused_at_limit.resets_at,
      stopped: stop === undefined ? null : stopTimeOf(stop),
      now: Date.now(),
    };
    last = { status, stop, paused };
    return paused;
  };
}
