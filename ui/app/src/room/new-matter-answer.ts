// New matter's question in the rail's dialog (new-matter.tsx): its answer, New folder or Existing folder, the answer
// before Back (focus goes back to it), and whether Existing folder's picker is up; and H20's default location, read as
// a form opens. Existing folder answers only once the picker gives a folder: cancelled, the question stays. And where
// Existing folder opens the picker (existingStart), as pick_folder_or_choose's start.
import { useEffect, useState } from "react";
import type { Branch } from "@clauseo/components/clauseo/matter-form.tsx";
import type { StartHint } from "../transport/protocol.ts";
import type { Matters } from "./matters.ts";

/** The question's answer. `pickExisting` opens the picker and gives the folder picked, or null; `forget` drops what
 * a form held, as Back goes to the question. */
export function useAnswer(pickExisting: () => Promise<string | null>, forget: () => void) {
  // null: the question asked
  const [branch, setBranch] = useState<Branch | null>(null);
  const [last, setLast] = useState<Branch | null>(null);
  const [picking, setPicking] = useState(false);
  /** The question answered, or asked again by Back (null). */
  const answer = async (next: Branch | null) => {
    if (next === null) {
      forget();
      setBranch(null);
      return;
    }
    setLast(next);
    if (next === "new") {
      setBranch("new");
      return;
    }
    if (picking) return;
    setPicking(true);
    try {
      if ((await pickExisting()) !== null) setBranch("existing");
    } catch {
      // the picker failed: the question stays
    } finally {
      setPicking(false);
    }
  };
  /** The dialog closed: the next New matter asks again. */
  const reset = () => {
    setBranch(null);
    setLast(null);
  };
  return { branch, last, picking, answer, reset };
}

/** H20's default location as the desktop writes it, read while `asked`; null before it comes, or with none. */
export function useDefaultLocation(matters: Matters, asked: boolean): string | null {
  const [location, setLocation] = useState<string | null>(null);
  useEffect(() => {
    if (!asked) return;
    let live = true;
    void matters.defaultLocation().then((display) => {
      if (live) setLocation(display);
    });
    return () => {
      live = false;
    };
  }, [asked, matters]);
  return location;
}

/** The windows' matters (one per window) whose Existing folder has opened the picker. */
const pickedBefore = new WeakSet<Matters>();

/** Where Existing folder opens the picker: Documents the first time in this window, the last folder picked after.
 * Clauseo checks nothing on disk (on a Mac that could raise the Documents prompt): the dialog falls back by itself.
 * folder-check.tsx's pickExisting passes it to pick_folder_or_choose. */
export function existingStart(matters: Matters): StartHint {
  const start: StartHint = pickedBefore.has(matters) ? "last" : "documents";
  pickedBefore.add(matters);
  return start;
}
