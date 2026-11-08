// @vitest-environment happy-dom
// UC's commands in the room, over FakeTransport with the protection wire. Design B: only a missing
// admin copy (blocked { admin_copy_missing }) and R17 PR-E M4's broken set-up (set_up_failed) give
// the installer repair. needs_set_up and set_up_refused (a set-up after an in-app Remove, or a
// re-run) keep the app's button, and the "Unknown publisher" confirm comes before a retry_protection that asks an
// administrator, as does Add room's; only confirming sends it (Cancel sends nothing). Without R17's
// retry_asks_admin the fallback asks for needs_set_up, set_up_refused and a full pool; the field
// alone decides once it is there. The waiting line ends on the answer or the first
// protection_changed, then the view is read once; a refusal says its code's words. A block says its
// reason. The firm's requirement holds send with its notice. The window split: the room sends only
// retry_protection, and nothing under room/ imports Settings' Safe workspace.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import {
  INSTALLER_CLOSE,
  protectionErrorWords,
  pw,
} from "@clauseo/components/lib/words-protection.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { MATTER } from "../../testing/stream-script.ts";
import type { Protection, ProtectionView } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
import type { RoomProtectionCommand } from "./commands.ts";
import { APP_VERSION } from "../../settings/version.ts";
import { NO_PROMPT, np, PROT, view } from "./fixtures.ts";
import { vt } from "@clauseo/components/lib/words-versions.ts";
import { installerPage } from "./installer.ts";

let h: Harness | null = null;
beforeEach(() =>
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER] }),
  ),
);
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

type Answer = { resolve: () => void; reject: (e: unknown) => void };

/** The room over a matter at `protection`, the computer at `computer` (with `over`), and each command held until the
 * test answers it. */
async function mount(
  protection: Protection,
  computer: Protection = protection,
  over: Partial<ProtectionView> = {},
) {
  const answers: Answer[] = [];
  h = await mountRoom({
    matters: [{ ...THE_MATTER, protection }],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: {
      view: () => view(computer, over),
      command: () => new Promise<void>((resolve, reject) => answers.push({ resolve, reject })),
    },
  });
  await settle();
  return answers;
}

const settle = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};
const inBody = (id: string) => document.body.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const sentRetries = () => h!.transport.protectionCommands();
const viewReads = () => h!.transport.only("query").filter((c) => c.query === "protection").length;

/** Opens the note from the line, then presses its retry. */
async function pressRetry() {
  await h!.click(h!.one("workspace-line"));
  await settle();
  await h!.click(inBody("protection-setup"));
  await settle();
}

test.each([
  ["needs_set_up", PROT.needs, "Set up the Safe workspace"],
  ["set_up_refused", PROT.refused, "Try again"],
] as const)(
  "%s with retry_asks_admin true: the app's retry opens the set-up's confirm first; Cancel sends nothing",
  async (_c, p, label) => {
    await mount(p, p, { retry_asks_admin: true } as Partial<ProtectionView>);
    await h!.click(h!.one("workspace-line"));
    await settle();
    expect(inBody("protection-installer")).toBeNull();
    expect(inBody("protection-setup")?.textContent).toBe(label);
    await h!.click(inBody("protection-setup"));
    await settle();
    const confirm = inBody("protection-confirm");
    expect(confirm?.dataset["act"]).toBe("setup");
    expect(confirm?.textContent).toContain(
      pw(
        "en",
        "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
      ),
    );
    expect(sentRetries()).toEqual([]);
    await h!.click(inBody("protection-confirm-cancel"));
    await settle();
    expect(inBody("protection-confirm")).toBeNull();
    expect(sentRetries()).toEqual([]);
  },
);

test("admin_copy_missing: the note says not set up yet, why, the repair, and Get the installer opens the page", async () => {
  await mount(PROT.missing);
  await h!.click(h!.one("workspace-line"));
  await settle();
  const note = inBody("np-note");
  expect(note?.dataset["cause"]).toBe("needs_set_up");
  for (const line of [
    "The Safe workspace isn't set up on this computer yet. Until it is, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.",
    "The Safe workspace wasn't set up when Clauseo was installed.",
    "Run the Clauseo installer again to set up the Safe workspace.",
  ] as const)
    expect(note?.textContent).toContain(pw("en", line));
  expect(note?.textContent).toContain(pw("en", INSTALLER_CLOSE));
  await h!.click(inBody("protection-installer"));
  await settle();
  expect(h!.transport.only("open_external")).toEqual([
    { call: "open_external", url: installerPage(APP_VERSION) },
  ]);
  expect(h!.transport.only("quit_app")).toEqual([{ call: "quit_app" }]);
  expect(sentRetries()).toEqual([]);
});

test.each([
  [
    "tool_missing",
    { not_protected: { cause: { blocked: { reason: "tool_missing" } } } } as Protection,
    "The program that sets up the Safe workspace is missing, or security software blocked it. Ask your IT team to look into it.",
  ],
  [
    "other_account",
    PROT.otherAccount,
    "The Safe workspace on this computer belongs to another Windows account. Ask your IT team.",
  ],
  ["a reason from a newer Clauseo", PROT.blockedNewer, "Ask your IT team to look into it."],
] as const)(
  "a block, %s: the note says what stops it and its line, with no retry",
  async (_c, p, line) => {
    await mount(p);
    await h!.click(h!.one("workspace-line"));
    await settle();
    const note = inBody("np-note");
    expect(note?.dataset["cause"]).toBe("blocked");
    expect(note?.textContent).toContain(
      pw(
        "en",
        "Something on this computer stops the Safe workspace, so Clauseo can't answer here.",
      ),
    );
    expect(note?.textContent).toContain(pw("en", line));
    expect(inBody("protection-setup")).toBeNull();
    expect(inBody("protection-installer")).toBeNull();
  },
);

test("Add room: its confirm first; confirming sends one retry_protection, and a second press while it runs sends nothing", async () => {
  const answers = await mount(PROT.pool, PROT.b);
  await pressRetry();
  const confirm = inBody("protection-confirm");
  expect(confirm?.dataset["act"]).toBe("add");
  expect(confirm?.textContent).toContain(pw("en", "Add room for 4 more?"));
  expect(confirm?.textContent).toContain(
    pw(
      "en",
      "Windows will ask to let Clauseo add room to the Safe workspace. It will say the publisher is unknown, because this early version of Clauseo isn't signed yet. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
    ),
  );
  expect(sentRetries()).toEqual([]);
  await h!.click(inBody("protection-confirm-ok"));
  await settle();
  expect(sentRetries()).toEqual(["retry_protection"]);
  // the waiting line stands in the button's place: nothing to press again
  expect(inBody("protection-setup")).toBeNull();
  expect(inBody("protection-wait")?.textContent).toBe(
    pw("en", "Choose Yes in the window Windows shows."),
  );
  await act(async () => answers[0]!.resolve());
  await settle();
  expect(sentRetries()).toEqual(["retry_protection"]);
});

test("set_up_failed (M4): a refusal with the installer's repair and its last resort, no Try again, nothing sent", async () => {
  await mount(PROT.failed);
  await h!.click(h!.one("workspace-line"));
  await settle();
  const note = inBody("np-note");
  expect(note?.dataset["cause"]).toBe("set_up_failed");
  const text = note?.textContent ?? "";
  const body = pw(
    "en",
    "The Safe workspace on this computer isn't working, so Clauseo can't answer here until it's set up again.",
  );
  const repair = pw("en", "Run the Clauseo installer again to set up the Safe workspace.");
  const last = pw("en", "If that doesn't help, ask your IT team.");
  expect(text).toContain(body);
  expect(text.indexOf(body)).toBeLessThan(text.indexOf(repair));
  expect(text.indexOf(repair)).toBeLessThan(text.indexOf(last));
  expect(text).not.toContain("can read any file");
  expect(inBody("protection-setup")).toBeNull();
  expect(inBody("protection-installer")?.textContent).toBe(pw("en", "Get the installer"));
  expect(sentRetries()).toEqual([]);
});

test("set_up_failed (M4) on a firm's install: the IT line, no Get the installer", async () => {
  await mount(PROT.failed, PROT.failed, { unprotected_auto_allowed: false });
  await h!.click(h!.one("workspace-line"));
  await settle();
  const text = inBody("np-note")?.textContent ?? "";
  expect(text).toContain(pw("en", "Ask your IT team to install Clauseo again."));
  expect(text).toContain(pw("en", "If that doesn't help, ask your IT team."));
  expect(inBody("protection-installer")).toBeNull();
  expect(inBody("protection-setup")).toBeNull();
});

test("probe_failed (#722): its line, Try again, and the press sends retry_protection at once with no confirm", async () => {
  const probe = np("probe_failed");
  await mount(probe, probe, NO_PROMPT);
  await h!.click(h!.one("workspace-line"));
  await settle();
  const note = inBody("np-note");
  expect(note?.dataset["cause"]).toBe("probe_failed");
  expect(note?.textContent).toContain(
    pw(
      "en",
      "Clauseo couldn't check the Safe workspace on this computer, so it can't answer here yet. Try again. If it keeps happening, restart Clauseo.",
    ),
  );
  expect(note?.textContent).not.toContain("can read any file");
  expect(inBody("protection-installer")).toBeNull();
  expect(inBody("protection-setup")?.textContent).toBe(vt("en", "Try again"));
  await h!.click(inBody("protection-setup"));
  await settle();
  expect(inBody("protection-confirm")).toBeNull();
  expect(sentRetries()).toEqual(["retry_protection"]);
});

test("probe_failed (#722) when retry_asks_admin says Windows prompts: the confirm comes first", async () => {
  const probe = np("probe_failed");
  await mount(probe, probe, { retry_asks_admin: true } as Partial<ProtectionView>);
  await pressRetry();
  expect(inBody("protection-confirm")).not.toBeNull();
  expect(sentRetries()).toEqual([]);
});

test("a cause from a newer Clauseo (M4): its own refusal, no Try again", async () => {
  await mount(PROT.newer);
  await h!.click(h!.one("workspace-line"));
  await settle();
  const note = inBody("np-note");
  expect(note?.dataset["cause"]).toBe("unknown");
  expect(note?.textContent).toContain(
    pw(
      "en",
      "This version of Clauseo doesn't recognise what's stopping the Safe workspace, so it can't answer here. Update Clauseo, or ask your IT team.",
    ),
  );
  expect(note?.textContent).not.toContain("can read any file");
  expect(inBody("protection-setup")).toBeNull();
});

test("needs_set_up with retry_asks_admin false: the field wins over the fallback, and the set-up goes at once", async () => {
  await mount(PROT.needs, PROT.needs, NO_PROMPT);
  await pressRetry();
  expect(inBody("protection-confirm")).toBeNull();
  expect(sentRetries()).toEqual(["retry_protection"]);
});

test.each([
  ["retry_asks_admin true", { retry_asks_admin: true }, true],
  ["retry_asks_admin false", { retry_asks_admin: false }, false],
  // an older host's view has no field: the fallback, which a full pool's Add room answers with the confirm
  ["no retry_asks_admin", {}, true],
] as const)(
  "a full pool's Add room with %s: the confirm comes first only when it asks",
  async (_c, over, confirms) => {
    const pool = {
      not_protected: { cause: { pool_full: { size: 8, open: 8, held: 0, owed: 0, set_aside: 0 } } },
    } as Protection;
    await mount(pool, PROT.b, over as Partial<ProtectionView>);
    await pressRetry();
    expect(inBody("protection-confirm") !== null).toBe(confirms);
    expect(sentRetries()).toEqual(confirms ? [] : ["retry_protection"]);
  },
);

test("the wait ends on the answer, then the view is read once; the protected matter's note closes", async () => {
  const answers = await mount(PROT.refused, PROT.refused, NO_PROMPT);
  await pressRetry();
  const before = viewReads();
  expect(inBody("protection-wait")).not.toBeNull();
  await act(async () => answers[0]!.resolve());
  await settle();
  expect(inBody("protection-wait")).toBeNull();
  expect(viewReads()).toBe(before + 1);
  await act(async () => h!.transport.protectionChanged(MATTER, PROT.b));
  await settle();
  expect(h!.one("workspace-line")).toBeNull();
  expect(h!.host.querySelector(".room-line")?.textContent).toContain("Safe workspace.");
  expect(inBody("np-note")).toBeNull();
});

test("the wait ends on the first protection_changed when it comes before the answer", async () => {
  await mount(PROT.refused, PROT.refused, NO_PROMPT);
  await pressRetry();
  expect(inBody("protection-wait")).not.toBeNull();
  const before = viewReads();
  await act(async () => h!.transport.protectionChanged(MATTER, PROT.refused));
  await settle();
  expect(inBody("protection-wait")).toBeNull();
  expect(viewReads()).toBe(before + 1);
});

test.each([["sandbox.setup_refused"], ["sandbox.setup_failed"], ["command.invalid"]])(
  "a refused retry (%s) says its code's words under the note",
  async (code) => {
    const answers = await mount(PROT.refused, PROT.refused, NO_PROMPT);
    await pressRetry();
    await act(async () => answers[0]!.reject({ code, message: "no", retryable: true }));
    await settle();
    expect(inBody("protection-failed")?.textContent).toBe(errorWords("en", code));
    expect(inBody("protection-setup")).not.toBeNull();
  },
);

test("a refused retry with sandbox.blocked says the generic words; the reason comes from the cause", async () => {
  const answers = await mount(PROT.refused, PROT.refused, NO_PROMPT);
  await pressRetry();
  await act(async () =>
    answers[0]!.reject({ code: "sandbox.blocked", message: "no", retryable: false }),
  );
  await settle();
  expect(inBody("protection-failed")?.textContent).toBe(
    protectionErrorWords("en", "sandbox.blocked"),
  );
});

test("a declined native confirm (transport.unauthorized) says nothing new", async () => {
  const answers = await mount(PROT.refused, PROT.refused, NO_PROMPT);
  await pressRetry();
  await act(async () =>
    answers[0]!.reject({ code: "transport.unauthorized", message: "no", retryable: true }),
  );
  await settle();
  expect(inBody("protection-failed")).toBeNull();
});

const field = () => h!.one("composer") as HTMLTextAreaElement | null;

test.each([
  [
    "needs_set_up with no prompt",
    PROT.needs,
    pw(
      "en",
      "Your firm requires the Safe workspace on this computer. Set it up once, then ask here.",
    ),
    true,
  ],
  [
    "set_up_refused with no prompt",
    PROT.refused,
    pw(
      "en",
      "Your firm requires the Safe workspace on this computer. Set it up once, then ask here.",
    ),
    true,
  ],
  [
    "needs_set_up behind the prompt",
    PROT.needs,
    pw(
      "en",
      "Your firm requires the Safe workspace on this computer. Set it up once, then ask here.",
    ),
    true,
  ],
  [
    "admin_copy_missing",
    PROT.missing,
    pw(
      "en",
      "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.",
    ),
    false,
  ],
  [
    "set_up_failed (M4: the installer's repair)",
    PROT.failed,
    pw(
      "en",
      "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.",
    ),
    false,
  ],
  ["unsupported", PROT.unsupported, errorWords("en", "sandbox.required"), false],
  ["switched off", PROT.off, errorWords("en", "sandbox.required"), false],
  [
    "a build without protection",
    np({ unsupported: { why: "not_built" } }),
    pw(
      "en",
      "Your firm requires the Safe workspace, and this version of Clauseo doesn't have it yet, so Clauseo can't answer here. The Safe workspace comes in an update.",
    ),
    false,
  ],
  ["a full pool", PROT.pool, errorWords("en", "sandbox.pool_full"), false],
] as const)(
  "firm requires protection, %s: its notice, send held, nothing sent",
  async (_c, p, words, button) => {
    await mount(p, p, { required: true, ...(_c.endsWith("no prompt") ? NO_PROMPT : {}) });
    const notice = h!.one("protection-required");
    expect(notice?.querySelector("p")?.textContent).toBe(words);
    expect(notice?.querySelector('[data-testid="protection-setup"]') !== null).toBe(button);
    expect(field()?.disabled).toBe(true);
    await h!.type("Draft the reply.");
    await h!.key({ key: "Enter" });
    expect(h!.sent("send_prompt")).toEqual([]);
    expect(h!.sent("create_thread")).toEqual([]);
  },
);

test("the notice's Set up the Safe workspace, with no prompt, sends at once", async () => {
  await mount(PROT.needs, PROT.needs, { required: true, ...NO_PROMPT });
  await h!.click(h!.one("protection-setup"));
  await settle();
  expect(inBody("protection-confirm")).toBeNull();
  expect(sentRetries()).toEqual(["retry_protection"]);
});

test("the notice behind the prompt asks IT to install Clauseo again (a firm install), with no button", async () => {
  await mount(PROT.missing, PROT.missing, { required: true });
  expect(h!.one("protection-repair")?.textContent).toBe(
    pw("en", "Ask your IT team to install Clauseo again."),
  );
  expect(h!.one("protection-setup")).toBeNull();
  expect(h!.one("protection-installer")).toBeNull();
});

test("required false: no notice, and send goes as today", async () => {
  await mount(PROT.unsupported);
  expect(h!.one("protection-required")).toBeNull();
  await h!.type("Draft the reply.");
  await h!.key({ key: "Enter" });
  expect(h!.sent("send_prompt")).toHaveLength(1);
});

test("the room's protection command type carries retry_protection alone", () => {
  const ok: RoomProtectionCommand = { retry_protection: {} };
  // @ts-expect-error: Settings' command, never the room's
  const remove: RoomProtectionCommand = { remove_sandbox_setup: {} };
  expect([ok, remove]).toHaveLength(2);
});

test("no file under room/ imports Settings' Safe workspace, and only commands.ts sends a protection command", () => {
  const src = join(import.meta.dirname, "..", "..");
  const files = readdirSync(join(src, "room"), { recursive: true })
    .map((f) => join(src, "room", String(f)))
    .filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f));
  for (const file of files)
    expect(readFileSync(file, "utf8"), file).not.toMatch(/settings\/sections\/workspace/);
  const senders = files.filter((f) =>
    /retry_protection: \{\}|remove_sandbox_setup: \{\}/.test(readFileSync(f, "utf8")),
  );
  expect(senders.map((f) => f.slice(src.length + 1))).toEqual(["room/protection/commands.ts"]);
});
