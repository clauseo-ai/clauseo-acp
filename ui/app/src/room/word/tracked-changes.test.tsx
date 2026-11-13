// @vitest-environment happy-dom
// A .docx's tracked changes over FakeTransport: the Tracked changes press on a .docx tile only, and only when the host
// found tracked changes or comments in it (never a .doc, which Clauseo's Word tool can't read, nor a PDF), opening
// the file's changes in the wall's place, with Back to the tiles.
// The list from list_revisions, each change by its author as recorded; Accept and Reject, one or all, send
// decide_revisions with the list's version and the changes' ids, read the list again and show the latest decision with
// Undo, which restores the file from the checkpoint before it. Word holding the file opens the lock's dialog, whose Try
// again sends the same decision as a new command; a stale list says so and is read again; a file the tool can't read,
// no tool, a failed edit and a running turn each say their line. axe finds nothing on the panel.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { rdt } from "@clauseo/components/lib/words-reader.ts";
import { afterEach, beforeAll, expect, test } from "vitest";
import { MATTER, uuid } from "../../testing/stream-script.ts";
import type {
  CommandOutcome,
  DecideRevisions,
  PaperSummary,
  Revision,
  RevisionList,
} from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { defaultOutcome, mountRoom, type Harness, type Script } from "../thread/harness.ts";
import { clockOf, localZone } from "../when.ts";

let h: Harness | null = null;
beforeAll(() => {
  // the page's own title, as index.html gives it, for axe's document-title rule
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const REPLY = "drafts/Reply.docx";
const paper = (n: number, path: string, tracked = false): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
  ...(tracked ? { tracked } : {}),
});
// the host found tracked changes or comments in Reply.docx alone
const PAPERS = [
  paper(1, REPLY, true),
  paper(2, "Old notice.DOC", true),
  paper(3, "Exhibit.pdf"),
  paper(4, "Clean.docx"),
];

const NOW = Date.now();
const rev = (ids: number[], kind: Revision["kind"], author: string, text?: string): Revision => ({
  ids,
  kind,
  author,
  at: NOW,
  block: `p${ids[0] ?? 0}`,
  ...(text === undefined ? {} : { text }),
});
const FIRST: Revision[] = [
  rev([1], "insert", "A. Mehta", "within fourteen days"),
  rev([2, 3], "delete", "R. Rao", "forthwith"),
  rev([4], "move", "A. Mehta", "The letter is annexed."),
  rev([5], "format", "A. Mehta"),
];
const listOf = (revisions: Revision[], version = "v1"): RevisionList => ({
  path: REPLY,
  version,
  revisions,
  truncated: false,
  total: revisions.length,
});
const refusal = (code: string, files?: string[]) => ({
  code,
  message: "refused",
  retryable: true,
  ...(files === undefined ? {} : { files }),
});

type World = {
  /** list_revisions' answer each time it is asked; throws a refusal to refuse it. */
  list: (n: number) => RevisionList;
  /** decide_revisions' answer; the nth decision's checkpoints by default. */
  decide?: (d: DecideRevisions, n: number) => CommandOutcome;
  /** restore_version's refusal, if any. */
  restore?: () => void;
};

async function mount(world: World, script: Script = {}): Promise<Harness> {
  let lists = 0;
  let decisions = 0;
  const room = await mountRoom({
    ...script,
    next: fakeNext({ matters: () => [], papers: PAPERS, parts: ["papers"] }),
    query: (query) => {
      if (typeof query === "object" && query.list_revisions !== undefined) {
        lists += 1;
        return { revisions: world.list(lists) };
      }
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (command) => {
      if (command.decide_revisions !== undefined) {
        decisions += 1;
        return (
          world.decide?.(command.decide_revisions, decisions) ?? {
            revisions_decided: { before: `before-${decisions}`, checkpoint: `cp-${decisions}` },
          }
        );
      }
      if (command.restore_version !== undefined) {
        world.restore?.();
        return { version_restored: { checkpoint: "cp-restored" } };
      }
      return defaultOutcome(command);
    },
  });
  await act(settled);
  await room.settle();
  return room;
}

const listed = () =>
  h!.transport
    .only("query")
    .filter((c) => typeof c.query === "object" && "list_revisions" in c.query);
const decisionsSent = () =>
  h!.transport.only("dispatch").filter((c) => c.command.command.decide_revisions !== undefined);
const panel = () => h!.one("word-changes");
const line = () => h!.one("word-changes-line")?.textContent ?? null;
const rows = () => [...(panel()?.querySelectorAll<HTMLElement>(".rd-change") ?? [])];
const tiles = () => [...(h?.host.querySelectorAll("aside.room-pane li.tile") ?? [])];
const press = async (el: Element | null | undefined) => {
  await h!.click(el);
  await act(settled);
  await h!.settle();
};
const openChanges = () => press(h!.one("tracked-changes"));
const acceptOf = (n: number) => rows()[n]?.querySelector(".rd-accept");
const dialog = () => document.querySelector<HTMLElement>("[role=dialog]");

test("the Tracked changes press shows on a .docx tile the host found changes in only, opens its changes in the wall's place, and Back brings the tiles back", async () => {
  h = await mount({ list: () => listOf(FIRST) });
  const presses = h.all("tracked-changes");
  expect(presses).toHaveLength(1);
  expect(presses[0]?.closest("li.tile")?.querySelector(".tile-title")?.textContent).toBe(
    "Reply.docx",
  );
  expect(presses[0]?.textContent).toBe("Tracked changes");
  // Open in Word stays each Word tile's main press, the .doc's too
  expect(h.all("open-in-word").map((b) => b.querySelector(".tile-title")?.textContent)).toEqual([
    "Reply.docx",
    "Old notice.DOC",
    "Clean.docx",
  ]);
  await openChanges();
  expect(panel()?.closest("aside.room-pane")).not.toBeNull();
  expect(tiles()).toHaveLength(0);
  expect(listed().map((c) => c.query)).toEqual([
    { list_revisions: { matter: MATTER, path: REPLY } },
  ]);
  expect(panel()?.querySelector(".wc-file")?.textContent).toBe("Reply.docx");
  await press(h.one("word-changes-back"));
  expect(panel()).toBeNull();
  expect(tiles()).toHaveLength(4);
  expect(document.activeElement).toBe(h.one("tracked-changes"));
});

test("the list shows each change's author as recorded, its kind's label, its words and its time, and the count", async () => {
  h = await mount({ list: () => listOf(FIRST) });
  await openChanges();
  expect(panel()?.querySelector(".rd-review-sub")?.textContent).toBe(
    "4 changes, each with its author and time, as Word shows them.",
  );
  expect(rows().map((r) => r.querySelector(".rd-change-head b")?.textContent)).toEqual([
    "A. Mehta",
    "R. Rao",
    "A. Mehta",
    "A. Mehta",
  ]);
  expect(rows().map((r) => r.querySelector(".rd-change-kind")?.textContent)).toEqual([
    "Added",
    "Taken out",
    "Moved",
    "Formatting changed",
  ]);
  expect(rows()[0]?.querySelector("ins")?.textContent).toBe("within fourteen days");
  expect(rows()[1]?.querySelector("del")?.textContent).toBe("forthwith");
  const time = clock("en", clockOf(NOW, Date.now(), localZone(), "en").time);
  expect(rows()[0]?.querySelector(".rd-change-when")?.textContent).toBe(`Today, ${time}`);
  for (const r of rows()) expect(r.textContent).toContain("Not yet accepted");
  expect(panel()?.textContent).not.toContain("by you");
  expect(panel()?.textContent).not.toContain("You, in Word");
  expect(panel()?.textContent).toContain("Accepting here accepts the change in the Word file too.");
});

test("Accept on one change sends its ids with the list's version, reads the list again, and says Accepted with Undo", async () => {
  h = await mount({
    list: (n) => (n === 1 ? listOf(FIRST) : listOf(FIRST.slice(0, 1).concat(FIRST.slice(2)), "v2")),
  });
  await openChanges();
  await press(acceptOf(1));
  expect(decisionsSent().map((c) => c.command.command.decide_revisions)).toEqual([
    { matter: MATTER, path: REPLY, version: "v1", ids: [2, 3], decision: "accept" },
  ]);
  expect(listed()).toHaveLength(2);
  expect(rows()).toHaveLength(3);
  expect(rows().map((r) => r.querySelector(".rd-change-head b")?.textContent)).not.toContain(
    "R. Rao",
  );
  const decided = h.one("word-changes-decided");
  expect(decided?.querySelector('[role="status"]')?.textContent).toMatch(/^Accepted \d/);
  expect(h.one("word-changes-undo")?.textContent).toBe("Undo");
});

test("Undo restores the file from the checkpoint before the latest decision, reads the list again, and goes; an earlier decision shows none", async () => {
  h = await mount({ list: () => listOf(FIRST) });
  await openChanges();
  await press(acceptOf(0));
  await press(rows()[1]?.querySelector(".rd-reject"));
  // the latest decision alone stands, with its Undo
  expect(h.all("word-changes-decided")).toHaveLength(1);
  expect(h.one("word-changes-decided")?.textContent).toMatch(/^Rejected \d/);
  await press(h.one("word-changes-undo"));
  expect(h.sent("restore_version")).toEqual([
    { matter: MATTER, checkpoint: "before-2", scope: { files: { paths: [REPLY] } } },
  ]);
  expect(listed()).toHaveLength(4);
  expect(h.one("word-changes-decided")).toBeNull();
});

test("Undo refused while an answer runs says restore's line, and Undo stays", async () => {
  h = await mount({
    list: () => listOf(FIRST),
    restore: () => {
      throw refusal("matter.turn_running");
    },
  });
  await openChanges();
  await press(acceptOf(0));
  await press(h.one("word-changes-undo"));
  expect(line()).toBe(errorWords("en", "matter.turn_running"));
  expect(h.one("word-changes-undo")).not.toBeNull();
});

test("an undo refused reads the list again, and the panel shows it", async () => {
  h = await mount({
    list: (n) =>
      n === 1
        ? listOf(FIRST)
        : n === 2
          ? listOf(FIRST.slice(1), "v2")
          : listOf(FIRST.slice(2), "v2"),
    restore: () => {
      throw refusal("matter.turn_running");
    },
  });
  await openChanges();
  await press(acceptOf(0));
  await press(h.one("word-changes-undo"));
  expect(line()).toBe(errorWords("en", "matter.turn_running"));
  expect(listed()).toHaveLength(3);
  expect(rows()).toHaveLength(2);
  // the file is as the decision left it: Undo stays
  expect(h.one("word-changes-undo")).not.toBeNull();
});

test("Undo goes once a read finds the file changed after the decision; a read of the same version keeps it", async () => {
  h = await mount({
    list: (n) =>
      n === 1
        ? listOf(FIRST)
        : n <= 3
          ? listOf(FIRST.slice(1), "v2")
          : listOf(FIRST.slice(2), "v3"),
    decide: (_d, n) => {
      if (n === 1) return { revisions_decided: { before: "before-1", checkpoint: "cp-1" } };
      throw refusal("word.edit_failed");
    },
  });
  await openChanges();
  await press(acceptOf(0));
  expect(h.one("word-changes-undo")).not.toBeNull();
  // a refused decision reads the list: same version, Undo stays
  await press(rows()[0]?.querySelector(".rd-reject"));
  expect(listed()).toHaveLength(3);
  expect(h.one("word-changes-undo")).not.toBeNull();
  // another version: the file changed after the decision, so Undo goes
  await press(rows()[0]?.querySelector(".rd-reject"));
  expect(listed()).toHaveLength(4);
  expect(rows()).toHaveLength(2);
  expect(h.one("word-changes-undo")).toBeNull();
  expect(h.one("word-changes-decided")).toBeNull();
});

test("Accept all and Reject all send all of the version in view, not the listed ids", async () => {
  h = await mount({ list: () => listOf(FIRST) });
  await openChanges();
  await press(panel()?.querySelector(".rd-accept-all"));
  await press(panel()?.querySelector(".rd-reject-all"));
  expect(decisionsSent().map((c) => c.command.command.decide_revisions)).toEqual([
    { matter: MATTER, path: REPLY, version: "v1", ids: [], decision: "accept", all: true },
    { matter: MATTER, path: REPLY, version: "v1", ids: [], decision: "reject", all: true },
  ]);
});

test("600 changes: the panel counts 600 and lists 500; one Accept all decides them all and none are left", async () => {
  const shown = Array.from({ length: 500 }, (_, i) => rev([i + 1], "insert", "A. Mehta", "x"));
  h = await mount({
    list: (n) => (n === 1 ? { ...listOf(shown), truncated: true, total: 600 } : listOf([], "v2")),
  });
  await openChanges();
  expect(rows()).toHaveLength(500);
  expect(panel()?.querySelector(".rd-review-head .rd-review-sub")?.textContent).toBe(
    "600 changes, each with its author and time, as Word shows them.",
  );
  expect(h.one("word-changes-more")?.textContent).toBe(
    "The first 500 are listed here. Accept all and Reject all decide all 600.",
  );
  await press(panel()?.querySelector(".rd-accept-all"));
  expect(decisionsSent().map((c) => c.command.command.decide_revisions)).toEqual([
    { matter: MATTER, path: REPLY, version: "v1", ids: [], decision: "accept", all: true },
  ]);
  expect(rows()).toHaveLength(0);
  expect(h.one("word-changes-empty")).not.toBeNull();
  expect(h.one("word-changes-more")).toBeNull();
  // one decision, one checkpoint before it: one Undo takes back all 600
  expect(h.one("word-changes-undo")).not.toBeNull();
});

test("a file Word holds opens the lock's dialog; Try again sends the same decision as a new command, Cancel changes nothing else", async () => {
  let locked = true;
  h = await mount({
    list: () => listOf(FIRST),
    decide: (_d, n) => {
      if (locked) throw refusal("matter.file_locked", [REPLY]);
      return { revisions_decided: { before: `before-${n}`, checkpoint: `cp-${n}` } };
    },
  });
  await openChanges();
  await press(rows()[1]?.querySelector(".rd-reject"));
  const lines = [...(dialog()?.querySelectorAll("h2, p") ?? [])].map((n) => n.textContent);
  expect(lines).toEqual([
    rdt("en", "Close '{file}' in Word to reject these changes.", { file: "Reply.docx" }),
    "If it isn't open in Word, close Word and try again.",
    "Word holds the file while it is open. Nothing has changed yet.",
  ]);
  expect(line()).toBeNull();
  await press(dialog()?.querySelector(".rd-try"));
  const [first, again] = decisionsSent();
  expect(again?.command.command).toEqual(first?.command.command);
  expect(again?.command.command_id).not.toBe(first?.command.command_id);
  expect(dialog()).not.toBeNull();
  // Cancel: the dialog closes, nothing is sent, the list stands as it was
  const cancel = [...(dialog()?.querySelectorAll("button") ?? [])].find(
    (b) => b.textContent === "Cancel",
  );
  await press(cancel);
  expect(dialog()).toBeNull();
  expect(decisionsSent()).toHaveLength(2);
  expect(rows()).toHaveLength(4);
  expect(h.one("word-changes-decided")).toBeNull();
  // once Word lets go, Try again writes it
  await press(rows()[1]?.querySelector(".rd-reject"));
  locked = false;
  await press(dialog()?.querySelector(".rd-try"));
  expect(dialog()).toBeNull();
  expect(h.one("word-changes-decided")?.textContent).toMatch(/^Rejected \d/);
});

test("a list made before the file changed says so, and the list is read again", async () => {
  h = await mount({
    list: (n) => (n === 1 ? listOf(FIRST) : listOf(FIRST.slice(1), "v2")),
    decide: () => {
      throw refusal("word.revisions_stale");
    },
  });
  await openChanges();
  await press(acceptOf(0));
  expect(line()).toBe(errorWords("en", "word.revisions_stale"));
  expect(line()).toBe(
    "This file changed after the list was made, so nothing was accepted or rejected. The list now shows its current changes.",
  );
  expect(listed()).toHaveLength(2);
  expect(rows()).toHaveLength(3);
  expect(h.one("word-changes-retry")).toBeNull();
  expect(h.one("word-changes-decided")).toBeNull();
});

test("a decision refused with any code reads the list again, and the panel shows it", async () => {
  for (const code of ["word.edit_failed", "matter.file_locked", "matter.turn_running"]) {
    h = await mount({
      list: (n) => (n === 1 ? listOf(FIRST) : listOf(FIRST.slice(1), "v2")),
      decide: () => {
        throw refusal(code, code === "matter.file_locked" ? [REPLY] : undefined);
      },
    });
    await openChanges();
    await press(acceptOf(0));
    expect(listed()).toHaveLength(2);
    expect(rows()).toHaveLength(3);
    expect(rows()[0]?.querySelector(".rd-change-head b")?.textContent).toBe("R. Rao");
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

test("a file the tool can't read, and no tool, say their lines in place of the list", async () => {
  for (const code of ["word.not_docx", "word.stetdoc_missing"]) {
    h = await mount({
      list: () => {
        throw refusal(code);
      },
    });
    await openChanges();
    expect(line()).toBe(errorWords("en", code));
    expect(rows()).toHaveLength(0);
    expect(panel()?.querySelector(".rd-accept-all")).toBeNull();
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

test("a failed edit says so with Try again, which sends the decision again as a new command", async () => {
  let fails = true;
  h = await mount({
    list: () => listOf(FIRST),
    decide: (_d, n) => {
      if (fails) throw refusal("word.edit_failed");
      return { revisions_decided: { before: `before-${n}`, checkpoint: `cp-${n}` } };
    },
  });
  await openChanges();
  await press(acceptOf(2));
  expect(line()).toContain("Clauseo couldn't save your decision. The file is as it was.");
  fails = false;
  await press(h.one("word-changes-retry"));
  const [first, again] = decisionsSent();
  expect(again?.command.command).toEqual(first?.command.command);
  expect(again?.command.command_id).not.toBe(first?.command.command_id);
  expect(line()).toBeNull();
  expect(h.one("word-changes-decided")?.textContent).toMatch(/^Accepted \d/);
});

test("a file with no tracked changes says so", async () => {
  h = await mount({ list: () => listOf([]) });
  await openChanges();
  expect(h.one("word-changes-empty")?.textContent).toBe("No tracked changes in this file.");
  expect(rows()).toHaveLength(0);
});

for (const language of ["en", "hi"] as const) {
  test(`axe finds no violation on the panel, its list and its line (${language})`, async () => {
    const found: string[] = [];
    h = await mount(
      {
        list: () => listOf(FIRST),
        decide: () => {
          throw refusal("word.edit_failed");
        },
      },
      { display: { language } },
    );
    await openChanges();
    await press(acceptOf(0));
    expect(h.one("word-changes-line")).not.toBeNull();
    const results = await axe.run(document, { resultTypes: ["violations"] });
    found.push(
      ...results.violations.map(
        (v) => `${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
      ),
    );
    expect(found).toEqual([]);
  }, 20_000);
}
