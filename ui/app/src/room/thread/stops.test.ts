// The line a step says for how its turn ended: the lawyer's stop, a step the lawyer didn't allow, a step Clauseo
// stopped for files outside the matter, and the limits.
import { expect, test } from "vitest";
import { step } from "../../testing/stream-script.ts";
import { stopLine } from "./stops.ts";

const stopped = (stop: "cancelled" | "not_allowed" | "outside_matter" | "end_turn") => {
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

test("the lawyer's own stop keeps its line, and a finished answer says nothing", () => {
  expect(stopLine(stopped("cancelled"))).toEqual({
    kind: "thread",
    key: "You stopped the answer. The answer so far is kept.",
  });
  expect(stopLine(stopped("end_turn"))).toBeNull();
});
