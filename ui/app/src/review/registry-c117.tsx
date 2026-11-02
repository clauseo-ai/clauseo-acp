// #117's copy review entries, the composer's: apart from registry.tsx, as registry-u4.tsx is. The `/` menu's name, the
// list's accessible name a screen reader says (it draws no heading), over the room with a thread whose agent lists
// its commands, `/` typed in the focused field.
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomAsking } from "./states.tsx";
import { threadWords } from "./words.ts";

/** The thread's header as Claude Code's agent fills it: a few of its commands, one taking an input. */
const COMMANDS = [
  { name: "compact", description: "Clear the conversation history but keep a summary in context" },
  { name: "init", description: "Initialize a new CLAUDE.md file with codebase documentation" },
  { name: "review", description: "Review a pull request", input_hint: "pull request number" },
];

/** #117's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const C117: Entry[] = [
  {
    id: "C117-01",
    words: threadWords("Commands"),
    screen: "room/composer-menu",
    render: () => (
      <RoomAsking
        host={() => threadHost({ entries: [], commands: COMMANDS })}
        question="/"
        send={false}
      />
    ),
  },
];
