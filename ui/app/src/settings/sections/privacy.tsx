// Settings, Privacy: what leaves this computer, and who sees it (design/03-system/src/pages/settings/sections-c.tsx,
// Privacy), holding only what is true now, icons first and in the fewest words (DESIGN.md §Icons). The group's note
// says once what goes to a plan; then one row per plan the window knows, naming where; for Claude, the provider
// Anthropic lists for its web search behind an info icon. Then a row for the websites Clauseo visits, its explanation behind an info
// icon, unless the host's switches have both tools off. Then the tools' switches, web search and the browser, each led
// by its glyph with the state on it, one helper line each, each flip one patch. A locked switch shows the host's value,
// its glyph locked, and the room's lock reason, "Managed by firm", in its helper's place. While the Browser switch is on and the app has no
// website check, the room's reason, "Not available yet", stands in its helper's place.
// Left out until something stands behind them: a case-law source's row, the row for Clauseo's anonymous counts, and the
// anonymous counts and crash report switches.
//
// The switches are the host's: SettingsView.tools, the effective `web_search` and `browser`, in get_settings and
// settings_changed, and a flip is HostCommand::SetToolSettings { patch }, refused whole with settings.locked when it
// names a locked key, then settings_changed. Only the settings window sends it (the main window's host refuses it),
// and this pane is the only place that does. A change applies from the next conversation's start; one already open
// keeps its tools. `locked` may list `tools.web_search` and `tools.browser`.
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { ToolGlyph } from "@clauseo/components/icons/tool-glyph.tsx";
import { tw } from "@clauseo/components/lib/words-tools.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import { InfoTip } from "@clauseo/components/ui/info-tip.tsx";
import { SettingRow } from "@clauseo/components/ui/settings-row.tsx";
import { PLAN_NAME } from "../../room/limits/plan-model.ts";
import type { AgentKind, ToolsSettingsView } from "../../transport/protocol.ts";
import type { SectionContext } from "../sections.tsx";
import { Group, PlateRow } from "./parts.tsx";
import { KINDS } from "./usage-note.ts";

/** The plans this window knows of, in the design's order: those in the limits view, or both while it has none. */
function plansOf(ctx: SectionContext): readonly AgentKind[] {
  const known = KINDS.filter((kind) => ctx.limits[kind] !== undefined);
  return known.length > 0 ? known : KINDS;
}

function Dest({ kind }: { kind: AgentKind }) {
  const w = useSettingsWords();
  const name = PLAN_NAME[kind];
  return (
    <div className="set-dest set-dest-alone" data-testid={`dest-${kind}`}>
      <p className="set-dest-to">
        <span>{w.say(w.s("To {name}, on your plan", { name }))}</span>
        {kind === "claude" && (
          <InfoTip label={w.s("More about Brave Search")}>
            {w.say(w.s("Anthropic lists Brave Search as its web search subprocessor."))}
          </InfoTip>
        )}
      </p>
    </div>
  );
}

/** Whether the app can check each website the browser opens. Nothing can yet, so every conversation's browser is off
 * (the room's no_proxy). Stage C's proxy flips it. */
const websiteCheck = false;

/** The websites Clauseo visits: the pages it reads, and those its browser opens, go to them from this computer. */
function WebsitesDest() {
  const w = useSettingsWords();
  return (
    <div className="set-dest set-dest-alone" data-testid="dest-websites">
      <p className="set-dest-to">
        <span>{w.say(w.s("To the websites Clauseo visits"))}</span>
        <InfoTip label={w.s("About websites")}>
          {w.say(w.s("Websites see Clauseo's visits, as they see any browser's."))}
        </InfoTip>
      </p>
    </div>
  );
}

function ToolRow({
  ctx,
  tools,
  tool,
  label,
  note,
  reason,
}: {
  ctx: SectionContext;
  tools: ToolsSettingsView;
  tool: keyof ToolsSettingsView;
  label: string;
  note: string;
  /** Optional: why the tool can't run yet, in the helper's place (one line at most). */
  reason?: string | undefined;
}) {
  const w = useSettingsWords();
  const locked = ctx.toolLocked(tool);
  const on = tools[tool];
  return (
    <PlateRow disabled={locked}>
      <SettingRow
        className="py-0"
        control="switch"
        icon={
          <ToolGlyph
            glyph={tool === "web_search" ? "web" : "browser"}
            state={locked ? "locked" : on ? "on" : "off"}
            size={16}
          />
        }
        label={label}
        note={locked ? toolsLine(w.lang, "Managed by firm") : (reason ?? note)}
        checked={on}
        disabled={locked}
        onCheckedChange={(next) => ctx.chooseTool(tool, next)}
      />
    </PlateRow>
  );
}

export function Privacy({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { tools } = ctx.settings;
  const webOff = tools !== undefined && !tools.web_search && !tools.browser;
  return (
    <>
      {/* what goes to a plan, said once for every plan, as the group's note */}
      <Group
        title={w.s("What leaves this computer")}
        note={w.s("Questions, the papers' parts they need, web searches.")}
      >
        {plansOf(ctx).map((kind) => (
          <Dest key={kind} kind={kind} />
        ))}
        {!webOff && <WebsitesDest />}
      </Group>
      {tools !== undefined && (
        <Group title={w.s("On the web")} note={tw(w.lang, "Changes apply to new conversations.")}>
          <ToolRow
            ctx={ctx}
            tools={tools}
            tool="web_search"
            label={w.s("Web search")}
            note={w.s("Search the web and read pages.")}
          />
          <ToolRow
            ctx={ctx}
            tools={tools}
            tool="browser"
            label={w.s("Browser")}
            note={w.s("Runs on this computer.")}
            reason={
              tools.browser && !websiteCheck ? toolsLine(w.lang, "Not available yet") : undefined
            }
          />
        </Group>
      )}
    </>
  );
}
