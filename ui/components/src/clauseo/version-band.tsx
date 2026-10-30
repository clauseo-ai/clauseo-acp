// The head of the work while Versions is in play. At now it is the turned page's own bar: the way into earlier
// versions at its left, the draft's chip, title and page at its right, and under it a plain line for whatever just
// happened or failed: a restore or an undo, with Undo beside it; a version that couldn't be saved, with Try again; a
// new draft saved. Looking back, it says plainly when the room stands, "The matter as it stood at 3:42 pm on 14
// October", who made that version and what it touched, and offers the ways on: Compare with now, Restore this
// version, Undo this change, Back to now. While an answer runs, Restore and Undo wait and the band says until when.
import type { ReactNode, RefObject } from "react";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { SegmentedSwitch } from "../ui/segmented.tsx";
import {
  IconAccept,
  IconBackToAnswer,
  IconChecking,
  IconOpen,
  IconPaperBookPage,
  IconUndo,
} from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import { Dot, Say } from "./say.tsx";

export type BandLook = {
  /** "The matter as it stood at 3:42 pm on 14 October". */
  line: string;
  /** "Clauseo, in Draft". */
  who: string;
  /** "Made Draft reply." */
  did: string;
  comparing: boolean;
  /** Whether this version's own change can be undone: a change by Clauseo or by the lawyer, not an import. */
  undoable: boolean;
  /** A plain note on this version, such as a file too large to keep versions. */
  note?: string | null;
};

/** A line under the bar at now: what just happened, or what failed, said plainly, with its one way on. */
export type BandNotice = { tone: "done" | "failed"; text: string; action?: ReactNode };

/** A quiet act that waits while an answer runs: faint, aria-disabled, still focusable. */
export function WaitingButton({
  waiting,
  className,
  onClick,
  children,
  kind = "tone",
  label,
}: {
  waiting: boolean;
  className?: string;
  onClick: () => void;
  children: ReactNode;
  kind?: "tone" | "quiet";
  label?: string;
}) {
  return (
    <Button
      kind={kind}
      size="small"
      className={cn(className)}
      aria-label={label}
      aria-disabled={waiting ? true : undefined}
      data-disabled={waiting ? "" : undefined}
      onClick={() => !waiting && onClick()}
    >
      {children}
    </Button>
  );
}

export function VersionBand({
  look,
  draft,
  lead,
  lookLead,
  notices = [],
  waiting,
  backRef,
  onCompare,
  onRestore,
  onUndo,
  onBack,
}: {
  look: BandLook | null;
  /** At now, the paper the bar is over: its title as filed and how many pages it runs to. */
  draft: { title: string; pages: number };
  /** At now, the way into Versions, at the bar's left. */
  lead?: ReactNode;
  /** Looking back, on a mid window with the panel put away: the way back into it, at the meta row's left. */
  lookLead?: ReactNode;
  /** At now: what just happened or failed, a line each. */
  notices?: readonly BandNotice[];
  /** While an answer runs: "Restore and Undo wait until ... expected by 5:16 pm." */
  waiting: string | null;
  backRef?: RefObject<HTMLButtonElement | null>;
  onCompare: () => void;
  onRestore: () => void;
  onUndo: () => void;
  onBack: () => void;
}) {
  const w = useVersionWords();
  if (look === null) {
    return (
      <>
        <div className="turn-bar ver-bar">
          {lead}
          <p className="turn-what">
            <Chip role="prepared" />
            <b>
              <Say>{draft.title}</Say>
            </b>
            <span className="turn-page">{w.t("Page 1 of {n}", { n: draft.pages })}</span>
          </p>
        </div>
        {notices.map((n) => (
          <p key={n.text} className="ver-notice" data-tone={n.tone} role="status">
            {n.tone === "done" ? (
              <IconAccept size={14} className="ver-notice-icon" />
            ) : (
              <IconPaperBookPage size={14} className="ver-notice-icon" />
            )}
            <span className="ver-notice-text">
              <Say>{n.text}</Say>
            </span>
            {n.action}
          </p>
        ))}
        {waiting !== null && notices.length > 0 && (
          <p className="ver-notice ver-notice-wait">
            <IconChecking size={14} className="ver-notice-icon" />
            <span className="ver-notice-text">
              <Say>{waiting}</Say>
            </span>
          </p>
        )}
      </>
    );
  }
  return (
    <div className="ver-band">
      <p className="ver-line" aria-live="polite">
        <Say>{look.line}</Say>
      </p>
      <div className="ver-band-row">
        {lookLead}
        <p className="ver-meta">
          <span>
            <Say>{look.who}</Say>
          </span>
          {look.did !== "" && (
            <span className="ver-meta-did">
              <Dot />
              <span>
                <Say>{look.did}</Say>
              </span>
            </span>
          )}
        </p>
        {/* key caps stand on inline prompts only: Escape is named to assistive technology and under the pointer */}
        <Button
          kind="quiet"
          size="small"
          className="ver-back"
          ref={backRef}
          aria-keyshortcuts="Escape"
          title="Esc"
          onClick={onBack}
        >
          <IconBackToAnswer size={16} />
          {w.v("Back to now")}
        </Button>
      </div>
      <div className="ver-band-row">
        <div className="ver-actions">
          <Button
            kind="tone"
            size="small"
            aria-pressed={look.comparing}
            className="ver-compare"
            onClick={onCompare}
          >
            <IconOpen size={16} />
            {w.v("Compare with now")}
          </Button>
          <WaitingButton waiting={waiting !== null} className="ver-restore" onClick={onRestore}>
            <IconUndo size={16} />
            {w.v("Restore this version")}
          </WaitingButton>
          {look.undoable && (
            <WaitingButton
              waiting={waiting !== null}
              kind="quiet"
              className="ver-undo-change"
              onClick={onUndo}
            >
              {w.v("Undo this change")}
            </WaitingButton>
          )}
        </div>
      </div>
      {look.note != null && (
        <p className="ver-wait ver-note">
          <IconPaperBookPage size={14} className="ver-wait-icon" />
          <span>
            <Say>{look.note}</Say>
          </span>
        </p>
      )}
      {waiting !== null && (
        <p className="ver-wait">
          <IconChecking size={14} className="ver-wait-icon" />
          <span>
            <Say>{waiting}</Say>
          </span>
        </p>
      )}
    </div>
  );
}

/** While comparing: which document, among those that differ from now, and how to read the marks. */
export function CompareBar({
  docs,
  doc,
  onDoc,
  legend,
  unchanged,
}: {
  docs: readonly { id: string; label: string }[];
  doc: string;
  onDoc: (d: string) => void;
  legend: string;
  unchanged: string;
}) {
  const w = useVersionWords();
  return (
    <div className="ver-compare-bar">
      {docs.length > 0 && (
        <SegmentedSwitch<string>
          label={w.v("Documents that differ from now")}
          value={doc}
          onValueChange={onDoc}
          options={docs.map((d) => ({ value: d.id, label: d.label }))}
        />
      )}
      <p className="ver-legend">
        {legend !== "" && <Say>{legend}</Say>}
        <span className="ver-unchanged">
          <Say>{unchanged}</Say>
        </span>
      </p>
    </div>
  );
}
