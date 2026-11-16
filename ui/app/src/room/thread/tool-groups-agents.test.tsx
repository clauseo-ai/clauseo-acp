// @vitest-environment happy-dom
// A group of Clauseo's own MCP tools' steps (the browser guard, the case-law connector), as each agent names them:
// Claude `mcp__<server>__<tool>` with the kind other, ChatGPT (codex-acp 2.2.2) `mcp.<server>.<tool>` with the kind
// execute. The folded line counts them by family, never as "Ran 1 task." or "Did 1 other thing.", and each row and
// the line draw the family's icon: the browser's the fetch globe, the case-law connector's the case-law glyph.
import { act } from "react";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, toolCall, userMessage, uuid } from "../../testing/stream-script.ts";
import type {
  ToolCallEntry,
  ToolCallStatus,
  ToolKind,
  ViewEntry,
} from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { drawnKindOf, groupKindOf, iconsOf, summaryOf } from "./steps.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

type Agent = "Claude" | "ChatGPT";
const AGENTS: Agent[] = ["Claude", "ChatGPT"];

/** A call of `server`'s `tool` as `agent` sends it, the n-th of its group. */
function named(
  agent: Agent,
  server: string,
  tool: string,
  n = 0,
  status: ToolCallStatus = "completed",
): ToolCallEntry {
  const claude = agent === "Claude";
  const kind: ToolKind = claude ? "other" : "execute";
  const input = claude ? {} : { server, tool, arguments: {} };
  return {
    ...toolCall(7 + n, 290 + n, status).tool_call!,
    tool_call: uuid(290 + n, 7),
    kind,
    title: claude ? `mcp__${server}__${tool}` : `mcp.${server}.${tool}`,
    raw_input: { inline: JSON.stringify(input) },
  };
}

/** Each family's tool, what it counts as, the icon it draws, and the folded line for one and for three. */
const FAMILIES: [
  server: string,
  tool: string,
  counted: string,
  icon: string,
  one: StepKey,
  three: StepKey,
][] = [
  [
    "browser",
    "agent_browser_open",
    "fetch",
    "fetch",
    "Looked online once.",
    "Looked online {n} times.",
  ],
  [
    "browser",
    "agent_browser_snapshot",
    "fetch",
    "fetch",
    "Looked online once.",
    "Looked online {n} times.",
  ],
  ...(["search_law", "read_document", "citator", "check_citations"] as const).map(
    (tool) =>
      [
        "case-law",
        tool,
        "case_law_search",
        "case_law",
        "Searched case law once.",
        "Searched case law {n} times.",
      ] as [string, string, string, string, StepKey, StepKey],
  ),
  [
    "case-law",
    "account",
    "case_law_credits",
    "case_law",
    "Checked your case-law credits once.",
    "Checked your case-law credits {n} times.",
  ],
  [
    "case-law",
    "recharge",
    "case_law_payment",
    "case_law",
    "Got a payment link for case-law credits once.",
    "Got {n} payment links for case-law credits.",
  ],
];

const CASES = FAMILIES.flatMap(([server, tool, counted, icon, one, three]) =>
  AGENTS.map((agent) => ({ agent, server, tool, counted, icon, one, three })),
);

const OFF = [
  "Ran 1 task.",
  "Ran {n} tasks.",
  "Did 1 other thing.",
  "Did {n} other things.",
] as const;

test.each(CASES)(
  "$server's $tool under $agent counts as $counted and draws $icon",
  ({ agent, server, tool, counted, icon, one, three }) => {
    const c = named(agent, server, tool);
    expect(groupKindOf(c)).toBe(counted);
    expect(drawnKindOf(c)).toBe(icon);
    expect(iconsOf([c])).toEqual([icon]);
    expect(summaryOf("en", [c])).toBe(steps("en", one));
    const group = [0, 1, 2].map((n) => named(agent, server, tool, n));
    expect(summaryOf("en", group)).toBe(steps("en", three, { n: 3 }));
    for (const off of OFF) expect(summaryOf("en", group)).not.toContain(steps("en", off, { n: 3 }));
  },
);

test.each(AGENTS)(
  "a mixed group under %s counts each family on its own, and any other step by its kind",
  (agent) => {
    const calls = [
      named(agent, "browser", "agent_browser_open", 0),
      named(agent, "browser", "agent_browser_click", 1),
      named(agent, "case-law", "search_law", 2),
      named(agent, "case-law", "account", 3),
      { ...toolCall(20, 400, "completed").tool_call!, kind: "execute" as const, title: "ls" },
    ];
    expect(summaryOf("en", calls)).toBe(
      [
        steps("en", "Looked online {n} times.", { n: 2 }),
        steps("en", "Searched case law once."),
        steps("en", "Checked your case-law credits once."),
        steps("en", "And 1 more."),
      ].join(" "),
    );
    expect(iconsOf(calls)).toEqual(["fetch", "case_law", "case_law", "execute"]);
  },
);

test("a third-party MCP tool keeps its agent's kind: ChatGPT's a task, Claude's another thing", () => {
  expect(groupKindOf(named("ChatGPT", "docs", "fetch"))).toBe("execute");
  expect(groupKindOf(named("Claude", "docs", "fetch"))).toBe("other");
  expect(drawnKindOf(named("Claude", "docs", "fetch"))).toBe("other");
});

const ASKED = userMessage(5, 1, "Find the latest on the Delhi High Court's order.");

test.each(
  AGENTS.flatMap((agent) => [
    {
      agent,
      server: "browser",
      tool: "agent_browser_open",
      line: "Looked online {n} times." as StepKey,
      icon: "fetch",
    },
    {
      agent,
      server: "case-law",
      tool: "search_law",
      line: "Searched case law {n} times." as StepKey,
      icon: "case_law",
    },
  ]),
)(
  "in the room, two $server steps under $agent: the folded line says '$line' and each row draws $icon",
  async ({ agent, server, tool, line, icon }) => {
    h = await mountRoom();
    const calls: ViewEntry[] = [0, 1].map((n) => ({ tool_call: named(agent, server, tool, n) }));
    h.deliver([reset([ASKED, ...calls, agentText(20, 1, "The order is out.")])]);
    await h.settle();
    const group = h.one("step-group")!;
    expect(group.textContent).toContain(steps("en", line, { n: 2 }));
    expect(group.textContent).not.toContain(steps("en", "Ran {n} tasks.", { n: 2 }));
    expect(group.textContent).not.toContain(steps("en", "Did {n} other things.", { n: 2 }));
    await act(async () => group.querySelector<HTMLButtonElement>("button")!.click());
    await h.settle();
    const rows = [...h.one("step-group")!.querySelectorAll<HTMLElement>("[data-kind]")];
    expect(rows.length).toBe(2);
    for (const row of rows) expect(row.dataset["kind"]).toBe(icon);
  },
);
