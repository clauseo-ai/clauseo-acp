// @vitest-environment happy-dom
// The window split for setup commands (UD2; crates/desktop/src/setup/admit.rs): the settings window may send all
// fifteen, the main window the nine sign-in ones and a first key, and desktop refuses SignOut, ClearAgentKey,
// SetFirmEndpoint, FindLogin and StopUsingLogin from it (transport.unauthorized). Over the room's first run in every
// sign-in state, with every button of the gate and the install line pressed, the main window sends none of those
// five; no file under room/ names one or imports Plans; and Plans, with every button pressed in every state, sends no
// host command but the window's own connectors check (no scan, no matter, no thread).
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, vi } from "vitest";
import {
  AT,
  CLAUDE,
  READY,
  info,
  install,
  mountFirstRun,
  press,
  setups,
  signIn,
  stubClipboard,
} from "../room/first-run/rig.tsx";
import { roomSettingsOf } from "../room/host-wire.ts";
import type { SettingsView, SetupCommand, SignIn } from "../transport/protocol.ts";
import * as plans from "./sections/plans-rig.tsx";

vi.mock("../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

/** The commands only the settings window may send. */
const SETTINGS_ONLY = [
  "sign_out",
  "clear_agent_key",
  "set_firm_endpoint",
  "find_login",
  "stop_using_login",
] as const satisfies readonly (keyof SetupCommand)[];

/** What the main window may send: the nine sign-in commands, and a first key. */
const MAIN = new Set<string>([
  "install_agent",
  "retry_agent_install",
  "start_sign_in",
  "cancel_sign_in",
  "paste_sign_in_code",
  "use_device_code",
  "open_sign_in_page",
  "use_found_login",
  "use_different_account",
  "set_agent_key",
]);

/** Every sign-in state the gate can show. */
const STATES: [string, SignIn][] = [
  ["signed out", { signed_out: {} }],
  ["found, reused_failed", { signed_out: { found: "reused_failed" } }],
  ["found, claude_settings", { signed_out: { found: "claude_settings" } }],
  ["queued", "queued"],
  ["waiting", { waiting: { since: AT, nudge: true } }],
  ["device code", { device_code: { url: "https://x", code: "ABCD-1234", expires_at: AT } }],
  ["login found", { login_found: { from: "claude_code", email: "a@b.in" } }],
  ["failed", { failed: { code: "signin.cancelled", at: AT } }],
  ["no plan", { failed: { code: "signin.no_plan", at: AT } }],
  ["key unreadable", { failed: { code: "agent.key_unreadable", at: AT } }],
  ["switching", { switching: { since: AT, stuck: true } }],
  ["signed in, reused", plans.signed({ source: "reused" })],
];

/** Presses every enabled button in the gate, the install line and the room's line, twice over (a press opens more). */
async function pressAll() {
  for (let round = 0; round < 3; round += 1)
    for (const el of document.querySelectorAll<HTMLButtonElement>(
      '[data-testid="signin"] button, [data-testid="install-foot"] button',
    ))
      if (el.isConnected && !el.disabled) await press(el);
}

test.each(STATES)(
  "the main window, %s: every press sends only the sign-in commands",
  async (_, state) => {
    stubClipboard();
    const t = await mountFirstRun({
      first: [info(), install(CLAUDE, READY), signIn(CLAUDE, state)],
    });
    await pressAll();
    const kinds = setups(t).map((c) => Object.keys(c)[0]!);
    expect(kinds.filter((k) => !MAIN.has(k))).toEqual([]);
    for (const kind of SETTINGS_ONLY) expect(kinds).not.toContain(kind);
  },
);

test("no file under room/, nor the room's entry, names a settings-only setup command or imports Plans", () => {
  const src = join(import.meta.dirname, "..");
  const files = readdirSync(join(src, "room"), { recursive: true })
    .map((f) => join(src, "room", String(f)))
    .filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f));
  expect(files.length).toBeGreaterThan(50);
  const named = new RegExp(`\\b(${SETTINGS_ONLY.join("|")})\\s*:`);
  for (const file of [...files, join(src, "main.tsx"), join(src, "App.tsx")]) {
    const text = readFileSync(file, "utf8");
    expect(text, file).not.toMatch(named);
    expect(text, file).not.toMatch(/settings\/sections\/plans/);
  }
});

test.each(STATES)(
  "Plans, %s: every press sends setup commands only, and no host command",
  async (_, state) => {
    const t = await plans.mountPlans({
      first: [install(CLAUDE, READY), signIn(CLAUDE, state)],
    });
    for (let round = 0; round < 2; round += 1)
      for (const el of document.querySelectorAll<HTMLButtonElement>(
        '[data-testid="plans-row"] button',
      ))
        if (el.isConnected && !el.disabled) {
          await plans.press(el);
          // a confirm's own act, in its dialog
          const yes = document.querySelector<HTMLButtonElement>('[data-testid$="-confirm"]');
          if (yes !== null) await plans.press(yes);
        }
    // the presses happened (switching offers nothing to press but the key's field, empty)
    if (typeof state !== "object" || state.switching === undefined)
      expect(t.setups().length).toBeGreaterThan(0);
    // the window's own connectors check on opening (U15) is the only host command
    expect(plans.hostCommands(t)).toEqual(["connector"]);
    expect(t.connectorCommands()).toEqual([{ check: {} }]);
    await plans.unmount();
  },
);

test("the firm's plans reach the settings window: roomSettingsOf carries SettingsView.plans", () => {
  const view = {
    ui: { theme: "system", font_size: 16, language: "en", lite: "automatic", motion: "system" },
    tools: { web_search: true, browser: true },
    privacy: { counts: true, crash_reports: false, counts_started: true },
    locked: [],
    plans: { [CLAUDE]: { endpoint: "https://ai.example.in" } },
  } as unknown as SettingsView;
  expect(roomSettingsOf(view).plans).toEqual({ [CLAUDE]: { endpoint: "https://ai.example.in" } });
  expect("plans" in roomSettingsOf({ ...view, plans: undefined } as unknown as SettingsView)).toBe(
    false,
  );
});
