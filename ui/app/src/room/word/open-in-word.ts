// "Open in Word" (H11): a Word document the wall's list_papers lists opens with the computer's program through
// open_file, from its tile and from a change note that names it. The room builds one WordOpen for the open matter and
// hands it down by context. A press while that file's open is on its way does nothing. A declined confirm is quiet; a
// failed open gives its code to the caller, which the change note shows in its error line (the room has no place to
// say why for a tile, so a tile's goes only to the console). The label comes from the
// reader's words, loaded only once a listed paper is a Word document, so the room's first load carries none of them.
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FileRefId } from "../../transport/protocol.ts";
import { refusalOf, type ListedPapers } from "../open.ts";
import { isWordFile } from "./lock.ts";

/** A press that opens a paper in Word, with its words. onOpen resolves to the refusal's code, or null when it opened,
 * the lawyer declined, or an open of the same file was already on its way. */
export type WordOpener = { label: string; onOpen: () => Promise<string | null> };

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
    const openers = new Map<string, WordOpener>();
    for (const p of listed.papers.papers) {
      if (!isWordFile(p.path)) continue;
      const file = p.file;
      openers.set(p.path, {
        label,
        onOpen: async () => {
          if (opening.has(file)) return null;
          opening.add(file);
          try {
            return await refusalOf("open_file", transport.openFile(file));
          } finally {
            opening.delete(file);
          }
        },
      });
    }
    return { openerFor: (path) => openers.get(path) ?? null };
  }, [transport, listed, rdt, lang, opening]);
}
