// @vitest-environment happy-dom
// The Markdown renderer's own chunk (markdown.ts): the thread opens only once the chunk is in, so a reopened thread's
// rows never render, settle or page on answers still waiting for it, and its answers show at once, never as raw syntax.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { loadThread } from "./chunk.ts";

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

const RESET = (hasOlder: boolean) => {
  const window = reset([
    userMessage(5, 1, "What does clause nine say?"),
    agentText(6, 1, "It needs **notice in writing**."),
    step(7),
    userMessage(8, 2, "And clause ten?"),
    agentText(9, 2, "Clause ten is *silent*."),
  ]);
  window.reset!.view_window.has_older = hasOlder;
  return window;
};

test("a Reset that arrives before the renderer's chunk shows nothing and asks nothing; the thread opens once the chunk is in", async () => {
  const phases: string[] = [];
  h = await mountRoom({
    waitForRenderer: false,
    query: () => ({ entries: [] }),
    onRender: (_id, phase) => phases.push(phase),
  });
  const pages = () =>
    h!.transport
      .only("query")
      .filter((c) => typeof c.query === "object" && "page_entries" in c.query);
  // list_threads has answered; the chunk hasn't: the thread isn't open, so the Reset finds no one watching
  await h.deliverAwaited([RESET(true)]);
  await h.settle();
  expect(h.transport.calls.filter((c) => c.call === "subscribe")).toEqual([]);
  expect(h.all("message-agent")).toHaveLength(0);
  expect(h.all("message-user")).toHaveLength(0);
  expect(h.host.textContent).not.toContain("**");
  expect(pages()).toEqual([]);
  gate.open();
  await act(async () => void (await loadThread()));
  await h.settle();
  expect(h.transport.calls.filter((c) => c.call === "subscribe")).toHaveLength(1);
  phases.length = 0;
  // without older entries, so no page goes in after it
  await h.deliverAwaited([RESET(false)]);
  // one commit, the answers' content in it: nothing suspends, nothing shows empty first
  expect(phases).toHaveLength(1);
  const answers = h.all("message-agent");
  expect(answers.map((a) => a.textContent)).toEqual([
    "It needs notice in writing.",
    "Clause ten is silent.",
  ]);
  expect(answers[0]?.querySelector("strong")?.textContent).toBe("notice in writing");
  expect(answers[1]?.querySelector("em")?.textContent).toBe("silent");
});
