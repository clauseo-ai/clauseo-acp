// The tool line's popover: one row per tool the open conversation's header speaks for, its name in primary, where it
// stands in a muted line, and the act it offers: under the browser with no Chrome, the download (Download Chrome is
// the consent; its progress stands here, the bar under its words). A tool the lawyer turned off is turned on in
// Settings: one Open Settings at the popover's foot, however many rows offer it, with what a change reaches.
// Its own chunk, loaded as the status line first shows the tool line (tool-line.tsx).
import { useSyncExternalStore } from "react";
import { Button } from "@clauseo/components/ui/button.tsx";
import { ToolLinePopover } from "@clauseo/components/clauseo/tool-line-popover.tsx";
import { Progress } from "@clauseo/components/ui/progress.tsx";
import { useToolsWords } from "@clauseo/components/lib/words-tools.ts";
import type { BrowserInstall, BrowserInstallStore } from "./browser-install.ts";
import type { ToolLineRoom } from "./tool-line-room.ts";
import { toolRow, type ToolAction } from "./tool-rows.ts";
import type { KnownTool } from "./tools.ts";

const nothing = () => () => {};
const none = (): BrowserInstall | null => null;

function useInstall(store: BrowserInstallStore | null): BrowserInstall | null {
  return useSyncExternalStore(store?.subscribe ?? nothing, store?.get ?? none, store?.get ?? none);
}

const ACTION_IDS: Record<ToolAction, string> = {
  settings: "tool-settings",
  download: "tool-download",
  retry: "tool-retry",
};

export function ToolPopover({
  summary,
  tools,
  room,
}: {
  summary: string;
  tools: readonly KnownTool[];
  room: ToolLineRoom;
}) {
  const w = useToolsWords();
  const install = useInstall(room.install);
  const act = (action: ToolAction) => {
    if (action === "settings") room.openSettings?.();
    else room.install?.install();
  };
  const actionWords = (action: ToolAction) =>
    w.t(
      action === "settings"
        ? "Open Settings"
        : action === "download"
          ? "Download Chrome"
          : "Try again",
    );
  const rows = tools.map((tool) => ({
    tool,
    row: toolRow(w.lang, tool, {
      plan: room.plan,
      settings: room.openSettings !== undefined,
      install,
    }),
  }));
  const settings = rows.some(({ row }) => row.action === "settings");
  return (
    <ToolLinePopover summary={summary}>
      <ul className="grid gap-s1">
        {rows.map(({ tool, row }) => {
          const action = row.action === "settings" ? null : row.action;
          return (
            <li
              key={tool.tool}
              className="grid gap-s1 px-s3 py-s2"
              data-testid="tool-row"
              data-tool={tool.tool}
            >
              <p className="text-ui font-ui text-primary deva-nudge">{row.name}</p>
              {row.lines.map((line) => (
                <p key={line} className="text-small text-muted" data-testid="tool-row-line">
                  {line}
                </p>
              ))}
              {row.progress !== null && (
                <Progress
                  value={row.progress.value}
                  max={row.progress.max}
                  label={row.progress.label}
                  className="mt-s1"
                />
              )}
              {action !== null && (
                <span className="mt-s1">
                  <Button
                    kind="tone"
                    size="small"
                    data-testid={ACTION_IDS[action]}
                    onClick={() => act(action)}
                  >
                    {actionWords(action)}
                  </Button>
                </span>
              )}
            </li>
          );
        })}
      </ul>
      {settings && (
        <div className="grid gap-s1 px-s3 pt-s1 pb-s2" data-testid="tool-settings-foot">
          <p className="text-small text-muted">{w.t("Changes apply to new conversations.")}</p>
          <span className="mt-s1">
            <Button
              kind="tone"
              size="small"
              data-testid={ACTION_IDS.settings}
              onClick={() => act("settings")}
            >
              {actionWords("settings")}
            </Button>
          </span>
        </div>
      )}
    </ToolLinePopover>
  );
}
