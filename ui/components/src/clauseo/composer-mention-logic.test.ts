// The `@` menu's rules apart from the page: a picked paper's words, quoted when its path holds a space or a bracket;
// the filter's order (name prefix, then name, then path), case and normalisation aside, Devanagari and Tamil included;
// the 50-row cap with the count of all matches; the highlight kept by id; and what a send mentions, each paper once.
import { expect, test } from "vitest";
import type { MentionItem } from "./composer.tsx";
import {
  filterMentions,
  foldMentions,
  keepMention,
  MAX_MENTION_ROWS,
  mentionedIn,
  mentionText,
} from "./composer-mention-logic.ts";
import { triggerAt } from "./composer-menu-logic.ts";

const item = (path: string, id = path): MentionItem => {
  const cut = path.lastIndexOf("/");
  return {
    id,
    label: cut === -1 ? path : path.slice(cut + 1),
    detail: cut === -1 ? undefined : path.slice(0, cut),
    text: path,
  };
};
const labels = (items: readonly MentionItem[]) => items.map((m) => m.text);

test("a picked paper reads as @ and its path; a space, a bracket or a quote puts the path in quotes", () => {
  expect(mentionText("Notice.pdf")).toBe("@Notice.pdf");
  expect(mentionText("Pleadings/Plaint.pdf")).toBe("@Pleadings/Plaint.pdf");
  expect(mentionText("Board minutes.pdf")).toBe('@"Board minutes.pdf"');
  expect(mentionText("Exhibit(A).pdf")).toBe('@"Exhibit(A).pdf"');
  expect(mentionText("Annex[2].docx")).toBe('@"Annex[2].docx"');
  expect(mentionText("Brief{draft}.docx")).toBe('@"Brief{draft}.docx"');
  expect(mentionText("Map<old>.pdf")).toBe('@"Map<old>.pdf"');
  expect(mentionText('Say"so".pdf')).toBe('@"Say"so".pdf"');
  expect(mentionText("a\u00a0b.pdf")).toBe('@"a\u00a0b.pdf"');
  expect(mentionText("उत्तर.docx")).toBe("@उत्तर.docx");
});

test("@ is a trigger at a run's start only, so an email address opens nothing; / and @ each only when asked for", () => {
  expect(triggerAt("@", 1, "/@")).toEqual({ char: "@", at: 0, query: "" });
  expect(triggerAt("see @lea", 8, "/@")).toEqual({ char: "@", at: 4, query: "lea" });
  expect(triggerAt("rohan@clauseo.chat", 18, "/@")).toBeNull();
  expect(triggerAt("@lea", 4, "/")).toBeNull();
  expect(triggerAt("/re", 3, "@")).toBeNull();
  expect(triggerAt("/re", 3, "")).toBeNull();
});

test("the filter: name prefixes first, then other name matches, then path matches, each in the list's order", () => {
  const list = foldMentions([
    item("Old lease.docx"),
    item("Correspondence/Lease notice.pdf"),
    item("Leases/Schedule.pdf"),
    item("Lease deed.pdf"),
    item("Other.pdf"),
  ]);
  // both names that start "lease", in the list's order; then "Old lease"; then the folder's "Leases/"
  expect(labels(filterMentions(list, "lease").items)).toEqual([
    "Correspondence/Lease notice.pdf",
    "Lease deed.pdf",
    "Old lease.docx",
    "Leases/Schedule.pdf",
  ]);
  // case aside, either way
  expect(labels(filterMentions(list, "LEASE D").items)).toEqual(["Lease deed.pdf"]);
  // the quoted form's opening quote is no part of the query
  expect(labels(filterMentions(list, '"lease d').items)).toEqual(["Lease deed.pdf"]);
  // nothing typed: the list as it is
  expect(filterMentions(list, "").total).toBe(5);
  expect(filterMentions(list, "zzz")).toEqual({ items: [], total: 0 });
});

test("Devanagari and Tamil names match typed in either normalisation (NFC on both sides)", () => {
  // क़ is U+0958, which NFC writes as क + nukta; கொ is U+0BCA, which NFC composes from ெ + ா
  const list = foldMentions([item("क़ानूनी नोटिस.pdf"), item("கொடுப்பனவு.docx"), item("Lease.pdf")]);
  expect(labels(filterMentions(list, "\u0915\u093c\u093e\u0928").items)).toEqual([
    "क़ानूनी नोटिस.pdf",
  ]);
  expect(labels(filterMentions(list, "\u0958\u093e\u0928").items)).toEqual(["क़ानूनी नोटिस.pdf"]);
  expect(labels(filterMentions(list, "\u0B95\u0BC6\u0BBE").items)).toEqual(["கொடுப்பனவு.docx"]);
  expect(labels(filterMentions(list, "\u0B95\u0BCA").items)).toEqual(["கொடுப்பனவு.docx"]);
  expect(labels(filterMentions(list, "नोटिस").items)).toEqual(["क़ानूनी नोटिस.pdf"]);
});

test("at most 50 rows, prefix matches kept first, with the count of every match", () => {
  const many = Array.from({ length: 120 }, (_, i) => item(`Folder/Exhibit ${i}.pdf`));
  const list = foldMentions([...many, item("Exhibits index.pdf")]);
  const { items, total } = filterMentions(list, "exhibit");
  expect(items).toHaveLength(MAX_MENTION_ROWS);
  expect(total).toBe(121);
  expect(items[0]?.text).toBe("Folder/Exhibit 0.pdf");
  // a path-only match never pushes out a name match
  const mixed = foldMentions([
    ...Array.from({ length: 60 }, (_, i) => item(`Notes/Doc ${i}.pdf`)),
    item("Notes.pdf"),
  ]);
  expect(filterMentions(mixed, "notes").items[0]?.text).toBe("Notes.pdf");
});

test("the highlight is kept by id across a new list, else the first row", () => {
  const items = [item("a.pdf", "1"), item("b.pdf", "2")];
  expect(keepMention(items, "2")).toBe(1);
  expect(keepMention(items, "9")).toBe(0);
  expect(keepMention(items, null)).toBe(0);
});

test("a send mentions each token's paper once, in the order the tokens first stand; unknown tokens mention nothing", () => {
  const a = item("a.pdf", "A");
  const b = item("b c.pdf", "B");
  const known = new Map([
    ["@a.pdf", a],
    ['@"b c.pdf"', b],
  ]);
  const tokens = [
    { text: '@"b c.pdf"' },
    { text: "/review" },
    { text: "@a.pdf" },
    { text: '@"b c.pdf"' },
  ];
  expect(mentionedIn(tokens, known)).toEqual([b, a]);
  expect(mentionedIn([], known)).toEqual([]);
});
