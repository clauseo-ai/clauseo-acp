// U3's entries for matter.file_refused: the matter's folder refused a change to a file, said by how much else the act
// did (nothing, all but one file, all but several) and why (the file's permissions, a second name, a saved copy not
// checked), nine sentences in place of one "U3-err-<code>" refusal (words-errors-refused.ts). Each is shown alone, as
// Versions' line or the tracked changes' line says it after a refused restore, undo or decision. Draft English (Rohan
// to approve); the Hindi is to come. A tenth, many.mixed, is for several files refused for different reasons, with no
// reason given (REFUSED_MIXED). Also matter.file_write_failed's three (words-errors-written.ts): writing a file in
// place stopped partway, the file put back with nothing else changed, put back after other papers may have changed,
// or possibly damaged. Apart from registry-offline.tsx for its length.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWordsOf } from "@clauseo/components/lib/words-errors.ts";
import {
  REFUSED,
  REFUSED_MIXED,
  type RefusedBecause,
  type RefusedCause,
  type RefusedReach,
} from "@clauseo/components/lib/words-errors-refused.ts";
import { WRITTEN, type WrittenCase } from "@clauseo/components/lib/words-errors-written.ts";
import type { Entry } from "./entry.ts";
import { errorWords } from "./words.ts";

/** Each sentence's cause as the host sends it. */
const BECAUSE: Record<RefusedCause, RefusedBecause> = {
  permissions: "permissions",
  second_name: "second_name",
  saved_copy: "saved_copy_unconfirmed",
};

/** The refused files and how many the act wrote or meant to, for each reach. */
const ACT: Record<RefusedReach, { files: string[]; of: number }> = {
  only: { files: ["Leases/Lease - Rathi Steels.docx"], of: 1 },
  one: { files: ["Leases/Lease - Rathi Steels.docx"], of: 4 },
  many: { files: ["Leases/Lease - Rathi Steels.docx", "Notices/Notice to quit.docx"], of: 4 },
};

/** The line alone, as the room says it for the refusal. */
const alone = (reach: RefusedReach, cause: RefusedCause) => (lang: Lang) => (
  <main lang={lang} className="review-alone">
    <p>
      {errorWordsOf(lang, {
        code: "matter.file_refused",
        files: ACT[reach].files,
        refusal: { because: BECAUSE[cause], of: ACT[reach].of },
      })}
    </p>
  </main>
);

type Key = Parameters<typeof errorWords>[0];

/** matter.file_refused's nine entries, "U3-err-matter.file_refused.<reach>.<cause>", and many.mixed's, in its code's
 * place. */
export const REFUSED_ENTRIES: Entry[] = [
  ...(Object.keys(REFUSED) as RefusedReach[]).flatMap((reach) =>
    (Object.keys(REFUSED[reach]) as RefusedCause[]).map((cause) => ({
      id: `U3-err-matter.file_refused.${reach}.${cause}`,
      words: errorWords(REFUSED[reach][cause] as Key),
      screen: "room/versions",
      render: alone(reach, cause),
    })),
  ),
  {
    id: "U3-err-matter.file_refused.many.mixed",
    words: errorWords(REFUSED_MIXED as Key),
    screen: "room/versions",
    render: (lang: Lang) => (
      <main lang={lang} className="review-alone">
        <p>
          {errorWordsOf(lang, {
            code: "matter.file_refused",
            files: ACT.many.files,
            refusal: { refused: 3, of: ACT.many.of },
          })}
        </p>
      </main>
    ),
  },
];

/** The write failure for each of matter.file_write_failed's sentences. */
const FAILURE: Record<WrittenCase, { put_back: boolean; of: number }> = {
  "only.put_back": { put_back: true, of: 1 },
  "others.put_back": { put_back: true, of: 4 },
  damaged: { put_back: false, of: 4 },
};

/** matter.file_write_failed's three entries, "U3-err-matter.file_write_failed.<case>", in its code's place; each line
 * alone, as Versions' line or the tracked changes' line says it after a restore, undo or decision stopped partway. */
export const WRITTEN_ENTRIES: Entry[] = (Object.keys(WRITTEN) as WrittenCase[]).map((c) => ({
  id: `U3-err-matter.file_write_failed.${c}`,
  words: errorWords(WRITTEN[c] as Key),
  screen: "room/versions",
  render: (lang: Lang) => (
    <main lang={lang} className="review-alone">
      <p>
        {errorWordsOf(lang, {
          code: "matter.file_write_failed",
          files: ["Leases/Lease - Rathi Steels.docx"],
          write_failure: FAILURE[c],
        })}
      </p>
    </main>
  ),
}));
