// "Open in Word" (H11): a Word document the wall's list_papers lists opens with the computer's
// program through open_file, from its tile and from a change note that names it. The room builds
// one WordOpen for the open matter and hands it down by context. A press while that file's open is
// on its way does nothing. A declined confirm is quiet; a failed open gives its code to the caller,
// which the change note shows in its error line, and an open Word started in Protected View (a path
// too long for Word, on Windows) says so, which the note, and the wall under its tiles, show in a
// quiet line. A path too long for Word comes with a way to show the matter's folder (reveal), where
// the folders to move or rename are; a show's refusal is given to the caller as an open's is. The
// label comes from the reader's words, loaded only once a listed paper is a Word document, so the
// room's first load carries none of them. A .docx's opener also carries the way to its tracked changes, which its tile
// offers as a second press (paper-wall.tsx), only when the host found tracked changes or comments in it. Every other
// file of a type the desktop opens (crates/desktop/src/open.rs, DOCUMENT_TYPES) opens the same way from its tile, with
// the computer's default app for its type, under the wall's words for it, Open it: wallOpenerFor gives those, while a
// change note offers Word's alone (openerFor).
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FileRefId, MatterId } from "../../transport/protocol.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { refusalOf, type ListedPapers } from "../open.ts";
import { isDocx, isWordFile } from "./lock.ts";

/** How a press went: the refusal's code, or null when it opened, the lawyer declined, or an open of the same file was
 * already on its way; and whether Word opened the file in Protected View. */
export type WordOpened = { refused: string | null; protectedView: boolean };

/** A press that opens a paper in Word, with its words: its label, and the line for an open in
 * Protected View; and Show in folder's press, which shows the matter's folder, for a path too
 * long for Word, and gives its refusal's code, or null when it went through or the lawyer
 * declined. */
export type WordOpener = {
  label: string;
  protectedLine: string;
  onOpen: () => Promise<WordOpened>;
  onReveal: () => Promise<string | null>;
  /** A .docx's tracked changes: the tile's press for them, and what their panel reads and decides them through. None
   * for a .doc, which Clauseo's Word tool can't read. */
  changes?: TrackedChanges;
};

/** The way to a .docx's tracked changes from its tile (room/word/changes-pane.tsx): the press's label, the transport
 * and the matter. */
export type TrackedChanges = { label: string; transport: HostTransport; matter: MatterId };

const NOT_SENT: WordOpened = { refused: null, protectedView: false };

export type WordOpen = {
  /** The opener of a matter-relative path: a Word document the wall lists, once the words are in; else null. */
  openerFor: (path: string) => WordOpener | null;
  /** The opener its tile offers: a Word document's, as openerFor's, or any other file's the desktop opens; else
   * null. */
  wallOpenerFor: (path: string) => WordOpener | null;
};

const NONE: WordOpen = { openerFor: () => null, wallOpenerFor: () => null };

/** The types open_file opens, as crates/desktop/src/open.rs's DOCUMENT_TYPES lists them: keep the two in step. */
const OPENABLE =
  /\.(doc|docx|dotx|rtf|odt|pdf|txt|md|csv|xls|xlsx|ods|ppt|pptx|odp|png|jpg|jpeg|gif|heic|tif|tiff)$/i;

/** Whether a file's tile opens it with the computer's default app. */
export const isOpenable = (path: string): boolean => OPENABLE.test(path);

export const WordOpenContext = createContext<WordOpen>(NONE);

/** The room's WordOpen, where it gives one; else one that opens nothing. */
export const useWordOpen = (): WordOpen => useContext(WordOpenContext);

type Rdt = (typeof import("@clauseo/components/lib/words-reader.ts"))["rdt"];

/** The WordOpen for the papers the wall listed: one per transport, list and language. */
export function useWordOpeners(
  transport: HostTransport,
  listed: ListedPapers,
  lang: Lang,
): WordOpen {
  const [rdt, setRdt] = useState<Rdt | null>(null);
  // the files whose open_file is on its way: a second press waits for the first
  const [opening] = useState(() => new Set<FileRefId>());
  const anyWord = listed?.papers.papers.some((p) => isWordFile(p.path)) ?? false;
  useEffect(() => {
    if (!anyWord || rdt !== null) return;
    let live = true;
    late(import("@clauseo/components/lib/words-reader.ts")).then(
      (m) => {
        if (live) setRdt(() => m.rdt);
      },
      (error: unknown) => console.warn(`the reader's words did not load: ${String(error)}`),
    );
    return () => {
      live = false;
    };
  }, [anyWord, rdt]);
  return useMemo(() => {
    if (listed === null) return NONE;
    const { matter } = listed;
    const onReveal = () => refusalOf("reveal", transport.reveal({ matter }));
    const openOf = (file: FileRefId) => async (): Promise<WordOpened> => {
      if (opening.has(file)) return NOT_SENT;
      opening.add(file);
      try {
        let protectedView = false;
        const opened = transport.openFile(file).then((o) => {
          protectedView = o.protected_view;
        });
        const refused = await refusalOf("open_file", opened);
        return { refused, protectedView };
      } finally {
        opening.delete(file);
      }
    };
    const word = new Map<string, WordOpener>();
    const others = new Map<string, WordOpener>();
    const openIt = mt(lang, "Open it");
    for (const p of listed.papers.papers) {
      if (isWordFile(p.path)) {
        if (rdt === null) continue;
        const changes: TrackedChanges = { label: rdt(lang, "Tracked changes"), transport, matter };
        word.set(p.path, {
          label: rdt(lang, "Open in Word"),
          protectedLine: rdt(
            lang,
            "This file's path is long, so Word opens it in Protected View. Choose Enable Editing to make changes.",
          ),
          onReveal,
          // the second press only for a .docx the host found tracked changes or comments in
          ...(isDocx(p.path) && p.tracked === true ? { changes } : {}),
          onOpen: openOf(p.file),
        });
      } else if (isOpenable(p.path)) {
        others.set(p.path, { label: openIt, protectedLine: "", onReveal, onOpen: openOf(p.file) });
      }
    }
    return {
      openerFor: (path) => word.get(path) ?? null,
      wallOpenerFor: (path) => word.get(path) ?? others.get(path) ?? null,
    };
  }, [transport, listed, rdt, lang, opening]);
}
