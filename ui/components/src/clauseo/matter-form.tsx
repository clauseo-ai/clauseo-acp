// The new matter form. A name as it stands on the cause list, then a client if any and the next date
// if fixed side by side, since the rail groups matters by their next date, and the folder Clauseo makes for it under
// Documents as one muted line with a quiet Change. The lawyer's words take the composer's setting, 17 px at 600. First
// launch opens it on the first screen, and the rail's New matter opens the same form. The folder's path is the
// computer's own, in English in both languages.
import { useEffect, useId, useRef, useState } from "react";
import { Field } from "@base-ui/react/field";
import { Input } from "@base-ui/react/input";
import { Button } from "../ui/button.tsx";
import { fieldLabel, textControl } from "../ui/recipes-controls.ts";
import { IconFolder } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useDisplay } from "../lib/display.ts";
import { mt } from "../lib/words-matters.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { nextDateError, parseNextDate } from "./matter-form-check.ts";
import { Heading } from "./heading.tsx";
import { Say } from "./say.tsx";

export type NewMatterFormProps = {
  /** The folder Clauseo would make for a matter of this name, such as "Documents/Clauseo/Acme Ltd v. State"; null,
   * or no folderFor at all, when there is none to show. */
  folderFor?: ((name: string) => string | null) | undefined;
  /** Optional: the folder line's path, given outright; null shows no folder words, Change alone in the row. A folder
   * Change picked shows over either. Left out, folderFor says it. */
  folder?: string | null | undefined;
  /** Whether the client and next date show. Default true; a host that can't keep them yet passes false. */
  details?: boolean | undefined;
  /** What the fields hold when the form opens. */
  name?: string;
  client?: string;
  /** Optional: the next date, as the lawyer writes it, "30 October 2026". */
  next?: string;
  /** Create the matter, pressed with a name: the words trimmed, the next date as typed and as a calendar date
   * ("2026-10-30", or null when there is none). A callback taking fewer arguments still works. Given a promise, Create
   * stands pressed and can't be pressed again until it settles; a rejected one is the caller's to say, by error. */
  onCreate: (name: string, client: string, next: string, nextDate: string | null) => unknown;
  /** Back, where there is somewhere to go back to. */
  onCancel?: () => void;
  /** Optional: Change, pressed, opens the computer's own folder picker; it gives the folder chosen, as the folder
   * line shows it, or null if none was, which keeps what was there. It may answer later: Change waits for it. Without
   * it, Change stands for the picker and keeps the folder Clauseo would make. */
  pickFolder?: ((current: string) => Promise<string | null> | string | null) | undefined;
  /** Optional: a line under the fields, from the caller, in the language: a folder refused, a matter not made. */
  error?: string | null | undefined;
  /** Optional: the next date's line, from the caller. Left out, the form reads the date itself and, where it doesn't
   * read, says how to write it and holds Create back. */
  nextError?: string | null | undefined;
  /** Optional: for a line that names a matter on the list (matter.folder_is_matter), Open it sits under the line and
   * calls this, which opens that matter. Left out, the line has no button. */
  openMatter?: (() => void) | undefined;
  /** Optional: test ids for the name field, Change, Create the matter and Open it. */
  testIds?: { name?: string; existing?: string; create?: string; open?: string } | undefined;
  /** Whether the form carries its own heading, "A new matter". */
  titled?: boolean;
  /** Optional: in a dialog, the form's buttons stand as a dialog's do, at its end with the one
   * that acts last, and the way out is Cancel rather than Back. */
  inDialog?: boolean;
};

/** One text field on the work's surface: its label, then the lawyer's own words at the composer's 17 px and 600. Names
 * and dates stay in English, as filed. */
function TextInput({
  label,
  value,
  onChange,
  placeholder,
  className,
  error,
  testId,
  onBlur,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onBlur?: () => void;
  placeholder?: string;
  className?: string;
  error?: string | null;
  testId?: string | undefined;
}) {
  const id = useId();
  const wrong = error !== undefined && error !== null;
  return (
    <Field.Root className={cn("grid gap-s1-5 min-w-0", className)}>
      <Field.Label className={fieldLabel}>
        <Say>{label}</Say>
      </Field.Label>
      <Field.Control
        render={<Input />}
        value={value}
        lang="en"
        autoComplete="off"
        spellCheck={false}
        placeholder={placeholder}
        aria-invalid={wrong ? true : undefined}
        aria-describedby={wrong ? id : undefined}
        data-testid={testId}
        onChange={(e) => onChange(e.currentTarget.value)}
        onBlur={onBlur}
        className={cn(
          textControl,
          "matter-form-input font-en text-field font-accepted h-auto min-h-control py-s2",
        )}
      />
      {wrong && (
        <p id={id} className="matter-form-error">
          <Say>{error}</Say>
        </p>
      )}
    </Field.Root>
  );
}

/** The form. Create the matter waits, faint, until the matter has a name. */
export function NewMatterForm({
  folderFor,
  folder: given,
  details = true,
  name: name0 = "",
  client: client0 = "",
  next: next0 = "",
  onCreate,
  onCancel,
  pickFolder,
  error,
  nextError,
  openMatter,
  testIds,
  titled = true,
  inDialog = false,
}: NewMatterFormProps) {
  const w = useMatterFormWords();
  const { lang } = useDisplay();
  const [name, setName] = useState(name0);
  const [client, setClient] = useState(client0);
  const [next, setNext] = useState(next0);
  const [picked, setPicked] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const [pending, setPending] = useState(false);
  // the next date's line waits until the lawyer leaves the field or tries Create, so a date half typed isn't wrong
  const [dateLeft, setDateLeft] = useState(false);
  const folderId = useId();
  // a promise that settles after the form has gone sets nothing
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const dateLine = !details
    ? null
    : nextError !== undefined
      ? nextError
      : nextDateError(lang, next);
  const ready = name.trim() !== "" && dateLine === null;
  const dateShown = dateLeft ? dateLine : null;
  const folder =
    picked ?? (given !== undefined ? given : (folderFor?.(name.trim() || name0) ?? null));
  const change = async () => {
    if (pickFolder === undefined || picking) return;
    setPicking(true);
    try {
      const chosen = await pickFolder(folder ?? "");
      if (live.current && chosen !== null) setPicked(chosen);
    } catch {
      // the picker failed: the folder stays as it was
    } finally {
      if (live.current) setPicking(false);
    }
  };
  const create = async () => {
    if (!ready || pending) {
      setDateLeft(true);
      return;
    }
    const typed = details ? next.trim() : "";
    const made = onCreate(name.trim(), details ? client.trim() : "", typed, parseNextDate(typed));
    if (!(made instanceof Promise)) return;
    setPending(true);
    try {
      await made;
    } catch {
      // the caller says what went wrong, by error
    } finally {
      if (live.current) setPending(false);
    }
  };
  const createButton = (
    <Button
      kind="tone"
      type="submit"
      disabled={!ready || pending}
      aria-busy={pending ? true : undefined}
      data-pending={pending ? "" : undefined}
      data-testid={testIds?.create}
    >
      <span className="deva-nudge">{w("Create the matter")}</span>
    </Button>
  );
  return (
    <form
      className="matter-form"
      onSubmit={(e) => {
        e.preventDefault();
        void create();
      }}
    >
      {titled && (
        <Heading level={2} className="matter-form-title">
          {w("A new matter")}
        </Heading>
      )}
      <TextInput
        label={w("Name, as it stands on the cause list")}
        value={name}
        onChange={setName}
        testId={testIds?.name}
      />
      {details && (
        <div className="matter-form-pair">
          <TextInput label={w("Client, if any")} value={client} onChange={setClient} />
          <TextInput
            label={w("Next date, if fixed")}
            value={next}
            onChange={setNext}
            placeholder={w("30 October 2026")}
            error={dateShown}
            onBlur={() => setDateLeft(true)}
          />
        </div>
      )}
      <div
        className="matter-form-folder"
        {...(folder !== null && picked === null
          ? { title: w("Clauseo makes this folder. Add papers to it any time.") }
          : {})}
      >
        {/* with no folder to name, Change stands alone: no icon beside nothing */}
        {folder !== null && <IconFolder size={16} className="matter-form-folder-icon" />}
        {/* what Change changes: the folder line, or where there is none, its name for a screen reader */}
        <p id={folderId} className="matter-form-folder-words">
          {folder !== null ? (
            <>
              <Say>{w("Folder:")}</Say>{" "}
              {/* a path is isolated, so a name with right-to-left marks can't reorder the line around it */}
              <bdi lang="en" className="matter-form-path">
                {folder}
              </bdi>
            </>
          ) : (
            <span className="sr-only">{w("The matter's folder")}</span>
          )}
        </p>
        <Button
          kind="quiet"
          size="small"
          className="matter-form-change"
          disabled={picking}
          aria-describedby={folderId}
          data-testid={testIds?.existing}
          onClick={() => void change()}
        >
          <span className="deva-nudge">{w("Change")}</span>
        </Button>
      </div>
      {error !== undefined && error !== null && (
        <div className="matter-form-refusal">
          <p role="alert" className="matter-form-error">
            <Say>{error}</Say>
          </p>
          {openMatter !== undefined && (
            <Button
              kind="quiet"
              size="small"
              className="matter-form-open"
              data-testid={testIds?.open}
              onClick={() => openMatter()}
            >
              <span className="deva-nudge">{mt(lang, "Open it")}</span>
            </Button>
          )}
        </div>
      )}
      {inDialog ? (
        <span className="flex flex-wrap justify-end gap-s2 mt-s2">
          {onCancel !== undefined && (
            <Button kind="quiet" onClick={onCancel}>
              <span className="deva-nudge">{w("Cancel")}</span>
            </Button>
          )}
          {createButton}
        </span>
      ) : (
        <span className="flex flex-wrap gap-s2 mt-s2">
          {createButton}
          {onCancel !== undefined && (
            <Button kind="quiet" onClick={onCancel}>
              <span className="deva-nudge">{w("Back")}</span>
            </Button>
          )}
        </span>
      )}
    </form>
  );
}
