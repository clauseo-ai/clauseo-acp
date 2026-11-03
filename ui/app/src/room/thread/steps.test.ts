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
  hoverOf,
  iconsOf,
  kindsOf,
  lineOf,
  markOf,
  restOf,
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

  test("at rest the line's words and icons count only finished steps: a step still waiting, failed or stopped is never told as done", () => {
    const lease = raw({ path: "a/Lease deed.pdf" });
    // UTC-17: one read done, one change still waiting on the lawyer: the words and the icons count the read alone
    expect(restOf("en", [call("read"), call("edit", "pending", lease)])).toEqual({
      kinds: ["read"],
      line: "1 paper read.",
      parts: ["1 paper read."],
      trouble: "",
    });
    expect(
      restOf("en", [call("read"), call("fetch", "failed"), call("search", "cancelled")]),
    ).toEqual({
      kinds: ["read"],
      line: "1 paper read.",
      parts: ["1 paper read."],
      trouble: "1 failed. 1 stopped.",
    });
    // none done: the step still to finish says what it is doing, with its own icon
    expect(restOf("en", [call("read", "failed", lease), call("edit", "pending", lease)])).toEqual({
      kinds: ["edit"],
      line: "Changing Lease deed.pdf.",
      trouble: "1 failed.",
    });
    const line = (calls: ToolCallEntry[]) =>
      lineOf(
        "en",
        calls.map((c) => ({ tool_call: c })),
        false,
      );
    expect(line([call("read"), call("edit", "pending", lease)]).kinds).toEqual(["read"]);
  });

  test("every step over and none finished: what was tried is the trouble, said once, never followed by It failed.", () => {
    const lease = raw({ path: "a/Lease deed.pdf" });
    // no kind icon: the break leads alone, so the one step isn't marked twice
    expect(restOf("en", [call("read", "failed", lease)])).toEqual({
      kinds: [],
      line: "",
      trouble: "Tried to read Lease deed.pdf.",
    });
    expect(restOf("hi", [call("read", "failed", lease)]).trouble).toBe(
      "Lease deed.pdf पढ़ने की कोशिश की गई।",
    );
    expect(
      restOf("en", [
        call("read", "failed", lease),
        ...Array.from({ length: 4 }, () => call("edit", "cancelled")),
      ]).trouble,
    ).toBe(
      "Tried to read Lease deed.pdf. Tried to change a paper. Tried to change a paper. And 2 more.",
    );
    for (const lang of ["en", "hi"] as const) {
      const one = restOf(lang, [call("fetch", "failed")]);
      expect(one.trouble).not.toContain(troubleOf(lang, [call("fetch", "failed")]));
    }
  });

  test("a failure stays on the line, one of many; a stop too", () => {
    expect(troubleOf("en", [call("read"), call("fetch", "failed")])).toBe("1 failed.");
    expect(
      troubleOf("en", [call("read", "failed"), call("fetch", "failed"), call("read", "cancelled")]),
    ).toBe("2 failed. 1 stopped.");
    expect(troubleOf("hi", [call("read"), call("fetch", "failed")])).toBe("1 विफल रहा।");
    expect(troubleOf("en", [call("read"), ...[1, 2].map(() => call("read", "cancelled"))])).toBe(
      "2 stopped.",
    );
  });

  test("live, a step waiting on the lawyer is told as waiting for them, never as underway", () => {
    const edit = {
      tool_call: call("edit", "pending", { title: "/Users/x/Notice to the bank.docx" }),
    };
    const done = { tool_call: call("read") };
    const asked = lineOf("en", [done, edit], true, "permission");
    expect(asked.line).toBe("Needs your permission.");
    expect(asked.more).toBe("1 done so far.");
    // led by the wait's own icon, never the edit's
    expect(asked.kinds).toEqual([]);
    expect(asked.asks).toBe("permission");
    expect(lineOf("en", [done, edit], true, "question").asks).toBe("question");
    expect(lineOf("en", [done, edit], true).asks).toBeUndefined();
    expect(lineOf("en", [done, edit], true, "question").line).toBe("A question for you.");
    expect(lineOf("hi", [done, edit], true, "permission").line).not.toMatch(/बदल/);
    // with nothing asked it says what the step is doing
    expect(lineOf("en", [done, edit], true).line).toMatch(/^Changing /);
  });

  test("the failed are counted on their own, never as 'of them' of a count that holds only what finished (UTC-40)", () => {
    const calls = [call("read"), call("fetch", "failed"), call("fetch", "failed")];
    const sentence = (lang: "en" | "hi") => {
      const r = restOf(lang, calls);
      return `${r.line} ${r.trouble}`;
    };
    expect(sentence("en")).toBe("1 paper read. 2 failed.");
    expect(sentence("hi")).toBe("1 दस्तावेज़ पढ़ा गया। 2 विफल रहे।");
    expect(sentence("en")).not.toContain("of them");
    expect(sentence("hi")).not.toContain("उनमें से");
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

  test("no path or words but the title: the row reads the title, its path cut to the file name and its folder on hover, never the generic sentence again", () => {
    const titled = (kind: ToolKind, title: string) => call(kind, "completed", { title });
    const reads = [
      titled("read", "Read /Users/x/Matters/Sharma/Complaint.pdf"),
      titled("read", "Read /Users/x/Matters/Sharma/Form DIR-12.pdf"),
    ];
    expect(reads.map((c) => doneOf("en", c))).toEqual([
      "Read Complaint.pdf.",
      "Read Form DIR-12.pdf.",
    ]);
    expect(reads.map(hoverOf)).toEqual(["/Users/x/Matters/Sharma", "/Users/x/Matters/Sharma"]);
    // a paper's step takes the paper its title names, so it reads in the room's own words
    expect(doneOf("en", titled("edit", "Edit C:\\Matters\\Sharma\\Reply.docx"))).toBe(
      "Changed Reply.docx.",
    );
    expect(hoverOf(titled("search", "grep notice period"))).toBeNull();
  });

  test("a step tried and not done never reads as done, even when only its title or a task's description names it", () => {
    const lease = { title: "Read /Users/x/Matters/Lease.pdf" };
    expect(doneOf("en", call("read", "failed", lease))).toBe("Tried to read Lease.pdf.");
    expect(doneOf("hi", call("read", "failed", lease))).toBe("Lease.pdf पढ़ने की कोशिश की गई।");
    expect(restOf("en", [call("read", "failed", lease)]).trouble).toBe("Tried to read Lease.pdf.");
    expect(doneOf("en", call("other", "cancelled", { title: "Check the cause list" }))).toBe(
      "Tried: Check the cause list.",
    );
    expect(doneOf("hi", call("other", "cancelled", { title: "Check the cause list." }))).toBe(
      "कोशिश की गई: Check the cause list।",
    );
    const count = raw({ command: "x", description: "Count the pages" });
    expect(doneOf("en", call("execute", "failed", count))).toBe("Tried: Count the pages.");
    // done, the agent's own words as they are
    expect(doneOf("en", call("execute", "completed", count))).toBe("Count the pages.");
    expect(doneOf("en", call("other", "completed", { title: "Check the cause list" }))).toBe(
      "Check the cause list.",
    );
  });

  test("a step in a state this version can't read is neither done nor failed nor running: no tense, no break, out of the counts, the line and the row agree", () => {
    const lease = {
      locations: [{ inside: { path: "papers/Lease.pdf" } }],
    } as Partial<ToolCallEntry>;
    const unknown = call("read", "unknown", lease);
    const words = [
      doneOf("en", unknown),
      doneOf("en", call("other", "unknown", { title: "Check the cause list" })),
      doneOf("en", call("search", "unknown", raw({ pattern: "notice period" }))),
      doneOf("en", call("execute", "unknown")),
      restOf("en", [unknown]).line,
    ];
    expect(words).toEqual([
      "Lease.pdf.",
      "Check the cause list.",
      "'notice period'.",
      "State not known.",
      "State not known.",
    ]);
    // never the running words nor the done or tried ones
    for (const w of words)
      expect(w).not.toMatch(/\b\w+ing\b|^(Read|Changed|Searched|Ran|Did|Tried)\b/);
    expect(restOf("en", [unknown])).toEqual({
      kinds: ["read"],
      line: "State not known.",
      trouble: "",
    });
    expect(restOf("hi", [unknown]).line).toBe("स्थिति ज्ञात नहीं।");
    expect(markOf("en", unknown)).toBe("State not known");
    expect(markOf("hi", unknown)).toBe("स्थिति ज्ञात नहीं");
    // a row nothing names says it once, with no mark after
    expect(markOf("en", call("execute", "unknown"))).toBeNull();
    // beside one done and one failed: counted in neither
    const mixed = [call("read", "completed", lease), unknown, call("fetch", "failed")];
    expect(restOf("en", mixed)).toEqual({
      kinds: ["read"],
      line: "1 paper read.",
      parts: ["1 paper read."],
      trouble: "1 failed.",
    });
    // beside one failed and none done: the failure alone is what was tried
    expect(restOf("en", [unknown, call("read", "failed", lease)]).trouble).toBe(
      "Tried to read Lease.pdf.",
    );
  });

  test("a web address in a title is no path: it stays whole in the words and on hover", () => {
    expect(titleOf("Fetch https://example.com/a/b")).toBe("Fetch https://example.com/a/b");
    expect(titleOf("Open file:///Users/x/a.pdf")).toBe("Open file:///Users/x/a.pdf");
    const open = call("other", "in_progress", { title: "Open https://indiankanoon.org/doc/123/" });
    expect(doingOf("en", open)).toBe("Open https://indiankanoon.org/doc/123/.");
    expect(doingOf("en", open)).not.toContain("https:123");
    expect(hoverOf(open)).toBe("https://indiankanoon.org/doc/123/");
    // a path after a word's slash or a colon is no path either; a real one beside an address is still cut
    expect(titleOf("Compare https://x.org/a/b with /Users/x/Lease.pdf")).toBe(
      "Compare https://x.org/a/b with Lease.pdf",
    );
    // with no title either, the generic sentence
    expect(doneOf("en", call("read"))).toBe("Read the papers.");
    expect(hoverOf(call("read"))).toBeNull();
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
    // a row that failed or was stopped says, in the past, what it set out to do; never as if it were still going
    expect(doneOf("en", call("read", "failed", raw({ path: "a/Lease.pdf" })))).toBe(
      "Tried to read Lease.pdf.",
    );
    expect(
      doneOf("en", call("fetch", "failed", raw({ url: "https://indiankanoon.org/doc/1/" }))),
    ).toBe("Tried to read indiankanoon.org online.");
    expect(doneOf("en", call("edit", "cancelled"))).toBe("Tried to change a paper.");
    expect(doneOf("en", call("other", "failed"))).toBe("Tried one other thing.");
    expect(doneOf("hi", call("read", "failed", raw({ path: "a/Lease.pdf" })))).toBe(
      "Lease.pdf पढ़ने की कोशिश की गई।",
    );
    // a step that waits or runs still says what it is doing
    expect(doneOf("en", call("read", "pending", raw({ path: "a/Lease.pdf" })))).toBe(
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
