// @vitest-environment happy-dom
// A step or a permission request the agent named only by a tool's or a permission's name ("SandboxNetworkAccess",
// "write_file", "mcp__browser__navigate", "fs.read", "ApplyPatch"): the room never shows the name. Each goes through the
// real room over FakeTransport, the prompt waiting on the lawyer and the settled group opened to its row; a known name
// reads as what Clauseo asks or did, any other as the plain ask or the kind's own line. A title in words stays.
import { act } from "react";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import type { StepKey } from "@clauseo/components/lib/words-steps.ts";
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  permission,
  reset,
  toolCall,
  userMessage,
} from "../../testing/stream-script.ts";
import type { ToolCallStatus, ToolKind, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { identifierLike, toolNameOf } from "./tool-names.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "Check the court's cause list for tomorrow.");
const named = (title: string, kind: ToolKind, status: ToolCallStatus): ViewEntry => ({
  tool_call: { ...toolCall(7, 290, status).tool_call!, kind, title },
});
const ASK = "Clauseo asks before going on.";

/** Each name, the kind it came with, the prompt's line beneath the ask (null: the ask alone), and its row once done. */
const NAMES: [title: string, kind: ToolKind, line: ThreadKey | null, done: StepKey][] = [
  [
    "SandboxNetworkAccess",
    "other",
    "Clauseo asks to let a command reach the internet.",
    "Let a task reach the internet.",
  ],
  ["write_file", "edit", "Clauseo asks to change a paper.", "Changed a paper."],
  ["mcp__browser__navigate", "fetch", "Clauseo asks to look online.", "Looked online."],
  ["fs.read", "read", "Clauseo asks to read a paper.", "Read the papers."],
  ["ApplyPatch", "edit", "Clauseo asks to change a paper.", "Changed a paper."],
  ["Bash", "execute", "Clauseo asks to run a task.", "Ran a task."],
  ["exec_command", "execute", "Clauseo asks to run a task.", "Ran a task."],
  // names Clauseo doesn't know: the plain ask, and the kind's own line
  ["mcp__docs__fetch", "other", null, "Did one other thing."],
  ["SomeNewPermission", "other", null, "Did one other thing."],
  ["future.tool:run", "other", null, "Did one other thing."],
];

const shown = () => document.body.textContent ?? "";

test.each(NAMES)(
  "a request named %s never shows the name: the prompt says what Clauseo asks, the working line waits on the lawyer",
  async (title, kind, line) => {
    h = await mountRoom();
    h.deliver([
      reset([ASKED, named(title, kind, "pending"), permission(8, 290)], header("needs_you")),
    ]);
    await h.settle();
    const prompt = h.one("permission")!;
    expect(prompt.textContent).toContain(ASK);
    if (line !== null) expect(prompt.textContent).toContain(th("en", line));
    expect(h.one("working")!.textContent).toContain(`${frame("en", "Needs your permission")}.`);
    expect(shown()).not.toContain(title);
  },
);

test.each(NAMES)(
  "a step named %s never shows the name: once done its row says what was done, or the kind's own line",
  async (title, kind, _line, done) => {
    h = await mountRoom();
    h.deliver([
      reset([
        ASKED,
        named(title, kind, "completed"),
        agentText(8, 1, "The list has two hearings."),
      ]),
    ]);
    await h.settle();
    const line = h.one("step-group")!.querySelector<HTMLButtonElement>("button")!;
    await act(async () => line.click());
    await h.settle();
    expect(h.one("step-group")!.textContent).toContain(steps("en", done));
    expect(shown()).not.toContain(title);
  },
);

test("a running step named SandboxNetworkAccess: the working line says what is being let through, never the name", async () => {
  h = await mountRoom();
  h.deliver([
    reset([ASKED, named("SandboxNetworkAccess", "other", "in_progress")], header("running")),
  ]);
  await h.settle();
  expect(h.one("working")!.textContent).toContain("Letting a task reach the internet.");
  expect(shown()).not.toContain("SandboxNetworkAccess");
});

test("a title in words stays as the agent sent it, on the prompt and on the row", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [ASKED, named("Read Notice to the bank.pdf", "read", "pending"), permission(8, 290)],
      header("needs_you"),
    ),
  ]);
  await h.settle();
  expect(h.one("permission")!.textContent).toContain("Read Notice to the bank.pdf");
  await h.unmount();
  h = await mountRoom();
  h.deliver([
    reset([ASKED, named("Run the checks", "other", "completed"), agentText(8, 1, "Done.")]),
  ]);
  await h.settle();
  await act(async () => h!.one("step-group")!.querySelector<HTMLButtonElement>("button")!.click());
  await h.settle();
  expect(h.one("step-group")!.textContent).toContain("Run the checks.");
});

test.each([
  ["SandboxNetworkAccess", true],
  ["write_file", true],
  ["mcp__browser__navigate", true],
  ["fs.read", true],
  ["ApplyPatch", true],
  ["read-file", true],
  ["readFile", true],
  ["mcp:fetch", true],
  ["Read", false],
  ["PDF", false],
  ["brief.txt", false],
  ["Lease deed.pdf", false],
  ["Read Notice to the bank.pdf", false],
  ["ls -la", false],
  ["notes/a.txt", false],
  ["", false],
] as const)("identifierLike(%j) is %s", (title, like) => {
  expect(identifierLike(title)).toBe(like);
});

test("a known name counts in any shape; words are never a name", () => {
  expect(toolNameOf("Bash")).toEqual({ as: "execute" });
  expect(toolNameOf("`exec_command`")).toEqual({ as: "execute" });
  expect(toolNameOf("mcp__browser__agent_browser_click")).toEqual({ as: "fetch" });
  expect(toolNameOf("mcp__docs__fetch")).toEqual({ as: null });
  expect(toolNameOf("Run the checks")).toBeUndefined();
  expect(toolNameOf("python3")).toBeUndefined();
});
