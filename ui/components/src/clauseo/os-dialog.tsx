// The computer's own dialog at the edge of the matter. When the lawyer allows Clauseo past the matter's folder, the
// host asks the computer to confirm, in a dialog the computer draws itself, so nothing inside the window can answer it
// for the lawyer. On the Mac it is macOS 26's alert: the app's icon, the question in bold, the risk put both ways, a
// box to remember the answer and two capsule buttons. On Windows it is a window of its own: Clauseo's title bar, the
// question, the risk both ways, and a foot holding the box and the buttons. Each draws in its system's own type and
// colour, which are the system's marks and never Clauseo's. Clauseo writes only the words (edgeDialogWords in
// words-versions.ts).
import { useEffect, useId, useRef, type KeyboardEvent } from "react";
import iconMac from "../assets/icon.svg";
import iconWin from "../assets/icon-windows.svg";
import { IconTick } from "../icons/icons.tsx";
import type { Platform } from "../lib/display.ts";
import { Say } from "./say.tsx";

export type OsDialogProps = {
  platform: Platform;
  /** The question, in the computer's own bold: "Allow Clauseo to read the folder 'Precedents'?" */
  title: string;
  /** Where it reaches, as the computer writes it, under the question. */
  detail?: string | null;
  /** The risk, put both ways: what allowing does, and what refusing does. */
  lines: readonly [string, string];
  /** The box that remembers the answer for the matter, ticked unless the lawyer clears it. */
  remember?: { label: string; checked: boolean; onChange: (checked: boolean) => void } | null;
  /** A line under the risk, where there's no box. */
  foot?: string | null;
  confirm: string;
  cancel: string;
  /** Which button the system makes the default, in its own accent: allowing a place, or keeping protection. */
  primary: "confirm" | "cancel";
  onConfirm: () => void;
  onCancel: () => void;
};

/** The computer's own dialog, drawn over the window where the system puts it. Return answers with the default
 * button and Escape with Cancel's, as both systems do. */
export function OsDialog(p: OsDialogProps) {
  const id = useId();
  const primaryRef = useRef<HTMLButtonElement>(null);
  useEffect(() => primaryRef.current?.focus(), []);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      p.onCancel();
    }
  };
  const confirm = (
    <button
      type="button"
      className="sysd-button"
      data-primary={p.primary === "confirm" ? "" : undefined}
      ref={p.primary === "confirm" ? primaryRef : undefined}
      onClick={p.onConfirm}
    >
      <Say>{p.confirm}</Say>
    </button>
  );
  const cancel = (
    <button
      type="button"
      className="sysd-button"
      data-primary={p.primary === "cancel" ? "" : undefined}
      ref={p.primary === "cancel" ? primaryRef : undefined}
      onClick={p.onCancel}
    >
      <Say>{p.cancel}</Say>
    </button>
  );
  const box =
    p.remember == null ? null : (
      <label className="sysd-check">
        <input
          type="checkbox"
          className="sysd-check-input"
          checked={p.remember.checked}
          onChange={(e) => p.remember?.onChange(e.currentTarget.checked)}
        />
        <span className="sysd-check-box" aria-hidden="true">
          <IconTick size={12} />
        </span>
        <span>
          <Say>{p.remember.label}</Say>
        </span>
      </label>
    );
  const words = (
    <>
      <h2 className="sysd-title" id={`${id}-t`}>
        <Say>{p.title}</Say>
      </h2>
      {p.detail != null && (
        <p className="sysd-detail" lang="en">
          {p.detail}
        </p>
      )}
      <div className="sysd-lines" id={`${id}-d`}>
        <p>
          <Say>{p.lines[0]}</Say>
        </p>
        <p>
          <Say>{p.lines[1]}</Say>
        </p>
        {p.foot != null && (
          <p className="sysd-foot-line">
            <Say>{p.foot}</Say>
          </p>
        )}
      </div>
    </>
  );

  if (p.platform === "mac") {
    return (
      <div className="sysd-place">
        <div
          className="sysd-dialog sysd-mac"
          role="alertdialog"
          aria-modal="true"
          aria-labelledby={`${id}-t`}
          aria-describedby={`${id}-d`}
          onKeyDown={onKey}
        >
          <img className="sysd-mac-icon" src={iconMac} alt="" />
          {words}
          {box}
          {/* macOS sets the default button last, at the right */}
          <div className="sysd-buttons">
            {p.primary === "confirm" ? cancel : confirm}
            {p.primary === "confirm" ? confirm : cancel}
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="sysd-place">
      <div
        className="sysd-dialog sysd-win"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={`${id}-t`}
        aria-describedby={`${id}-d`}
        onKeyDown={onKey}
      >
        <div className="sysd-win-bar">
          <img className="sysd-win-icon" src={iconWin} alt="" />
          <span className="sysd-win-name" lang="en">
            Clauseo
          </span>
          <button
            type="button"
            className="sysd-win-close"
            aria-label={p.cancel}
            tabIndex={-1}
            onClick={p.onCancel}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true" className="sysd-win-glyph">
              <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
            </svg>
          </button>
        </div>
        <div className="sysd-win-body">{words}</div>
        <div className="sysd-win-foot">
          {box}
          {/* Windows sets the default button first, at the left */}
          <div className="sysd-buttons">
            {p.primary === "confirm" ? confirm : cancel}
            {p.primary === "confirm" ? cancel : confirm}
          </div>
        </div>
      </div>
    </div>
  );
}
