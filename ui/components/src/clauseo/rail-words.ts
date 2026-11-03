// What a matter and a conversation on the rail say they're doing, in the language. The mark that leads each status is
// rail-mark.tsx's.
import { clock } from "../lib/words.ts";
import { frame } from "../lib/words-frame.ts";
import { lim } from "../lib/words-limits.ts";
import { mt } from "../lib/words-matters.ts";
import type { Lang } from "../lib/display.ts";

/** What a matter is doing while the lawyer looks elsewhere. One at a time:
 * - working: Clauseo is answering: "Answering.", or with a clock time it expects to finish, where one is given;
 * - question, permission: Clauseo waits on the lawyer, so the row takes court yellow, as the prepared draft does;
 * - ready: the answer finished while the lawyer was away, a quiet mark until the matter is opened;
 * - stopped: the lawyer stopped it, said in concrete;
 * - paused: the plan's limit stopped it until the reset. While the lawyer hasn't chosen how it goes on, Continue at
 *   the reset or ask the other plan, it waits on the lawyer's hand and takes court yellow; pausedSet, once Continue
 *   is chosen, is concrete with its time;
 * - limit: the plan's limit reached, until the reset if the plan gave one, muted, with no choice asked for;
 * - moved: its folder isn't where it was, said in concrete, with Locate folder in the room and in its menu. */
export type MatterState =
  | "idle"
  | "working"
  | "question"
  | "permission"
  | "ready"
  | "stopped"
  | "paused"
  | "pausedSet"
  | "limit"
  | "moved";

/** What a conversation is doing while the lawyer reads another. waiting: behind another conversation's answer. */
export type ConversationState = "idle" | "working" | "question" | "ready" | "stopped" | "waiting";

/** A clock time that holds together on its line: "4:32 pm" never breaks before "pm". */
const held = (lang: Lang, at: string) => clock(lang, at).replace(/ (am|pm)$/, "\u00a0$1");

/** The line under a matter's note that says what it's doing, in the language; null when it's doing nothing. `day` is
 * the day of the reset or of the stop, in the language, when it isn't today. With `at` "", working says "Answering." and limit says the
 * reset wasn't given. */
export function matterStatus(lang: Lang, state: MatterState, at: string, day = ""): string | null {
  const time = held(lang, at);
  switch (state) {
    case "working":
      return at === ""
        ? frame(lang, "Answering.")
        : frame(lang, "Answering. Expected by {time}", { time });
    case "question":
      return frame(lang, "A question for you");
    case "permission":
      return frame(lang, "Needs your permission");
    case "ready":
      return frame(lang, "Answer ready");
    case "stopped":
      return day === ""
        ? frame(lang, "Stopped at {time}", { time })
        : frame(lang, "Stopped on {day} at {time}", { day, time });
    case "paused":
      // the choice still open, asked as a question, so it doesn't read like the paused matter whose choice is made
      if (at !== "")
        return day === ""
          ? frame(lang, "Limit reached. Continue at {time}?", { time })
          : frame(lang, "Limit reached. Continue on {day} at {time}?", { day, time });
      return frame(lang, "Paused at the plan's limit");
    case "pausedSet":
      // the reset to the minute; when the plan didn't say, the limit alone
      if (at === "") return frame(lang, "Paused at the plan's limit");
      return day === ""
        ? frame(lang, "Paused until {time}", { time })
        : frame(lang, "Paused until {day} {time}", { day, time });
    case "limit":
      // the reset, as the limit's words say it: today's time, another day's with its day, or that none was given
      if (at === "") return lim(lang, "Limit reached. Reset not given.");
      return day === ""
        ? lim(lang, "Limit reached until {time}", { time })
        : lim(lang, "Limit reached until {day} {time}", { day, time });
    case "moved":
      return mt(lang, "Folder moved");
    case "idle":
      return null;
  }
}

/** A conversation's status line's words, in the language. `behind` is the title it waits behind. */
export function conversationStatus(
  lang: Lang,
  state: ConversationState,
  at: string,
  behind = "",
): string | null {
  const time = held(lang, at);
  switch (state) {
    case "working":
      return at === ""
        ? frame(lang, "Answering.")
        : frame(lang, "Answering. Expected by {time}", { time });
    case "question":
      return frame(lang, "A question for you");
    case "ready":
      return frame(lang, "Answer ready");
    case "stopped":
      return frame(lang, "Stopped at {time}", { time });
    case "waiting":
      return mt(lang, "Waiting for '{title}'", { title: behind });
    case "idle":
      return null;
  }
}
