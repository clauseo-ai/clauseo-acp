// The room's states: asking before a change, what Clauseo changed, the empty states, an earlier exchange folded to its
// line, and Versions opened from its button.
import { useRef, useState } from "react";
import { ChangeNote } from "@clauseo/components/clauseo/change-note.tsx";
import { EarlierExchange } from "@clauseo/components/clauseo/earlier.tsx";
import { EmptyPapers, FirstQuestion, SearchEmpty } from "@clauseo/components/clauseo/empty.tsx";
import { HistoryPanel } from "@clauseo/components/clauseo/history.tsx";
import { PermissionPrompt } from "@clauseo/components/clauseo/permission.tsx";
import type {
  PermissionAnswer,
  PermissionState,
} from "@clauseo/components/clauseo/permission-keys.ts";
import { Popover, PopoverTrigger } from "@clauseo/components/ui/popover.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Sample } from "./frame.tsx";
import {
  CHANGED,
  EARLIER,
  HISTORY,
  MISSING_QUERY,
  PAPER_COUNT,
  PERMISSION_PAPER,
  nothing,
} from "./room-data.ts";

/** Asking before a change: the prompt holds its three answers until one is given, then folds to a line. */
function AskingPermission() {
  const [state, setState] = useState<PermissionState>("pending");
  return (
    <PermissionPrompt
      paper={PERMISSION_PAPER}
      state={state}
      onAnswer={(a: PermissionAnswer) => setState(a)}
    />
  );
}

/** Versions, opened from its own button under a tone trigger, its right edge on the button's. */
function HistoryFromButton() {
  const box = useRef<HTMLDivElement>(null);
  return (
    <div ref={box} className="flex justify-end">
      <Popover>
        <PopoverTrigger render={<Button kind="tone" />}>Versions</PopoverTrigger>
        <HistoryPanel rows={HISTORY} anchor={() => box.current} />
      </Popover>
    </div>
  );
}

export function StateSamples() {
  return (
    <>
      <Sample name="PermissionPrompt, asking before a change">
        <AskingPermission />
      </Sample>
      <Sample name="PermissionPrompt, allowed once">
        <PermissionPrompt paper={PERMISSION_PAPER} state="once" onAnswer={nothing} />
      </Sample>
      <Sample name="ChangeNote, papers changed">
        <ChangeNote papers={CHANGED} onUndo={nothing} onCompare={nothing} />
      </Sample>
      <Sample name="ChangeNote, a paper edited since, Undo asks first">
        <ChangeNote
          papers={CHANGED}
          editedSince="Notice to the bank.docx"
          onUndo={nothing}
          onCompare={nothing}
        />
      </Sample>
      <Sample name="ChangeNote, undone">
        <ChangeNote papers={CHANGED} state="undone" />
      </Sample>
      <Sample name="EmptyPapers, a matter with no papers yet">
        <EmptyPapers onAdd={nothing} />
      </Sample>
      <Sample name="FirstQuestion, a new matter's thread">
        <FirstQuestion papers={PAPER_COUNT} />
      </Sample>
      <Sample name="SearchEmpty, nothing matches">
        <SearchEmpty query={MISSING_QUERY} />
      </Sample>
      <Sample name="EarlierExchange, folded to its line">
        <EarlierExchange
          at={EARLIER.at}
          question={EARLIER.question}
          reply={EARLIER.reply}
          made={EARLIER.made}
        />
      </Sample>
      <Sample name="HistoryPanel, opened from its button">
        <HistoryFromButton />
      </Sample>
    </>
  );
}
