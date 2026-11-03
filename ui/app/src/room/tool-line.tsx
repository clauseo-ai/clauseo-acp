// The tool line on the status line, after the plan: one glyph per tool, its state on it (on, slashed off, the firm's
// lock), in one quiet button named in words, its tooltip saying the same, that opens where each stands (DESIGN.md
// §Icons: no visible words). The popover, its rows, the Chrome download and their words are a chunk of their own
// (tool-popover.tsx), loaded with the line's own chunk (tool-line-room.ts), as the line first shows. Until it is in,
// the glyphs stand alone, named the same.
import { lazy, Suspense, use, useMemo } from "react";
import { ToolGlyph } from "@clauseo/components/icons/tool-glyph.tsx";
import { late } from "@clauseo/components/lib/late.ts";
import { browserInstallOf } from "./browser-install.ts";
import { ToolLineContext } from "./tool-line-context.ts";
import type { ToolLineRoom } from "./tool-line-room.ts";
import { glyphState, type KnownTool } from "./tools.ts";

const popover = late(import("./tool-popover.tsx"));
const ToolPopover = lazy(() => popover.then((m) => ({ default: m.ToolPopover })));

/** The tools' glyphs at the line's 14 px, web search first; hidden, as the button's name says them. */
export function ToolGlyphs({ tools }: { tools: readonly KnownTool[] }) {
  return tools.map((t) => {
    const state = glyphState(t);
    return (
      <span key={t.tool} className="contents" data-tool={t.tool} data-state={state}>
        <ToolGlyph glyph={t.tool === "web_search" ? "web" : "browser"} state={state} size={14} />
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
      install: transport === null ? null : browserInstallOf(transport),
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
