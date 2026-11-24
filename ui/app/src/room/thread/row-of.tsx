// One row of an exchange, live or from an older page. Apart from rows.tsx so a test that wraps Row (stream-commits)
// wraps the rows this renders too.
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { useEntry, useViewStore } from "../../store/hooks.ts";
import { HelperCard, HelperWordsFlat } from "./helper-card.tsx";
import { HELPER_PREFIX, HELPER_TEXT_PREFIX } from "./helper-entries.ts";
import { PagedRow, Row } from "./rows.tsx";

/** Words of a helper the window has no entry for, live: flat. */
function LiveHelperWords({ id, stream }: { id: EntryId; stream: StreamId }) {
  const entry = useEntry(useViewStore(), stream, id);
  return entry === undefined ? null : <HelperWordsFlat id={id} entry={entry} />;
}

/** One row: a live one by id, or an older page's by its entry (a turn can start on a page and go on in the window). */
export function RowOf({
  id,
  stream,
  paged,
  sentNow,
}: {
  id: EntryId;
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
  sentNow: boolean;
}) {
  // a helper's card (helper-card.tsx), live or paged; a helper's words that reach the rows: flat
  if (id.startsWith(HELPER_PREFIX)) return <HelperCard id={id} stream={stream} paged={paged} />;
  const entry = paged.get(id);
  if (id.startsWith(HELPER_TEXT_PREFIX))
    return entry === undefined ? (
      <LiveHelperWords id={id} stream={stream} />
    ) : (
      <HelperWordsFlat id={id} entry={entry} />
    );
  if (entry !== undefined)
    return sentNow ? <PagedRow entry={entry} sentNow /> : <PagedRow entry={entry} />;
  return sentNow ? <Row id={id} stream={stream} sentNow /> : <Row id={id} stream={stream} />;
}
