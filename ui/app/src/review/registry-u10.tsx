// U10's copy review entries (when something breaks), apart from registry.tsx to keep that file under the size limit:
// an answer Claude stopped giving, as it arrives while the lawyer watches (the working line, the mark in the thread,
// and Restart's details once the host restarts, H10 faked); a stream that failed; and the start-up lines H18 will show,
// each on a plain full-window line until H18 brings its screen.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { STARTUP_WORDS, startup, type StartupCode } from "@clauseo/components/lib/words-startup.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { breakWords, startupWords } from "./words.ts";

const thread = (state: ThreadState) => () => <RoomOver host={() => threadHost(state)} />;

const ASKED = userMessage(5, 1, "Summarise the notice to the bank.");
const SO_FAR = agentText(6, 1, "Clause nine requires **notice in writing**, served at");
/** The answer under way, then Claude's process exiting: the failed step arrives live, at the time it arrives. */
const stopped = (restart: boolean) =>
  thread({
    entries: [ASKED, SO_FAR],
    status: "running",
    later: [step(9, { stop: "failed", failure: "agent.exited", ts: Date.now() })] as ViewEntry[],
    restart,
  });

/** A start-up line as H18's window will hold it: alone, centred, nothing to press. */
const startupLine = (code: StartupCode) => (lang: Lang) => (
  <main
    data-testid="startup-failed"
    data-code={code}
    lang={lang}
    className="grid min-h-dvh place-items-center bg-page p-(--s4)"
  >
    <p className="max-w-[36rem] text-center">{startup(lang, STARTUP_WORDS[code])}</p>
  </main>
);

/** U10's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U10: Entry[] = [
  {
    id: "U10-01",
    words: breakWords(
      "The answer stopped at {time}. Your question and the answer so far are kept.",
    ),
    screen: "room/composer",
    render: stopped(false),
  },
  {
    id: "U10-02",
    words: breakWords("The answer stopped here at {time}"),
    screen: "room/thread",
    render: stopped(false),
  },
  {
    id: "U10-03",
    words: breakWords(
      "The answer stopped with no reason given. Restart asks your question once more. The answer so far stays above.",
    ),
    screen: "room/composer",
    render: stopped(true),
  },
  {
    id: "U10-04",
    words: breakWords("This conversation stopped updating in this window."),
    screen: "room/composer",
    render: thread({ entries: [ASKED, SO_FAR], lose: "store.io", loseRetryable: true }),
  },
  ...(Object.keys(STARTUP_WORDS) as StartupCode[]).map((code, i): Entry => ({
    id: `U10-${String(5 + i).padStart(2, "0")}`,
    words: startupWords(STARTUP_WORDS[code]),
    screen: "startup-failed",
    render: startupLine(code),
  })),
];
