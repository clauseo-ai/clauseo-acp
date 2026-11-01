// @vitest-environment happy-dom
// The Markdown renderer's own chunk (markdown.ts): a reopened thread's answers stand empty while the chunk loads, never
// showing their raw syntax, and show in one commit once it arrives.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { loadMarkdown } from "./markdown.ts";

// the chunk arrives when the test says
const gate = vi.hoisted(() => {
  let open: () => void = () => {};
  const opened = new Promise<void>((resolve) => (open = resolve));
  return { open: () => open(), opened };
});
vi.mock("@clauseo/components/markdown/markdown.tsx", async (load) => {
  await gate.opened;
  return load();
});

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

test("a reopened thread's answers show once the renderer's chunk arrives, in one commit, and never as raw syntax", async () => {
  const phases: string[] = [];
  h = await mountRoom({ waitForRenderer: false, onRender: (_id, phase) => phases.push(phase) });
  await h.deliverAwaited([
    reset([
      userMessage(5, 1, "What does clause nine say?"),
      agentText(6, 1, "It needs **notice in writing**."),
      step(7),
      userMessage(8, 2, "And clause ten?"),
      agentText(9, 2, "Clause ten is *silent*."),
    ]),
  ]);
  const answers = () => h!.all("message-agent");
  // the rows stand, empty: the question and everything else around them show
  expect(answers()).toHaveLength(2);
  expect(answers().map((a) => a.textContent)).toEqual(["", ""]);
  expect(h.host.textContent).not.toContain("**");
  expect(h.all("message-user")).toHaveLength(2);
  phases.length = 0;
  gate.open();
  await act(async () => void (await loadMarkdown()));
  await h.settle();
  expect(phases).toHaveLength(1);
  expect(answers().map((a) => a.textContent)).toEqual([
    "It needs notice in writing.",
    "Clause ten is silent.",
  ]);
  expect(answers()[0]?.querySelector("strong")?.textContent).toBe("notice in writing");
  expect(answers()[1]?.querySelector("em")?.textContent).toBe("silent");
});
