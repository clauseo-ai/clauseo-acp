// The hook a code block uses: the block's tokens once the worker has them, else null and the block shows plain text.
import { useContext, useEffect, useState } from "react";
import type { HighlightClient } from "./client.ts";
import { HighlightClientContext } from "./context.ts";
import type { Lines } from "./languages.ts";
import { highlightClient } from "./start-worker.ts";

/**
 * The tokens for a code block, or null while it should show as plain text. Nothing is asked until `finished` is true
 * (the fence has closed), so a block still streaming in never starts the worker; a block seen before colours on its
 * first render, from the client's LRU. The client is `client` if given, else HighlightClientContext's, else the
 * page's one client.
 */
export function useHighlight(
  code: string,
  lang: string,
  finished: boolean,
  given?: HighlightClient,
): Lines | null {
  const provided = useContext(HighlightClientContext);
  const client = given ?? provided ?? highlightClient();
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
      // Null leaves the block plain, as it already is: no state change and no render.
      if (live && lines) setState({ key, lines });
    });
    return () => {
      live = false;
    };
  }, [client, code, lang, finished, key, known]);
  return finished ? known : null;
}
