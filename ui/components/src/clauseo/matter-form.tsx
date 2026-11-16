// The new matter form: a matter is a name, a client if there is one, and a folder. "Matter name", "Client (optional)",
// then the Folder row (matter-form-folder.tsx): the chosen path, Choose folder… that opens the computer's own folder
// picker at once, and, where the host has a place for new matters, "Use a new folder in ~/Documents/Clauseo". Every
// folder comes through the picker or that offer, never typed, since picking it in the computer's own panel is what lets
// Clauseo read it. Create matter waits until there's a name and a folder; tried before (Enter), the field that's
// missing says so under it. The lawyer's words take the composer's setting, 17 px at 600. First launch opens the form on
// its first screen, and the rail's New matter opens the same form in a dialog. A warning about the folder picked, from
// the caller, sits under the folder row, its two buttons in Create's place; a refusal sits there too, with its one way
// on: Open {name} for a folder inside or holding a matter on the list, Choose another folder for any other folder
// refused.
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Field } from "@base-ui/react/field";
import { Input } from "@base-ui/react/input";
import { Button } from "../ui/button.tsx";
import { fieldLabel, textControl } from "../ui/recipes-controls.ts";
import { isolate } from "../lib/bidi.ts";
import { cn } from "../lib/cn.ts";
import { useDisplay } from "../lib/display.ts";
import { fr } from "../lib/words-folder.ts";
import { mt } from "../lib/words-matters.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { homeShown, under } from "./folder-path.ts";
import { FolderRow } from "./matter-form-folder.tsx";
import { Heading } from "./heading.tsx";
import { PickerLine } from "./picker-line.tsx";
import { Say } from "./say.tsx";

/** The folder the form holds: one the picker gave, as the desktop writes it; the new one under the host's place for
 * new matters; or none yet. */
type Choice = { kind: "picked"; display: string } | { kind: "made" } | null;

export type NewMatterFormProps = {
  /** Whether the client shows. Default true; a host that can't keep it yet passes false. */
  details?: boolean | undefined;
  /** What the fields hold when the form opens. */
  name?: string;
  client?: string;
  /** Optional: a folder chosen already as the form opens, as the desktop writes it ("Documents/Acme"). */
  folder?: string | null | undefined;
  /** Optional: where the host makes a new matter's folder, as the desktop writes it ("Documents/Clauseo"). Given, the
   * form offers "Use a new folder in ~/Documents/Clauseo"; null or left out, it offers no such folder. */
  location?: string | null | undefined;
  /** Create the matter, pressed with a name and a folder: the words trimmed, and whether the folder is the new one
   * under `location` (true) or the one the picker gave (false). Given a promise, Create stands pressed and can't be
   * pressed again until it settles; a rejected one is the caller's to say, by error. */
  onCreate: (name: string, client: string, made: boolean) => unknown;
  /** Back, where there is somewhere to go back to. */
  onCancel?: () => void;
  /** Choose folder…, pressed, opens the computer's own folder picker; it gives the folder chosen, as the desktop writes
   * it, or null if none was, which keeps what was there. It may answer later: the button waits for it. */
  pickFolder?: (() => Promise<string | null> | string | null) | undefined;
  /** Optional: "Use a new folder in" was chosen, so the caller drops the folder the picker gave, and its checks. */
  onMade?: (() => void) | undefined;
  /** Optional: the folder picker is on its way (its pick has run 300 ms, the caller's wait): a muted line under the
   * folder row says so, and goes when the pick resolves. */
  opening?: boolean | undefined;
  /** Optional: a line under the folder row, from the caller, in the language: a folder refused, a matter not made. */
  error?: string | null | undefined;
  /** Optional: the line is about the folder: Choose another folder sits under it and opens the picker. */
  folderError?: boolean | undefined;
  /** Optional: for a line that names a matter on the list (a folder inside or holding its folder), Open {name} sits
   * under the line, the name isolated, and calls `open`, which opens that matter, in Choose another folder's place. */
  openMatter?: { name: string; open: () => void } | undefined;
  /** Optional: a warning about the folder picked, under the folder row, from the caller in the language. While it
   * shows, its two buttons stand in Create's place: `choose`, the main one, calls `onChoose` or, left out, opens the
   * picker as Choose folder… does; `anyway` creates as Create does. */
  warning?:
    | { note: ReactNode; choose: string; anyway: string; onChoose?: (() => void) | undefined }
    | undefined;
  /** Optional: Create held back, for a folder the caller refused. */
  blocked?: boolean | undefined;
  /** Optional: the scan of the folder picked, under the folder row and above any warning, from the caller. */
  scan?: ReactNode;
  /** Optional: a count the caller bumps to drop the folder the picker gave (the scan's Stop), so the row goes back to
   * none. */
  unpick?: number | undefined;
  /** Optional: test ids for the name field, Choose folder…, Use a new folder, Create matter and Open it, and the
   * warning's buttons. */
  testIds?:
    | {
        name?: string;
        existing?: string;
        made?: string;
        create?: string;
        open?: string;
        choose?: string;
        anyway?: string;
      }
    | undefined;
  /** Whether the form carries its own heading, "A new matter". */
  titled?: boolean;
  /** Optional: in a dialog, the form's buttons stand as a dialog's do, at its end with the one that acts last, and the
   * way out is Cancel rather than Back. */
  inDialog?: boolean;
};

/** One text field on the work's surface: its label, then the lawyer's own words at the composer's 17 px and 600. Names
 * stay in English, as filed. */
function TextInput({
  label,
  value,
  onChange,
  error,
  testId,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  error?: string | null;
  testId?: string | undefined;
}) {
  const id = useId();
  const wrong = error !== undefined && error !== null;
  return (
    <Field.Root className="grid gap-s1-5 min-w-0">
      <Field.Label className={fieldLabel}>
        <Say>{label}</Say>
      </Field.Label>
      <Field.Control
        render={<Input />}
        value={value}
        lang="en"
        autoComplete="off"
        spellCheck={false}
        aria-invalid={wrong ? true : undefined}
        aria-describedby={wrong ? id : undefined}
        data-testid={testId}
        onChange={(e) => onChange(e.currentTarget.value)}
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

/** The form. Create matter waits, faint, until the matter has a name and a folder. */
export function NewMatterForm({
  details = true,
  name: name0 = "",
  client: client0 = "",
  folder: folder0,
  location,
  onCreate,
  onCancel,
  pickFolder,
  onMade,
  error,
  folderError = false,
  openMatter,
  warning,
  blocked = false,
  scan,
  unpick = 0,
  opening = false,
  testIds,
  titled = true,
  inDialog = false,
}: NewMatterFormProps) {
  const w = useMatterFormWords();
  const { lang } = useDisplay();
  const [name, setName] = useState(name0);
  const [client, setClient] = useState(client0);
  const [choice, setChoice] = useState<Choice>(
    folder0 === undefined || folder0 === null ? null : { kind: "picked", display: folder0 },
  );
  // the caller dropped the pick: set while rendering, as React allows for state that follows a prop
  const [unpicked, setUnpicked] = useState(unpick);
  if (unpicked !== unpick) {
    setUnpicked(unpick);
    if (choice?.kind === "picked") setChoice(null);
  }
  const [picking, setPicking] = useState(false);
  const [pending, setPending] = useState(false);
  // Create tried before the form was ready: the missing field says so under it, until it's filled
  const [tried, setTried] = useState(false);
  const errorId = useId();
  // a promise that settles after the form has gone sets nothing
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const place = location ?? null;
  // a made folder needs the place the host makes it in
  const held = choice?.kind === "made" && place === null ? null : choice;
  const named = name.trim() !== "";
  const ready = named && held !== null && !blocked;
  const shown =
    held === null
      ? null
      : held.kind === "picked"
        ? homeShown(held.display)
        : under(homeShown(place ?? ""), name.trim() === "" ? "…" : name.trim());
  const choose = async () => {
    if (pickFolder === undefined || picking) return;
    setPicking(true);
    try {
      const chosen = await pickFolder();
      if (live.current && chosen !== null) setChoice({ kind: "picked", display: chosen });
    } catch {
      // the picker failed: the folder stays as it was
    } finally {
      if (live.current) setPicking(false);
    }
  };
  const create = async () => {
    if (!ready || pending) {
      setTried(true);
      return;
    }
    const made = onCreate(
      name.trim(),
      details ? client.trim() : "",
      held !== null && held.kind === "made",
    );
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
  const hasError = error !== undefined && error !== null;
  const createButton = (
    <Button
      kind={warning === undefined ? "primary" : "quiet"}
      type="submit"
      disabled={!ready || pending}
      aria-busy={pending ? true : undefined}
      data-pending={pending ? "" : undefined}
      data-testid={warning === undefined ? testIds?.create : testIds?.anyway}
    >
      <span className="deva-nudge">{warning?.anyway ?? w("Create matter")}</span>
    </Button>
  );
  // the warning's main button: another folder, by the caller's way or the picker Choose folder… opens
  const chooseButton = warning !== undefined && (
    <Button
      kind="primary"
      disabled={picking}
      data-testid={testIds?.choose}
      onClick={() => (warning.onChoose === undefined ? void choose() : warning.onChoose())}
    >
      <span className="deva-nudge">{warning.choose}</span>
    </Button>
  );
  const cancelButton = onCancel !== undefined && (
    <Button kind="quiet" onClick={onCancel}>
      <span className="deva-nudge">{inDialog ? w("Cancel") : w("Back")}</span>
    </Button>
  );
  return (
    <form
      className="matter-form"
      onSubmit={(e) => {
        e.preventDefault();
        void create();
      }}
      onKeyDown={(e) => {
        // Enter in a field while Create waits: the browser submits nothing past a disabled button, so say what's missing
        if (e.key === "Enter" && !ready && e.target instanceof HTMLInputElement) {
          e.preventDefault();
          setTried(true);
        }
      }}
    >
      {titled && (
        <Heading level={2} className="matter-form-title">
          {w("A new matter")}
        </Heading>
      )}
      <TextInput
        label={w("Matter name")}
        value={name}
        onChange={setName}
        error={tried && !named ? w("Name the matter.") : null}
        testId={testIds?.name}
      />
      {details && <TextInput label={w("Client (optional)")} value={client} onChange={setClient} />}
      <FolderRow
        shown={shown}
        made={held?.kind === "made"}
        offer={place === null || held?.kind === "made" ? null : homeShown(place)}
        picking={picking}
        wrong={hasError || (tried && held === null) ? errorId : undefined}
        onChoose={() => void choose()}
        onOffer={() => {
          setChoice({ kind: "made" });
          onMade?.();
        }}
        testIds={{ choose: testIds?.existing, offer: testIds?.made }}
      />
      {opening && <PickerLine />}
      {tried && held === null && !hasError && (
        <p id={errorId} className="matter-form-error">
          <Say>{w("Choose a folder for the matter.")}</Say>
        </p>
      )}
      {scan}
      {warning?.note}
      {hasError && (
        <div className="matter-form-refusal">
          <p id={errorId} role="alert" className="matter-form-error">
            <Say>{error}</Say>
          </p>
          {openMatter !== undefined ? (
            <Button
              kind="tone"
              size="small"
              data-testid={testIds?.open}
              onClick={() => openMatter.open()}
            >
              <span className="deva-nudge">
                {mt(lang, "Open {name}", { name: isolate(openMatter.name) })}
              </span>
            </Button>
          ) : (
            folderError &&
            pickFolder !== undefined && (
              <Button
                kind="tone"
                size="small"
                disabled={picking}
                data-testid="folder-choose-another"
                onClick={() => void choose()}
              >
                <span className="deva-nudge">{fr(lang, "Choose another folder")}</span>
              </Button>
            )
          )}
        </div>
      )}
      {/* in a dialog the act comes last, after Cancel, by the platform's habit; Create still comes first by Tab */}
      <span className={cn("matter-form-actions", inDialog && "matter-form-actions-end")}>
        {chooseButton}
        {createButton}
        {cancelButton}
      </span>
    </form>
  );
}
