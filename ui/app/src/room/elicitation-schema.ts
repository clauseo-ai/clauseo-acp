// An elicitation's form, read from the JSON schema the agent sent (ACP's restricted subset: an object of primitive
// properties). parseForm turns the schema into fields; a draft holds what the lawyer has put in them; content turns a
// draft into the values answer_elicitation sends. Supported: one choice (oneOf with const and title, or enum with
// optional enumNames), several choices (an array whose items are anyOf or enum), a string (format email, uri or date;
// minLength, maxLength), a number or integer (minimum, maximum) and a boolean, each with title, description, default
// and required. Claude's AskUserQuestion pairs each `question_<n>` with a free-text `question_<n>_custom`: that field
// joins its question as its Other, independent of the pick (the agent reads it as a note beside one). An unsupported
// optional property is left out; an unsupported required one, or a schema that isn't an object with properties, gives
// no form at all, so the lawyer can only decline or stop.
import type { RawPayload } from "../transport/protocol.ts";

/** One option of a choice: its value as the schema gives it (sent back unchanged), its title and its line. */
export type Choice = { value: unknown; title: string; description: string | null };

type Common = {
  /** The property's key in the values. */
  key: string;
  title: string | null;
  description: string | null;
  required: boolean;
};

/** A free-text field: single-line unless multiline. */
export type TextField = Common & {
  kind: "text";
  input: "text" | "email" | "url" | "date";
  minLength: number | null;
  maxLength: number | null;
  multiline: boolean;
  initial: string;
};

export type Field =
  | (Common & {
      kind: "choice";
      choices: Choice[];
      /** The default's option, by index; null for none. */
      initial: number | null;
      /** The question's own Other: `<key>_custom`, a free-text field beneath its options. */
      other: TextField | null;
    })
  | (Common & {
      kind: "multi";
      choices: Choice[];
      minItems: number | null;
      maxItems: number | null;
      /** The default's options, by index. */
      initial: number[];
      other: TextField | null;
    })
  | TextField
  | (Common & {
      kind: "number";
      integer: boolean;
      minimum: number | null;
      maximum: number | null;
      initial: string;
    })
  | (Common & {
      kind: "boolean";
      /** Its default; null with none, so an untouched box is not yet an answer. */
      initial: boolean | null;
    });

export type Form = { fields: Field[] };

/** A number field whose words the input can't read as a number ("1e", "abc"): the input reports "" for them, so the
 * draft keeps this instead, and the field doesn't hold. */
export const BAD_NUMBER = { badInput: true } as const;
export type BadNumber = typeof BAD_NUMBER;

/** What a field holds while the lawyer fills it in: a choice's option index, a multi's option indexes in schema order,
 * a text or number field's words (or BAD_NUMBER), a boolean's tick (null until touched, with no default). */
export type FieldValue = number | null | readonly number[] | string | boolean | BadNumber;
export type Draft = Readonly<Record<string, FieldValue>>;

/** Why a field doesn't hold: empty though required, or not what it asks for. */
export type FieldProblem = "required" | "invalid";

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown): string | null => (typeof v === "string" && v !== "" ? v : null);
const count = (v: unknown): number | null =>
  typeof v === "number" && Number.isInteger(v) && v >= 0 ? v : null;
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** A string is a short textarea past this many characters allowed. */
const LONG = 200;

/** The options of a oneOf/anyOf list (each a const, with a title and a description), or an enum with its enumNames;
 * null when neither is usable. */
function choicesOf(list: unknown, en: unknown, names: unknown): Choice[] | null {
  if (Array.isArray(list)) {
    const out: Choice[] = [];
    for (const o of list) {
      if (!isObj(o) || !("const" in o)) return null;
      out.push({
        value: o["const"],
        title: str(o["title"]) ?? String(o["const"]),
        description: str(o["description"]),
      });
    }
    return out.length > 0 ? out : null;
  }
  if (Array.isArray(en) && en.length > 0) {
    const titles = Array.isArray(names) ? names : [];
    return en.map((value: unknown, i) => ({
      value,
      title: str(titles[i]) ?? String(value),
      description: null,
    }));
  }
  return null;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

function textField(common: Common, p: Obj, multiline = false): TextField | null {
  if (p["type"] !== "string") return null;
  const format = p["format"];
  const input =
    format === "email" ? "email" : format === "uri" ? "url" : format === "date" ? "date" : "text";
  const maxLength = count(p["maxLength"]);
  return {
    ...common,
    kind: "text",
    input,
    minLength: count(p["minLength"]),
    maxLength,
    multiline: input === "text" && (multiline || (maxLength !== null && maxLength > LONG)),
    initial: typeof p["default"] === "string" ? p["default"] : "",
  };
}

/** One property as a field; null when it is of a kind the form can't show. */
function fieldOf(key: string, p: unknown, required: boolean): Field | null {
  if (!isObj(p)) return null;
  const common: Common = {
    key,
    title: str(p["title"]),
    description: str(p["description"]),
    required,
  };
  const type = p["type"];
  if (type === "array") {
    const items = p["items"];
    if (!isObj(items)) return null;
    const choices = choicesOf(items["anyOf"], items["enum"], items["enumNames"]);
    if (choices === null) return null;
    const d = Array.isArray(p["default"]) ? p["default"] : [];
    return {
      ...common,
      kind: "multi",
      choices,
      minItems: count(p["minItems"]),
      maxItems: count(p["maxItems"]),
      initial: choices.flatMap((c, i) => (d.some((v) => same(v, c.value)) ? [i] : [])),
      other: null,
    };
  }
  const choices = choicesOf(p["oneOf"], p["enum"], p["enumNames"]);
  if (choices !== null) {
    const i = "default" in p ? choices.findIndex((c) => same(c.value, p["default"])) : -1;
    return { ...common, kind: "choice", choices, initial: i < 0 ? null : i, other: null };
  }
  if (type === "string") return textField(common, p);
  if (type === "number" || type === "integer") {
    const d = num(p["default"]);
    return {
      ...common,
      kind: "number",
      integer: type === "integer",
      minimum: num(p["minimum"]),
      maximum: num(p["maximum"]),
      initial: d === null ? "" : String(d),
    };
  }
  if (type === "boolean")
    return {
      ...common,
      kind: "boolean",
      initial: typeof p["default"] === "boolean" ? p["default"] : null,
    };
  return null;
}

const CUSTOM = /^(.+)_custom$/;

/** The schema's form; null when it can't be shown (not an object with properties, a required property of a kind the
 * form can't show, or nothing showable left). */
export function parseForm(schema: RawPayload): Form | null {
  if (!isObj(schema) || (schema["type"] !== undefined && schema["type"] !== "object")) return null;
  const props = schema["properties"];
  if (!isObj(props)) return null;
  const req = Array.isArray(schema["required"]) ? schema["required"] : [];
  const keys = Object.keys(props);
  const fields: Field[] = [];
  const byKey = new Map<string, Field>();
  for (const key of keys) {
    const f = fieldOf(key, props[key], req.includes(key));
    if (f === null) {
      if (req.includes(key)) return null;
      continue;
    }
    byKey.set(key, f);
  }
  // a question's _custom joins it as its Other, a short textarea
  const joined = new Set<string>();
  for (const [key, f] of byKey) {
    const of = CUSTOM.exec(key)?.[1];
    const q = of === undefined ? undefined : byKey.get(of);
    if (f.kind !== "text" || q === undefined || (q.kind !== "choice" && q.kind !== "multi"))
      continue;
    q.other = { ...f, multiline: f.input === "text" };
    joined.add(key);
  }
  for (const key of keys) {
    const f = byKey.get(key);
    if (f !== undefined && !joined.has(key)) fields.push(f);
  }
  return fields.length > 0 ? { fields } : null;
}

/** Every field, and every question's Other, as the draft keys them. */
const allFields = (form: Form): Field[] =>
  form.fields.flatMap((f) =>
    (f.kind === "choice" || f.kind === "multi") && f.other !== null ? [f, f.other] : [f],
  );

/** The draft a form starts from: each field's default. */
export function initialDraft(form: Form): Draft {
  const d: Record<string, FieldValue> = {};
  for (const f of allFields(form)) d[f.key] = f.initial;
  return d;
}

/** A string's length as JSON Schema counts it: in code points. */
const chars = (s: string) => s.match(/./gsu)?.length ?? 0;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

function validUrl(s: string): boolean {
  try {
    return new URL(s).protocol !== "";
  } catch {
    return false;
  }
}

/** Whether a field holds something the lawyer put there (or that the agent defaulted); words of spaces are nothing. */
function filled(f: Field, v: FieldValue | undefined): boolean {
  switch (f.kind) {
    case "choice":
      return typeof v === "number";
    case "multi":
      return Array.isArray(v) && v.length > 0;
    case "text":
      return typeof v === "string" && v.trim() !== "";
    case "number":
      return v === BAD_NUMBER || (typeof v === "string" && v.trim() !== "");
    case "boolean":
      // touched, or defaulted: false is an answer too
      return typeof v === "boolean";
  }
}

/** What's wrong with a field's value, or null when it holds. */
export function problemOf(f: Field, v: FieldValue | undefined): FieldProblem | null {
  if (!filled(f, v)) return f.required && f.kind !== "boolean" ? "required" : null;
  switch (f.kind) {
    case "multi": {
      const n = (v as readonly number[]).length;
      if (f.minItems !== null && n < f.minItems) return "invalid";
      if (f.maxItems !== null && n > f.maxItems) return "invalid";
      return null;
    }
    case "text": {
      const s = (v as string).trim();
      const n = chars(s);
      if (f.minLength !== null && n < f.minLength) return "invalid";
      if (f.maxLength !== null && n > f.maxLength) return "invalid";
      if (f.input === "email" && !EMAIL.test(s)) return "invalid";
      if (f.input === "url" && !validUrl(s)) return "invalid";
      if (f.input === "date" && !DATE.test(s)) return "invalid";
      return null;
    }
    case "number": {
      if (v === BAD_NUMBER) return "invalid";
      const n = Number((v as string).trim());
      if (!Number.isFinite(n) || (f.integer && !Number.isInteger(n))) return "invalid";
      if (f.minimum !== null && n < f.minimum) return "invalid";
      if (f.maximum !== null && n > f.maximum) return "invalid";
      return null;
    }
    default:
      return null;
  }
}

/** Every field's problem, by key; empty when the draft holds. */
export function problems(form: Form, draft: Draft): Map<string, FieldProblem> {
  const out = new Map<string, FieldProblem>();
  for (const f of allFields(form)) {
    const p = problemOf(f, draft[f.key]);
    if (p !== null) out.set(f.key, p);
  }
  return out;
}

/** Whether anything at all is filled in: an accept with nothing in it is a decline, so Send waits for something. */
export const anyFilled = (form: Form, draft: Draft): boolean =>
  allFields(form).some((f) => filled(f, draft[f.key]));

/** Whether Send can go: every field holds, and something is filled in. */
export const canSend = (form: Form, draft: Draft): boolean =>
  problems(form, draft).size === 0 && anyFilled(form, draft);

/** The values answer_elicitation sends: a choice's const as given, a multi's consts in schema order, words trimmed
 * (left out when empty), a number as a number (left out when empty), a boolean always (false when untouched). */
export function content(form: Form, draft: Draft): Record<string, RawPayload> {
  const out: Record<string, RawPayload> = {};
  for (const f of allFields(form)) {
    const v = draft[f.key];
    switch (f.kind) {
      case "choice":
        if (typeof v === "number" && f.choices[v] !== undefined)
          out[f.key] = f.choices[v].value as RawPayload;
        break;
      case "multi":
        if (Array.isArray(v) && v.length > 0)
          out[f.key] = f.choices.flatMap((c, i) => (v.includes(i) ? [c.value as RawPayload] : []));
        break;
      case "text":
        if (typeof v === "string" && v.trim() !== "") out[f.key] = v.trim();
        break;
      case "number":
        if (typeof v === "string" && v.trim() !== "") out[f.key] = Number(v.trim());
        break;
      case "boolean":
        out[f.key] = v === true;
        break;
    }
  }
  return out;
}

/** The test ids the e2e specs and the tests use. */
export const ELICITATION_TEST_IDS = {
  form: "elicitation-form",
  send: "elicitation-send",
  decline: "elicitation-decline",
  stop: "elicitation-stop",
  hint: "elicitation-hint",
  error: "elicitation-error",
  /** The line on a schema the form can't show. */
  unsupported: "elicitation-unsupported",
  /** A field's wrapper (a fieldset for a question), by its key. */
  field: (key: string) => `elicitation-field-${key}`,
  /** A text, number or boolean field's control, or a question's Other, by its key. */
  input: (key: string) => `elicitation-input-${key}`,
  /** A question's option, by its key and the option's index. */
  choice: (key: string, i: number) => `elicitation-choice-${key}-${i}`,
  /** A field's problem line, by its key. */
  problem: (key: string) => `elicitation-problem-${key}`,
} as const;
