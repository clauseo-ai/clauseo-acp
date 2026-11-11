// @vitest-environment happy-dom
// UC's Safe workspace in the settings window (settings.html#workspace), over FakeTransport with the
// protection wire: one test per row of the brief's Settings table; the quiet unavailable state
// while the query answers command.invalid (before R17 PR-B admits this window), never an error; no
// section without the wire; "Remove set-up" always through its confirm, Cancel sending nothing; the
// set-up's confirm by the same rule as the room's, after a Remove too; design B's repair, only for
// a missing admin copy, with Get the installer without a firm's policy and the firm's line under
// one; a block's reason line; the wait and a refusal; no account name or SID for any fixture; and
// no timer at rest.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { INSTALLER_CLOSE, pw } from "@clauseo/components/lib/words-protection.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { NO_PROMPT, np, PROT, view } from "../../room/protection/fixtures.ts";
import { installerPage } from "../../room/protection/installer.ts";
import { APP_VERSION } from "../version.ts";
import type { ProtectionView } from "../../transport/protocol.ts";
import {
  block,
  held,
  mount,
  one,
  press,
  reset,
  scripted,
  settle,
  text,
  unmount,
} from "./workspace-rig.tsx";

afterEach(reset);

test("the section is named Safe workspace, and shows only where the transport carries the protection wire", async () => {
  await mount({ view: () => view(PROT.a) }, "mac");
  expect(document.body.textContent).toContain(st("en", "Safe workspace"));
  expect(block()).not.toBeNull();
  await unmount();
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
    "The Safe workspace isn't set up on this computer yet.",
    "Set up the Safe workspace",
  ],
  ["set_up_refused", PROT.refused, "Windows didn't allow the set-up.", "Set up again"],
  [
    "set_up_failed",
    PROT.failed,
    "The Safe workspace couldn't be set up on this computer.",
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

test("set_up_failed with R17 PR-E's 1385 cause adds the set-up-again line", async () => {
  await mount({ view: () => view(np({ set_up_failed: { why: "logon_right_denied_1385" } })) });
  expect(block()?.dataset["state"]).toBe("set_up_failed");
  expect(text()).toContain("Setting up the Safe workspace again usually fixes this.");
  expect(text()).not.toContain("looks after this computer");
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

test("the firm's no-auto line stays hidden until the host enforces it (UC-76), whatever the policy says", async () => {
  await mount({ view: () => view(PROT.refused, { unprotected_auto_allowed: false }) });
  expect(one("protection-no-auto")).toBeNull();
  expect(document.body.textContent).not.toContain(
    st("en", "Your firm doesn't allow Clauseo to work without asking on this computer."),
  );
  await unmount();
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

test("required: Remove set-up still shows, and its confirm says Clauseo won't answer afterwards", async () => {
  await mount(held(PROT.b, { required: true }).host);
  await press(one("protection-remove"));
  const said = one("protection-confirm")?.textContent ?? "";
  expect(said).toContain(
    pw(
      "en",
      "Your firm requires the Safe workspace, so afterwards Clauseo won't answer on this computer until it's set up again.",
    ),
  );
  expect(said).not.toContain("works on this computer without");
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
      "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, enter the password of an administrator account on this computer.",
    ),
  );
  await press(one("protection-confirm-ok"));
  expect(t.protectionCommands()).toEqual(["remove_sandbox_setup", "retry_protection"]);
});

test("admin_copy_missing on a per-user install: not set up yet, the repair, and Get the installer opens the installed version's page", async () => {
  scripted.update = { current: "0.1.0", mode: "auto", state: "idle" };
  const t = await mount({ view: () => view(PROT.missing) });
  expect(block()?.dataset["state"]).toBe("blocked");
  expect(text()).toContain(st("en", "The Safe workspace isn't set up on this computer yet."));
  expect(one("protection-block")?.textContent).toBe(
    pw("en", "The Safe workspace wasn't set up when Clauseo was installed."),
  );
  expect(one("protection-repair")?.textContent).toBe(
    pw("en", "Run the Clauseo installer again to set up the Safe workspace."),
  );
  await press(one("protection-installer"));
  expect(t.only("open_external")).toEqual([
    { call: "open_external", url: installerPage(APP_VERSION), alwaysAsk: true },
  ]);
  expect(t.only("quit_app")).toEqual([{ call: "quit_app" }]);
  expect(t.protectionCommands()).toEqual([]);
});

test("the per-user repair says Clauseo will close, beside Get the installer, in English", async () => {
  scripted.update = { current: "0.1.0", mode: "auto", state: "idle" };
  await mount({ view: () => view(PROT.missing) });
  expect(one("protection-close-line")?.textContent).toBe(pw("en", INSTALLER_CLOSE));
});

test("the same line in Hindi when the window is in Hindi", async () => {
  scripted.update = { current: "0.1.0", mode: "auto", state: "idle" };
  scripted.language = "hi";
  await mount({ view: () => view(PROT.missing) });
  expect(one("protection-close-line")?.textContent).toBe(pw("hi", INSTALLER_CLOSE));
});

test("admin_copy_missing on an install for everyone with no firm's policy: the per-user repair, Get the installer", async () => {
  scripted.update = { current: "0.1.0", mode: "auto", state: { managed: { by: "per_machine" } } };
  await mount({ view: () => view(PROT.missing) });
  expect(one("protection-repair")?.textContent).toBe(
    pw("en", "Run the Clauseo installer again to set up the Safe workspace."),
  );
  expect(one("protection-installer")).not.toBeNull();
  expect(text()).not.toMatch(/your firm/i);
});

test.each([
  ["update.mode none by policy", {}, "policy"],
  ["required", { required: true }, null],
  ["no unprotected auto mode", { unprotected_auto_allowed: false }, null],
] as const)(
  "admin_copy_missing under a firm's policy (%s): the firm manages the install, with no button",
  async (_c, over, by) => {
    if (by !== null)
      scripted.update = { current: "0.1.0", mode: "auto", state: { managed: { by } } };
    const t = await mount({ view: () => view(PROT.missing, over) });
    expect(one("protection-repair")?.textContent).toBe(
      pw("en", "Your firm manages how Clauseo is installed on this computer."),
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
    "A Windows policy on this computer blocks the program that sets up the Safe workspace.",
  ],
  [
    "other_account",
    PROT.otherAccount,
    "The Safe workspace on this computer belongs to another Windows account, so Clauseo can't use it from this one.",
  ],
  [
    "a reason from a newer Clauseo",
    PROT.blockedNewer,
    "Something on this computer stops the Safe workspace.",
  ],
] as const)(
  "a block, %s: what stops it, its line, and no act; Copy details for IT only under a firm's policy",
  async (_c, p, line) => {
    await mount({ view: () => view(p, { unprotected_auto_allowed: false }) });
    expect(block()?.dataset["state"]).toBe("blocked");
    expect(text()).toContain(pw("en", "Something on this computer stops the Safe workspace."));
    expect(one("protection-block")?.textContent).toBe(line);
    expect(one("protection-it")?.textContent).toBe("Copy details for IT");
    expect(one("protection-repair")).toBeNull();
    expect(one("protection-installer")).toBeNull();
    expect(one("protection-setup")).toBeNull();
    await unmount();
    await mount({ view: () => view(p) });
    expect(one("protection-block")?.textContent).toBe(line);
    expect(one("protection-it")).toBeNull();
    expect(text()).not.toMatch(/\bIT\b|[Yy]our firm/);
  },
);

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
        "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this.",
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

test("under a firm's policy, a firm-only block offers Copy details for IT: its reason and version, never a matter's path; stage C's don't", async () => {
  const path = "C:\\Users\\asha\\Matters\\Sharma v. State of Delhi";
  const writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  await mount({
    view: () =>
      view(np({ blocked: { reason: "policy", path } }), { unprotected_auto_allowed: false }),
  });
  await press(one("protection-it"));
  expect(writeText).toHaveBeenCalledWith(
    `Clauseo's Safe workspace is blocked on this computer.\nReason: policy\nClauseo version: ${APP_VERSION}\nClauseo for firm IT: https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md#sandbox-policy`,
  );
  expect(writeText.mock.calls[0]![0]).not.toContain(path);
  expect(one("protection-it")?.textContent).toBe("Message copied");
  Reflect.deleteProperty(navigator, "clipboard");
  for (const reason of ["tool_missing", "admin_copy_changed", "admin_copy_unsafe"]) {
    await unmount();
    await mount({
      view: () => view(np({ blocked: { reason } }), { unprotected_auto_allowed: false }),
    });
    expect(one("protection-it")).toBeNull();
  }
});

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
    await unmount();
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
