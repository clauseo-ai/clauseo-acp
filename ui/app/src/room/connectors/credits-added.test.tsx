// @vitest-environment happy-dom
// "Credits added. Ask again to search case law." (UC-1460, r7381) over FakeTransport: a turn runs as Clauseo Case Law
// goes out of credit (the refusal), the turn ends, credits arrive (the status leaves out_of_credit, or the figure
// rises from 0), and the line takes the out-of-credit note's place by the composer; it goes once the next message's
// turn starts, and nothing is resent. A conversation whose turn wasn't running as the credit went out shows nothing.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type {
  ConnectorStatus,
  ConnectorsState,
  TurnId,
  ViewEntry,
} from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";
import { stubFonts } from "../thread/testing.ts";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const T2 = uuid(2, 4);
const T3 = uuid(3, 4);
const LINE = "Credits added. Ask again to search case law.";
const OK: ConnectorStatus = { signed_in: { low: false, balance: { credits: 400 } } };
const PAID: ConnectorStatus = { signed_in: { low: false, balance: { credits: 2_400 } } };
const OUT: ConnectorStatus = { out_of_credit: {} };

const stateOf = (status: ConnectorStatus): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Clauseo Case Law",
      price_paise: 23_600,
      packs: [{ pack: "p2000", credits: 2_000, paise: 23_600 }],
      status,
    },
  ],
});

const head = (status: "running" | "idle"): ViewEntry => ({
  header: { ...header().header!, status },
});
const asked = (order: number, n: number, turn: TurnId, question: string, answer: string) => [
  { user_message: { ...userMessage(order, n, question).user_message!, turn } },
  { agent_text: { ...agentText(order + 1, n, answer).agent_text!, turn } },
];

/** The thread with one ended exchange and T2's, running when `running`, else ended too. */
const mount = async (running = true) => {
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    openSettings: () => {},
  });
  h.deliver([
    reset(
      [
        userMessage(1, 1, "Find Supreme Court judgments on section 138 NI Act."),
        agentText(2, 1, "Here they are."),
        step(3),
        ...asked(4, 2, T2, "And the High Courts?", "The search was refused."),
        ...(running ? [] : [step(6, { turn: T2 })]),
      ],
      head(running ? "running" : "idle"),
    ),
  ]);
  await h.settle();
  return h;
};

const status = async (s: ConnectorStatus) => {
  await act(async () => h!.transport.appEvent({ connectors_changed: stateOf(s) }));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
};

const line = () => h!.one("credits-added")?.textContent ?? null;

test("refused mid-turn, then credits arrive: the line, until the next message's turn starts", async () => {
  await mount();
  await status(OK);
  await status(OUT);
  expect(h!.all("out-of-credit")).toHaveLength(1);
  expect(line()).toBeNull();
  // the refused turn ends
  h!.deliver([upsert(step(6, { turn: T2 })), upsert(head("idle"))]);
  await h!.settle();
  await status(PAID);
  expect(h!.all("out-of-credit")).toHaveLength(0);
  expect(line()).toBe(LINE);
  // a balance read that changes nothing keeps it; nothing was resent
  await status({ signed_in: { low: false, balance: { credits: 2_380 } } });
  expect(line()).toBe(LINE);
  expect(h!.sent("send_prompt")).toEqual([]);
  // the next message: its turn starts and the line goes
  h!.deliver([
    ...asked(7, 3, T3, "Search again.", "Searching.").map(upsert),
    upsert(head("running")),
  ]);
  await h!.settle();
  expect(line()).toBeNull();
});

test("credits rising from 0 count as arriving too", async () => {
  await mount();
  await status(OK);
  await status({ signed_in: { low: true, balance: { credits: 0 } } });
  expect(h!.all("out-of-credit")).toHaveLength(1);
  await status(PAID);
  expect(line()).toBe(LINE);
});

test("a conversation whose turn wasn't running as the credit went out shows nothing", async () => {
  await mount(false);
  await status(OK);
  await status(OUT);
  await status(PAID);
  expect(h!.all("out-of-credit")).toHaveLength(0);
  expect(line()).toBeNull();
});
