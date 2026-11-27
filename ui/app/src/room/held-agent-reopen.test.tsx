// @vitest-environment happy-dom
// A held plan's switch waiting in the queue survives leaving the conversation (held-agent.ts, the room's holds by
// thread): opened again while Claude still answers, the line shows ChatGPT with its model and effort, and a new
// message goes to ChatGPT with ChatGPT's picks. With no switch waiting nothing changes; once the header names ChatGPT
// the line reads as usual; a switching message removed from the queue ends the hold.
import { afterEach, expect, test } from "vitest";
import { header, reset, upsert, userMessage, uuid } from "../testing/stream-script.ts";
import type {
  AgentModels,
  ConfigOption,
  ThreadId,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { fakeNext } from "./fake-next.ts";
import type { AgentSummary } from "./host-next.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const T1 = uuid(1, 1);
const T2 = uuid(2, 1);
const QUEUED = uuid(7, 4);

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

const thread = (id: ThreadId, title: string): ThreadSummary => ({
  thread: id,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status: "running",
  title,
});

const headerOf = (id: ThreadId, status: "idle" | "running", agent = "claude-acp"): ViewEntry => ({
  header: {
    ...header(status).header!,
    thread: id,
    agent,
    config_options: [select("model", "model", "opus"), select("effort", "thought_level", "medium")],
  },
});

const CHATGPT = { model: "gpt-6.2-sol", reasoning_effort: "medium" };
const planWords = () => h!.host.querySelector(".room-status-plan")?.textContent ?? "";
const said = (id: string) => h!.one(`pick-${id}`)?.textContent?.trim() ?? null;
const rowOf = (title: string) =>
  h!.all("conversation").find((r) => r.querySelector(".room-conv-title")?.textContent === title);

async function ask(text: string) {
  await h!.type(text);
  await h!.key({ key: "Enter", keyCode: 13 });
  await h!.settle();
}

async function holdChatGPT() {
  const find = () =>
    [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
      (o) => o.textContent === "ChatGPT",
    );
  for (let i = 0; i < 20 && find() === undefined; i += 1) {
    if (document.querySelector('[role="listbox"]') === null)
      await h!.click(h!.host.querySelector(".room-status-plan"));
    await h!.settle();
  }
  await h!.click(find());
  await h!.settle();
  expect(planWords()).toBe("ChatGPT from your next message");
}

/** Two conversations, Claude answering in both; "Indemnity" (T2) opens first. */
async function room() {
  h = await mountRoom({
    threads: [thread(T1, "Lease notice"), thread(T2, "Indemnity")],
    next: fakeNext({ matters: () => [], agents: AGENTS, parts: ["agents"] }),
    dispatch: (command) =>
      command.send_prompt === undefined
        ? defaultOutcome(command)
        : { prompt_accepted: { turn: QUEUED, message: uuid(7, 5) } },
  });
  h.deliverOn(`thread:${T2}`, [reset([], headerOf(T2, "running"))]);
  await h.settle();
}

/** Leaves for "Lease notice" and comes back to "Indemnity". */
async function reopen() {
  await h!.click(rowOf("Lease notice"));
  h!.deliverOn(`thread:${T1}`, [reset([], headerOf(T1, "idle"))]);
  await h!.settle();
  expect(planWords()).toBe("Uses your Claude plan");
  await h!.click(rowOf("Indemnity"));
  await h!.settle();
}

/** ChatGPT held in "Indemnity" and a message sent behind Claude's answer, which the host queues. */
async function queuedSwitch() {
  await room();
  await holdChatGPT();
  await ask("Now check the reply against clause 14");
  expect(h!.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp", config: CHATGPT });
  const queued = userMessage(8, 7, "Now check the reply against clause 14");
  Object.assign(queued.user_message!, { turn: QUEUED, status: "queued" });
  h!.deliverOn(`thread:${T2}`, [upsert(queued)]);
  await h!.settle();
}

test("opened again while the switch waits in the queue: ChatGPT's line, and a new message goes with ChatGPT's picks", async () => {
  await queuedSwitch();
  await reopen();
  expect(planWords()).toBe("ChatGPT from your next message");
  expect(said("model")).toBe("gpt-6.2-sol");
  await ask("And against clause 15");
  expect(h!.sent("send_prompt").at(-1)).toMatchObject({
    thread: T2,
    agent: "codex-acp",
    config: CHATGPT,
  });
  // the queued switch starts: the header names ChatGPT, and the line reads as usual
  h!.deliverOn(`thread:${T2}`, [upsert(headerOf(T2, "running", "codex-acp"))]);
  await h!.settle();
  expect(planWords()).not.toContain("from your next message");
  expect(h!.host.querySelector(".room-status-back")).toBeNull();
  await reopen();
  expect(planWords()).not.toContain("from your next message");
});

test("with no switch waiting, coming back changes nothing", async () => {
  await room();
  await reopen();
  expect(planWords()).toBe("Uses your Claude plan");
  await ask("What does clause nine say?");
  const sent = h!.sent("send_prompt").at(-1) as Record<string, unknown>;
  expect(sent["agent"]).toBeUndefined();
});

test("the switching message removed from the queue ends the hold", async () => {
  await queuedSwitch();
  const removed = userMessage(8, 7, "Now check the reply against clause 14");
  Object.assign(removed.user_message!, { turn: QUEUED, status: "removed" });
  h!.deliverOn(`thread:${T2}`, [upsert(removed)]);
  await h!.settle();
  expect(planWords()).toBe("Uses your Claude plan");
  await reopen();
  expect(planWords()).toBe("Uses your Claude plan");
});
