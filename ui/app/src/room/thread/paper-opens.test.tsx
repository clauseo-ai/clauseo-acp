// @vitest-environment happy-dom The change note's press for each paper it names (R24), over FakeTransport: a .docx
// opens in Word, an .xlsx in Excel, a .pdf with the default app (Open), each through open_file with its own file ref;
// a kind no program opens from Clauseo (.zip) shows the matter's folder (Show in Finder on the Mac); a deleted paper
// has none. Two papers on one note each open their own; many papers fold past five.
import { act } from "react";
import type { Platform } from "@clauseo/components/lib/display.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test, vi } from "vitest";
import { MATTER, reset, step, uuid } from "../../testing/stream-script.ts";
import type { ChangeKind, PaperSummary, ViewEntry } from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const paper = (n: number, path: string): PaperSummary => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});
const PAPERS = [
  paper(1, "Advice memo.docx"),
  paper(2, "Damages schedule.xlsx"),
  paper(3, "Exhibit C.pdf"),
  paper(4, "Bundle.zip"),
  paper(5, "Old rates.xls"),
  paper(6, "Reply.docx"),
];

const changed = (paths: readonly string[], change: ChangeKind = "modified"): ViewEntry =>
  step(9, {
    files: paths.map((path) => ({ path, change, size_bytes: 10, may_include_lawyer_edits: false })),
    checkpoint_after: uuid(1, 9),
  });

async function noteWith(entry: ViewEntry, platform: Platform = "mac") {
  const room = await mountRoom({
    platform,
    next: fakeNext({ matters: () => [], papers: PAPERS, parts: ["papers"] }),
  });
  await act(settled);
  await room.settle();
  room.deliver([reset([entry])]);
  await act(settled);
  await room.settle();
  return room;
}

/** The note's rows: each paper's name and its press's words and test id, or null for none. */
const rows = () =>
  [...(h!.one("files-changed")?.querySelectorAll("li.change-row") ?? [])].map((li) => {
    const press = li.querySelector<HTMLElement>("button");
    return [
      li.querySelector('[data-testid="file-changed"]')?.textContent,
      press === null ? null : [press.textContent, press.dataset["testid"]],
    ];
  });
const pressFor = (name: string) =>
  h!.one("files-changed")?.querySelector<HTMLElement>(`button[data-paper="${name}"]`) ?? null;
const opened = () => h!.transport.only("open_file").map((c) => c.file);

test("a .docx, an .xlsx, a .pdf and a .zip each show the press for their kind", async () => {
  h = await noteWith(
    changed([
      "Advice memo.docx",
      "Damages schedule.xlsx",
      "Exhibit C.pdf",
      "Bundle.zip",
      "Old rates.xls",
    ]),
  );
  expect(rows()).toEqual([
    ["Advice memo.docx", ["Open in Word", "open-in-word"]],
    ["Damages schedule.xlsx", ["Open in Excel", "open-paper"]],
    ["Exhibit C.pdf", ["Open", "open-paper"]],
    ["Bundle.zip", ["Show in Finder", "show-paper"]],
    ["Old rates.xls", ["Open in Excel", "open-paper"]],
  ]);
});

test("on Windows a kind no program opens says Show in File Explorer", async () => {
  h = await noteWith(changed(["Bundle.zip"]), "windows");
  expect(rows()).toEqual([["Bundle.zip", ["Show in File Explorer", "show-paper"]]]);
});

test("each press opens its own paper by its file ref; Show in Finder shows the matter's folder", async () => {
  h = await noteWith(
    changed(["Advice memo.docx", "Damages schedule.xlsx", "Exhibit C.pdf", "Bundle.zip"]),
  );
  await h.click(pressFor("Damages schedule.xlsx"));
  await h.settle();
  expect(opened()).toEqual([uuid(2, 8)]);
  await h.click(pressFor("Exhibit C.pdf"));
  await h.settle();
  await h.click(pressFor("Advice memo.docx"));
  await h.settle();
  expect(opened()).toEqual([uuid(2, 8), uuid(3, 8), uuid(1, 8)]);
  expect(h.transport.only("reveal")).toEqual([]);
  await h.click(pressFor("Bundle.zip"));
  await h.settle();
  expect(h.transport.only("reveal")).toEqual([{ call: "reveal", target: { matter: MATTER } }]);
  expect(opened()).toHaveLength(3);
});

test("two Word documents on one note each open their own", async () => {
  h = await noteWith(changed(["Advice memo.docx", "Reply.docx"]));
  expect(rows().map((r) => r[1])).toEqual([
    ["Open in Word", "open-in-word"],
    ["Open in Word", "open-in-word"],
  ]);
  await h.click(pressFor("Reply.docx"));
  await h.settle();
  await h.click(pressFor("Advice memo.docx"));
  await h.settle();
  expect(opened()).toEqual([uuid(6, 8), uuid(1, 8)]);
});

test("a deleted paper, and an openable one the wall doesn't list, have no press", async () => {
  h = await noteWith(changed(["Advice memo.docx"], "deleted"));
  expect(rows()).toEqual([["Advice memo.docx", null]]);
  await h.unmount();
  document.body.replaceChildren();
  h = await noteWith(changed(["Unlisted.xlsx"]));
  expect(rows()).toEqual([["Unlisted.xlsx", null]]);
});

test("a failed open says why in the note's error line, by its code", async () => {
  h = await noteWith(changed(["Damages schedule.xlsx"]));
  vi.spyOn(console, "warn").mockImplementation(() => {});
  h.transport.host.openFile = () => {
    throw { code: "internal", message: "no app", retryable: false };
  };
  await h.click(pressFor("Damages schedule.xlsx"));
  await h.settle();
  const error = h.one("files-changed")?.querySelector<HTMLElement>('[data-testid="error"]');
  expect(error?.dataset["code"]).toBe("internal");
});

test("past six papers the rest fold under {n} more, and Show fewer folds them again", async () => {
  const many = Array.from({ length: 8 }, (_, i) => `Exhibit ${i + 1}.pdf`);
  h = await noteWith(changed(many));
  expect(rows()).toHaveLength(5);
  const more = h.one("files-changed")?.querySelector<HTMLElement>('[data-testid="change-more"]');
  expect(more?.textContent).toBe("3 more");
  await h.click(more);
  expect(rows()).toHaveLength(8);
  expect(more?.textContent).toBe("Show fewer");
});
