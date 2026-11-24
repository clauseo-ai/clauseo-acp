// The tool line on the status line, after the plan: per tool its glyph with its state on it (on,
// slashed off, the firm's lock), its name and On or Off, so a lawyer reads what each is and where it
// stands (issue 4 of 24 Nov: the glyphs alone said neither). One quiet button, named in words, its
// tooltip saying the same, that opens where each stands and how to change it. The popover, its rows and their words are a chunk of their own
// (tool-popover.tsx), loaded with the line's own chunk (tool-line-room.ts), as the line first shows. Until it is in,
// the glyphs stand alone, named the same.
import { lazy, Suspense, use, useMemo } from "react";
import { ToolGlyph } from "@clauseo/components/icons/tool-glyph.tsx";
import { cn } from "@clauseo/components/lib/cn.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { late } from "@clauseo/components/lib/late.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import { chip } from "@clauseo/components/ui/recipes.ts";
import { browserPolicyOf } from "./browser-policy.ts";
import { quietly } from "./open.ts";
import { CHROME_DOWNLOAD, ToolLineContext } from "./tool-line-context.ts";
import type { ToolLineRoom } from "./tool-line-room.ts";
import { glyphState, toolName, type KnownTool } from "./tools.ts";

const popover = late(import("./tool-popover.tsx"));
const ToolPopover = lazy(() => popover.then((m) => ({ default: m.ToolPopover })));

/** The tools at the line's 14 px, web search first: each glyph, its name, and On or Off in the
 * popover's chip. */
export function ToolGlyphs({ tools }: { tools: readonly KnownTool[] }) {
  const { lang } = useDisplay();
  return tools.map((t) => {
    const state = glyphState(t);
    const on = t.state === "available";
    return (
      <span key={t.tool} className="room-tool" data-tool={t.tool} data-state={state}>
        <ToolGlyph glyph={t.tool === "web_search" ? "web" : "browser"} state={state} size={14} />
        <span className="deva-nudge">{toolsLine(lang, toolName(t))}</span>
        <span className={cn(chip({ role: "for" }), on && "text-primary")} data-on={on}>
          <span className="cap-trim">{toolsLine(lang, on ? "On" : "Off")}</span>
        </span>
      </span>
    );
  });
}

export function ToolLine({ label, tools }: { label: string; tools: readonly KnownTool[] }) {
  const { plan, openSettings, transport } = use(ToolLineContext);
  const room = useMemo<ToolLineRoom>(
    () => ({
      plan,
      openSettings,
      policy: transport === null ? null : browserPolicyOf(transport),
      // a plain web link: the host opens it at once, with no confirm (HostTransport.openExternal)
      openChromeDownload:
        transport === null
          ? null
          : () => void quietly("open_external", transport.openExternal(CHROME_DOWNLOAD)),
    }),
    [plan, openSettings, transport],
  );
  const glyphs = <ToolGlyphs tools={tools} />;
  return (
    <Suspense
      fallback={
        <span className="room-plan room-tool-glyphs" role="img" aria-label={label}>
          {glyphs}
        </span>
      }
    >
      <ToolPopover label={label} glyphs={glyphs} tools={tools} room={room} />
    </Suspense>
  );
}
