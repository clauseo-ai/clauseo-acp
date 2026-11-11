// The wall's dimmed tiles (wall-away.ts), in the real room over a scripted host: files only online and a folder
// Clauseo couldn't open, among the papers, and the new conversation's count of how many more are only online. On
// Windows in OneDrive with two files away and a folder not opened; on the Mac in OneDrive with one; in a folder synced
// by a service Clauseo doesn't name, with two; on this computer with one; a matter whose files are all in OneDrive; and
// one whose only entry is a folder not opened. A tile's tip shows on hover or focus; its words are its description for
// assistive technology, so the page marks them in the room as it stands. Spread into registry-u6.tsx.
import type { Entry } from "./entry.ts";
import type { Papers, PaperSummary } from "../room/host-next.ts";
import { MATTER, roomHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { matterWords, roomWords } from "./words.ts";
import { uuid } from "../testing/stream-script.ts";

const papers = (...paths: string[]): PaperSummary[] =>
  paths.map((path, i) => ({ file: uuid(i + 1, 8), path, size: 1000, modified: 1_730_000_000_000 }));
const only = (...paths: string[]): Papers["skipped"] =>
  paths.map((path) => ({ path, reason: "cloud_only" as const }));
const ONE_DRIVE: Papers["place"] = { synced: { service: "one_drive" } };

const room = (
  listed: PaperSummary[],
  listing: Partial<Omit<Papers, "papers">>,
  platform?: "mac" | "windows",
) => {
  const away = platform === undefined ? { listing } : { listing, platform };
  return () => <RoomOver host={() => roomHost([MATTER], listed, away)} />;
};

const STATES = {
  windows: room(
    papers("Plaint.pdf", "Reply.docx", "Written statement.pdf"),
    { place: ONE_DRIVE, skipped: only("Lease deed.pdf", "Vakalatnama.pdf"), unread: ["Annexures"] },
    "windows",
  ),
  mac: room(
    papers("Plaint.pdf", "Reply.docx"),
    { place: ONE_DRIVE, skipped: only("Lease deed.pdf") },
    "mac",
  ),
  other: room(
    papers("Plaint.pdf", "Reply.docx", "Written statement.pdf"),
    { place: { synced: { service: "other" } }, skipped: only("Lease deed.pdf", "Vakalatnama.pdf") },
    "windows",
  ),
  local: room(papers("Plaint.pdf"), { place: "local", skipped: only("Lease deed.pdf") }, "mac"),
  allOnline: room(
    [],
    { place: ONE_DRIVE, skipped: only("Lease deed.pdf", "Plaint.pdf", "Reply.docx") },
    "windows",
  ),
  onlyUnread: room([], { place: "local", unread: ["Annexures"] }, "windows"),
};

/** The dimmed tiles' entries, each id written as it is. */
export const WALL_AWAY: Entry[] = [
  {
    id: "WA-01",
    words: roomWords("Only in {service}"),
    screen: "room/wall-away",
    render: STATES.windows,
  },
  {
    id: "WA-02",
    words: roomWords(
      "Not on this computer, so Clauseo doesn't open it or save new versions of it.",
    ),
    screen: "room/wall-away",
    render: STATES.windows,
  },
  {
    id: "WA-03",
    words: roomWords(
      "To keep it here, right-click it in File Explorer and choose Always keep on this device.",
    ),
    screen: "room/wall-away",
    render: STATES.windows,
  },
  {
    id: "WA-04",
    words: roomWords("Not opened"),
    screen: "room/wall-away",
    render: STATES.windows,
  },
  {
    id: "WA-05",
    words: matterWords("{n} more are only in {service}."),
    screen: "room/wall-away",
    render: STATES.windows,
  },
  {
    id: "WA-06",
    words: roomWords(
      "To keep it here, Control-click it in Finder and choose Always Keep on This Device.",
    ),
    screen: "room/wall-away",
    render: STATES.mac,
  },
  {
    id: "WA-07",
    words: matterWords("1 more is only in {service}."),
    screen: "room/wall-away",
    render: STATES.mac,
  },
  {
    id: "WA-08",
    words: roomWords("Only online"),
    screen: "room/wall-away",
    render: STATES.other,
  },
  {
    id: "WA-09",
    words: roomWords("Download it to this computer to change that."),
    screen: "room/wall-away",
    render: STATES.other,
  },
  {
    id: "WA-10",
    words: matterWords("{n} more are only online."),
    screen: "room/wall-away",
    render: STATES.other,
  },
  {
    id: "WA-11",
    words: matterWords("1 more is only online."),
    screen: "room/wall-away",
    render: STATES.local,
  },
  {
    id: "WA-12",
    words: roomWords("Only in {service}"),
    screen: "room/wall-away",
    render: STATES.allOnline,
  },
  {
    id: "WA-13",
    words: roomWords(
      "Clauseo couldn't look inside this folder. It may be only online, or Clauseo may not be allowed to open it.",
    ),
    screen: "room/wall-away",
    render: STATES.onlyUnread,
  },
];
