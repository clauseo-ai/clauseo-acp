// The wall's files Clauseo doesn't read, as words and order (wall-away.ts): a file only online and a folder not opened
// sit among the papers by path, a folder where "<folder>/" sorts, each with a DOM-safe id; a file over the size cap
// stays out. A file only online says the service that syncs the folder, or "online", and its tip how to keep it, by
// OneDrive on Windows or the Mac, else download it; a folder says it wasn't opened. The new conversation's count adds
// how many more are only online, folders never counted, and nothing at 0.
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import { expect, test } from "vitest";
import { uuid } from "../testing/stream-script.ts";
import type { FolderPlace } from "../transport/protocol.ts";
import type { Papers } from "./host-next.ts";
import {
  allAwayWords,
  awayWords,
  cloudOnlyCount,
  hasLine,
  moreWords,
  wallEmpty,
  wallItems,
} from "./wall-away.ts";

const paper = (n: number, path: string) => ({
  file: uuid(n, 8),
  path,
  size: 1000,
  modified: 1_730_000_000_000,
});
const listing = (over: Partial<Papers>): Papers => ({
  papers: [],
  skipped: [],
  unread: [],
  truncated: false,
  place: "local",
  ...over,
});
const ONE_DRIVE: FolderPlace = { synced: { service: "one_drive" } };
const DROPBOX: FolderPlace = { synced: { service: "dropbox" } };
const FIRST = rt(
  "en",
  "Not on this computer, so Clauseo keeps no versions of it yet. Reading it in a conversation downloads it.",
);

test("files only online and folders not opened sit among the papers by path, with ids of their own", () => {
  const papers = listing({
    papers: [paper(1, "Annexures-index.pdf"), paper(2, "Plaint.pdf"), paper(3, "Reply.docx")],
    skipped: [
      { path: "Lease deed.pdf", reason: "cloud_only" },
      { path: "Hearing.mp4", reason: "over_size_cap" },
      { path: "Vakalatnama.pdf", reason: "cloud_only" },
    ],
    // "Annexures/" sorts after "Annexures-index.pdf": '/' is after '-'
    unread: ["Annexures"],
  });
  const items = wallItems(papers);
  expect(items.map((i) => (i.kind === "paper" ? i.paper.path : `${i.id} ${i.path}`))).toEqual([
    "Annexures-index.pdf",
    "away-0 Annexures",
    "away-1 Lease deed.pdf",
    "Plaint.pdf",
    "Reply.docx",
    "away-2 Vakalatnama.pdf",
  ]);
  expect(items.filter((i) => i.kind === "away").map((i) => i.kind === "away" && i.folder)).toEqual([
    true,
    false,
    false,
  ]);
  expect(cloudOnlyCount(papers)).toBe(2);
});

test("the wall is empty only with no paper, no file only online and no folder not opened", () => {
  expect(wallEmpty(listing({}))).toBe(true);
  expect(wallEmpty(listing({ skipped: [{ path: "Hearing.mp4", reason: "over_size_cap" }] }))).toBe(
    true,
  );
  expect(wallEmpty(listing({ skipped: [{ path: "Lease.pdf", reason: "cloud_only" }] }))).toBe(
    false,
  );
  expect(wallEmpty(listing({ unread: ["Annexures"] }))).toBe(false);
});

test("a file only online names the service that syncs the folder, or says online", () => {
  expect(awayWords("en", "windows", ONE_DRIVE, false).line).toBe("Only in OneDrive");
  expect(awayWords("en", "mac", { synced: { service: "icloud" } }, false).line).toBe(
    "Only in iCloud Drive",
  );
  for (const place of ["local", "network", { synced: { service: "other" } }] as FolderPlace[])
    expect(awayWords("en", "mac", place, false).line).toBe("Only online");
  expect(awayWords("hi", "windows", ONE_DRIVE, false).line).toBe("केवल OneDrive में");
  expect(awayWords("hi", "mac", "local", false).line).toBe("केवल ऑनलाइन");
});

test("UC-915: a file only online's tip says Clauseo keeps no versions and that reading it downloads it, everywhere", () => {
  for (const [platform, place] of [
    ["linux", ONE_DRIVE],
    ["windows", DROPBOX],
    ["mac", "local"],
  ] as const)
    expect(awayWords("en", platform, place, false).tip).toBe(FIRST);
  // Hindi later: the English stays
  expect(awayWords("hi", "mac", "local", false).tip).toBe(FIRST);
});

test("on OneDrive the tip says what reading it does, then how to keep it here: on Windows, or on the Mac", () => {
  expect(awayWords("en", "windows", ONE_DRIVE, false).tip).toBe(
    `${FIRST} To keep it here, right-click it in File Explorer and choose Always keep on this device.`,
  );
  expect(awayWords("en", "mac", ONE_DRIVE, false).tip).toBe(
    `${FIRST} To keep it here, Control-click it in Finder and choose Always Keep on This Device.`,
  );
  // UC-915's Hindi is later, so its English stays; the OS's menu names stay in English too
  expect(awayWords("hi", "windows", ONE_DRIVE, false).tip).toBe(
    `${FIRST} इसे यहाँ रखने के लिए File Explorer में इस पर राइट-क्लिक करें और Always keep on this device चुनें।`,
  );
});

test("a folder not opened says so, wherever it lives", () => {
  for (const place of [ONE_DRIVE, "local"] as FolderPlace[])
    expect(awayWords("en", "windows", place, true)).toEqual({
      line: "Not opened",
      tip: "Clauseo couldn't look inside this folder. It may be only online, or Clauseo may not be allowed to open it.",
    });
  expect(awayWords("hi", "mac", "local", true).line).toBe("नहीं खोला गया");
});

test("UC-913: one more paper only in a named service says reading it downloads it", () => {
  expect(moreWords("en", ONE_DRIVE, 1)).toBe(
    "1 more is only in OneDrive. Reading it downloads it to this computer.",
  );
  expect(moreWords("en", { synced: { service: "icloud" } }, 1)).toBe(
    "1 more is only in iCloud Drive. Reading it downloads it to this computer.",
  );
  // Hindi later: the English until it comes
  expect(moreWords("hi", ONE_DRIVE, 1)).toBe(
    "1 more is only in OneDrive. Reading it downloads it to this computer.",
  );
});

test("UC-914: several papers only in a named service say reading them downloads them", () => {
  expect(moreWords("en", ONE_DRIVE, 2)).toBe(
    "2 more are only in OneDrive. Reading them downloads them to this computer.",
  );
  expect(moreWords("en", ONE_DRIVE, 1200)).toBe(
    "1,200 more are only in OneDrive. Reading them downloads them to this computer.",
  );
  expect(moreWords("hi", ONE_DRIVE, 2)).toBe(
    mt("hi", "{n} more are only in {service}. Reading them downloads them to this computer.", {
      n: 2,
      service: "OneDrive",
    }),
  );
});

test("the count with no named service keeps its fallback, and none at 0", () => {
  expect(moreWords("en", "local", 2)).toBe("2 more are only online.");
  expect(moreWords("en", "network", 1)).toBe("1 more is only online.");
  expect(moreWords("en", { synced: { service: "other" } }, 1)).toBe("1 more is only online.");
  expect(moreWords("en", ONE_DRIVE, 0)).toBeNull();
  expect(moreWords("en", "local", 0)).toBeNull();
  expect(moreWords("hi", "local", 1)).toBe("1 और दस्तावेज़ केवल ऑनलाइन है।");
});

test("with no paper to read, the new conversation's line says the papers are all only online, in its four forms", () => {
  expect(allAwayWords("en", ONE_DRIVE, 3, 0)).toBe(
    "A new conversation. This matter's 3 papers are only in OneDrive.",
  );
  expect(allAwayWords("en", ONE_DRIVE, 1, 0)).toBe(
    "A new conversation. This matter's one paper is only in OneDrive.",
  );
  expect(allAwayWords("en", "local", 2, 0)).toBe(
    "A new conversation. This matter's 2 papers are only online.",
  );
  expect(allAwayWords("en", { synced: { service: "other" } }, 1, 0)).toBe(
    "A new conversation. This matter's one paper is only online.",
  );
  // folders not opened are never counted, and files only online come first
  expect(allAwayWords("en", ONE_DRIVE, 2, 3)).toBe(
    "A new conversation. This matter's 2 papers are only in OneDrive.",
  );
  expect(allAwayWords("hi", ONE_DRIVE, 2, 0)).toBe(
    "एक नई बातचीत। इस मामले के 2 दस्तावेज़ केवल OneDrive में हैं।",
  );
  expect(allAwayWords("hi", "local", 1, 0)).toBe(
    "एक नई बातचीत। इस मामले का एक दस्तावेज़ केवल ऑनलाइन है।",
  );
});

test("with only folders not opened, the line says Clauseo couldn't open them, one or more", () => {
  expect(allAwayWords("en", ONE_DRIVE, 0, 1)).toBe(
    "A new conversation. Clauseo couldn't open this matter's folder.",
  );
  expect(allAwayWords("en", "local", 0, 2)).toBe(
    "A new conversation. Clauseo couldn't open this matter's folders.",
  );
  expect(allAwayWords("hi", "local", 0, 1)).toBe(
    "एक नई बातचीत। Clauseo इस मामले के फ़ोल्डर नहीं खोल सका।",
  );
  expect(allAwayWords("hi", "local", 0, 2)).toBe(
    "एक नई बातचीत। Clauseo इस मामले के फ़ोल्डर नहीं खोल सका।",
  );
});

test("a new conversation has a line whenever the wall has a tile", () => {
  expect(hasLine(listing({}))).toBe(false);
  expect(hasLine(listing({ skipped: [{ path: "Hearing.mp4", reason: "over_size_cap" }] }))).toBe(
    false,
  );
  expect(hasLine(listing({ papers: [paper(1, "Plaint.pdf")] }))).toBe(true);
  expect(hasLine(listing({ skipped: [{ path: "Lease.pdf", reason: "cloud_only" }] }))).toBe(true);
  expect(hasLine(listing({ unread: ["Annexures"] }))).toBe(true);
});
