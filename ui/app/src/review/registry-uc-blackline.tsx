// UC's copy review entries for a Word document's marked-up changes in Versions (UC-113 to UC-116), apart from
// registry-uc.tsx to keep that file under the size limit: the notice compared with now, over registry-u8.tsx's host,
// whose word_blackline runs, fails or cuts the document short as each entry asks. UC_ALL is registry-uc.tsx's UC
// entries, then registry-uc-blocks.tsx's UC_BLOCKS, then these, so registry.tsx lists UC in one place.
import { compared } from "./registry-u8.tsx";
import type { Entry } from "./entry.ts";
import { UC_BLOCKS } from "./registry-uc-blocks.tsx";
import { UC } from "./registry-uc.tsx";
import { versionWords } from "./words.ts";

const NOTICE = "Notice to the bank.docx";
const COMPARED = {
  compare: [
    {
      path: NOTICE,
      change: "modified" as const,
      size_bytes: 1200,
      may_include_lawyer_edits: false,
    },
  ],
};

export const UC_BLACKLINE: Entry[] = [
  compared("UC-113", versionWords("Marking up the changes…"), { ...COMPARED, mark: "hold" }),
  compared(
    "UC-114",
    versionWords("Clauseo's Word tool is missing. Reinstall Clauseo to see the marked-up changes."),
    { ...COMPARED, mark: { refused: "blackline.no_word_tool", retryable: false } },
  ),
  compared("UC-115", versionWords("The changes in this document couldn't be marked up."), {
    ...COMPARED,
    mark: { refused: "blackline.failed", retryable: true },
  }),
  compared("UC-116", versionWords("This document is very long, so only part of it is shown."), {
    ...COMPARED,
    mark: { truncated: true },
  }),
];

/**
 * UC's entries: registry-uc.tsx's (UC-01 to UC-78), registry-uc-blocks.tsx's (UC-80 to UC-106), then
 * the Word blackline's (UC-113 to UC-116).
 */
export const UC_ALL: Entry[] = [...UC, ...UC_BLOCKS, ...UC_BLACKLINE];
