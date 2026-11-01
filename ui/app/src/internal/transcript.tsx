// The open thread's transcript: its entry ids in order, on a virtualized list. The list renders again only when an
// entry is added or removed; each row reads its own entry.
import { LegendList } from "@legendapp/list/react";
import { MarkdownHostContext, type MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { useCallback } from "react";
import { useEntryIds, useViewStore } from "../store/hooks.ts";
import type { EntryId, StreamId } from "../transport/protocol.ts";
import { Row } from "./rows.tsx";

const keyOf = (id: EntryId) => id;

/** What a link or reference in an answer does when clicked. Nothing yet: the app has no `open_external` or
 * `open_reference` command, and the webview never navigates (the renderer cancels every click). Wire both here once
 * those commands exist. */
const ANSWER_HOST: MarkdownHost = {
  openExternal() {},
  openReference() {},
};

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
      <MarkdownHostContext value={ANSWER_HOST}>
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
      </MarkdownHostContext>
    </div>
  );
}
