// @vitest-environment happy-dom
// UC's Safe workspace in the settings window (settings.html#workspace), over FakeTransport with the
// protection wire: one test per row of the brief's Settings table; the quiet unavailable state
// while the query answers command.invalid (before R17 PR-B admits this window), never an error; no
// section without the wire; "Remove set-up" always through its confirm, Cancel sending nothing; the
// set-up's confirm by the same rule as the room's, after a Remove too; design B's repair, only for
// a missing admin copy, with Get the installer on a per-user install and IT's line on a per-machine
// one; a block's reason line; the wait and a refusal; no account name or SID for any fixture; and
// no timer at rest.
import { settled } from "@clauseo/components/lib/late.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { INSTALLER_CLOSE, pw } from "@clauseo/components/lib/words-protection.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import { DEFAULT_UI, type DisplayStore } from "../../room/display-store.ts";
import { NO_PROMPT, np, PROT, view } from "../../room/protection/fixtures.ts";
import { installerPage } from "../../room/protection/installer.ts";
import { APP_VERSION } from "../version.ts";
import { FakeTransport, type FakeProtectionHost } from "../../transport/fake-transport.ts";
import type { Protection, ProtectionView } from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;
/** The updater's view `get_update` answers; undefined: not scripted (refused). */
let update: unknown = undefined;
/** The language the settings window's ui settings name. */
let language: "en" | "hi" = "en";

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  host = null;
  root = null;
  store = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  update = undefined;
  language = "en";
});

async function settle() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
    await act(async () => settled());
  }
}

type Answer = { resolve: () => void; reject: (e: unknown) => void };

/** The settings window at the Safe workspace, on `platform`, over `protection` (none: no wire). */
async function mount(
  protection: FakeProtectionHost | undefined,
  platform: "mac" | "windows" | "linux" = "windows",
  hash = "#workspace",
) {
  const settings = { ui: { ...DEFAULT_UI, language }, locked: [] };
  const transport = new FakeTransport({
    platform,
    query: (query) => {
      if (query === "get_settings") return { settings: settings as never };
      if (query === "get_update" && update !== undefined) return { update: update as never };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    next: { query: () => ({ settings }) as never },
    ...(protection === undefined ? {} : { protection }),
  });
  history.replaceState(null, "", `/settings.html${hash}`);
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

/** A computer at `computer` with `over`; each command held until the test answers it. */
function held(computer: Protection, over: Partial<ProtectionView> = {}) {
  const answers: Answer[] = [];
  const state = { computer };
  const host: FakeProtectionHost = {
    view: () => view(state.computer, over),
    command: () => new Promise<void>((resolve, reject) => answers.push({ resolve, reject })),
  };
  return { host, answers, state };
}

const one = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const block = () => one("settings-workspace");
const text = () => block()?.textContent ?? "";
async function press(el: HTMLElement | null) {
  if (el === null) throw new Error("nothing to press");
  await act(async () => el.click());
  await settle();
}

test("the section is named Safe workspace, and shows only where the transport carries the protection wire", async () => {
  await mount({ view: () => view(PROT.a) }, "mac");
  expect(document.body.textContent).toContain(st("en", "Safe workspace"));
  expect(block()).not.toBeNull();
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await mount(undefined, "mac");
  expect(block()).toBeNull();
  expect(document.body.textContent).not.toContain(st("en", "Safe workspace"));
});

test("while the query answers command.invalid (not admitted yet), the quiet unavailable state: no error, no act", async () => {
  const warn = vi.spyOn(console, "warn");
  await mount({
    view: () => {
      throw { code: "command.invalid", message: "not admitted", retryable: false };
    },
  });
  expect(block()?.dataset["state"]).toBe("unavailable");
  expect(text()).toBe(st("en", "Clauseo can't show this computer's Safe workspace here yet."));
  expect(one("protection-setup")).toBeNull();
  expect(warn).not.toHaveBeenCalledWith(expect.stringContaining("protection"));
});

test.each([
  ["a, Mac", PROT.a, "mac", "a-mac", "macOS keeps Clauseo inside each matter's folder.", null],
  [
    "a, Windows",
    PROT.a,
    "windows",
    "a-win",
    "Windows keeps Clauseo inside each matter's folder.",
    null,
  ],
  ["b", PROT.b, "windows", "b", "Set up with Windows' permission.", "protection-remove"],
] as const)(
  "protected %s: On for every matter, and what keeps Clauseo in",
  async (_c, p, platform, state, note, act) => {
    await mount({ view: () => view(p) }, platform);
    expect(block()?.dataset["state"]).toBe(state);
    expect(text()).toContain(st("en", "On for every matter"));
    expect(text()).toContain(st("en", note));
    expect(one("protection-remove") !== null).toBe(act !== null);
    expect(one("protection-setup")).toBeNull();
  },
);

test("tier b with R17's set_up_at says when, with Windows' permission", async () => {
  const at = Date.UTC(2026, 10, 3, 6, 0);
  await mount({ view: () => view(PROT.b, { set_up_at: at } as Partial<ProtectionView>) });
  expect(text()).toContain("Set up on 3 November 2026, with Windows' permission.");
});

test.each([
  [
    "needs_set_up",
    PROT.needs,
    "The Safe workspace isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.",
    "Set up the Safe workspace",
  ],
  [
    "set_up_refused",
    PROT.refused,
    "Windows didn't allow the set-up, so Clauseo asks before changing anything outside a matter's folder.",
    "Set up again",
  ],
  [
    "set_up_failed",
    PROT.failed,
    "The Safe workspace couldn't be set up on this computer, so Clauseo asks before changing anything outside a matter's folder.",
    "Set up again",
  ],
] as const)(
  "%s, set up with no prompt: Not protected, why, and its set-up",
  async (state, p, note, label) => {
    await mount({ view: () => view(p, NO_PROMPT) });
    expect(block()?.dataset["state"]).toBe(state);
    expect(text()).toContain(st("en", "Not protected"));
    expect(text()).toContain(st("en", note));
    expect(one("protection-setup")?.textContent).toBe(label);
  },
);

test("set_up_failed with R17 PR-E's 1385 cause adds the line for IT", async () => {
  await mount({ view: () => view(np({ set_up_failed: { why: "logon_right_denied_1385" } })) });
  expect(block()?.dataset["state"]).toBe("set_up_failed");
  expect(text()).toContain("Allow log on locally");
});

test.each([
  [
    "plain unsupported on Linux",
    "unsupported",
    "linux",
    "This computer can't run the Safe workspace",
  ],
  ["why os", { unsupported: { why: "os" } }, "linux", "isn't available on Linux yet"],
  [
    "why windows_arm",
    { unsupported: { why: "windows_arm" } },
    "windows",
    "isn't available on Windows on ARM yet",
  ],
  [
    "why not_built",
    { unsupported: { why: "not_built" } },
    "windows",
    "This version of Clauseo doesn't have the Safe workspace yet",
  ],
  ["a newer cause", "some_newer_cause", "windows", "This computer can't run the Safe workspace"],
] as const)("can't, %s: its line, and no act", async (_c, cause, platform, words) => {
  await mount({ view: () => view(np(cause)) }, platform);
  expect(block()?.dataset["state"]).toBe("cant");
  expect(text()).toContain(words);
  expect(one("protection-setup")).toBeNull();
  expect(one("protection-remove")).toBeNull();
});

test("required: Managed by firm beside the status; Not protected adds the firm's line", async () => {
  await mount({ view: () => view(PROT.needs, { required: true }) });
  expect(one("managed")?.textContent).toBe(st("en", "Managed by firm"));
  expect(one("protection-required")?.textContent).toBe(
    st(
      "en",
      "Your firm requires the Safe workspace, so Clauseo doesn't answer on this computer until it's set up.",
    ),
  );
});

test("required while protected: Managed by firm, no firm line", async () => {
  await mount({ view: () => view(PROT.b, { required: true }) });
  expect(one("managed")).not.toBeNull();
  expect(one("protection-required")).toBeNull();
});

test("Not protected where the firm allows no unprotected auto mode says so; allowed says nothing", async () => {
  await mount({ view: () => view(PROT.refused, { unprotected_auto_allowed: false }) });
  expect(one("protection-no-auto")?.textContent).toBe(
    st("en", "Your firm doesn't allow Clauseo to work without asking on this computer."),
  );
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await mount({ view: () => view(PROT.refused) });
  expect(one("protection-no-auto")).toBeNull();
});

test("Remove set-up always confirms first: Cancel sends nothing, confirming sends one remove_sandbox_setup and waits", async () => {
  const s = held(PROT.b, { retry_asks_admin: false } as Partial<ProtectionView>);
  const t = await mount(s.host);
  await press(one("protection-remove"));
  const confirm = one("protection-confirm");
  expect(confirm?.dataset["act"]).toBe("remove");
  expect(confirm?.textContent).toContain(pw("en", "Remove the Safe workspace from this computer?"));
  expect(confirm?.textContent).toContain("It will say the publisher is unknown");
  expect(confirm?.textContent).toContain(
    pw(
      "en",
      "Afterwards, Clauseo works on this computer without the Safe workspace until it's set up again.",
    ),
  );
  await press(one("protection-confirm-cancel"));
  expect(t.protectionCommands()).toEqual([]);
  await press(one("protection-remove"));
  await press(one("protection-confirm-ok"));
  expect(t.protectionCommands()).toEqual(["remove_sandbox_setup"]);
  expect(one("protection-wait")?.textContent).toBe(
    pw("en", "Choose Yes in the window Windows shows."),
  );
  const reads = t.only("query").filter((c) => c.query === "protection").length;
  s.state.computer = PROT.needs;
  await act(async () => s.answers[0]!.resolve());
  await settle();
  expect(t.only("query").filter((c) => c.query === "protection")).toHaveLength(reads + 1);
  expect(block()?.dataset["state"]).toBe("needs_set_up");
});

test("Set up again with no Windows prompt sends at once, with no confirm, and a refusal says its words", async () => {
  const s = held(PROT.refused, NO_PROMPT);
  const t = await mount(s.host);
  await press(one("protection-setup"));
  expect(one("protection-confirm")).toBeNull();
  expect(t.protectionCommands()).toEqual(["retry_protection"]);
  await act(async () =>
    s.answers[0]!.reject({ code: "sandbox.setup_refused", message: "no", retryable: true }),
  );
  await settle();
  expect(one("protection-failed")?.textContent).toBe(errorWords("en", "sandbox.setup_refused"));
});

test.each([
  ["needs_set_up", PROT.needs, "Set up the Safe workspace"],
  ["set_up_refused", PROT.refused, "Set up again"],
  ["set_up_failed (a re-run)", PROT.failed, "Set up again"],
] as const)(
  "%s with retry_asks_admin true keeps the app's button, through the set-up's confirm; no repair",
  async (_c, p, label) => {
    const s = held(p, { retry_asks_admin: true } as Partial<ProtectionView>);
    const t = await mount(s.host);
    expect(one("protection-repair")).toBeNull();
    expect(one("protection-installer")).toBeNull();
    expect(one("protection-setup")?.textContent).toBe(label);
    await press(one("protection-setup"));
    expect(one("protection-confirm")?.dataset["act"]).toBe("setup");
    expect(one("protection-confirm")?.textContent).toContain(
      pw("en", "Set up the Safe workspace?"),
    );
    expect(t.protectionCommands()).toEqual([]);
    await press(one("protection-confirm-ok"));
    expect(t.protectionCommands()).toEqual(["retry_protection"]);
  },
);

test("after a finished in-app Remove, the app offers Set up, with the announce line before Windows' prompt", async () => {
  const s = held(PROT.b, { retry_asks_admin: true } as Partial<ProtectionView>);
  const t = await mount(s.host);
  await press(one("protection-remove"));
  await press(one("protection-confirm-ok"));
  expect(t.protectionCommands()).toEqual(["remove_sandbox_setup"]);
  // the admin copy stays after an in-app Remove, so the host says needs_set_up, not
  // admin_copy_missing
  s.state.computer = PROT.needs;
  await act(async () => s.answers[0]!.resolve());
  await settle();
  expect(block()?.dataset["state"]).toBe("needs_set_up");
  expect(one("protection-repair")).toBeNull();
  expect(one("protection-installer")).toBeNull();
  await press(one("protection-setup"));
  const confirm = one("protection-confirm");
  expect(confirm?.dataset["act"]).toBe("setup");
  expect(confirm?.textContent).toContain(
    pw(
      "en",
      "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
    ),
  );
  await press(one("protection-confirm-ok"));
  expect(t.protectionCommands()).toEqual(["remove_sandbox_setup", "retry_protection"]);
});

test("admin_copy_missing on a per-user install: not set up yet, the repair, and Get the installer opens the installed version's page", async () => {
  update = { current: "0.1.0", mode: "auto", state: "idle" };
  const t = await mount({ view: () => view(PROT.missing) });
  expect(block()?.dataset["state"]).toBe("blocked");
  expect(text()).toContain(
    st(
      "en",
      "The Safe workspace isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.",
    ),
  );
  expect(one("protection-block")?.textContent).toBe(
    pw("en", "The Safe workspace wasn't set up when Clauseo was installed."),
  );
  expect(one("protection-repair")?.textContent).toBe(
    pw("en", "Run the Clauseo installer again to set up the Safe workspace."),
  );
  await press(one("protection-installer"));
  expect(t.only("open_external")).toEqual([
    { call: "open_external", url: installerPage(APP_VERSION) },
  ]);
  expect(t.only("quit_app")).toEqual([{ call: "quit_app" }]);
  expect(t.protectionCommands()).toEqual([]);
});

test("the per-user repair says Clauseo will close, beside Get the installer, in English", async () => {
  update = { current: "0.1.0", mode: "auto", state: "idle" };
  await mount({ view: () => view(PROT.missing) });
  expect(one("protection-close-line")?.textContent).toBe(pw("en", INSTALLER_CLOSE));
});

test("the same line in Hindi when the window is in Hindi", async () => {
  update = { current: "0.1.0", mode: "auto", state: "idle" };
  language = "hi";
  await mount({ view: () => view(PROT.missing) });
  expect(one("protection-close-line")?.textContent).toBe(pw("hi", INSTALLER_CLOSE));
});

test.each([
  ["per machine, by the updater", {}, "per_machine"],
  ["update.mode none by policy", {}, "policy"],
  ["required", { required: true }, null],
  ["no unprotected auto mode", { unprotected_auto_allowed: false }, null],
] as const)(
  "admin_copy_missing on a firm install (%s): ask IT, with no button",
  async (_c, over, by) => {
    if (by !== null) update = { current: "0.1.0", mode: "auto", state: { managed: { by } } };
    const t = await mount({ view: () => view(PROT.missing, over) });
    expect(one("protection-repair")?.textContent).toBe(
      pw("en", "Ask your IT team to install Clauseo again."),
    );
    expect(one("protection-installer")).toBeNull();
    expect(one("protection-close-line")).toBeNull();
    expect(one("protection-setup")).toBeNull();
    expect(t.only("quit_app")).toEqual([]);
  },
);

test.each([
  [
    "policy",
    PROT.policy,
    "A Windows policy on this computer blocks the program that sets up the Safe workspace. Ask your IT team to allow it.",
  ],
  [
    "other_account",
    PROT.otherAccount,
    "The Safe workspace on this computer belongs to another Windows account. Ask your IT team.",
  ],
  ["a reason from a newer Clauseo", PROT.blockedNewer, "Ask your IT team to look into it."],
] as const)("a block, %s: what stops it, its line, and no act", async (_c, p, line) => {
  await mount({ view: () => view(p) });
  expect(block()?.dataset["state"]).toBe("blocked");
  expect(text()).toContain(
    pw(
      "en",
      "Something on this computer stops the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.",
    ),
  );
  expect(one("protection-block")?.textContent).toBe(line);
  expect(one("protection-repair")).toBeNull();
  expect(one("protection-installer")).toBeNull();
  expect(one("protection-setup")).toBeNull();
});

test.each([
  ["Windows' prompt follows: the set-up's confirm first, then", true],
  ["no prompt: at once", false],
] as const)(
  "a block, logon_right: setting up again usually fixes it, and the set-up is offered; %s retry_protection",
  async (_c, asks) => {
    const t = await mount(
      held(PROT.logon, { retry_asks_admin: asks } as Partial<ProtectionView>).host,
    );
    expect(block()?.dataset["state"]).toBe("blocked");
    expect(one("protection-block")?.textContent).toBe(
      pw(
        "en",
        "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this. If it doesn't, ask whoever looks after this computer to add ClauseoSandboxUsers to the Allow log on locally policy.",
      ),
    );
    expect(one("protection-installer")).toBeNull();
    expect(one("protection-setup")?.textContent).toBe(pw("en", "Set up the Safe workspace"));
    await press(one("protection-setup"));
    if (asks) {
      expect(one("protection-confirm")?.dataset["act"]).toBe("setup");
      expect(t.protectionCommands()).toEqual([]);
      await press(one("protection-confirm-ok"));
    } else expect(one("protection-confirm")).toBeNull();
    expect(t.protectionCommands()).toEqual(["retry_protection"]);
  },
);

test("#protection opens the Safe workspace, the name the wire gives it", async () => {
  await mount({ view: () => view(PROT.a) }, "mac", "#protection");
  expect(document.querySelector("main h2")?.textContent).toBe("Safe workspace");
  expect(block()).not.toBeNull();
});

test("a check the view says is in progress shows its own line, not the set-up's", async () => {
  await mount({
    view: () => view(PROT.b, { in_progress: "check" } as Partial<ProtectionView>),
  });
  expect(one("protection-wait")?.textContent).toBe(pw("en", "Checking the Safe workspace."));
  expect(one("protection-wait")?.textContent).not.toBe(pw("en", "Setting up the Safe workspace."));
});

test("a set-up the view says is in progress (after a reload) shows the wait, and protection_view_changed replaces the view", async () => {
  const t = await mount({
    view: () => view(PROT.needs, { in_progress: "set_up" } as Partial<ProtectionView>),
  });
  expect(one("protection-wait")?.textContent).toBe(
    pw("en", "Choose Yes in the window Windows shows."),
  );
  expect(one("protection-setup")).toBeNull();
  await act(async () => t.protectionViewChanged(view(PROT.b)));
  await settle();
  expect(block()?.dataset["state"]).toBe("b");
});

test("no account name or SID for any fixture", async () => {
  for (const p of [
    PROT.a,
    PROT.b,
    PROT.needs,
    PROT.refused,
    PROT.failed,
    PROT.unsupported,
    PROT.pool,
    PROT.off,
  ]) {
    await mount({ view: () => view(p, { required: true, unprotected_auto_allowed: false }) });
    expect(document.body.textContent).not.toMatch(/S-1-5-|clauseo-agent|sandbox|session/i);
    await act(async () => root?.unmount());
    document.body.replaceChildren();
  }
});

test("zero timers at rest in the section", async () => {
  await mount({ view: () => view(PROT.refused) });
  const timeout = vi.spyOn(globalThis, "setTimeout");
  const interval = vi.spyOn(globalThis, "setInterval");
  const frame = vi.spyOn(globalThis, "requestAnimationFrame");
  await act(async () => new Promise((resolve) => queueMicrotask(() => resolve(undefined))));
  expect(timeout).not.toHaveBeenCalled();
  expect(interval).not.toHaveBeenCalled();
  expect(frame).not.toHaveBeenCalled();
});

test("keys: Remove set-up's confirm starts on Cancel; Escape sends nothing and gives focus back to Remove set-up", async () => {
  const s = held(PROT.b, { retry_asks_admin: false } as Partial<ProtectionView>);
  const t = await mount(s.host);
  one("protection-remove")?.focus();
  await press(one("protection-remove"));
  expect(document.activeElement).toBe(one("protection-confirm-cancel"));
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
  });
  await settle();
  await act(async () => new Promise((resolve) => setTimeout(resolve, 80)));
  expect(one("protection-confirm")).toBeNull();
  expect(t.protectionCommands()).toEqual([]);
  expect(document.activeElement).toBe(one("protection-remove"));
});

test("keys: confirming Remove set-up puts focus on the waiting line in its place, then on the section's head once it ends", async () => {
  const s = held(PROT.b, { retry_asks_admin: false } as Partial<ProtectionView>);
  await mount(s.host);
  one("protection-remove")?.focus();
  await press(one("protection-remove"));
  await press(one("protection-confirm-ok"));
  await act(async () => new Promise((resolve) => setTimeout(resolve, 80)));
  expect(document.activeElement).toBe(one("protection-wait"));
  s.state.computer = PROT.needs;
  await act(async () => s.answers[0]!.resolve());
  await settle();
  expect(one("protection-wait")).toBeNull();
  expect(document.activeElement).toBe(document.querySelector(".set-pane h2"));
});

test("keys: Set up the Safe workspace without a confirm puts focus on the waiting line", async () => {
  const s = held(PROT.needs, { retry_asks_admin: false } as Partial<ProtectionView>);
  await mount(s.host);
  one("protection-setup")?.focus();
  await press(one("protection-setup"));
  expect(one("protection-confirm")).toBeNull();
  expect(document.activeElement).toBe(one("protection-wait"));
});

test("the settings window never offers the in-app repair, even where display_info says it runs", async () => {
  update = { current: "0.1.0", mode: "auto", state: "idle" };
  const t = await mount({
    view: () => view(PROT.missing),
    repair: { available: true, note: "launch_failed" },
  });
  expect(one("protection-installer")).not.toBeNull();
  expect(one("protection-repair-again")).toBeNull();
  expect(text()).not.toContain(pw("en", "Download and run the installer again"));
  expect(text()).not.toContain(errorWords("en", "repair.launch_failed"));
  expect(t.only("repair_install")).toEqual([]);
});
