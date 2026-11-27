// @vitest-environment happy-dom
// The header follows the switch from its send (held-agent.ts's useShownAgent): once the host took a message sent with
// ChatGPT held (queued behind Claude's answer, or started before the header says so), the toolbar names ChatGPT and
// the status line says "Uses your ChatGPT plan", though the host's header still names Claude. The hold still ends
// only when the host's header names ChatGPT, so a message sent in between goes to ChatGPT too; a switching message
// removed from the queue puts Claude back.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { header, reset, upsert, userMessage, uuid } from "../testing/stream-script.ts";
import type {
  AgentLimitsView,
  AgentModels,
  ConfigOption,
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

const T = uuid(2, 1);
const TURN = uuid(7, 4);
const TEXT = "Now check the reply against clause 14";

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

// the list's row keeps naming Claude throughout: the host's list follows the switch only later
const THREAD: ThreadSummary = {
  thread: T,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status: "running",
  title: "Indemnity",
};

const headerOf = (status: "idle" | "running", agent = "claude-acp"): ViewEntry => ({
  header: {
    ...header(status).header!,
    thread: T,
    agent,
    config_options: [select("model", "model", "opus"), select("effort", "thought_level", "medium")],
  },
});

// a view for each plan, so the toolbar's plan stands; with no sign-in heard it names the agent alone
const limits = (plan: string): AgentLimitsView => ({
  plan,
  billing: "plan",
  windows: [],
  at_limit_text: "",
  freshness: "fresh",
  checked_at: Date.now(),
});

const planWords = () => h!.host.querySelector(".room-status-plan")?.textContent ?? "";
const toolbar = () => h!.one("plan")?.textContent?.replace(/x$/, "") ?? null;

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

/** Claude's conversation (`status`), ChatGPT held, a message sent with it and taken by the host (queued with an answer
 * running), the header still naming Claude. */
async function switchTaken(status: "idle" | "running") {
  h = await mountRoom({
    threads: [THREAD],
    next: fakeNext({ matters: () => [], agents: AGENTS, parts: ["agents"] }),
    dispatch: (command) =>
      command.send_prompt === undefined
        ? defaultOutcome(command)
        : { prompt_accepted: { turn: TURN, message: uuid(7, 5) } },
  });
  h.deliverOn(`thread:${T}`, [reset([], headerOf(status))]);
  await act(async () =>
    h!.transport.limitsChanged({ claude: limits("Claude Max"), codex: limits("ChatGPT Plus") }),
  );
  await h.settle();
  expect(toolbar()).toBe("Claude");
  await holdChatGPT();
  // only held: the header still names Claude
  expect(toolbar()).toBe("Claude");
  await ask(TEXT);
  expect(h.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp" });
  const sent = userMessage(8, 7, TEXT);
  Object.assign(sent.user_message!, { turn: TURN, status: status === "idle" ? "sent" : "queued" });
  h.deliverOn(`thread:${T}`, [upsert(sent)]);
  await h.settle();
}

test("a switch queued behind the answer: the header names ChatGPT from the send, and the hold stays until the host's header does", async () => {
  await switchTaken("running");
  expect(toolbar()).toBe("ChatGPT");
  expect(planWords()).toBe("Uses your ChatGPT plan");
  expect(h!.host.querySelector(".room-status-back")).toBeNull();
  // the hold still holds: a message sent now goes to ChatGPT as well
  await ask("And against clause 15");
  expect(h!.sent("send_prompt").at(-1)).toMatchObject({ agent: "codex-acp" });
  // the switch starts: the host's header names ChatGPT, the hold has done its work, and the header stays on ChatGPT
  h!.deliverOn(`thread:${T}`, [upsert(headerOf("running", "codex-acp"))]);
  await h!.settle();
  expect(toolbar()).toBe("ChatGPT");
  expect(planWords()).toBe("Uses your ChatGPT plan");
  await ask("And clause 16");
  expect((h!.sent("send_prompt").at(-1) as Record<string, unknown>)["agent"]).toBeUndefined();
});

test("a switch taken with nothing running: ChatGPT in the header before the host's header names it", async () => {
  await switchTaken("idle");
  expect(toolbar()).toBe("ChatGPT");
  expect(planWords()).toBe("Uses your ChatGPT plan");
});

test("the switching message removed from the queue: the header names Claude again", async () => {
  await switchTaken("running");
  expect(toolbar()).toBe("ChatGPT");
  const removed = userMessage(8, 7, TEXT);
  Object.assign(removed.user_message!, { turn: TURN, status: "removed" });
  h!.deliverOn(`thread:${T}`, [upsert(removed)]);
  await h!.settle();
  expect(toolbar()).toBe("Claude");
  expect(planWords()).toBe("Uses your Claude plan");
});
