// The helper cards' review scenes (H1, helper-card.tsx): a title dispute with nine papers and a
// long question, the real room over the fake host. Claude's answer with four helpers (two working,
// one done, one failed) and ChatGPT's with one of its own, shaped as C2's CONTRACT.md §6. Times are
// minutes before the page opened, so the cards read "Working for 3 min" whatever the hour.
import type {
  ConfigOption,
  PaperSummary,
  ToolCallEntry,
  ToolCallStatus,
  ViewEntry,
} from "../transport/protocol.ts";
import {
  askedBy,
  helperPartOf,
  helperStepViewEntry,
  helperTextViewEntry,
  helperViewEntry,
  type HelperEntry,
} from "../room/thread/helper-entries.ts";
import { agentText, permission, step, TURN, userMessage, uuid } from "../testing/stream-script.ts";
import { LIMIT_SUMMARY } from "../testing/helper-script.ts";
import type { ThreadState } from "./hosts.ts";

const MIN = 60_000;
/** When the question went: seven minutes before the page opened. */
const T0 = Date.now() - 7 * MIN;

export const HELPER_PAPERS: PaperSummary[] = [
  "Plaint.pdf",
  "Written statement.pdf",
  "Sale deed 1987.pdf",
  "Sale deed 1998.pdf",
  "Sale deed 2004.pdf",
  "Gift deed 2011.pdf",
  "Mutation entries.pdf",
  "Encumbrance certificate.pdf",
  "Survey map, Plot 14.pdf",
].map((path, i) => ({ file: uuid(i + 1, 8), path, size: 40_000 + i * 3_700, modified: T0 }));

const QUESTION = userMessage(
  5,
  1,
  "We act for the plaintiffs. Go through all the deeds and the mutation entries and trace the title to Plot 14 from 1987 to today. Tell me where the chain breaks, if it does, and whether the defendants' claim of adverse possession since 1995 has any support in Supreme Court or Delhi High Court judgments. I need this for the replication due Friday.",
);

const hid = (n: number) => uuid(n, 11);

/** The helper an entry is, or undefined. */
const helperOf = (e: ViewEntry): HelperEntry | undefined => {
  const part = helperPartOf(e);
  return part?.kind === "helper" ? part.entry : undefined;
};

/** A step `n` at `order`, `at` minutes after T0; `of`: its helper. */
function call(
  order: number,
  n: number,
  at: number,
  over: Partial<ToolCallEntry> & Pick<ToolCallEntry, "title" | "kind">,
  of?: number,
): ViewEntry {
  const entry: ToolCallEntry = {
    order,
    tool_call: uuid(n, 7),
    turn: TURN,
    started_at: T0 + at * MIN,
    status: "completed",
    locations: [],
    content: [],
    ...over,
  };
  return helperStepViewEntry(entry, of === undefined ? null : hid(of));
}

const read = (
  order: number,
  n: number,
  at: number,
  file: string,
  of: number,
  s: ToolCallStatus = "completed",
) =>
  call(
    order,
    n,
    at,
    { title: `Read ${file}`, kind: "read", status: s, locations: [{ inside: { path: file } }] },
    of,
  );

const search = (
  order: number,
  n: number,
  at: number,
  query: string,
  of: number,
  s: ToolCallStatus = "completed",
) =>
  call(
    order,
    n,
    at,
    {
      title: `Web search: ${query}`,
      kind: "fetch",
      status: s,
      raw_input: { inline: JSON.stringify({ query }) },
    },
    of,
  );

const agent = (order: number, n: number, at: number, label: string, s: ToolCallStatus) =>
  call(order, n, at, { title: label, kind: "think", tool_name: "Agent", status: s });

const helper = (order: number, n: number, at: number, over: Partial<HelperEntry>): ViewEntry =>
  helperViewEntry({
    order,
    helper: hid(n),
    turn: TURN,
    name: "paper-reader",
    started_at: T0 + at * MIN,
    ...over,
  });

const words = (order: number, n: number, of: number, text: string): ViewEntry =>
  helperTextViewEntry({ order, event: uuid(n, 12), turn: TURN, helper: hid(of), text });

const select = (
  id: string,
  category: "model" | "thought_level",
  values: [string, string][],
): ConfigOption => ({
  id,
  name: id,
  category,
  kind: {
    select: {
      current_value: values[0]![0],
      options: values.map(([value, name]) => ({ value, name })),
    },
  },
});

const CLAUDE_OPTIONS = [
  select("model", "model", [
    ["claude-opus-5-5", "Opus 5.5"],
    ["claude-sonnet-5-5", "Sonnet 5.5"],
    ["claude-haiku-5-5", "Haiku 5.5"],
  ]),
  select("effort", "thought_level", [["medium", "Medium"]]),
];

/** Claude's answer while it works: four helpers, two working, one done, one failed. */
export function claudeHelpers(): ThreadState {
  return {
    status: "running",
    options: CLAUDE_OPTIONS,
    papers: HELPER_PAPERS,
    title: "Title to Plot 14",
    entries: [
      QUESTION,
      agentText(
        8,
        1,
        "I'll have four helpers work on this at once: three on the deeds and one on the case law.",
      ),
      agent(10, 50, 0, "Trace 1987 to 1998", "in_progress"),
      helper(11, 1, 0, {
        call: uuid(50, 7),
        label: "Trace the title from the 1987 deed to the 1998 deed",
        model: "claude-haiku-5-5",
        task: { inline: "Read Sale deed 1987.pdf and Sale deed 1998.pdf and trace the title." },
      }),
      agent(12, 51, 0, "Trace 1998 to today", "in_progress"),
      helper(13, 2, 0, {
        call: uuid(51, 7),
        label: "Trace the title from 1998 to today through the later deeds",
        model: "claude-haiku-5-5",
      }),
      agent(14, 52, 0, "Read the mutation entries", "completed"),
      helper(15, 3, 0, {
        call: uuid(52, 7),
        label: "Check the mutation entries against the deeds",
        model: "claude-haiku-5-5",
        outcome: "done",
        finished_at: T0 + 4 * MIN + 20_000,
        summary: {
          inline:
            "The mutation entries follow every deed until 1998. The 1998 sale was never mutated: the revenue record still shows the 1987 buyer, so the defendants cannot lean on the record for their possession.\n\nEntry by entry, with dates, below.",
        },
      }),
      agent(16, 53, 0, "Find case law", "failed"),
      helper(17, 4, 0, {
        call: uuid(53, 7),
        name: "researcher",
        label:
          "Find Supreme Court and Delhi High Court judgments on adverse possession against a co-owner",
        model: "claude-haiku-5-5",
        outcome: "failed",
        finished_at: T0 + 2 * MIN,
      }),
      read(20, 60, 1, "Sale deed 1987.pdf", 1),
      read(21, 61, 1, "Mutation entries.pdf", 3),
      search(22, 62, 1, "adverse possession co-owner ouster Supreme Court", 4),
      read(23, 63, 1, "Sale deed 1998.pdf", 2),
      words(24, 1, 1, "The 1987 deed conveys Plot 14 whole to R. K. Sharma. "),
      read(25, 64, 2, "Encumbrance certificate.pdf", 3),
      search(26, 65, 2, "Delhi High Court adverse possession 1995 Plot", 4, "failed"),
      read(27, 66, 3, "Sale deed 2004.pdf", 2),
      read(28, 67, 3, "Gift deed 2011.pdf", 2, "in_progress"),
      read(29, 68, 3, "Sale deed 1998.pdf", 1, "in_progress"),
    ],
  };
}

/** ChatGPT's answer: a helper with a name it chose and no task, working, with a helper of its own
 * inside its card; another done. */
export function chatgptHelpers(): ThreadState {
  return {
    status: "running",
    agent: "codex-acp",
    options: [
      select("model", "model", [["gpt-6.2-sol", "6.2 Sol"]]),
      select("effort", "thought_level", [["medium", "Medium"]]),
    ],
    papers: HELPER_PAPERS,
    title: "Title to Plot 14",
    entries: [
      QUESTION,
      agentText(8, 1, "I'll start two helpers: one reads the deeds, one checks the survey map."),
      helper(10, 1, 0, { name: "Read documents", model: "gpt-6.2-sol" }),
      helper(11, 2, 0, {
        name: "Check the survey map",
        model: "gpt-6.2-sol",
        outcome: "done",
        finished_at: T0 + 1 * MIN + 30_000,
        summary: {
          inline:
            "**Boundaries:** the survey map's Plot 14 matches Sale_deed_1987.pdf; the 2011 gift deed takes 40 square yards more on the east ([survey rules](https://dlrc.delhigovt.nic.in/)).",
        },
      }),
      call(12, 70, 1, { title: "cat '/matter/Sale deed 1987.pdf'", kind: "execute" }, 1),
      call(13, 71, 1, { title: "cat '/matter/Survey map, Plot 14.pdf'", kind: "execute" }, 2),
      words(14, 2, 1, "The **1987 deed** is clear. Going on to the 1998 deed."),
      helper(15, 3, 2, { parent: hid(1), name: "Find the vendor's heirs", model: "gpt-6.2-sol" }),
      call(
        16,
        73,
        2,
        {
          title: "rg -n heirs '/matter/Mutation entries.pdf'",
          kind: "execute",
          status: "in_progress",
        },
        3,
      ),
      read(17, 72, 3, "Sale deed 1998.pdf", 1, "in_progress"),
    ],
  };
}

/** Settled: Claude's answer ended, its helpers done, stopped and failed. */
export function settledHelpers(): ThreadState {
  const s = claudeHelpers();
  const ended = s.entries.map((e): ViewEntry => {
    const h = helperOf(e);
    if (h !== undefined && h.outcome === undefined)
      return helperViewEntry({
        ...h,
        outcome: h.helper === hid(1) ? "stopped" : "done",
        finished_at: T0 + 5 * MIN,
        summary: {
          inline: "The chain holds from 1998 to today: each deed recites the one before it.",
        },
      });
    // the stopped helper's step in hand stopped with it; every other step finished
    if (e.tool_call?.status === "in_progress")
      return {
        tool_call: {
          ...e.tool_call,
          status: e.tool_call.tool_call === uuid(68, 7) ? "cancelled" : "completed",
        },
      };
    return e;
  });
  return {
    ...s,
    status: "idle",
    entries: [
      ...ended,
      agentText(40, 2, "The chain breaks in 1998: the sale was never mutated."),
      step(41),
    ],
  };
}

/** Claude's answer with a helper of no type ("default"), named by its place: "Helper 1". */
export function unnamedHelper(): ThreadState {
  const s = claudeHelpers();
  return {
    ...s,
    entries: s.entries.map((e) => {
      const h = helperOf(e);
      return h?.helper === hid(2) ? helperViewEntry({ ...h, name: "default" }) : e;
    }),
  };
}

/** Settled, the second helper cut at its turn limit before it reported: done, the limit said. */
export function limitedHelper(): ThreadState {
  const s = settledHelpers();
  return {
    ...s,
    entries: s.entries.map((e) => {
      const h = helperOf(e);
      return h?.helper === hid(2)
        ? helperViewEntry({ ...h, summary: { inline: LIMIT_SUMMARY } })
        : e;
    }),
  };
}

/** The answer ended, the second helper still working in the background (the host leaves a
 * background helper open when its turn ends by itself, and ends it on its own end frame). */
export function backgroundHelper(): ThreadState {
  const s = settledHelpers();
  return {
    ...s,
    entries: s.entries.map((e) => {
      const h = helperOf(e);
      if (h?.helper !== hid(2)) return e;
      const { outcome: _o, finished_at: _f, summary: _s, ...open } = h;
      return helperViewEntry(open);
    }),
  };
}

/** ChatGPT's answer, its helper "Read documents" asking to run a command: the ask names it. */
export function helperAsks(): ThreadState {
  const s = chatgptHelpers();
  return {
    ...s,
    status: "needs_you",
    entries: [
      ...s.entries,
      // its step, stamped with it, and the permission it asks for that step
      call(
        18,
        74,
        3,
        { title: "pdftotext '/matter/Sale deed 1998.pdf'", kind: "execute", status: "pending" },
        1,
      ),
      askedBy(permission(19, 74), hid(1)),
    ],
  };
}

/** The same ask from a helper the window has no entry for: "A helper asks:". */
export function unknownHelperAsks(): ThreadState {
  const s = helperAsks();
  return {
    ...s,
    entries: s.entries.map((e) => (e.permission === undefined ? e : askedBy(e, hid(9)))),
  };
}
