// The tool line's popover: one row per tool the open conversation's header speaks for, its glyph and name, an On or
// Off chip, and only when off, the reason in a few words (DESIGN.md §Icons). Under the browser with no Chrome (or
// Edge, on Windows), what to install, in full, and on a Mac (or Linux) Clauseo's own browser: Get a browser for
// Clauseo is the consent, with a line above it when the last download failed (TLS, a refused address, the server's
// error, a full disk, no network); then how far its download has got, its bar and Cancel (or
// the wait for the network, with Cancel), then that it is ready; or that the firm doesn't allow it. Under the browser
// the firm's policy blocks, or whose policy Clauseo couldn't check, the reason in full; for the policy not checked,
// Try again, which asks the host to read it now and waits (disabled, busy) until the read ends, then the row follows
// what it found: Ready for new conversations. A tool the lawyer turned off is turned on in Settings: one Open Settings
// at the popover's foot, however many rows are off there, its tooltip saying what a change reaches. Its own chunk,
// loaded as the status line first shows the tool line (tool-line.tsx).
import { useId, useSyncExternalStore, type ReactElement, type ReactNode } from "react";
import {
  DownloadBar,
  LineTip,
  ToolLinePopover,
} from "@clauseo/components/clauseo/tool-line-popover.tsx";
import { IconDownload, IconTick, IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { ToolGlyph } from "@clauseo/components/icons/tool-glyph.tsx";
import { cn } from "@clauseo/components/lib/cn.ts";
import { useToolsWords } from "@clauseo/components/lib/words-tools.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { chip } from "@clauseo/components/ui/recipes.ts";
import type { BrowserInstall, BrowserInstallStore } from "./browser-install.ts";
import type { BrowserPolicy, BrowserPolicyStore } from "./browser-policy.ts";
import type { ToolLineRoom } from "./tool-line-room.ts";
import { toolRow, type InstallPart, type RecheckPart } from "./tool-rows.ts";
import type { KnownTool } from "./tools.ts";

const nothing = () => () => {};
const unheard = (): BrowserPolicy | null => null;

function usePolicy(store: BrowserPolicyStore | null): BrowserPolicy | null {
  return useSyncExternalStore(
    store?.subscribe ?? nothing,
    store?.get ?? unheard,
    store?.get ?? unheard,
  );
}

const none = (): BrowserInstall | null => null;

function useInstall(store: BrowserInstallStore | null): BrowserInstall | null {
  return useSyncExternalStore(store?.subscribe ?? nothing, store?.get ?? none, store?.get ?? none);
}

/** Cancel, for a download under way or waiting for the network. */
function CancelInstall({ text, onCancel }: { text: string; onCancel: () => void }) {
  return (
    <Button kind="quiet" size="small" data-testid="tool-install-cancel" onClick={onCancel}>
      {text}
    </Button>
  );
}

/** Clauseo's own browser under the browser row with none: the offer (with its line, when it has one), the download with
 * Cancel, the wait for the network with Cancel, the firm's policy, or ready. */
function Install({
  part,
  onInstall,
  onCancel,
}: {
  part: InstallPart;
  onInstall: () => void;
  onCancel: () => void;
}) {
  switch (part.kind) {
    case "offer":
      return (
        <div className="tool-row-sub grid gap-s2">
          {part.line !== null && (
            <p className="text-small text-muted" data-testid="tool-install-line">
              {part.line}
            </p>
          )}
          <div className="flex items-center gap-s2">
            <Button
              kind="tone"
              size="small"
              data-testid="tool-install"
              data-waiting={part.waiting}
              disabled={part.waiting}
              aria-busy={part.waiting}
              onClick={onInstall}
            >
              <IconDownload size={16} />
              {part.button}
            </Button>
          </div>
        </div>
      );
    case "getting":
      return (
        <div className="tool-row-sub grid gap-s2" data-testid="tool-install-getting">
          {part.behind !== null && (
            <p className="text-small text-primary" data-testid="tool-install-behind">
              {part.behind}
            </p>
          )}
          {part.text !== "" && (
            <p className="text-small text-muted" data-testid="tool-install-line">
              {part.text}
            </p>
          )}
          <div className="flex items-center gap-s2">
            {/* the line above says how far in words; the bar's own figure is the percent alone */}
            <DownloadBar
              value={part.value}
              max={part.max}
              text={part.text === "" ? "" : `${part.percent}%`}
              name={part.text === "" ? part.cancel : part.text}
              className="min-w-0 flex-1"
            />
            <CancelInstall text={part.cancel} onCancel={onCancel} />
          </div>
        </div>
      );
    case "offline":
      return (
        <div className="tool-row-sub flex items-center gap-s2">
          <p className="min-w-0 flex-1 text-small text-muted" data-testid="tool-install-line">
            {part.line}
          </p>
          <CancelInstall text={part.cancel} onCancel={onCancel} />
        </div>
      );
    case "forbidden":
    case "done":
      return (
        <p className="tool-row-sub text-small text-muted" data-testid="tool-install-line">
          {part.line}
        </p>
      );
  }
}

/** Try again under the browser whose policy Clauseo couldn't check: disabled and busy while the read runs. */
function Recheck({ part, onRecheck }: { part: RecheckPart; onRecheck: () => void }) {
  return (
    <div className="tool-row-sub flex items-center gap-s2">
      <Button
        kind="tone"
        size="small"
        data-testid="tool-recheck"
        data-waiting={part.waiting}
        disabled={part.waiting}
        aria-busy={part.waiting}
        onClick={onRecheck}
      >
        <IconTryAgain size={16} />
        {part.button}
      </Button>
    </div>
  );
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

/** A reason on its own line under the row's name; once a policy read found nothing blocking, a tick before it and a
 * tooltip on when. */
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
  const { platform } = useDisplay();
  const policy = usePolicy(room.policy);
  const install = useInstall(room.install);
  const rows = tools.map((tool) =>
    toolRow(w.lang, tool, {
      settings: room.openSettings !== undefined,
      platform,
      policy,
      install,
    }),
  );
  const settings = rows.some((row) => row.settings);
  const onRecheck = () => room.policy?.recheck();
  const onInstall = () => room.install?.install();
  const onCancel = () => room.install?.cancel();
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
            {row.note !== null && (
              <p className="tool-row-sub text-small text-muted" data-testid="tool-row-full">
                {row.note}
              </p>
            )}
            {row.recheck !== null && <Recheck part={row.recheck} onRecheck={onRecheck} />}
            {row.install !== null && (
              <Install part={row.install} onInstall={onInstall} onCancel={onCancel} />
            )}
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
