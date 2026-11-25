// Who asks, above a permission request or a question a helper sent (C2's #1310: the entry's
// `helper`, read through helper-entries.ts askerOf): "Paper reader asks:", the helper named as its
// card names it; "A helper asks:" when the window has no entry for it. The lead's own asks show no
// line. The rows give the stream and an older page's entries through HelperAskContext (helper-ask-context.ts, given in row-of.tsx).
import { use, useMemo } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { HelperAskContext } from "./helper-ask-context.ts";
import { helperEntryId, helperPartOf, type HelperId } from "./helper-entries.ts";
import { nameWords, unnamedPick } from "./helper-view.ts";

const NO_PAGE: ReadonlyMap<EntryId, ViewEntry> = new Map();

/** The helper's name from its entry (a page's, else the view's); null when neither has it. */
const namePick =
  (helper: HelperId, paged: ReadonlyMap<EntryId, ViewEntry>) =>
  (v: StreamView | undefined): string | null => {
    const id = helperEntryId(helper);
    const part = helperPartOf(paged.get(id) ?? v?.entries.get(id));
    return part?.kind === "helper" ? part.entry.name : null;
  };

/** "{helper} asks:" above a helper's ask; nothing for the lead's (`helper` null). */
export function HelperAsker({ helper }: { helper: HelperId | null }) {
  if (helper === null) return null;
  return <Asker helper={helper} />;
}

function Asker({ helper }: { helper: HelperId }) {
  const { lang } = useDisplay();
  const at = use(HelperAskContext);
  const paged = at?.paged ?? NO_PAGE;
  const stream = at?.stream ?? ("" as StreamId);
  const store = useViewStore();
  const pickName = useMemo(() => namePick(helper, paged), [helper, paged]);
  const pickN = useMemo(() => unnamedPick(helper, paged), [helper, paged]);
  const name = useStreamPart(store, stream, pickName);
  const n = useStreamPart(store, stream, pickN);
  const words =
    name === null
      ? steps(lang, "A helper asks:")
      : steps(lang, "{helper} asks:", { helper: nameWords(lang, name, n) });
  return (
    <p className="helper-asker text-small font-ui" data-testid="helper-asker" data-helper={helper}>
      <Say>{words}</Say>
    </p>
  );
}
