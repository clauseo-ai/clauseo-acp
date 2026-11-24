// @vitest-environment happy-dom
// The home card offering Clauseo Case Law over FakeTransport: it shows on a new conversation while the connector is
// listed and not added, goes the moment `connectors_changed` lists it added (no remount), stays gone while added but
// broken, and its button opens Skills and connectors at Clauseo Case Law's card, listed or not. The price and terms as
// two lines, or the terms alone with no figure. Not in an open conversation, nor with connectors off.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
import type { ConnectorStatus, ConnectorsState } from "../../transport/protocol.ts";
import { SEEN_KEY } from "../../library/seen.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let h: Harness | null = null;
beforeEach(() => localStorage.removeItem(SEEN_KEY));
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const SENTENCE =
  "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.";
const SIGNED_IN: ConnectorStatus = { signed_in: { low: false, balance: { credits: 200 } } };

const stateOf = (
  status: ConnectorStatus,
  available = true,
  price: number | null = 23_600,
): ConnectorsState => ({
  available,
  locked: false,
  connectors: [
    {
      id: "case-law",
      name: "Clauseo Case Law",
      ...(price === null ? {} : { price_paise: price }),
      packs: [{ pack: "p2000", credits: 2_000, paise: 23_600 }],
      status,
    },
  ],
});

/** The room on a new conversation (no thread yet), or on an open one with an answer. */
const mount = async (open: "new" | "thread" = "new") => {
  h = await mountRoom({
    ...(open === "new" ? { threads: [] } : {}),
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: (query) => {
      if (query === "list_skill_packs") return { skill_packs: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    openSettings: () => {},
  });
  if (open === "thread")
    h.deliver([
      reset([userMessage(1, 1, "Is the notice valid?"), agentText(2, 1, "Yes."), step(3)]),
    ]);
  await h.settle();
  return h;
};

const play = async (event: Parameters<FakeTransport["appEvent"]>[0]) => {
  await act(async () => h!.transport.appEvent(event));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
};

test("the card shows on a new conversation while the connector is not added", async () => {
  await mount();
  expect(h!.all("case-law-offer")).toHaveLength(0);
  await play({ connectors_changed: stateOf("not_added") });
  const card = h!.one("case-law-offer")!;
  expect(card.querySelector("section")?.getAttribute("aria-label")).toBe("Clauseo Case Law");
  expect(card.querySelector(".lim-note-title")?.textContent).toBe("Clauseo Case Law");
  // with a price: the price and the terms, each on its own line
  expect(h!.one("case-law-offer-price")?.textContent).toBe("From ₹236.");
  expect(h!.one("case-law-offer-terms")?.textContent).toBe(
    "Prepaid, including GST. No subscription.",
  );
  expect(h!.one("case-law-offer-price")?.className).toContain("block");
  expect(h!.one("case-law-offer-terms")?.className).toContain("block");
  expect(card.querySelector(".lim-note-sub")?.textContent).toBe(
    `${SENTENCE}From ₹236.` + "Prepaid, including GST. No subscription.",
  );
  expect(h!.one("case-law-offer-add")?.textContent).toBe("Add Clauseo Case Law");
  expect(h!.one("case-law-offer-source")?.textContent).toBe("Powered by Indian Kanoon");
  // no logo: the tile holds the case-law glyph alone
  expect(card.querySelector("img")).toBeNull();
  // the composer keeps its place below it
  expect(h!.one("composer")).not.toBeNull();
});

test("without a price from the host, no figure: the terms alone", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added", true, null) });
  const sub = h!.one("case-law-offer")?.querySelector(".lim-note-sub")?.textContent ?? "";
  expect(sub).toBe(`${SENTENCE}Prepaid credits. No subscription.`);
  expect(sub).not.toContain("₹");
  expect(h!.all("case-law-offer-price")).toHaveLength(0);
});

/** Presses the card's button, the library's chunk loaded. */
const add = async () => {
  await h!.click(h!.one("case-law-offer-add"));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h!.settle();
};

test("with no connector listed (the host's liveness check failed), the card still shows and leads to Case Law", async () => {
  await mount();
  await play({ connectors_changed: { available: true, locked: false, connectors: [] } });
  expect(h!.one("case-law-offer")?.querySelector(".lim-note-sub")?.textContent).toBe(
    `${SENTENCE}Prepaid credits. No subscription.`,
  );
  await add();
  expect(h!.one("case-law-card")).not.toBeNull();
  expect(h!.one("library-detail")?.querySelector(".lib-detail-name")?.textContent).toBe(
    "Clauseo Case Law",
  );
  expect(h!.all("case-law-offer")).toHaveLength(0);
});

test("it goes the moment a state lists Clauseo Case Law added, with no restart", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added") });
  expect(h!.all("case-law-offer")).toHaveLength(1);
  await play({ connectors_changed: stateOf("signing_in") });
  expect(h!.all("case-law-offer")).toHaveLength(0);
  await play({ connectors_changed: stateOf(SIGNED_IN) });
  expect(h!.all("case-law-offer")).toHaveLength(0);
});

test("added but broken, the card stays gone", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added") });
  await play({ connectors_changed: stateOf(SIGNED_IN) });
  const broken: ConnectorStatus[] = [
    "key_revoked",
    "signed_out",
    { error: { message: "refused", kind: "sign_in_failed" } },
    { out_of_credit: {} },
  ];
  for (const status of broken) {
    await play({ connectors_changed: stateOf(status) });
    expect(h!.all("case-law-offer")).toHaveLength(0);
  }
});

test("Add Clauseo Case Law opens Skills and connectors on Connectors, Clauseo Case Law picked", async () => {
  await mount();
  await play({ connectors_changed: stateOf("not_added") });
  await add();
  // the Connectors tab (its Case Law card), with Clauseo Case Law picked in the panel
  expect(h!.one("case-law-card")).not.toBeNull();
  expect(h!.one("library-detail")?.querySelector(".lib-detail-name")?.textContent).toBe(
    "Clauseo Case Law",
  );
  expect(h!.all("case-law-offer")).toHaveLength(0);
  // nothing sent yet: the library's card adds it
  expect(h!.transport.only("open_settings")).toEqual([]);
  expect(h!.transport.connectorCommands()).toEqual([]);
});

test("no card in an open conversation, nor with connectors turned off", async () => {
  await mount("thread");
  await play({ connectors_changed: stateOf("not_added") });
  expect(h!.all("case-law-offer")).toHaveLength(0);
  await h!.unmount();
  h = null;
  await mount();
  await play({ connectors_changed: stateOf("not_added", false) });
  expect(h!.all("case-law-offer")).toHaveLength(0);
});
