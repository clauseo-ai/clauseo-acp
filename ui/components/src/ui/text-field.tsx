import { Field } from "@base-ui/react/field";
import { Input } from "@base-ui/react/input";
import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";
import { Mixed } from "../lib/mixed.tsx";
import { fieldError, fieldLabel, fieldNote, textControl } from "./recipes-controls.ts";

/** A text field on the work's surface, with its label above and one line on what it's for beneath. multiline makes
 * it a textarea three lines tall that the lawyer can drag taller. When what's in it no longer holds, such as a
 * folder that has moved, the error says so in concrete, never red, with the one action that mends it beside it. */
export function TextField({
  label,
  note,
  error,
  action,
  defaultValue,
  placeholder,
  invalid = false,
  disabled = false,
  multiline = false,
  lang,
  className,
}: {
  label: string;
  note: string;
  /** What's wrong, said plainly: "This folder has moved." Shown while invalid. */
  error?: string;
  /** The one way to mend it, beside the error: a Button, "Locate folder". */
  action?: ReactNode;
  defaultValue?: string;
  placeholder?: string;
  invalid?: boolean;
  disabled?: boolean;
  multiline?: boolean;
  /** The words' own language, where they stay as filed: a folder's path is lang="en". */
  lang?: "en";
  className?: string;
}) {
  const control = multiline ? <textarea rows={3} /> : <Input />;
  return (
    <Field.Root
      invalid={invalid}
      disabled={disabled}
      className={cn("grid gap-s1-5 min-w-0", className)}
    >
      <Field.Label className={fieldLabel}>
        <Mixed>{label}</Mixed>
      </Field.Label>
      <Field.Control
        render={control}
        lang={lang}
        className={cn(
          textControl,
          multiline ? "py-s2 resize-y" : "h-control",
          lang === "en" && "font-en",
        )}
        {...(defaultValue === undefined ? {} : { defaultValue })}
        {...(placeholder === undefined ? {} : { placeholder })}
      />
      <Field.Description className={fieldNote}>
        <Mixed>{note}</Mixed>
      </Field.Description>
      {invalid && error !== undefined && (
        <div className="flex flex-wrap items-center gap-x-s3 gap-y-s2">
          <Field.Error match className={fieldError}>
            <Mixed>{error}</Mixed>
          </Field.Error>
          {action}
        </div>
      )}
    </Field.Root>
  );
}
