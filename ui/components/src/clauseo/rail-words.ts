// What a matter and a conversation on the rail say they're doing, in the language, and the dot that leads each status.
import { clock } from "../lib/words.ts";
import { frame } from "../lib/words-frame.ts";
import { mt } from "../lib/words-matters.ts";
import type { Lang } from "../lib/display.ts";

/** What a matter is doing while the lawyer looks elsewhere. One at a time:
 * - working: Clauseo is answering, with the clock time it expects to finish;
 * - question, permission: Clauseo waits on the lawyer, so the row takes court yellow, as the prepared draft does;
 * - ready: the answer finished while the lawyer was away, a quiet mark until the matter is opened;
 * - stopped: the lawyer stopped it, said in concrete;
 * - paused: the plan's limit stopped it until the reset. While the lawyer hasn't chosen how it goes on, Continue at
 *   the reset or ask the other plan, it waits on the lawyer's hand and takes court yellow; pausedSet, once Continue
 *   is chosen, is concrete with its time;
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
  | "moved";

/** What a conversation is doing while the lawyer reads another. waiting: behind another conversation's answer. */
export type ConversationState = "idle" | "working" | "question" | "ready" | "stopped" | "waiting";

/** A clock time that holds together on its line: "4:32 pm" never breaks before "pm". */
const held = (lang: Lang, at: string) => clock(lang, at).replace(/ (am|pm)$/, "\u00a0$1");

/** The line under a matter's note that says what it's doing, in the language; null when it's doing nothing. `day` is
 * the reset's day, in the language, when it isn't today. */
export function matterStatus(lang: Lang, state: MatterState, at: string, day = ""): string | null {
  const time = held(lang, at);
  switch (state) {
    case "working":
      return frame(lang, "Answering. Expected by {time}", { time });
    case "question":
      return frame(lang, "A question for you");
    case "permission":
      return frame(lang, "Needs your permission");
    case "ready":
      return frame(lang, "Answer ready");
    case "stopped":
      return frame(lang, "Stopped at {time}", { time });
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
    case "moved":
      return mt(lang, "Folder moved");
    case "idle":
      return null;
  }
}

/** Each matter status's mark: one 8 px dot leading the status line, in one place for every status. Yellow only where
 * the matter waits for the lawyer's hand, as the prepared draft waits; an answer ready in primary, news to open;
 * working and stopped in the muted tone; a folder that moved as an open ring, since nothing is there to fill it. */
export const MATTER_MARK: Record<Exclude<MatterState, "idle">, string> = {
  question: "bg-yellow ring-yellow",
  permission: "bg-yellow ring-yellow",
  ready: "bg-primary",
  working: "bg-muted",
  stopped: "bg-muted",
  paused: "bg-yellow ring-yellow",
  pausedSet: "bg-muted",
  moved: "room-dot-ring",
};

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
      return frame(lang, "Answering. Expected by {time}", { time });
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
