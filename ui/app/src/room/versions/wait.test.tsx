// @vitest-environment happy-dom
// Restore and Undo wait while an answer runs in the matter: dimmed and still focusable, with the line that says so,
// for every status list_threads gives a running conversation and after the host refuses an act because one runs. The
// line never says when the answer is expected, in either language.
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { streamOf } from "../../store/view.ts";
import { header, reset, STREAM, THREAD, TURN, upsert } from "../../testing/stream-script.ts";
import type { ThreadStatus, ThreadSummary, ViewEntry } from "../../transport/protocol.ts";
import {
  file,
  mountVersions,
  NOON,
  OTHER,
  stepEntry,
  thread,
  version,
  type VersionsHarness,
} from "./testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const VERSIONS: ViewEntry[] = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];

const lookAt = async (h: VersionsHarness, checkpoint: string) =>
  h.click(h.all("version").find((r) => r.dataset["checkpoint"] === checkpoint));
const waits = (h: VersionsHarness) => {
  for (const id of ["version-undo", "version-restore"])
    expect(h.one(id)?.getAttribute("aria-disabled"), id).toBe("true");
};

/** The thread's header, in any status. */
const headerIn = (status: ThreadStatus): ViewEntry => {
  const e = header();
  return e.header === undefined ? e : { header: { ...e.header, status } };
};

/** The other conversation's header, in a status. */
const otherHeader = (status: "idle" | "running"): ViewEntry => {
  const e = header(status);
  return e.header === undefined ? e : { header: { ...e.header, thread: OTHER } };
};

const BUSY: [string, ThreadStatus][] = [
  ["starting", "starting"],
  ["running", "running"],
  ["needs_you", "needs_you"],
  ["queued", { queued: { behind: 1 } } as ThreadStatus],
];

for (const [name, status] of BUSY) {
  test(`Restore and Undo wait while a conversation is ${name}, and pressing them asks nothing`, async () => {
    // list_threads and the conversation's own header agree: the header, where the room holds the stream, is the newer
    h = await mountVersions(VERSIONS, STEPS, {
      threads: [thread(THREAD, "Draft reply", status)],
    });
    await h.deliver(STREAM, [upsert(headerIn(status))]);
    await h.openVersions();
    await lookAt(h, "cp2");
    waits(h);
    expect(h.one("versions-wait")?.textContent).toBe(
      "Restore and Undo wait until the answer in Draft reply is done.",
    );
    await h.click(h.one("version-undo"));
    await h.click(h.one("version-restore"));
    expect(document.body.querySelector('[role="dialog"]')).toBeNull();
    expect(
      h.transport.only("query").some((c) => JSON.stringify(c).includes("preview_restore")),
    ).toBe(false);
  });
}

const refusal = { code: "matter.turn_running", message: "the host's own words", retryable: true };

test("after undo_step is refused because an answer runs, Restore and Undo wait until a list_threads asked after it says none does", async () => {
  const later: { answer?: (r: { threads: ThreadSummary[] }) => void } = {};
  let refusedYet = false;
  h = await mountVersions(VERSIONS, STEPS, {
    query: (q) =>
      typeof q === "object" && q.list_threads !== undefined && refusedYet
        ? new Promise((resolve) => (later.answer = resolve))
        : undefined,
    dispatch: () => {
      refusedYet = true;
      throw refusal;
    },
  });
  await h.openVersions();
  await h.lookAt("cp2");
  expect(h.one("versions-wait")).toBeNull();
  await h.click(h.one("version-undo"));
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("undo_step")).toHaveLength(1);
  waits(h);
  expect(h.one("versions-wait")?.textContent).toBe(errorWords("en", "matter.turn_running"));
  expect(document.body.textContent).not.toContain(refusal.message);
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBe("true");
  // the conversations read again after the refusal: none runs now, and Restore and Undo go ahead again
  later.answer?.({ threads: [thread()] });
  await h.settle();
  expect(h.one("versions-wait")).toBeNull();
  expect(h.one("dialog-confirm")?.getAttribute("aria-disabled")).toBeNull();
});

test("after a refusal, a conversation list_threads calls running is held until its header rests, with nothing new on the matter's stream", async () => {
  let running = false;
  h = await mountVersions(VERSIONS, STEPS, {
    // the room opens the newest conversation: the other one is listed only once it runs
    query: (q) =>
      typeof q === "object" && q.list_threads !== undefined
        ? {
            threads: running
              ? [thread(THREAD, "Notes"), thread(OTHER, "Draft reply", "running")]
              : [thread(THREAD, "Notes")],
          }
        : undefined,
    dispatch: () => {
      running = true;
      throw refusal;
    },
  });
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-undo"));
  await h.click(h.one("dialog-confirm"));
  waits(h);
  expect(h.one("versions-wait")?.textContent).toBe(
    "Restore and Undo wait until the answer in Draft reply is done.",
  );
  // Versions holds the running conversation's stream, which the room doesn't otherwise
  const other = streamOf(OTHER);
  expect(h.transport.only("subscribe").some((c) => c.stream === other)).toBe(true);
  await h.deliver(other, [reset([], otherHeader("running"))]);
  waits(h);
  // the turn changed no file: its header rests, the matter's stream stays still, and the buttons come back
  await h.deliver(other, [upsert(otherHeader("idle"))]);
  expect(h.one("versions-wait")).toBeNull();
  for (const id of ["version-undo", "version-restore"])
    expect(h.one(id)?.getAttribute("aria-disabled"), id).toBeNull();
  // and the hold is let go
  expect(h.transport.only("unsubscribe").some((c) => c.stream === other)).toBe(true);
  expect(h.store.view(other)).toBeUndefined();
});

test("after restore_version is refused because an answer runs, the line names the conversation list_threads says runs", async () => {
  let running = false;
  h = await mountVersions(VERSIONS, STEPS, {
    query: (q) => {
      if (typeof q !== "object") return undefined;
      if (q.preview_restore !== undefined)
        return {
          restore_preview: {
            changes: [file("brief.txt")],
            added_since: [],
            kept: [],
            locked: [],
          },
        };
      if (q.list_threads !== undefined)
        return {
          threads: running
            ? [thread(THREAD, "Notes"), thread(OTHER, "Draft reply", "running")]
            : [thread(THREAD, "Notes")],
        };
      return undefined;
    },
    dispatch: () => {
      running = true;
      throw refusal;
    },
  });
  await h.openVersions();
  await lookAt(h, "cp2");
  await h.click(h.one("version-restore"));
  await h.click(h.one("dialog-confirm"));
  expect(h.sent("restore_version")).toHaveLength(1);
  waits(h);
  // the refusal reads the conversations again: the one that runs names the line
  expect(h.one("versions-wait")?.textContent).toBe(
    "Restore and Undo wait until the answer in Draft reply is done.",
  );
  // a running answer is no failure: no line under the band says the act failed
  expect(document.body.querySelector('.ver-notice[data-tone="failed"]')).toBeNull();
});

for (const language of ["en", "hi"] as const) {
  test(`no line in the room says when an answer is expected while one runs (${language})`, async () => {
    h = await mountVersions(VERSIONS, [...STEPS, stepEntry(9, { files: [file("late.txt")] })], {
      threads: [thread(THREAD, "Draft reply", "running")],
      display: { language },
    });
    await h.deliver(STREAM, [upsert(header("running"))]);
    await h.openVersions();
    const expected = /expected by|अपेक्षित/i;
    // at now, with the not-saved notice and the line that says Restore and Undo wait
    expect(h.one("versions-wait")).not.toBeNull();
    expect(document.body.textContent).not.toMatch(expected);
    // looking back
    await lookAt(h, "cp2");
    expect(h.one("versions-wait")).not.toBeNull();
    expect(document.body.textContent).not.toMatch(expected);
  });
}
