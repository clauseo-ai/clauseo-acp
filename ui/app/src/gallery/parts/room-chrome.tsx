// The room's chrome: the toolbar over the matter, the window's own controls, and the rail of matters with its find.
import { CaptionButtons, CornerLights } from "@clauseo/components/clauseo/caption.tsx";
import { RailNotice } from "@clauseo/components/clauseo/rail-notice.tsx";
import { Rail } from "@clauseo/components/clauseo/rail.tsx";
import { RoomToolbar } from "@clauseo/components/clauseo/room-toolbar.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Sample } from "./frame.tsx";
import { Own } from "./room-own.tsx";
import {
  FIND_ALL,
  FIND_RECENT,
  HISTORY,
  MATTER_HEAD,
  NOTICE,
  PINNED,
  PLAN_CLOSE,
  PLAN_PLENTY,
  RAIL_GROUPS,
  RAIL_QUERY,
  SETTINGS_LINK,
  nothing,
} from "./room-data.ts";

/** The toolbar on the Mac, with the plan and History. */
function MacToolbar() {
  return (
    <Own>
      <RoomToolbar
        {...MATTER_HEAD}
        plan={PLAN_PLENTY}
        history={HISTORY}
        canUndo={false}
        onUndo={nothing}
        railOpen={false}
        onRail={nothing}
        platform="mac"
      />
    </Own>
  );
}

/** The toolbar on Windows: the plan stands left of the caption buttons, Undo is ready, a matter waits. */
function WindowsToolbar() {
  return (
    <Own>
      <RoomToolbar
        {...MATTER_HEAD}
        plan={PLAN_CLOSE}
        history={HISTORY}
        canUndo
        onUndo={nothing}
        railOpen={false}
        onRail={nothing}
        platform="windows"
        railWaits
      />
    </Own>
  );
}

/** The rail's notice about Clauseo itself, at the foot, with its two ways on. */
function Notice() {
  return (
    <RailNotice
      label="Updates"
      title={NOTICE.title}
      line={NOTICE.line}
      firm
      actions={
        <>
          <Button kind="tone" size="small">
            Restart after this answer
          </Button>
          <Button kind="quiet" size="small">
            Later
          </Button>
        </>
      }
    />
  );
}

/** The rail with its matters by date, Pinned above them, and Find under New matter. */
function RailByDate() {
  return (
    <Own>
      <div className="flex flex-col h-measure min-h-0">
        <Rail
          open
          groups={RAIL_GROUPS}
          pinned={PINNED}
          onNewMatter={nothing}
          find={{ all: FIND_ALL, recent: FIND_RECENT }}
          scratch={{ current: false, onOpen: nothing }}
          settings={SETTINGS_LINK}
          notice={<Notice />}
        />
      </div>
    </Own>
  );
}

/** The rail with Find open on a client's name: the matter it finds shows its client. */
function RailFinding() {
  return (
    <Own>
      <div className="flex flex-col h-measure min-h-0">
        <Rail
          open
          groups={RAIL_GROUPS}
          pinned={PINNED}
          onNewMatter={nothing}
          find={{ all: FIND_ALL, recent: FIND_RECENT, query: RAIL_QUERY, open: true }}
          scratch={{ current: false, onOpen: nothing }}
          settings={SETTINGS_LINK}
        />
      </div>
    </Own>
  );
}

export function ChromeSamples() {
  return (
    <>
      <Sample name="RoomToolbar, Mac, plan and Versions" wide>
        <MacToolbar />
      </Sample>
      <Sample name="RoomToolbar, Windows, Undo ready, a matter waits" wide>
        <WindowsToolbar />
      </Sample>
      <Sample name="CornerLights">
        <CornerLights />
      </Sample>
      <Sample name="CaptionButtons, Windows">
        <CaptionButtons />
      </Sample>
      <Sample name="Rail, matters by date" wide>
        <RailByDate />
      </Sample>
      <Sample name="Rail, finding a matter by its client" wide>
        <RailFinding />
      </Sample>
    </>
  );
}
