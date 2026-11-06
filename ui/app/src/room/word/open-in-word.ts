// "Open in Word" (H11): a Word document the wall's list_papers lists opens with the computer's program through
// open_file, from its tile and from a change note that names it. The room builds one WordOpen for the open matter and
// hands it down by context. A press while that file's open is on its way does nothing. A declined confirm is quiet; a
// failed open gives its code to the caller, which the change note shows in its error line, and an open Word started in
// Protected View (a path too long for Word, on Windows) says so, which the note, and the wall under its tiles, show in
// a quiet line. A path too long for Word comes with a way to show the matter's folder (reveal), where the folders to
// move or rename are; a tile's other failures go only to the console. The label comes from the
// reader's words, loaded only once a listed paper is a Word document, so the room's first load carries none of them.
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FileRefId } from "../../transport/protocol.ts";
import { quietly, refusalOf, type ListedPapers } from "../open.ts";
import { isWordFile } from "./lock.ts";

/** How a press went: the refusal's code, or null when it opened, the lawyer declined, or an open of the same file was
 * already on its way; and whether Word opened the file in Protected View. */
export type WordOpened = { refused: string | null; protectedView: boolean };

/** A press that opens a paper in Word, with its words: its label, and the line for an open in Protected View; and
 * Show in folder's press, which shows the matter's folder, for a path too long for Word. */
export type WordOpener = {
  label: string;
  protectedLine: string;
  onOpen: () => Promise<WordOpened>;
  onReveal: () => void;
};

const NOT_SENT: WordOpened = { refused: null, protectedView: false };

export type WordOpen = {
  /** The opener of a matter-relative path: a Word document the wall lists, once the words are in; else null. */
  openerFor: (path: string) => WordOpener | null;
};

const NONE: WordOpen = { openerFor: () => null };

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
    if (listed === null || rdt === null) return NONE;
    const label = rdt(lang, "Open in Word");
    const protectedLine = rdt(
      lang,
      "This file's path is long, so Word opens it in Protected View. Choose Enable Editing to make changes.",
    );
    const { matter } = listed;
    const onReveal = () => void quietly("reveal", transport.reveal({ matter }));
    const openers = new Map<string, WordOpener>();
    for (const p of listed.papers.papers) {
      if (!isWordFile(p.path)) continue;
      const file = p.file;
      openers.set(p.path, {
        label,
        protectedLine,
        onReveal,
        onOpen: async () => {
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
        },
      });
    }
    return { openerFor: (path) => openers.get(path) ?? null };
  }, [transport, listed, rdt, lang, opening]);
}
