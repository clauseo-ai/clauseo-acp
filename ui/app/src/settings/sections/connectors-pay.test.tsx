// @vitest-environment happy-dom
// Settings, Connectors (U15): Add credits' packs, the pay link's guard with a faked clock, and the API key field.
// The guard: a new pay_url that arrives within 30 s of "Continue to payment", with the same view still open, sends
// open_pay_link once; any other arrival shows "Open the payment page" and sends nothing until it's pressed.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type {
  CommandEnvelope,
  CommandOutcome,
  ConnectorCommand,
} from "../../transport/protocol.ts";
import { connectorsStoreOf } from "../../store/connectors.ts";
import { mountSettings, PACKS, SIGNED_IN, stateOf, type Rig } from "./connectors-rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

const ID = "case-law";
const URL_A = "https://data.clauseo.chat/pay/a";
const URL_B = "https://data.clauseo.chat/pay/b";
const T0 = new Date(2026, 10, 6, 12).getTime();
let rig: Rig | null = null;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(T0);
});

afterEach(async () => {
  await rig?.unmount();
  rig = null;
  vi.useRealTimers();
});

const opens = (r: Rig) => r.transport.connectorCommands().filter((c) => "open_pay_link" in c);
const later = (ms: number) => vi.setSystemTime(T0 + ms);

/** Signed in, the packs open and Continue pressed at T0 on the first pack. */
async function pressed(over: Parameters<typeof stateOf>[0] = {}): Promise<Rig> {
  const r = await mountSettings({
    state: stateOf({ status: SIGNED_IN, ...over }),
    hash: "#connectors",
  });
  rig = r;
  await r.press("Add credits");
  await r.press("Continue to payment");
  return r;
}

test("the packs: one radio per pack in the host's order, the GST line, and Continue sends add_credits with the chosen pack", async () => {
  const r = await mountSettings({ state: stateOf({ status: SIGNED_IN }), hash: "#connectors" });
  rig = r;
  await r.press("Add credits");
  const packs = r.el.querySelector('[data-testid="connectors-packs"]');
  expect(packs?.textContent).toContain("Choose a pack");
  expect(packs?.textContent).toContain("Prices include GST.");
  const labels = [...(packs?.querySelectorAll("label") ?? [])].map((l) => l.textContent);
  expect(labels).toEqual([
    "200 credits, ₹23.60",
    "1,000 credits, ₹118",
    "10,00,000 credits, ₹1,18,000",
  ]);
  expect(packs?.querySelectorAll('[role="radio"]')).toHaveLength(PACKS.length);
  expect(r.buttons()).toContain("Continue to payment");
  expect(r.buttons()).toContain("Cancel");
  expect(r.buttons()).not.toContain("Sign out");
  const second = packs?.querySelectorAll<HTMLElement>('[role="radio"]')[1];
  await act(async () => second?.click());
  await r.press("Continue to payment");
  expect(r.transport.connectorCommands().slice(1)).toEqual([
    { add_credits: { id: ID, pack: "p1k" } },
  ]);
});

test("a new pay link 5 s after the press, the view still open, opens once; a repeat of the state sends nothing more", async () => {
  const r = await pressed();
  later(5_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  expect(opens(r)).toEqual([{ open_pay_link: { id: ID } }]);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  expect(opens(r)).toHaveLength(1);
  expect(r.text()).toContain("The payment page is open in your browser.");
  expect(r.text()).toContain("Your balance updates here once the payment goes through.");
  await r.press("Open the payment page again");
  expect(opens(r)).toHaveLength(2);
});

test("at 31 s it sends nothing and shows Open the payment page, whose press sends one open_pay_link", async () => {
  const r = await pressed();
  later(31_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  expect(opens(r)).toEqual([]);
  expect(r.buttons()).toContain("Open the payment page");
  expect(r.text()).not.toContain("The payment page is open in your browser.");
  await r.press("Open the payment page");
  expect(opens(r)).toEqual([{ open_pay_link: { id: ID } }]);
  expect(r.buttons()).toContain("Open the payment page again");
});

test("after the section closed and reopened, nothing opens: the button waits", async () => {
  const r = await pressed();
  await r.side("Appearance");
  await r.side("Connectors");
  later(5_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  expect(opens(r)).toEqual([]);
  expect(r.buttons()).toContain("Open the payment page");
});

test("arriving while the window shows another section, nothing opens, then or on return", async () => {
  const r = await pressed();
  await r.side("Appearance");
  later(5_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  await r.side("Connectors");
  expect(opens(r)).toEqual([]);
  expect(r.buttons()).toContain("Open the payment page");
});

test("after Cancel, or another pack chosen after the press, nothing opens", async () => {
  const r = await pressed();
  await r.press("Cancel");
  later(5_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  expect(opens(r)).toEqual([]);
  expect(r.buttons()).toContain("Open the payment page");
  // a second press, then a change of pack: the guard drops
  await r.press("Add credits");
  vi.setSystemTime(T0 + 10_000);
  await r.press("Continue to payment");
  const radios = r.el.querySelectorAll<HTMLElement>(
    '[data-testid="connectors-packs"] [role="radio"]',
  );
  await act(async () => radios[2]?.click());
  vi.setSystemTime(T0 + 12_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_B }));
  expect(opens(r)).toEqual([]);
});

test("the same link as before the press opens nothing; Open the payment page again shows only while pay_url is set", async () => {
  const r = await pressed({ pay_url: URL_A });
  later(2_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_A }));
  expect(opens(r)).toEqual([]);
  later(4_000);
  await r.play(stateOf({ status: SIGNED_IN, pay_url: URL_B }));
  expect(opens(r)).toHaveLength(1);
  expect(r.buttons()).toContain("Open the payment page again");
  await r.play(stateOf({ status: SIGNED_IN }));
  expect(r.buttons()).not.toContain("Open the payment page again");
  expect(r.buttons()).not.toContain("Open the payment page");
  expect(r.el.querySelector('[data-testid="connectors-balance"]')).not.toBeNull();
});

test("with out_of_credit's own pay_url and no view pay_url, no control sends open_pay_link", async () => {
  const r = await mountSettings({
    state: stateOf({ status: { out_of_credit: { pay_url: URL_A } } }),
    hash: "#connectors",
  });
  rig = r;
  for (const words of r.buttons()) {
    if (words === "Remove" || words === "Sign out") continue;
    await r.press(words);
  }
  for (const words of r.buttons()) if (words !== "Continue to payment") await r.press(words);
  expect(opens(r)).toEqual([]);
});

/** Puts words in the key field as typing would leave them: in the field's own DOM value. */
async function type(r: Rig, words: string) {
  const input = r.el.querySelector<HTMLInputElement>('input[type="password"]');
  expect(input).not.toBeNull();
  await act(async () => {
    input!.value = words;
  });
  return input!;
}

test("the key: a password field; Add key sends add_api_key with the typed text, and the field is empty after", async () => {
  const KEY = "pasted-review-key-123";
  const r = await mountSettings({ state: stateOf({}), hash: "#connectors" });
  rig = r;
  await r.press("Use an API key instead");
  const field = r.el.querySelector('[data-testid="connectors-key"]');
  expect(field?.textContent).toContain("API key");
  expect(field?.textContent).toContain(
    "Your firm's key for Clauseo's case-law service. Kept on this computer.",
  );
  const input = await type(r, KEY);
  expect(input.getAttribute("autocomplete")).toBe("off");
  expect(input.getAttribute("spellcheck")).toBe("false");
  expect(r.buttons()).toEqual(["Add key", "Cancel"]);
  await r.press("Add key");
  expect(r.transport.connectorCommands().slice(1)).toEqual([{ add_api_key: { id: ID, key: KEY } }]);
  expect(input.value).toBe("");
  expect(document.body.textContent).not.toContain(KEY);
  expect(JSON.stringify(connectorsStoreOf(r.transport).state())).not.toContain(KEY);
  expect(JSON.stringify(r.store.ui())).not.toContain(KEY);
  expect(JSON.stringify(r.store.tools())).not.toContain(KEY);
});

test("the key: Add key is off while the command is out; a refused key stays in the field, and Cancel closes it", async () => {
  let answer: { ok: () => void; refuse: () => void } | null = null;
  const dispatch = (command: CommandEnvelope): Promise<CommandOutcome> => {
    const c = command.command.connector as ConnectorCommand | undefined;
    if (c === undefined || !("add_api_key" in c)) return Promise.resolve("done");
    return new Promise((resolve, reject) => {
      answer = {
        ok: () => resolve("done"),
        refuse: () => reject({ code: "command.invalid", message: "bad key", retryable: false }),
      };
    });
  };
  const r = await mountSettings({
    state: stateOf({ status: "key_revoked" }),
    hash: "#connectors",
    dispatch,
  });
  rig = r;
  await r.press("Add a new key");
  const input = await type(r, "wrong-key");
  const add = () =>
    [...r.el.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "Add key",
    );
  vi.spyOn(console, "warn").mockImplementation(() => {});
  await r.press("Add key");
  expect(add()?.disabled).toBe(true);
  await act(async () => answer?.refuse());
  expect(add()?.disabled).toBe(false);
  expect(input.value).toBe("wrong-key");
  // the console names the command's kind and the code, never the key
  expect(JSON.stringify(vi.mocked(console.warn).mock.calls)).not.toContain("wrong-key");
  await r.press("Cancel");
  expect(r.el.querySelector('input[type="password"]')).toBeNull();
  expect(r.buttons()).toContain("Add a new key");
});
