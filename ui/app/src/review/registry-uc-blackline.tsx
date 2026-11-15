// UC's copy review entries for a Word document's marked-up changes in Versions (UC-113 to UC-116, and its notices and
// the Word tool's own refusals, UC-270 to UC-278), apart from
// registry-uc.tsx to keep that file under the size limit: the notice compared with now, over registry-u8.tsx's host,
// whose word_blackline runs, fails or cuts the document short as each entry asks. UC_ALL is registry-uc.tsx's UC
// entries, then registry-uc-blocks.tsx's UC_BLOCKS, then these, so registry.tsx lists UC in one place.
import { compared } from "./registry-u8.tsx";
import type { Entry } from "./entry.ts";
import { UC_BLOCKS } from "./registry-uc-blocks.tsx";
import { UC } from "./registry-uc.tsx";
import { versionWords } from "./words.ts";
import type { VersionKey } from "@clauseo/components/lib/words-versions.ts";
import type { BlacklineNotice } from "../transport/protocol.ts";

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

/** The host's notices over a marked-up document, each with its line (UC-270 to UC-273). */
const NOTICES: [string, BlacklineNotice, VersionKey][] = [
  ["UC-270", "part_not_shown", "Part of this document couldn't be marked up. The rest is below."],
  [
    "UC-271",
    "hidden_text",
    "This document has text that Word hides. It's shown here like the rest.",
  ],
  ["UC-272", "numbers_may_differ", "Some numbers here may not match the ones Word shows."],
  ["UC-273", "may_differ", "Some of this may look different in Word."],
];

/** The Word tool's own refusals, each with its line and no Try again (UC-274 to UC-278). */
const REFUSALS: [string, string, VersionKey][] = [
  [
    "UC-274",
    "blackline.nested_table",
    "Clauseo couldn't compare these two versions because of a table inside a table. Open both in Word to compare them.",
  ],
  [
    "UC-275",
    "blackline.cannot_compare",
    "Clauseo couldn't compare these two versions. Open both in Word to compare them.",
  ],
  [
    "UC-276",
    "blackline.not_readable",
    "Clauseo couldn't read one of these versions. It may be damaged, or it may not be a Word document.",
  ],
  [
    "UC-277",
    "blackline.too_large",
    "This document is too large to mark up. Open both versions in Word to compare them.",
  ],
  [
    "UC-278",
    "blackline.format_unknown",
    "This version of Clauseo can't mark up these changes. Update Clauseo to see them.",
  ],
];

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
  ...NOTICES.map(([id, notice, words]) =>
    compared(id, versionWords(words), { ...COMPARED, mark: { notices: [notice] } }),
  ),
  ...REFUSALS.map(([id, code, words]) =>
    compared(id, versionWords(words), { ...COMPARED, mark: { refused: code, retryable: false } }),
  ),
];

/**
 * UC's entries: registry-uc.tsx's (UC-01 to UC-78), registry-uc-blocks.tsx's (UC-80 to UC-106), then
 * the Word blackline's (UC-113 to UC-116, UC-270 to UC-278).
 */
export const UC_ALL: Entry[] = [...UC, ...UC_BLOCKS, ...UC_BLACKLINE];
