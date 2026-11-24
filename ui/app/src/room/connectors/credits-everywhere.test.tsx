// @vitest-environment happy-dom
// Clauseo Case Law's credit says the same thing by the composer and on the library's Case Law card (r7363;
// store/case-law-credit.ts): the host's out_of_credit with 7 credits last read and with 0, a balance of 0 the host
// still calls signed in, and Add credits opening the host's pay link through the app's link path. Not out of credit
// with 7 left keeps today's running-low words. Settings › Connectors has its own file (connectors-out.test.tsx).
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SEEN_KEY } from "../../library/seen.ts";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import type { ConnectorStatus, ConnectorsState } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
beforeEach(() => localStorage.removeItem(SEEN_KEY));
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const NOT_ENOUGH = "Not enough credits for a search. Add credits to keep searching.";
const NONE = "No credits. Add credits to search.";
const PAY = "https://data.clauseo.chat/pay/r1";

const signed = (credits: number, low = true): ConnectorStatus => ({
  signed_in: { low, balance: { credits } },
});
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

const mount = async () => {
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (query === "list_skill_packs") return { skill_packs: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    openSettings: () => {},
  });
  h.deliver([
    reset([userMessage(1, 1, "Find the leading cases."), agentText(2, 1, "Here."), step(3)]),
  ]);
  await h.settle();
  return h;
};

const settled = async () => {
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
};

const play = async (...statuses: ConnectorStatus[]) => {
  for (const status of statuses) {
    await act(async () => h!.transport.appEvent({ connectors_changed: stateOf(status) }));
    await settled();
  }
};

/** The composer's note: its first line and its second. */
const note = () => {
  const el = h!.one("out-of-credit");
  return el === null
    ? null
    : {
        lead: el.querySelector(".lim-field-lead")?.textContent ?? "",
        rest: el.querySelector(".lim-field-rest")?.textContent ?? "",
      };
};

/** Opens Skills and connectors on Connectors, Case Law picked: its card's meta and its detail's credits. */
const library = async () => {
  await h!.click(h!.one("library-entry"));
  await settled();
  const tab = [...h!.host.querySelectorAll('[role="tab"]')].find(
    (e) => e.textContent === "Connectors",
  );
  await h!.click(tab);
  const card = h!.one("case-law-card")!;
  await h!.click(card.querySelector(".lib-feature-hit"));
  return {
    meta: card.querySelector(".lib-feature-meta")?.textContent ?? "",
    detail: h!.one("case-law-credits")?.textContent ?? null,
  };
};

test("out of credit with 7 last read: the composer and the library say not enough, with the figure", async () => {
  await mount();
  await play(signed(7), { out_of_credit: { pay_url: PAY } });
  expect(note()).toEqual({
    lead: "Clauseo Case Law · 7 credits left",
    rest: `${NOT_ENOUGH} Add credits`,
  });
  expect(h!.one("out-of-credit")?.textContent).not.toContain("Indian Kanoon");
  expect(h!.all("low-credit-before")).toHaveLength(0);
  const lib = await library();
  expect(lib.meta).toContain(NOT_ENOUGH);
  expect(lib.meta).not.toContain("7 credits left");
  expect(lib.detail).toBe(NOT_ENOUGH);
});

test("a never-funded account at 0, still signed in: No credits, never Running low", async () => {
  await mount();
  await play(signed(0));
  expect(note()).toEqual({ lead: "Clauseo Case Law", rest: `${NONE} Add credits` });
  expect(h!.all("low-credit-before")).toHaveLength(0);
  expect((await library()).detail).toBe(NONE);
});

test("Add credits opens the host's pay link by the composer and in the library; without one, Settings", async () => {
  await mount();
  await play(signed(7), { out_of_credit: { pay_url: PAY } });
  await h!.click(h!.one("out-of-credit-add"));
  await library();
  await h!.click(h!.one("case-law-add-credits"));
  // through the app's link path, always behind the native confirm
  expect(h!.transport.only("open_external")).toEqual([
    { call: "open_external", url: PAY, alwaysAsk: true },
    { call: "open_external", url: PAY, alwaysAsk: true },
  ]);
  expect(h!.transport.only("open_settings")).toEqual([]);
  await play({ out_of_credit: {} });
  await h!.click(h!.one("case-law-add-credits"));
  expect(h!.transport.only("open_settings")).toEqual([
    { call: "open_settings", section: "connectors" },
  ]);
});

test("not out of credit with 7 left: today's running-low line by the composer, and no out note", async () => {
  await mount();
  await play(signed(7));
  expect(note()).toBeNull();
  expect(h!.one("low-credit-before")?.textContent).toContain(
    "Running low. Add credits to keep searching.",
  );
  const lib = await library();
  expect(lib.detail).toContain("7 credits left");
  expect(lib.detail).not.toContain(NOT_ENOUGH);
});
