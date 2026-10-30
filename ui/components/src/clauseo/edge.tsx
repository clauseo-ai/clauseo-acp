// Asking at the edge of the matter. Inside the matter's folder Clauseo works freely. At its edge, a folder beyond the
// matter, a website the matter hasn't used, or protection switched off, the thread asks in the room's permission
// pattern: on the well, what Clauseo wants, one plain line on what Clauseo does, and the answers with their keys,
// acting from the composer. Allowing hands the question to the computer's own dialog (os-dialog.tsx), and while it's
// up the prompt says so. Answered, it folds to one past-tense line. After 3 refusals in a row, or 20 in all, Clauseo
// stops and asks, in the same pattern, with what was refused.
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { useEffect, useId } from "react";
import { Button } from "../ui/button.tsx";
import { CollapsiblePanel } from "../ui/collapsible.tsx";
import { Kbd } from "../ui/kbd.tsx";
import {
  IconAccept,
  IconFold,
  IconFolder,
  IconNewMatter,
  IconNotProtected,
  IconWebsite,
} from "../icons/icons.tsx";
import type { Platform } from "../lib/display.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import { Say } from "./say.tsx";

/** Which edge: a folder beyond the matter, a website not used before, or protection switched off. */
export type EdgeKind = "folder" | "site" | "off";
/** Waiting on the lawyer; the computer's own dialog up; or answered, and whether the answer is kept for the matter. */
export type EdgeState =
  | { kind: "pending" }
  | { kind: "confirming" }
  | { kind: "allowed"; remembered: boolean; at: string }
  | { kind: "refused"; remembered: boolean };

/** Whether a key pressed where focus stands is the prompt's: in the composer's field, or with nothing focused. */
function fromComposer(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return true;
  if (target === target.ownerDocument.body) return true;
  return target instanceof HTMLTextAreaElement && target.form?.dataset["composer"] !== undefined;
}

/** Enter for the first answer, Escape for the second, from the composer, while the prompt waits. */
function useKeys(on: boolean, first: () => void, second: () => void) {
  useEffect(() => {
    if (!on) return;
    const onKey = (e: KeyboardEvent) => {
      if (
        e.defaultPrevented ||
        e.isComposing ||
        e.altKey ||
        e.metaKey ||
        e.ctrlKey ||
        e.shiftKey ||
        !fromComposer(e.target)
      )
        return;
      if (e.key === "Enter") first();
      else if (e.key === "Escape") second();
      else return;
      e.preventDefault();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [on, first, second]);
}

const ICON = { folder: IconFolder, site: IconWebsite, off: IconNotProtected } as const;

export function EdgePrompt({
  kind,
  place,
  state,
  platform,
  keys = false,
  onAllow,
  onRefuse,
}: {
  kind: EdgeKind;
  /** Where it reaches, as the thread names it: "Documents/Precedents", "example.org". */
  place: string;
  state: EdgeState;
  platform: Platform;
  /** Whether this prompt takes the keys: one prompt on the page at a time. */
  keys?: boolean;
  /** For a folder or a website, allowing; for protection, switching it off. Either opens the computer's dialog. */
  onAllow: () => void;
  onRefuse: () => void;
}) {
  const w = useVersionWords();
  const id = useId();
  const pending = state.kind === "pending";
  // protection's safe answer comes first and takes both keys: Escape dismisses, so a reflex Escape keeps protection,
  // and switching off has no key at all
  useKeys(keys && pending, kind === "off" ? onRefuse : onAllow, onRefuse);

  if (state.kind === "allowed" || state.kind === "refused") {
    const r = state.remembered;
    const said =
      kind === "off"
        ? state.kind === "allowed"
          ? w.v("Protection switched off for this matter at {time}.", { time: w.clock(state.at) })
          : w.v("Protection kept. Clauseo left the scanned annexures unread.")
        : state.kind === "allowed"
          ? kind === "folder"
            ? w.v(
                r
                  ? "Allowed for this matter. Clauseo can read {place}."
                  : "Allowed this time. Clauseo can read {place} for this answer.",
                { place },
              )
            : w.v(
                r
                  ? "Allowed for this matter. Clauseo can open {place}."
                  : "Allowed this time. Clauseo can open {place} for this answer.",
                { place },
              )
          : kind === "folder"
            ? w.v(
                r
                  ? "Not allowed for this matter. Clauseo keeps to this matter's folder."
                  : "Not allowed. Clauseo kept to this matter's folder.",
              )
            : w.v(
                r
                  ? "Not allowed for this matter. Clauseo won't open {place}."
                  : "Not allowed. Clauseo didn't open {place}.",
                { place },
              );
    return (
      <p className="perm-done edge-done" data-state={state.kind}>
        {state.kind === "refused" ? (
          <IconNewMatter size={14} className="perm-done-icon rotate-45" />
        ) : (
          <IconAccept size={14} className="perm-done-icon" />
        )}
        <span>
          <Say>{said}</Say>
        </span>
      </p>
    );
  }

  const Icon = ICON[kind];
  const ask =
    kind === "folder"
      ? w.v("Clauseo wants to read a folder outside this matter: {place}.", { place })
      : kind === "site"
        ? w.v("Clauseo wants to open a website this matter hasn't used before: {place}.", { place })
        : w.v(
            "Clauseo asks to work without protection in this matter, to read the scanned annexures with a program on this computer.",
          );
  const kept =
    kind === "off"
      ? w.v("Without protection, Clauseo can reach any file on this computer without asking.")
      : w.v("Clauseo works inside this matter's folder and asks before going beyond it.");
  const allow = (
    <Button
      kind={kind === "off" ? "quiet" : "tone"}
      size="small"
      className="edge-allow"
      aria-keyshortcuts={keys && kind !== "off" ? "Enter" : undefined}
      onClick={onAllow}
    >
      <span className="deva-nudge">{kind === "off" ? w.v("Switch off") : w.v("Allow")}</span>
      {keys && kind !== "off" && <Kbd className="perm-key">Enter</Kbd>}
    </Button>
  );
  const refuse = (
    <Button
      kind={kind === "off" ? "tone" : "quiet"}
      size="small"
      className="edge-refuse"
      aria-keyshortcuts={keys ? (kind === "off" ? "Enter Escape" : "Escape") : undefined}
      onClick={onRefuse}
    >
      <span className="deva-nudge">
        {kind === "off" ? w.v("Keep protection") : w.v("Don't allow")}
      </span>
      {keys && <Kbd className="perm-key">{kind === "off" ? "Enter" : "Esc"}</Kbd>}
    </Button>
  );
  return (
    <div
      className="perm edge"
      role="group"
      aria-labelledby={`${id}-ask`}
      aria-describedby={`${id}-kept`}
      data-kind={kind}
      data-state={state.kind}
    >
      <p className="perm-ask">
        <Icon size={16} className="perm-icon" />
        <span id={`${id}-ask`}>
          <Say>{ask}</Say>
        </span>
      </p>
      <p className="perm-kept" id={`${id}-kept`}>
        <Say>{kept}</Say>
      </p>
      <div className="perm-actions" inert={pending ? undefined : true}>
        {kind === "off" ? refuse : allow}
        {kind === "off" ? allow : refuse}
      </div>
      {state.kind === "confirming" && (
        <p className="edge-confirming" role="status">
          <Say>
            {platform === "mac"
              ? w.v("macOS is asking you to confirm.")
              : w.v("Windows is asking you to confirm.")}
          </Say>
        </p>
      )}
    </div>
  );
}

/** One thing the protected space refused, with its clock time. */
export type Refused = { at: string; what: string };

export function RefusalPause({
  count,
  refused,
  earlier,
  state,
  keys = false,
  onGoOn,
  onStop,
}: {
  /** 3 in a row, or 20 in the answer. */
  count: "row" | "total";
  /** The latest, newest last. */
  refused: readonly Refused[];
  /** How many came before those shown. */
  earlier: number;
  state: { kind: "pending" } | { kind: "goneOn" } | { kind: "stopped"; at: string };
  keys?: boolean;
  onGoOn: () => void;
  onStop: () => void;
}) {
  const w = useVersionWords();
  const id = useId();
  useKeys(keys && state.kind === "pending", onGoOn, onStop);
  if (state.kind !== "pending") {
    return (
      <p className="perm-done edge-done" data-state={state.kind}>
        <IconAccept size={14} className="perm-done-icon" />
        <span>
          <Say>
            {state.kind === "goneOn"
              ? w.v("You told Clauseo to go on inside the matter.")
              : w.v("You stopped the answer at {time}. The answer so far is kept.", {
                  time: w.clock(state.at),
                })}
          </Say>
        </span>
      </p>
    );
  }
  return (
    <div className="perm edge" role="group" aria-labelledby={`${id}-ask`} data-kind="refusals">
      <p className="perm-ask">
        <IconFolder size={16} className="perm-icon" />
        <span id={`${id}-ask`}>
          <Say>
            {count === "row"
              ? w.v(
                  "Clauseo stopped to ask you. Its last 3 tries went beyond this matter and were refused.",
                )
              : w.v(
                  "Clauseo stopped to ask you. 20 tries in this answer went beyond this matter and were refused.",
                )}
          </Say>
        </span>
      </p>
      <p className="perm-kept">
        <Say>{w.v("Nothing beyond the matter was read or changed.")}</Say>
      </p>
      <BaseCollapsible.Root className="edge-refused" defaultOpen>
        <BaseCollapsible.Trigger className="edge-refused-head">
          <IconFold size={13} className="edge-refused-chev" />
          <span className="deva-nudge">{w.v("What was refused")}</span>
        </BaseCollapsible.Trigger>
        <CollapsiblePanel>
          <ul className="edge-refused-list">
            {earlier > 0 && (
              <li className="edge-refused-row edge-refused-earlier">
                <span className="work-at" />
                <span>{w.v("{n} earlier", { n: earlier })}</span>
              </li>
            )}
            {refused.map((r) => (
              <li key={`${r.at}${r.what}`} className="edge-refused-row">
                <span className="work-at">{w.clock(r.at)}</span>
                <span>
                  <Say>{r.what}</Say>
                </span>
              </li>
            ))}
          </ul>
        </CollapsiblePanel>
      </BaseCollapsible.Root>
      <div className="perm-actions">
        <Button
          kind="tone"
          size="small"
          aria-keyshortcuts={keys ? "Enter" : undefined}
          onClick={onGoOn}
        >
          <span className="deva-nudge">{w.v("Go on inside the matter")}</span>
          {keys && <Kbd className="perm-key">Enter</Kbd>}
        </Button>
        <Button
          kind="quiet"
          size="small"
          aria-keyshortcuts={keys ? "Escape" : undefined}
          onClick={onStop}
        >
          <span className="deva-nudge">{w.v("Stop the answer")}</span>
          {keys && <Kbd className="perm-key">Esc</Kbd>}
        </Button>
      </div>
    </div>
  );
}
