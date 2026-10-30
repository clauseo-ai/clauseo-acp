// Versions in the room's group: the timeline and its foot, the band at now and looking back, the compare bar, the
// papers as they stood on their desk, the papers wall, and the restore and undo dialogs closed until pressed. Each
// sample is its own room, so two of the wall or the sheets never share an id or a node.
import type { ReactNode } from "react";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { MissingSheet, PdfSheet } from "@clauseo/components/clauseo/version-page.tsx";
import { VersionWall } from "@clauseo/components/clauseo/version-wall.tsx";
import { Group, Sample } from "./frame.tsx";
import {
  EARLY,
  HISTORY,
  LARGE_AT,
  LATE,
  OWN,
  PDF,
  PDF_NOW,
  PDF_THEN,
  UNSAVED,
  at,
} from "./versions-data.ts";
import {
  BandLooking,
  BandNow,
  CompareSample,
  DatesDesk,
  DraftDesk,
  RestoreTrigger,
  TimelinePanel,
  WhereFoot,
} from "./versions-samples.tsx";

const noop = () => {};

/** A sample's own room: its ids and nodes are its own. */
function Room({ children }: { children: ReactNode }) {
  return <RoomScopeProvider>{children}</RoomScopeProvider>;
}

export function VersionsParts() {
  return (
    <Group title="Versions">
      <Sample name="Version panel, many versions">
        <Room>
          <TimelinePanel history={HISTORY} />
        </Room>
      </Sample>
      <Sample name="Version panel, a version not kept">
        <Room>
          <TimelinePanel history={EARLY} unsaved={UNSAVED} />
        </Room>
      </Sample>
      <Sample name="Where versions are kept, closed">
        <Room>
          <WhereFoot />
        </Room>
      </Sample>
      <Sample name="Version band, at now">
        <Room>
          <BandNow variant="plain" />
        </Room>
      </Sample>
      <Sample name="Version band, at now, after a restore, an answer running">
        <Room>
          <BandNow variant="restored" busy />
        </Room>
      </Sample>
      <Sample name="Version band, at now, a change not kept">
        <Room>
          <BandNow variant="failed" />
        </Room>
      </Sample>
      <Sample name="Version band, looking back">
        <Room>
          <BandLooking id="v4" />
        </Room>
      </Sample>
      <Sample name="Version band, looking back at a large file">
        <Room>
          <BandLooking id={LARGE_AT.id} />
        </Room>
      </Sample>
      <Sample name="Version band, looking back, an answer running">
        <Room>
          <BandLooking id="v4" busy />
        </Room>
      </Sample>
      <Sample name="Compare bar, two documents differ">
        <Room>
          <CompareSample />
        </Room>
      </Sample>
      <Sample name="Desk, draft marked against now" wide>
        <Room>
          <DraftDesk />
        </Room>
      </Sample>
      <Sample name="Desk, list of dates marked against now">
        <Room>
          <DatesDesk />
        </Room>
      </Sample>
      <Sample name="PDF, changed">
        <Room>
          <PdfSheet
            title={PDF.title}
            changed
            state={PDF.changed}
            then={PDF_THEN}
            now={PDF_NOW}
            onOpen={noop}
          />
        </Room>
      </Sample>
      <Sample name="PDF, unchanged">
        <Room>
          <PdfSheet
            title={PDF.title}
            changed={false}
            state={PDF.unchanged}
            then={PDF_THEN}
            now={PDF_NOW}
            onOpen={noop}
          />
        </Room>
      </Sample>
      <Sample name="Paper not in the matter yet">
        <Room>
          <MissingSheet title={LATE.title} first={LATE.first} />
        </Room>
      </Sample>
      <Sample name="Papers wall, now" wide>
        <Room>
          <VersionWall version={at("v9")} looking={false} own={OWN} />
        </Room>
      </Sample>
      <Sample name="Papers wall, looking back" wide>
        <Room>
          <VersionWall version={at("v4")} looking own={OWN} />
        </Room>
      </Sample>
      <Sample name="Restore dialog, from its trigger">
        <Room>
          <RestoreTrigger mode="restore" />
        </Room>
      </Sample>
      <Sample name="Restore dialog, Word holds the file">
        <Room>
          <RestoreTrigger mode="word" />
        </Room>
      </Sample>
      <Sample name="Undo dialog, a change made again since">
        <Room>
          <RestoreTrigger mode="undo" />
        </Room>
      </Sample>
    </Group>
  );
}
