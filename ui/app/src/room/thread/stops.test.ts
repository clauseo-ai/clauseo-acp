// The line a step says for how its turn ended: the lawyer's stop, a step the lawyer didn't allow, and the limits.
import { expect, test } from "vitest";
import { step } from "../../testing/stream-script.ts";
import { stopLine } from "./stops.ts";

const stopped = (stop: "cancelled" | "not_allowed" | "end_turn") => {
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

test("the lawyer's own stop keeps its line, and a finished answer says nothing", () => {
  expect(stopLine(stopped("cancelled"))).toEqual({
    kind: "thread",
    key: "You stopped the answer. The answer so far is kept.",
  });
  expect(stopLine(stopped("end_turn"))).toBeNull();
});
