// The first run's own small pieces (design/03-system/src/pages/first-launch/parts.tsx and gate.tsx): a thing done, in
// place; the fine print up front; and the rows of the gate in the composer's well, the act with its words and its
// button where Ask will stand, the quiet ways round it beneath, and the one line the lawyer pastes in.
import { useState, type ReactNode } from "react";
import { IconPaperBookPage, IconTick } from "../../icons/icons.tsx";
import { cn } from "../../lib/cn.ts";
import { useFirstLaunchWords } from "../../lib/words-first-launch.ts";
import { Button } from "../../ui/button.tsx";
import { PlanMeter } from "../../ui/meter.tsx";
import { Say } from "../say.tsx";

/** A thing finished, in place: the tick in primary, never green, which means checked, and the words. */
export function Done({
  children,
  className,
  testId,
}: {
  children: ReactNode;
  className?: string | undefined;
  testId?: string | undefined;
}) {
  return (
    <p className={cn("flex items-start gap-s2", className)} role="status" data-testid={testId}>
      <IconTick size={16} className="flex-none fl-done-tick" />
      <span className="min-w-0 deva-nudge">{children}</span>
    </p>
  );
}

/** The fine print, up front: what stays on this computer and what the AI company sees. Its glyph is the paper's, so
 * the lock stays the Safe workspace's alone. */
export function DocumentsLine({ name }: { name: string }) {
  const w = useFirstLaunchWords();
  return (
    <>
      <IconPaperBookPage size={14} className="flex-none" />
      <span>
        <Say>
          {w("Your papers stay on this computer. {name} sees what each question sends it.", {
            name,
          })}
        </Say>
      </span>
    </>
  );
}

/** The act's row: its words, and the button where Ask will stand. dot: the act waits for the lawyer's hand in the
 * browser, so it takes court yellow's dot as the rail's question does. */
export function Act({
  children,
  act,
  dot = false,
}: {
  children: ReactNode;
  act?: ReactNode;
  dot?: boolean;
}) {
  return (
    <div className="fl-gate-act">
      <span className="fl-gate-words" role="status">
        {dot && <i aria-hidden="true" className="fl-gate-dot" />}
        <span className="deva-nudge">{children}</span>
      </span>
      {act}
    </div>
  );
}

/** The quiet row under the act. */
export function Ways({ children }: { children: ReactNode }) {
  return <div className="fl-gate-ways">{children}</div>;
}

/** A few muted words in the quiet row, before its buttons. */
export function Note({ children }: { children: string }) {
  return (
    <span className="fl-gate-note">
      <Say>{children}</Say>
    </span>
  );
}

/** One line of text the lawyer pastes in: a code from the browser, or an API key. It is handed on, never kept: the
 * field is the only place it stands, and it clears once it is taken (at once, when `onGo` promises nothing). */
export function PasteRow({
  label,
  secret,
  go,
  onGo,
  cancel,
  testId,
}: {
  label: string;
  secret: boolean;
  go: string;
  /** Hands the words on. A promise says whether desktop took them: the field keeps them until it settles, cleared
   * once taken and kept after a refusal, so a retry needs no second paste; Send waits meanwhile. */
  onGo: (value: string) => void | Promise<boolean>;
  cancel?: ReactNode;
  testId?: string | undefined;
}) {
  const [value, setValue] = useState("");
  const [sending, setSending] = useState(false);
  const send = () => {
    const v = value.trim();
    if (v === "" || sending) return;
    const taken = onGo(v);
    if (taken === undefined) {
      setValue("");
      return;
    }
    setSending(true);
    void taken.then(
      (ok) => {
        setSending(false);
        if (ok) setValue("");
      },
      () => setSending(false),
    );
  };
  return (
    <div className="fl-gate-act">
      <input
        className="fl-gate-input deva-nudge-field"
        type={secret ? "password" : "text"}
        lang="en"
        aria-label={label}
        placeholder={label}
        value={value}
        autoComplete="off"
        spellCheck={false}
        data-testid={testId}
        onChange={(e) => setValue(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          send();
        }}
      />
      <Button
        kind="tone"
        className="room-field-ask"
        disabled={value.trim() === "" || sending}
        onClick={send}
      >
        {go}
      </Button>
      {cancel}
    </div>
  );
}

/** The bar: received of total, primary on the inset tone. Assistive technology hears the words, never a number. */
export function InstallBar({
  received,
  total,
  words,
}: {
  received: number;
  total: number;
  words: string;
}) {
  // to a tenth of a percent: a first byte's share would read 5e-7, which ARIA's aria-valuenow refuses
  const value = total > 0 ? Math.min(100, Math.round((received / total) * 1000) / 10) : 0;
  return (
    <PlanMeter
      className="fl-bar"
      name={<span className="sr-only">{words}</span>}
      value={value}
      valueText={words}
      status={null}
    />
  );
}
