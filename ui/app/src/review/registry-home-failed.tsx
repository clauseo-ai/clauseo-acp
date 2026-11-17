// agent.home_failed's lines for the open conversation's agent (words-errors.ts's plannedKey): Clauseo couldn't prepare
// the agent's own folder before starting it. Claude's is U3-err-agent.home_failed (registry.tsx's ERRORS); these are
// ChatGPT's, an agent of no kind by its name, and the nameless line, each under the code's own id with the plan's
// suffix. Each is the stop line under the question, as a turn whose launch failed shows it; the nameless line shows
// only where no name is known at all, which the room never has today, so it renders by itself.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords as sentence } from "@clauseo/components/lib/words-errors.ts";
import type { AgentId } from "../transport/protocol.ts";
import { step, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

const QUESTION = userMessage(5, 1, "Summarise the notice to the bank.");

/** A turn of `agent` whose launch failed with agent.home_failed: the stop line under the question. */
const homeFailed = (agent: AgentId) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [QUESTION, step(7, { stop: "failed", failure: "agent.home_failed" })],
        agent,
      })
    }
  />
);

/** The nameless line, alone. */
const nameless = (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>{sentence(lang, "agent.home_failed", { kind: undefined, name: null })}</p>
  </main>
);

export const HOME_FAILED: Entry[] = [
  {
    id: "U3-err-agent.home_failed.chatgpt",
    words: errorWords(
      "ChatGPT didn't start, because Clauseo couldn't set up the folder it keeps for ChatGPT on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.",
    ),
    screen: "room/thread",
    render: homeFailed("codex-acp"),
  },
  {
    id: "U3-err-agent.home_failed.named",
    words: errorWords(
      "{name} didn't start, because Clauseo couldn't set up the folder it keeps for {name} on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.",
    ),
    screen: "room/thread",
    render: homeFailed("gemini"),
  },
  {
    id: "U3-err-agent.home_failed.nameless",
    words: errorWords(
      "The AI didn't start, because Clauseo couldn't set up the folder it keeps for the AI on this computer. Your question is kept. If Try again doesn't help, quit Clauseo and open it again. If it keeps happening, show this to whoever looks after this computer.",
    ),
    screen: "room/thread",
    render: nameless,
  },
];
