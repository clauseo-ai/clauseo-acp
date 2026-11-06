// @vitest-environment happy-dom
// Settings, Connectors (U15), one test per state of the brief's Screens table: the words, the side's note, the buttons
// offered, and each button sending exactly its one command with `id: "case-law"`. Then the window's `check` on open,
// a section hidden when nothing is listed, and the lawyer's own switch apart from the account's actions. The packs,
// the pay link's guard and the key are connectors-pay.test.tsx's.
import { afterEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { ConnectorCommand } from "../../transport/protocol.ts";
import { mountSettings, SIGNED_IN, stateOf, type Rig } from "./connectors-rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

const ID = "case-law";
let rig: Rig | null = null;

afterEach(async () => {
  await rig?.unmount();
  rig = null;
  vi.useRealTimers();
});

const open = async (state: Parameters<typeof stateOf>[0], top?: Parameters<typeof stateOf>[1]) => {
  rig = await mountSettings({ state: stateOf(state, top), hash: "#connectors" });
  return rig;
};

/** The connector commands sent after the window's own check. */
const sent = (r: Rig): ConnectorCommand[] => r.transport.connectorCommands().slice(1);

/** Presses `words` and expects exactly `command` to go out. */
async function sends(r: Rig, words: string, command: ConnectorCommand) {
  const before = sent(r).length;
  await r.press(words);
  expect(sent(r).slice(before)).toEqual([command]);
}

test("not added: the name, what it covers, the price, Add with its line, the key link; the side says Not added", async () => {
  const r = await open({});
  expect(r.text()).toContain("Indian Kanoon");
  expect(r.text()).toContain(
    "Judgments of the Supreme Court, the High Courts and tribunals, with statutes.",
  );
  expect(r.text()).toContain("Credits are bought in packs, from ₹23.60, including GST.");
  expect(r.text()).toContain("Your browser opens to sign in.");
  expect(r.note("Connectors")).toBe("Not added");
  expect(r.buttons()).toEqual(["Add", "Use an API key instead"]);
  await sends(r, "Add", { add: { id: ID } });
  // no price on the wire: no price line
  await r.play({
    available: true,
    locked: false,
    connectors: [{ id: ID, name: "Indian Kanoon", packs: [], status: "not_added" }],
  });
  expect(r.text()).not.toContain("Credits are bought in packs");
  // the section's head and its lead
  expect(r.el.querySelector("h2")?.textContent).toBe("Connectors");
  expect(r.text()).toContain("Paid sources Clauseo can search while it answers.");
});

test("signing in: the line, and Open the sign-in page again sends add again", async () => {
  const r = await open({ status: "signing_in" });
  expect(r.text()).toContain("Finish signing in on the page your browser opened.");
  expect(r.note("Connectors")).toBe("Signing in");
  expect(r.buttons()).toEqual(["Open the sign-in page again", "Turn off"]);
  await sends(r, "Open the sign-in page again", { add: { id: ID } });
});

test("signed in with a balance: credits as the numeral, the account's actions, the side gives the credits; no rupees anywhere", async () => {
  const r = await open({ status: SIGNED_IN });
  expect(r.text()).toContain("Ready to search.");
  const tile = r.el.querySelector('[data-testid="connectors-balance"]');
  expect(tile?.querySelector(".set-numeral")?.textContent).toBe("10,000");
  expect(tile?.querySelector(".set-window-used")?.getAttribute("aria-label")).toBe(
    "10,000 credits left",
  );
  // Balance.paise is the account's figure before GST: never on screen (every amount shown is GST-inclusive)
  expect(tile?.textContent).not.toContain("Worth");
  expect(r.text()).not.toContain("₹");
  expect(r.note("Connectors")).toBe("10,000 credits left");
  expect(r.buttons()).toEqual(["Add credits", "Sign out", "Remove", "Turn off"]);
  await sends(r, "Sign out", { sign_out: { id: ID } });
  await sends(r, "Remove", { remove: { id: ID } });
});

test("signed in with no rupee figure: the same credits, no Worth line", async () => {
  const r = await open({ status: { signed_in: { balance: { credits: 10_000 }, low: false } } });
  expect(r.text()).toContain("10,000");
  expect(r.text()).not.toContain("Worth");
  expect(r.note("Connectors")).toBe("10,000 credits left");
});

test("signed in before the first read: Balance not read yet, Check balance sends refresh_balance, no side note", async () => {
  const r = await open({ status: { signed_in: { low: false } } });
  expect(r.text()).toContain("Ready to search.");
  expect(r.text()).toContain("Balance not read yet.");
  expect(r.note("Connectors")).toBeUndefined();
  await sends(r, "Check balance", { refresh_balance: { id: ID } });
});

test("low: the balance in credits, the running-low line in primary, Add credits first", async () => {
  const r = await open({
    status: { signed_in: { balance: { credits: 180, paise: 1_800 }, low: true } },
  });
  expect(r.el.querySelector(".set-numeral")?.textContent).toBe("180");
  expect(r.text()).not.toContain("₹");
  const low = r.el.querySelector('[data-testid="connectors-low"]');
  expect(low?.textContent).toBe("Running low. Add credits to keep searching.");
  expect(low?.className).toContain("text-primary");
  expect(r.el.querySelector('[data-testid="connectors-balance"]')?.getAttribute("data-state")).toBe(
    "close",
  );
  expect(r.buttons()[0]).toBe("Add credits");
});

test("recent spend: today first, then each readable day newest first, credits only; nothing spent; no group without it", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 6, 12));
  const r = await open({
    status: SIGNED_IN,
    recent_spend: {
      today_credits: 340,
      days: [
        { day: "2026-11-03", credits: 500 },
        { day: "2026-11-04", credits: 1_200 },
        { day: "not a day", credits: 9 },
        { day: "2026-02-30", credits: 9 },
      ],
    },
  });
  const group = [...r.el.querySelectorAll("section.set-group")].find(
    (s) => s.querySelector("h3")?.textContent === "Recent spend",
  );
  const rows = [...(group?.querySelectorAll(".set-cn-spend") ?? [])].map((row) =>
    [...row.querySelectorAll("p")].map((p) => p.textContent),
  );
  expect(rows).toEqual([
    ["Today", "340 credits"],
    ["Wednesday 4 November", "1,200 credits"],
    ["Tuesday 3 November", "500 credits"],
  ]);
  expect(group?.textContent).not.toContain("₹");
  await r.play(stateOf({ status: SIGNED_IN, recent_spend: { today_credits: 0, days: [] } }));
  expect(r.el.querySelector('[data-testid="spend-none"]')?.textContent).toBe("Nothing spent yet.");
  await r.play(stateOf({ status: SIGNED_IN }));
  expect(r.text()).not.toContain("Recent spend");
});

test("out of credit: the line, Add credits first, then Sign out and Remove; its own pay_url is never a control", async () => {
  const r = await open({
    status: { out_of_credit: { pay_url: "https://data.clauseo.chat/pay/x" } },
  });
  expect(r.text()).toContain("No credits left. Case-law searches stop until you add more.");
  expect(r.note("Connectors")).toBe("Out of credit");
  expect(r.buttons()).toEqual(["Add credits", "Sign out", "Remove", "Turn off"]);
});

test("signed out: the line, Sign in sends add, the key link, and Remove", async () => {
  const r = await open({ status: "signed_out" });
  expect(r.text()).toContain("Signed out. Sign in to search case law again.");
  expect(r.note("Connectors")).toBe("Signed out");
  expect(r.buttons()).toEqual(["Sign in", "Use an API key instead", "Remove", "Turn off"]);
  await sends(r, "Sign in", { add: { id: ID } });
  await sends(r, "Remove", { remove: { id: ID } });
});

test("key revoked: the line, Add a new key opens the key field, Sign in sends add, and Remove", async () => {
  const r = await open({ status: "key_revoked" });
  expect(r.text()).toContain("This API key no longer works. Add a new key, or sign in instead.");
  expect(r.note("Connectors")).toBe("Key no longer works");
  expect(r.buttons()).toEqual(["Add a new key", "Sign in", "Remove", "Turn off"]);
  await sends(r, "Sign in", { add: { id: ID } });
  await r.press("Add a new key");
  expect(r.el.querySelector('input[type="password"]')).not.toBeNull();
  expect(sent(r)).toHaveLength(1);
});

test("error: the line, Try again sends add, and the host's message only in the Details fold, in English", async () => {
  const message = "token exchange failed: 502";
  const r = await open({ status: { error: { message } } });
  expect(r.text()).toContain(
    "The sign-in didn't go through. Try again, or use an API key instead.",
  );
  expect(r.note("Connectors")).toBe("Sign-in failed");
  const fold = r.el.querySelector("details.set-cn-details");
  expect(fold?.querySelector("summary")?.textContent).toBe("Details");
  expect(fold?.querySelector('p[lang="en"]')?.textContent).toBe(message);
  expect(document.body.textContent?.split(message)).toHaveLength(2);
  await sends(r, "Try again", { add: { id: ID } });
});

test("locked by the firm: the line, no Add, no packs, no switch; an added connector still offers Sign out and Remove", async () => {
  const r = await open({}, { available: false, locked: true });
  expect(r.el.querySelector('[data-testid="connectors-off"]')?.textContent).toBe(
    "Turned off by your firm.",
  );
  expect(r.note("Connectors")).toBe("Turned off by your firm");
  expect(r.buttons()).toEqual([]);
  await r.play(stateOf({ status: SIGNED_IN }, { available: false, locked: true }));
  expect(r.buttons()).toEqual(["Sign out", "Remove"]);
  expect(r.text()).not.toContain("10,000");
  await sends(r, "Sign out", { sign_out: { id: ID } });
});

test("the lawyer's own off: the line, Turn on sends set_tool_settings { connectors: true }; added still offers Sign out and Remove", async () => {
  const r = await open({ status: SIGNED_IN }, { available: false, locked: false });
  expect(r.text()).toContain("Turned off on this computer.");
  expect(r.note("Connectors")).toBe("Turned off on this computer");
  expect(r.buttons()).toEqual(["Turn on", "Sign out", "Remove"]);
  await r.press("Turn on");
  expect(r.transport.only("next_command").map((c) => c.command)).toEqual([
    { set_tool_settings: { patch: { connectors: true } } },
  ]);
  expect(sent(r)).toEqual([]);
});

test("on, added: Turn off sits in On this computer, apart from Sign out and Remove, and sends connectors: false; absent locked or not added", async () => {
  const r = await open({ status: SIGNED_IN });
  const group = r.el
    .querySelector('[data-testid="connectors-switch"]')
    ?.closest("section.set-group");
  expect(group?.querySelector("h3")?.textContent).toBe("On this computer");
  expect(group?.textContent).toContain("Case-law searches");
  expect(group?.textContent).toContain(
    "Turning off stops case-law searches on this computer. Your credits stay.",
  );
  const turnOff = [...(group?.querySelectorAll("button") ?? [])].map((b) => b.textContent);
  expect(turnOff).toEqual(["Turn off"]);
  const accountRow = [...r.el.querySelectorAll(".set-cn-actions")].find((row) =>
    row.textContent?.includes("Sign out"),
  );
  expect(accountRow?.textContent).not.toContain("Turn off");
  // the group stands after the block and before what each search sends
  const heads = [...r.el.querySelectorAll("main h3")].map((h) => h.textContent);
  expect(heads).toEqual(["Indian Kanoon", "On this computer", "What each search sends"]);
  await r.press("Turn off");
  expect(r.transport.only("next_command").map((c) => c.command)).toEqual([
    { set_tool_settings: { patch: { connectors: false } } },
  ]);
  await r.play(stateOf({ status: SIGNED_IN }, { locked: true }));
  expect(r.buttons()).not.toContain("Turn off");
  await r.play(stateOf({}));
  expect(r.buttons()).not.toContain("Turn off");
});

test("what each search sends shows in every state, naming both agents when the window knows no plan", async () => {
  const r = await open({});
  const sends = r.el.querySelector('[data-testid="connectors-sends"]');
  expect([...(sends?.querySelectorAll("li") ?? [])].map((li) => li.textContent)).toEqual([
    "The words of each search go to Clauseo's server, which sends them on to Indian Kanoon.",
    "Papers aren't sent. Clauseo asks Claude and ChatGPT to keep client names and confidential facts out of its searches.",
    "Clauseo's server deletes the words of each search after 7 days.",
    "Indian Kanoon sees Clauseo's server, never you.",
    "Each search and each judgment read costs credits.",
  ]);
  await r.play(stateOf({}, { available: false, locked: true }));
  expect(r.el.querySelector('[data-testid="connectors-sends"]')).not.toBeNull();
});

test("not available: no Connectors on the side, and #connectors opens the first section", async () => {
  rig = await mountSettings({
    state: { available: true, locked: false, connectors: [] },
    hash: "#connectors",
  });
  const names = [...rig.el.querySelectorAll(".set-nav-name")].map((n) => n.textContent);
  expect(names).not.toContain("Connectors");
  expect(rig.el.querySelector("h2")?.textContent).toBe(names[0]);
  expect(rig.el.querySelector('[data-testid="connectors-section"]')).toBeNull();
});

test("check: one on mount, before any state, and none on a section change", async () => {
  rig = await mountSettings();
  expect(rig.transport.connectorCommands()).toEqual([{ check: {} }]);
  await rig.play(stateOf({ status: SIGNED_IN }));
  await rig.side("Connectors");
  await rig.side("Appearance");
  await rig.side("Connectors");
  expect(rig.transport.connectorCommands()).toEqual([{ check: {} }]);
});

test("zero timers after any state", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "setInterval"] });
  const r = await open({ status: SIGNED_IN });
  for (const status of ["not_added", "signing_in", "signed_out", "key_revoked"] as const)
    await r.play(stateOf({ status }));
  await r.play(stateOf({ status: SIGNED_IN, pay_url: "https://data.clauseo.chat/pay/1" }));
  expect(vi.getTimerCount()).toBe(0);
});
