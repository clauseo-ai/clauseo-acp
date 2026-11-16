// The probe's first read (?thread=first-read, room/probe-main.tsx): the matter's first read under way, its latest read
// codex-acp's quoted shell read of a file outside the matter, as stage C's session had it (D-019), so the WebKit probe
// and a browser see the line under the composer as the release draws it. With ?file=long the file's name is long
// enough to give way in its middle at every width. Not shipped.
import type { ViewEntry, ViewPatchBody } from "../../transport/protocol.ts";
import { header, MATTER, reset, THREAD, TURN, userMessage } from "../../testing/stream-script.ts";
import { seedMarks } from "./marks.ts";
import { FIRST_READ_PROMPT } from "./prompt.ts";

const SHORT = "matter-brief.md";
const LONG = "matter-brief-for-the-hearing-on-the-fourteenth-of-november-before-the-registrar.md";

/** codex-acp's read: a shell `cat` of the agent's notes, its path in quotes, no location in the matter. */
const codexRead = (file: string): ViewEntry => ({
  tool_call: {
    order: 3,
    tool_call: "00000000-0000-7000-8000-0000000000a7",
    turn: TURN,
    started_at: 3000,
    title: `cat '/Users/lawyer/Library/Application Support/Clauseo/agent-state/01a1/notes/${file}'`,
    kind: "read",
    status: "in_progress",
    locations: [],
    content: [],
  },
});

/** The page's thread, and the mark that makes it the matter's first read over `papers` files. */
export function firstReadProbe(long: boolean, papers: number): ViewPatchBody[] {
  seedMarks({ [MATTER]: { added: true, thread: THREAD, turns: [TURN], papers } });
  return [
    reset(
      [userMessage(2, 1, FIRST_READ_PROMPT), codexRead(long ? LONG : SHORT)],
      header("running"),
    ),
  ];
}
