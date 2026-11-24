// @vitest-environment happy-dom
// Claude's code mode on an answer (code-mode.ts), in the real room: a quiet "Code mode" tag, heard as "Claude used code
// mode for this answer.". An ended answer shows it from its own step (StepEntry.code_mode), so every past answer that
// used code mode keeps it, a reload too; the running answer shows it after the model and effort on the status line
// from the moment the header's code_mode flips. Never in the composer at rest, never before the flag, never on an
// answer whose step says nothing of it.
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { HeaderEntry, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "List the notice period in each of the six supply agreements.");
const PARTIAL = agentText(20, 1, "Reading the agreements.");
const ANSWER = agentText(20, 1, "Five of the six allow termination for convenience.");
/** An earlier exchange, its own turn, which used code mode. */
const EARLIER = uuid(2, 4);
const OLDER_ASKED: ViewEntry = {
  user_message: {
    ...userMessage(1, 2, "Which agreements are in this folder?").user_message!,
    turn: EARLIER,
  },
};
const OLDER: ViewEntry = {
  agent_text: { ...agentText(2, 2, "Six supply agreements.").agent_text!, turn: EARLIER },
};
const OLDER_STEP: ViewEntry = { step: { ...step(3, { code_mode: true }).step!, turn: EARLIER } };

const head = (status: "idle" | "running", more: Partial<HeaderEntry> = {}): ViewEntry => {
  const base = header(status);
  return { header: { ...base.header!, ...more } };
};

const SAID = "Claude used code mode for this answer.";
const lineTag = () => h!.host.querySelector(".room-status-mark");
const footTags = () => h!.all("answer-code-mode");
const composer = () => h!.host.querySelector("[data-composer]")?.textContent ?? "";
/** Whether the tag stands at the foot of the answer of each exchange, oldest first. */
const tagged = () =>
  h!.all("answer-actions").map((a) => a.querySelector('[data-testid="answer-code-mode"]') !== null);

test("a running answer shows no tag until the host's code_mode turns true, then the tag after the effort", async () => {
  h = await mountRoom();
  await h.deliverAwaited([reset([ASKED, PARTIAL], head("running"))]);
  await h.settle();
  expect(h.one("cancel")).not.toBeNull();
  expect(lineTag()).toBeNull();
  expect(composer()).not.toContain("Code mode");
  h.deliver([upsert(head("running", { code_mode: false }))]);
  await h.settle();
  expect(lineTag()).toBeNull();
  // the first script call: the host sets the flag
  h.deliver([upsert(head("running", { code_mode: true }))]);
  await h.settle();
  expect(lineTag()?.textContent).toBe(`Code mode${SAID}`);
  expect(lineTag()?.querySelector(".room-tag")?.textContent).toBe("Code mode");
  expect(lineTag()?.querySelector(".room-tag")?.getAttribute("aria-hidden")).toBe("true");
  expect(footTags()).toHaveLength(0);
});

test("once the answer ends the tag leaves the status line and stands at its foot, from its step", async () => {
  h = await mountRoom();
  await h.deliverAwaited([reset([ASKED, PARTIAL], head("running", { code_mode: true }))]);
  await h.settle();
  expect(lineTag()).not.toBeNull();
  h.deliver([
    reset([ASKED, ANSWER, step(30, { code_mode: true })], head("idle", { code_mode: true })),
  ]);
  await h.settle();
  // the composer at rest says nothing of it
  expect(lineTag()).toBeNull();
  expect(composer()).not.toContain("Code mode");
  const tags = footTags();
  expect(tags).toHaveLength(1);
  expect(tags[0]!.textContent).toBe(`Code mode${SAID}`);
  expect(tags[0]!.closest('[data-testid="answer-actions"]')).not.toBeNull();
});

test("past answers keep their tag, a reload too; an answer whose step says false or nothing shows none", async () => {
  h = await mountRoom();
  const view = [OLDER_ASKED, OLDER, OLDER_STEP, ASKED, ANSWER, step(30)];
  await h.deliverAwaited([reset(view, head("idle"))]);
  await h.settle();
  expect(tagged()).toEqual([true, false]);
  // a new turn starts: the header's flag is cleared, the past answer keeps its tag, the line says nothing yet
  h.deliver([upsert(head("running"))]);
  await h.settle();
  expect(tagged()).toEqual([true, false]);
  expect(lineTag()).toBeNull();
  await h.unmount();
  // a reload: the same view from the host, the tag still on the answer that used code mode
  h = await mountRoom();
  await h.deliverAwaited([reset(view, head("idle"))]);
  await h.settle();
  expect(tagged()).toEqual([true, false]);
  await h.unmount();
  h = await mountRoom();
  // the header's flag alone, at rest, puts no tag on an answer whose step says false
  await h.deliverAwaited([
    reset([ASKED, ANSWER, step(30, { code_mode: false })], head("idle", { code_mode: true })),
  ]);
  await h.settle();
  expect(footTags()).toHaveLength(0);
  expect(lineTag()).toBeNull();
});
