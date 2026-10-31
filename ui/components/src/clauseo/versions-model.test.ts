import { expect, test } from "vitest";
import { anyChange, diffParagraphs, diffRows, diffWords, plain } from "./versions-diff.ts";
import {
  addedSince,
  asItStood,
  changedDocs,
  changedSince,
  differing,
  eraOf,
  FOLD_AT,
  foldedSpan,
  groupsOf,
  OPEN_DAYS,
  papersOf,
  rowVersion,
  sameHand,
  undoChange,
  whoOf,
  type Calendar,
  type Version,
} from "./versions-model.ts";
import {
  largeNote,
  restoredLine,
  restoreWords,
  sheetLabel,
  undidChangeLine,
  undoWords,
} from "./versions-lines.ts";
import {
  CAL,
  DATES_REVS,
  DRAFT_REVS,
  HISTORY,
  PARA,
  ROWS,
  TODAY,
  at,
  nameOf,
} from "./versions-fixture.ts";
import { VERSION_WORDS, edgeDialogWords, vt } from "../lib/words-versions.ts";

const now = HISTORY[HISTORY.length - 1];
if (now === undefined) throw new Error("no history");
const stamp = { day: TODAY, at: "5:12 pm" };
const rev = (v: Version | undefined, d: string) => v?.state.docs[d];

test("the history runs across two conversations and several days, each version saying who, when and what", () => {
  const who = HISTORY.map((v) => whoOf("en", CAL, v, HISTORY));
  expect(who).toContain("Clauseo, in Draft");
  expect(who).toContain("Clauseo, in Dates");
  expect(who).toContain("You, outside Clauseo");
  expect(who).toContain("Undo of the change at 4:05 pm on 9 March");
  expect(who).toContain("Imported 4 documents");
  expect(new Set(HISTORY.map((v) => v.day)).size).toBe(12);
  expect(papersOf("en", at("v8"), nameOf)).toBe("List of dates and Draft reply");
  // content stays as given; the list's own word is the language's
  expect(papersOf("hi", at("v7"), nameOf)).toBe("Letter from the bank और Junior's note");
  // an import names no papers of its own
  expect(papersOf("en", at("v1"), nameOf)).toBe("");
});

test("the plain line names the time in both languages, and says today for today", () => {
  expect(asItStood("en", CAL, at("v4"))).toBe("The matter as it stood at 3:42 pm on 9 March");
  expect(asItStood("hi", CAL, at("v4"))).toBe("मामला जैसा 9 March को दोपहर 3:42 बजे था");
  expect(asItStood("en", CAL, at("v8"))).toBe("The matter as it stood at 4:31 pm today");
  // the same version read on a later day is no longer today
  const later: Calendar = { ...CAL, today: "2027-03-30" };
  expect(asItStood("en", later, at("v8"))).toBe("The matter as it stood at 4:31 pm on 29 March");
});

test("restoring 3:42 pm on 9 March would change three documents and keep what was added since", () => {
  const v4 = at("v4");
  expect(differing(v4.state, now.state)).toEqual(["draft", "dates", "memo"]);
  expect(addedSince(v4.state, now.state)).toEqual(["letter"]);
  // a restore the host made reads as one, and one to a version no longer there reads as nothing
  const restored: Version = { ...now, id: "r1", who: { kind: "restore", to: "v4" } };
  expect(whoOf("en", CAL, restored, [...HISTORY, restored])).toBe("Restore to 3:42 pm on 9 March");
});

test("a restore names the version it restored by id, and one whose version is missing still says what it is", () => {
  const a: Version = { ...at("v4"), id: "a", at: "3:42 pm" };
  const b: Version = { ...at("v4"), id: "b", at: "3:42 pm", day: "2027-03-10" };
  const restore: Version = { ...now, id: "r", who: { kind: "restore", to: "b" } };
  const all = [a, b, restore];
  expect(whoOf("en", CAL, restore, all)).toBe("Restore to 3:42 pm on 10 March");
  expect(whoOf("en", CAL, { ...restore, who: { kind: "restore", to: "a" } }, all)).toBe(
    "Restore to 3:42 pm on 9 March",
  );
  const gone: Version = { ...now, id: "g", who: { kind: "restore", to: "nope" } };
  expect(whoOf("en", CAL, gone, HISTORY)).toBe("Restore of a version");
  expect(whoOf("hi", CAL, gone, HISTORY)).toBe("संस्करण पुनर्स्थापित किया गया");
});

test("an undo of a restore names the restore by id, with its time and day read at render", () => {
  const restore: Version = {
    ...at("v4"),
    id: "r",
    at: "3:42 pm",
    who: { kind: "restore", to: "v4" },
  };
  const other: Version = { ...at("v4"), id: "o", at: "3:42 pm", day: "2027-03-10" };
  const undo: Version = { ...now, id: "u", who: { kind: "undoRestore", of: "r" } };
  const all = [restore, other, undo];
  expect(whoOf("en", CAL, undo, all)).toBe("Undo of the restore at 3:42 pm on 9 March");
  expect(whoOf("en", CAL, { ...undo, who: { kind: "undoRestore", of: "o" } }, all)).toBe(
    "Undo of the restore at 3:42 pm on 10 March",
  );
  // undoing a restore made the same day names only its time
  expect(whoOf("en", CAL, { ...undo, day: restore.day }, all)).toBe(
    "Undo of the restore at 3:42 pm",
  );
  expect(whoOf("hi", CAL, undo, all)).toBe("9 March को दोपहर 3:42 बजे का पुनर्स्थापन पूर्ववत किया गया");
});

test("an undo of a restore whose version isn't in the history still says what it is", () => {
  const undo: Version = { ...now, id: "u", who: { kind: "undoRestore", of: "gone" } };
  expect(whoOf("en", CAL, undo, HISTORY)).toBe("Undo of a restore");
  expect(whoOf("hi", CAL, undo, HISTORY)).toBe("पुनर्स्थापन पूर्ववत किया गया");
});

test("an undo whose version isn't in the history still says what it is", () => {
  const undo: Version = { ...now, id: "u", who: { kind: "undo", of: "gone" } };
  expect(whoOf("en", CAL, undo, HISTORY)).toBe("Undo of a change");
  expect(whoOf("hi", CAL, undo, HISTORY)).toBe("बदलाव पूर्ववत किया गया");
});

test("an undo names the version it undid by id, so two versions made in the same minute stay apart", () => {
  const a: Version = { ...at("v4"), id: "a", at: "3:42 pm" };
  const b: Version = { ...at("v4"), id: "b", at: "3:42 pm", day: "2027-03-10" };
  const undo: Version = { ...now, id: "u", who: { kind: "undo", of: "b" } };
  const all = [a, b, undo];
  expect(whoOf("en", CAL, undo, all)).toBe("Undo of the change at 3:42 pm on 10 March");
  expect(whoOf("en", CAL, { ...undo, who: { kind: "undo", of: "a" } }, all)).toBe(
    "Undo of the change at 3:42 pm on 9 March",
  );
  // undoing a version made the same day names only its time
  expect(whoOf("en", CAL, { ...undo, day: "2027-03-10" }, all)).toBe(
    "Undo of the change at 3:42 pm",
  );
});

test("the restore dialog's words name the time, every document and what stays", () => {
  const r = restoreWords("en", CAL, HISTORY, "v4", nameOf);
  expect(r?.title).toBe("Restore the matter to how it was at 3:42 pm on 9 March?");
  expect(r?.body).toBe(
    "3 documents will change: Draft reply, List of dates and Junior's note. You can undo this.",
  );
  expect(r?.stays).toBe("'Letter from the bank', added since, stays.");
  expect(restoreWords("en", CAL, HISTORY, now.id, nameOf)?.body).toBe(
    "Nothing will change. The matter already stands as it did then.",
  );
});

test("the lines after a restore or an undo, over a sheet, and on a file too large", () => {
  expect(restoredLine("en", CAL, at("v4"), 3)).toBe(
    "Restored to 3:42 pm on 9 March. 3 documents changed.",
  );
  expect(restoredLine("en", CAL, at("v8"), 1)).toBe(
    "Restored to 4:31 pm today. 1 document changed.",
  );
  expect(undidChangeLine("en", "4:31 pm", ["draft", "dates"], nameOf)).toBe(
    "Undid the change at 4:31 pm. Draft reply and List of dates went back, and the undo is a version of its own.",
  );
  expect(sheetLabel("en", CAL, "Draft reply", at("v8"))).toBe(
    "Draft reply, as it stood at 4:31 pm today",
  );
  expect(sheetLabel("en", CAL, "Draft reply", null)).toBe("Draft reply, as it stands now");
  const large = HISTORY.find((v) => v.large !== undefined);
  expect(large && largeNote("en", large, "512 MB")).toMatch(
    /^Bank statements, scanned\.pdf is 740 MB, too large to keep versions\. Versions keep files up to 512 MB\./,
  );
  expect(largeNote("en", at("v4"), "512 MB")).toBeNull();
});

test("the wall at a version: papers not there yet, and papers it changed", () => {
  expect(eraOf("letter", at("v4"))).toBe("gone");
  expect(eraOf("draft", at("v3"))).toBe("gone");
  expect(eraOf("draft", at("v4"))).toBe("touched");
  expect(eraOf("memo", at("v7"))).toBe("touched");
  expect(eraOf("plaint", at("v7"))).toBe("there");
  // an import's papers are there, not changed by it
  expect(eraOf("plaint", at("v1"))).toBe("there");
  expect(changedDocs(at("v7"))).toBe(true);
  expect(changedDocs(at("v2"))).toBe(false);
});

test("the blackline: words in, words out, the lawyer's own at their weight", () => {
  const runs = diffWords(PARA.p4b, PARA.p4c);
  expect(runs.filter((r) => r.kind === "del")).toEqual([]);
  const ins = runs.filter((r) => r.kind === "ins");
  expect(ins.map((r) => r.text.trim())).toEqual([", at Annexures P-2 and P-3"]);
  expect(ins.every((r) => r.mine)).toBe(true);
  expect(plain(runs.map((r) => r.text).join(""))).toBe(plain(PARA.p4c));
  const back = diffParagraphs(DRAFT_REVS["P1"] ?? [], DRAFT_REVS["P4"] ?? []);
  expect(back.map((d) => d.kind)).toEqual([
    "same",
    "changed",
    "changed",
    "changed",
    "added",
    "changed",
  ]);
  expect(anyChange(diffParagraphs(DRAFT_REVS["P4"] ?? [], DRAFT_REVS["P4"] ?? []))).toBe(false);
  // a draft not made yet: every paragraph comes in
  expect(diffParagraphs([], DRAFT_REVS["P1"] ?? []).every((d) => d.kind === "added")).toBe(true);
  // Hindi compares word by word too, and keeps its danda apart
  const hi = diffWords(PARA.hi6b, PARA.hi6c);
  expect(hi.filter((r) => r.kind === "ins").map((r) => r.text.trim())).toEqual([
    "और 2 मार्च 2027 का आदेश",
  ]);
  const dates = diffRows(ROWS, DATES_REVS["D1"] ?? {}, DATES_REVS["D3"] ?? {});
  expect(dates.map((d) => d.kind)).toEqual([
    "added",
    "added",
    "same",
    "changed",
    "same",
    "same",
    "same",
  ]);
});

test("undoing one change asks about each paper changed again since, and moves forward", () => {
  // Clauseo's change at 4:31 pm today touched the list of dates and the draft; the lawyer changed the draft at 4:58
  const since = changedSince(HISTORY, "v8");
  expect(since.map((s) => s.id)).toEqual(["draft"]);
  expect(whoOf("en", CAL, since[0]?.by ?? now, HISTORY)).toBe("You, outside Clauseo");
  const keep = undoChange(HISTORY, "v8", ["draft"], stamp);
  expect(keep?.changed).toEqual(["dates"]);
  expect(rev(keep?.version, "draft")).toBe("P4");
  const anyway = undoChange(HISTORY, "v8", [], stamp);
  expect(anyway?.changed).toEqual(["draft", "dates"]);
  expect(rev(anyway?.version, "dates")).toBe("D1");
  expect(anyway?.version.who).toEqual({ kind: "undo", of: "v8" });
  expect(whoOf("en", CAL, anyway?.version ?? now, [...HISTORY, anyway?.version ?? now])).toBe(
    "Undo of the change at 4:31 pm",
  );
  expect(undoChange(HISTORY, "nope", [], stamp)).toBeNull();
  expect(changedSince(HISTORY, "nope")).toEqual([]);
  // the dialog's words
  const kept = undoWords("en", CAL, HISTORY, "v8", ["draft"], nameOf);
  expect(kept?.title).toBe("Undo the change Clauseo made at 4:31 pm today?");
  expect(kept?.body).toBe(
    "1 document goes back to how it was just before it: List of dates. The undo is kept as a version of its own.",
  );
  expect(kept?.editedHead).toBe("1 document changed again after it.");
  expect(kept?.edited).toEqual([
    { id: "draft", title: "Draft reply", at: "Changed by you, outside Clauseo, at 4:58 pm" },
  ]);
  expect(undoWords("en", CAL, HISTORY, "v8", [], nameOf)?.body).toMatch(/^2 documents go back/);
});

test("the timeline groups by day, newest first, with four days open and the rest under Earlier", () => {
  const groups = groupsOf(HISTORY, "all", "en", CAL);
  expect(groups.length).toBe(12);
  expect(groups.map((g) => g.head).slice(0, 4)).toEqual([
    "Today",
    "Sunday 28 March",
    "Saturday 27 March",
    "Friday 26 March",
  ]);
  expect(groups.reduce((n, g) => n + g.count, 0)).toBe(HISTORY.length);
  expect(foldedSpan("en", CAL, groups)).toBe("1 to 20 March");
  // across months, the first day names its month too
  const feb = HISTORY.map((v, i) => (i === 0 ? { ...v, day: "2027-02-26" } : v));
  expect(foldedSpan("en", CAL, groupsOf(feb, "all", "en", CAL))).toBe("26 February to 20 March");
  // four days or fewer fold nothing
  expect(foldedSpan("en", CAL, groups.slice(0, 4))).toBe("");
  expect(groupsOf([], "all", "en", CAL)).toEqual([]);
});

test("a run of 3 or more by one hand on one day folds behind its latest; now, and papers made or added, stand alone", () => {
  const [today, d28, d27, d26] = groupsOf(HISTORY, "all", "en", CAL);
  // now stands alone, then Clauseo's 4:31 pm with the 9 before it
  expect(today?.rows.map((r) => r.kind)).toEqual(["one", "run"]);
  expect(today?.rows[0] && rowVersion(today.rows[0]).id).toBe("v9");
  const run = today?.rows[1];
  expect(run?.kind === "run" && run.head.v.id).toBe("v8");
  expect(run?.kind === "run" && run.rest.length).toBe(9);
  // a file too large stands on its own line
  expect(d28?.rows.map((r) => r.kind)).toEqual(["run", "one"]);
  // two by the lawyer are too few to fold
  expect(d27?.rows.map((r) => r.kind)).toEqual(["run", "one", "one"]);
  expect(d26?.rows.map((r) => r.kind)).toEqual(["run"]);
  // the same hand on another day, or another conversation, is another run
  expect(sameHand(at("v8"), at("v4"))).toBe(false);
  expect(sameHand(at("v3"), at("v4"))).toBe(false);
  expect(sameHand(at("v4"), at("v5"))).toBe(true);
});

/** A bare version by the lawyer, outside Clauseo, on a day of March 2027. */
const bare = (id: string, day: string, time: string): Version => ({
  ...at("v9"),
  id,
  day: `2027-03-${day}`,
  at: time,
  who: { kind: "outside" },
  touched: ["draft"],
  added: [],
  made: [],
});
const kinds = (h: readonly Version[], day: string) =>
  groupsOf(h, "all", "en", CAL)
    .find((g) => g.key === `2027-03-${day}`)
    ?.rows.map((r) => r.kind);

test("a run folds at exactly FOLD_AT versions by one hand on one day, and not one fewer", () => {
  expect(FOLD_AT).toBe(3);
  const now = bare("now", "29", "5:00 pm");
  const three = [
    bare("a", "20", "10:00 am"),
    bare("b", "20", "10:05 am"),
    bare("c", "20", "10:10 am"),
  ];
  expect(kinds([...three, now], "20")).toEqual(["run"]);
  expect(kinds([...three.slice(1), now], "20")).toEqual(["one", "one"]);
});

test("exactly OPEN_DAYS days show no Earlier, and one day more folds under it", () => {
  expect(OPEN_DAYS).toBe(4);
  const days = ["21", "22", "23", "24", "25"].map((d) => bare(`d${d}`, d, "10:00 am"));
  const four = groupsOf(days.slice(1), "all", "en", CAL);
  expect(four.length).toBe(4);
  expect(four.slice(OPEN_DAYS)).toEqual([]);
  expect(foldedSpan("en", CAL, four)).toBe("");
  const five = groupsOf(days, "all", "en", CAL);
  expect(five.slice(OPEN_DAYS).map((g) => g.key)).toEqual(["2027-03-21"]);
  expect(foldedSpan("en", CAL, five)).toBe("21 to 21 March");
});

test("the switch shows only Clauseo's versions or only the lawyer's", () => {
  const mine = groupsOf(HISTORY, "you", "en", CAL).flatMap((g) => g.rows.map(rowVersion));
  expect(mine.length).toBeGreaterThan(0);
  expect(mine.every((v) => v.who.kind !== "clauseo")).toBe(true);
  const clauseo = groupsOf(HISTORY, "clauseo", "en", CAL).flatMap((g) => g.rows.map(rowVersion));
  expect(clauseo.every((v) => v.who.kind === "clauseo")).toBe(true);
});

test("the computer's dialog puts the risk both ways, and keeping protection is its default", () => {
  const folder = edgeDialogWords("en", { kind: "folder", name: "Precedents" });
  expect(folder.title).toBe("Allow Clauseo to read the folder 'Precedents'?");
  expect(folder.lines[1]).toBe(
    "If you don't, Clauseo keeps to this matter's folder and answers without Precedents.",
  );
  expect(folder.primary).toBe("confirm");
  const off = edgeDialogWords("en", { kind: "off", matter: "Acme Ltd v. State" });
  expect(off.title).toBe("Switch off protection for Acme Ltd v. State?");
  expect(off.primary).toBe("cancel");
  expect(off.remember).toBeNull();
});

const BANNED =
  /\b(git|commit|branch|hash|token|session|terminal|CLI|adapter|thread|pencil|ink|step|I|I'm|safe)\b|[\u2013\u2014]|Opus|Sonnet|GPT-\d/i;
const BANNED_HI = /पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/;

test("no string of Versions says I, names a model, uses a dash, or a banned word, in either language", () => {
  for (const [en, hi] of Object.entries(VERSION_WORDS)) {
    expect(en.replace("Safe workspace", ""), en).not.toMatch(BANNED);
    expect(hi, en).not.toMatch(BANNED_HI);
    expect(hi, en).not.toMatch(/[\u0900-\u097F]\.$|[\u0966-\u096F]|[\u2013\u2014]/);
    if (en.endsWith(".")) expect(hi, en).toMatch(/।$/);
    if (en.endsWith("?")) expect(hi, en).toMatch(/\?$/);
  }
  // the screen catches what it should
  expect("Restore the commit").toMatch(BANNED);
  expect(vt("hi", "Try again")).not.toMatch(BANNED_HI);
});
