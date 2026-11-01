// The thread's shape from its id list: exchanges and the agent's latest output, by prefix alone.
import { expect, test } from "vitest";
import { exchangesOf, latestOutput, latestQuestion, sameIds } from "./ids.ts";

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

test("the latest output skips questions, notices, markers and versions, and nothing else", () => {
  expect(latestOutput(ids)).toBe("agent_text:a2");
  expect(latestOutput([...ids, "notice:n", "marker:m", "version:v"])).toBe("agent_text:a2");
  expect(latestOutput([...ids, "tool_call:t2"])).toBe("tool_call:t2");
  expect(latestOutput(["user_message:q", "notice:n"])).toBeNull();
  // a new list (the view makes one on every add) is read afresh
  const grown = [...ids, "agent_text:a3"];
  expect(latestOutput(grown)).toBe("agent_text:a3");
});

test("the latest question, and id lists compared by their ids", () => {
  expect(latestQuestion(ids)).toBe("user_message:q2");
  expect(latestQuestion(["agent_text:a"])).toBeNull();
  expect(sameIds(["a", "b"], ["a", "b"])).toBe(true);
  expect(sameIds(["a", "b"], ["a"])).toBe(false);
  expect(sameIds(["a", "b"], ["a", "c"])).toBe(false);
});
