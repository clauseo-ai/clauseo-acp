import { Field } from "@base-ui/react/field";
import { Fieldset } from "@base-ui/react/fieldset";
import { useId } from "react";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import { Checkbox } from "./checkbox.tsx";
import { RadioGroup, RadioItem } from "./radio-group.tsx";
import { settingLabel, settingNote } from "./recipes-controls.ts";
import { Switch } from "./switch.tsx";

/** A settings row: the setting in the interface's words and one line on what it does, with its control. A switch
 * stands at the row's end, centred on both lines; a checkbox leads, on the label's first line. A press on the
 * label flips it too. */
export function SettingRow({
  control,
  label,
  note,
  checked,
  onCheckedChange,
  disabled = false,
  className,
}: {
  control: "switch" | "checkbox";
  label: string;
  note: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
}) {
  const props = { checked, onCheckedChange, disabled };
  return (
    <Field.Root
      disabled={disabled}
      className={cn(
        "flex gap-s3 py-s3",
        control === "switch" ? "items-center" : "items-start",
        className,
      )}
    >
      {control === "checkbox" && (
        <span className="flex flex-none items-center h-ui-line">
          <Checkbox {...props} />
        </span>
      )}
      <div className="min-w-0 flex-1">
        <Field.Label className={settingLabel}>
          <Mixed>{label}</Mixed>
        </Field.Label>
        <Field.Description className={settingNote}>
          <Mixed>{note}</Mixed>
        </Field.Description>
      </div>
      {control === "switch" && <Switch {...props} />}
    </Field.Root>
  );
}

export type RadioChoice<V extends string> = {
  value: V;
  label: string;
  note: string;
  disabled?: boolean;
};

/** A setting with a few choices, each with its line: the setting's name and line, then one row per choice. */
export function RadioSetting<V extends string>({
  label,
  note,
  value,
  choices,
  onValueChange,
  disabled = false,
  className,
}: {
  label: string;
  note: string;
  value: V;
  choices: readonly RadioChoice<V>[];
  onValueChange: (value: V) => void;
  disabled?: boolean;
  className?: string;
}) {
  const noteId = useId();
  return (
    <Field.Root disabled={disabled} className={cn("py-s3", className)}>
      <Fieldset.Root
        render={
          <RadioGroup
            value={value}
            aria-describedby={noteId}
            onValueChange={(next) => {
              const chosen = choices.find((c) => c.value === next);
              if (chosen !== undefined) onValueChange(chosen.value);
            }}
          />
        }
        className="grid"
      >
        <Fieldset.Legend className={settingLabel}>
          <Mixed>{label}</Mixed>
        </Fieldset.Legend>
        <p id={noteId} className={settingNote}>
          <Mixed>{note}</Mixed>
        </p>
        <div className="grid gap-s3 mt-s3">
          {choices.map((c) => (
            <Field.Item
              key={c.value}
              disabled={c.disabled ?? false}
              className="flex items-start gap-s3"
            >
              <span className="flex flex-none items-center h-ui-line">
                <RadioItem value={c.value} />
              </span>
              <div className="min-w-0 flex-1">
                <Field.Label className={settingLabel}>
                  <Mixed>{c.label}</Mixed>
                </Field.Label>
                <Field.Description className={settingNote}>
                  <Mixed>{c.note}</Mixed>
                </Field.Description>
              </div>
            </Field.Item>
          ))}
        </div>
      </Fieldset.Root>
    </Field.Root>
  );
}
