// One row of an exchange, live or from an older page. Apart from rows.tsx so a test that wraps Row (stream-commits)
// wraps the rows this renders too.
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { PagedRow, Row } from "./rows.tsx";

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
  const entry = paged.get(id);
  if (entry !== undefined)
    return sentNow ? (
      <PagedRow entry={entry} stream={stream} sentNow />
    ) : (
      <PagedRow entry={entry} stream={stream} />
    );
  return sentNow ? <Row id={id} stream={stream} sentNow /> : <Row id={id} stream={stream} />;
}
