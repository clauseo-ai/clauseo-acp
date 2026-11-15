// A step Claude's own checks refused, in plain words (#plain-refusal): the refusal texts are Claude Code 0.3.293's
// own (claude-agent-sdk-linux-x64's package/claude: its `fnr`, `f2t` and deny rule messages), as claude-agent-acp
// 0.89.1 sends them: the result in the raw output as a JSON string, and fenced in the content.
import { expect, test } from "vitest";
import { refusalWords } from "@clauseo/components/lib/words-refusals.hi.ts";
import "@clauseo/components/lib/words-hi.ts";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus } from "../../transport/protocol.ts";
import { lookedOf, refusalOf, refusalWhy, type Looked } from "./refusals.ts";
import { whyOf } from "./task-words.ts";

/** Claude Code's block message (`fnr`) for a reason, its tail as the binary has it. */
const blocked = (reason: string): string =>
  `Permission for this action was denied by the Claude Code auto mode classifier. Reason: ${reason}. If you have ` +
  "other tasks that don't depend on this action, continue working on those. If you believe this capability is " +
  "essential to complete the user's request, first try a safer method. Get as much of the rest of the task done " +
  "as you can, then STOP and explain to the user what you were trying to do and why you need this permission. " +
  "Let the user decide how to proceed.";

/** A failed call whose result is `text`, as claude-agent-acp reports it: raw output and fenced content. */
const refused = (text: string, status: ToolCallStatus = "failed"): ToolCallEntry => ({
  ...toolCall(7, 1, status).tool_call!,
  kind: "read",
  raw_output: { inline: JSON.stringify(text) },
  content: [{ text: { text: { inline: `\`\`\`\n${text}\n\`\`\`` } } }],
});

const CHECK = "Claude's built-in safety check stopped this";
const CARRY = "Nothing was changed, and Claude carries on another way.";

test("a safety check block reads its rule's name in plain words, never the raw text", () => {
  const why = refusalWhy(
    "en",
    refused(blocked("[Unauthorized Access] Reading ~/.ssh is outside the task")),
  );
  expect(why).toBe(
    `${CHECK}: it looked like reaching files or systems Claude wasn't asked to use. ${CARRY}`,
  );
  // and in Hindi, once the Hindi chunk is in
  expect(refusalWhy("hi", refused(blocked("[Unauthorized Access] x")))).toMatch(
    /^Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: /,
  );
  // what the row showed before: the raw text, cut at 120 characters
  expect(whyOf(refused(blocked("[Unauthorized Access] x")))).toMatch(
    /^Permission for this action was denied/,
  );
});

test("every rule name Claude Code's binary names, and the ones seen in Clauseo, has its plain words", () => {
  const names: [string, Looked][] = [
    // seen in Clauseo's sessions
    ["Unauthorized Access", "access"],
    ["Undermining Oversight", "oversight"],
    // the binary's own examples, in its classifier prompt
    ["Data Exfiltration", "outside_data"],
    ["Logging Audit Tampering", "oversight"],
    ["Irreversible Deletion general", "deletion"],
    ["Irreversible Local Destruction", "deletion"],
    ["Instruction Poisoning", "instructions"],
    ["Auto-Mode Bypass", "oversight"],
    ["Self-Modification", "self"],
    ["Tmux Self Drive", "self"],
    ["Synthetic Input Self Drive", "self"],
    ["Credential Exploration", "credentials"],
    ["External System Writes", "outside_change"],
  ];
  for (const [name, looked] of names) {
    expect(lookedOf(name), name).toBe(looked);
    expect(refusalOf(refused(blocked(`[${name}] because`))), name).toEqual({
      kind: "check",
      looked,
    });
  }
  // a word inside another ("source", "force") is no rule's
  expect(lookedOf("Force Source Rewrite")).toBe("outside_change");
});

test("an unknown rule name, or a reason with none, gets the general line", () => {
  const general = `${CHECK}. ${CARRY}`;
  expect(refusalWhy("en", refused(blocked("[Quantum Flux] odd")))).toBe(general);
  expect(refusalWhy("en", refused(blocked("This reads outside the matter")))).toBe(general);
  // the shorter form Claude Code counts as the same block
  expect(
    refusalWhy("en", refused("Permission for this action has been denied. Reason: [Mystery Rule]")),
  ).toBe(general);
});

test("a check that gave no verdict says it couldn't look, and that nothing ran", () => {
  const line =
    "Claude's built-in safety check couldn't look at this just now, so it didn't run. Nothing was changed.";
  for (const text of [
    "Bash was not reviewed: the auto mode classifier transcript exceeded its context window, and no permission " +
      "prompt is raised for this tool. This is not a judgment that the action is unsafe; the same call will hit " +
      "the same limit until the conversation is shorter.",
    "Classifier request refused. This is not a judgment that the action is unsafe. Retrying it will hit the same " +
      "refusal, so don't rewrite or rework the action to get around this.",
    "The API told auto mode's safety classifier (claude-sonnet) to wait 2 minutes from now, so auto mode cannot " +
      "determine the safety of Bash until then.",
  ])
    expect(refusalWhy("en", refused(text)), text).toBe(line);
});

test("Clauseo's own deny rules say the place is off limits, without the path or the rule", () => {
  const line =
    "This tried to reach somewhere Clauseo keeps off limits here, such as Clauseo's own app data or another matter. Nothing was changed.";
  for (const text of [
    "Permission to read /Users/lawyer/Library/Application Support/Clauseo/agents/state.json has been denied.",
    "Permission to edit /Users/lawyer/Documents/Sharma/.clauseo/matter.json has been denied.",
    "Permission to use Bash with command cat ~/Documents/Other/notes.txt has been denied.",
  ]) {
    expect(refusalWhy("en", refused(text)), text).toBe(line);
    expect(whyOf(refused(text))).toMatch(/^Permission to/);
  }
});

test("the content alone, as the first result update has it, is read too; an array result's text as well", () => {
  const text = blocked("[Data Exfiltration] upload");
  const { raw_output: _, ...contentOnly } = refused(text);
  expect(refusalOf(contentOnly)).toEqual({ kind: "check", looked: "outside_data" });
  const blocks = {
    ...refused(text),
    raw_output: { inline: JSON.stringify([{ type: "text", text }]) },
  };
  expect(refusalOf(blocks)).toEqual({ kind: "check", looked: "outside_data" });
});

test("anything else is no refusal: another failure, a finished call, a task whose output says classifier", () => {
  expect(refusalOf(refused(blocked("[Data Exfiltration] x"), "completed"))).toBeNull();
  expect(refusalOf(refused("Error: the file is password-protected"))).toBeNull();
  expect(
    refusalOf(refused("Traceback\nValueError: the classifier in auto mode failed")),
  ).toBeNull();
  expect(
    refusalOf({ ...refused("x"), raw_output: { inline: JSON.stringify({ stderr: "boom" }) } }),
  ).toBeNull();
});

test("no refusal line says I, a banned word or the agent's words for the check; each has its Hindi", () => {
  for (const en of Object.keys(refusalWords)) {
    expect(en).not.toMatch(
      /\bI\b|[\u2013\u2014]|\b(step|git|token|session|terminal|command|safe|agent|classifier|auto mode|rule)\b/i,
    );
    const hi = refusalWords[en]!;
    expect(hi, en).toMatch(/।$/);
    expect(hi, en).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)|क्लॉड|क्लॉज़ियो/);
    expect(hi, en).toMatch(en.startsWith("Claude") ? /^Claude / : /Clauseo/);
  }
});
