// The line a step says for how its turn ended: the lawyer's stop, a step the lawyer didn't allow, a step Clauseo
// stopped for files outside the matter, a turn too long for the model to read, and the limits.
import { expect, test } from "vitest";
import { step } from "../../testing/stream-script.ts";
import { stopLine } from "./stops.ts";

const stopped = (
  stop: "cancelled" | "not_allowed" | "outside_matter" | "too_long" | "end_turn",
) => {
  const entry = step(1).step;
  if (entry === undefined) throw new Error("no step");
  return { ...entry, stop };
};

test("a step the lawyer didn't allow says so, and that the answer so far is kept", () => {
  expect(stopLine(stopped("not_allowed"))).toEqual({
    kind: "thread",
    key: "You didn't allow a request, so the answer stopped. The answer so far is kept.",
  });
});

test("a step Clauseo stopped, for files outside the matter, names the agent, or the AI where none is known", () => {
  expect(stopLine(stopped("outside_matter"), { kind: "codex", name: "codex-acp" })).toEqual({
    kind: "thread",
    key: "Clauseo stopped the answer: {name} tried to open files outside this matter. The answer so far is kept.",
    slots: { name: "ChatGPT" },
  });
  expect(stopLine(stopped("outside_matter"), { kind: undefined, name: " Gemini " })).toMatchObject({
    slots: { name: "Gemini" },
  });
  for (const agent of [undefined, { kind: undefined, name: null }] as const)
    expect(stopLine(stopped("outside_matter"), agent)).toEqual({
      kind: "thread",
      key: "Clauseo stopped the answer: the AI tried to open files outside this matter. The answer so far is kept.",
    });
});

test("a turn too long for the model to read names the agent, Claude or ChatGPT, or the AI where none is known", () => {
  expect(stopLine(stopped("too_long"), { kind: "claude", name: "claude-acp" })).toEqual({
    kind: "thread",
    key: "That was too long for {name} to read at once. Shorten your question, or ask it in a new conversation. Your question and the answer so far are kept.",
    slots: { name: "Claude" },
  });
  expect(stopLine(stopped("too_long"), { kind: "codex", name: "codex-acp" })).toMatchObject({
    slots: { name: "ChatGPT" },
  });
  expect(stopLine(stopped("too_long"), { kind: undefined, name: " Gemini " })).toMatchObject({
    slots: { name: "Gemini" },
  });
  for (const agent of [undefined, { kind: undefined, name: null }] as const)
    expect(stopLine(stopped("too_long"), agent)).toEqual({
      kind: "thread",
      key: "That was too long for the AI to read at once. Shorten your question, or ask it in a new conversation. Your question and the answer so far are kept.",
    });
  // a stop, never a failure: no error code
  expect(stopLine(stopped("too_long"))?.code).toBeUndefined();
});

test("the lawyer's own stop keeps its line, and a finished answer says nothing", () => {
  expect(stopLine(stopped("cancelled"))).toEqual({
    kind: "thread",
    key: "You stopped the answer. The answer so far is kept.",
  });
  expect(stopLine(stopped("end_turn"))).toBeNull();
});
