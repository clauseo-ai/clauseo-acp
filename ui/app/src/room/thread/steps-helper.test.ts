// A step that asks a helper (UC-906, UC-907; helper-words.ts): Claude's Agent or Task, said by its description, and
// ChatGPT's spawnAgent, said by its prompt, in the words of the done and failed row, counted as other in the folded
// line, and kept to the other-step words where the call gives no task. Pure, like steps.test.ts.
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
/** ChatGPT's spawn as codex-acp 2.2.2 reports it: kind other, titled spawnAgent, its input holding the prompt. */
const spawn = (prompt: string | null, status: ToolCallStatus = "completed") =>
  call("other", status, {
    title: "spawnAgent",
    raw_input: { inline: JSON.stringify(prompt === null ? {} : { prompt, model: "gpt-5" }) },
  });
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

  test("ChatGPT's spawn says its prompt as the task, done and failed", () => {
    expect(doneOf("en", spawn(TASK))).toBe("Asked a helper: Compare termination clauses.");
    expect(doneOf("en", spawn(TASK, "failed"))).toBe(
      "Tried to ask a helper: Compare termination clauses.",
    );
    expect(helperOf(spawn(TASK))).toBe(true);
  });

  test("a prompt's line breaks and controls are one plain line", () => {
    expect(helperTaskOf(spawn(`Compare\n\n  termination\tclauses\u202E`))).toBe(
      "Compare termination clauses",
    );
  });

  test("a title's full stop is not doubled, and its paths are cut to their names", () => {
    expect(doneOf("en", agent("Compare the clauses in /Users/rohan/Matter/Lease.pdf."))).toBe(
      "Asked a helper: Compare the clauses in Lease.pdf.",
    );
  });

  test("a long task is cut at 80 characters, the last one an ellipsis, with no full stop after it", () => {
    const long =
      "Compare the termination and renewal clauses in each supplier contract, one clause at a time, against the bundle";
    const task = helperTaskOf(agent(long));
    expect(task).toBe(`${long.slice(0, 79)}…`);
    expect(task).toHaveLength(80);
    expect(doneOf("en", agent(long))).toBe(`Asked a helper: ${long.slice(0, 79)}…`);
    expect(doneOf("en", spawn(long, "failed"))).toBe(
      `Tried to ask a helper: ${long.slice(0, 79)}…`,
    );
  });

  test("with no task, a helper says the other-step words, as the folded line counts it", () => {
    expect(doneOf("en", agent(""))).toBe("Did one other thing.");
    expect(doneOf("en", agent("", "failed"))).toBe("Tried one other thing.");
    expect(helperTaskOf(agent(""))).toBe("");
    expect(summaryOf("en", [agent("")])).toBe("Did 1 other thing.");
  });

  test("ChatGPT's spawn with no prompt keeps the other-step words", () => {
    expect(doneOf("en", spawn(null))).toBe(steps("en", "Did one other thing."));
    expect(doneOf("en", spawn("   "))).toBe(steps("en", "Did one other thing."));
    expect(doneOf("en", spawn(null, "failed"))).toBe(steps("en", "Tried one other thing."));
  });

  test("a spawnAgent title on a step of another kind is no helper", () => {
    const other = call("read", "completed", { title: "spawnAgent" });
    expect(helperOf(other)).toBe(false);
  });

  test("a step that is no helper, with the same kind and title, reads as before", () => {
    expect(doneOf("en", call("think", "completed", { title: TASK }))).toBe("Thought it through.");
    expect(helperOf(call("think", "completed", { tool_name: "Read" }))).toBe(false);
  });
});

describe("a helper in the folded line", () => {
  test("a helper counts as other, so the line says it did one other thing, not that it planned", () => {
    expect(groupKindOf(agent(TASK))).toBe("other");
    expect(groupKindOf(spawn(TASK))).toBe("other");
    expect(summaryOf("en", [agent(TASK)])).toBe("Did 1 other thing.");
    expect(summaryOf("en", [read("Lease.pdf"), read("Notice.pdf"), agent(TASK)])).toBe(
      "2 papers read. Did 1 other thing.",
    );
    expect(summaryOf("en", [read("Lease.pdf"), spawn(TASK)])).toBe(
      "1 paper read. Did 1 other thing.",
    );
  });

  test("a thought that is no helper still says it planned the work", () => {
    expect(summaryOf("en", [call("think")])).toBe("Planned the work.");
  });

  test("the row keeps the think glyph", () => {
    expect(drawnKindOf(agent(TASK))).toBe("think");
  });
});
