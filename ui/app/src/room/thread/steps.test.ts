// The step group's model and words: which rows fold into a group, the folded line's summary for each mix of kinds,
// what failed kept on it, each row's words, and the paper named by its file, its folder kept for the hover.
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { describe, expect, test } from "vitest";
import { agentText, header, toolCall } from "../../testing/stream-script.ts";
import type { EntryId, ToolCallEntry, ToolCallStatus, ToolKind } from "../../transport/protocol.ts";
import type { StreamView } from "../../store/view.ts";
import { doingOf, doingPick, placeOf, splitPath, titleOf } from "./step-names.ts";
import {
  doneOf,
  doneSoFar,
  iconsOf,
  kindsOf,
  lineOf,
  markOf,
  segmentsOf,
  summaryOf,
  troubleOf,
  type Part,
} from "./steps.ts";

let n = 0;
/** A tool call of `kind`, with what the agent gave for it. */
function call(
  kind: ToolKind,
  status: ToolCallStatus = "completed",
  over: Partial<ToolCallEntry> = {},
): ToolCallEntry {
  n += 1;
  return { ...toolCall(10 + n, 300 + n, status).tool_call!, kind, title: "", ...over };
}
const raw = (v: object) => ({ raw_input: { inline: JSON.stringify(v) } });

describe("segmentsOf", () => {
  const parts: Record<string, Part> = {};
  const of = (spec: string) => {
    // t: a tool call, h: a thought, a: an answer's words, p: a permission, q: an elicitation
    const ids = spec.split("").map((c, i) => {
      const id = `${c}${i}`;
      parts[id] = c === "t" ? "tool" : c === "h" ? "thought" : "alone";
      return id;
    });
    return segmentsOf(ids, (id) => parts[id]!).map((s) =>
      s.row !== undefined ? s.row : `[${s.group.join(" ")}]`,
    );
  };

  test("everything between two of the agent's texts folds into one group, thoughts with the tool calls", () => {
    expect(of("aththa")).toEqual(["a0", "[t1 h2 t3 h4]", "a5"]);
    expect(of("tttt")).toEqual(["[t0 t1 t2 t3]"]);
  });

  test("a run of thoughts alone is no group: its thoughts stand as rows", () => {
    expect(of("ahha")).toEqual(["a0", "h1", "h2", "a3"]);
  });

  test("a permission prompt or a question parts the runs either side, and never folds", () => {
    expect(of("ttpttqt")).toEqual(["[t0 t1]", "p2", "[t3 t4]", "q5", "[t6]"]);
  });
});

describe("the folded line", () => {
  test("one sentence per kind, the most done first, ties in the order they came; one and many counted apart", () => {
    const calls = [
      call("search"),
      call("read"),
      call("read"),
      call("read"),
      call("search"),
      call("execute"),
    ];
    expect(summaryOf("en", calls)).toBe("3 papers read. Searched 2 times. Ran 1 task.");
    expect(summaryOf("hi", calls)).toBe("3 दस्तावेज़ पढ़े गए। 2 बार खोजा गया। 1 काम चलाया गया।");
    expect(summaryOf("en", [call("read")])).toBe("1 paper read.");
    expect(summaryOf("en", [call("search")])).toBe("Searched once.");
  });

  test("every kind has its words", () => {
    const kinds: ToolKind[] = ["edit", "delete", "move", "fetch", "think", "other"];
    expect(kinds.map((k) => summaryOf("en", [call(k), call(k)]))).toEqual([
      "2 papers changed.",
      "2 papers deleted.",
      "2 papers moved.",
      "Looked online 2 times.",
      "Planned the work.",
      "Did 2 other things.",
    ]);
    // a kind from a newer agent, or switch_mode, counts as other
    expect(summaryOf("en", [call("switch_mode"), call("unknown")])).toBe("Did 2 other things.");
  });

  test("past three kinds, the rest are counted as more", () => {
    const calls = [
      call("read"),
      call("read"),
      call("search"),
      call("edit"),
      call("fetch"),
      call("move"),
    ];
    expect(summaryOf("en", calls)).toBe(
      "2 papers read. Searched once. 1 paper changed. And 2 more.",
    );
    expect(kindsOf(calls).map((k) => k.kind)).toEqual(["read", "search", "edit", "fetch", "move"]);
    // switch_mode is counted as other but draws its own glyph, unless another kind shares other's count
    expect(iconsOf([call("read"), call("switch_mode")])).toEqual(["read", "switch_mode"]);
    expect(iconsOf([call("switch_mode"), call("unknown")])).toEqual(["other"]);
    expect(lineOf("en", [{ tool_call: call("switch_mode") }], false).kinds).toEqual([
      "switch_mode",
    ]);
  });

  test("a failure stays on the line: a group of one, or one of many; a stop too", () => {
    expect(troubleOf("en", [call("read", "failed")])).toBe("It failed.");
    expect(troubleOf("en", [call("read"), call("fetch", "failed")])).toBe("1 of them failed.");
    expect(
      troubleOf("en", [call("read", "failed"), call("fetch", "failed"), call("read", "cancelled")]),
    ).toBe("2 of them failed. 1 of them was stopped.");
    expect(troubleOf("hi", [call("read"), call("fetch", "failed")])).toBe("उनमें से 1 विफल रहा।");
    expect(troubleOf("en", [call("read")])).toBe("");
  });

  test("live, the count done so far", () => {
    expect(doneSoFar("en", [call("read", "in_progress")])).toBe("");
    expect(doneSoFar("en", [call("read"), call("read", "in_progress")])).toBe("1 done so far.");
    expect(doneSoFar("hi", [call("read"), call("read"), call("read", "in_progress")])).toBe(
      "अब तक 2 पूरे हुए।",
    );
  });
});

describe("friendly names", () => {
  test("a paper inside the matter by its file name, its folder for the hover", () => {
    const c = call("read", "completed", {
      title: "Read /Users/asha/Matters/Acme/papers/Lease deed.pdf",
      locations: [{ inside: { path: "papers/2019/Lease deed.pdf", line: 4 } }],
    });
    expect(placeOf(c)).toMatchObject({
      name: "Lease deed.pdf",
      where: "papers/2019",
      outside: false,
    });
    expect(doneOf("en", c)).toBe("Read Lease deed.pdf.");
    expect(doingOf("en", c)).toBe("Reading Lease deed.pdf.");
    expect(doneOf("hi", c)).toBe("Lease deed.pdf पढ़ा गया।");
  });

  test("with no locations, the raw input's path; outside the matter, flagged", () => {
    const c = call("edit", "completed", raw({ file_path: "/Users/asha/Matters/Acme/Reply.docx" }));
    expect(placeOf(c)).toMatchObject({ name: "Reply.docx", where: "/Users/asha/Matters/Acme" });
    expect(doneOf("en", c)).toBe("Changed Reply.docx.");
    const out = call("read", "completed", { locations: [{ outside: { display: "/etc/hosts" } }] });
    expect(placeOf(out)).toMatchObject({ name: "hosts", where: "/etc", outside: true });
  });

  test("a search by its words, a fetch by its site, a task by its description", () => {
    expect(doneOf("en", call("search", "completed", raw({ pattern: "notice period" })))).toBe(
      "Searched for 'notice period'.",
    );
    expect(
      doneOf("en", call("fetch", "completed", raw({ url: "https://www.indiankanoon.org/doc/1/" }))),
    ).toBe("Read indiankanoon.org online.");
    expect(doingOf("en", call("fetch", "in_progress", raw({ query: "Section 138 NI Act" })))).toBe(
      "Searching online for 'Section 138 NI Act'.",
    );
    expect(
      doneOf(
        "en",
        call("execute", "completed", raw({ command: "ls", description: "List the papers" })),
      ),
    ).toBe("List the papers.");
    expect(doneOf("en", call("execute"))).toBe("Ran a task.");
    expect(doneOf("en", call("read"))).toBe("Read the papers.");
  });

  test("nothing else to go on: the title, its absolute paths cut to their file names", () => {
    expect(titleOf("Ask the service about /Users/asha/Matters/Acme/notes.txt")).toBe(
      "Ask the service about notes.txt",
    );
    expect(titleOf("Open C:\\Users\\asha\\Lease.pdf now")).toBe("Open Lease.pdf now");
    expect(doneOf("en", call("other", "completed", { title: "Ask the document service" }))).toBe(
      "Ask the document service.",
    );
    expect(doneOf("en", call("other"))).toBe("Did one other thing.");
    expect(splitPath("papers/")).toEqual({ name: "papers", folder: "" });
  });

  test("a row's state where it didn't simply finish", () => {
    expect(markOf("en", call("read", "failed"))).toBe("Failed");
    expect(markOf("en", call("read", "cancelled"))).toBe("Stopped");
    expect(markOf("en", call("read", "pending"))).toBe(steps("en", "Waiting"));
    // the row's word is U7's, one string for it
    expect(markOf("en", call("read", "in_progress", { background: { steer: "m" } }))).toBe(
      mt("en", "In the background"),
    );
    expect(markOf("en", call("read"))).toBeNull();
    // a row that didn't finish says what it was doing
    expect(doneOf("en", call("read", "failed", raw({ path: "a/Lease.pdf" })))).toBe(
      "Reading Lease.pdf.",
    );
  });
});

describe("doingPick: the working line's words", () => {
  const view = (status: "running" | "idle", entries: { id: EntryId; e: object }[]): StreamView =>
    ({
      header: header(status),
      ids: entries.map((x) => x.id),
      entries: new Map(entries.map((x) => [x.id, x.e])),
    }) as unknown as StreamView;
  const pick = doingPick("en");

  test("what the newest step is doing while it runs; a thought; nothing while the answer is written", () => {
    const reading = call("read", "in_progress", raw({ path: "papers/Lease.pdf" }));
    expect(pick(view("running", [{ id: "tool_call:a", e: { tool_call: reading } }]))).toBe(
      "Reading Lease.pdf.",
    );
    expect(pick(view("running", [{ id: "agent_text:t", e: agentText(1, 1, "", "thought") }]))).toBe(
      stepLive("en", "Thinking it through."),
    );
    expect(pick(view("running", [{ id: "agent_text:a", e: agentText(1, 1, "So") }]))).toBeNull();
    expect(
      pick(view("running", [{ id: "tool_call:a", e: { tool_call: call("read") } }])),
    ).toBeNull();
    expect(pick(view("idle", [{ id: "tool_call:a", e: { tool_call: reading } }]))).toBeNull();
  });
});
