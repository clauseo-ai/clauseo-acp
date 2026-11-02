// "Open in Word" (H11): a Word document the wall's list_papers lists opens with the computer's program through
// open_file, from its tile and from a change note that names it. The room builds one WordOpen for the open matter and
// hands it down by context; a declined confirm or a failed open is quiet (open.ts's quietly). The label comes from the
// reader's words, loaded only once a listed paper is a Word document, so the room's first load carries none of them.
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { quietly, type ListedPapers } from "../open.ts";
import { isWordFile } from "./lock.ts";

/** A press that opens a paper in Word, with its words. */
export type WordOpener = { label: string; onOpen: () => void };

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
        onOpen: () => void quietly("open_file", transport.openFile(file)),
      });
    }
    return { openerFor: (path) => openers.get(path) ?? null };
  }, [transport, listed, rdt, lang]);
}
