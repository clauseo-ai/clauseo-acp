// New matter's question in the rail's dialog (new-matter.tsx): its answer, New folder or Existing folder, the answer
// before Back (focus goes back to it), and whether Existing folder's picker is up; and H20's default location, read as
// a form opens. Existing folder answers only once the picker gives a folder: cancelled, the question stays. And where
// Existing folder opens the picker (existingStart), for stage E's pick_folder start.
import { useEffect, useState } from "react";
import type { Branch } from "@clauseo/components/clauseo/matter-form.tsx";
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

/** Where the computer's folder picker opens, as stage E's pick_folder and pick_folder_or_choose take it: the last
 * folder picked (their default), Documents, Clauseo's matters folder, or a path. */
export type PickStart = "last" | "documents" | "matters" | { path: string };

/** Whether Existing folder has opened the picker in this window. */
let pickedBefore = false;

/** Where Existing folder opens the picker: Documents the first time in this window, the last folder picked after.
 * Clauseo checks nothing on disk (on a Mac that could raise the Documents prompt): the dialog falls back by itself.
 * Not sent yet: once stage E's binding lands, folder-check.tsx's pickExisting passes it to pick_folder. */
export function existingStart(): PickStart {
  const start: PickStart = pickedBefore ? "last" : "documents";
  pickedBefore = true;
  return start;
}
