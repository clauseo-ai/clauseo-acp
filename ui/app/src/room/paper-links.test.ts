// Which paper on the wall a file link in an answer names (paper-links.ts): every form an agent writes it in, the
// matter's papers only, and plain text (undefined) for a file anywhere else.
import { describe, expect, test } from "vitest";
import { paperLinkOf } from "./paper-links.ts";

const REPLY = "Reply to legal notice.docx";
const WALL = [
  REPLY,
  "drafts/Notice.docx",
  "a/Brief.docx",
  "b/Brief.docx",
  "evidence/Bank letter.pdf",
];
/** As the host shows a matter's folder below the home folder. */
const FOLDER = "Documents/Clauseo/Sharma v. State";
const HOME = "/Users/rohan";
const MATTER = `${HOME}/${FOLDER}`;

const at = (href: string, folder: string | null = FOLDER, wall: readonly string[] = WALL) =>
  paperLinkOf(href, wall, folder);

describe("a link to a paper on the wall names it", () => {
  test.each([
    // the forms ChatGPT and Claude write: the matter's absolute path, raw and percent-encoded
    ["an absolute path with spaces", `${MATTER}/${REPLY}`],
    [
      "an absolute path with %20",
      `${MATTER}/Reply%20to%20legal%20notice.docx`.replaceAll(" ", "%20"),
    ],
    ["a file:// URL", `file://${MATTER.replaceAll(" ", "%20")}/Reply%20to%20legal%20notice.docx`],
    ["a file://localhost URL", `file://localhost${MATTER}/${REPLY}`],
    ["ChatGPT's sandbox path", `sandbox:/mnt/data/${REPLY}`],
    ["a sandbox path, percent-encoded", "sandbox:/mnt/data/Reply%20to%20legal%20notice.docx"],
    ["a sandbox: name", `sandbox:${REPLY}`],
    ["a bare name", REPLY],
    ["a ./ name", `./${REPLY}`],
    ["a bare name, percent-encoded, with a fragment", "Reply%20to%20legal%20notice.docx#page=2"],
    ["the tilde form", `~/${FOLDER}/${REPLY}`],
    ["a path through .. that stays inside", `${MATTER}/drafts/../${REPLY}`],
  ])("%s", (_, href) => {
    expect(at(href)).toBe(REPLY);
  });

  test("a relative path from the matter's folder, and an absolute one into a subfolder", () => {
    expect(at("drafts/Notice.docx")).toBe("drafts/Notice.docx");
    expect(at("./drafts/Notice.docx")).toBe("drafts/Notice.docx");
    expect(at(`${MATTER}/evidence/Bank%20letter.pdf`)).toBe("evidence/Bank letter.pdf");
  });

  test("a Windows path, with backslashes and another case, inside the matter", () => {
    const wall = ["Drafts/Reply.docx"];
    const folder = "Documents\\Clauseo\\Sharma";
    expect(
      at("C:\\Users\\Rohan\\Documents\\Clauseo\\Sharma\\Drafts\\Reply.docx", folder, wall),
    ).toBe("Drafts/Reply.docx");
    expect(
      at("c:\\users\\rohan\\documents\\clauseo\\SHARMA\\drafts\\reply.DOCX", folder, wall),
    ).toBe("Drafts/Reply.docx");
    expect(
      at("file:///C:/Users/Rohan/Documents/Clauseo/Sharma/Drafts/Reply.docx", folder, wall),
    ).toBe("Drafts/Reply.docx");
  });

  test("a matter outside the home folder, shown whole", () => {
    expect(at(`/Volumes/Cases/Sharma/${REPLY}`, "/Volumes/Cases/Sharma")).toBe(REPLY);
    expect(
      at(`/Users/rohan/Volumes/Cases/Sharma/${REPLY}`, "/Volumes/Cases/Sharma"),
    ).toBeUndefined();
    expect(at("D:\\Cases\\Sharma\\drafts\\notice.docx", "D:\\Cases\\Sharma")).toBe(
      "drafts/Notice.docx",
    );
  });
});

describe("a link to a file anywhere else names nothing", () => {
  test.each([
    [
      "Clauseo's run folder",
      `${HOME}/Library/Application Support/ai.clauseo.desktop/agents/codex/home/${REPLY}`,
    ],
    [
      "Clauseo's work folder, file://",
      `file://${HOME}/Library/Caches/ai.clauseo.desktop/run/${REPLY}`.replaceAll(" ", "%20"),
    ],
    ["a temp folder", `/private/var/folders/4m/T/clauseo/${REPLY}`],
    ["/etc/passwd", "/etc/passwd"],
    ["file:///etc/passwd", "file:///etc/passwd"],
    ["../outside.docx", "../outside.docx"],
    ["a .. that climbs out", `drafts/../../${REPLY}`],
    ["an absolute path that climbs out", `${MATTER}/../${REPLY}`],
    ["a name two papers share, in two subfolders", "Brief.docx"],
    ["a sandbox name two papers share", "sandbox:/mnt/data/Brief.docx"],
    ["a paper not on the wall", "Reply notes.md"],
    ["a sandbox paper not on the wall", "sandbox:/mnt/data/Reply notes.md"],
    ["a relative path to a folder the paper isn't in", `old/${REPLY}`],
    ["the matter's folder under a deeper home", `/Users/rohan/x/${FOLDER}/${REPLY}`],
    ["another place with the same tail", `/tmp/${FOLDER}/${REPLY}`],
    ["a file host", `file://server/share/${REPLY}`],
    ["another scheme", "javascript:alert(1)"],
    ["nothing", ""],
  ])("%s", (_, href) => {
    expect(at(href)).toBeUndefined();
  });

  test("an absolute path is never matched by its name alone, and no folder means no absolute match", () => {
    expect(at(`/Users/rohan/Downloads/${REPLY}`)).toBeUndefined();
    expect(at(`${MATTER}/${REPLY}`, null)).toBeUndefined();
    expect(at(`${MATTER}/${REPLY}`, "")).toBeUndefined();
    // a relative name still matches with no folder
    expect(at(REPLY, null)).toBe(REPLY);
  });

  test("an empty wall names nothing", () => {
    expect(at(REPLY, FOLDER, [])).toBeUndefined();
    expect(at(`${MATTER}/${REPLY}`, FOLDER, [])).toBeUndefined();
  });
});
