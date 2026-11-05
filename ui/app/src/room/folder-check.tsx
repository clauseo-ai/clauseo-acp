// New matter's folder check (folder rights, FR7 to FR10, FR13, FR14): a folder picked, the form asks the host about
// the place the create would use (FolderSupport) with the source the create sends. A network drive or a drive without
// file permissions shows a warning under the folder line, why behind its info icon, with Choose another folder, which
// opens the picker again, and Create anyway in Create's place. Create's own refusal (matter.folder_*) is said at once,
// and Create held back for that folder. Anything else, a check that couldn't answer included, shows nothing: Create
// goes ahead and the host checks again. Create waits for the answer, at most the 5 s the host allows; a new pick drops
// the old answer. Part of New matter's chunk, with the warning (folder-warning.tsx) and its words (words-folder.ts).
import { useRef, useState } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { FolderRefId, HostError } from "../transport/protocol.ts";
import type { NextMatterSource } from "./host-next.ts";
import { FolderWarning, type Warned } from "./folder-warning.tsx";
import type { Matters } from "./matters.ts";

export type Picked = { folder: FolderRefId; display: string };
/** The check's answer: a warning, create's refusal, or nothing to say (null). */
type Answer = { warn: Warned } | { refused: HostError } | null;
/** Where a create goes, and the folder its refusal names. */
export type Where = { source: NextMatterSource; path?: string | undefined };

/** How long Create waits for the check: the host answers check_failed by then. */
export const CHECK_WAIT_MS = 5000;

async function answerOf(matters: Matters, source: NextMatterSource): Promise<Answer> {
  try {
    const support = await matters.folderSupport(source);
    return support === "network" || support === "no_permissions" ? { warn: support } : null;
  } catch (thrown) {
    const error = toHostError(thrown);
    return error.code.startsWith("matter.folder_") ? { refused: error } : null;
  }
}

/** The answer, or null when it takes longer than the host allows. */
async function within(asked: Promise<Answer>): Promise<Answer> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      asked,
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), CHECK_WAIT_MS);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

/** The folder the form picked, an existing folder by Change or a parent by Create's picker, and its check. */
export function usePickedFolder(matters: Matters, lang: Lang) {
  const [existing, setExisting] = useState<Picked | null>(null);
  const [parent, setParent] = useState<Picked | null>(null);
  // undefined while the check runs
  const [answer, setAnswer] = useState<Answer | undefined>(null);
  // the latest pick's check: an answer for any other is dropped
  const asked = useRef<Promise<Answer> | null>(null);
  const check = (source: NextMatterSource) => {
    const done = answerOf(matters, source);
    asked.current = done;
    setAnswer(undefined);
    void done.then((found) => {
      if (asked.current === done) setAnswer(found);
    });
  };
  const forget = () => {
    asked.current = null;
    setParent(null);
    setAnswer(null);
  };
  const reset = () => {
    forget();
    setExisting(null);
  };
  /** Change: an existing folder, its display for the form, or null when the lawyer cancelled. */
  const pickExisting = async () => {
    const picked = await matters.pickFolder();
    if (picked === null) return null;
    setExisting(picked);
    setParent(null);
    check({ existing_folder: { folder: picked.folder } });
    return picked.display;
  };
  const pickParent = async () => {
    const picked = await matters.pickFolder();
    if (picked === null) return null;
    setParent(picked);
    check({ new_folder: { folder: picked.folder } });
    return picked;
  };
  /** Where Create goes, once the check has answered: the default location when `useDefault` and nothing is picked;
   * a parent folder picked now when none was. Null stops the create: a cancelled picker, a refused existing folder
   * (its line shows), a warning that shows only now, or a new pick meanwhile. A parent's refusal comes back with it,
   * for the caller's line, and the parent is forgotten. */
  const where = async (useDefault: boolean): Promise<(Where & { refused?: HostError }) | null> => {
    const shown = answer;
    let found: Where;
    if (existing !== null) {
      found = { source: { existing_folder: { folder: existing.folder } }, path: existing.display };
    } else if (useDefault) {
      return { source: "default_location" };
    } else {
      const folder = parent ?? (await pickParent());
      if (folder === null) return null;
      found = { source: { new_folder: { folder: folder.folder } }, path: folder.display };
    }
    const now = asked.current;
    const said = now === null ? null : await within(now);
    if (asked.current !== now) return null;
    if (said !== null && "refused" in said) {
      if (existing !== null) return null;
      forget();
      return { ...found, refused: said.refused };
    }
    if (said !== null && "warn" in said && !(shown != null && "warn" in shown)) return null;
    return found;
  };
  const warned = answer != null && "warn" in answer ? answer.warn : null;
  return {
    existing,
    parent,
    /** Create's refusal of the existing folder picked, said at once; Create waits on another pick. */
    refused: existing !== null && answer != null && "refused" in answer ? answer.refused : null,
    warning:
      warned === null
        ? undefined
        : {
            note: <FolderWarning kind={warned} />,
            choose: fr(lang, "Choose another folder"),
            anyway: fr(lang, "Create anyway"),
            // a parent picked by Create: another parent, as Create's picker gives it
            ...(existing === null ? { onChoose: () => void pickParent() } : {}),
          },
    pickExisting,
    where,
    reset,
  };
}
