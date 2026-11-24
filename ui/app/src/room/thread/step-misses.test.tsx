// @vitest-environment happy-dom
// A page that couldn't open and a step Claude's safety check held back are never "failed" (Windows tester item C):
// each reason for both agents, the folded line's count, and "failed" kept for a paper not read, an edit not saved and
// a command that broke. Steps shaped as the agents send them (testing/step-lines.ts).
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, userMessage } from "../../testing/stream-script.ts";
import {
  AGENTS,
  claudeOwn,
  commandOf,
  fetchOf,
  heldOf,
  readOf,
  RULED,
  stepCall,
  yieldStep,
} from "../../testing/step-lines.ts";
import { summaryOf } from "./step-summary.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { missOf } from "./step-misses.ts";
import { doneOf, drawnKindOf, lineOf, markOf, troubleOf } from "./steps.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const URL = "https://www.livelaw.in/high-court/";

/** Each error as the fetch reports it, its reason, and its row. */
const ERRORS: [said: string, miss: string, row: string][] = [
  ["Request failed with status code 403", "refused", "Couldn't open livelaw.in: the site refused."],
  ["Request failed with status code 401", "refused", "Couldn't open livelaw.in: the site refused."],
  ["Request failed with status code 404", "not_found", "Couldn't open livelaw.in: not found."],
  [
    "getaddrinfo ENOTFOUND www.livelaw.in",
    "unreachable",
    "Couldn't open livelaw.in: couldn't reach the site.",
  ],
  [
    "connect ECONNREFUSED 104.21.3.4:443",
    "unreachable",
    "Couldn't open livelaw.in: couldn't reach the site.",
  ],
  [
    "maxContentLength size of 10485760 exceeded",
    "too_large",
    "Couldn't open livelaw.in: too large.",
  ],
  ["timeout of 60000ms exceeded", "slow", "Couldn't open livelaw.in: the site took too long."],
  // no reason at all: the page couldn't open, said without one
  ["", "unknown", "Couldn't open livelaw.in."],
];

test.each(AGENTS.flatMap((agent) => ERRORS.map((e) => [agent, ...e] as const)))(
  "under %s, a fetch that failed with %j couldn't open its page (%s), untagged",
  (agent, said, miss, row) => {
    const c = fetchOf(agent, URL, "failed", said);
    expect(missOf(c)).toBe(miss);
    expect(doneOf("en", c)).toBe(row);
    expect(markOf("en", c)).toBeNull();
    expect(troubleOf("en", [c])).toBe("");
  },
);

test("Claude's completed WebFetch with the site's 403 or 404 is a page that couldn't open", () => {
  const forbidden = fetchOf("Claude", URL, "completed", "The server returned HTTP 403 Forbidden.");
  const gone = fetchOf("Claude", URL, "completed", "The server returned HTTP 404 Not Found.");
  expect(doneOf("en", forbidden)).toBe("Couldn't open livelaw.in: the site refused.");
  expect(doneOf("en", gone)).toBe("Couldn't open livelaw.in: not found.");
  // a page that opened is no miss
  expect(missOf(fetchOf("Claude", URL, "completed", "The order is dated 12 March."))).toBeNull();
});

test("the tester's card: nine pages that couldn't open and two held back, none of them failed", () => {
  const calls: ToolCallEntry[] = [
    readOf("Claude", "Plaint.docx"),
    ...Array.from({ length: 4 }, () =>
      fetchOf("Claude", URL, "completed", "The server returned HTTP 403 Forbidden."),
    ),
    fetchOf("Claude", URL, "failed", "Request failed with status code 404"),
    fetchOf("Claude", URL, "failed", "getaddrinfo ENOTFOUND www.livelaw.in"),
    fetchOf("Claude", URL, "failed", "connect ECONNREFUSED 104.21.3.4:443"),
    fetchOf("Claude", URL, "failed", "maxContentLength size of 10485760 exceeded"),
    fetchOf("Claude", URL, "failed", "timeout of 60000ms exceeded"),
    heldOf(),
    heldOf(),
  ];
  const line = lineOf(
    "en",
    calls.map((tool_call) => ({ tool_call })),
    false,
  );
  expect(line.line).toBe("Read 1 paper.");
  expect(line.trouble).toBe("");
  expect(line.more).toBe("Couldn't open 9 pages. 2 held back by Claude's safety check.");
  expect(markOf("en", heldOf())).toBe("Held back by Claude's safety check");
});

test("only setbacks: the line says them, with no break", () => {
  const line = lineOf("en", [{ tool_call: heldOf() }], false);
  expect(line.line).toBe("1 held back by Claude's safety check.");
  expect(line.trouble).toBe("");
  expect(line.more).toBe("");
});

test.each(AGENTS)(
  "under %s, failed stays for a paper not read, an edit not saved and a command that broke",
  (agent) => {
    const paper = readOf(agent, "Plaint.docx", "failed");
    const edit = stepCall("edit", "Edit Reply.docx", "failed", {
      locations: [{ inside: { path: "Reply.docx" } }],
    });
    const command = commandOf(agent, "pdfinfo Plaint.pdf", null, "failed");
    for (const c of [paper, edit, command]) {
      expect(missOf(c)).toBeNull();
      expect(markOf("en", c)).toBe("Failed");
    }
    expect(troubleOf("en", [readOf(agent, "a.pdf"), paper, edit, command])).toBe("3 failed.");
  },
);

test("in the room, the card counts the pages that couldn't open and lists each with its reason", async () => {
  h = await mountRoom();
  const calls = [
    readOf("Claude", "Plaint.docx"),
    fetchOf("Claude", URL, "completed", "The server returned HTTP 403 Forbidden."),
    fetchOf(
      "Claude",
      "https://www.sci.gov.in/x.pdf",
      "failed",
      "maxContentLength size of 10485760 exceeded",
    ),
  ].map((tool_call) => ({ tool_call }));
  h.deliver([
    reset([
      userMessage(5, 1, "Find the Delhi High Court's latest order on the possession notice."),
      ...calls,
      agentText(30, 1, "The order is dated 12 March 2024."),
    ]),
  ]);
  await h.settle();
  const group = h.one("step-group")!;
  expect(group.textContent).toContain("Couldn't open 2 pages.");
  expect(group.textContent).not.toMatch(/failed/i);
  await act(async () => group.querySelector<HTMLButtonElement>("button")!.click());
  await h.settle();
  const text = h.one("step-group")!.textContent ?? "";
  expect(text).toContain("Couldn't open livelaw.in: the site refused.");
  expect(text).toContain("Couldn't open sci.gov.in: too large.");
  expect(text).not.toMatch(/Failed|maxContentLength|HTTP 403/);
});

test.each(AGENTS)(
  "under %s, a page whose result says something else (the lawyer's refusal, a blocked site) stays failed, its reason shown",
  (agent) => {
    for (const said of [
      "The user doesn't want to proceed with this tool use.",
      "Clauseo blocked this site: it is on the list of unsafe sites.",
      "Something else went wrong",
    ]) {
      const c = fetchOf(agent, URL, "failed", said);
      expect(missOf(c)).toBeNull();
      expect(doneOf("en", c)).toBe("Tried to read livelaw.in online.");
      expect(markOf("en", c)).toBe("Failed");
      expect(troubleOf("en", [c])).toBe("1 failed.");
    }
  },
);

test("Clauseo's own off-limits rule: its own mark, agreeing with the reason, and its own count", () => {
  const ruled = heldOf("failed", RULED);
  expect(markOf("en", ruled)).toBe("Clauseo keeps this off limits");
  expect(troubleOf("en", [ruled])).toBe("");
  const line = lineOf(
    "en",
    [{ tool_call: readOf("Claude", "Plaint.docx") }, { tool_call: ruled }],
    false,
  );
  expect(line.more).toBe("1 kept off limits by Clauseo.");
  expect(line.trouble).toBe("");
  // two rules, and a check beside them
  const more = lineOf(
    "en",
    [readOf("Claude", "a.pdf"), ruled, heldOf("failed", RULED), heldOf()].map((tool_call) => ({
      tool_call,
    })),
    false,
  ).more;
  expect(more).toBe("1 held back by Claude's safety check. 2 kept off limits by Clauseo.");
});

test("a held-back edit or command stays in the trouble line, with the break, its mark naming who held it", () => {
  const edit = heldOf("failed", undefined, "edit", "Edit");
  const command = heldOf("failed", undefined, "execute", "Bash");
  const ruledEdit = heldOf("failed", RULED, "edit", "Write");
  for (const c of [edit, command])
    expect(markOf("en", c)).toBe("Held back by Claude's safety check");
  expect(markOf("en", ruledEdit)).toBe("Clauseo keeps this off limits");
  const line = lineOf(
    "en",
    [readOf("Claude", "a.pdf"), edit, command, ruledEdit].map((tool_call) => ({ tool_call })),
    false,
  );
  expect(line.trouble).toBe("3 failed.");
  expect(line.more).toBe("");
});

test("Claude's own tools say what they did, never their names", () => {
  const rows: [name: string, done: string, tried: string][] = [
    ["TodoWrite", "Planned the work.", "Tried to plan the work."],
    ["TaskCreate", "Planned the work.", "Tried to plan the work."],
    ["TaskGet", "Planned the work.", "Tried to plan the work."],
    ["TaskList", "Planned the work.", "Tried to plan the work."],
    ["TaskUpdate", "Planned the work.", "Tried to plan the work."],
    ["ToolSearch", "Planned the work.", "Tried to plan the work."],
    ["EnterPlanMode", "Planned the work.", "Tried to plan the work."],
    ["ExitPlanMode", "Planned the work.", "Tried to plan the work."],
    ["SendMessage", "Asked a helper.", "Tried to ask a helper."],
    ["TaskStop", "Stopped background work.", "Tried to stop background work."],
    ["AskUserQuestion", "Asked you.", "Tried to ask you."],
  ];
  for (const [name, done, tried] of rows) {
    expect(doneOf("en", claudeOwn(name))).toBe(done);
    expect(doneOf("en", claudeOwn(name, "failed"))).toBe(tried);
    expect(summaryOf("en", [claudeOwn(name)]).toLowerCase()).not.toMatch(
      /used|todo|task|ask user|plan mode|tool search|send message/,
    );
  }
  // switch_mode as claude-agent-acp sends ExitPlanMode: its own words title it, its tool name says what it did
  const plan = stepCall("switch_mode", "Ready to code?", "completed", {
    tool_name: "ExitPlanMode",
  });
  expect(doneOf("en", plan)).toBe("Planned the work.");
  expect(
    summaryOf("en", [
      readOf("Claude", "a.pdf"),
      claudeOwn("TodoWrite"),
      claudeOwn("SendMessage"),
      claudeOwn("TaskStop"),
      claudeOwn("AskUserQuestion"),
    ]),
  ).toBe("Read 1 paper, planned the work, asked a helper, stopped background work, asked you.");
  expect(drawnKindOf(claudeOwn("TodoWrite"))).toBe("think");
});

test("a failed Yield says it tried to wait, in the trouble line as any failure", () => {
  const y = yieldStep("failed");
  expect(doneOf("en", y)).toBe("Tried to wait for the background work.");
  expect(markOf("en", y)).toBe("Failed");
  expect(troubleOf("en", [readOf("Claude", "a.pdf"), y])).toBe("1 failed.");
});

test("in Hindi, a line whose parts fall back to English reads as English: a capital and a full stop", () => {
  expect(summaryOf("hi", [readOf("Claude", "a.pdf"), claudeOwn("AskUserQuestion")])).toBe(
    "Read 1 paper, asked you.",
  );
});
