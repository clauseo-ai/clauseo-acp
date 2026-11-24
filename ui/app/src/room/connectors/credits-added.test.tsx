// @vitest-environment happy-dom
// "Credits added. Ask again to search case law." (UC-1460, r7458, r7462) over FakeTransport: the conversation's own
// entries hold the host's case_law_out_of_credit notice after the lawyer's last message (met-refusal.ts), and the
// credit is back. The notice arrives after the refused step, as the host logs it; the next message clears the line by
// itself; it holds for a conversation opened later and after a reload; it never shows while out, 0 → some → 0 included.
// The notice's own row says what happened, past tense, and stays as history.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  reset,
  step,
  toolCall,
  upsert,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import type { ConnectorStatus, ConnectorsState, ViewEntry } from "../../transport/protocol.ts";
import { CASE_LAW_OUT_OF_CREDIT } from "../thread/case-law-notice.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";
import { stubFonts } from "../thread/testing.ts";
import { metRefusal } from "./met-refusal.ts";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const LINE = "Credits added. Ask again to search case law.";
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

/** The host's mark, logged after the refused step (stand-in kind until the binding lands). */
const marked = (order: number): ViewEntry => ({
  notice: {
    order,
    event: uuid(order, 10),
    level: "warning",
    text: "Clauseo Case Law is out of credit.",
    kind: CASE_LAW_OUT_OF_CREDIT,
    ts: order * 1000,
  },
});

/** The question, the refused Case Law step, then the mark: the real order. */
const REFUSED: ViewEntry[] = [
  userMessage(1, 1, "Find the leading Supreme Court cases on section 138."),
  toolCall(2, 1, "failed"),
  marked(3),
  agentText(4, 1, "The search was refused: no credits."),
  step(5),
];

const mount = async (entries: ViewEntry[]) => {
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    openSettings: () => {},
  });
  h.deliver([reset(entries)]);
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

test("metRefusal: the mark after the last message holds; a later message ends it; none before any mark", () => {
  expect(metRefusal(REFUSED)).toBe(true);
  expect(metRefusal([...REFUSED, userMessage(6, 2, "Search again.")])).toBe(false);
  expect(metRefusal([userMessage(1, 1, "Hello."), agentText(2, 1, "Hi."), step(3)])).toBe(false);
  expect(metRefusal([marked(1), userMessage(2, 1, "Later.")])).toBe(false);
});

test("the mark after the refused step, then credit back: the line; the next message clears it", async () => {
  await mount([REFUSED[0]!, REFUSED[1]!]);
  await status(OUT);
  // the mark arrives after the step, as the host logs it
  h!.deliver([upsert(marked(3))]);
  await h!.settle();
  expect(line()).toBeNull();
  // the row says what happened; the strip carries the action, so no sentence shows twice
  expect(h!.one("case-law-notice")?.textContent).toBe(
    "Clauseo Case Law couldn't search here: out of credits.",
  );
  expect(h!.one("out-of-credit")?.textContent).toContain(
    "Not enough credits for a search. Add credits to keep searching.",
  );
  expect(h!.one("case-law-notice")?.querySelector("button")).toBeNull();
  await status(PAID);
  expect(line()).toBe(LINE);
  // the row stays as history
  expect(h!.one("case-law-notice")?.textContent).toBe(
    "Clauseo Case Law couldn't search here: out of credits.",
  );
  expect(h!.sent("send_prompt")).toEqual([]);
  h!.deliver([upsert(userMessage(6, 2, "Search again."))]);
  await h!.settle();
  expect(line()).toBeNull();
});

test("a conversation opened after the credits came back, or reloaded, still shows the line", async () => {
  // credits already back when the refused conversation is opened (a closed one, or after a reload)
  await mount(REFUSED);
  await status(PAID);
  expect(line()).toBe(LINE);
  // the view arrives again, as after a reload: the line still holds from the entries
  h!.deliver([reset(REFUSED)]);
  await h!.settle();
  expect(line()).toBe(LINE);
});

test("0 → some → 0: never the line while out", async () => {
  await mount(REFUSED);
  await status({ signed_in: { low: true, balance: { credits: 0 } } });
  expect(line()).toBeNull();
  expect(h!.all("out-of-credit")).toHaveLength(1);
  await status(PAID);
  expect(line()).toBe(LINE);
  await status({ signed_in: { low: true, balance: { credits: 0 } } });
  expect(line()).toBeNull();
  expect(h!.all("out-of-credit")).toHaveLength(1);
});

test("a turn written while out, with no mark in it, is not marked", async () => {
  await mount([userMessage(1, 1, "Hello."), agentText(2, 1, "Hi."), step(3)]);
  await status(OUT);
  h!.deliver([
    upsert(userMessage(4, 2, "Draft the reply to the notice.")),
    upsert(agentText(5, 2, "Here is the draft.")),
    upsert(step(6)),
  ]);
  await h!.settle();
  await status(PAID);
  expect(line()).toBeNull();
  expect(h!.all("case-law-notice")).toHaveLength(0);
});

test("a conversation without the mark shows nothing when credit comes back", async () => {
  await mount([userMessage(1, 1, "Hello."), agentText(2, 1, "Hi."), step(3)]);
  await status(OUT);
  await status(PAID);
  expect(line()).toBeNull();
});
