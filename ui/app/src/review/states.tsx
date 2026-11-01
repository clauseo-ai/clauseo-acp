// The states the registered strings show in, built from the real parts, each in a room of its own.
import { EmptyPapers } from "@clauseo/components/clauseo/empty.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";

const nothing = () => {};

/** The room's toolbar as the frame shows it: the matter's name over an empty line, no plan, nothing to undo, the rail
 * folded behind Matters. */
export function RoomFrameToolbar({ title }: { title: string }) {
  return (
    <RoomScopeProvider>
      <RoomToolbar
        title={title}
        titleHint=""
        nextDate=""
        caseNo=""
        court=""
        subtitle=""
        plan={null}
        history={[]}
        canUndo={false}
        onUndo={nothing}
        railOpen={false}
        onRail={nothing}
      />
    </RoomScopeProvider>
  );
}

/** The wall of a matter with no papers yet. */
export function RoomFrameWall() {
  return (
    <RoomScopeProvider>
      <aside className="review-wall">
        <EmptyPapers />
      </aside>
    </RoomScopeProvider>
  );
}
