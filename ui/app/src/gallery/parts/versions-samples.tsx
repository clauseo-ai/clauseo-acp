// The stateful samples of Versions: each part as the room holds it, with the small state that lets a press answer. The
// overlays stay closed until their trigger is pressed. Words come from the parts; the matter comes from versions-data.
import { useRef, useState } from "react";
import { Button } from "@clauseo/components/ui/button.tsx";
import { IconHistory, IconUndo } from "@clauseo/components/icons/icons.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useVersionWords } from "@clauseo/components/lib/words-versions.ts";
import {
  CompareBar,
  VersionBand,
  type BandLook,
  type BandNotice,
} from "@clauseo/components/clauseo/version-band.tsx";
import {
  Desk,
  DatesSheet,
  DraftSheet,
  SheetHead,
} from "@clauseo/components/clauseo/version-page.tsx";
import { VersionPanel, type Unsaved } from "@clauseo/components/clauseo/version-panel.tsx";
import {
  RestoreDialog,
  type EditedChoice,
  type RestoreAsk,
} from "@clauseo/components/clauseo/restore-dialog.tsx";
import { VersionWhere } from "@clauseo/components/clauseo/version-where.tsx";
import {
  asItStood,
  didOf,
  whoOf,
  type Version,
} from "@clauseo/components/clauseo/versions-model.ts";
import {
  largeNote,
  restoredLine,
  restoreWords,
  sheetLabel,
  undoWords,
  waitingLine,
} from "@clauseo/components/clauseo/versions-lines.ts";
import { diffParagraphs, diffRows } from "@clauseo/components/clauseo/versions-diff.ts";
import {
  CAL,
  DATES_REVS,
  DRAFT_REVS,
  HISTORY,
  ROWS,
  SHEET,
  UNSAVED,
  at,
  nameOf,
} from "./versions-data.ts";

const noop = () => {};

/** The timeline, its foot saying where the history is kept. The newest version is in view to start. */
export function TimelinePanel({
  history,
  unsaved = null,
}: {
  history: readonly Version[];
  unsaved?: Unsaved | null;
}) {
  const [here, setHere] = useState(history.length - 1);
  return (
    <VersionPanel
      history={history}
      calendar={CAL}
      name={nameOf}
      at={here}
      onPick={setHere}
      onClose={noop}
      unsaved={unsaved}
      where={<WhereFoot />}
    />
  );
}

/** Where the history lives: a line at the foot that opens the rest when pressed. */
export function WhereFoot() {
  const [open, setOpen] = useState(false);
  return (
    <VersionWhere
      platform="mac"
      folder="~/Library/Application Support/Clauseo/Versions"
      size="740 MB"
      settings="#settings"
      open={open}
      onOpenChange={setOpen}
      onShow={noop}
    />
  );
}

/** The band at now: the draft's bar, with what just happened or failed under it. */
export function BandNow({
  variant,
  busy = false,
}: {
  variant: "plain" | "restored" | "failed";
  busy?: boolean;
}) {
  const w = useVersionWords();
  const notices: BandNotice[] =
    variant === "restored"
      ? [
          {
            tone: "done",
            text: restoredLine(w.lang, CAL, at("v4"), 2),
            action: (
              <Button kind="quiet" size="small">
                {w.v("Undo")}
              </Button>
            ),
          },
        ]
      : variant === "failed"
        ? [
            {
              tone: "failed",
              text: UNSAVED.why,
              action: (
                <Button kind="tone" size="small">
                  {w.v("Try again")}
                </Button>
              ),
            },
          ]
        : [];
  return (
    <VersionBand
      look={null}
      draft={{ title: "Written statement", pages: 4 }}
      lead={
        <Button kind="quiet" size="small">
          <IconHistory size={16} />
          {w.v("Versions")}
        </Button>
      }
      notices={notices}
      waiting={busy ? waitingLine(w.lang, "Draft", "5:16 pm") : null}
      onCompare={noop}
      onRestore={noop}
      onUndo={noop}
      onBack={noop}
    />
  );
}

/** The band looking back at a version: the room as it stood, who made it and what it did, and the ways on. */
export function BandLooking({ id, busy = false }: { id: string; busy?: boolean }) {
  const w = useVersionWords();
  const [comparing, setComparing] = useState(false);
  const v = at(id);
  const look: BandLook = {
    line: asItStood(w.lang, CAL, v),
    who: whoOf(w.lang, CAL, v, HISTORY),
    did: didOf(w.lang, v, nameOf),
    comparing,
    undoable: v.who.kind !== "import",
    note: largeNote(w.lang, v, "200 MB"),
  };
  return (
    <VersionBand
      look={look}
      draft={{ title: "Written statement", pages: 4 }}
      waiting={busy ? waitingLine(w.lang, "Draft", "5:16 pm") : null}
      notices={[]}
      onCompare={() => setComparing((c) => !c)}
      onRestore={noop}
      onUndo={noop}
      onBack={noop}
    />
  );
}

/** While comparing: which of the two documents that differ from now is shown. */
export function CompareSample() {
  const [doc, setDoc] = useState("draft");
  return (
    <CompareBar
      docs={[
        { id: "draft", label: "Written statement" },
        { id: "dates", label: "List of dates" },
      ]}
      doc={doc}
      onDoc={setDoc}
      legend="Words that came in are underlined. Words that went out are struck through."
      unchanged="Paragraphs that read the same are not marked."
    />
  );
}

/** The draft as it stood at its first version, laid against now: the paragraphs that changed carry a bar. */
export function DraftDesk() {
  const { lang } = useVersionWords();
  const scroll = useRef<HTMLDivElement>(null);
  const label = sheetLabel(lang, CAL, "Written statement", at("v4"));
  return (
    <Desk label={label} scrollRef={scroll} className="ver-desk-compare">
      <SheetHead label={label} />
      <DraftSheet head={SHEET} diffs={diffParagraphs(DRAFT_REVS.P1, DRAFT_REVS.P4)} marked />
    </Desk>
  );
}

/** The list of dates as it stood then, laid against now: the rows that changed are marked. */
export function DatesDesk() {
  const { lang } = useVersionWords();
  const scroll = useRef<HTMLDivElement>(null);
  const label = sheetLabel(lang, CAL, "List of dates", at("v4"));
  return (
    <Desk label={label} scrollRef={scroll}>
      <SheetHead label={label} />
      <DatesSheet
        title="List of dates"
        rows={diffRows(ROWS, DATES_REVS.D1, DATES_REVS.D3)}
        marked
      />
    </Desk>
  );
}

type Mode = "restore" | "undo" | "word";

/** The dialog's ask for a mode: a restore to the draft's first version, a word file held open, or an undo of the
 * draft's latest change, with each change made since kept or undone by the lawyer. */
function askFor(mode: Mode, lang: Lang, kept: readonly string[]): RestoreAsk | null {
  if (mode === "word") return { kind: "word", act: "restore", file: "Written statement.docx" };
  if (mode === "restore") {
    const r = restoreWords(lang, CAL, HISTORY, "v4", nameOf);
    return r === null
      ? null
      : { kind: "confirm", act: "restore", ...r, edited: [], editedHead: null };
  }
  const u = undoWords(lang, CAL, HISTORY, "v8", kept, nameOf);
  if (u === null) return null;
  return {
    kind: "confirm",
    act: "undo",
    title: u.title,
    body: u.body,
    stays: null,
    editedHead: u.editedHead,
    edited: u.edited.map((e) => ({ ...e, choice: kept.includes(e.id) ? "keep" : "undo" })),
  };
}

/** A restore or an undo, opened from its trigger. The dialog stands closed until the trigger is pressed. */
export function RestoreTrigger({ mode }: { mode: Mode }) {
  const w = useVersionWords();
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState<Record<string, EditedChoice>>({ draft: "keep" });
  const kept = Object.keys(choice).filter((id) => choice[id] === "keep");
  return (
    <>
      <Button kind="tone" size="small" onClick={() => setOpen(true)}>
        <IconUndo size={16} />
        {w.v(mode === "undo" ? "Undo this change" : "Restore this version")}
      </Button>
      <RestoreDialog
        ask={open ? askFor(mode, w.lang, kept) : null}
        waiting={null}
        onClose={() => setOpen(false)}
        onChoice={(id, c) => setChoice((s) => ({ ...s, [id]: c }))}
        onRestore={() => setOpen(false)}
        onTryAgain={() => setOpen(false)}
      />
    </>
  );
}
