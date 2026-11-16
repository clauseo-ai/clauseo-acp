// matter.file_refused's sentences (words-errors.ts): the matter's folder refused a change to a file, said by why it
// refused the files (no reason when they were refused for different ones) and by how much else the act did: nothing
// (the act was on that file alone), everything but that one file, or everything but several. Apart from words-errors.ts to keep it under the size limit;
// their Hindi is in words-errors.hi.ts with the rest.
import { fileName } from "./words-core.ts";
import type { ErrorKey } from "./words-errors.ts";

/** Why the folder refused the change: the protocol's `RefusedBecause` (ui/app/src/transport/bindings.ts), copied, since
 * the parts can't import the app. ui/app/src/room/thread/error-codes.test.ts fails when the two differ. */
export type RefusedBecause = "permissions" | "second_name" | "saved_copy_unconfirmed" | "unknown";

/** The protocol's `FileRefusal`, copied as RefusedBecause is: why every refused file was refused (missing when the
 * reasons differ), how many were refused (the error's files name at most 50), and how many files the act wrote,
 * deleted or meant to (1: the act was on that file alone, and did nothing). */
export type FileRefusal = {
  because?: RefusedBecause | undefined;
  refused?: number | undefined;
  of: number;
};

/** How much the act did besides: nothing (only), all but one file (one), all but several (many). */
export type RefusedReach = "only" | "one" | "many";

/** The cause as the sentence says it; an `unknown` cause reads as the file's permissions. */
export type RefusedCause = "permissions" | "second_name" | "saved_copy";

/** matter.file_refused's sentences, by reach and cause. `{name}` is the first refused file's name, `{count}` how many
 * files were refused. Draft English (Rohan to approve); the Hindi is to come. */
export const REFUSED: Readonly<Record<RefusedReach, Record<RefusedCause, ErrorKey>>> = {
  only: {
    permissions:
      "Clauseo couldn't change {name} because the file is set so it can't be changed. It's as it was.",
    second_name:
      "Clauseo couldn't change {name} because the same file also appears somewhere else on this computer. It's as it was.",
    saved_copy:
      "Clauseo left {name} as it was because it couldn't first check its saved copy of the file. Try again.",
  },
  one: {
    permissions:
      "Clauseo couldn't change {name} because the file is set so it can't be changed. Everything else was done.",
    second_name:
      "Clauseo couldn't change {name} because the same file also appears somewhere else on this computer. Everything else was done.",
    saved_copy:
      "Clauseo left {name} as it was because it couldn't first check its saved copy of the file. Everything else was done; try again to change this one too.",
  },
  many: {
    permissions:
      "Clauseo couldn't change {count} papers, including {name}, which is set so it can't be changed. Everything else was done.",
    second_name:
      "Clauseo couldn't change {count} papers, including {name}, which also appears somewhere else on this computer. Everything else was done.",
    saved_copy:
      "Clauseo left {count} papers as they were, including {name}, because it couldn't first check its saved copies. Everything else was done; try again to change them too.",
  },
};

/** matter.file_refused's sentence for several files refused for different reasons: it gives no reason. The app track's
 * English (provisional); the Hindi is to come. */
export const REFUSED_MIXED: ErrorKey =
  "Clauseo couldn't change {count} papers, including {name}, for different reasons. Everything else was done.";

/** The protocol's `WriteFailure`, copied as FileRefusal is: whether the file was put back as it was, and how many
 * files the act wrote, deleted or meant to (1: nothing else changed). */
export type WriteFailure = { put_back: boolean; of: number };

/** What of a host error the sentence reads: the files it names, sorted, the one to name first; the refusal
 * (matter.file_refused); and the write failure (matter.file_write_failed, words-errors-written.ts). */
export type RefusedDetail = {
  files?: readonly string[] | undefined;
  refusal?: FileRefusal | undefined;
  write_failure?: WriteFailure | undefined;
};

/** matter.file_refused's sentence for the error, with its slots; null when the error names no file, which no sentence
 * can say. `{count}` is the refusal's count of refused files, else the files named. With no refusal the act reads as
 * on that file alone, for the file's permissions; several files with no shared reason read as REFUSED_MIXED, one file
 * with no reason as its permissions. */
export function refusedKey(
  error: RefusedDetail,
): { key: ErrorKey; slots: Record<string, string | number> } | null {
  const files = error.files ?? [];
  const first = files[0];
  if (first === undefined) return null;
  const { because, refused, of = 1 } = error.refusal ?? {};
  const count = refused ?? files.length;
  const reach: RefusedReach = of <= 1 ? "only" : count <= 1 ? "one" : "many";
  const slots = { name: fileName(first), count };
  if (because === undefined && reach === "many") return { key: REFUSED_MIXED, slots };
  const cause: RefusedCause =
    because === "second_name"
      ? "second_name"
      : because === "saved_copy_unconfirmed"
        ? "saved_copy"
        : "permissions";
  return { key: REFUSED[reach][cause], slots };
}
