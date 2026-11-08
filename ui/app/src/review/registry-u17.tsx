// U17's copy review entries: what came back from a tool call, under its row in an open step group, in the real room
// over FakeTransport. Each state opens the group and presses rows as the lawyer would: the press closed (U17-row),
// the forms opened side by side (MCP text, pretty JSON, a plain string, an error marking its row Failed, text that
// isn't JSON), the fold at 12 lines, the cap, nothing came back, a blob's read held past 400 ms, and a blob too large
// beside one whose read was refused. The outputs are review fixtures (tool-result/fixtures.ts), hand-made in MCP's
// CallToolResult shape inside codex-acp's {result, error}: no recorded codex-acp sample exists yet.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tr, type ToolResultKey } from "@clauseo/components/lib/words-tool-result.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import * as F from "../room/thread/tool-result/fixtures.ts";
import type { TextOrBlob, ToolCallEntry, ToolKind, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { RoomClicking } from "./states.tsx";
import type { WordsRef } from "./words.ts";

/** A string of a row's result (words-tool-result.ts). */
const words = (key: ToolResultKey): WordsRef => ({
  file: "lib/words-tool-result.ts",
  key,
  text: (lang) => tr(lang, key),
  later: tr.hi[key] === null,
});
/** Two lines of one id, joined with " / " as the approved page writes them. */
const pair = (one: ToolResultKey, many: ToolResultKey): WordsRef => ({
  file: "lib/words-tool-result.ts",
  key: `${one} / ${many}`,
  text: (lang: Lang) => `${tr(lang, one)} / ${tr(lang, many)}`,
  later: tr.hi[one] === null || tr.hi[many] === null,
});

/** A local time today. */
const atLocal = (h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};

/** A finished call titled as Codex names an MCP tool, `output` its raw output, a minute after the last. */
const row = (
  i: number,
  title: string,
  output: TextOrBlob,
  over: Partial<ToolCallEntry> = {},
  kind: ToolKind = "other",
): ViewEntry => {
  const started_at = atLocal(16, 24) + i * 60_000;
  return {
    tool_call: F.called(output, {
      order: 10 + i,
      kind,
      title,
      started_at,
      finished_at: started_at + 40_000,
      ...over,
    }),
  };
};

const QUESTION = (() => {
  const asked = userMessage(5, 1, "Find Supreme Court judgments on section 138 of the NI Act.");
  return { user_message: { ...asked.user_message!, ts: atLocal(16, 23) } };
})();
const ANSWER = agentText(90, 1, "Three Supreme Court judgments on dishonoured cheques came back.");
const ENDED = step(91, { ts: atLocal(16, 32) });

const GROUP = '[data-testid="step-group"] button';
/** The press of the group's `n`th row (1-based), not a block's own Show all. */
const press = (n: number) =>
  `[data-testid="tool-call"]:nth-child(${n}) > .step-row-text > .step-row-press`;
const SHOW_ALL = ".step-row-result > .step-row-press";

/** The settled thread with `calls` before the answer, its group opened, then `clicks` pressed in order. */
const state = (calls: ViewEntry[], clicks: string[], blobs?: ThreadState["blobs"]) => () => (
  <RoomClicking
    host={() =>
      threadHost({
        entries: [QUESTION, ...calls, ANSWER, ENDED],
        ...(blobs === undefined ? {} : { blobs }),
      })
    }
    clicks={[GROUP, ...clicks]}
  />
);

const CLOSED = state(
  [
    row(0, "Search Indian Kanoon", F.MCP_JUDGMENTS),
    row(1, "Read file 1", F.JSON_RESULT, {}, "read"),
    row(2, "Open the judgment", F.QUOTED),
  ],
  [],
);
// the forms, each opened: MCP text, pretty JSON, a plain string, an error (its row Failed), text that isn't JSON
const FORMS = state(
  [
    row(0, "Search Indian Kanoon", F.MCP_JUDGMENTS),
    row(1, "Fetch the judgment's details", F.JSON_RESULT),
    row(2, "Save the judgment", F.QUOTED),
    row(3, "Search Indian Kanoon again", F.OPAQUE_ERROR),
    row(4, "Count the papers", F.NOT_JSON),
  ],
  [press(1), press(2), press(3), press(4), press(5)],
);
const MIXED = state([row(0, "Look at the page", F.MCP_MIXED)], [press(1)]);
const LONG = state([row(0, "List the paragraphs", F.LONG_RESULT)], [press(1)]);
const CAPPED = state([row(0, "List every page", F.HUGE_RESULT)], [press(1), SHOW_ALL]);
const EMPTY = state([row(0, "Ask the court's site", F.NULL)], [press(1)]);
const READING = state([row(0, "Search Indian Kanoon", { blob: F.BLOB })], [press(1)], {
  [F.BLOB.hash]: "pending",
});
const LARGE = state(
  [
    row(0, "Read the whole judgment", { blob: F.BLOB_LARGE }),
    row(1, "Search Indian Kanoon", { blob: F.BLOB_REFUSED }),
  ],
  [press(1), press(2)],
  { [F.BLOB_REFUSED.hash]: "refused" },
);

const entry = (id: string, w: WordsRef, render: () => ReactNode): Entry => ({
  id,
  words: w,
  screen: "room/thread",
  render,
});

/** U17's entries, each id written as it is. */
export const U17: Entry[] = [
  entry("U17-01", words("Show the result"), CLOSED),
  // tall enough for every form, the error's row last
  { ...entry("U17-02", words("Hide the result"), FORMS), height: 1300 },
  entry("U17-03", pair("1 picture isn't shown here.", "{n} pictures aren't shown here."), MIXED),
  entry(
    "U17-04",
    pair("1 more item isn't shown here.", "{n} more items aren't shown here."),
    MIXED,
  ),
  entry("U17-05", words("Show all"), LONG),
  entry("U17-06", words("The rest of this result isn't shown here."), CAPPED),
  entry("U17-07", words("Nothing came back."), EMPTY),
  entry("U17-08", words("Reading the result."), READING),
  entry("U17-09", words("This result is too large to show here."), LARGE),
];
