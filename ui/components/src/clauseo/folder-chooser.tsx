// Clauseo's own folder chooser, for Windows when its folder window would stall on a share that is down: the desktop
// lists the folders (crates/desktop/src/chooser.rs) and this shows them. A title and why it's here; where it is, as
// crumbs; the places to start (the drives, Home, Desktop, Last place); the folders, a listbox the arrows move through,
// Enter or a press opening one, Backspace going up; a typed location and Go; then Cancel and Choose this folder, which
// chooses the folder shown, never the highlighted one. Folders Clauseo refuses are listed greyed with why, and don't
// open. Escape cancels; focus stays inside. It opens over New matter: Base UI's Escape stops at the chooser, and a
// press outside doesn't close it. It shows before the desktop knows where it may start or what the first folder holds:
// "Looking…" stands, quiet, where each is still on its way. In a short window the places to start stand in a column
// beside the folders, and the folders take the height left (room/folder-chooser.css). The words are words-matters.ts's.
import { Dialog as Base } from "@base-ui/react/dialog";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { cn } from "../lib/cn.ts";
import { useMatterWords, type MatterKey } from "../lib/words-matters.ts";
import {
  dialogPlace,
  dialogPlane,
  dialogTitle,
  row,
  rowNote,
  scrim,
} from "../ui/recipes-overlays.ts";
import { fieldLabel, textControl } from "../ui/recipes-controls.ts";
import { Button } from "../ui/button.tsx";
import { IconFolder } from "../icons/icons.tsx";
import { Say } from "./say.tsx";

/** A place to start from, as the desktop gives it. */
export type ChooserStartPoint = {
  key: number;
  kind: "drive" | "home" | "desktop" | "last_pick";
  display: string;
};

/** Where to go: a start point, a folder of the listing by its index, up, a typed location, or the folder shown again. */
export type ChooserGo =
  | { kind: "start"; key: number }
  | { kind: "into"; index: number }
  | { kind: "up" }
  | { kind: "typed"; path: string }
  | { kind: "again" };

/** The folder shown and what reading it gave, as the desktop gives it. */
export type ChooserListing = {
  display: string;
  /** Its parts from the drive (`C:`), or from home, shown as an empty first part. */
  crumbs: readonly string[];
  state:
    | { kind: "folders"; items: readonly { name: string; refused: boolean }[]; more: boolean }
    | { kind: "no_answer" }
    | { kind: "not_found" }
    | { kind: "not_allowed" };
};

const START_WORDS: Record<Exclude<ChooserStartPoint["kind"], "drive">, MatterKey> = {
  home: "Home",
  desktop: "Desktop",
  last_pick: "Last place",
};

const STATE_LINE: Record<Exclude<ChooserListing["state"]["kind"], "folders">, MatterKey> = {
  no_answer: "This folder didn't answer in time.",
  not_found: "This folder can't be found.",
  not_allowed: "Clauseo can't use this folder.",
};

/** The row to highlight in a new listing: the folder just come up from, else the first. */
function highlightOf(next: ChooserListing, last: ChooserListing | null): number {
  if (next.state.kind !== "folders" || last === null) return 0;
  const n = next.crumbs.length;
  const prefix = last.crumbs.length > n && next.crumbs.every((c, i) => c === last.crumbs[i]);
  if (!prefix) return 0;
  const from = last.crumbs[n];
  return Math.max(
    0,
    next.state.items.findIndex((item) => item.name === from),
  );
}

export type FolderChooserProps = {
  open: boolean;
  /** The places to start; null while the desktop reads the drives. */
  starts: readonly ChooserStartPoint[] | null;
  /** The folder shown; null before the first listing lands. */
  listing: ChooserListing | null;
  /** A listing is on its way: the folders shown stay, quiet, and nothing moves until it lands. */
  busy?: boolean;
  /** Choose this folder was pressed and the desktop's own window asks to use it. */
  waiting?: boolean;
  onGo: (go: ChooserGo) => void;
  onChoose: () => void;
  onCancel: () => void;
};

export function FolderChooser({
  open,
  starts,
  listing,
  busy = false,
  waiting = false,
  onGo,
  onChoose,
  onCancel,
}: FolderChooserProps) {
  const w = useMatterWords();
  const listId = useId();
  const lineId = useId();
  const fieldId = useId();
  const list = useRef<HTMLDivElement>(null);
  const [typed, setTyped] = useState("");
  // the highlighted row, kept with the listing it belongs to so a new listing starts afresh
  const [hi, setHi] = useState<{ of: ChooserListing | null; at: number }>({ of: null, at: 0 });
  const [last, setLast] = useState<ChooserListing | null>(null);
  if (listing !== hi.of) {
    setHi({ of: listing, at: listing === null ? 0 : highlightOf(listing, last) });
    setLast(listing);
  }
  const items = listing?.state.kind === "folders" ? listing.state.items : [];
  const at = Math.min(hi.at, Math.max(0, items.length - 1));
  const still = busy || waiting;

  useEffect(() => {
    if (items.length === 0) return;
    // the options are the listbox's children, in order
    list.current?.children[at]?.scrollIntoView?.({ block: "nearest" });
  }, [at, items.length]);

  const go = (where: ChooserGo) => {
    if (!still) onGo(where);
  };
  const openRow = (index: number) => {
    const item = items[index];
    if (item === undefined || item.refused) return;
    go({ kind: "into", index });
  };
  const atRoot = listing !== null && listing.crumbs.length <= 1 && listing.crumbs[0] !== "";
  const canChoose = listing?.state.kind === "folders" && !still;

  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const move = (to: number) => {
      e.preventDefault();
      if (items.length > 0) setHi({ of: listing, at: Math.max(0, Math.min(items.length - 1, to)) });
    };
    if (e.key === "ArrowDown") move(at + 1);
    else if (e.key === "ArrowUp" && e.altKey) {
      e.preventDefault();
      if (!atRoot) go({ kind: "up" });
    } else if (e.key === "ArrowUp") move(at - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(items.length - 1);
    else if (e.key === "PageDown") move(at + 8);
    else if (e.key === "PageUp") move(at - 8);
    else if (e.key === "Enter") {
      e.preventDefault();
      openRow(at);
    } else if (e.key === "Backspace") {
      e.preventDefault();
      if (!atRoot) go({ kind: "up" });
    } else if (e.key.length === 1 && /\S/.test(e.key) && !e.ctrlKey && !e.metaKey) {
      // type-ahead: the next folder after the highlighted one whose name starts with the letter
      const key = e.key.toLowerCase();
      const n = items.length;
      for (let step = 1; step <= n; step += 1) {
        const i = (at + step) % n;
        if (items[i]?.name.toLowerCase().startsWith(key)) {
          move(i);
          break;
        }
      }
    }
  };

  const stateLine =
    listing === null || listing.state.kind === "folders"
      ? null
      : w.m(STATE_LINE[listing.state.kind]);
  const crumbs = listing?.crumbs ?? [];
  const where = crumbs.map((c) => (c === "" ? w.m("Home") : c));
  // what a screen reader hears as the chooser changes: the wait, a folder that didn't open, or where it is now
  const looking = w.m("Looking…");
  const heard = waiting
    ? w.m("Waiting for you to confirm…")
    : listing === null
      ? looking
      : busy
        ? ""
        : (stateLine ?? where.join(", "));

  return (
    <Base.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      disablePointerDismissal
    >
      <Base.Portal>
        <Base.Backdrop className={scrim} />
        <Base.Popup
          data-dialog-plane=""
          className={cn(dialogPlace, dialogPlane, "folder-chooser")}
          initialFocus={list}
          data-testid="folder-chooser"
        >
          <Base.Title className={dialogTitle}>
            <Say>{w.m("Choose the matter's folder")}</Say>
          </Base.Title>
          <Base.Description className="mt-s1-5 text-small text-muted">
            <Say>
              {w.m(
                "Windows' folder window is slow to open on this computer, so Clauseo shows its own.",
              )}
            </Say>
          </Base.Description>

          <div className="folder-chooser-body mt-s3">
            <div
              className="folder-chooser-starts flex flex-wrap gap-s1-5"
              data-testid="chooser-starts"
            >
              {starts === null && (
                <p
                  className="flex min-h-control-small items-center text-small text-muted"
                  data-testid="chooser-starts-looking"
                >
                  <Say>{looking}</Say>
                </p>
              )}
              {starts?.map((s) => (
                <Button
                  key={s.key}
                  kind="tone"
                  size="small"
                  disabled={still}
                  data-testid="chooser-start"
                  onClick={() => go({ kind: "start", key: s.key })}
                >
                  {s.kind === "drive" ? (
                    <span lang="en">{s.display}</span>
                  ) : (
                    w.m(START_WORDS[s.kind])
                  )}
                </Button>
              ))}
            </div>

            <div className="folder-chooser-where mt-s3 flex items-center gap-s2 min-w-0">
              <Button
                kind="quiet"
                size="small"
                disabled={still || atRoot || listing === null}
                data-testid="chooser-up"
                onClick={() => go({ kind: "up" })}
              >
                {w.m("Up")}
              </Button>
              <p
                className="folder-chooser-crumbs min-w-0 flex-1 text-small-row text-primary"
                title={where.join(" › ")}
                data-testid="chooser-where"
              >
                {where.map((c, i) => (
                  <span key={i} className="whitespace-nowrap">
                    {i > 0 && (
                      <span aria-hidden="true" className="px-s1 text-faint">
                        ›
                      </span>
                    )}
                    {crumbs[i] === "" ? <Say>{c}</Say> : <span lang="en">{c}</span>}
                  </span>
                ))}
              </p>
            </div>

            <div className="folder-chooser-list mt-s2" aria-busy={still || undefined}>
              <div
                ref={list}
                id={listId}
                role="listbox"
                tabIndex={0}
                aria-label={where.join(", ")}
                aria-activedescendant={items.length > 0 ? `${listId}-${at}` : undefined}
                aria-describedby={stateLine === null ? undefined : lineId}
                className={cn("outline-none", busy && "opacity-60")}
                data-testid="chooser-list"
                onKeyDown={onListKey}
              >
                {items.map((item, i) => (
                  <div
                    key={`${i}-${item.name}`}
                    id={`${listId}-${i}`}
                    role="option"
                    aria-selected={i === at}
                    aria-disabled={item.refused || undefined}
                    data-highlighted={i === at ? "" : undefined}
                    data-disabled={item.refused ? "" : undefined}
                    data-testid="chooser-item"
                    className={cn(row, "folder-chooser-row")}
                    onPointerDown={(e) => {
                      // the list keeps the focus: a press opens, as Enter does
                      e.preventDefault();
                      list.current?.focus();
                      setHi({ of: listing, at: i });
                    }}
                    onClick={() => openRow(i)}
                  >
                    <IconFolder size={16} className="shrink-0 text-muted" />
                    <span lang="en" className="min-w-0 flex-1 truncate">
                      {item.name}
                    </span>
                    {item.refused && (
                      <span className={cn(rowNote, "shrink-0")}>
                        <Say>{w.m("Can't be a matter")}</Say>
                      </span>
                    )}
                  </div>
                ))}
              </div>
              {listing === null && (
                <p className="px-s3 py-s1-5 text-small text-muted" data-testid="chooser-looking">
                  <Say>{looking}</Say>
                </p>
              )}
              {listing?.state.kind === "folders" && listing.state.more && (
                <p className={cn(rowNote, "px-s3 py-s1-5")} data-testid="chooser-more">
                  <Say>{w.m("Some folders aren't shown.")}</Say>
                </p>
              )}
              {stateLine !== null && (
                <div className="grid justify-items-start gap-s2 px-s3 py-s2">
                  <p id={lineId} className="text-small text-primary" data-testid="chooser-state">
                    <Say>{stateLine}</Say>
                  </p>
                  {listing?.state.kind === "no_answer" && (
                    <Button
                      kind="tone"
                      size="small"
                      disabled={still}
                      data-testid="chooser-again"
                      onClick={() => go({ kind: "again" })}
                    >
                      {w.m("Try again")}
                    </Button>
                  )}
                </div>
              )}
            </div>
          </div>

          <form
            className="folder-chooser-typed mt-s3 grid gap-s1-5"
            onSubmit={(e) => {
              e.preventDefault();
              const path = typed.trim();
              if (path === "") return;
              go({ kind: "typed", path });
            }}
          >
            <label htmlFor={fieldId} className={cn(fieldLabel, "text-small")}>
              <Say>{w.m("Or type a folder's location")}</Say>
            </label>
            <div className="flex gap-s2">
              <input
                id={fieldId}
                lang="en"
                className={cn(textControl, "h-control flex-1 font-en")}
                value={typed}
                placeholder="C:\Matters\Iyer"
                autoComplete="off"
                spellCheck={false}
                data-testid="chooser-typed"
                onChange={(e) => setTyped(e.target.value)}
              />
              <Button
                type="submit"
                kind="tone"
                disabled={still || typed.trim() === ""}
                data-testid="chooser-go"
              >
                {w.m("Go")}
              </Button>
            </div>
          </form>

          <div
            className="folder-chooser-acts mt-s4 flex flex-wrap items-center justify-end gap-s2"
            data-dialog-acts=""
          >
            {waiting && (
              <p className="mr-auto text-small text-muted" data-testid="chooser-waiting">
                <Say>{w.m("Waiting for you to confirm…")}</Say>
              </p>
            )}
            <Button data-testid="chooser-cancel" onClick={onCancel}>
              {w.m("Cancel")}
            </Button>
            <Button
              kind="primary"
              disabled={!canChoose}
              data-testid="chooser-choose"
              onClick={() => {
                if (canChoose) onChoose();
              }}
            >
              {w.m("Choose this folder")}
            </Button>
          </div>
          <p role="status" className="sr-only" data-testid="chooser-heard">
            {heard}
          </p>
        </Base.Popup>
      </Base.Portal>
    </Base.Root>
  );
}
