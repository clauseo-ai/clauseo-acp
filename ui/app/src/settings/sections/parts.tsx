// The settings sections' own building blocks (design/03-system/src/pages/settings/parts.tsx): a group is a section head
// over a plate set into the work's surface, on the well tone, its rows parted by a hairline of inset shadow; a choice
// row puts a few short words in a segmented switch at the row's end. A row a firm's policy locks says so.
import { useId, type ReactNode } from "react";
import { cn } from "@clauseo/components/lib/cn.ts";
import { Heading } from "@clauseo/components/clauseo/heading.tsx";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";

/** A group of settings: its head in the section head's voice, an optional line, and what it holds. */
export function Group({
  title,
  note,
  children,
  plate = true,
}: {
  /** The head; a group whose one setting names itself goes without. */
  title?: string;
  note?: string;
  children: ReactNode;
  /** Whether the rows stand on a plate. */
  plate?: boolean;
}) {
  const w = useSettingsWords();
  const id = useId();
  return (
    <section aria-labelledby={title === undefined ? undefined : id} className="set-group">
      {title !== undefined && (
        <div className="set-group-head">
          <Heading level={3} id={id} className="set-group-title">
            {w.say(title)}
          </Heading>
        </div>
      )}
      {note !== undefined && <p className="set-group-note">{w.say(note)}</p>}
      {plate ? <div className="set-plate">{children}</div> : children}
    </section>
  );
}

/** A row on a plate; a disabled one greys its labels (settingLabel's in-data-disabled). */
export function PlateRow({
  children,
  className,
  disabled = false,
}: {
  children: ReactNode;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <div className={cn("set-row", className)} data-disabled={disabled ? "" : undefined}>
      {children}
    </div>
  );
}

/** The line a locked row carries: the firm's policy holds it. Its control names `id` in aria-describedby. */
export function Managed({ id, className }: { id: string; className?: string }) {
  const w = useSettingsWords();
  return (
    <p id={id} className={cn(settingNote, className)} data-testid="managed">
      {w.say(w.s("Managed by your organisation"))}
    </p>
  );
}

/** A setting chosen from a few short words: its name and line at the start, the control at the end, dropping under
 * the words once they'd have too little room. A locked one says so under its line, as `managedId`, which its control
 * names in aria-describedby. */
export function ChoiceRow({
  label,
  note,
  locked = false,
  managedId,
  children,
}: {
  label: string;
  note?: string;
  locked?: boolean;
  /** The managed line's id, for the control's aria-describedby. */
  managedId: string;
  children: ReactNode;
}) {
  const w = useSettingsWords();
  return (
    <PlateRow className="set-choice" disabled={locked}>
      <div className="set-choice-words">
        <p className={settingLabel}>{w.say(label)}</p>
        {note !== undefined && <p className={settingNote}>{w.say(note)}</p>}
        {locked && <Managed id={managedId} />}
      </div>
      {children}
    </PlateRow>
  );
}
