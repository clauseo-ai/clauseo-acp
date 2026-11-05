// noticeLine: the app's line for each notice kind it knows, and null for everything else, so the row shows the notice's
// text. Read by shape: an absent kind, an unknown string, and an object this version doesn't know give null.
import { t, type Key } from "@clauseo/components/lib/words.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { expect, test } from "vitest";
import type { NoticeKind, PackSkipReason } from "../../transport/protocol.ts";
import { noticeLine, type NoticeWords } from "./notice-words.ts";

const words = (lang: "en" | "hi"): NoticeWords => ({
  lang,
  th: (key: ThreadKey, slots) => th(lang, key, slots),
  t: (key: Key, slots) => t(lang, key, slots),
});
const en = words("en");

/** Papers as Versions names them: isolated, so a right-to-left mark can't reorder the line. */
const iso = (name: string) => `\u2068${name}\u2069`;

const CONTEXT =
  "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.";

test("a known kind gives the app's line, in the page's words", () => {
  expect(noticeLine("context_not_restored", en)).toBe(CONTEXT);
});

test("no kind (an agent's notice) gives null, so the row shows its own text", () => {
  expect(noticeLine(undefined, en)).toBeNull();
});

test("a kind this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine("something_newer" as NoticeKind, en)).toBeNull();
});

test("an object kind this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine({ other: {} } as unknown as NoticeKind, en)).toBeNull();
  expect(noticeLine(null as unknown as NoticeKind, en)).toBeNull();
});

test("the Hindi line is the English until its Hindi comes (hi: later)", () => {
  expect(noticeLine("context_not_restored", words("hi"))).toBe(CONTEXT);
});

test("one paper that came back: its name, and the singular line", () => {
  const kind: NoticeKind = { files_came_back: { files: ["Pleadings/Reply.docx"], more: 0 } };
  expect(noticeLine(kind, en)).toBe(
    `1 paper came back after the undo: ${iso("Reply.docx")}. Clauseo didn't remove it again.`,
  );
});

test("several papers that came back, none more: each name, joined as Versions joins them", () => {
  const kind: NoticeKind = {
    files_came_back: { files: ["Drafts/Lease.docx", "Notice.docx", "Reply.docx"], more: 0 },
  };
  expect(noticeLine(kind, en)).toBe(
    `3 papers came back after the undo: ${iso("Lease.docx")}, ${iso("Notice.docx")} and ${iso("Reply.docx")}. Clauseo didn't remove them again.`,
  );
});

test("papers with more beyond the five: the shared '{n} more' ends the list, and n counts them all", () => {
  const kind: NoticeKind = {
    files_came_back: {
      files: ["A.docx", "B.docx", "C.docx", "D.docx", "E.docx"],
      more: 3,
    },
  };
  expect(noticeLine(kind, en)).toBe(
    `8 papers came back after the undo: ${iso("A.docx")}, ${iso("B.docx")}, ${iso("C.docx")}, ${iso("D.docx")}, ${iso("E.docx")} and 3 more. Clauseo didn't remove them again.`,
  );
});

test("two papers with one bare name in different folders: each shows its whole path", () => {
  const kind: NoticeKind = {
    files_came_back: {
      files: ["Drafts/Reply.docx", "Pleadings/Reply.docx", "Notice.docx"],
      more: 0,
    },
  };
  expect(noticeLine(kind, en)).toBe(
    `3 papers came back after the undo: ${iso("Drafts/Reply.docx")}, ${iso("Pleadings/Reply.docx")} and ${iso("Notice.docx")}. Clauseo didn't remove them again.`,
  );
});

test("an object kind with no files and no more gives null, so the row shows its text", () => {
  expect(noticeLine({ files_came_back: { files: [], more: 0 } }, en)).toBeNull();
});

const skipped = (reason: string): NoticeKind =>
  ({ skill_pack_not_loaded: { pack: "nda-kit", reason } }) as NoticeKind;

test.each<[PackSkipReason, string]>([
  [
    "not_turned_on",
    "Clauseo didn't load the skill pack 'nda-kit'. This matter is set to use it, but it isn't turned on.",
  ],
  [
    "not_admitted",
    "Clauseo didn't load the skill pack 'nda-kit'. The skill pack settings on this computer don't allow it.",
  ],
  [
    "changed",
    "Clauseo didn't load the skill pack 'nda-kit'. Its files changed after it was turned on. Turn it on again to use it.",
  ],
  [
    "missing",
    "Clauseo didn't load the skill pack 'nda-kit'. Clauseo couldn't find it on this computer.",
  ],
  ["invalid", "Clauseo didn't load the skill pack 'nda-kit'. Its files aren't a valid skill pack."],
  [
    "too_many",
    "Clauseo didn't load the skill pack 'nda-kit'. Clauseo loads at most 16 skill packs at a time.",
  ],
])("a skill pack left out for %s gives its line, with the pack named", (reason, line) => {
  expect(noticeLine(skipped(reason), en)).toBe(line);
  expect(noticeLine(skipped(reason), words("hi"))).toBe(line);
});

test("a skill pack left out for a reason this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine(skipped("unknown"), en)).toBeNull();
  expect(noticeLine(skipped("something_newer"), en)).toBeNull();
});

test("a skill pack notice with no reason gives null, so the row shows its text", () => {
  expect(
    noticeLine({ skill_pack_not_loaded: { pack: "nda-kit" } } as unknown as NoticeKind, en),
  ).toBeNull();
});

test("a skill pack notice with no pack gives null, so the row shows its text", () => {
  expect(
    noticeLine({ skill_pack_not_loaded: { reason: "changed" } } as unknown as NoticeKind, en),
  ).toBeNull();
});
