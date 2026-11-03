// Settings, Privacy: what leaves this computer, and who sees it (design/03-system/src/pages/settings/sections-c.tsx,
// Privacy), holding only what is true now. One row per plan the window knows, its words naming the web searches each
// question may send, and for Claude the provider Anthropic lists for its web search. Then the tools' switches, web
// search and the browser, each one patch, a locked one disabled with its managed line as Appearance's rows are.
// Then a row for the websites Clauseo visits, unless the host's switches have both tools off. A locked switch shows the
// host's value, and the lock's reason stands in place of its note. Under the Browser switch, while it is on and the app
// has no website check, the reason the conversations give for its being off.
// Left out until something stands behind them: Indian Kanoon's row, the row for Clauseo's anonymous counts, and the
// anonymous counts and crash report switches.
//
// The switches' values and writes are app-local (host-next.ts's ToolSettings, the fake's set_tool_settings): the host's
// SettingsView carries `ui` and `locked` only, so the real window shows no "On the web" group. Host work that makes it
// real (stage E): SettingsView.tools, the effective `web_search` and `browser`, in get_settings and settings_changed,
// and HostCommand::SetToolSettings { patch }, refused whole with settings.locked when it names a locked key, then
// settings_changed. A change applies from the next conversation's start; one already open keeps its tools.
// The locks are real already: `locked` may list `tools.web_search` and `tools.browser`.
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { tw } from "@clauseo/components/lib/words-tools.ts";
import { SettingRow } from "@clauseo/components/ui/settings-row.tsx";
import { PLAN_NAME } from "../../room/limits/plan-model.ts";
import type { ToolSettings } from "../../room/host-next.ts";
import type { AgentKind } from "../../transport/protocol.ts";
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
    <div className="set-dest" data-testid={`dest-${kind}`}>
      <p className="set-dest-to">{w.say(w.s("To {name}, on your plan", { name }))}</p>
      <div>
        <p className="set-dest-what">
          {w.say(
            w.s(
              "Each question, the parts of the papers it needs, and its web searches. {name} sees what each question sends it.",
              { name },
            ),
          )}
        </p>
        {kind === "claude" && (
          <p className="set-note">
            {w.say(w.s("Anthropic lists Brave Search as its web search subprocessor."))}
          </p>
        )}
      </div>
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
    <div className="set-dest" data-testid="dest-websites">
      <p className="set-dest-to">{w.say(w.s("To the websites Clauseo visits"))}</p>
      <p className="set-dest-what">
        {w.say(
          w.s(
            "When Clauseo reads a page on this computer or opens one in its browser, the website sees the visit, as it sees any browser's.",
          ),
        )}
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
  tools: ToolSettings;
  tool: keyof ToolSettings;
  label: string;
  note: string;
  /** Optional: a further line under the note, such as why the tool can't run yet. */
  reason?: string | undefined;
}) {
  const w = useSettingsWords();
  const locked = ctx.toolLocked(tool);
  return (
    <PlateRow disabled={locked}>
      <SettingRow
        className="py-0"
        control="switch"
        label={label}
        note={locked ? w.s("Managed by your organisation") : note}
        checked={tools[tool]}
        disabled={locked}
        managed={reason}
        onCheckedChange={(on) => ctx.chooseTool(tool, on)}
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
      <Group title={w.s("What leaves this computer")}>
        {plansOf(ctx).map((kind) => (
          <Dest key={kind} kind={kind} />
        ))}
        {!webOff && <WebsitesDest />}
      </Group>
      {tools !== undefined && (
        <Group
          title={w.s("On the web")}
          note={w.s("A change applies to new conversations. Ones already open keep what they had.")}
        >
          <ToolRow
            ctx={ctx}
            tools={tools}
            tool="web_search"
            label={w.s("Web search")}
            note={w.s("Clauseo can search the web and read pages. On unless you turn it off.")}
          />
          <ToolRow
            ctx={ctx}
            tools={tools}
            tool="browser"
            label={w.s("Browser")}
            note={w.s("Clauseo can open web pages in its own browser, on this computer.")}
            reason={
              tools.browser && !websiteCheck
                ? tw(w.lang, "Off until Clauseo can check each website the browser opens.")
                : undefined
            }
          />
        </Group>
      )}
    </>
  );
}
