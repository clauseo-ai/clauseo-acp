// @vitest-environment happy-dom
// Settings, Plan and usage with the setup wire (D-060): the host reads a Claude with no sign-in and no key as billed
// by use (no subscription, no limits), so a plan the setup wire says isn't signed in shows nothing, in the section and
// in the note beside its name; signed in with a key, it shows who bills as before; with no setup wire, as before.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { AgentLimitsView, SetupEvent, SignIn } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

const items = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => items.get(key) ?? null,
  setItem: (key: string, value: string) => void items.set(key, value),
  removeItem: (key: string) => void items.delete(key),
  clear: () => items.clear(),
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/settings.html#usage");
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  root = null;
  host = null;
  store = null;
});

/** Claude as the host reads it with no subscription and no limits: billed by use, through Anthropic. */
const PAYG: AgentLimitsView = {
  billing: "pay_as_you_go",
  provider: "anthropic",
  windows: [],
  at_limit_text: "",
  freshness: "not_applicable",
};
const SIGNED_OUT: SignIn = { signed_out: {} };
const KEY: SignIn = { signed_in: { method: "api_key", source: "own", at: 0 } };

/** The window at Plan and usage, Claude read as billed by use, over a transport whose setup wire says `signIn` for
 * Claude (none: no setup wire). */
async function mount(signIn?: SignIn) {
  const first: SetupEvent[] =
    signIn === undefined
      ? []
      : [
          { setup_info: { keys_available: true } },
          { sign_in_changed: { agent: "claude-acp", state: signIn } },
        ];
  const transport = new FakeTransport(
    signIn === undefined ? {} : { setup: { first: () => first } },
  );
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  host = el;
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await act(async () => transport.limitsChanged({ claude: PAYG }));
  await act(async () => settled());
  return { transport, el };
}

const card = () => document.querySelector('[data-testid="usage-claude"]');
const note = () =>
  [...document.querySelectorAll(".set-nav-row")]
    .find((r) => r.querySelector(".set-nav-name")?.textContent === "Plan and usage")
    ?.querySelector(".set-nav-note")?.textContent ?? null;

test("a Claude that isn't signed in, with no key, shows no billing: no card and no note", async () => {
  await mount(SIGNED_OUT);
  expect(card()).toBeNull();
  expect(document.body.textContent).not.toContain("Pay as you go");
  expect(document.body.textContent).not.toContain("billed");
  expect(note()).toBeNull();
});

test("signed in with a key, Claude shows who bills, as before", async () => {
  await mount(KEY);
  expect(card()?.textContent).toContain("Pay as you go, billed to your Anthropic account");
  expect(note()).toBe("Pay as you go");
});

test("a sign-in arriving later brings the billing back, and a sign-out takes it away", async () => {
  const { transport } = await mount(SIGNED_OUT);
  expect(card()).toBeNull();
  await act(async () =>
    transport.setupEvent({ sign_in_changed: { agent: "claude-acp", state: KEY } }),
  );
  expect(card()).not.toBeNull();
  await act(async () =>
    transport.setupEvent({ sign_in_changed: { agent: "claude-acp", state: SIGNED_OUT } }),
  );
  expect(card()).toBeNull();
});

test("with no setup wire, billed by use shows as before", async () => {
  await mount();
  expect(card()?.textContent).toContain("Pay as you go, billed to your Anthropic account");
  expect(note()).toBe("Pay as you go");
});
