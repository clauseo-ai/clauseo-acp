import { describe, expect, test } from "vitest";
import { approve, layout, parseArgs, parseIds, summaryText, today } from "./copy-approve.mjs";

const DATE = "2026-11-16";
const BY = `Rohan (review page, ${DATE})`;

const files = () => ({
  "U1.json": layout({
    "U1-01": { en: "Open", hi: "खोलें", status: "approved", at: "2026-11-02", by: "Rohan" },
    "U1-02": {
      en: "Close",
      hi: "बंद करें",
      hi_status: "provisional",
      hi_at: "2026-11-08",
      hi_by: "app track",
      status: "provisional",
      at: "2026-11-08",
      by: "app track (provisional; Rohan batch review)",
      note: "kept as written",
    },
  }),
  "UC.json": layout({
    "UC-10": {
      en: "Later",
      hi: null,
      hi_status: "later",
      status: "provisional",
      at: "2026-11-09",
      by: "app track",
    },
    "UC-11": { en: "Again", hi: null, hi_status: "later", status: "change", at: "2026-11-09" },
  }),
});

describe("copy:approve", () => {
  test("--all approves every entry not yet approved, and changes only status, at and by", () => {
    const before = files();
    const r = approve(before, { date: DATE });
    expect(r.approved).toEqual([
      ["U1.json", "U1-02"],
      ["UC.json", "UC-10"],
      ["UC.json", "UC-11"],
    ]);
    const u1 = JSON.parse(r.texts["U1.json"]);
    expect(u1["U1-01"]).toEqual(JSON.parse(before["U1.json"])["U1-01"]);
    expect(u1["U1-02"]).toEqual({
      ...JSON.parse(before["U1.json"])["U1-02"],
      status: "approved",
      at: DATE,
      by: BY,
    });
    // the keys keep their order, the Hindi and the note untouched
    expect(Object.keys(u1["U1-02"])).toEqual(Object.keys(JSON.parse(before["U1.json"])["U1-02"]));
    // a change entry with no by gains it last
    expect(Object.keys(JSON.parse(r.texts["UC.json"])["UC-11"]).at(-1)).toBe("by");
  });

  test("the diff is the status, at and by lines alone", () => {
    const before = files();
    const after = approve(before, { date: DATE }).texts["U1.json"];
    const a = before["U1.json"].split("\n");
    const b = after.split("\n");
    expect(b.length).toBe(a.length);
    const differ = a.map((l, i) => [l, b[i]]).filter(([x, y]) => x !== y);
    expect(differ).toEqual([
      ['    "status": "provisional",', '    "status": "approved",'],
      ['    "at": "2026-11-08",', `    "at": "${DATE}",`],
      ['    "by": "app track (provisional; Rohan batch review)",', `    "by": "${BY}",`],
    ]);
  });

  test("--except leaves its ids as they are; with --change they become change", () => {
    const kept = approve(files(), { except: ["UC-10"], date: DATE });
    expect(kept.kept).toEqual([["UC.json", "UC-10"]]);
    expect(JSON.parse(kept.texts["UC.json"])["UC-10"]).toEqual(
      JSON.parse(files()["UC.json"])["UC-10"],
    );
    const changed = approve(files(), { except: ["UC-10", "U1-02"], change: true, date: DATE });
    expect(changed.changed.map(([, id]) => id)).toEqual(["U1-02", "UC-10"]);
    expect(JSON.parse(changed.texts["UC.json"])["UC-10"]).toMatchObject({
      status: "change",
      at: DATE,
      by: BY,
    });
    expect(summaryText(changed, "dir")).toContain(
      '1 approved, 2 set to "change", 0 left as they were, in 2 files',
    );
  });

  test("an --except id not waiting for approval, or a file out of layout, is refused before anything changes", () => {
    expect(() => approve(files(), { except: ["U1-01"], date: DATE })).toThrow(/U1-01/);
    expect(() => approve(files(), { except: ["U9-99"], date: DATE })).toThrow(/U9-99/);
    expect(() => approve({ "A.json": '{"A-01":{"en":"a"}}' }, { date: DATE })).toThrow(/A\.json/);
    expect(() => approve(files(), { date: "16 Nov" })).toThrow(/YYYY-MM-DD/);
  });

  test("the command line wants --all, and --change only with --except", () => {
    expect(parseArgs(["--all", "--except", "A-01, B-02", "--change", "--date", DATE], "d")).toEqual(
      {
        all: true,
        only: null,
        except: ["A-01", "B-02"],
        change: true,
        dir: "d",
        date: DATE,
      },
    );
    expect(() => parseArgs([], "d")).toThrow(/--all/);
    expect(parseArgs(["--only", "ids.txt"], "d")).toMatchObject({ all: false, only: "ids.txt" });
    expect(() => parseArgs(["--all", "--only", "ids.txt"], "d")).toThrow(/not both/);
    expect(() => parseArgs(["--all", "--change"], "d")).toThrow(/--except/);
    expect(() => parseArgs(["--all", "--except"], "d")).toThrow(/needs a value/);
    expect(() => parseArgs(["--all", "--yes"], "d")).toThrow(/unknown/);
    expect(today(new Date(2026, 10, 5, 23, 30))).toBe("2026-11-05");
  });

  describe("--only", () => {
    const text = (a) => JSON.parse(a);
    const list = (...lines) =>
      parseIds(`# main 0123456789 2026-11-16 09:00 UTC\n${lines.join("\n")}\n`);

    test("reads ids, with or without their English, and skips comments and blank lines", () => {
      expect(list("U1-02", "", "  # a comment", "UC-10 # trailing", 'UC-11\t"Again"')).toEqual([
        { id: "U1-02", en: null },
        { id: "UC-10", en: null },
        { id: "UC-11", en: "Again" },
      ]);
      expect(() => list("UC-11\tAgain")).toThrow(/line 2/);
      expect(() => list("UC 11")).toThrow(/line 2/);
    });

    test("approves only the listed ids; one added after the list was made stays provisional", () => {
      // UC-10 was added after the page was built, so the list names only U1-02
      const r = approve(files(), { only: list('U1-02\t"Close"'), date: DATE });
      expect(r.approved).toEqual([["U1.json", "U1-02"]]);
      expect(Object.keys(r.texts)).toEqual(["U1.json"]);
      expect(text(r.texts["U1.json"])["U1-02"]).toMatchObject({
        status: "approved",
        at: DATE,
        by: BY,
      });
      expect(r.texts["UC.json"]).toBeUndefined();
    });

    test("an unknown id fails, naming it, and nothing is written", () => {
      expect(() => approve(files(), { only: list("U1-02", "U9-99"), date: DATE })).toThrow(
        /U9-99: no copy-approvals file has it/,
      );
    });

    test("an approved id with the listed words is already approved; with other words it fails", () => {
      const r = approve(files(), { only: list('U1-01\t"Open"', "U1-02"), date: DATE });
      expect(r.already).toEqual([["U1.json", "U1-01"]]);
      expect(r.approved).toEqual([["U1.json", "U1-02"]]);
      expect(summaryText(r, "dir")).toContain("already approved: U1-01");
      expect(() => approve(files(), { only: list('U1-01\t"Open it"'), date: DATE })).toThrow(
        /U1-01: the list has "Open it", but U1\.json has "Open" \(approved\)/,
      );
    });

    test("a waiting id whose words changed since the list was made fails", () => {
      expect(() => approve(files(), { only: list('U1-02\t"Close it"'), date: DATE })).toThrow(
        /U1-02/,
      );
    });

    test("combines with --except and --change, whose ids must be in the list", () => {
      const only = list("U1-02", "UC-10");
      const kept = approve(files(), { only, except: ["UC-10"], date: DATE });
      expect(kept.approved).toEqual([["U1.json", "U1-02"]]);
      expect(kept.kept).toEqual([["UC.json", "UC-10"]]);
      const changed = approve(files(), { only, except: ["UC-10"], change: true, date: DATE });
      expect(changed.changed).toEqual([["UC.json", "UC-10"]]);
      // UC-11 waits, but isn't listed
      expect(() => approve(files(), { only, except: ["UC-11"], date: DATE })).toThrow(
        /in the --only list/,
      );
    });
  });
});
