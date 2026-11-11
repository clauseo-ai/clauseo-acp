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
import { awayWords, cloudOnlyCount, moreWords, wallEmpty, wallItems } from "./wall-away.ts";

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
  "Not on this computer, so Clauseo doesn't open it or save new versions of it.",
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

test("its tip says why, then how to keep it: OneDrive on Windows, OneDrive on the Mac, else download it", () => {
  expect(awayWords("en", "windows", ONE_DRIVE, false).tip).toBe(
    `${FIRST} To keep it here, right-click it in File Explorer and choose Always keep on this device.`,
  );
  expect(awayWords("en", "mac", ONE_DRIVE, false).tip).toBe(
    `${FIRST} To keep it here, Control-click it in Finder and choose Always Keep on This Device.`,
  );
  const elsewhere = `${FIRST} Download it to this computer to change that.`;
  expect(awayWords("en", "linux", ONE_DRIVE, false).tip).toBe(elsewhere);
  expect(awayWords("en", "windows", DROPBOX, false).tip).toBe(elsewhere);
  expect(awayWords("en", "mac", "local", false).tip).toBe(elsewhere);
  // the same first sentence in Hindi, the OS's menu names in English
  expect(awayWords("hi", "windows", ONE_DRIVE, false).tip).toBe(
    "यह इस कंप्यूटर पर नहीं है, इसलिए Clauseo इसे नहीं खोलता और इसके नए संस्करण नहीं सहेजता। " +
      "इसे यहाँ रखने के लिए File Explorer में इस पर राइट-क्लिक करें और Always keep on this device चुनें।",
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

test("the count's sentence, in its four forms, and none at 0", () => {
  expect(moreWords("en", ONE_DRIVE, 2)).toBe("2 more are only in OneDrive.");
  expect(moreWords("en", ONE_DRIVE, 1)).toBe("1 more is only in OneDrive.");
  expect(moreWords("en", "local", 2)).toBe("2 more are only online.");
  expect(moreWords("en", "network", 1)).toBe("1 more is only online.");
  expect(moreWords("en", ONE_DRIVE, 1200)).toBe("1,200 more are only in OneDrive.");
  expect(moreWords("en", ONE_DRIVE, 0)).toBeNull();
  expect(moreWords("hi", ONE_DRIVE, 2)).toBe(
    mt("hi", "{n} more are only in {service}.", { n: 2, service: "OneDrive" }),
  );
  expect(moreWords("hi", "local", 1)).toBe("1 और दस्तावेज़ केवल ऑनलाइन है।");
});
