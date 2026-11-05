// The tool line's popover: one row per tool the open conversation's header speaks for, its glyph and name, an On or
// Off chip, and only when off, the reason in a few words (DESIGN.md §Icons). Under the browser with no Chrome, the
// download: Download Chrome is the consent, then its bar, Unpacking, then the reason Ready for new conversations or what
// went wrong with Try again. A tool
// the lawyer turned off is turned on in Settings: one Open Settings at the popover's foot, however many rows are off
// there, its tooltip saying what a change reaches. Its own chunk, loaded as the status line first shows the tool line
// (tool-line.tsx).
import { useId, useSyncExternalStore, type ReactElement, type ReactNode } from "react";
import {
  DownloadBar,
  LineTip,
  ToolLinePopover,
} from "@clauseo/components/clauseo/tool-line-popover.tsx";
import {
  IconBreak,
  IconDownload,
  IconTick,
  IconTryAgain,
} from "@clauseo/components/icons/icons.tsx";
import { ToolGlyph } from "@clauseo/components/icons/tool-glyph.tsx";
import { cn } from "@clauseo/components/lib/cn.ts";
import { useToolsWords } from "@clauseo/components/lib/words-tools.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { chip } from "@clauseo/components/ui/recipes.ts";
import type { BrowserInstall, BrowserInstallStore } from "./browser-install.ts";
import type { ToolLineRoom } from "./tool-line-room.ts";
import { toolRow, type DownloadPart } from "./tool-rows.ts";
import type { KnownTool } from "./tools.ts";

const nothing = () => () => {};
const none = (): BrowserInstall | null => null;

function useInstall(store: BrowserInstallStore | null): BrowserInstall | null {
  return useSyncExternalStore(store?.subscribe ?? nothing, store?.get ?? none, store?.get ?? none);
}

/** A button whose tooltip says more than its words, the same words its description, for assistive technology. The
 * tooltip opens `side` of it, so it never covers its row. */
function Described({
  tip,
  side,
  children,
}: {
  tip: string;
  side?: "top" | "bottom";
  children: (id: string) => ReactElement;
}) {
  const id = useId();
  return (
    <>
      <LineTip content={tip} side={side}>
        {children(id)}
      </LineTip>
      <span id={id} hidden>
        {tip}
      </span>
    </>
  );
}

function Download({ part, onInstall }: { part: DownloadPart; onInstall: () => void }) {
  const w = useToolsWords();
  switch (part.kind) {
    case "offer":
      return (
        <div className="tool-row-sub flex items-center gap-s2">
          <Described tip={part.tip} side="bottom">
            {(id) => (
              <Button
                kind="tone"
                size="small"
                data-testid="tool-download"
                aria-describedby={id}
                onClick={onInstall}
              >
                <IconDownload size={16} />
                {part.button}
              </Button>
            )}
          </Described>
          <span className="text-small text-muted" data-testid="tool-download-size">
            {part.size}
          </span>
        </div>
      );
    case "none":
      return (
        <p className="tool-row-sub text-small text-muted" data-testid="tool-row-note">
          {part.line}
        </p>
      );
    case "progress":
      return (
        <DownloadBar
          value={part.value}
          max={part.max}
          text={part.text}
          name={part.name}
          className="tool-row-sub"
        />
      );
    case "unpacking":
      // no bar: DESIGN's bars are determinate only, and nothing spins
      return (
        <p
          className="tool-row-sub flex items-center gap-s2 text-small text-primary"
          data-testid="tool-progress-text"
        >
          <IconDownload size={14} />
          {part.text}
        </p>
      );
    case "done":
      // the row's reason says it
      return null;
    case "failed":
      return (
        <div className="tool-row-sub flex items-center gap-s2 text-small text-primary">
          <IconBreak size={14} />
          <span data-testid="tool-row-note">{part.cause}</span>
          <LineTip content={w.t("Try again")}>
            <Button
              kind="quiet"
              size="small"
              shape="icon"
              aria-label={w.t("Try again")}
              data-testid="tool-retry"
              onClick={onInstall}
            >
              <IconTryAgain size={16} />
            </Button>
          </LineTip>
        </div>
      );
  }
}

/** A reason on its own line under the row's name; once Chrome is in, a tick before it and a tooltip on when. */
function Reason({ text, tip }: { text: string; tip: string | null }) {
  if (tip === null)
    return (
      <p className="tool-row-sub text-small text-muted" data-testid="tool-row-reason">
        {text}
      </p>
    );
  return (
    <Described tip={tip} side="bottom">
      {(id) => (
        // focusable, so the keyboard reaches its tooltip as the pointer does
        <p
          tabIndex={0}
          aria-describedby={id}
          className="tool-row-sub flex items-center gap-s2 justify-self-start rounded-ref text-small text-muted"
          data-testid="tool-row-reason"
        >
          <IconTick size={14} />
          {text}
        </p>
      )}
    </Described>
  );
}

export function ToolPopover({
  label,
  glyphs,
  tools,
  room,
}: {
  label: string;
  glyphs: ReactNode;
  tools: readonly KnownTool[];
  room: ToolLineRoom;
}) {
  const w = useToolsWords();
  const install = useInstall(room.install);
  const rows = tools.map((tool) =>
    toolRow(w.lang, tool, { settings: room.openSettings !== undefined, install }),
  );
  const settings = rows.some((row) => row.settings);
  const onInstall = () => room.install?.install();
  return (
    <ToolLinePopover label={label} glyphs={glyphs}>
      <ul className="grid">
        {rows.map((row) => (
          <li key={row.tool} className="tool-row" data-testid="tool-row" data-tool={row.tool}>
            <ToolGlyph
              glyph={row.tool === "web_search" ? "web" : "browser"}
              state={row.glyph}
              size={16}
            />
            <p className="text-ui font-ui text-primary deva-nudge" data-testid="tool-row-name">
              {row.name}
            </p>
            <span className="flex items-center gap-s2">
              <span
                className={cn(chip({ role: "for" }), row.on && "text-primary")}
                data-testid="tool-row-chip"
                data-on={row.on}
              >
                <span className="cap-trim">{row.chip}</span>
              </span>
              {row.reason !== null && row.beside && (
                <span className="text-small text-muted" data-testid="tool-row-reason">
                  {row.reason}
                </span>
              )}
            </span>
            {row.reason !== null && !row.beside && <Reason text={row.reason} tip={row.reasonTip} />}
            {row.download !== null && <Download part={row.download} onInstall={onInstall} />}
          </li>
        ))}
      </ul>
      {settings && (
        <div className="tool-pop-foot px-s3 py-s2" data-testid="tool-settings-foot">
          <Described tip={w.t("Changes apply to new conversations.")}>
            {(id) => (
              <Button
                kind="tone"
                size="small"
                data-testid="tool-settings"
                aria-describedby={id}
                onClick={() => room.openSettings?.()}
              >
                {w.t("Open Settings")}
              </Button>
            )}
          </Described>
        </div>
      )}
    </ToolLinePopover>
  );
}
