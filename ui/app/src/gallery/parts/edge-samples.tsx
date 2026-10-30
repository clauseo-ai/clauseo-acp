// The edge's samples that hold state, so a press shows the part's next state: the prompt that hands the question to the
// computer's dialog, the refusal pause, the note on Not protected, and the room's line that opens what the matter
// allows. Overlays open on a press, never on load.
import { useState } from "react";
import {
  AllowedList,
  NotProtectedMark,
  WorkspaceLine,
  type Unprotected,
} from "@clauseo/components/clauseo/allowed.tsx";
import {
  EdgePrompt,
  RefusalPause,
  type EdgeKind,
  type EdgeState,
} from "@clauseo/components/clauseo/edge.tsx";
import { OsDialog } from "@clauseo/components/clauseo/os-dialog.tsx";
import { NotProtectedNote } from "@clauseo/components/clauseo/protection.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { Platform } from "@clauseo/components/lib/display.ts";
import {
  edgeDialogWords,
  useVersionWords,
  type EdgeAsk,
} from "@clauseo/components/lib/words-versions.ts";
import {
  ALLOWED,
  FOLDER,
  MATTER_FOLDER,
  MATTER_NAME,
  NOW,
  REFUSALS,
  REFUSED,
} from "./edge-data.ts";

const noop = () => {};

/** A prompt that asks, then shows the computer's dialog on Allow, and answers from the dialog's buttons. */
export function EdgeFlow({
  kind,
  place,
  platform,
  keys = false,
}: {
  kind: EdgeKind;
  place: string;
  platform: Platform;
  /** Whether this prompt takes the keys: one prompt on the page at a time. */
  keys?: boolean;
}) {
  const { lang } = useVersionWords();
  const [state, setState] = useState<EdgeState>({ kind: "pending" });
  const [remember, setRemember] = useState(true);
  const ask: EdgeAsk =
    kind === "folder"
      ? { kind, name: FOLDER.name }
      : kind === "site"
        ? { kind, place }
        : { kind, matter: MATTER_NAME };
  const d = edgeDialogWords(lang, ask);
  return (
    <>
      <EdgePrompt
        kind={kind}
        place={place}
        state={state}
        platform={platform}
        keys={keys}
        onAllow={() => setState({ kind: "confirming" })}
        onRefuse={() => setState({ kind: "refused", remembered: false })}
      />
      {state.kind === "confirming" && (
        <OsDialog
          platform={platform}
          title={d.title}
          lines={d.lines}
          foot={d.foot}
          remember={
            d.remember === null
              ? null
              : { label: d.remember, checked: remember, onChange: setRemember }
          }
          confirm={d.confirm}
          cancel={d.cancel}
          primary={d.primary}
          onConfirm={() =>
            setState({
              kind: "allowed",
              remembered: kind === "off" ? false : remember,
              at: NOW,
            })
          }
          onCancel={() =>
            setState({ kind: "refused", remembered: kind === "off" ? false : remember })
          }
        />
      )}
    </>
  );
}

/** The third refusal in a row, or the twentieth, with its two answers taking a press. */
export function RefusalFlow() {
  const [state, setState] = useState<
    { kind: "pending" } | { kind: "goneOn" } | { kind: "stopped"; at: string }
  >({ kind: "pending" });
  return (
    <RefusalPause
      count="row"
      refused={REFUSALS}
      earlier={0}
      state={state}
      onGoOn={() => setState({ kind: "goneOn" })}
      onStop={() => setState({ kind: "stopped", at: NOW })}
    />
  );
}

/** Not protected, explained once. Its trigger is the mark itself, so the note stays shut until a press. */
export function NoteSample({ why, platform }: { why: Unprotected["why"]; platform: Platform }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button kind="quiet" size="small" onClick={() => setOpen(true)}>
        <NotProtectedMark short />
      </Button>
      <NotProtectedNote
        open={open}
        matter={MATTER_NAME}
        why={why}
        platform={platform}
        onClose={() => setOpen(false)}
        onProtect={() => setOpen(false)}
      />
    </>
  );
}

/** The room's line, with what the matter allows in its card, which opens on a press. */
export function WorkspaceSample({ unprotected }: { unprotected: Unprotected | null }) {
  const [open, setOpen] = useState(false);
  return (
    <WorkspaceLine unprotected={unprotected} open={open} onOpenChange={setOpen}>
      <AllowedList
        folder={MATTER_FOLDER}
        allowed={ALLOWED}
        refused={REFUSED}
        unprotected={unprotected}
        onRemove={noop}
        onProtection={noop}
      />
    </WorkspaceLine>
  );
}
