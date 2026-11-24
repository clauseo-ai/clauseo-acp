// @vitest-environment happy-dom
// Settings › Connectors out of credit by the one rule (store/case-law-credit.ts, r7363, r7458): the host's
// out_of_credit, whatever balance was read before it, and a balance of 0 say what the library and the composer say,
// with no figure (out_of_credit carries none), never "Ready to search." or "Running low"; Add credits leads as the
// primary button. With no packs listed, the status's pay link when it has one, else the sentence without "Add credits
// to…". Not out of credit with 7 left keeps today's running-low words.
import { afterEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { ConnectorView } from "../../transport/protocol.ts";
import { mountSettings, stateOf, type Rig } from "./connectors-rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

let rig: Rig | null = null;
afterEach(async () => {
  await rig?.unmount();
  rig = null;
});

const NOT_ENOUGH = "Not enough credits for a search. Add credits to keep searching.";
const NONE = "No credits. Add credits to search.";
const signed = (credits: number, low: boolean): ConnectorView["status"] => ({
  signed_in: { balance: { credits }, low },
});
const OUT: ConnectorView["status"] = {
  out_of_credit: { pay_url: "https://data.clauseo.chat/pay/x" },
};

const line = (r: Rig, testId: string) =>
  r.el.querySelector(`[data-testid="${testId}"]`)?.textContent ?? null;
const addCredits = (r: Rig) =>
  [...r.el.querySelectorAll("button")].find((b) => b.textContent === "Add credits");

/** The window at #connectors, a balance read first when `before` is given, then `status`. */
async function open(status: ConnectorView["status"], before?: number) {
  rig = await mountSettings({
    state: stateOf({ status: before === undefined ? status : signed(before, true) }),
    hash: "#connectors",
  });
  if (before !== undefined) await rig.play(stateOf({ status }));
  return rig;
}

const saysOut = (r: Rig) => {
  expect(r.text()).not.toContain("Ready to search.");
  expect(r.text()).not.toContain("Running low");
  expect(r.note("Connectors")).toBe("Out of credit");
  expect(r.buttons()[0]).toBe("Add credits");
  expect(addCredits(r)).toBeDefined();
};

test("out of credit after 7 credits were read: the line, and no figure, which the status doesn't carry", async () => {
  const r = await open(OUT, 7);
  expect(line(r, "connectors-out")).toBe(NOT_ENOUGH);
  expect(r.text()).not.toContain("7 credits left");
  expect(r.el.querySelector('[data-testid="connectors-balance"]')).toBeNull();
  saysOut(r);
});

test("out of credit with no packs listed: its pay link, or the sentence without Add credits", async () => {
  const pay = await mountSettings({
    state: stateOf({ status: OUT, packs: [] }),
    hash: "#connectors",
  });
  rig = pay;
  expect(line(pay, "connectors-out")).toBe(NOT_ENOUGH);
  await pay.press("Open the payment page");
  expect(pay.transport.only("open_external")).toEqual([
    { call: "open_external", url: "https://data.clauseo.chat/pay/x", alwaysAsk: true },
  ]);
  await pay.unmount();
  const bare = await mountSettings({
    state: stateOf({ status: { out_of_credit: {} }, packs: [] }),
    hash: "#connectors",
  });
  rig = bare;
  expect(line(bare, "connectors-out")).toBe("Not enough credits for a search.");
  expect(bare.buttons()).not.toContain("Open the payment page");
  expect(bare.buttons()).not.toContain("Add credits");
});

test("a never-funded account at 0 says No credits, never Ready to search or Running low", async () => {
  const r = await open(signed(0, true));
  expect(line(r, "connectors-out")).toBe(NONE);
  saysOut(r);
});

test("Add credits is the primary button out of credit, and tone otherwise", async () => {
  const out = await open(OUT, 7);
  const primary = addCredits(out)!.className;
  await out.unmount();
  rig = null;
  const ok = await open(signed(10_000, false));
  expect(addCredits(ok)!.className).not.toBe(primary);
});

test("not out of credit with 7 left: today's running-low words", async () => {
  const r = await open(signed(7, true));
  expect(line(r, "connectors-out")).toBeNull();
  expect(line(r, "connectors-low")).toBe("Running low. Add credits to keep searching.");
  expect(r.text()).toContain("Ready to search.");
  expect(r.note("Connectors")).toBe("7 credits left");
});
