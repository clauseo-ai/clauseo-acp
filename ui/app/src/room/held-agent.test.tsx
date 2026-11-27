// @vitest-environment happy-dom
// The two refusals of a switch to the held plan (host #1349), in the real room over FakeTransport: a message refused
// with thread.background_work keeps its words in the box, the hold and its line; a switch whose handover
// was too long for the other agent fails its turn, its step's row says so and the question comes back into
// the field. The hold itself, Back and the send's agent and config: pickers-room.test.tsx.
import { expect, test, afterEach } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { AgentModels, ConfigOption, ViewEntry } from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import type { AgentSummary } from "./host-next.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const BUSY =
  "Your message wasn't sent, because the agent in this conversation is still finishing something it started. Your message and your choice of agent are kept. Answer anything it's asking you, or wait a minute, then send again.";
const TOO_LONG =
  "The other agent didn't get your message, because this conversation is too long to hand over to it. The conversation stays with its agent. To ask the other agent, start a new conversation with it and send your message there. To stay, send it here again.";

const level = (id: string) => ({ id, name: id[0]!.toUpperCase() + id.slice(1) });
const LMH = ["low", "medium", "high"].map(level);
const models = (effort: string, ids: string[]): AgentModels => ({
  model_option: "model",
  effort_option: effort,
  current_model: ids[0]!,
  current_effort: "medium",
  models: ids.map((id) => ({ id, name: id, efforts: LMH, default_effort: "medium" })),
});
const AGENTS: AgentSummary[] = [
  { id: "claude-acp", kind: "claude", models: models("effort", ["opus"]) },
  { id: "codex-acp", kind: "codex", models: models("reasoning_effort", ["gpt-6.2-sol"]) },
];
const select = (id: string, category: string, current: string): ConfigOption =>
  ({
    id,
    name: id,
    category,
    kind: { select: { current_value: current, options: [{ value: current, name: current }] } },
  }) as ConfigOption;

/** Claude's conversation, idle after one answer. */
const HEAD: ViewEntry = {
  header: {
    ...header("idle").header!,
    agent: "claude-acp",
    config_options: [select("model", "model", "opus"), select("effort", "thought_level", "medium")],
  },
};
const ASKED = [
  userMessage(5, 1, "Summarise the notice."),
  agentText(6, 1, "It gives sixty days."),
  step(7),
];
const FOLLOW_UP = "Check the possession notice against it.";

const planWords = () => h!.host.querySelector(".room-status-plan")?.textContent ?? "";
const field = () => h!.one("composer") as HTMLTextAreaElement;

/** Holds ChatGPT from the conversation's plan menu. */
async function holdChatGPT() {
  // the plan words become a menu once the host's agents are in, and its list comes with its chunk
  const find = () =>
    [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
      (o) => o.textContent === "ChatGPT",
    );
  for (let i = 0; i < 20 && find() === undefined; i += 1) {
    if (document.querySelector('[role="listbox"]') === null)
      await h!.click(h!.host.querySelector(".room-status-plan"));
    await h!.settle();
  }
  const option = find();
  await h!.click(option);
  await h!.settle();
  expect(planWords()).toBe("ChatGPT from your next message");
}

async function ask(text: string) {
  await h!.type(text);
  await h!.key({ key: "Enter", keyCode: 13 });
  await h!.settle();
}

test("thread.background_work: the words stay in the box, the hold stays, and the line says why and what to do", async () => {
  h = await mountRoom({
    next: fakeNext({ matters: () => [], agents: AGENTS, parts: ["agents"] }),
    dispatch: (command) => {
      if (command.send_prompt?.agent !== undefined)
        throw { code: "thread.background_work", message: "raw host text", retryable: true };
      return defaultOutcome(command);
    },
  });
  h.deliver([reset(ASKED, HEAD)]);
  await h.settle();
  await holdChatGPT();
  await ask(FOLLOW_UP);
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp" });
  expect(h.one("error")?.textContent).toContain(BUSY);
  expect(h.host.textContent).not.toContain("raw host text");
  expect(field().value).toBe(FOLLOW_UP);
  expect(planWords()).toBe("ChatGPT from your next message");
  expect(h.host.querySelector(".room-status-back")?.textContent).toBe("Back to Claude");
});

test("prompt.handoff_too_long: the stopped line says the conversation stays, and the question comes back", async () => {
  // the follow-up's own turn, as the host gives each prompt one
  const turn = uuid(2, 4);
  h = await mountRoom({
    next: fakeNext({ matters: () => [], agents: AGENTS, parts: ["agents"] }),
    dispatch: (command) =>
      command.send_prompt === undefined
        ? defaultOutcome(command)
        : { prompt_accepted: { turn, message: uuid(2, 5) } },
  });
  h.deliver([reset(ASKED, HEAD)]);
  await h.settle();
  await holdChatGPT();
  await ask(FOLLOW_UP);
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp" });
  // the host took the message; its turn failed before anything reached ChatGPT, and no move happened
  const status = (s: "running" | "idle"): ViewEntry => ({ header: { ...HEAD.header!, status: s } });
  const followUp = userMessage(8, 2, FOLLOW_UP);
  followUp.user_message!.turn = turn;
  h.deliver([upsert(followUp), upsert(status("running"))]);
  await h.settle();
  h.deliver([
    upsert(step(9, { turn, stop: "failed", failure: "prompt.handoff_too_long" })),
    upsert(status("idle")),
  ]);
  await h.settle();
  // the step's row says it (the composer's stopped line is only for what Restart mends), and the question is back
  const rows = [...h.host.querySelectorAll('[data-testid="turn-stopped"]')];
  expect(rows.at(-1)?.textContent).toContain(TOO_LONG);
  expect(h.one("agent-stopped")).toBeNull();
  expect(field().value).toBe(FOLLOW_UP);
  // the hold ended with the accepted send: the line names the conversation's own plan
  expect(planWords()).toBe("Uses your Claude plan");
});
