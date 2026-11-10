// @vitest-environment happy-dom
// Going offline mid-answer (D-050): the turn the host ends `failed` with `agent.offline` stops the working line, keeps
// the answer so far, says the offline words on its stop line (never the agent's transport text, which the host keeps
// out of the view) and offers Try again, which sends the same question in the same conversation.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  THREAD,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "What notice period does clause five set?");
const PARTIAL = agentText(20, 1, "Clause five says");

test("offline mid-answer: the spinner stops, the answer so far stays, the offline words and Try again show", async () => {
  h = await mountRoom();
  h.deliver([reset([ASKED, PARTIAL], header("running"))]);
  expect(h.one("working")?.textContent).toBe(frame("en", "Answering."));
  expect(h.one("answer-try-again")).toBeNull();
  // the host ends the turn offline
  h.deliver([
    upsert(step(30, { stop: "failed", failure: "agent.offline" })),
    upsert(header("idle")),
  ]);
  await h.settle();
  expect(h.one("working")).toBeNull();
  const stopped = h.one("turn-stopped");
  expect(stopped?.dataset["failure"]).toBe("agent.offline");
  expect(stopped?.textContent).toBe(errorWords("en", "agent.offline"));
  expect(stopped?.textContent).toBe(
    "You're offline, so the answer stopped. The answer so far is kept. Try again when you're back online.",
  );
  expect(h.host.textContent).toContain("Clause five says");
  expect(h.host.textContent).not.toContain("WebSockets");
  // Try again: the same question, as a new turn of this conversation
  await h.click(h.one("answer-try-again"));
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: "What notice period does clause five set?" } }] },
  ]);
});

test("a Codex thread's offline line is the same: the sentence names no agent", async () => {
  expect(errorWords("en", "agent.offline", { kind: "codex", name: "Codex" })).toBe(
    errorWords("en", "agent.offline"),
  );
});
