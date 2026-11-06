// @vitest-environment happy-dom
// UC's commands in the room, over FakeTransport with the protection wire: the "Unknown publisher" confirm comes before
// a retry_protection that asks an administrator, and only confirming sends it (Cancel sends nothing); without R17's
// retry_asks_admin the fallback asks for needs_set_up and set_up_refused only, and the field wins once it is there. The
// waiting line ends on the answer or the first protection_changed, then the view is read once; a refusal says its
// code's words. The firm's requirement holds send with its notice. The window split: the room sends only
// retry_protection, and nothing under room/ imports Settings' Safe workspace.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { MATTER } from "../../testing/stream-script.ts";
import type { Protection, ProtectionView } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
import type { RoomProtectionCommand } from "./commands.ts";
import { PROT, view } from "./fixtures.ts";

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
  ["needs_set_up", PROT.needs, "Set up protection"],
  ["set_up_refused", PROT.refused, "Try again"],
] as const)("%s: the retry opens the confirm first; Cancel sends nothing", async (_c, p, label) => {
  await mount(p);
  await h!.click(h!.one("workspace-line"));
  await settle();
  expect(inBody("protection-setup")?.textContent).toBe(label);
  await h!.click(inBody("protection-setup"));
  await settle();
  const confirm = inBody("protection-confirm");
  expect(confirm?.dataset["act"]).toBe("setup");
  expect(confirm?.textContent).toContain(
    pw(
      "en",
      "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to set up protection.",
    ),
  );
  expect(sentRetries()).toEqual([]);
  await h!.click(inBody("protection-confirm-cancel"));
  await settle();
  expect(inBody("protection-confirm")).toBeNull();
  expect(sentRetries()).toEqual([]);
});

test("confirming sends one retry_protection, and a second press while it runs sends nothing", async () => {
  const answers = await mount(PROT.needs);
  await pressRetry();
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

test("set_up_failed with no retry_asks_admin: Try again sends at once, no confirm", async () => {
  await mount(PROT.failed);
  await pressRetry();
  expect(inBody("protection-confirm")).toBeNull();
  expect(sentRetries()).toEqual(["retry_protection"]);
  expect(inBody("protection-wait")?.textContent).toBe(pw("en", "Setting up the Safe workspace."));
});

test.each([
  ["needs_set_up", PROT.needs, false, false],
  ["set_up_failed", PROT.failed, true, true],
] as const)(
  "%s with retry_asks_admin %s scripted: the field wins over the fallback",
  async (_c, p, asks, confirms) => {
    await mount(p, p, { retry_asks_admin: asks } as Partial<ProtectionView>);
    await pressRetry();
    expect(inBody("protection-confirm") !== null).toBe(confirms);
    expect(sentRetries()).toEqual(confirms ? [] : ["retry_protection"]);
  },
);

test("a full pool's Add room always confirms first", async () => {
  const pool = {
    not_protected: { cause: { pool_full: { size: 8, open: 8, held: 0, owed: 0, set_aside: 0 } } },
  } as Protection;
  await mount(pool, PROT.b, { retry_asks_admin: false } as Partial<ProtectionView>);
  await pressRetry();
  expect(inBody("protection-confirm")?.dataset["act"]).toBe("setup");
  expect(sentRetries()).toEqual([]);
});

test("the wait ends on the answer, then the view is read once; the protected matter's note closes", async () => {
  const answers = await mount(PROT.failed);
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
  await mount(PROT.failed);
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
    const answers = await mount(PROT.failed);
    await pressRetry();
    await act(async () => answers[0]!.reject({ code, message: "no", retryable: true }));
    await settle();
    expect(inBody("protection-failed")?.textContent).toBe(errorWords("en", code));
    expect(inBody("protection-setup")).not.toBeNull();
  },
);

test("a declined native confirm (transport.unauthorized) says nothing new", async () => {
  const answers = await mount(PROT.failed);
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
    "needs_set_up",
    PROT.needs,
    pw("en", "Your firm requires protection on this computer. Set it up once, then ask here."),
    true,
  ],
  [
    "set_up_refused",
    PROT.refused,
    pw("en", "Your firm requires protection on this computer. Set it up once, then ask here."),
    true,
  ],
  ["set_up_failed", PROT.failed, errorWords("en", "sandbox.required"), false],
  ["unsupported", PROT.unsupported, errorWords("en", "sandbox.required"), false],
  ["switched off", PROT.off, errorWords("en", "sandbox.required"), false],
  [
    "a full pool",
    PROT.pool,
    pw(
      "en",
      "Your firm requires protection, and all of this computer's protected spaces are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
    ),
    false,
  ],
] as const)(
  "firm requires protection, %s: its notice, send held, nothing sent",
  async (_c, p, words, button) => {
    await mount(p, p, { required: true });
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

test("the notice's Set up protection goes through the confirm", async () => {
  await mount(PROT.needs, PROT.needs, { required: true });
  await h!.click(h!.one("protection-setup"));
  await settle();
  expect(inBody("protection-confirm")).not.toBeNull();
  expect(sentRetries()).toEqual([]);
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
