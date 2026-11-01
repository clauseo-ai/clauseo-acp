// The hook a code block uses: the block's tokens once the worker has them, else null and the block shows plain text.
import { useEffect, useState } from "react";
import type { HighlightClient } from "./client.ts";
import type { Lines } from "./languages.ts";
import { highlightClient } from "./start-worker.ts";

/**
 * The tokens for a code block, or null while it should show as plain text. Nothing is asked until `finished` is true
 * (the fence has closed), so a block still streaming in never starts the worker; a block seen before colours on its
 * first render, from the client's LRU. `client` is for the tests.
 */
export function useHighlight(
  code: string,
  lang: string,
  finished: boolean,
  client: HighlightClient = highlightClient(),
): Lines | null {
  const key = `${lang}\u0000${code}`;
  const [state, setState] = useState<{ key: string; lines: Lines | null }>(() => ({
    key,
    lines: finished ? (client.peek(code, lang) ?? null) : null,
  }));
  const known = state.key === key ? state.lines : null;
  useEffect(() => {
    if (!finished || known) return;
    let live = true;
    void client.highlight(code, lang).then((lines) => {
      if (live) setState({ key, lines });
    });
    return () => {
      live = false;
    };
  }, [client, code, lang, finished, key, known]);
  return finished ? known : null;
}
