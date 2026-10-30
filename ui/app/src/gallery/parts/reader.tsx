// The reader: a judgment the answer cites, read in its frame and described beside it; an entry of the CCI and NCLAT
// index; Indian Kanoon's results, its lookup guard and its absence; and the draft in review, with its tracked changes,
// its citations checked, and Word holding the file. The readers lie over the thread in the room; here each lies over
// a box of its own height.
import { createRef, type ReactNode } from "react";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import { CheckChip } from "@clauseo/components/clauseo/draft-pane.tsx";
import { DraftSheet, DraftView } from "@clauseo/components/clauseo/draft-review.tsx";
import { IndexPane, JudgmentPane } from "@clauseo/components/clauseo/judgment-pane.tsx";
import { IndexReader, JudgmentReader } from "@clauseo/components/clauseo/judgment-reader.tsx";
import {
  IkUnavailable,
  LookupGuard,
  PoweredByIK,
  ResearchResults,
} from "@clauseo/components/clauseo/ik.tsx";
import { Group, Sample } from "./frame.tsx";
import { DraftPaneSample, LookupSample, WordLockSample } from "./reader-samples.tsx";
import {
  DRAFT,
  INDEX_ENTRY,
  JUDGMENT,
  READ_ALREADY,
  REVIEW_OPEN,
  REVIEW_SETTLING,
  RESULTS,
  SNAPSHOT,
} from "./reader-data.ts";

const noop = () => {};
/** Only the Bluebell Mills judgment is one the reader holds, so its name in the list since opens it. */
const opensBluebell = (id: string) => id === "bluebell";
/** The box a reader lies over, a measure tall; parts.css brings the reader's foot to the box's, with no composer. */
const BOX = "parts-pane h-measure";
const LOOKUPS = { searches: 120, read: 90, fit: 12 };

/** Each reader's parts get a room of their own, so the ids they name stay unique on the page. */
function Scoped({ children }: { children: ReactNode }) {
  return <RoomScopeProvider>{children}</RoomScopeProvider>;
}

export function ReaderParts() {
  return (
    <Group title="The reader">
      <Sample name="JudgmentReader, the quoted paragraph" wide>
        <Scoped>
          <div className={BOX}>
            <JudgmentReader
              judgment={JUDGMENT}
              open
              from={null}
              backRef={createRef<HTMLButtonElement>()}
              onBack={noop}
            />
          </div>
        </Scoped>
      </Sample>
      <Sample name="JudgmentPane, cited and checked">
        <Scoped>
          <JudgmentPane judgment={JUDGMENT} offline={false} opens={opensBluebell} onOpen={noop} />
        </Scoped>
      </Sample>
      <Sample name="JudgmentPane, offline">
        <Scoped>
          <JudgmentPane judgment={JUDGMENT} offline opens={opensBluebell} onOpen={noop} />
        </Scoped>
      </Sample>
      <Sample name="IndexReader, an entry" wide>
        <Scoped>
          <div className={BOX}>
            <IndexReader
              entry={INDEX_ENTRY}
              snapshot={SNAPSHOT}
              open
              from={null}
              backRef={createRef<HTMLButtonElement>()}
              onBack={noop}
              onOpenOrder={noop}
            />
          </div>
        </Scoped>
      </Sample>
      <Sample name="IndexPane, the snapshot">
        <Scoped>
          <IndexPane snapshot={SNAPSHOT} size={{ cases: 2900, appeals: 425 }} />
        </Scoped>
      </Sample>
      <Sample name="PoweredByIK">
        <PoweredByIK />
      </Sample>
      <Sample name="ResearchResults, online">
        <ResearchResults results={RESULTS} highCourts={7} offline={false} onOpen={noop} />
      </Sample>
      <Sample name="ResearchResults, offline">
        <ResearchResults results={RESULTS} highCourts={7} offline onOpen={noop} />
      </Sample>
      <Sample name="LookupGuard, asking">
        <LookupSample so={LOOKUPS} expected="4:44 pm" />
      </Sample>
      <Sample name="LookupGuard, continued">
        <LookupGuard state="continued" at="4:39 pm" so={LOOKUPS} onAnswer={noop} />
      </Sample>
      <Sample name="LookupGuard, stopped">
        <LookupGuard state="stopped" at="4:39 pm" so={LOOKUPS} onAnswer={noop} />
      </Sample>
      <Sample name="IkUnavailable">
        <IkUnavailable retry="4:52 pm" read={READ_ALREADY} onOpen={noop} />
      </Sample>
      <Sample name="DraftView, changes" wide>
        <Scoped>
          <div className={BOX}>
            <DraftView
              draft={DRAFT}
              review={REVIEW_OPEN}
              tab="changes"
              wordSince={null}
              backRef={createRef<HTMLButtonElement>()}
              onBack={noop}
              onOpenInWord={noop}
              onFocus={noop}
            />
          </div>
        </Scoped>
      </Sample>
      <Sample name="DraftSheet, citations">
        <Scoped>
          <DraftSheet
            draft={DRAFT}
            review={{ ...REVIEW_OPEN, focus: "cite:bluebell" }}
            tab="citations"
            onFocus={noop}
          />
        </Scoped>
      </Sample>
      <Sample name="DraftPane, changes">
        <DraftPaneSample start={REVIEW_SETTLING} tab="changes" />
      </Sample>
      <Sample name="DraftPane, citations">
        <DraftPaneSample start={REVIEW_OPEN} tab="citations" />
      </Sample>
      <Sample name="CheckChip, verified">
        <CheckChip status="verified" />
      </Sample>
      <Sample name="CheckChip, unverifiable">
        <CheckChip status="unverifiable" />
      </Sample>
      <Sample name="CheckChip, mismatched">
        <CheckChip status="mismatched" />
      </Sample>
      <Sample name="CheckChip, not found">
        <CheckChip status="notfound" />
      </Sample>
      <Sample name="CheckChip, unchecked">
        <CheckChip status="unchecked" />
      </Sample>
      <Sample name="CheckChip, checking">
        <CheckChip status="checking" />
      </Sample>
      <Sample name="WordLockDialog, closed">
        <WordLockSample />
      </Sample>
    </Group>
  );
}
