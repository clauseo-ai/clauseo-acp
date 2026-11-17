// A step that asks a helper (UC-906, UC-907; helper-words.ts): Claude's Agent or Task, and ChatGPT's spawn_agent, said by
// the helper's task in the words of the done and failed row, counted as other in the folded line, and kept to today's
// words where the call gives no task. Pure, like steps.test.ts.
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { describe, expect, test } from "vitest";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus, ToolKind } from "../../transport/protocol.ts";
import { helperOf, helperTaskOf } from "./helper-words.ts";
import { doneOf, drawnKindOf, groupKindOf, summaryOf } from "./steps.ts";

let n = 0;
/** A tool call of `kind`, with what the agent gave for it. */
function call(
  kind: ToolKind,
  status: ToolCallStatus = "completed",
  over: Partial<ToolCallEntry> = {},
): ToolCallEntry {
  n += 1;
  return { ...toolCall(400 + n, 900 + n, status).tool_call!, kind, title: "", ...over };
}
/** Claude's Agent tool as claude-agent-acp reports it: kind think, the helper's description as its title. */
const agent = (title: string, status: ToolCallStatus = "completed", name = "Agent") =>
  call("think", status, { title, tool_name: name });
const read = (title: string) => call("read", "completed", { title });

const TASK = "Compare termination clauses";

describe("a helper's step, done or failed", () => {
  test("Claude's Agent with a title says the task it was given", () => {
    expect(doneOf("en", agent(TASK))).toBe("Asked a helper: Compare termination clauses.");
  });

  test("a failed helper says what it tried, and a stopped one too", () => {
    expect(doneOf("en", agent(TASK, "failed"))).toBe(
      "Tried to ask a helper: Compare termination clauses.",
    );
    expect(doneOf("en", agent(TASK, "cancelled"))).toBe(
      "Tried to ask a helper: Compare termination clauses.",
    );
  });

  test("Claude's older name for the helper, Task, reads the same", () => {
    expect(doneOf("en", agent(TASK, "completed", "Task"))).toBe(
      "Asked a helper: Compare termination clauses.",
    );
    expect(doneOf("en", agent(TASK, "failed", "task"))).toBe(
      "Tried to ask a helper: Compare termination clauses.",
    );
  });

  test("ChatGPT's spawn_agent with a task says it the same way", () => {
    expect(
      doneOf("en", call("other", "completed", { title: TASK, tool_name: "spawn_agent" })),
    ).toBe("Asked a helper: Compare termination clauses.");
  });

  test("a title's full stop is not doubled, and its paths are cut to their names", () => {
    expect(doneOf("en", agent("Compare the clauses in /Users/rohan/Matter/Lease.pdf."))).toBe(
      "Asked a helper: Compare the clauses in Lease.pdf.",
    );
  });

  test("a long task is cut at 80 characters, the last one an ellipsis", () => {
    const long =
      "Compare the termination and renewal clauses in each supplier contract, one clause at a time, against the bundle";
    const task = helperTaskOf(agent(long));
    expect(task).toBe(`${long.slice(0, 79)}…`);
    expect(task).toHaveLength(80);
    expect(doneOf("en", agent(long))).toBe(`Asked a helper: ${long.slice(0, 79)}….`);
  });

  test("with an empty title, today's words stay: a thought it went through, or one tried", () => {
    expect(doneOf("en", agent(""))).toBe("Thought it through.");
    expect(doneOf("en", agent("", "failed"))).toBe("Tried to think it through.");
    expect(helperTaskOf(agent(""))).toBe("");
  });

  test("a title that is only a tool's name is no task: ChatGPT's spawnAgent keeps its words", () => {
    const spawn = call("other", "completed", { title: "spawnAgent", tool_name: "spawn_agent" });
    expect(helperOf(spawn)).toBe(true);
    expect(doneOf("en", spawn)).toBe(steps("en", "Did one other thing."));
  });

  test("a step that is no helper, with the same kind and title, reads as before", () => {
    expect(doneOf("en", call("think", "completed", { title: TASK }))).toBe("Thought it through.");
    expect(helperOf(call("think", "completed", { tool_name: "Read" }))).toBe(false);
  });
});

describe("a helper in the folded line", () => {
  test("a helper counts as other, so the line says it did one other thing, not that it planned", () => {
    expect(groupKindOf(agent(TASK))).toBe("other");
    expect(summaryOf("en", [agent(TASK)])).toBe("Did 1 other thing.");
    expect(summaryOf("en", [read("Lease.pdf"), read("Notice.pdf"), agent(TASK)])).toBe(
      "2 papers read. Did 1 other thing.",
    );
  });

  test("a thought that is no helper still says it planned the work", () => {
    expect(summaryOf("en", [call("think")])).toBe("Planned the work.");
  });

  test("the row keeps the think glyph", () => {
    expect(drawnKindOf(agent(TASK))).toBe("think");
  });
});
