// The settings window's frame. Settings opens as a window of its own on both computers, never as a sheet over the
// room: a matter stays in view and keeps working beside it, and on the Mac it is where Clauseo > Settings and ⌘, lead,
// as every Mac app's settings do. The sections stand on the bare ground, each saying where it stands, as the rail of
// matters does; the open section stands on the work's raised surface. This is a part: it knows no host and no
// settings. The app passes the sections, which is open, the window's wiring and the open section's body.
import { useEffect, useRef, type ReactNode } from "react";
import type { Platform } from "../lib/display.ts";
import { say } from "../lib/words-ui.ts";
import { useSettingsWords } from "../lib/words-settings.ts";
import { CaptionButtons, CornerLights, type CaptionWiring } from "./caption.tsx";
import { Heading } from "./heading.tsx";

/** A section as the window's side lists it: its name, and where it stands now. */
export type SettingsNavItem = { id: string; name: string; note?: string };

/** The window: on Windows and Linux a title bar with the caption buttons, on the Mac the corner lights; the sections
 * down the side, the open one marked; and the open section's head and body in the pane, which starts at its top each
 * time the section changes. Settings is a page of its own, so the window's name is its one h1 (the title bar's on
 * Windows and Linux, the side's head on the Mac), the section's name an h2 and its groups h3. */
export function SettingsWindow({
  platform,
  title,
  sections,
  current,
  onSection,
  lead,
  caption,
  nativeLights = false,
  children,
}: {
  platform: Platform;
  /** The window's name: on the Mac the side's head, elsewhere the title bar's. */
  title: string;
  sections: readonly SettingsNavItem[];
  /** The open section's id. */
  current: string;
  onSection: (id: string) => void;
  /** Optional: the open section's one line, under its name. */
  lead?: string | undefined;
  /** Optional: the caption buttons' wiring, off the Mac (ui/app's useWindowFrame). */
  caption?: CaptionWiring | undefined;
  /** Whether macOS draws its own lights in the corner lights' place. */
  nativeLights?: boolean;
  /** The open section's body. */
  children: ReactNode;
}) {
  const w = useSettingsWords();
  const pane = useRef<HTMLDivElement>(null);
  // a new section starts at its top
  useEffect(() => {
    pane.current?.scrollTo?.({ top: 0 });
  }, [current]);
  const open = sections.find((s) => s.id === current);
  return (
    <div
      className="set-win"
      data-platform={platform}
      role="dialog"
      aria-modal="false"
      aria-label={title}
    >
      {platform === "mac" ? (
        <div className="set-win-lights" data-tauri-drag-region>
          <CornerLights native={nativeLights} />
        </div>
      ) : (
        <div className="set-win-bar" data-tauri-drag-region>
          <Heading level={1} className="set-win-title" data-tauri-drag-region>
            {say(title)}
          </Heading>
          <CaptionButtons {...caption} />
        </div>
      )}
      <nav className="set-side" aria-label={w.s("Sections")}>
        {platform === "mac" && (
          <Heading level={1} className="set-side-title">
            {say(title)}
          </Heading>
        )}
        <ul className="set-nav">
          {sections.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                className="set-nav-row"
                aria-current={s.id === current ? "page" : undefined}
                onClick={() => onSection(s.id)}
              >
                <span className="set-nav-name">{say(s.name)}</span>
                {s.note !== undefined && <span className="set-nav-note">{say(s.note)}</span>}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <div className="set-pane" ref={pane}>
        <div className="set-pane-inner" key={current}>
          <header className="set-pane-head">
            <Heading level={2} className="set-pane-title">
              {say(open?.name ?? "")}
            </Heading>
            {lead !== undefined && <p className="set-pane-lead">{say(lead)}</p>}
          </header>
          {children}
        </div>
      </div>
    </div>
  );
}
