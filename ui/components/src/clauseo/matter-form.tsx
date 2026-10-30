// The new matter form. A name as it stands on the cause list, then a client if any and the next date
// if fixed side by side, since the rail groups matters by their next date, and the folder Clauseo makes for it under
// Documents as one muted line with a quiet Change. The lawyer's words take the composer's setting, 17 px at 600. First
// launch opens it on the first screen, and the rail's New matter opens the same form. The folder's path is the
// computer's own, in English in both languages.
import { useState } from "react";
import { Field } from "@base-ui/react/field";
import { Input } from "@base-ui/react/input";
import { Button } from "../ui/button.tsx";
import { fieldLabel, textControl } from "../ui/recipes-controls.ts";
import { IconFolder } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { Heading } from "./heading.tsx";
import { Say } from "./say.tsx";

export type NewMatterFormProps = {
  /** The folder Clauseo would make for a matter of this name, such as "Documents/Clauseo/Acme Ltd v. State". */
  folderFor: (name: string) => string;
  /** What the fields hold when the form opens. */
  name?: string;
  client?: string;
  /** Optional: the next date, as the lawyer writes it, "30 October 2026". */
  next?: string;
  /** The next date is optional: a callback taking two arguments still works. */
  onCreate: (name: string, client: string, next: string) => void;
  /** Back, where there is somewhere to go back to. */
  onCancel?: () => void;
  /** Optional: Change, pressed, opens the computer's own folder picker; it gives the folder chosen,
   * or null if none was. Without it, Change stands for the picker and keeps the folder Clauseo would make. */
  pickFolder?: (current: string) => string | null;
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
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  className?: string;
}) {
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
        onChange={(e) => onChange(e.currentTarget.value)}
        className={cn(
          textControl,
          "matter-form-input font-en text-field font-accepted h-auto min-h-control py-s2",
        )}
      />
    </Field.Root>
  );
}

/** The form. Create the matter waits, faint, until the matter has a name. */
export function NewMatterForm({
  folderFor,
  name: name0 = "",
  client: client0 = "",
  next: next0 = "",
  onCreate,
  onCancel,
  pickFolder,
  titled = true,
  inDialog = false,
}: NewMatterFormProps) {
  const w = useMatterFormWords();
  const [name, setName] = useState(name0);
  const [client, setClient] = useState(client0);
  const [next, setNext] = useState(next0);
  const [picked, setPicked] = useState<string | null>(null);
  const ready = name.trim() !== "";
  const folder = picked ?? folderFor(name.trim() || name0);
  return (
    <form
      className="matter-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) onCreate(name.trim(), client.trim(), next.trim());
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
      />
      <div className="matter-form-pair">
        <TextInput label={w("Client, if any")} value={client} onChange={setClient} />
        <TextInput
          label={w("Next date, if fixed")}
          value={next}
          onChange={setNext}
          placeholder={w("30 October 2026")}
        />
      </div>
      <div
        className="matter-form-folder"
        title={w("Clauseo makes this folder. Add papers to it any time.")}
      >
        <IconFolder size={16} className="matter-form-folder-icon" />
        <p className="matter-form-folder-words">
          <Say>{w("Folder:")}</Say>{" "}
          <span lang="en" className="matter-form-path">
            {folder}
          </span>
        </p>
        <Button
          kind="quiet"
          size="small"
          className="matter-form-change"
          onClick={() => setPicked(pickFolder?.(folder) ?? picked)}
        >
          <span className="deva-nudge">{w("Change")}</span>
        </Button>
      </div>
      {inDialog ? (
        <span className="flex flex-wrap justify-end gap-s2 mt-s2">
          {onCancel !== undefined && (
            <Button kind="quiet" onClick={onCancel}>
              <span className="deva-nudge">{w("Cancel")}</span>
            </Button>
          )}
          <Button kind="tone" type="submit" disabled={!ready}>
            <span className="deva-nudge">{w("Create the matter")}</span>
          </Button>
        </span>
      ) : (
        <span className="flex flex-wrap gap-s2 mt-s2">
          <Button kind="tone" type="submit" disabled={!ready}>
            <span className="deva-nudge">{w("Create the matter")}</span>
          </Button>
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
