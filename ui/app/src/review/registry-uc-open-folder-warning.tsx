// UC's entries for the first screen's step for a folder New matter would warn about (UC-283 to UC-285): Open the folder
// for a matter pressed, a folder picked on a network drive or a drive without file permissions, the host's FolderSupport
// reading it; the step says New matter's warning's words (FR-07, FR-08, FR-13, FR-14) with Choose another folder
// (FR-09) and Open anyway, the one new line. The real room over FakeTransport, on the first screen.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { FolderSupportKind } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { render } from "./registry-ud1-scene.tsx";
import { folderWords as w, type WordsRef } from "./words.ts";

/** Several lines of one id, joined with " / " as the approved page writes them. */
const joined = (parts: readonly WordsRef[]): WordsRef => ({
  file: [...new Set(parts.map((p) => p.file))].join(", "),
  key: parts.map((p) => p.key).join(" / "),
  text: (lang: Lang) => parts.map((p) => p.text(lang)).join(" / "),
  later: parts.some((p) => p.later),
});

/** The first screen, Open the folder pressed, a folder picked that the check reads as `kind`: the step shows. */
const stepped = (kind: FolderSupportKind) =>
  render({
    first: true,
    events: [],
    clicks: ['[data-testid="open-folder"]'],
    support: kind,
    open: {
      pickFolder: () => ({
        folder: "review-folder",
        display:
          kind === "network" ? "Volumes/fileserver/Rathi Steels" : "Volumes/USB/Rathi Steels",
      }),
      dispatch: () => new Promise(() => {}),
    },
  });

const DRIVE = w("Claude asks before every change. ChatGPT can't work here.");

export const UC_OPEN_FOLDER_WARNING: Entry[] = [
  {
    id: "UC-283",
    words: w("Open anyway"),
    screen: "first-launch/welcome",
    render: stepped("network"),
  },
  {
    id: "UC-284",
    words: joined([
      w("On a network drive"),
      DRIVE,
      w("Clauseo can't keep Claude and ChatGPT inside a folder on a network drive."),
      w("Choose another folder"),
    ]),
    screen: "first-launch/welcome",
    render: stepped("network"),
  },
  {
    id: "UC-285",
    words: joined([
      w("No file permissions"),
      DRIVE,
      w(
        "Clauseo can't keep Claude and ChatGPT inside a folder on this drive. USB drives often don't keep permissions.",
      ),
      w("Choose another folder"),
    ]),
    screen: "first-launch/welcome",
    render: stepped("no_permissions"),
  },
];
