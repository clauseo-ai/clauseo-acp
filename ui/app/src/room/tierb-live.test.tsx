// @vitest-environment happy-dom
// R17 PR-E's tier-B refusals through the room, as they arrive while the lawyer watches: the full pool's refused turn
// with Try again (once per press; the refilled question leaves the field and is asked once more), an item named where
// (pool_owned, cant_label) in the step row with no Try again, and the stop for leaving the Safe workspace, naming the
// plan.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { ErrorCode, StepEntry, ViewEntry } from "../transport/protocol.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

const NOW = Date.UTC(2026, 10, 8, 11, 0);
let h: Harness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const QUESTION = "Summarise the notice to the bank.";
const field = () => h!.one("composer") as HTMLTextAreaElement;
const status = (s: "running" | "idle", agent?: string): ViewEntry => {
  const head = header();
  head.header!.status = s;
  if (agent !== undefined) head.header!.agent = agent;
  return head;
};

async function asked(agent?: string): Promise<Harness> {
  let sends = 0;
  const room = await mountRoom({
    dispatch: (command) => {
      if (command.send_prompt === undefined) return defaultOutcome(command);
      sends += 1;
      return { prompt_accepted: { turn: uuid(sends, 4), message: uuid(sends, 5) } };
    },
  });
  room.deliver([reset([])]);
  await room.type(QUESTION);
  await room.key({ key: "Enter", keyCode: 13 });
  room.deliver([
    upsert(userMessage(5, 1, QUESTION)),
    upsert(agentText(6, 1, "Clause nine requires notice")),
    upsert(status("running", agent)),
  ]);
  return room;
}

const ended = (over: Partial<StepEntry>, agent?: string) => [
  upsert(step(9, { ts: NOW, ...over })),
  upsert(status("idle", agent)),
];
const refused = (failure: ErrorCode, at?: string) =>
  ended({ stop: "failed", failure, ...(at === undefined ? {} : { failure_at: at }) });

test("a full pool's refused turn: its words, Try again once per press, the question asked once more and gone from the field", async () => {
  h = await asked();
  h.deliver(refused("sandbox.pool_full"));
  await h.settle();
  const line = h.one("ask-again");
  expect(line?.dataset["code"]).toBe("sandbox.pool_full");
  expect(line?.textContent).toContain(errorWords("en", "sandbox.pool_full"));
  expect(field().value).toBe(QUESTION);
  const retry = h.one("retry");
  expect(retry).not.toBeNull();
  await h.click(retry);
  await h.settle();
  expect(h.sent("send_prompt").map((c) => c.content[0]?.text?.text)).toEqual([QUESTION, QUESTION]);
  expect(h.one("ask-again")).toBeNull();
  expect(h.one("retry")).toBeNull();
  expect(field().value).toBe("");
});

test.each([
  [
    "pool_owned with at",
    "sandbox.pool_owned",
    "Pleadings/Draft.docx",
    "\u2068Pleadings/Draft.docx\u2069 in this matter still belongs to the Safe workspace, so Clauseo can't answer here. Ask your IT team to give you back ownership of \u2068Pleadings/Draft.docx\u2069.",
  ],
  [
    "pool_owned without at",
    "sandbox.pool_owned",
    undefined,
    "Part of this matter's folder still belongs to the Safe workspace, so Clauseo can't answer here. Ask your IT team to give you back ownership of it.",
  ],
  [
    "cant_label with at",
    "sandbox.cant_label",
    "Pleadings",
    "The Safe workspace couldn't be given access to \u2068Pleadings\u2069, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
  ],
  [
    "cant_label without at",
    "sandbox.cant_label",
    undefined,
    "The Safe workspace couldn't be prepared for this matter, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
  ],
] as const)("%s: the step row says where, with no Try again", async (_c, code, at, words) => {
  h = await asked();
  h.deliver(refused(code, at));
  await h.settle();
  const row = h.one("turn-stopped");
  expect(row?.dataset["failure"]).toBe(code);
  expect(row?.querySelector(`[data-code="${code}"]`)?.textContent).toBe(words);
  expect(h.one("ask-again")).toBeNull();
  expect(h.one("retry")).toBeNull();
});

test.each([
  [undefined, "Claude"],
  ["codex-acp", "ChatGPT"],
])(
  "leaving the Safe workspace (agent %s): the notice names %s, and the way on",
  async (agent, name) => {
    h = await asked(agent);
    h.deliver(ended({ stop: "left_safe_workspace" }, agent));
    await h.settle();
    const notice = h.one("left-safe-workspace");
    expect(h.one("turn-stopped")?.dataset["stopReason"]).toBe("left_safe_workspace");
    expect(notice?.getAttribute("role")).toBe("status");
    expect(notice?.textContent).toContain(
      `Clauseo stopped ${name}: part of it tried to run outside the Safe workspace.`,
    );
    expect(notice?.textContent).toContain(
      "Ask again to carry on. If this keeps happening, tell whoever looks after this computer.",
    );
    expect(h.one("ask-again")).toBeNull();
  },
);
