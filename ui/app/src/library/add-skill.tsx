// Add a skill (RC A1, library L3): one field for a GitHub link or a skills.sh name, skills.sh's
// matches under it as the lawyer types (arrows and Enter pick one), Choose a zip and Choose a
// folder through the native dialog, then the preview: each skill found with its description (a
// tick each when there are several, all ticked), where they will be on, and Add. A refusal reads in
// the panel, with Try again or Look again where one mends it. The flow is add-skill-model.ts; the
// library's place and Settings › Skills both show it. Its sheet, add-skill.css, is each window's
// first load's (library.tsx, settings.css).
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Checkbox } from "@clauseo/components/ui/checkbox.tsx";
import { RadioGroup, RadioItem } from "@clauseo/components/ui/radio-group.tsx";
import { fieldLabel, textControl } from "@clauseo/components/ui/recipes-controls.ts";
import { useId, useState, type KeyboardEvent } from "react";
import type { AddonScope, SkillPreview } from "../transport/bindings.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import {
  readField,
  shownPreview,
  troubleOf,
  useAddSkill,
  type AddSkill,
} from "./add-skill-model.ts";
import { fromLine } from "./model.ts";

export type AddSkillProps = {
  transport: HostTransport;
  /** The matter the skills may go to alone, with its name; null offers every matter only. */
  matter: { id: MatterId; name: string } | null;
  /** Where the skills start on. */
  scope0: AddonScope;
  /** After Add: the list read again. */
  onAdded: () => void;
  /** Choose a folder's own act, where a folder adds at once (Settings › Skills' add_skill); left
   * out, a folder previews like a zip. */
  onFolder?: (() => void) | undefined;
};

function Field({ flow }: { flow: AddSkill }) {
  const { lang } = useDisplay();
  const id = useId();
  const [active, setActive] = useState(-1);
  const [closed, setClosed] = useState(false);
  const read = readField(flow.text);
  const busy = flow.phase.k === "previewing" || flow.phase.k === "adding";
  const shown = closed ? [] : flow.matches;
  const open = shown.length > 0;
  const pickAt = (i: number) => {
    const item = shown[i];
    if (item === undefined) return;
    flow.setText(item.id);
    flow.look({ skills_sh: item.id });
  };
  const keys = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" && open) {
      e.preventDefault();
      setActive((i) => (i + 1) % shown.length);
    } else if (e.key === "ArrowUp" && open) {
      e.preventDefault();
      setActive((i) => (i <= 0 ? shown.length - 1 : i - 1));
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      setClosed(true);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (open && active >= 0) pickAt(active);
      else if (read.kind === "from") flow.look(read.from);
    }
  };
  return (
    <div className="add-skill-field">
      <label htmlFor={`${id}-in`} className={fieldLabel}>
        {lb(lang, "Paste a GitHub link or a skills.sh name")}
      </label>
      <div className="add-skill-ask">
        <input
          id={`${id}-in`}
          data-testid="add-skill-field"
          className={`${textControl} add-skill-input`}
          value={flow.text}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={`${id}-list`}
          aria-describedby={`${id}-note`}
          aria-activedescendant={open && active >= 0 ? `${id}-o${active}` : undefined}
          autoComplete="off"
          spellCheck={false}
          lang="en"
          onChange={(e) => {
            flow.setText(e.target.value);
            setActive(-1);
            setClosed(false);
          }}
          onKeyDown={keys}
        />
        <Button
          kind="tone"
          data-testid="add-skill-look"
          disabled={busy || read.kind !== "from"}
          onClick={() => read.kind === "from" && flow.look(read.from)}
        >
          {flow.phase.k === "previewing" ? lb(lang, "Previewing…") : lb(lang, "Look at it")}
        </Button>
      </div>
      <ul id={`${id}-list`} role="listbox" className="add-skill-matches" hidden={!open}>
        {shown.map((item, i) => (
          <li
            key={item.id}
            id={`${id}-o${i}`}
            role="option"
            aria-selected={i === active}
            data-testid="add-skill-match"
            className="add-skill-match"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => pickAt(i)}
          >
            <bdi className="add-skill-match-name">{item.name}</bdi>
            <span className="add-skill-match-source" lang="en">
              {item.source}
            </span>
          </li>
        ))}
      </ul>
      <p id={`${id}-note`} className="add-skill-note">
        {lb(lang, "Your search words go to skills.sh. Nothing from your matters does.")}
      </p>
    </div>
  );
}

function Preview({
  flow,
  preview,
  p,
}: {
  flow: AddSkill;
  preview: SkillPreview;
  p: AddSkillProps;
}) {
  const { lang } = useDisplay();
  const id = useId();
  const many = preview.skills.length > 1;
  const adding = flow.phase.k === "adding";
  const expired = flow.phase.k === "failed" && flow.phase.code === "skill.preview_expired";
  const programs = preview.skills.some((s) => s.has_scripts && flow.pick.has(s.name));
  const from = fromLine(lang, { source: "added", from: preview.from } as AddonView);
  const choice = flow.scope === "all" ? "all" : "matter";
  return (
    <div className="add-skill-preview" data-testid="add-skill-preview">
      <p className="add-skill-found">
        <bdi>{from}</bdi>{" "}
        {many
          ? lb(lang, "It holds {n} skills. Tick the ones to add.", { n: preview.skills.length })
          : lb(lang, "It holds one skill.")}
      </p>
      <ul className="add-skill-skills">
        {preview.skills.map((s) => (
          <li key={s.name} className="add-skill-skill" data-testid="add-skill-skill">
            {many && (
              <Checkbox
                checked={flow.pick.has(s.name)}
                disabled={adding}
                label={s.name}
                onCheckedChange={(on) => flow.tick(s.name, on)}
              />
            )}
            <span className="add-skill-skill-text">
              <bdi className="add-skill-skill-name">{s.name}</bdi>
              {s.description !== "" && <span className="add-skill-desc">{s.description}</span>}
              {s.already_added && (
                <span className="add-skill-flag">
                  {lb(lang, "You have this one. Adding it again replaces yours.")}
                </span>
              )}
              {s.has_scripts && (
                <span className="add-skill-flag">{lb(lang, "Holds programs")}</span>
              )}
            </span>
          </li>
        ))}
      </ul>
      {programs && (
        <p className="add-skill-note" data-testid="add-skill-trust">
          {lb(
            lang,
            "Skills that hold programs can run them on this computer, with the same access you have. Add only skills you trust.",
          )}
        </p>
      )}
      <div className="add-skill-where">
        <p className={fieldLabel} id={`${id}-where`}>
          {lb(lang, "Use them in")}
        </p>
        {p.matter === null ? (
          <p className="add-skill-note">{lb(lang, "Every matter")}</p>
        ) : (
          <RadioGroup
            value={choice}
            aria-labelledby={`${id}-where`}
            disabled={adding}
            className="add-skill-choices"
            onValueChange={(v) =>
              flow.setScope(v === "all" || p.matter === null ? "all" : { matter: p.matter.id })
            }
          >
            {(
              [
                ["matter", lb(lang, "Only {matters}", { matters: p.matter.name })],
                ["all", lb(lang, "Every matter")],
              ] as const
            ).map(([value, name]) => (
              <span key={value} className="add-skill-choice">
                <RadioItem value={value} aria-labelledby={`${id}-${value}`} />
                <bdi id={`${id}-${value}`}>{name}</bdi>
              </span>
            ))}
          </RadioGroup>
        )}
      </div>
      <div className="add-skill-acts">
        <Button
          kind="primary"
          data-testid="add-skill-add"
          disabled={adding || expired || flow.pick.size === 0}
          onClick={flow.add}
        >
          {adding ? lb(lang, "Adding…") : lb(lang, "Add")}
        </Button>
        <Button kind="quiet" disabled={adding} onClick={flow.reset}>
          {lb(lang, "Cancel")}
        </Button>
      </div>
    </div>
  );
}

/** The flow's panel: the field and the dialogs' buttons, a refusal, then the preview. */
export function AddSkillPanel(p: AddSkillProps) {
  const { lang } = useDisplay();
  const flow = useAddSkill(p.transport, p.scope0, p.onAdded);
  const busy = flow.phase.k === "previewing" || flow.phase.k === "adding";
  const preview = shownPreview(flow.phase);
  const trouble = flow.phase.k === "failed" ? troubleOf(lang, flow.phase.code) : null;
  const files = p.transport.pickSkill !== undefined;
  return (
    <div className="add-skill" data-testid="add-skill" aria-busy={busy}>
      <p className="add-skill-note">
        {lb(lang, "Nothing is added until you've seen what it holds.")}
      </p>
      <Field flow={flow} />
      {(files || p.onFolder !== undefined) && (
        <div className="add-skill-files">
          {files && (
            <Button
              kind="quiet"
              size="small"
              data-testid="add-skill-zip"
              disabled={busy}
              onClick={() => flow.choose(true)}
            >
              {lb(lang, "Choose a zip")}
            </Button>
          )}
          <Button
            kind="quiet"
            size="small"
            data-testid="add-skill-folder"
            disabled={busy}
            onClick={() => (p.onFolder === undefined ? flow.choose(false) : p.onFolder())}
          >
            {lb(lang, "Choose a folder")}
          </Button>
        </div>
      )}
      {trouble !== null && (
        <div className="add-skill-trouble" role="alert" data-testid="add-skill-trouble">
          <p>{trouble.line}</p>
          {trouble.mend !== null && (
            <Button kind="tone" size="small" data-testid="add-skill-again" onClick={flow.again}>
              {trouble.mend === "again" ? lb(lang, "Try again") : lb(lang, "Look again")}
            </Button>
          )}
        </div>
      )}
      {preview !== null && <Preview flow={flow} preview={preview} p={p} />}
    </div>
  );
}
