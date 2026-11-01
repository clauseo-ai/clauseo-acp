// The states the registered strings show in, built from the real parts, each in a room of its own.
import { EmptyPapers } from "@clauseo/components/clauseo/empty.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";

const nothing = () => {};

/** The room's toolbar as the frame shows it (src/room/): the computer's own window chrome, no search, Versions or plan,
 * the matter's name over an empty line, the rail folded behind Matters. A matter's name is in English in both
 * languages; the frame's own words, such as "No matters yet", take the page's language (titleLang). */
export function RoomFrameToolbar({ title, titleLang = "en" }: { title: string; titleLang?: Lang }) {
  return (
    <RoomScopeProvider>
      <RoomToolbar
        chrome="os"
        search={false}
        versions={false}
        title={title}
        titleLang={titleLang}
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

/** The wall of a matter with no papers yet, as the frame shows it: the title alone. */
export function RoomFrameWall() {
  return (
    <RoomScopeProvider>
      <aside className="review-wall">
        <EmptyPapers body={false} />
      </aside>
    </RoomScopeProvider>
  );
}
