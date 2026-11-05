// @vitest-environment happy-dom
// The line after an undo: it names the files the host put back, as the step_undone marker on the conversation's
// stream says, in the order the version's row names them (made, then changed), never a file the host couldn't write;
// and it stays through the outside edits the host wrote in the same commit as the undo.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { STREAM, THREAD, TURN, upsert, uuid } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import {
  file,
  MATTER_STREAM,
  mountVersions,
  NOON,
  stepEntry,
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
/** The turn edited brief.txt, made notes/a.txt and deleted old.txt, in that order. */
const STEPS = [
  stepEntry(5, {
    files: [
      file("brief.txt"),
      file("notes/a.txt", { change: "added" }),
      file("old.txt", { change: "deleted" }),
    ],
    checkpoint_after: "cp2",
  }),
];

/** The host's marker on the conversation's stream for the undo cp`n` wrote. */
const undoneMarker = (restored: string[], kept: string[], n = 5): ViewEntry => ({
  marker: {
    order: 50,
    event: uuid(50, 8),
    ts: NOON + 50 * 60_000,
    kind: { step_undone: { turn: TURN, checkpoint: `cp${n}`, restored, kept } },
  },
});

const line = () =>
  [...document.body.querySelectorAll(".ver-notice")]
    .map((n) => n.textContent ?? "")
    .find((t) => t.startsWith("Undid the change"));
/** The line past its time, which is the machine's zone's. */
const papers = () => line()?.replace(/^Undid the change at [^.]+\. /, "");
const went = (names: string) => `${names} went back, and the undo is a version of its own.`;
const iso = (name: string) => `\u2068${name}\u2069`;

async function undoCp2(h: VersionsHarness): Promise<void> {
  await h.openVersions();
  await h.lookAt("cp2");
  await h.click(h.one("version-undo"));
  await h.click(h.one("dialog-confirm"));
}

test("the undo's line names what went back in the row's order, then what the host's marker says it put back", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    dispatch: () => ({ step_undone: { checkpoint: "cp4" } }),
  });
  await undoCp2(h);
  // the lawyer's choices, as the row names them: the file made first, then those changed
  expect(papers()).toBe(went(`${iso("a.txt")}, ${iso("brief.txt")} and ${iso("old.txt")}`));
  // old.txt couldn't be written: the host kept it, and the line no longer says it went back
  await h.deliver(MATTER_STREAM, [upsert(version(4, "undo", { thread: THREAD, turn: TURN }))]);
  await h.deliver(STREAM, [upsert(undoneMarker(["brief.txt", "notes/a.txt"], ["old.txt"], 4))]);
  expect(papers()).toBe(went(`${iso("a.txt")} and ${iso("brief.txt")}`));
  // cold mount: the room and Versions' chunk load in this test, 0.9 s alone; 30 s for a loaded machine
}, 30_000);

test("the outside edits the host wrote with the undo, in one commit, keep the undo's line", async () => {
  h = await mountVersions(VERSIONS, STEPS, {
    dispatch: () => ({ step_undone: { checkpoint: "cp5" } }),
  });
  await undoCp2(h);
  await h.deliver(MATTER_STREAM, [
    upsert(version(4, "outside_edits", { files: [file("brief.txt")] })),
    upsert(version(5, "undo", { thread: THREAD, turn: TURN })),
  ]);
  expect(line()).not.toBeUndefined();
  // a version after the undo's: the line goes
  await h.deliver(MATTER_STREAM, [upsert(version(6, "outside_edits", { files: [file("x.txt")] }))]);
  expect(line()).toBeUndefined();
});
