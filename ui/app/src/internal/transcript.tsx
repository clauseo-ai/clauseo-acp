// The open thread's transcript: its entry ids in order, on a virtualized list. The list renders again only when an
// entry is added or removed; each row reads its own entry.
import { LegendList } from "@legendapp/list/react";
import { useCallback } from "react";
import { useEntryIds, useViewStore } from "../store/hooks.ts";
import type { EntryId, StreamId } from "../transport/protocol.ts";
import { Row } from "./rows.tsx";

const keyOf = (id: EntryId) => id;

export function Transcript({ stream }: { stream: StreamId }) {
  const ids = useEntryIds(useViewStore(), stream);
  const renderItem = useCallback(
    ({ item }: { item: EntryId }) => <Row id={item} stream={stream} />,
    [stream],
  );
  return (
    <div
      data-testid="transcript"
      role="log"
      aria-label="Transcript"
      style={{ flex: 1, minHeight: 0 }}
    >
      <LegendList
        data={ids}
        keyExtractor={keyOf}
        renderItem={renderItem}
        estimatedItemSize={48}
        recycleItems={false}
        maintainScrollAtEnd
        alignItemsAtEnd
        style={{ height: "100%" }}
      />
    </div>
  );
}
