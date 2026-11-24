// @vitest-environment happy-dom
// A tool Clauseo ships (the browser guard, the case-law connector), as each agent names its MCP call on the way to
// the room. Claude (claude-agent-acp) sends the tool's name as the title, `mcp__<server>__<tool>`, with the kind `other`
// and the tool's own input; ChatGPT (codex-acp 2.2.2, `McpToolReporter`) sends `mcp.<server>.<tool>`, with the kind
// `execute` and the input `{server, tool, arguments}`. Either way the row says what the tool does in the room's words,
// running, done and failed, never falls back to "Ran a command." or "Did one other thing.", and never shows the name.
// Each goes through the real room over FakeTransport.
import { act } from "react";
import { stepLive, type StepLiveKey } from "@clauseo/components/lib/words-steps-live.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import { afterEach, expect, test } from "vitest";
import { agentText, header, reset, toolCall, userMessage } from "../../testing/stream-script.ts";
import type { ToolCallStatus, ToolKind, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { caseLawOf, toolNameOf, type CaseLaw } from "./tool-names.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "Find the latest on the Delhi High Court's order.");

type Call = { title: string; kind: ToolKind; input: object };

/** One tool's call as Claude sends it. */
const claude = (server: string, tool: string, args: object): Call => ({
  title: `mcp__${server}__${tool}`,
  kind: "other",
  input: args,
});
/** The same call as ChatGPT sends it. */
const codex = (server: string, tool: string, args: object): Call => ({
  title: `mcp.${server}.${tool}`,
  kind: "execute",
  input: { server, tool, arguments: args },
});

const call = ({ title, kind, input }: Call, status: ToolCallStatus): ViewEntry => ({
  tool_call: {
    ...toolCall(7, 290, status).tool_call!,
    kind,
    title,
    raw_input: { inline: JSON.stringify(input) },
  },
});

/** Each family's tool, what it reads as (tool-names.ts: toolNameOf's online, or caseLawOf's), its arguments, and its
 * words: running, done, tried. */
const FAMILIES: [
  family: string,
  as: "online" | CaseLaw,
  server: string,
  tool: string,
  args: object,
  running: StepLiveKey,
  done: StepKey,
  tried: StepKey,
][] = [
  [
    "browser guard",
    "online",
    "browser",
    "agent_browser_open",
    { url: "https://example.com" },
    "Looking online.",
    "Looked online.",
    "Tried to look online.",
  ],
  [
    "browser guard",
    "online",
    "browser",
    "agent_browser_click",
    { selector: "@e3" },
    "Looking online.",
    "Looked online.",
    "Tried to look online.",
  ],
  [
    "case law",
    "search",
    "case-law",
    "search_law",
    { query: "Section 138 NI Act" },
    "Searching case law.",
    "Searched case law.",
    "Tried to search case law.",
  ],
  [
    "case law",
    "search",
    "case-law",
    "read_document",
    { docid: 1 },
    "Searching case law.",
    "Searched case law.",
    "Tried to search case law.",
  ],
  [
    "case law",
    "search",
    "case-law",
    "citator",
    { docid: 1 },
    "Searching case law.",
    "Searched case law.",
    "Tried to search case law.",
  ],
  [
    "case law",
    "search",
    "case-law",
    "check_citations",
    { text: "AIR 1973 SC 1461" },
    "Searching case law.",
    "Searched case law.",
    "Tried to search case law.",
  ],
  [
    "case law",
    "credits",
    "case-law",
    "account",
    {},
    "Checking your case-law credits.",
    "Checked your case-law credits.",
    "Tried to check your case-law credits.",
  ],
  [
    "case law",
    "payment",
    "case-law",
    "recharge",
    { credits: 100 },
    "Getting a payment link for case-law credits.",
    "Got a payment link for case-law credits.",
    "Tried to get a payment link for case-law credits.",
  ],
];

const CASES = FAMILIES.flatMap(([family, as, server, tool, args, running, done, tried]) =>
  (
    [
      ["Claude", claude(server, tool, args)],
      ["ChatGPT", codex(server, tool, args)],
    ] as const
  ).map(([agent, c]) => ({ family, as, agent, c, running, done, tried })),
);

const shown = () => document.body.textContent ?? "";

test.each(CASES)(
  "$family under $agent ($c.title): the name maps to what the tool does",
  ({ c, as }) => {
    if (as === "online") {
      expect(toolNameOf(c.title)).toEqual({ as: "fetch" });
      expect(caseLawOf(c.title)).toBeNull();
    } else expect(caseLawOf(c.title)).toBe(as);
  },
);

test.each(CASES)(
  "$family under $agent ($c.title), running: the working line says '$running'",
  async ({ c, running }) => {
    h = await mountRoom();
    h.deliver([reset([ASKED, call(c, "in_progress")], header("running"))]);
    await h.settle();
    expect(h.one("working")!.textContent).toContain(stepLive("en", running));
    expect(shown()).not.toContain(stepLive("en", "Running a task."));
    expect(shown()).not.toContain(stepLive("en", "Working on it."));
    expect(shown()).not.toContain(c.title);
  },
);

test.each(
  CASES.flatMap((k) => [
    { ...k, status: "completed" as const, words: k.done },
    { ...k, status: "failed" as const, words: k.tried },
  ]),
)(
  "$family under $agent ($c.title), $status: its row says '$words'",
  async ({ c, status, words }) => {
    h = await mountRoom();
    h.deliver([reset([ASKED, call(c, status), agentText(8, 1, "The order is out.")])]);
    await h.settle();
    const line = h.one("step-group")!.querySelector<HTMLButtonElement>("button")!;
    await act(async () => line.click());
    await h.settle();
    const group = h.one("step-group")!.textContent;
    expect(group).toContain(steps("en", words));
    expect(group).not.toContain(steps("en", "Ran a command."));
    expect(group).not.toContain(steps("en", "Tried to run a command."));
    expect(group).not.toContain(steps("en", "Used a tool."));
    expect(group).not.toContain(steps("en", "Tried to use a tool."));
    expect(shown()).not.toContain(c.title);
  },
);
