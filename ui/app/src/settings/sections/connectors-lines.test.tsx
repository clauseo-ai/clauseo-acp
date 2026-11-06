// @vitest-environment happy-dom
// Settings, Connectors (U15), the lines the host's new fields add (part 3): the balance's worth and when it was read,
// the spend in rupees, the pasted key's line, a sign-in that could not be saved, and how the last recharge ended. Each
// shows when its field is set, and today's line stays when it is absent. No line says "+ GST" (every amount is
// GST-inclusive, the orchestrator's ruling of 2026-11-06).
import { afterEach, expect, test, vi } from "vitest";
import { HI } from "@clauseo/components/lib/words-connectors.hi.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import type { Balance, ConnectorView, ConnectorsState } from "../../transport/protocol.ts";
import { mountSettings, SIGNED_IN, stateOf, type Rig } from "./connectors-rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

const NOW = new Date(2026, 10, 6, 16, 31);
const READ = new Date(2026, 10, 6, 16, 29).getTime();
let rig: Rig | null = null;

afterEach(async () => {
  await rig?.unmount();
  rig = null;
  vi.useRealTimers();
});

/** Mounts the window at #connectors with `state` as the catch-up, the clock at NOW. */
async function open(state: ConnectorsState) {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  rig = await mountSettings({ state, hash: "#connectors" });
  return rig;
}

/** A signed-in status with `balance`, the sign-in's method when given. */
const balanceOf = (balance: Balance, method?: "account" | "api_key"): ConnectorView["status"] => ({
  signed_in: method === undefined ? { balance, low: false } : { balance, low: false, method },
});

const line = (r: Rig, testId: string) =>
  r.el.querySelector(`[data-testid="${testId}"]`)?.textContent;

test("the balance shows its worth in rupees, GST-inclusive, and when the account was read", async () => {
  const r = await open(
    stateOf({
      status: {
        signed_in: { balance: { credits: 10_000, paise: 1_18_000, as_of: READ }, low: false },
      },
    }),
  );
  expect(line(r, "connectors-worth")).toBe("Worth ₹1,180.");
  expect(line(r, "connectors-as-of")).toBe("As of 4:29 pm.");
  // the credits and the side's note stay as they were
  expect(r.el.querySelector(".set-numeral")?.textContent).toBe("10,000");
  expect(r.note("Connectors")).toBe("10,000 credits left");
});

test("a balance with neither field keeps today's line: the credits, no worth, no as-of", async () => {
  const r = await open(stateOf({ status: balanceOf({ credits: 10_000 }) }));
  expect(line(r, "connectors-worth")).toBeUndefined();
  expect(line(r, "connectors-as-of")).toBeUndefined();
  expect(r.el.querySelector(".set-numeral")?.textContent).toBe("10,000");
  expect(r.text()).toContain("Ready to search.");
});

test("each field on its own: a worth without a read time, and a read time without a worth", async () => {
  let r = await open(stateOf({ status: balanceOf({ credits: 180, paise: 2_124 }) }));
  expect(line(r, "connectors-worth")).toBe("Worth ₹21.24.");
  expect(line(r, "connectors-as-of")).toBeUndefined();
  await r.play(stateOf({ status: balanceOf({ credits: 180, as_of: READ }) }));
  expect(line(r, "connectors-worth")).toBeUndefined();
  expect(line(r, "connectors-as-of")).toBe("As of 4:29 pm.");
});

test("a pasted key says Using your firm's key.; an account, or an older host's, keeps Ready to search.", async () => {
  let r = await open(stateOf({ status: balanceOf({ credits: 10_000 }, "api_key") }));
  expect(r.text()).toContain("Using your firm's key.");
  expect(r.text()).not.toContain("Ready to search.");
  await r.play(stateOf({ status: balanceOf({ credits: 10_000 }, "account") }));
  expect(r.text()).toContain("Ready to search.");
  expect(r.text()).not.toContain("Using your firm's key.");
  await r.play(stateOf({ status: SIGNED_IN }));
  expect(r.text()).toContain("Ready to search.");
});

test("a sign-in that could not be saved says so; the side says nothing; Try again stays; a failed sign-in keeps its line", async () => {
  const message = "the keychain refused the credential twice";
  let r = await open(stateOf({ status: { error: { message, kind: "persist_failed" } } }));
  expect(r.text()).toContain(
    "You're signed in, but Clauseo couldn't save the sign-in on this computer. Sign in again after Clauseo restarts.",
  );
  expect(r.text()).not.toContain("The sign-in didn't go through.");
  expect(r.note("Connectors")).toBeUndefined();
  expect(r.buttons()).toEqual(["Try again", "Use an API key instead", "Turn off"]);
  // the host's English message still sits in the Details fold, never on the line
  expect(r.el.querySelector('details.set-cn-details p[lang="en"]')?.textContent).toBe(message);
  await r.play(stateOf({ status: { error: { message, kind: "sign_in_failed" } } }));
  expect(r.text()).toContain(
    "The sign-in didn't go through. Try again, or use an API key instead.",
  );
  expect(r.note("Connectors")).toBe("Sign-in failed");
  await r.play(stateOf({ status: { error: { message } } }));
  expect(r.text()).toContain(
    "The sign-in didn't go through. Try again, or use an API key instead.",
  );
  expect(r.note("Connectors")).toBe("Sign-in failed");
});

test("the last recharge shows as a quiet line after the balance, for each outcome, and goes when the field clears", async () => {
  const cases: [NonNullable<ConnectorView["last_recharge"]>, string][] = [
    ["paid", "Payment received."],
    ["expired", "That payment page expired. Add credits to try again."],
    ["cancelled", "The payment was cancelled."],
  ];
  let r = await open(stateOf({ status: SIGNED_IN }));
  expect(line(r, "connectors-recharge")).toBeUndefined();
  for (const [outcome, words] of cases) {
    await r.play(stateOf({ status: SIGNED_IN, last_recharge: outcome }));
    const el = r.el.querySelector('[data-testid="connectors-recharge"]');
    expect(el?.textContent).toBe(words);
    expect(el?.className).toContain("set-note");
    // after the balance tile in the block
    const tile = r.el.querySelector('[data-testid="connectors-balance"]');
    const after =
      tile !== null &&
      el !== null &&
      (tile.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
    expect(after).toBe(true);
  }
  await r.play(stateOf({ status: SIGNED_IN }));
  expect(line(r, "connectors-recharge")).toBeUndefined();
});

test("the last recharge also shows out of credit, where the balance is the line", async () => {
  const r = await open(stateOf({ status: { out_of_credit: {} }, last_recharge: "expired" }));
  expect(line(r, "connectors-recharge")).toBe(
    "That payment page expired. Add credits to try again.",
  );
  expect(r.text()).toContain("No credits left. Case-law searches stop until you add more.");
});

test("spend shows its rupees when the host sends them, today and each day; credits alone when it does not", async () => {
  const withRupees = await open(
    stateOf({
      status: SIGNED_IN,
      recent_spend: {
        today_credits: 340,
        today_paise: 4_012,
        days: [
          { day: "2026-11-04", credits: 1_200, paise: 14_160 },
          { day: "2026-11-03", credits: 560 },
        ],
      },
    }),
  );
  const rows = () =>
    [...withRupees.el.querySelectorAll(".set-cn-spend")].map((row) =>
      [...row.querySelectorAll("p")].map((p) => p.textContent),
    );
  expect(rows()).toEqual([
    ["Today", "340 credits, ₹40.12"],
    ["Wednesday 4 November", "1,200 credits, ₹141.60"],
    ["Tuesday 3 November", "560 credits"],
  ]);
  await withRupees.play(
    stateOf({
      status: SIGNED_IN,
      recent_spend: { today_credits: 340, days: [{ day: "2026-11-04", credits: 1_200 }] },
    }),
  );
  expect(rows()).toEqual([
    ["Today", "340 credits"],
    ["Wednesday 4 November", "1,200 credits"],
  ]);
});

test("no line the section can show says + GST, and every amount in the words is GST-inclusive", async () => {
  for (const key of Object.keys(HI)) expect(key).not.toMatch(/\+\s*GST/);
  const r = await open(
    stateOf({
      status: {
        signed_in: {
          balance: { credits: 10_000, paise: 1_18_000, as_of: READ },
          low: false,
          method: "api_key",
        },
      },
      recent_spend: { today_credits: 340, today_paise: 4_012, days: [] },
      last_recharge: "paid",
    }),
  );
  expect(r.text()).not.toMatch(/\+\s*GST/);
  await r.play(stateOf({ status: { error: { message: "x", kind: "persist_failed" } } }));
  expect(r.text()).not.toMatch(/\+\s*GST/);
  await r.play(stateOf({ status: "not_added" }));
  expect(r.text()).not.toMatch(/\+\s*GST/);
});
