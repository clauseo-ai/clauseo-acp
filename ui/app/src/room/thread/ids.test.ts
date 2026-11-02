// The thread's shape from its id list: exchanges and the agent's latest output, by prefix alone.
import { expect, test } from "vitest";
import { exchangesOf, latestQuestion, sameIds } from "./ids.ts";

const ids = [
  "agent_text:a0",
  "user_message:q1",
  "agent_text:a1",
  "tool_call:t1",
  "step:s1",
  "user_message:q2",
  "agent_text:a2",
];

test("exchanges start at each question; a run before the first question is its own", () => {
  expect(exchangesOf(ids)).toEqual([
    ["agent_text:a0"],
    ["user_message:q1", "agent_text:a1", "tool_call:t1", "step:s1"],
    ["user_message:q2", "agent_text:a2"],
  ]);
  expect(exchangesOf([])).toEqual([]);
});

test("the latest question, and id lists compared by their ids", () => {
  expect(latestQuestion(ids)).toBe("user_message:q2");
  expect(latestQuestion(["agent_text:a"])).toBeNull();
  expect(sameIds(["a", "b"], ["a", "b"])).toBe(true);
  expect(sameIds(["a", "b"], ["a"])).toBe(false);
  expect(sameIds(["a", "b"], ["a", "c"])).toBe(false);
});
