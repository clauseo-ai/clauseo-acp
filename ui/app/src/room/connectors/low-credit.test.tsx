// @vitest-environment happy-dom
// The running-low line (U15) over FakeTransport: Settings' words under the answer of the turn that was running when
// the case-law connector's credit turned low, with the out-of-credit note's "Add credits"; none when it was low as the
// turn started, again after credit came back and turned low in a later turn, never beside the out-of-credit note, and
// nothing while connectors are off.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
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
const LINE = "Running low. Add credits to keep searching.";
const OK: ConnectorStatus = { signed_in: { low: false, balance: { credits: 200 } } };
const LOW: ConnectorStatus = { signed_in: { low: true, balance: { credits: 8 } } };
const OUT: ConnectorStatus = { out_of_credit: {} };

const stateOf = (status: ConnectorStatus, available = true): ConnectorsState => ({
  available,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      price_paise: 2_360,
      packs: [{ pack: "p200", credits: 200, paise: 2_360 }],
      status,
    },
  ],
});

const head = (status: "running" | "idle"): ViewEntry => ({
  header: { ...header().header!, status },
});

/** A question of `turn` and the start of its answer, `n` apart from the others. */
const asked = (order: number, n: number, turn: TurnId, question: string, answer: string) => [
  { user_message: { ...userMessage(order, n, question).user_message!, turn } },
  { agent_text: { ...agentText(order + 1, n, answer).agent_text!, turn } },
];

/** TURN's exchange, ended, and T2's, running. */
const mount = async () => {
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    openSettings: () => {},
  });
  h.deliver([
    reset(
      [
        userMessage(1, 1, "Find Supreme Court judgments on section 138 NI Act."),
        agentText(2, 1, "Here is what the web turned up."),
        step(3),
        ...asked(4, 2, T2, "And the High Courts?", "Some High Court rulings."),
      ],
      head("running"),
    ),
  ]);
  await h.settle();
  return h;
};

const play = async (event: Parameters<FakeTransport["appEvent"]>[0]) => {
  await act(async () => h!.transport.appEvent(event));
  await h!.settle();
};
const status = (s: ConnectorStatus, available = true) =>
  play({ connectors_changed: stateOf(s, available) });

const end = async (order: number, turn: TurnId) => {
  h!.deliver([upsert(step(order, { turn })), upsert(head("idle"))]);
  await h!.settle();
};
const start = async (order: number, n: number, turn: TurnId, question: string, answer: string) => {
  h!.deliver([...asked(order, n, turn, question, answer).map(upsert), upsert(head("running"))]);
  await h!.settle();
};

/** The answer text of the section each line is under. */
const linesUnder = () =>
  h!.all("low-credit").map((l) => l.closest("section.step-answer")?.textContent ?? null);

test("low turning true mid-turn shows the line once under that turn, and Add credits opens Settings as the out-of-credit note's does", async () => {
  await mount();
  await status(OK);
  expect(h!.all("low-credit")).toHaveLength(0);
  await status(LOW);
  const under = linesUnder();
  expect(under).toHaveLength(1);
  expect(under[0]).toContain("Some High Court rulings.");
  expect(under[0]).not.toContain("Here is what the web turned up.");
  expect(h!.one("low-credit")?.textContent).toContain(LINE);
  expect(h!.one("low-credit-add")?.textContent).toBe("Add credits");
  await h!.click(h!.one("low-credit-add"));
  const fromLine = h!.transport.only("open_settings");
  expect(fromLine).toEqual([{ call: "open_settings", section: "connectors" }]);
  // the turn ends: the line stays under it, and only there
  await end(6, T2);
  expect(linesUnder()).toHaveLength(1);
  // the out-of-credit note's Add credits sends the very same call
  await status(OUT);
  await h!.click(h!.one("out-of-credit-add"));
  expect(h!.transport.only("open_settings")).toEqual([...fromLine, ...fromLine]);
  expect(h!.transport.connectorCommands()).toEqual([]);
});

test("low already when the turn starts: no line", async () => {
  await mount();
  await status(OK);
  await end(6, T2);
  await status(LOW);
  await start(7, 3, T3, "Any from Bombay?", "Two from Bombay.");
  // a later state, still low, mid-turn
  await status({ signed_in: { low: true, balance: { credits: 6 } } });
  await end(9, T3);
  expect(h!.all("low-credit")).toHaveLength(0);
});

test("low, then not low, then low again in a later turn: the line shows again, under that turn", async () => {
  await mount();
  await status(OK);
  await status(LOW);
  expect(linesUnder()).toHaveLength(1);
  await end(6, T2);
  await status(OK);
  expect(h!.all("low-credit")).toHaveLength(0);
  await start(7, 3, T3, "Any from Bombay?", "Two from Bombay.");
  await status(LOW);
  const under = linesUnder();
  expect(under).toHaveLength(1);
  expect(under[0]).toContain("Two from Bombay.");
});

test("out of credit at the same time: only the out-of-credit note", async () => {
  await mount();
  await status(OK);
  await status(OUT);
  expect(h!.all("out-of-credit")).toHaveLength(1);
  expect(h!.all("low-credit")).toHaveLength(0);
  // low first, then out of credit mid-turn: the note replaces the line
  await status(OK);
  await status(LOW);
  expect(h!.all("low-credit")).toHaveLength(1);
  await status(OUT);
  expect(h!.all("out-of-credit")).toHaveLength(1);
  expect(h!.all("low-credit")).toHaveLength(0);
  // hidden in this conversation, the note still keeps the line away
  await h!.click(h!.one("out-of-credit-hide"));
  expect(h!.all("out-of-credit")).toHaveLength(0);
  expect(h!.all("low-credit")).toHaveLength(0);
});

test("connectors off: nothing", async () => {
  await mount();
  await status(OK, false);
  await status(LOW, false);
  expect(h!.all("low-credit")).toHaveLength(0);
  // on again while low: no change seen mid-turn, so still nothing
  await status(LOW);
  expect(h!.all("low-credit")).toHaveLength(0);
  // a line shown goes when connectors go off
  await status(OK);
  await status(LOW);
  expect(h!.all("low-credit")).toHaveLength(1);
  await status(LOW, false);
  expect(h!.all("low-credit")).toHaveLength(0);
});
