// Settings › Tools' form (UC-176 to UC-189): add or edit one tool. A local tool is a name, the program's full path, its
// arguments one per line and its environment values; a tool at an address (behind REMOTE_TOOLS) is a name, the address
// and its headers. A secret's value lives only in its password field's own DOM state: Save reads it from the form as
// it sends, puts it in the one save and keeps it nowhere else (as connectors-key.tsx does). A saved value is never
// shown: its row says Saved, with Replace (a new value, kept as it was when left empty) and Remove. The host's refusal
// shows under its field; Escape and Cancel leave without saving. Focus starts in Name.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { cn } from "@clauseo/components/lib/cn.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { mw, type McpToolsKey } from "@clauseo/components/lib/words-mcp-tools.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import {
  fieldError,
  fieldLabel,
  fieldNote,
  settingLabel,
  settingNote,
  textControl,
} from "@clauseo/components/ui/recipes-controls.ts";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  isFullPath,
  type McpServerDraft,
  type McpServerView,
  type McpToolsError,
  type SecretEdit,
  type ToolsAnswer,
} from "./tools-model.ts";
import { commandExample, refusalOf, URL_EXAMPLE, type Place } from "./mcp-tools-refusal.ts";

/** One secret's row: saved before (its value kept unless replaced), or new. */
type Row = { key: number; name: string; saved: boolean; replacing: boolean };

/** The form field holding a row's new value. */
const valueField = (list: string, key: number) => `${list}-${key}`;

/** A field: its label, the control, and under it its line, or its refusal in the line's place. */
function FieldBox({
  label,
  note,
  error,
  children,
}: {
  label: string;
  note?: string | undefined;
  error?: string | undefined;
  children: (ids: { id: string; describedBy: string | undefined; invalid: boolean }) => ReactNode;
}) {
  const id = useId();
  const line = error ?? note;
  return (
    <div className="grid gap-s1-5 min-w-0">
      <label htmlFor={id} className={fieldLabel}>
        {say(label)}
      </label>
      {children({
        id,
        describedBy: line === undefined ? undefined : `${id}-line`,
        invalid: error !== undefined,
      })}
      {line !== undefined && (
        <p
          id={`${id}-line`}
          className={error === undefined ? fieldNote : fieldError}
          data-testid={error === undefined ? undefined : "tools-error"}
        >
          {say(line)}
        </p>
      )}
    </div>
  );
}

/** The rows of secrets: the environment values, or a remote tool's headers. */
function Secrets({
  list,
  title,
  rows,
  setRows,
  error,
  nextKey,
}: {
  list: "env" | "header";
  title: string;
  rows: Row[];
  setRows: (rows: Row[]) => void;
  error: string | undefined;
  nextKey: () => number;
}) {
  const { lang } = useDisplay();
  const head = useId();
  // the field to focus once it renders: a new row's name, or a saved row's new value after Replace
  const focusNext = useRef<string | null>(null);
  const focusOnce = (id: string) => (el: HTMLInputElement | null) => {
    if (el === null || focusNext.current !== id) return;
    focusNext.current = null;
    el.focus();
  };
  const update = (key: number, patch: Partial<Row>) =>
    setRows(rows.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  return (
    <div
      role="group"
      aria-labelledby={head}
      className="grid gap-s2 min-w-0"
      data-testid={`tools-${list}`}
    >
      <p id={head} className={fieldLabel}>
        {say(title)}
      </p>
      {rows.map((row) => (
        <div
          key={row.key}
          className="flex flex-wrap items-center gap-s2"
          data-testid="tools-secret"
        >
          {row.saved ? (
            <span className="min-w-0 flex-1 basis-40">
              <span className={cn(settingLabel, "font-en")} lang="en">
                <bdi>{row.name}</bdi>
              </span>
              {!row.replacing && (
                <span className={settingNote} data-testid="tools-saved">
                  {say(mw(lang, "Saved"))}
                </span>
              )}
            </span>
          ) : (
            <input
              aria-label={mw(lang, "Name")}
              className={cn(textControl, "h-control font-en min-w-0 flex-1 basis-40")}
              lang="en"
              spellCheck={false}
              autoComplete="off"
              value={row.name}
              ref={focusOnce(`name-${row.key}`)}
              onChange={(e) => update(row.key, { name: e.currentTarget.value })}
              data-testid="tools-secret-name"
            />
          )}
          {(!row.saved || row.replacing) && (
            <input
              type="password"
              name={valueField(list, row.key)}
              aria-label={row.saved ? row.name : title}
              autoComplete="off"
              spellCheck={false}
              className={cn(textControl, "h-control min-w-0 flex-1 basis-40")}
              ref={focusOnce(`value-${row.key}`)}
              data-testid="tools-secret-value"
            />
          )}
          {row.saved && !row.replacing && (
            <Button
              kind="quiet"
              size="small"
              onClick={() => {
                focusNext.current = `value-${row.key}`;
                update(row.key, { replacing: true });
              }}
            >
              {mw(lang, "Replace")}
            </Button>
          )}
          <Button
            kind="quiet"
            size="small"
            aria-label={row.name === "" ? mt(lang, "Remove") : `${mt(lang, "Remove")} ${row.name}`}
            onClick={() => setRows(rows.filter((r) => r.key !== row.key))}
          >
            {mt(lang, "Remove")}
          </Button>
        </div>
      ))}
      {error !== undefined && (
        <p className={fieldError} data-testid="tools-error">
          {say(error)}
        </p>
      )}
      <div>
        <Button
          kind="quiet"
          size="small"
          onClick={() => {
            const key = nextKey();
            focusNext.current = `name-${key}`;
            setRows([...rows, { key, name: "", saved: false, replacing: false }]);
          }}
          data-testid={`tools-add-${list}`}
        >
          {cw(lang, "Add")}
        </Button>
      </div>
    </div>
  );
}

/** The form: `editing` null adds a tool. `save` sends the draft and answers the host's refusal or null. */
export function ToolForm({
  editing,
  remote,
  save,
  onDone,
  onCancel,
}: {
  editing: McpServerView | null;
  /** Whether adding may choose a tool at an address (REMOTE_TOOLS). */
  remote: boolean;
  save: (previousName: string | null, draft: McpServerDraft) => Promise<ToolsAnswer>;
  /** Saved, under this name. */
  onDone: (name: string) => void;
  onCancel: () => void;
}) {
  const { lang, platform } = useDisplay();
  // a new row's key: past every key the rows started with (env from 0, headers from 10,000)
  const counter = useRef(20_000);
  // the form opens on the lawyer's press, at its first field
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => first.current?.focus(), []);
  const nextKey = () => (counter.current += 1);
  const rowsOf = (secrets: McpServerView["env"], from: number): Row[] =>
    secrets.map((s, i) => ({ key: from + i, name: s.name, saved: s.saved, replacing: false }));
  const [kind, setKind] = useState<"local" | "remote">(editing?.kind ?? "local");
  const [name, setName] = useState(editing?.name ?? "");
  const [command, setCommand] = useState(editing?.command ?? "");
  const [args, setArgs] = useState((editing?.args ?? []).join("\n"));
  const [url, setUrl] = useState(editing?.url ?? "");
  const [env, setEnv] = useState<Row[]>(() => rowsOf(editing?.env ?? [], 0));
  const [headers, setHeaders] = useState<Row[]>(() => rowsOf(editing?.headers ?? [], 10_000));
  const [refusal, setRefusal] = useState<{ place: Place; words: string } | null>(null);
  const [sending, setSending] = useState(false);
  const errorAt = (place: Place) => (refusal?.place === place ? refusal.words : undefined);
  const m = (key: McpToolsKey, slots?: Record<string, string>) => mw(lang, key, slots);

  const edits = (rows: Row[], list: "env" | "header", data: FormData): SecretEdit[] =>
    rows
      .filter((r) => r.name.trim() !== "")
      .map((r) => {
        const typed = data.get(valueField(list, r.key));
        const value = typeof typed === "string" ? typed : "";
        return {
          name: r.name.trim(),
          value: r.saved && value === "" ? { keep: true } : { set: value },
        };
      });

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (sending) return;
    const data = new FormData(event.currentTarget);
    const trimmed = name.trim();
    // what the form can tell before the host: a missing name, a program named without its path
    const early: McpToolsError | null =
      trimmed === ""
        ? "nameEmpty"
        : kind === "local" && !isFullPath(command.trim())
          ? "commandNotAbsolute"
          : null;
    if (early !== null) {
      setRefusal(refusalOf(lang, platform, early));
      return;
    }
    const draft: McpServerDraft = {
      name: trimmed,
      kind,
      command: kind === "local" ? command.trim() : null,
      args:
        kind === "local"
          ? args
              .split("\n")
              .map((a) => a.trim())
              .filter((a) => a !== "")
          : [],
      url: kind === "remote" ? url.trim() : null,
      env: kind === "local" ? edits(env, "env", data) : [],
      headers: kind === "remote" ? edits(headers, "header", data) : [],
      enabled: editing?.enabled ?? true,
    };
    setSending(true);
    setRefusal(null);
    void save(editing?.name ?? null, draft).then(
      (answer) => {
        setSending(false);
        if (answer === null) onDone(trimmed);
        else setRefusal(refusalOf(lang, platform, answer));
      },
      () => {
        setSending(false);
        setRefusal(refusalOf(lang, platform, "secretStoreFailed"));
      },
    );
  };

  const escape = (event: KeyboardEvent<HTMLFormElement>) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onCancel();
  };

  const commandLine = m("Use the program's full path, such as {example}.", {
    example: commandExample(platform),
  });
  const urlLine = m("Use a full address, such as {example}.", { example: URL_EXAMPLE });
  return (
    <form
      className="set-tools-form"
      onSubmit={submit}
      onKeyDown={escape}
      noValidate
      data-testid="tools-form"
      data-kind={kind}
    >
      {editing === null && remote && (
        <SegmentedSwitch
          className="justify-self-start"
          label={m("Add a tool")}
          value={kind}
          options={[
            { value: "local", label: m("Command") },
            { value: "remote", label: m("Address") },
          ]}
          onValueChange={(next) => {
            setKind(next);
            setRefusal(null);
          }}
        />
      )}
      <FieldBox label={m("Name")} error={errorAt("name")}>
        {({ id, describedBy, invalid }) => (
          <input
            id={id}
            className={cn(textControl, "h-control font-en")}
            lang="en"
            spellCheck={false}
            autoComplete="off"
            ref={first}
            aria-invalid={invalid || undefined}
            aria-describedby={describedBy}
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            data-testid="tools-name"
          />
        )}
      </FieldBox>
      {kind === "local" ? (
        <>
          <FieldBox
            label={m("Command")}
            note={errorAt("command") === undefined ? commandLine : undefined}
            error={errorAt("command")}
          >
            {({ id, describedBy, invalid }) => (
              <input
                id={id}
                className={cn(textControl, "h-control font-en")}
                lang="en"
                spellCheck={false}
                autoComplete="off"
                aria-invalid={invalid || undefined}
                aria-describedby={describedBy}
                value={command}
                onChange={(e) => setCommand(e.currentTarget.value)}
                data-testid="tools-command"
              />
            )}
          </FieldBox>
          <FieldBox label={m("Arguments, one per line")}>
            {({ id }) => (
              <textarea
                id={id}
                rows={3}
                className={cn(textControl, "py-s2 resize-y font-en")}
                lang="en"
                spellCheck={false}
                value={args}
                onChange={(e) => setArgs(e.currentTarget.value)}
                data-testid="tools-args"
              />
            )}
          </FieldBox>
          <Secrets
            list="env"
            title={m("Environment values")}
            rows={env}
            setRows={setEnv}
            error={errorAt("env")}
            nextKey={nextKey}
          />
        </>
      ) : (
        <>
          <FieldBox
            label={m("Address")}
            note={errorAt("url") === undefined ? urlLine : undefined}
            error={errorAt("url")}
          >
            {({ id, describedBy, invalid }) => (
              <input
                id={id}
                type="url"
                className={cn(textControl, "h-control font-en")}
                lang="en"
                spellCheck={false}
                autoComplete="off"
                aria-invalid={invalid || undefined}
                aria-describedby={describedBy}
                value={url}
                onChange={(e) => setUrl(e.currentTarget.value)}
                data-testid="tools-url"
              />
            )}
          </FieldBox>
          <Secrets
            list="header"
            title={m("Headers")}
            rows={headers}
            setRows={setHeaders}
            error={errorAt("headers")}
            nextKey={nextKey}
          />
        </>
      )}
      {refusal?.place === "form" && (
        <p className={fieldError} role="status" data-testid="tools-error">
          {say(refusal.words)}
        </p>
      )}
      <div className="set-cn-actions">
        <Button kind="tone" size="small" type="submit" disabled={sending} data-testid="tools-save">
          {mt(lang, "Save")}
        </Button>
        <Button kind="quiet" size="small" onClick={onCancel} data-testid="tools-cancel">
          {ui(lang, "Cancel")}
        </Button>
      </div>
    </form>
  );
}
