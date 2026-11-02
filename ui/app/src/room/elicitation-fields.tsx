// The field renderers of the agent's question form (elicitation-form.tsx): a question's radios or checkboxes, with
// its Other beneath them, a box, and a text or number field, each with its name, its line and its problem (FieldCtx).
import type { ReactNode } from "react";
import { IconTick } from "@clauseo/components/icons/icons.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { textControl } from "@clauseo/components/ui/recipes-controls.ts";
import {
  type Choice,
  type Draft,
  type Field,
  type FieldValue,
  type TextField,
  BAD_NUMBER,
  ELICITATION_TEST_IDS,
} from "./elicitation-schema.ts";

const T = ELICITATION_TEST_IDS;

/** A native radio or checkbox, drawn as the design system's (recipes-controls.ts's radio and checkBox): set into the
 * plane with its lip, primary once chosen. Native, so a shared name gives the arrows within a group for free. */
const NATIVE = [
  "peer col-start-1 row-start-1 m-0 appearance-none size-check motion-control",
  "bg-inset set-in-lip is-hover:bg-well-strong is-active:bg-inset",
  "checked:bg-primary checked:shadow-icon-primary is-hover:checked:bg-primary-hover",
  "is-disabled:bg-well-strong is-disabled:shadow-none",
].join(" ");
const MARK = "relative pointer-events-none col-start-1 row-start-1 hidden peer-checked:block";

/** A field's ids, and whether its problem shows. */
export type Ids = { base: string; problem: string | null };

export type FieldCtx = {
  draft: Draft;
  set: (key: string, v: FieldValue) => void;
  touch: (key: string) => void;
  idsOf: (key: string, slot: string) => Ids;
  problemLine: (key: string, ids: Ids) => ReactNode;
  required: ReactNode;
  other: string;
  /** The agent's message, naming a field that has neither title nor description. */
  msgId: string;
};

const ids = (...list: (string | null | false | undefined)[]) =>
  list.filter(Boolean).join(" ") || undefined;

/** A field's name: its title, else its description (then not shown again beneath it), else none, and the agent's
 * message names it. */
const nameOf = (f: Field) => f.title ?? f.description;

function Marker({ children }: { children: ReactNode }) {
  return <span className="ms-s2 text-small text-muted">{children}</span>;
}

export function FieldView({
  field: f,
  index,
  ctx,
}: {
  field: Field;
  index: number;
  ctx: FieldCtx;
}) {
  const own = ctx.idsOf(f.key, String(index));
  const name = nameOf(f);
  // a description standing in for the title is said once, as the name
  const described = f.title !== null && f.description !== null;
  const descId = described ? `${own.base}-desc` : null;
  const nameId = `${own.base}-name`;
  const desc = !described ? null : (
    <p id={descId ?? undefined} className="mt-s0-5 text-small text-muted" dir="auto">
      <bdi>{f.description}</bdi>
    </p>
  );
  if (f.kind === "choice" || f.kind === "multi") {
    const v = ctx.draft[f.key];
    const multi = f.kind === "multi";
    return (
      <fieldset
        className="m-0 p-0 border-0 min-w-0"
        aria-labelledby={name === null ? ctx.msgId : undefined}
        aria-describedby={ids(descId, own.problem)}
        aria-invalid={own.problem === null ? undefined : true}
        data-testid={T.field(f.key)}
      >
        {(name !== null || f.required) && (
          <legend id={nameId} className="p-0 text-ui font-ui text-primary">
            {name !== null && <bdi>{name}</bdi>}
            {f.required && <Marker>{ctx.required}</Marker>}
          </legend>
        )}
        {desc}
        {/* the radios alone are the radiogroup; Other sits beside it, in the question's group */}
        <div
          className="mt-s1 grid"
          role={multi ? undefined : "radiogroup"}
          aria-labelledby={multi ? undefined : name === null ? ctx.msgId : nameId}
          aria-required={!multi && f.required ? true : undefined}
        >
          {f.choices.map((c, i) => (
            <Option
              key={i}
              choice={c}
              multi={multi}
              name={own.base}
              testId={T.choice(f.key, i)}
              checked={multi ? Array.isArray(v) && v.includes(i) : v === i}
              required={!multi && f.required}
              onClear={multi ? undefined : () => ctx.set(f.key, null)}
              onChange={(on) => {
                if (!multi) return ctx.set(f.key, i);
                const was = Array.isArray(v) ? v : [];
                ctx.set(f.key, on ? [...was, i].sort((a, b) => a - b) : was.filter((x) => x !== i));
                ctx.touch(f.key);
              }}
            />
          ))}
        </div>
        {f.other !== null && <Other field={f.other} index={index} ctx={ctx} />}
        {ctx.problemLine(f.key, own)}
      </fieldset>
    );
  }
  if (f.kind === "boolean")
    return (
      <div data-testid={T.field(f.key)}>
        <label className="flex items-start gap-s3">
          <span className="flex flex-none items-center h-ui-line">
            <span className="inline-grid place-items-center">
              <input
                type="checkbox"
                className={`${NATIVE} rounded-label`}
                checked={ctx.draft[f.key] === true}
                required={f.required || undefined}
                aria-required={f.required || undefined}
                onChange={(e) => ctx.set(f.key, e.currentTarget.checked)}
                aria-labelledby={name === null ? ctx.msgId : undefined}
                aria-describedby={ids(descId)}
                data-testid={T.input(f.key)}
              />
              <IconTick size={14} className={`${MARK} text-on-primary`} />
            </span>
          </span>
          <span className="min-w-0 text-ui font-ui text-primary">
            {name !== null && <bdi>{name}</bdi>}
            {f.required && <Marker>{ctx.required}</Marker>}
          </span>
        </label>
        {desc}
      </div>
    );
  return <Typed field={f} index={index} ctx={ctx} desc={desc} descId={descId} />;
}

/** A text or number field, standing on its own: its label, the control, its line and its problem. */
function Typed({
  field: f,
  index,
  ctx,
  desc,
  descId,
}: {
  field: Extract<Field, { kind: "text" | "number" }>;
  index: number;
  ctx: FieldCtx;
  desc: ReactNode;
  descId: string | null;
}) {
  const own = ctx.idsOf(f.key, String(index));
  const v = ctx.draft[f.key];
  const value = typeof v === "string" ? v : "";
  const common = {
    id: own.base,
    value,
    dir: "auto" as const,
    required: f.required,
    "aria-required": f.required || undefined,
    "aria-invalid": own.problem === null ? undefined : true,
    "aria-describedby": ids(descId, own.problem),
    "aria-labelledby": nameOf(f) === null ? ctx.msgId : undefined,
    "data-testid": T.input(f.key),
    onBlur: () => ctx.touch(f.key),
  };
  const control =
    f.kind === "number" ? (
      <input
        {...common}
        type="number"
        inputMode={f.integer ? "numeric" : "decimal"}
        step={f.integer ? 1 : "any"}
        min={f.minimum ?? undefined}
        max={f.maximum ?? undefined}
        className={`${textControl} h-control`}
        // words the input can't read as a number come as "", kept as BAD_NUMBER so the field doesn't hold. onInput too:
        // from "" to "abc" the value stays "", and React's onChange hears no change
        onChange={(e) => ctx.set(f.key, numberOf(e.currentTarget))}
        onInput={(e) => ctx.set(f.key, numberOf(e.currentTarget))}
      />
    ) : (
      <TextControl field={f} common={common} ctx={ctx} />
    );
  return (
    <div className="grid gap-s1-5 min-w-0" data-testid={T.field(f.key)}>
      <label htmlFor={own.base} className="text-ui font-ui text-primary">
        {nameOf(f) !== null && <bdi>{nameOf(f)}</bdi>}
        {f.required && <Marker>{ctx.required}</Marker>}
      </label>
      {control}
      {desc}
      {ctx.problemLine(f.key, own)}
    </div>
  );
}

/** A number field's words, or BAD_NUMBER where the input can't read them. */
const numberOf = (input: HTMLInputElement) => (input.validity.badInput ? BAD_NUMBER : input.value);

type Common = {
  id: string;
  value: string;
  dir: "auto";
  required: boolean;
  "aria-required": boolean | undefined;
  "aria-invalid": boolean | undefined;
  "aria-describedby": string | undefined;
  "aria-labelledby": string | undefined;
  "data-testid": string;
  onBlur: () => void;
};

function TextControl({
  field: f,
  common,
  ctx,
}: {
  field: TextField;
  common: Common;
  ctx: FieldCtx;
}) {
  const limits = {
    minLength: f.minLength ?? undefined,
    maxLength: f.maxLength ?? undefined,
  };
  return f.multiline ? (
    <textarea
      {...common}
      {...limits}
      rows={2}
      className={`${textControl} py-s2 resize-y`}
      onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
    />
  ) : (
    <input
      {...common}
      {...limits}
      type={f.input}
      className={`${textControl} h-control`}
      onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
    />
  );
}

/** A question's Other: our word for its label, the agent's description as its hint, a short textarea beneath the
 * options, inside the question's group. Independent of the pick. */
function Other({ field: f, index, ctx }: { field: TextField; index: number; ctx: FieldCtx }) {
  const own = ctx.idsOf(f.key, `${index}o`);
  const id = own.base;
  const hintId = f.description === null ? null : `${id}-hint`;
  const v = ctx.draft[f.key];
  return (
    <div className="mt-s2 grid gap-s1">
      <label htmlFor={id} className="text-small font-ui text-primary">
        <Say>{ctx.other}</Say>
      </label>
      <textarea
        id={id}
        rows={2}
        dir="auto"
        value={typeof v === "string" ? v : ""}
        maxLength={f.maxLength ?? undefined}
        aria-describedby={ids(hintId, own.problem)}
        aria-invalid={own.problem === null ? undefined : true}
        data-testid={T.input(f.key)}
        className={`${textControl} py-s2 resize-y`}
        onChange={(e) => ctx.set(f.key, e.currentTarget.value)}
        onBlur={() => ctx.touch(f.key)}
      />
      {hintId !== null && (
        <p id={hintId} className="text-small text-muted" dir="auto">
          <bdi>{f.description}</bdi>
        </p>
      )}
      {ctx.problemLine(f.key, own)}
    </div>
  );
}

/** One option of a question: a native radio or checkbox, its title and its line. */
function Option({
  choice,
  multi,
  name,
  testId,
  checked,
  required,
  onChange,
  onClear,
}: {
  choice: Choice;
  multi: boolean;
  name: string;
  testId: string;
  checked: boolean;
  required: boolean;
  onChange: (on: boolean) => void;
  /** A chosen radio's click or Space clears it. */
  onClear?: (() => void) | undefined;
}) {
  const lineId = `${name}-${testId}-line`;
  return (
    <label className="flex items-start gap-s3 py-s1-5">
      <span className="flex flex-none items-center h-ui-line">
        <span className="inline-grid place-items-center">
          <input
            type={multi ? "checkbox" : "radio"}
            name={name}
            className={`${NATIVE} ${multi ? "rounded-label" : "rounded-full"}`}
            checked={checked}
            required={required || undefined}
            aria-describedby={choice.description === null ? undefined : lineId}
            data-testid={testId}
            onChange={(e) => onChange(e.currentTarget.checked)}
            // a click on the radio already chosen (Space on it is one too) changes nothing natively; it clears it.
            // The arrows move the choice with a click on the newly chosen radio, which wasn't chosen, so they never clear
            onClick={onClear === undefined || !checked ? undefined : onClear}
          />
          {multi ? (
            <IconTick size={14} className={`${MARK} text-on-primary`} />
          ) : (
            <span className={`${MARK} size-check-dot rounded-full bg-on-primary`} />
          )}
        </span>
      </span>
      <span className="min-w-0">
        <span className="block text-ui font-ui text-primary" dir="auto">
          <bdi>{choice.title}</bdi>
        </span>
        {choice.description !== null && (
          <span id={lineId} className="block mt-s0-5 text-small text-muted" dir="auto">
            <bdi>{choice.description}</bdi>
          </span>
        )}
      </span>
    </label>
  );
}
