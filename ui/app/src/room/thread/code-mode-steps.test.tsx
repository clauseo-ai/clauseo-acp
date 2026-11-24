// @vitest-environment happy-dom
// Claude's code mode tools (stage B): REPL, Poll and StructuredOutput never arrive over ACP (stage B's live captures);
// should one come, with kind other or think, its tool name or only its title, it shows nothing: no row, no live words,
// no count, never "Used …". A command or a real tool with another name stays as it was.
import { expect, test } from "vitest";
import { readOf, stepCall } from "../../testing/step-lines.ts";
import type { ToolKind } from "../../transport/protocol.ts";
import { doingPick } from "./step-names.ts";
import { lineOf, segmentsPick, summaryOf } from "./steps.ts";
import { header } from "../../testing/stream-script.ts";
import { entryIdOf, type StreamView } from "../../store/view.ts";

const SHAPES = (["REPL", "Poll", "StructuredOutput"] as const).flatMap((title) =>
  (["other", "think"] as ToolKind[]).flatMap((kind) =>
    [true, false].map((named) => ({ title, kind, named })),
  ),
);

test.each(SHAPES)(
  "$title ($kind, tool name: $named) never shows: no row, no live words, no count",
  ({ title, kind, named }) => {
    const code = (status: "completed" | "in_progress") =>
      stepCall(kind, title, status, named ? { tool_name: title, raw: {} } : { raw: {} });
    const read = readOf("Claude", "Plaint.docx");
    // not counted, never named
    expect(summaryOf("en", [read, code("completed")])).toBe("Read 1 paper.");
    // live, the step before it says what is being done
    const live = lineOf("en", [{ tool_call: read }, { tool_call: code("in_progress") }], true);
    expect(live.line).not.toMatch(new RegExp(title, "i"));
    // no row: left out of the group
    const entries = [{ tool_call: read }, { tool_call: code("completed") }];
    const ids = entries.map(entryIdOf);
    const paged = new Map(entries.map((e) => [entryIdOf(e), e] as const));
    expect(segmentsPick(ids, paged)(undefined)).toEqual([{ group: [ids[0]] }]);
    // the working line skips it
    const running = code("in_progress");
    const v = {
      ids: [entryIdOf({ tool_call: running })],
      entries: new Map([[entryIdOf({ tool_call: running }), { tool_call: running }]]),
      header: header("running"),
    } as unknown as StreamView;
    expect(doingPick("en")(v) ?? "").not.toMatch(new RegExp(title, "i"));
  },
);

test("a command titled Poll, or a tool named otherwise, is no code mode tool", () => {
  const poll = stepCall("execute", "Poll", "completed", {
    tool_name: "Bash",
    raw: { command: "poll" },
  });
  expect(summaryOf("en", [poll])).toBe("Ran 1 command.");
  const other = stepCall("other", "REPL", "completed", { tool_name: "FetchCauseList", raw: {} });
  expect(summaryOf("en", [other])).toBe("Used fetch cause list.");
});
