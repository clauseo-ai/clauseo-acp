// matter.file_write_failed's sentences (words-errors.ts): writing a file in place stopped partway, said by whether the
// file was put back as it was and by whether the act changed other files: put back with nothing else changed, put
// back after other papers may have changed, or possibly damaged (also when the error doesn't say). Each names the
// file and, where a version holds the earlier state, points to Versions. Apart from words-errors.ts to keep it under
// the size limit; their Hindi is in words-errors.hi.ts with the rest.
import { fileName } from "./words-core.ts";
import type { ErrorKey } from "./words-errors.ts";
import type { RefusedDetail } from "./words-errors-refused.ts";

/** Which sentence: put back, the act on that file alone (only.put_back) or with other files (others.put_back); or not
 * known to be put back (damaged). */
export type WrittenCase = "only.put_back" | "others.put_back" | "damaged";

/** matter.file_write_failed's sentences. `{name}` is the file's bare name. The app track's English (provisional); the
 * Hindi is to come. */
export const WRITTEN: Readonly<Record<WrittenCase, ErrorKey>> = {
  "only.put_back":
    "Clauseo couldn't finish changing {name}, so it put the file back as it was. Nothing else was changed.",
  "others.put_back":
    "Clauseo couldn't finish changing {name}, so it put that file back as it was. It may already have changed other papers. To put them back, restore the version from just before this in Versions.",
  damaged:
    "Clauseo couldn't finish changing {name}, and the file may be damaged. To put it right, restore the version from just before this in Versions.",
};

/** Which of WRITTEN the error's write failure picks: with none, or not put back, the line that says it may be
 * damaged. */
export function writtenCase(error: RefusedDetail): WrittenCase {
  const failure = error.write_failure;
  if (failure?.put_back !== true) return "damaged";
  return failure.of <= 1 ? "only.put_back" : "others.put_back";
}

/** matter.file_write_failed's sentence for the error, with its slots; null when the error names no file, which no
 * sentence can say. */
export function writtenKey(
  error: RefusedDetail,
): { key: ErrorKey; slots: Record<string, string> } | null {
  const first = (error.files ?? [])[0];
  if (first === undefined) return null;
  return { key: WRITTEN[writtenCase(error)], slots: { name: fileName(first) } };
}
