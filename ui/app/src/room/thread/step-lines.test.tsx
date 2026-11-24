// @vitest-environment happy-dom
// Steps name what ran (issue 13), for each kind under both agents, with steps shaped as claude-agent-acp and codex-acp
// send them (testing/step-lines.ts): a paper by its file name, a search by its words, a skill by its name, an MCP tool
// by its server and tool made readable, a command by the agent's own words, a tool the room doesn't know by its name,
// a helper asked. The folded line lists them in one sentence and never says "other things" or "a task".
import { act } from "react";
import { afterEach, describe, expect, test } from "vitest";
import { agentText, header, reset, userMessage } from "../../testing/stream-script.ts";
import {
  AGENTS,
  commandOf,
  helperOf,
  mcpOf,
  readOf,
  searchOf,
  sharmaSteps,
  skillOf,
  stepCall,
  unknownOf,
  yieldStep,
  type Agent,
} from "../../testing/step-lines.ts";
import type { ToolCallEntry, ToolCallStatus } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { doingOf } from "./step-names.ts";
import { doneOf, drawnKindOf, summaryOf } from "./steps.ts";
import { readableServer, readableTool, toolLabelOf } from "./tool-names.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const KANOON = { query: "SARFAESI section 13(2) notice period" };

/** Each kind's step under `agent` in `status`, and its row done and tried. */
const KINDS: [
  kind: string,
  make: (agent: Agent, status: ToolCallStatus) => ToolCallEntry,
  done: string,
  tried: string,
][] = [
  [
    "a paper read",
    (a, s) => readOf(a, "Plaint.docx", s),
    "Read Plaint.docx.",
    "Tried to read Plaint.docx.",
  ],
  [
    "a search of the papers",
    (a, s) => searchOf(a, "notice period", s),
    "Searched for 'notice period'.",
    "Tried to search for 'notice period'.",
  ],
  [
    "a skill",
    (a, s) => skillOf(a, "clauseo:legal-drafting", s),
    "Used the legal drafting skill.",
    "Tried to use the legal drafting skill.",
  ],
  [
    "an MCP tool",
    (a, s) => mcpOf(a, "indian-kanoon", "search_judgments", KANOON, s),
    "Used Indian Kanoon: search judgments.",
    "Tried to use Indian Kanoon: search judgments.",
  ],
  [
    "a tool the room doesn't know",
    (a, s) => unknownOf(a, "fetch_cause_list", s),
    "Used fetch cause list.",
    "Tried to use fetch cause list.",
  ],
  ["a helper with no task", (a, s) => helperOf(a, s), "Asked a helper.", "Tried to ask a helper."],
];

describe.each(AGENTS)("under %s", (agent) => {
  test.each(KINDS)("%s names what ran, done and tried", (_kind, make, done, tried) => {
    expect(doneOf("en", make(agent, "completed"))).toBe(done);
    expect(doneOf("en", make(agent, "failed"))).toBe(tried);
  });

  test("the folded line names each part in one sentence, never other things or a task", () => {
    const calls = sharmaSteps(agent).map((e) => e.tool_call!);
    const line = summaryOf("en", calls);
    expect(line).toMatch(/^Read 3 papers, /);
    expect(line).toContain("used Indian Kanoon");
    expect(line).toContain("used the legal drafting skill");
    expect(line).toContain("searched the papers");
    expect(line).not.toMatch(/other thing|\btasks?\b|\bmore\b/);
  });
});

test("a command says the agent's own words for it after Ran:, else the program, else a command", () => {
  const pages = "Count the pages of the plaint";
  expect(doneOf("en", commandOf("Claude", "pdfinfo Plaint.pdf", pages))).toBe(
    "Ran: count the pages of the plaint.",
  );
  expect(doneOf("en", commandOf("Claude", "pdfinfo Plaint.pdf", pages, "failed"))).toBe(
    "Tried to run: count the pages of the plaint.",
  );
  // ChatGPT's exec carries no words of its own: the program and the paper it ran on
  expect(doneOf("en", commandOf("ChatGPT", "pdfinfo Plaint.pdf", null))).toBe(
    "Ran pdfinfo on Plaint.pdf.",
  );
  expect(doneOf("en", commandOf("ChatGPT", "mkdir drafts && cp a.docx drafts", null))).toBe(
    "Ran 2 commands.",
  );
  for (const agent of AGENTS)
    expect(summaryOf("en", [commandOf(agent, "pdfinfo Plaint.pdf", pages)])).toBe("Ran 1 command.");
});

test("several MCP servers and skills: the servers listed, the skills counted", () => {
  const calls = [
    mcpOf("Claude", "indian-kanoon", "search_judgments", KANOON),
    mcpOf("ChatGPT", "scc_online", "find_case", {}),
    mcpOf("Claude", "indian-kanoon", "get_document", { docid: 1 }),
    skillOf("Claude", "clauseo:legal-drafting"),
    skillOf("ChatGPT", "clauseo:legal-research"),
  ];
  expect(summaryOf("en", calls)).toBe("Used Indian Kanoon and SCC Online, used 2 skills.");
});

test("readable names: no prefix, underscores and capitals to spaces, a server's words capitalised", () => {
  expect(readableServer("indian-kanoon")).toBe("Indian Kanoon");
  expect(readableServer("indiankanoon")).toBe("Indian Kanoon");
  expect(readableServer("firm_docs")).toBe("Firm Docs");
  expect(readableServer("SCC")).toBe("SCC");
  expect(readableTool("search_judgments")).toBe("search judgments");
  expect(readableTool("TodoWrite")).toBe("todo write");
  expect(readableTool("get_PDF_text")).toBe("get PDF text");
  expect(toolLabelOf({ title: "mcp.indian-kanoon.search_judgments" })).toBe(
    "Indian Kanoon: search judgments",
  );
});

const ASKED = userMessage(
  5,
  1,
  "Read the plaint, the written statement and the loan agreement, and tell me whether the bank's notice under section 13(2) gave the sixty days.",
);

test.each(AGENTS)(
  "in the room under %s: the folded line names what ran, and the opened rows each name their object",
  async (agent) => {
    h = await mountRoom();
    h.deliver([reset([ASKED, ...sharmaSteps(agent), agentText(30, 1, "The notice fell short.")])]);
    await h.settle();
    const group = h.one("step-group")!;
    expect(group.textContent).toContain("Read 3 papers, ");
    expect(group.textContent).not.toMatch(/other thing|Ran 1 task/);
    await act(async () => group.querySelector<HTMLButtonElement>("button")!.click());
    await h.settle();
    const text = h.one("step-group")!.textContent ?? "";
    for (const said of [
      "Read Plaint.docx.",
      "Read Written statement.pdf.",
      "Used the legal drafting skill.",
      "Used Indian Kanoon: search judgments.",
    ])
      expect(text).toContain(said);
    expect(text).not.toContain("mcp");
    expect(text).not.toContain("clauseo:legal-drafting");
  },
);

describe("Claude's Yield (tool_name Yield): a wait for its own background work", () => {
  test("its row, its part of the line and its live words say it waited, never another thing", () => {
    expect(doneOf("en", yieldStep())).toBe("Waited for the background work.");
    expect(doneOf("en", yieldStep("failed"))).toBe("Tried to wait for the background work.");
    expect(doingOf("en", yieldStep("in_progress"))).toBe("Waiting for the background work.");
    expect(drawnKindOf(yieldStep())).toBe("think");
    const line = summaryOf("en", [
      readOf("Claude", "Plaint.docx"),
      helperOf("Claude"),
      yieldStep(),
      yieldStep(),
    ]);
    expect(line).toBe("Read 1 paper, asked a helper, waited for the background work.");
    expect(line).not.toMatch(/other thing|used/);
  });

  test("while it waits, the working line says so, never its name", async () => {
    h = await mountRoom();
    h.deliver([
      reset(
        [ASKED, { tool_call: helperOf("Claude") }, { tool_call: yieldStep("in_progress") }],
        header("running"),
      ),
    ]);
    await h.settle();
    expect(h.one("working")!.textContent).toContain("Waiting for the background work.");
    expect(document.body.textContent).not.toContain("Yield");
  });

  test("the answer in one reply after the wait: one group, its line naming the wait, then the answer", async () => {
    h = await mountRoom();
    h.deliver([
      reset([
        ASKED,
        { tool_call: helperOf("Claude") },
        { tool_call: yieldStep() },
        agentText(30, 1, "The notice fell short by thirteen days."),
      ]),
    ]);
    await h.settle();
    const groups = h.all("step-group");
    expect(groups).toHaveLength(1);
    expect(groups[0]!.textContent).toContain("Asked a helper, waited for the background work.");
    expect(document.body.textContent).toContain("The notice fell short by thirteen days.");
    expect(document.body.textContent).not.toMatch(/Yield|other thing/);
  });

  test("the answer in two replies (launched, then the result): each reply after its own group, nothing broken", async () => {
    h = await mountRoom();
    h.deliver([
      reset([
        ASKED,
        { tool_call: helperOf("Claude") },
        agentText(20, 1, "I have asked a helper to read the loan agreement."),
        { tool_call: yieldStep() },
        agentText(30, 1, "The helper is back: the notice fell short by thirteen days."),
      ]),
    ]);
    await h.settle();
    const groups = h.all("step-group");
    expect(groups.map((g) => g.textContent?.includes("Asked a helper."))).toContain(true);
    expect(groups.map((g) => g.textContent?.includes("Waited for the background work."))).toContain(
      true,
    );
    const text = document.body.textContent ?? "";
    expect(text.indexOf("I have asked a helper")).toBeLessThan(text.indexOf("The helper is back"));
    expect(text).not.toMatch(/Yield|other thing|State not known/);
  });
});

describe("ChatGPT's own helper steps (codex-acp 2.10.0: kind other, no tool name, the title all there is)", () => {
  const titled = (title: string, raw: object = {}, status: ToolCallStatus = "completed") =>
    stepCall("other", title, status, { raw });

  test("a wait waits for helpers: its row, its live words, its part of the line", () => {
    const wait = titled("wait", { ids: ["019a"] });
    expect(doneOf("en", wait)).toBe("Waited for helpers.");
    expect(doneOf("en", titled("wait", {}, "failed"))).toBe(
      "Tried to wait for the background work.",
    );
    expect(doingOf("en", titled("wait", {}, "in_progress"))).toBe("Waiting for helpers.");
    expect(summaryOf("en", [readOf("ChatGPT", "Plaint.docx"), wait])).toBe(
      "Read 1 paper, waited for the background work.",
    );
  });

  test.each([
    "Start subagent extract",
    "Complete subagent extract",
    "Interact with subagent extract",
    "Interrupt subagent extract",
    "sendInput",
    "resumeAgent",
    "closeAgent",
    "sendMessage",
    "followupTask",
    "interruptAgent",
    "listAgents",
  ])("%s worked with a helper, never its title", (title) => {
    const c = titled(title);
    expect(doneOf("en", c)).toBe("Worked with a helper.");
    expect(doneOf("en", titled(title, {}, "failed"))).toBe("Tried to ask a helper.");
    expect(doingOf("en", titled(title, {}, "in_progress"))).toBe("Working on it.");
    expect(summaryOf("en", [c])).toBe("Asked a helper.");
  });

  test("a spawn with no prompt worked with a helper; with one, it says the task (UC-906)", () => {
    expect(doneOf("en", titled("spawnAgent"))).toBe("Worked with a helper.");
    expect(
      doneOf("en", titled("spawnAgent", { prompt: "Extract the dates from the plaint" })),
    ).toBe("Asked a helper: Extract the dates from the plaint.");
  });

  test("the same title from Claude, with a tool name, is not one of ChatGPT's", () => {
    expect(doneOf("en", stepCall("other", "wait", "completed", { tool_name: "wait" }))).not.toBe(
      "Waited for helpers.",
    );
  });
});
