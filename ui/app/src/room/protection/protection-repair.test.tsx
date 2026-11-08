// @vitest-environment happy-dom
// Design B's in-app repair in the room's note, over FakeTransport with the protection wire and a
// scripted repair_install: on a personal install where display_info says the repair runs here, a
// missing admin copy offers "Download and run the installer again" beside Get the installer. The
// press waits, disabled and busy, on "Downloading the installer…", then on Windows' prompt; a
// second press sends nothing; each refusal says its code's words, keeps Get the installer and offers
// the button again. A firm's install, or a build without the repair, keeps today's note. After the
// installer, display_info's `repair` opens the note by itself once: the copy still missing says the
// installer didn't finish; the copy there opens nothing (the installer ran the set-up); a launch
// that failed says its words.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { MATTER } from "../../testing/stream-script.ts";
import type { FakeRepairHost } from "../../transport/fake-protection.ts";
import type { Protection, ProtectionView, RepairStep } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
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

/** A repair_install the test drives: its steps, then its answer. */
function driven() {
  const runs: {
    step: (s: RepairStep) => void;
    resolve: () => void;
    reject: (e: unknown) => void;
  }[] = [];
  const repair: FakeRepairHost = {
    available: true,
    run: (step) => new Promise<void>((resolve, reject) => runs.push({ step, resolve, reject })),
  };
  return { repair, runs };
}

async function mount(
  repair: FakeRepairHost,
  protection: Protection = PROT.missing,
  over: Partial<ProtectionView> = {},
) {
  h = await mountRoom({
    matters: [{ ...THE_MATTER, protection }],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: { view: () => view(protection, over), repair },
  });
  await settle();
}

const settle = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};
const inBody = (id: string) => document.body.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const repairs = () => h!.transport.only("repair_install").length;
const openNote = async () => {
  await h!.click(h!.one("workspace-line"));
  await settle();
};
const AGAIN = "Download and run the installer again";
const cancelButton = () => inBody("protection-repair-cancel") as HTMLButtonElement | null;

test("idle, then downloading (disabled and busy), then Windows' prompt; a second press starts nothing", async () => {
  const d = driven();
  await mount(d.repair);
  await openNote();
  const button = inBody("protection-repair-again") as HTMLButtonElement;
  expect(button.textContent).toBe(pw("en", AGAIN));
  expect(button.disabled).toBe(false);
  // the repair is the dark primary; Understood, last in the row, turns quiet and keeps the focus
  const understood = inBody("np-note")!.querySelector<HTMLElement>(
    "[data-dialog-acts] > :last-child",
  );
  expect(button.classList.contains("bg-primary")).toBe(true);
  expect(understood?.textContent).toBe("Understood");
  expect(understood?.classList.contains("bg-primary")).toBe(false);
  expect(inBody("protection-installer")).not.toBeNull();
  expect(inBody("protection-repair-wait")).toBeNull();
  await h!.click(button);
  await settle();
  const busy = inBody("protection-repair-again") as HTMLButtonElement;
  expect(busy.disabled).toBe(true);
  expect(busy.getAttribute("aria-busy")).toBe("true");
  expect(inBody("protection-repair-wait")?.textContent).toBe(
    pw("en", "Downloading the installer…"),
  );
  busy.click();
  await h!.click(busy);
  await settle();
  expect(repairs()).toBe(1);
  await act(async () => d.runs[0]!.step("downloading"));
  await settle();
  expect(inBody("protection-repair-wait")?.textContent).toBe(
    pw("en", "Downloading the installer…"),
  );
  await act(async () => d.runs[0]!.step("launching"));
  await settle();
  expect(inBody("protection-repair-wait")?.textContent).toBe(
    pw("en", "Windows will ask for permission. Choose Yes."),
  );
  expect(repairs()).toBe(1);
  expect(h!.transport.protectionCommands()).toEqual([]);
});

test("Cancel beside the busy button while the installer downloads; none while Windows' prompt is up, nor idle", async () => {
  const d = driven();
  await mount(d.repair);
  await openNote();
  expect(cancelButton()).toBeNull();
  await h!.click(inBody("protection-repair-again"));
  await settle();
  expect(cancelButton()?.textContent).toBe("Cancel");
  await act(async () => d.runs[0]!.step("downloading"));
  await settle();
  expect(cancelButton()).not.toBeNull();
  await act(async () => d.runs[0]!.step("launching"));
  await settle();
  expect(cancelButton()).toBeNull();
  expect(inBody("protection-repair-wait")?.textContent).toBe(
    pw("en", "Windows will ask for permission. Choose Yes."),
  );
  expect(h!.transport.only("repair_cancel")).toEqual([]);
});

test("Cancel sends repair_cancel once; the cancelled answer leaves the card idle, with no error line", async () => {
  const d = driven();
  await mount(d.repair);
  await openNote();
  await h!.click(inBody("protection-repair-again"));
  await settle();
  await act(async () => d.runs[0]!.step("downloading"));
  await settle();
  const cancel = cancelButton()!;
  // two presses before the answer: the second one sends nothing
  await act(async () => {
    cancel.click();
    cancel.click();
  });
  await settle();
  expect(h!.transport.only("repair_cancel")).toHaveLength(1);
  expect(repairs()).toBe(1);
  // the run answered "cancelled": the card is idle again
  expect(cancelButton()).toBeNull();
  expect(inBody("protection-repair-wait")).toBeNull();
  expect(inBody("protection-failed")).toBeNull();
  expect(inBody("protection-installer")).not.toBeNull();
  const again = inBody("protection-repair-again") as HTMLButtonElement;
  expect(again.disabled).toBe(false);
  expect(again.getAttribute("aria-busy")).toBe("false");
  await h!.click(again);
  await settle();
  expect(repairs()).toBe(2);
  expect(inBody("protection-repair-wait")?.textContent).toBe(
    pw("en", "Downloading the installer…"),
  );
});

test.each([
  "repair.download_failed",
  "repair.verify_failed",
  "repair.launch_failed",
  "repair.not_eligible",
  "command.conflict",
])("%s says its words, keeps Get the installer, and offers the button again", async (code) => {
  const d = driven();
  await mount(d.repair);
  await openNote();
  await h!.click(inBody("protection-repair-again"));
  await settle();
  await act(async () => {
    d.runs[0]!.step("downloading");
    d.runs[0]!.reject({ code, message: "", retryable: true });
  });
  await settle();
  expect(inBody("protection-failed")?.textContent).toBe(errorWords("en", code));
  expect(inBody("protection-installer")).not.toBeNull();
  expect(inBody("protection-repair-wait")).toBeNull();
  const again = inBody("protection-repair-again") as HTMLButtonElement;
  expect(again.disabled).toBe(false);
  await h!.click(again);
  await settle();
  expect(repairs()).toBe(2);
  expect(inBody("protection-failed")).toBeNull();
});

test.each([
  ["required by the firm", { required: true }],
  ["a firm's policy (unprotected_auto_allowed off)", { unprotected_auto_allowed: false }],
] as const)("a firm install (%s): no repair button, as today", async (_c, over) => {
  await mount({ available: true }, PROT.missing, over);
  await openNote();
  expect(inBody("protection-repair-again")).toBeNull();
  expect(inBody("protection-installer")).toBeNull();
  expect(document.body.textContent).toContain(
    pw("en", "Ask your IT team to install Clauseo again."),
  );
});

test("repair_available false: today's note, with Get the installer and no new button", async () => {
  await mount({ available: false });
  await openNote();
  expect(inBody("protection-repair-again")).toBeNull();
  expect(inBody("protection-installer")).not.toBeNull();
  expect(repairs()).toBe(0);
});

test("a cause other than the missing copy offers no repair, even where it runs", async () => {
  await mount({ available: true }, PROT.policy);
  await openNote();
  expect(inBody("np-note")).not.toBeNull();
  expect(inBody("protection-repair-again")).toBeNull();
});

const DIDNT_FINISH =
  "The installer didn't finish setting up the Safe workspace. Try again, or choose Get the installer.";

test("returned with the copy still missing: the note opens by itself, says the installer didn't finish, with both buttons", async () => {
  await mount({ available: true, note: "returned" });
  const note = inBody("np-note");
  expect(note).not.toBeNull();
  expect(note?.textContent).toContain(pw("en", DIDNT_FINISH));
  expect(inBody("protection-repair-again")).not.toBeNull();
  expect(inBody("protection-installer")).not.toBeNull();
});

test("returned with the copy there: nothing is forced open; the installer's --adopt ran the set-up", async () => {
  await mount({ available: true, note: "returned" }, PROT.needs);
  // No note, no dialog, no settings window: the protection shows where it always does.
  expect(inBody("np-note")).toBeNull();
  expect(document.querySelector('[role="dialog"], [role="alertdialog"]')).toBeNull();
  expect(h!.transport.only("open_settings")).toHaveLength(0);
  expect(document.body.textContent).not.toContain(pw("en", DIDNT_FINISH));
});

test("returned, already protected: nothing opens", async () => {
  await mount({ available: true, note: "returned" }, PROT.b);
  expect(inBody("np-note")).toBeNull();
});

test("launch_failed (the window built again): the note opens with its words and Get the installer", async () => {
  await mount({ available: true, note: "launch_failed" });
  expect(inBody("np-note")).not.toBeNull();
  expect(inBody("protection-failed")?.textContent).toBe(errorWords("en", "repair.launch_failed"));
  expect(inBody("protection-installer")).not.toBeNull();
  expect(inBody("protection-repair-again")).not.toBeNull();
});
