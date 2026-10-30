// What this matter allows, and Not protected. The room's line under the question box says what the workspace does,
// "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.", and a press on it
// opens what the matter allows: what it always may reach, the places the lawyer allowed or refused for it, each with
// Remove, and whether protection is on. Where protection is off, or couldn't be set up, the same line says "Not
// protected" for as long as that lasts, with the open lock: a lasting mark in concrete, never in red, because it's a
// state of the matter the lawyer chose or the computer refused, not a failure.
import { Popover as BasePopover } from "@base-ui/react/popover";
import { useState, type ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import {
  IconFolder,
  IconNotProtected,
  IconSafeWorkspace,
  IconUndo,
  IconWebsite,
} from "../icons/icons.tsx";
import { useVersionWords } from "../lib/words-versions.ts";
import { FloatingCard } from "./floating-card.tsx";
import { Heading } from "./heading.tsx";
import { Say } from "./say.tsx";

/** A place the matter allows or refuses: a folder or a website, as the computer writes it, and when. */
export type Allowance = {
  id: string;
  kind: "folder" | "site";
  place: string;
  /** "Read only, allowed at 5:12 pm today", in the language. */
  note: string;
  refused?: boolean;
};

/** Why a matter is not protected: switched off, with when, or the computer wouldn't set it up. */
export type Unprotected = { why: "off"; since: string } | { why: "setup" };

/** The room's line, in either state: a press opens what the matter allows. */
export function WorkspaceLine({
  unprotected = null,
  open,
  onOpenChange,
  children,
}: {
  unprotected?: Unprotected | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The card's contents: AllowedList. */
  children: ReactNode;
}) {
  const w = useVersionWords();
  return (
    <BasePopover.Root open={open} onOpenChange={onOpenChange}>
      <BasePopover.Trigger
        className="ws-line"
        data-unprotected={unprotected !== null ? "" : undefined}
      >
        {unprotected === null ? (
          <>
            <IconSafeWorkspace size={14} className="ws-line-icon" />
            <span>
              <Say>
                {w.v(
                  "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.",
                )}
              </Say>
            </span>
          </>
        ) : (
          <NotProtectedMark
            {...(unprotected.why === "setup"
              ? { meaning: w.v("Clauseo asks before each change in this matter.") }
              : {})}
          />
        )}
      </BasePopover.Trigger>
      <FloatingCard side="top" className="allows">
        {children}
      </FloatingCard>
    </BasePopover.Root>
  );
}

/** Not protected, as a matter carries it for as long as it lasts, everywhere in the app: the open lock, "Not
 * protected." at chip weight in primary, then what it means in the muted tone, all in concrete and never red. On the
 * room's line it is the way into what the matter allows. The rail and the toolbar carry the short form, the lock and
 * "Not protected" alone. First launch's line, "Not protected." with Try again and Settings, takes this same mark. */
export function NotProtectedMark({
  short = false,
  meaning,
}: {
  short?: boolean;
  /** What Not protected means where it isn't protection switched off. When the workspace couldn't
   * be set up, or Windows was told no, Clauseo falls back to asking before each change, and the caller says so in its
   * own words: "Clauseo asks before each change in this matter." Without it, the sentence about protection switched
   * off stands. */
  meaning?: string;
}) {
  const w = useVersionWords();
  return (
    <span className="np-mark" data-short={short ? "" : undefined}>
      <IconNotProtected size={14} className="np-icon" />
      <span className="np-words">
        <b className="np-name">{short ? w.v("Not protected") : w.v("Not protected.")}</b>
        {!short && (
          <>
            {" "}
            <Say>
              {meaning ?? w.v("Clauseo can reach beyond this matter's folder without asking.")}
            </Say>
          </>
        )}
      </span>
    </span>
  );
}

function Place({ a, onRemove }: { a: Allowance; onRemove: (id: string) => void }) {
  const w = useVersionWords();
  const [gone, setGone] = useState(false);
  const Icon = a.kind === "folder" ? IconFolder : IconWebsite;
  if (gone) {
    return (
      <li className="allows-row allows-removed" role="status">
        <span className="allows-removed-words">
          <Say>{w.v("Removed. Clauseo asks again before using it.")}</Say>
        </span>
        <Button kind="quiet" size="small" className="allows-undo" onClick={() => setGone(false)}>
          <IconUndo size={16} />
          <span className="deva-nudge">{w.v("Undo")}</span>
        </Button>
      </li>
    );
  }
  return (
    <li className="allows-row">
      <Icon size={16} className="allows-icon" />
      <span className="allows-what">
        <span className="allows-place" lang="en">
          {a.place}
        </span>
        <span className="allows-note">
          <Say>{a.note}</Say>
        </span>
      </span>
      <Button
        kind="quiet"
        size="small"
        className="allows-remove"
        aria-label={`${w.v("Remove")} ${a.place}`}
        onClick={() => {
          setGone(true);
          onRemove(a.id);
        }}
      >
        <span className="deva-nudge">{w.v("Remove")}</span>
      </Button>
    </li>
  );
}

export function AllowedList({
  folder,
  allowed,
  refused,
  unprotected = null,
  onRemove,
  onProtection,
}: {
  /** The matter's own folder, as the computer writes it. */
  folder: string;
  allowed: readonly Allowance[];
  refused: readonly Allowance[];
  unprotected?: Unprotected | null;
  onRemove: (id: string) => void;
  /** Switch protection off, through the computer's own dialog, or back on. */
  onProtection: () => void;
}) {
  const w = useVersionWords();
  const fixed = [
    {
      icon: IconFolder,
      name: w.v("This matter's folder"),
      note: folder,
      en: true,
      what: w.v("Read and change"),
    },
    {
      icon: IconWebsite,
      name: w.v("Your Claude plan"),
      note: w.v("Your questions, and the parts of the papers they need"),
      en: false,
      what: null,
    },
    {
      icon: IconWebsite,
      name: "Indian Kanoon",
      note: w.v("Searches for judgments, without your papers"),
      en: false,
      what: null,
    },
  ];
  return (
    <div className="allows-card">
      <BasePopover.Title render={<Heading level={2} className="allows-title" />}>
        {w.v("What this matter allows")}
      </BasePopover.Title>
      <p className="allows-sub">
        <Say>
          {w.v("Clauseo works inside this matter's folder and asks before going beyond it.")}
        </Say>
      </p>
      <section className="allows-group">
        <Heading level={3} className="allows-head">
          {w.v("Always")}
        </Heading>
        <ul className="allows-list">
          {fixed.map((f) => (
            <li key={f.name} className="allows-row">
              <f.icon size={16} className="allows-icon" />
              <span className="allows-what">
                <span className="allows-place">
                  <Say>{f.name}</Say>
                </span>
                <span className="allows-note" lang={f.en ? "en" : undefined}>
                  {f.en ? f.note : <Say>{f.note}</Say>}
                </span>
              </span>
              {f.what !== null && <span className="allows-scope">{f.what}</span>}
            </li>
          ))}
        </ul>
      </section>
      {allowed.length > 0 && (
        <section className="allows-group">
          <Heading level={3} className="allows-head">
            {w.v("Allowed for this matter")}
          </Heading>
          <ul className="allows-list">
            {allowed.map((a) => (
              <Place key={a.id} a={a} onRemove={onRemove} />
            ))}
          </ul>
        </section>
      )}
      {refused.length > 0 && (
        <section className="allows-group">
          <Heading level={3} className="allows-head">
            {w.v("Refused for this matter")}
          </Heading>
          <ul className="allows-list">
            {refused.map((a) => (
              <Place key={a.id} a={a} onRemove={onRemove} />
            ))}
          </ul>
        </section>
      )}
      <div className="allows-foot" data-unprotected={unprotected !== null ? "" : undefined}>
        <p className="allows-protection">
          <span className="allows-protection-name">{w.v("Protection")}</span>
          <span className="allows-protection-state">
            <Say>
              {unprotected === null
                ? w.v(
                    "On. Clauseo works inside this matter's folder and asks before going beyond it.",
                  )
                : unprotected.why === "off"
                  ? w.v(
                      "Off since {time} today. Clauseo can reach any file on this computer without asking.",
                      { time: w.clock(unprotected.since) },
                    )
                  : w.v(
                      "Couldn't be set up on this computer. Clauseo asks before each change in this matter.",
                    )}
            </Say>
          </span>
        </p>
        <Button
          kind={unprotected === null ? "quiet" : "tone"}
          size="small"
          className="allows-switch"
          onClick={onProtection}
        >
          <span className="deva-nudge">
            {unprotected === null ? w.v("Switch off protection") : w.v("Switch protection on")}
          </span>
        </Button>
      </div>
    </div>
  );
}
