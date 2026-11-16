// @vitest-environment happy-dom
// U15's room part over FakeTransport: the case-law hint under its turn's answer in its thread only, its two buttons,
// and when it goes; the out-of-credit note in each conversation's composer, after U12's warning, hidden per
// conversation until credit returns and runs out again, cleared when credit returns, never blocking typing or Ask; and
// the room's connector commands, which are only the hint's dismissal and the focus's balance read.
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  MATTER,
  reset,
  step,
  THREAD,
  TURN,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type {
  ConnectorStatus,
  ConnectorsState,
  LimitsView,
  ViewEntry,
} from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";
import { stubFonts } from "../thread/testing.ts";
import { dismissHint, refreshBalance } from "./main-commands.ts";
import { OutOfCredit } from "./out-of-credit.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
let root: Root | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const T2 = uuid(2, 4);
const OUT: ConnectorStatus = { out_of_credit: { pay_url: "https://data.clauseo.chat/pay/old" } };
const SIGNED_IN: ConnectorStatus = { signed_in: { low: false, balance: { credits: 200 } } };

const stateOf = (
  status: ConnectorStatus,
  over: Partial<Omit<ConnectorsState, "connectors">> = {},
  price: number | null = 2_360,
): ConnectorsState => ({
  available: true,
  locked: false,
  ...over,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      ...(price === null ? {} : { price_paise: price }),
      packs: [{ pack: "p200", credits: 200, paise: 2_360 }],
      status,
    },
  ],
});

/** Two exchanges in the open thread: TURN's, ended, and T2's. */
const ENTRIES: ViewEntry[] = [
  userMessage(1, 1, "Find Supreme Court judgments on section 138 NI Act."),
  agentText(2, 1, "Here is what the web turned up."),
  step(3),
  { user_message: { ...userMessage(4, 2, "And the High Courts?").user_message!, turn: T2 } },
  { agent_text: { ...agentText(5, 2, "Some High Court rulings.").agent_text!, turn: T2 } },
  step(6, { turn: T2 }),
];

const mount = async (opts: { settings?: boolean } = {}) => {
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    ...(opts.settings === false ? {} : { openSettings: () => {} }),
  });
  h.deliver([reset(ENTRIES)]);
  await h.settle();
  return h;
};

const play = async (event: Parameters<FakeTransport["appEvent"]>[0]) => {
  await act(async () => h!.transport.appEvent(event));
  await h!.settle();
};

/** The Claude plan at `used` percent of its 5-hour window (limits.test.tsx's entry). */
const limits = async (used: number) => {
  const tone = used >= 100 ? "at_limit" : used >= 75 ? "near" : "neutral";
  const resets_at = Date.now() + 3_600_000;
  const view: LimitsView = {
    claude: {
      plan: "Claude Max",
      billing: "plan",
      meter: { tone, used_percent: used, window_label: "5-hour limit", resets_at },
      windows: [
        {
          id: "five_hour",
          kind: "session",
          label: "5-hour limit",
          used_percent: used,
          tone,
          resets_at,
          freshness: "fresh",
        },
      ],
      at_limit_text: "",
      freshness: "fresh",
      checked_at: Date.now(),
    },
  };
  await act(async () => h!.transport.limitsChanged(view));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
};

const hint = (thread = THREAD, turn = TURN, matter = MATTER) => ({
  case_law_hint: { matter, thread, turn },
});

/** The answer section that holds the hint, by its exchange's answer text. */
const answerOfHint = () =>
  h!.one("case-law-hint")?.closest("section.step-answer")?.textContent ?? null;

test("the hint shows under its turn's answer in its thread only, with the price, its name and its buttons", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added") });
  await play(hint(uuid(9, 1)));
  await play(hint(THREAD, uuid(9, 4)));
  expect(h!.all("case-law-hint")).toHaveLength(0);
  await play(hint(THREAD, T2));
  expect(h!.all("case-law-hint")).toHaveLength(1);
  expect(answerOfHint()).toContain("Some High Court rulings.");
  expect(answerOfHint()).not.toContain("Here is what the web turned up.");
  const card = h!.one("case-law-hint")!;
  expect(card.querySelector("section")?.getAttribute("aria-label")).toBe(
    "Indian Kanoon suggestion",
  );
  expect(card.textContent).toContain("For judgments, Clauseo can search Indian Kanoon directly.");
  expect(card.textContent).toContain("It's a paid add-on, from ₹23.60, including GST.");
  expect(h!.one("case-law-set-up")?.textContent).toBe("Set up in Settings");
  expect(h!.one("case-law-dismiss")?.textContent).toBe("Don't show again");
});

test("without a price the hint's second sentence has none", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added", {}, null) });
  await play(hint());
  expect(h!.one("case-law-hint")?.textContent).toContain("It's a paid add-on.");
  expect(h!.one("case-law-hint")?.textContent).not.toContain("₹");
});

test("Set up in Settings opens Settings at Connectors and leaves the hint", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added") });
  await play(hint());
  await h!.click(h!.one("case-law-set-up"));
  expect(h!.transport.only("open_settings")).toEqual([
    { call: "open_settings", section: "connectors" },
  ]);
  expect(h!.all("case-law-hint")).toHaveLength(1);
  expect(h!.transport.connectorCommands()).toEqual([]);
});

test("Don't show again sends the dismissal for its matter and the hint goes at once", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added") });
  await play(hint());
  h!.one("case-law-dismiss")?.focus();
  await h!.click(h!.one("case-law-dismiss"));
  expect(h!.transport.connectorCommands()).toEqual([{ dismiss_case_law_hint: { matter: MATTER } }]);
  expect(h!.all("case-law-hint")).toHaveLength(0);
  // the button went with the card: focus is in the composer, not on the page
  expect(document.activeElement).toBe(h!.host.querySelector('[data-testid="composer"]'));
});

test("the hint goes when the connector is added or connectors are off, and never shows unlisted", async () => {
  await mount();
  await play(hint());
  // no state yet, then none listed: nothing
  expect(h!.all("case-law-hint")).toHaveLength(0);
  await play({ connectors_changed: { available: true, locked: false, connectors: [] } });
  expect(h!.all("case-law-hint")).toHaveLength(0);
  await play({ connectors_changed: stateOf("not_added") });
  expect(h!.all("case-law-hint")).toHaveLength(1);
  await play({ connectors_changed: stateOf("signing_in") });
  expect(h!.all("case-law-hint")).toHaveLength(0);
  // added and removed again: the old hint stays gone
  await play({ connectors_changed: stateOf("not_added") });
  expect(h!.all("case-law-hint")).toHaveLength(0);
  await play(hint());
  await play({ connectors_changed: stateOf("not_added", { available: false, locked: true }) });
  expect(h!.all("case-law-hint")).toHaveLength(0);
});

test("the note shows under the field while out of credit, never blocks typing or Ask, and clears when credit returns", async () => {
  await mount();
  await play({ connectors_changed: stateOf(SIGNED_IN) });
  expect(h!.all("out-of-credit")).toHaveLength(0);
  await play({ connectors_changed: stateOf(OUT) });
  const note = h!.one("out-of-credit")!;
  expect(note.textContent).toContain("Out of case-law credits.");
  expect(note.textContent).toContain("Searches of Indian Kanoon stop until you add more.");
  expect(h!.one("out-of-credit-hide")?.getAttribute("aria-label")).toBe(
    "Hide in this conversation",
  );
  // the pay link of the OutOfCredit status is nowhere
  expect(h!.host.innerHTML).not.toContain("pay/old");
  await h!.type("Draft the reply.");
  const field = h!.host.querySelector<HTMLTextAreaElement>('[data-testid="composer"]')!;
  expect(field.disabled).toBe(false);
  expect(field.value).toBe("Draft the reply.");
  expect((h!.one("send") as HTMLButtonElement).disabled).toBe(false);
  await h!.click(h!.one("out-of-credit-add"));
  expect(h!.transport.only("open_settings")).toEqual([
    { call: "open_settings", section: "connectors" },
  ]);
  await play({ connectors_changed: stateOf(SIGNED_IN) });
  expect(h!.all("out-of-credit")).toHaveLength(0);
  // connectors off: no note
  await play({ connectors_changed: stateOf(OUT, { available: false, locked: false }) });
  expect(h!.all("out-of-credit")).toHaveLength(0);
});

test("hidden in this conversation, the note stays hidden through more out-of-credit states, and comes back after credit returns and runs out again", async () => {
  await mount();
  await play({ connectors_changed: stateOf(OUT) });
  h!.one("out-of-credit-hide")?.focus();
  await h!.click(h!.one("out-of-credit-hide"));
  // Hide went with the note: focus is in the composer, not on the page
  expect(document.activeElement).toBe(h!.host.querySelector('[data-testid="composer"]'));
  expect(h!.all("out-of-credit")).toHaveLength(0);
  await play({ connectors_changed: stateOf({ out_of_credit: {} }) });
  expect(h!.all("out-of-credit")).toHaveLength(0);
  await play({ connectors_changed: stateOf(SIGNED_IN) });
  await play({ connectors_changed: stateOf(OUT) });
  expect(h!.all("out-of-credit")).toHaveLength(1);
});

test("focus while the note shows asks for one balance read; none while it doesn't", async () => {
  await mount();
  await play({ connectors_changed: stateOf(SIGNED_IN) });
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(h!.transport.connectorCommands()).toEqual([]);
  await play({ connectors_changed: stateOf(OUT) });
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(h!.transport.connectorCommands()).toEqual([{ refresh_balance: { id: "case-law" } }]);
});

test("the note yields to U12's limit warning, and comes back when the warning goes", async () => {
  // a new conversation whose agent has a plan (limits.test.tsx's room), with the app feed
  h = await mountRoom({
    threads: [],
    next: fakeNext({
      matters: () => [],
      agents: [{ id: "claude-acp", kind: "claude" }],
      parts: ["agents", "app"],
    }),
  });
  h.deliver([reset([])]);
  await play({ connectors_changed: stateOf(OUT) });
  expect(h.all("out-of-credit")).toHaveLength(1);
  await limits(96);
  expect(h.all("limit-warning")).toHaveLength(1);
  expect(h.all("out-of-credit")).toHaveLength(0);
  await limits(30);
  expect(h.all("limit-warning")).toHaveLength(0);
  expect(h.all("out-of-credit")).toHaveLength(1);
});

test("in every open conversation the note shows, and hiding it in one leaves the others", async () => {
  const transport = new FakeTransport({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    openSettings: () => {},
  });
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      createElement(
        "div",
        null,
        createElement(
          "div",
          { "data-c": "a" },
          createElement(OutOfCredit, { transport, conversation: "a", onFocusField: () => {} }),
        ),
        createElement(
          "div",
          { "data-c": "b" },
          createElement(OutOfCredit, { transport, conversation: "b", onFocusField: () => {} }),
        ),
      ),
    ),
  );
  const notes = () =>
    [...host.querySelectorAll('[data-testid="out-of-credit"]')].map((n) =>
      n.closest("[data-c]")?.getAttribute("data-c"),
    );
  const wait = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  await act(async () => transport.appEvent({ connectors_changed: stateOf(OUT) }));
  await wait();
  await wait();
  expect(notes()).toEqual(["a", "b"]);
  await act(async () =>
    host.querySelector<HTMLElement>('[data-c="a"] [data-testid="out-of-credit-hide"]')!.click(),
  );
  expect(notes()).toEqual(["b"]);
  await act(async () => transport.appEvent({ connectors_changed: stateOf(SIGNED_IN) }));
  expect(notes()).toEqual([]);
  await act(async () => transport.appEvent({ connectors_changed: stateOf(OUT) }));
  expect(notes()).toEqual(["a", "b"]);
});

test("the room's commands are the hint's dismissal and the balance read; the type takes nothing else", async () => {
  const transport = new FakeTransport({});
  await dismissHint(transport, MATTER);
  await refreshBalance(transport, "case-law");
  expect(transport.connectorCommands()).toEqual([
    { dismiss_case_law_hint: { matter: MATTER } },
    { refresh_balance: { id: "case-law" } },
  ]);
  // @ts-expect-error: the room has no add, nor any other settings-only command
  const add: import("./main-commands.ts").MainConnectorCommand = { add: { id: "case-law" } };
  expect(add).toBeDefined();
});
