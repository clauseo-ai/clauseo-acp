// @vitest-environment happy-dom
// UC in the room while the firm requires the Safe workspace (ProtectionView.required): the Not protected note says
// the firm's line for its cause, the composer notice's own, and never that Clauseo works without the Safe workspace;
// it opens only on a press, since the composer's notice already says it. Without the requirement the note is
// protection.test.tsx's.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { MatterSummary, Protection, ProtectionView } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
import { NO_PROMPT, np, PROT, view } from "./fixtures.ts";

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const REQUIRED = { required: true } as Partial<ProtectionView>;

const mount = async (protection: Protection, over: Partial<ProtectionView> = REQUIRED) => {
  const m: MatterSummary = { ...THE_MATTER, protection };
  h = await mountRoom({
    matters: [m],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: { view: () => view(protection, over) },
  });
  await settle();
};
const settle = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};
const note = () => document.body.querySelector<HTMLElement>('[data-testid="np-note"]');
const noteText = () => note()?.textContent ?? "";
const openNote = async () => {
  await h!.click(h!.one("workspace-line"));
  await settle();
};

/** Lines that say Clauseo goes on working here without the Safe workspace. */
const WORKS_WITHOUT = /works without|answers without|can read any file|anywhere on this computer/;

const POOL_LINE = pw(
  "en",
  "Your firm requires the Safe workspace, and all its places on this computer are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
);

test("the required pool note says the firm's line, not that the matter works without it", async () => {
  await mount(PROT.pool);
  await openNote();
  expect(note()?.dataset["required"]).toBe("true");
  expect(noteText()).toContain(POOL_LINE);
  expect(noteText()).not.toMatch(WORKS_WITHOUT);
  // the firm's line already says to end a conversation; the counts and Add room still stand
  expect(noteText()).not.toContain(
    pw("en", "To free one now, end a conversation in another matter."),
  );
  expect(document.body.querySelector('[data-testid="np-pool"]')).not.toBeNull();
  expect(noteText()).toContain(
    pw("en", "Clauseo can add room for 4 more. Windows asks for permission first."),
  );
});

test.each([
  ["needs_set_up", PROT.needs, NO_PROMPT, "Set it up once"],
  ["set_up_refused", PROT.refused, {}, "Set it up once"],
  ["set_up_failed", PROT.failed, {}, null],
  ["unsupported", PROT.unsupported, {}, null],
  ["a newer cause", PROT.newer, {}, null],
  ["switched off", PROT.off, {}, null],
  ["not_built", np({ unsupported: { why: "not_built" } }), {}, "comes in an update"],
  ["admin_copy_missing", PROT.missing, {}, "until it's set up"],
  ["logon_right", PROT.logon, {}, "Set it up once"],
  ["policy", PROT.policy, {}, null],
  ["other_account", PROT.otherAccount, {}, null],
] as const)(
  "required, %s: the note says the composer's line, never that Clauseo works without it",
  async (_, protection, over, says) => {
    await mount(protection, { ...REQUIRED, ...over });
    const notice = h!.one("protection-required")?.querySelector("p")?.textContent ?? "";
    expect(notice).not.toBe("");
    await openNote();
    expect(note()?.dataset["required"]).toBe("true");
    expect(noteText()).toContain(notice);
    expect(noteText()).not.toMatch(WORKS_WITHOUT);
    if (says === null) expect(notice).toBe(errorWords("en", "sandbox.required"));
    else expect(notice).toContain(says);
  },
);

test.each([
  ["set-up", PROT.needs],
  ["can't", PROT.unsupported],
  ["switched off", PROT.off],
] as const)(
  "required, %s: the note waits for a press, the composer's notice says it",
  async (_, protection) => {
    await mount(protection);
    expect(h!.one("protection-required")).not.toBeNull();
    expect(note()).toBeNull();
    await openNote();
    expect(note()).not.toBeNull();
  },
);

test("not required, the note is unchanged: the cause's body, and it opens by itself", async () => {
  await mount(PROT.needs, {});
  expect(note()?.dataset["required"]).toBe("false");
  expect(noteText()).toContain(
    pw(
      "en",
      "The Safe workspace isn't set up on this computer yet. Until it is, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.",
    ),
  );
  expect(h!.one("protection-required")).toBeNull();
});
