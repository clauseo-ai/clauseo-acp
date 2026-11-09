// @vitest-environment happy-dom
// R17 PR-D3a in Settings' Safe workspace: a build that sets the Safe workspace up but doesn't start
// Claude or ChatGPT in it yet sends `set_up_state` beside a `computer` of `unsupported { not_built
// }`. The block still says what `computer` says (Not protected, the not-built line, the firm's
// lines), and the set-up state adds its line and its cause's controls. Without `set_up_state` the
// section is exactly as before. Linux's `unsupported { os }` says Linux's line, and only on Linux.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { settled } from "@clauseo/components/lib/late.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import { DEFAULT_UI, type DisplayStore } from "../../room/display-store.ts";
import { np, PROT, view } from "../../room/protection/fixtures.ts";
import { FakeTransport, type FakeProtectionHost } from "../../transport/fake-transport.ts";
import type { Protection, ProtectionView } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  host = null;
  root = null;
  store = null;
  document.body.replaceChildren();
});

async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await act(async () => settled());
  }
}

/** The settings window at the Safe workspace, on `platform`, over `protection`. */
async function mount(
  protection: FakeProtectionHost,
  platform: "mac" | "windows" | "linux" = "windows",
) {
  const settings = { ui: { ...DEFAULT_UI, language: "en" }, locked: [] };
  const transport = new FakeTransport({
    platform,
    query: (query) => {
      if (query === "get_settings") return { settings: settings as never };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    next: { query: () => ({ settings }) as never },
    protection,
  });
  history.replaceState(null, "", "/settings.html#workspace");
  store = connectDisplay(transport).store;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const made = store;
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await settle();
  return transport;
}

const NOT_BUILT = np({ unsupported: { why: "not_built" } });
const PROTECTED_B = { protected: { tier: "b" } } as Protection;

/** A build that sets the Safe workspace up without starting Claude or ChatGPT in it; commands held. */
const setUpHost = (state: Protection | undefined, over: Partial<ProtectionView> = {}) => ({
  view: () =>
    view(NOT_BUILT, {
      ...(state === undefined ? {} : { set_up_state: state }),
      ...over,
    } as Partial<ProtectionView>),
  command: () => new Promise<void>(() => {}),
});

const one = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const block = () => one("settings-workspace");
const text = () => block()?.textContent ?? "";
async function press(el: HTMLElement | null) {
  if (el === null) throw new Error("nothing to press");
  await act(async () => el.click());
  await settle();
}

const NOT_BUILT_LINE = st(
  "en",
  "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo asks before changing anything outside a matter's folder.",
);
const NO_AUTO = st(
  "en",
  "Your firm doesn't allow Clauseo to work without asking on this computer.",
);
const s = (key: SettingsKey) => st("en", key);

/** What `computer` alone decides, which a set-up state never changes: the title, the not-built line, the firm's lines. */
function expectComputerSays() {
  expect(block()?.dataset["state"]).toBe("cant");
  expect(block()?.querySelector(".set-status-title")?.textContent).toBe(s("Not protected"));
  expect(block()?.querySelector(".set-status")?.hasAttribute("data-off")).toBe(true);
  expect(text()).toContain(NOT_BUILT_LINE);
  expect(text()).not.toContain(s("On for every matter"));
}

test("set_up_state absent: the section is exactly as before, the not-built line and nothing more", async () => {
  await mount(setUpHost(undefined));
  const before = block()!.outerHTML;
  expect(block()?.dataset["setUp"]).toBeUndefined();
  expectComputerSays();
  expect(one("protection-set-up")).toBeNull();
  expect(one("protection-setup")).toBeNull();
  expect(one("protection-block")).toBeNull();
  expect([...block()!.querySelectorAll(".set-note")].map((p) => p.textContent)).toEqual([
    NOT_BUILT_LINE,
  ]);
  // a null set_up_state reads the same
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await mount({
    ...setUpHost(undefined),
    view: () => ({ ...view(NOT_BUILT), set_up_state: null }) as never,
  });
  expect(block()!.outerHTML).toBe(before);
});

test.each([
  ["protected (tier a), macOS", PROT.a, "mac"],
  ["protected (tier b), Windows", PROT.b, "windows"],
  ["needs_set_up", PROT.needs, "windows"],
] as const)(
  "set_up_state absent beside %s: the same block as a view without the field",
  async (_, computer, platform) => {
    await mount({ view: () => view(computer) }, platform);
    const plain = block()!.outerHTML;
    await act(async () => root?.unmount());
    document.body.replaceChildren();
    await mount(
      { view: () => ({ ...view(computer), set_up_state: undefined }) as never },
      platform,
    );
    expect(block()!.outerHTML).toBe(plain);
  },
);

test("set_up_state protected: Not protected and the not-built line stay, the set-up line joins them, no control", async () => {
  await mount(setUpHost(PROTECTED_B));
  expectComputerSays();
  expect(block()?.dataset["setUp"]).toBe("protected");
  expect(one("protection-set-up")?.textContent).toBe(
    s("Set up. Clauseo will start using it in an update."),
  );
  expect(one("protection-setup")).toBeNull();
  expect(one("protection-remove")).toBeNull();
  expect(one("protection-installer")).toBeNull();
});

test("set_up_state protected in Hindi says the set-up line in Hindi", () => {
  expect(st("hi", "Set up. Clauseo will start using it in an update.")).toBe(
    "सेट अप हो गया। Clauseo किसी अपडेट में इसका उपयोग शुरू करेगा।",
  );
});

test.each([
  ["protected", PROTECTED_B],
  ["needs_set_up", PROT.needs],
  ["set_up_refused", PROT.refused],
  ["set_up_failed", PROT.failed],
  ["blocked (logon_right)", PROT.logon],
  ["blocked (policy)", PROT.policy],
  ["blocked (admin_copy_missing)", PROT.missing],
  ["other_account", PROT.otherAccount],
] as const)(
  "set_up_state %s: computer still decides the firm's lines, so auto modes stay blocked and required still says so",
  async (_, state) => {
    await mount(setUpHost(state, { unprotected_auto_allowed: false, required: true }));
    expectComputerSays();
    expect(one("protection-no-auto")?.textContent).toBe(NO_AUTO);
    expect(one("protection-required")).not.toBeNull();
  },
);

const SET_UP_CASES = [
  [
    "needs_set_up",
    PROT.needs,
    "Set up the Safe workspace now, so it's ready when Clauseo starts using it in an update.",
    pw("en", "Set up the Safe workspace"),
  ],
  [
    "set_up_refused",
    PROT.refused,
    "Set up the Safe workspace again, so it's ready when Clauseo starts using it in an update. Windows didn't allow the last set-up.",
    st("en", "Set up again"),
  ],
  [
    "set_up_failed",
    PROT.failed,
    "Set up the Safe workspace again, so it's ready when Clauseo starts using it in an update. The last set-up didn't work.",
    st("en", "Set up again"),
  ],
] as const;

test.each(SET_UP_CASES)(
  "set_up_state %s: its line and its button; with retry_asks_admin false the press only checks again, at once",
  async (name, state, line, label) => {
    const t = await mount(setUpHost(state, { retry_asks_admin: false } as Partial<ProtectionView>));
    expectComputerSays();
    expect(block()?.dataset["setUp"]).toBe(name);
    expect(one("protection-set-up")?.textContent).toBe(s(line));
    expect(one("protection-setup")?.textContent).toBe(label);
    await press(one("protection-setup"));
    expect(one("protection-confirm")).toBeNull();
    expect(t.protectionCommands()).toEqual(["retry_protection"]);
    expect(one("protection-wait")?.textContent).toBe(pw("en", "Setting up the Safe workspace."));
  },
);

test.each(SET_UP_CASES)(
  "set_up_state %s with retry_asks_admin true: the set-up's confirm first, then the command",
  async (_, state) => {
    const t = await mount(setUpHost(state, { retry_asks_admin: true } as Partial<ProtectionView>));
    await press(one("protection-setup"));
    expect(one("protection-confirm")?.dataset["act"]).toBe("setup");
    expect(t.protectionCommands()).toEqual([]);
    await press(one("protection-confirm-ok"));
    expect(t.protectionCommands()).toEqual(["retry_protection"]);
  },
);

test.each([
  [
    "blocked (logon_right)",
    PROT.logon,
    "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this.",
    true,
  ],
  [
    "blocked (policy)",
    PROT.policy,
    "A Windows policy on this computer blocks the program that sets up the Safe workspace. Your firm manages this policy on this computer.",
    false,
  ],
  [
    "blocked (a reason from a newer Clauseo)",
    PROT.blockedNewer,
    "Something on this computer stops the Safe workspace. Your firm manages this computer's settings.",
    false,
  ],
  [
    "other_account",
    PROT.otherAccount,
    "The Safe workspace on this computer belongs to another Windows account. Your firm manages this computer's Windows accounts.",
    false,
  ],
] as const)(
  "set_up_state %s: the block's reason line, Set up only where setting up again fixes it",
  async (_, state, reason, setUp) => {
    await mount(setUpHost(state));
    expectComputerSays();
    expect(one("protection-block")?.textContent).toBe(pw("en", reason as never));
    expect(one("protection-set-up")).toBeNull();
    expect(one("protection-setup")?.textContent ?? null).toBe(
      setUp ? pw("en", "Set up the Safe workspace") : null,
    );
    expect(text()).not.toMatch(/\bagent\b|ask (your )?IT|contact IT/i);
  },
);

test("set_up_state blocked (admin_copy_missing): the reason, the repair, and the firm's install line on a firm's install", async () => {
  await mount(setUpHost(PROT.missing, { required: true }));
  expectComputerSays();
  expect(one("protection-block")?.textContent).toBe(
    pw("en", "The Safe workspace wasn't set up when Clauseo was installed."),
  );
  expect(one("protection-repair")?.textContent).toBe(
    pw("en", "Your firm manages how Clauseo is installed on this computer."),
  );
  expect(one("protection-installer")).toBeNull();
});

test("Linux's unsupported { os } says Linux's line; on Windows (before 10 22H2) the same why says the general one", async () => {
  await mount({ view: () => view(np({ unsupported: { why: "os" } })) }, "linux");
  const linux = s(
    "The Safe workspace isn't available on Linux yet, so Clauseo asks before changing anything outside a matter's folder.",
  );
  expect(text()).toContain(linux);
  expect(text()).not.toMatch(/\bagent\b|IT\b/);
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await mount({ view: () => view(np({ unsupported: { why: "os" } })) }, "windows");
  expect(text()).not.toContain(linux);
  expect(text()).toContain(
    s(
      "This computer can't run the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.",
    ),
  );
});

test("only Settings' Safe workspace reads set_up_state", () => {
  const src = join(__dirname, "../..");
  const readers: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name)) {
        if (readFileSync(path, "utf8").includes("set_up_state")) readers.push(relative(src, path));
      }
    }
  };
  walk(src);
  expect(readers.sort()).toEqual([
    "review/registry-uc-set-up.tsx",
    "settings/sections/workspace.tsx",
    "transport/bindings.ts",
  ]);
});
