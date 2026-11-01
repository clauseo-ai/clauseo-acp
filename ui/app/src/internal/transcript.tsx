// The open thread's transcript: its entry ids in order, on a virtualized list. The list renders again only when an
// entry is added or removed; each row reads its own entry. It follows the end while the person is there, and offers
// "Jump to latest" once they have scrolled away from it.
import { LegendList, type LegendListRef } from "@legendapp/list/react";
import { MarkdownHostContext, type MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { useCallback, useRef, useSyncExternalStore, type RefObject } from "react";
import { useEntryIds, useViewStore } from "../store/hooks.ts";
import type { EntryId, StreamId } from "../transport/protocol.ts";
import { Row } from "./rows.tsx";
import { JUMP_TO_LATEST } from "./words.ts";

const keyOf = (id: EntryId) => id;

/** What a link or reference in an answer does when clicked. Nothing yet: the app has no `open_external` or
 * `open_reference` command, and the webview never navigates (the renderer cancels every click). Wire both here once
 * those commands exist. */
const ANSWER_HOST: MarkdownHost = {
  openExternal() {},
  openReference() {},
};

/**
 * Whether the list follows its end: LegendList's own flag, true while the scroll is within its
 * `maintainScrollAtEndThreshold` (10% of the viewport) of the end. It turns false when the person scrolls up, and
 * true again only when they come back that close, or jump.
 */
function useFollowing(list: RefObject<LegendListRef | null>): boolean {
  const subscribe = (changed: () => void) =>
    list.current?.getState().listen("isWithinMaintainScrollAtEndThreshold", changed) ?? (() => {});
  // Read from the list itself, so a list that mounts away from its end shows the button at once. Before the list
  // mounts there is nothing to jump to: following.
  const read = () => list.current?.getState().isWithinMaintainScrollAtEndThreshold ?? true;
  return useSyncExternalStore(subscribe, read, read);
}

export function Transcript({ stream }: { stream: StreamId }) {
  const ids = useEntryIds(useViewStore(), stream);
  const renderItem = useCallback(
    ({ item }: { item: EntryId }) => <Row id={item} stream={stream} />,
    [stream],
  );
  const list = useRef<LegendListRef>(null);
  const following = useFollowing(list);
  // A control rather than "follow again when back near the end": an answer that grows while the person reads above
  // keeps moving the end away, so no fixed region catches their return (in the macOS scoreboard's stall, #106, the list
  // ended the turn 7,600 px short of its end after 1,200 px up and back), and a region large enough would pull a reader
  // past text they have not read. The person decides; once at the end, the list follows again.
  // The button leaves with the click, so focus goes to the log, not to the page's body.
  const log = useRef<HTMLDivElement>(null);
  const jump = useCallback(() => {
    void list.current?.scrollToEnd({ animated: false });
    log.current?.focus({ preventScroll: true });
  }, []);
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        position: "relative",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        ref={log}
        tabIndex={-1}
        data-testid="transcript"
        role="log"
        aria-label="Transcript"
        style={{ flex: 1, minHeight: 0 }}
      >
        <MarkdownHostContext value={ANSWER_HOST}>
          <LegendList
            ref={list}
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
      {!following && (
        <button
          type="button"
          data-testid="jump-to-latest"
          onClick={jump}
          style={{ position: "absolute", bottom: 8, left: "50%", transform: "translateX(-50%)" }}
        >
          {JUMP_TO_LATEST}
        </button>
      )}
    </div>
  );
}
