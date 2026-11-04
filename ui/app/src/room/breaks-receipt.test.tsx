// @vitest-environment happy-dom
// U10 when the break outruns the receipt: the host commits the question and its receipt together, but the receipt
// comes back as the command's answer and the turn over the stream, two ways that may arrive in either order (on
// Windows the e2e saw it). An agent that exits at once can end its turn in the view before send_prompt's answer is in,
// while the field still holds the question that answer is about to clear. The question still comes back into the field.
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  TURN,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { CommandOutcome, ViewEntry } from "../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const QUESTION = "Summarise the indemnity clause";
const field = () => h!.one("composer") as HTMLTextAreaElement;
const status = (s: "running" | "agent_exited"): ViewEntry => {
  const head = header();
  head.header!.status = s;
  return head;
};

test("the agent exits before send_prompt's answer is in: the answer so far stays, and the question comes back into the field once the answer clears it", async () => {
  // send_prompt's answer held until the test lets it go
  let answer: (outcome: CommandOutcome) => void = () => {};
  h = await mountRoom({
    dispatch: (command) =>
      command.send_prompt === undefined
        ? defaultOutcome(command)
        : new Promise<CommandOutcome>((resolve) => {
            answer = resolve;
          }),
  });
  h.deliver([reset([])]);
  await h.type(QUESTION);
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt")).toHaveLength(1);
  // on its way: the field keeps the question until the host's answer
  expect(field().value).toBe(QUESTION);
  // the whole turn over the stream first: the question, one chunk, the agent gone
  h.deliver([
    upsert(userMessage(5, 1, QUESTION)),
    upsert(agentText(6, 1, "Starting on the answer.")),
    upsert(status("running")),
  ]);
  await h.settle();
  h.deliver([
    upsert(step(9, { stop: "failed", failure: "agent.exited" })),
    upsert(status("agent_exited")),
  ]);
  await h.settle();
  expect(h.one("agent-stopped")?.dataset["code"]).toBe("agent.exited");
  // then send_prompt's answer
  answer({ prompt_accepted: { turn: TURN, message: uuid(1, 5) } });
  await h.settle();
  expect(h.one("message-agent")?.textContent).toBe("Starting on the answer.");
  expect(h.one("agent-stopped")?.dataset["code"]).toBe("agent.exited");
  expect(field().value).toBe(QUESTION);
  // asking again sends it as a new turn
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt").map((c) => c.content[0]?.text?.text)).toEqual([QUESTION, QUESTION]);
});
