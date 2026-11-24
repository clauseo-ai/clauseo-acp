// @vitest-environment happy-dom
// Settings › Connectors out of credit by the one rule (store/case-law-credit.ts, r7363): the host's out_of_credit with
// 7 credits last read and with 0, and a balance of 0, all say what the library and the composer say, never "Ready to
// search." or "Running low"; Add credits leads as the primary button. Not out of credit with 7 left keeps today's
// running-low words.
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

test("out of credit with 7 credits last read: the line and the figure beside it", async () => {
  const r = await open(OUT, 7);
  expect(line(r, "connectors-out")).toBe(NOT_ENOUGH);
  expect(line(r, "connectors-out-left")).toBe("7 credits left");
  saysOut(r);
});

test("out of credit with 0 last read: No credits, and no figure", async () => {
  const r = await open(OUT, 0);
  expect(line(r, "connectors-out")).toBe(NONE);
  expect(line(r, "connectors-out-left")).toBeNull();
  expect(r.el.querySelector('[data-testid="connectors-balance"]')).toBeNull();
  saysOut(r);
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
