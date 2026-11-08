// R18's entries in the thread: a folder, a file or a site the Safe workspace refused, and the answer paused after too
// many refusals. The card asks; it grants nothing. Allow sends `request_escalation`, the host draws the computer's own
// dialog, and the card waits, its buttons held, until that command comes back (the dialog closed); the entry's state
// then says what was answered. Not now sends `dismiss_denial`. A refusal of either says its code's words on the card.
// What the host never widens (`not_offered`) and the network's sign-in are one quiet line with no button. Answered,
// dismissed or ended, the card folds to one line: the path or host, a middle dot, what came of it, and while the
// header says the agent is restarting for an allowed folder or file, that too. The cards take no keys: they don't hold
// the answer up, so Enter and Escape stay the composer's. The permission row's layout (live.css .perm), its icons and
// its folded line. Every path, host and matter name goes through isolate.ts first: no control or bidi character of
// its own, wrapped in FSI … PDI, so it can't reorder the card's words or fake a line.
import { useCallback, useRef, useState, type ReactNode } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import {
  IconAccept,
  IconCross,
  IconFolder,
  IconTick,
  IconWebsite,
} from "@clauseo/components/icons/icons.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { isolate } from "@clauseo/components/lib/isolate.ts";
import { dn } from "@clauseo/components/lib/words-denials.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import { toHostError } from "../../transport/host-transport.ts";
import type {
  DenialEntry,
  DenialsPausedEntry,
  EntryId,
  ErrorCode,
  StreamId,
} from "../../transport/protocol.ts";
import { turnRunning } from "./live.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { useThreadActions } from "./session.ts";

/** The space above a row under another in its section (rows.tsx's ROW). */
const ROW = "not-first:mt-(--s3)";
const INDENTED = "text-small text-quiet mt-(--s0-5) pl-(--perm-indent)";

/** What a denial's kind shows; null for a kind from a newer Clauseo, which shows nothing. */
type Shown =
  | { kind: "folder" | "file"; place: string; other: string | null }
  | { kind: "site"; place: string; shared: boolean }
  | { kind: "upstream_auth" };

function shownOf(entry: DenialEntry): Shown | null {
  const k = entry.kind;
  if (typeof k === "string") return null;
  if (k.folder !== undefined)
    return { kind: "folder", place: k.folder.path, other: k.folder.other_matter?.name ?? null };
  if (k.file !== undefined)
    return { kind: "file", place: k.file.path, other: k.file.other_matter?.name ?? null };
  if (k.site !== undefined)
    return { kind: "site", place: k.site.host, shared: k.site.shared_hosting === true };
  if (k.upstream_auth !== undefined) return { kind: "upstream_auth" };
  return null;
}

/** The card's state as `data-state`: the entry's, or `asking` while the dialog is up. */
type CardState = DenialEntry["state"] | "asking";

const TITLE = {
  folder: "Allow access to this folder?",
  file: "Allow access to this file?",
  site: "Allow this site?",
} as const;
const ALLOW = { folder: "Allow access", file: "Allow access", site: "Allow this site" } as const;
const KEPT = {
  folder: "Clauseo kept this folder closed. It can't be allowed for this matter.",
  file: "Clauseo kept this file closed. It can't be allowed for this matter.",
  site: "Clauseo kept this site closed. It can't be allowed for this matter.",
} as const;
const RESTARTING = {
  folder: "Going on with the folder…",
  file: "Going on with the file…",
} as const;

/** A folded state's words, after the place. */
function foldedWords(
  lang: Lang,
  state: DenialEntry["state"],
  kind: "folder" | "file" | "site",
  restarting: boolean,
): string {
  switch (state) {
    case "allowed":
      return restarting && kind !== "site"
        ? `${dn(lang, "Allowed.")} ${dn(lang, RESTARTING[kind])}`
        : dn(lang, "Allowed.");
    case "refused":
      return dn(lang, "Not allowed.");
    case "dismissed":
      return dn(lang, "Not now.");
    case "not_offered":
      return dn(lang, KEPT[kind]);
    default:
      // expired, and any state from a newer Clauseo: nothing is left to ask
      return dn(lang, "This request has ended.");
  }
}

/** A tool call's refusal (Windows) the host never offers, of a folder or file in another matter: that matter's name. */
function crossMatterOf(entry: DenialEntry, shown: Shown): string | null {
  if (entry.state !== "not_offered" || shown.kind === "site" || shown.kind === "upstream_auth")
    return null;
  const source = entry.source;
  if (typeof source !== "object" || !("tool_call" in source)) return null;
  return shown.other;
}

const TRIED =
  "{name} tried to open a file from another matter, {matter}. Clauseo kept it closed. To share that matter's files, choose Use files from another matter.";

function ErrorLine({ lang, code }: { lang: Lang; code: ErrorCode | null }) {
  if (code === null) return null;
  return (
    <p className={`perm-error ${INDENTED} mt-(--s2)`} role="status" data-testid="denial-error">
      <Say>{errorWords(lang, code)}</Say>
    </p>
  );
}

/** One denial: its card while it waits, else its line. `restarting`: the header's `restarting_since` is set. */
export function DenialCard({
  id,
  entry,
  restarting,
}: {
  id: EntryId;
  entry: DenialEntry;
  restarting: boolean;
}) {
  const { lang } = useDisplay();
  const { escalate, dismissDenial } = useThreadActions();
  const agent = useAgentPlan();
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<ErrorCode | null>(null);
  // one press at a time, even two in one frame
  const flying = useRef(false);
  const pending = entry.state === "pending";
  const send = useCallback(
    (ask: boolean) => {
      if (!pending || flying.current) return;
      flying.current = true;
      setError(null);
      if (ask) setAsking(true);
      const done = ask ? escalate(entry.denial) : dismissDenial(entry.denial);
      done
        .catch((e: unknown) => setError(toHostError(e).code))
        .finally(() => {
          flying.current = false;
          setAsking(false);
        });
    },
    [pending, escalate, dismissDenial, entry.denial],
  );
  const shown = shownOf(entry);
  if (shown === null) return null;
  const data = {
    "data-testid": "denial",
    "data-entry-id": id,
    "data-kind": shown.kind,
  };

  if (shown.kind === "upstream_auth")
    return (
      <p className={`perm-done ${ROW}`} data-glyph="none" data-state={entry.state} {...data}>
        <span>
          <Say>{dn(lang, "Your network needs a sign-in Clauseo can't give.")}</Say>
        </span>
      </p>
    );

  const crossMatter = crossMatterOf(entry, shown);
  if (crossMatter !== null) {
    const own = agent?.name?.trim() ?? "";
    const name =
      agent?.kind === "claude"
        ? "Claude"
        : agent?.kind === "codex"
          ? "Codex"
          : own === ""
            ? dn(lang, "The AI tool")
            : own;
    return (
      <p
        className={`perm-done ${ROW}`}
        data-glyph="none"
        data-state={entry.state}
        data-cross-matter=""
        {...data}
      >
        <span>
          <Say>{dn(lang, TRIED, { name, matter: isolate(crossMatter) })}</Say>
        </span>
      </p>
    );
  }

  if (!pending) {
    const state = entry.state;
    const glyph: ReactNode =
      state === "allowed" ? (
        <IconAccept size={14} className="perm-done-icon" />
      ) : state === "refused" ? (
        <IconCross size={14} className="perm-done-icon" />
      ) : null;
    return (
      <div className={ROW}>
        <p
          className="perm-done"
          data-glyph={glyph === null ? "none" : undefined}
          data-state={state}
          {...data}
        >
          {glyph}
          <span>
            <bdi data-testid="denial-place">{isolate(shown.place)}</bdi>
            {" · "}
            <Say>{foldedWords(lang, state, shown.kind, restarting)}</Say>
          </span>
        </p>
        <ErrorLine lang={lang} code={error} />
      </div>
    );
  }

  const Icon = shown.kind === "site" ? IconWebsite : IconFolder;
  const line =
    shown.kind === "site"
      ? shown.shared
        ? dn(lang, "This address is shared by many sites. Allowing it allows all of them.")
        : null
      : dn(lang, "Clauseo will be able to read it for this matter only.");
  const other = shown.kind === "site" ? null : shown.other;
  const state: CardState = asking ? "asking" : "pending";
  return (
    <div
      className={`perm ${ROW}`}
      role="group"
      aria-labelledby={`${id}-ask`}
      aria-describedby={`${id}-place`}
      data-state={state}
      data-confirming={asking ? "" : undefined}
      {...data}
    >
      <p className="perm-ask">
        <Icon size={16} className="perm-icon" />
        <span id={`${id}-ask`}>
          <Say>{dn(lang, TITLE[shown.kind])}</Say>
        </span>
      </p>
      <p className={`perm-title ${INDENTED}`} id={`${id}-place`}>
        <bdi data-testid="denial-place">{isolate(shown.place)}</bdi>
      </p>
      {other !== null && (
        <p className="perm-kept" data-testid="denial-other-matter">
          <Say>{dn(lang, "This is in another matter: {matter}.", { matter: isolate(other) })}</Say>
        </p>
      )}
      {line !== null && (
        <p className="perm-kept" data-testid={shown.kind === "site" ? "denial-shared" : undefined}>
          <Say>{line}</Say>
        </p>
      )}
      <div className="perm-actions">
        <Button
          kind="tone"
          size="small"
          data-testid="denial-allow"
          aria-disabled={asking ? true : undefined}
          onClick={() => send(true)}
        >
          <IconTick size={16} />
          <span className="deva-nudge">{dn(lang, ALLOW[shown.kind])}</span>
        </Button>
        <Button
          kind="quiet"
          size="small"
          data-testid="denial-not-now"
          aria-disabled={asking ? true : undefined}
          onClick={() => send(false)}
        >
          <span className="deva-nudge">{dn(lang, "Not now")}</span>
        </Button>
      </div>
      {asking && (
        <p className="perm-confirming" role="status" data-testid="denial-asking">
          <Say>{dn(lang, "Waiting for your answer in the window Clauseo opened…")}</Say>
        </p>
      )}
      <ErrorLine lang={lang} code={error} />
    </div>
  );
}

const restartingPick = (v: StreamView | undefined): boolean =>
  v?.header?.header?.restarting_since !== undefined;
const runningPick = (v: StreamView | undefined): boolean => turnRunning(v?.header?.header?.status);

/** A denial's row, live or paged: the header's restart is read live either way. */
export function DenialRow({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: DenialEntry;
}) {
  const restarting = useStreamPart(useViewStore(), stream, restartingPick);
  return <DenialCard id={id} entry={entry} restarting={restarting} />;
}

/** The answer paused after too many refusals; Stop the answer while the thread still runs. Go on inside the matter
 * waits for the host's command for it (R18 PR 3b). */
export function DenialsPaused({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: DenialsPausedEntry;
}) {
  const w = useThreadWords();
  const { stop } = useThreadActions();
  const running = useStreamPart(useViewStore(), stream, runningPick);
  return (
    <div
      className={`perm ${ROW}`}
      role="group"
      aria-labelledby={`${id}-ask`}
      data-testid="denials-paused"
      data-entry-id={id}
      data-plain=""
    >
      <p className="perm-ask">
        <span id={`${id}-ask`}>
          <Say>
            {dn(w.lang, "Clauseo stopped after {n} blocked attempts outside this matter.", {
              n: entry.refused,
            })}
          </Say>
        </span>
      </p>
      {running && (
        <div className="perm-actions">
          <Button kind="quiet" size="small" data-testid="denials-paused-stop" onClick={stop}>
            {w.th("Stop the answer")}
          </Button>
        </div>
      )}
    </div>
  );
}
