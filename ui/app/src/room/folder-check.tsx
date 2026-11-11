// New matter's folder check (folder rights, FR7 to FR10, FR13, FR14): a folder picked, the form asks the host about
// the place the create would use (FolderSupport) with the source the create sends. A network drive or a drive without
// file permissions shows a warning under the folder line, why behind its info icon, with Choose another folder, which
// opens the picker again, and Create anyway in Create's place. Create's own refusal (matter.folder_*) is said at once,
// and Create held back for that folder. Anything else, a check that couldn't answer included, shows nothing: Create
// goes ahead and the host checks again. Create waits for the answer, at most the 5 s the host allows; a new pick drops
// the old answer. Part of New matter's chunk, with the warning (folder-warning.tsx) and its words (words-folder.ts).
// An existing folder picked is scanned too (UD2, room/scan/): its lines show under the folder line, Stop drops the pick,
// a refused scan is said as the check's refusal is, and Create waits for the scan's first event and a synced folder's
// "Got it".
import { useRef, useState } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { fr } from "@clauseo/components/lib/words-folder.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { FolderRefId, HostError } from "../transport/protocol.ts";
import type { NextMatterSource } from "./host-next.ts";
import { FolderWarning, type Warned } from "./folder-warning.tsx";
import type { Matters } from "./matters.ts";
import { ScanLines } from "./scan/scan-lines.tsx";
import { useScan } from "./scan/use-scan.ts";
import { usePickWait } from "./picker-wait.ts";

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

/** The folder the form picked by Choose folder…, and its check. */
export function usePickedFolder(matters: Matters, lang: Lang) {
  const [existing, setExisting] = useState<Picked | null>(null);
  // undefined while the check runs
  const [answer, setAnswer] = useState<Answer | undefined>(null);
  const scan = useScan(matters);
  // the picker's line while a pick is slow
  const wait = usePickWait();
  // bumped as Stop drops the pick, so the form drops the folder Change showed (matter-form.tsx's unpick)
  const [unpick, setUnpick] = useState(0);
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
    setAnswer(null);
  };
  const reset = () => {
    forget();
    scan.clear();
    setExisting(null);
  };
  /** The scan's Stop: the scan cancelled and the pick dropped, the form as it was before the pick. */
  const stop = () => {
    reset();
    setUnpick((n) => n + 1);
  };
  /** Choose folder…: an existing folder, its display for the form, or null when the lawyer cancelled. */
  const pickExisting = async () => {
    const picked = await wait.track(() => matters.pickFolder());
    if (picked === null) return null;
    setExisting(picked);
    check({ existing_folder: { folder: picked.folder } });
    scan.start(picked.folder);
    return picked.display;
  };
  /** Where Create goes, once the check has answered: the existing folder picked, else the default location when
   * `useDefault`. Null stops the create: no folder chosen, a refused existing folder (its line shows), a warning that
   * shows only now, or a new pick meanwhile. Every folder comes through the picker: none is asked for here. */
  const where = async (useDefault: boolean): Promise<(Where & { refused?: HostError }) | null> => {
    const shown = answer;
    if (existing === null) return useDefault ? { source: "default_location" } : null;
    const found: Where = {
      source: { existing_folder: { folder: existing.folder } },
      path: existing.display,
    };
    const now = asked.current;
    const said = now === null ? null : await within(now);
    if (asked.current !== now) return null;
    if (said !== null && "refused" in said) return null;
    if (said !== null && "warn" in said && !(shown != null && "warn" in shown)) return null;
    // the scan's first event says where the folder lives: a synced one waits for "Got it"
    if (!(await scan.settle())) return null;
    return found;
  };
  const warned = answer != null && "warn" in answer ? answer.warn : null;
  const checkRefused = answer != null && "refused" in answer ? answer.refused : null;
  const scanned = existing !== null && scan.scan?.folder === existing.folder ? scan.scan : null;
  return {
    existing,
    /** Create's refusal of the existing folder picked, said at once; Create waits on another pick. */
    refused: existing === null ? null : (checkRefused ?? scanned?.refused ?? null),
    /** The scan's lines under the folder line, for the existing folder picked. */
    scan:
      scanned === null ? undefined : (
        <ScanLines scan={scanned} onStop={stop} onGotIt={scan.gotIt} />
      ),
    unpick,
    opening: wait.opening,
    warning:
      warned === null
        ? undefined
        : {
            note: <FolderWarning kind={warned} />,
            choose: fr(lang, "Choose another folder"),
            anyway: fr(lang, "Create anyway"),
          },
    pickExisting,
    where,
    reset,
  };
}
