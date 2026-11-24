// Library L2's controls: the head's lens ("Switches for [matter | Every matter]"), a row's switch,
// the detail's "Where {who} uses it" with its tick boxes, the detail's foot (Remove, or why it
// stays), and Remove's confirmation. Each sends through addon-acts.ts; scope-model.ts decides
// what each may do.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { lb } from "@clauseo/components/lib/words-library.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Checkbox } from "@clauseo/components/ui/checkbox.tsx";
import { RadioGroup, RadioItem } from "@clauseo/components/ui/radio-group.tsx";
import { SegmentedSwitch } from "@clauseo/components/ui/segmented.tsx";
import { Switch } from "@clauseo/components/ui/switch.tsx";
import type { MatterId } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import { nameOf } from "./model.ts";
import {
  lockedOn,
  onIn,
  switchChanges,
  switchFor,
  usable,
  type AddonChange,
  type Lens,
} from "./scope-model.ts";

type Names = ReadonlyMap<MatterId, string>;

/** "Switches for [Sharma v. State of Delhi | Every matter]": the matter open last first. Without
 * a matter open, the switches are every matter's and there is nothing to choose. */
export function LensSwitch({
  here,
  names,
  lens,
  onLens,
}: {
  here: MatterId | null;
  names: Names;
  lens: Lens;
  onLens: (lens: Lens) => void;
}) {
  const { lang } = useDisplay();
  const name = here === null ? undefined : names.get(here);
  if (here === null || name === undefined) return null;
  return (
    <div className="lib-lens" data-testid="library-lens">
      <span className="text-small text-muted" id="lib-lens">
        {lb(lang, "Switches for")}
      </span>
      <SegmentedSwitch
        labelledBy="lib-lens"
        value={lens === "all" ? "all" : "matter"}
        options={[
          { value: "matter", label: name },
          { value: "all", label: lb(lang, "Every matter") },
        ]}
        onValueChange={(v) => onLens(v === "all" ? "all" : { matter: here })}
      />
    </div>
  );
}

/** What a row's and a detail's controls need: the lens, the matters, and the sender. */
export type SwitchActs = {
  lens: Lens;
  names: Names;
  /** The matter a change to "only some matters" starts with: the lens's, else the first. */
  first: MatterId | null;
  send: (a: AddonView, changes: readonly AddonChange[]) => void;
};

/** A row's switch, for the lens; none for a built-in skill. */
export function RowSwitch({ a, acts }: { a: AddonView; acts: SwitchActs }) {
  const { lang } = useDisplay();
  const state = switchFor(a, acts.lens, acts.names.size);
  if (state === null) return null;
  const name = nameOf(lang, a);
  const matter = acts.lens === "all" ? undefined : acts.names.get(acts.lens.matter);
  const label =
    matter === undefined
      ? lb(lang, "{name}, on in every matter", { name })
      : lb(lang, "{name}, on in {matter}", { name, matter });
  return (
    <Switch
      checked={state.checked}
      disabled={state.disabled}
      label={label}
      className="lib-row-switch"
      onCheckedChange={(on) => acts.send(a, switchChanges(a, acts.lens, on, acts.first))}
    />
  );
}

/** "Where {who} uses it": every matter (new ones too) or only the matters ticked, then one tick
 * box per matter. The choice is set_addon_scope; a tick is that matter's set_addon_on. A built-in
 * tool and a locked row keep their scope; under the firm's lock with scope "all" no tick moves. */
export function WhereBlock({ a, who, acts }: { a: AddonView; who: string; acts: SwitchActs }) {
  const { lang } = useDisplay();
  const scope = a.scope === "all" ? "all" : "matter";
  const fixed = a.source === "built_in" || a.locked || !usable(a);
  const ticksFixed = lockedOn(a) || !usable(a);
  const here = acts.lens === "all" ? null : acts.lens.matter;
  const choose = (v: string) => {
    if (fixed || v === scope) return;
    if (v === "all") acts.send(a, [{ set_addon_scope: { kind: a.kind, id: a.id, scope: "all" } }]);
    else if (acts.first !== null)
      acts.send(a, [
        { set_addon_scope: { kind: a.kind, id: a.id, scope: { matter: acts.first } } },
      ]);
  };
  const tick = (matter: MatterId, on: boolean) => {
    if (!ticksFixed) acts.send(a, [{ set_addon_on: { kind: a.kind, id: a.id, matter, on } }]);
  };
  const choices = [
    {
      value: "all",
      name: lb(lang, "Every matter"),
      note: lb(lang, "New matters too. Untick a matter to leave it out."),
      off: fixed,
    },
    {
      value: "matter",
      name: lb(lang, "Only the matters you tick"),
      note: lb(lang, "New matters start without it."),
      off: fixed || acts.first === null,
    },
  ];
  // each choice and tick box is named by its words beside it, which a press also reaches
  return (
    <section className="lib-detail-block" data-testid="library-where">
      <h4 className="lib-detail-label" id="lib-where">
        {lb(lang, "Where {who} uses it", { who })}
      </h4>
      <RadioGroup
        value={scope}
        aria-labelledby="lib-where"
        disabled={fixed}
        onValueChange={(v) => choose(String(v))}
        className="lib-choices"
      >
        {choices.map((c) => (
          <div key={c.value} className="lib-choice">
            <RadioItem
              value={c.value}
              disabled={c.off}
              aria-labelledby={`lib-choice-${c.value}`}
              aria-describedby={`lib-choice-${c.value}-note`}
            />
            <span onClick={() => !c.off && choose(c.value)}>
              <span className="lib-choice-name" id={`lib-choice-${c.value}`}>
                {c.name}
              </span>
              <span className="lib-choice-note" id={`lib-choice-${c.value}-note`}>
                {c.note}
              </span>
            </span>
          </div>
        ))}
      </RadioGroup>
      <ul className="lib-matters" aria-labelledby="lib-where">
        {[...acts.names].map(([id, name]) => {
          const on = usable(a) && onIn(a, id);
          return (
            <li key={id} className="lib-matter" data-testid="library-tick" data-matter={id}>
              <Checkbox
                checked={on}
                disabled={ticksFixed}
                label={name}
                onCheckedChange={(next) => tick(id, next)}
              />
              <bdi className="min-w-0 truncate" onClick={() => tick(id, !on)}>
                {name}
              </bdi>
              {id === here && (
                <span className="text-small text-quiet">{lb(lang, "open last")}</span>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** The detail's foot: when a change applies, then Remove, or why it stays: it comes with Clauseo,
 * or the firm decides. */
export function DetailFoot({ a, onRemove }: { a: AddonView; onRemove: () => void }) {
  const { lang } = useDisplay();
  const stays =
    a.source === "built_in"
      ? a.kind === "skill"
        ? lb(lang, "Comes with Clauseo, so it stays on in every matter.")
        : lb(lang, "Comes with Clauseo, so it stays. Turn it off where you don't want it.")
      : a.locked
        ? lb(lang, "Your firm decides where this is used and whether it stays.")
        : null;
  return (
    <footer className="lib-detail-foot">
      {a.kind === "skill" && a.source === "built_in" ? null : (
        <p className="text-small text-muted">
          {lb(lang, "Changes apply from each conversation's next message.")}
        </p>
      )}
      {stays !== null ? (
        <p className="text-small text-muted" data-testid="library-stays">
          {stays}
        </p>
      ) : a.can_remove ? (
        <div className="lib-detail-acts">
          <Button kind="quiet" size="small" onClick={onRemove} data-testid="library-remove">
            {lb(lang, "Remove")}
          </Button>
        </div>
      ) : null}
    </footer>
  );
}

/** "Remove Notice drafting?": what removing does, Cancel first (it can't be undone). */
export function RemoveConfirm({
  a,
  onClose,
  onRemove,
}: {
  a: AddonView;
  onClose: () => void;
  onRemove: () => void;
}) {
  const { lang } = useDisplay();
  const body =
    a.kind === "skill"
      ? lb(
          lang,
          "Clauseo deletes its files from this computer and turns it off in every matter. To use it again, add it again.",
        )
      : lb(
          lang,
          "Clauseo removes it and its saved keys from this computer, and turns it off in every matter.",
        );
  return (
    <AlertDialog open onOpenChange={(open) => (open ? undefined : onClose())}>
      <AlertDialogPopup
        title={lb(lang, "Remove {name}?", { name: nameOf(lang, a) })}
        body={body}
        undoable={false}
        data-testid="library-remove-confirm"
        actions={
          <>
            <AlertDialogClose render={<Button kind="quiet" />}>
              {lb(lang, "Cancel")}
            </AlertDialogClose>
            <Button kind="primary" onClick={onRemove}>
              {lb(lang, "Remove")}
            </Button>
          </>
        }
      />
    </AlertDialog>
  );
}
