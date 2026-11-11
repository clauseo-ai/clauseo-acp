// Settings › Tools (UC-169 on): the lawyer's own MCP servers, called tools. The section's lead says plainly what a tool
// can reach; under it, where this build has the Safe workspace, that the Safe workspace doesn't use them. Then one
// Import button per app the host found with servers to import (the view's `importable`), leading; the pressed one says
// Importing… while the host's confirm and the import run, and after it the lines on how it went (`last_import`), or the
// host's refusal; Cancel in the confirm leaves the section as it was. Then the
// list: each tool's name, a quiet detail (the program a launcher runs, else its program's file name, or its address's
// host), its switch, Edit and Remove (through a confirm); or, with none, one line. Add a tool comes last, secondary
// while there is something to import. The form (mcp-tools-form.tsx) stands in the list's
// place while open, and on Save or Cancel focus goes back to the row or the button that opened it. A firm's policy list
// is read-only with the lock's treatment (its rows greyed, the switches off limits) and says the firm set it; the
// firm's own MCP shows one line and no list. The view is the host's, sent at the catch-up and after every change
// (mcp_tools_changed); every command goes through tools-model.ts's ToolsCommands.
import { IconCommand } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { mw } from "@clauseo/components/lib/words-mcp-tools.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { Switch } from "@clauseo/components/ui/switch.tsx";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { SectionContext } from "../sections.tsx";
import { carriesProtectionWire } from "./workspace-shown.ts";
import { ToolForm } from "./mcp-tools-form.tsx";
import { importLines, refusalOf, SOURCE_NAME } from "./mcp-tools-refusal.ts";
import { Group, PlateRow } from "./parts.tsx";
import {
  hostOf,
  localDetail,
  remoteOffered,
  SAFE_WORKSPACE_NOTE,
  toolsCommandsOf,
  type ImportSource,
  type McpServerView,
  type ToolsView,
  type ToolsAnswer,
  type ToolsCommands,
} from "./tools-model.ts";

/** The form's state: closed, adding, or editing a tool. */
type Open = null | { editing: McpServerView | null };

/** A server's quiet detail: its program's file name, or its address's host. */
const detailOf = (server: McpServerView): string =>
  server.kind === "remote"
    ? hostOf(server.url ?? "")
    : localDetail(server.command ?? "", server.args);

/** The view the host sends (`mcp_tools_changed`): null before the catch-up, then each one as it comes. */
function useToolsView(commands: ToolsCommands): ToolsView | null {
  const [over] = useState(() => ({
    subscribe: (wake: () => void) => commands.subscribe(() => wake()),
    view: () => commands.current(),
  }));
  return useSyncExternalStore(over.subscribe, over.view, over.view);
}

export function McpTools({ ctx }: { ctx: SectionContext }) {
  const commands = toolsCommandsOf(ctx.transport);
  if (commands === null) return null;
  return (
    <Tools commands={commands} safe={SAFE_WORKSPACE_NOTE && carriesProtectionWire(ctx.transport)} />
  );
}

function Tools({ commands, safe }: { commands: ToolsCommands; safe: boolean }) {
  const { lang } = useDisplay();
  const view = useToolsView(commands);
  const [open, setOpen] = useState<Open>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [importedLines, setImportedLines] = useState<string[] | null>(null);
  const [importFailed, setImportFailed] = useState<string | null>(null);
  // the app being imported from, while its import (and the host's confirm) is in flight
  const [importing, setImporting] = useState<ImportSource | null>(null);
  // where focus goes once the list is back: a tool's Edit (its name), or Add a tool ("")
  const back = useRef<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open !== null || back.current === null || root.current === null) return;
    const name = back.current;
    const target =
      [...root.current.querySelectorAll<HTMLElement>("[data-tool-edit]")].find(
        (b) => b.dataset["toolEdit"] === name,
      ) ?? root.current.querySelector<HTMLElement>('[data-testid="tools-add"]');
    if (target === null) return;
    back.current = null;
    target.focus();
  });

  const answered = async (answer: Promise<ToolsAnswer>) => {
    const refusal = await answer.catch(() => "mcp.secret_store_failed" as const);
    setFailed(
      refusal === null || refusal === "mcp.save_declined" ? null : refusalOf(lang, refusal).words,
    );
  };

  if (view === null) return null;
  if (view.control === "firm_managed")
    return (
      <p className="set-note" data-testid="tools-section" data-control="firm_managed">
        {say(mw(lang, "Your firm manages tools on this computer."))}
      </p>
    );
  const firm = view.control === "firm_list";
  const imports = firm ? [] : view.importable;
  const importFrom = async (source: ImportSource) => {
    if (importing !== null) return;
    setImporting(source);
    setFailed(null);
    const answer = await commands.mcpImport(source).catch(() => "mcp.secret_store_failed" as const);
    setImporting(null);
    // Cancel in the host's confirm: the section as it was, the last import's lines included
    if (answer === "mcp.save_declined") return;
    setImportedLines(null);
    setImportFailed(null);
    if (answer === null) return;
    if (typeof answer === "object") setImportedLines(importLines(lang, answer));
    else setImportFailed(refusalOf(lang, answer).words);
  };
  const close = (name: string | null) => {
    back.current = name ?? "";
    setOpen(null);
  };
  return (
    <div ref={root} className="set-tools" data-testid="tools-section" data-control={view.control}>
      {safe && (
        <p className="set-note" data-testid="tools-safe-note">
          {say(mw(lang, "Not used in the Safe workspace."))}
        </p>
      )}
      {open !== null ? (
        <Group title={open.editing?.name ?? mw(lang, "Add a tool")}>
          <PlateRow>
            <ToolForm
              editing={open.editing}
              remote={remoteOffered(commands)}
              save={(previousName, draft) => commands.save(previousName, draft)}
              onDone={(name) => {
                setFailed(null);
                close(name);
              }}
              onCancel={() => close(open.editing?.name ?? null)}
            />
          </PlateRow>
        </Group>
      ) : (
        <>
          {imports.length > 0 && (
            <div className="set-cn-actions" data-testid="tools-imports">
              {imports.map(({ source, count }) => (
                <Button
                  key={source}
                  kind="tone"
                  size="small"
                  disabled={importing !== null}
                  aria-busy={importing === source}
                  data-testid={`tools-import-${source}`}
                  onClick={() => void importFrom(source)}
                >
                  {importing === source
                    ? mw(lang, "Importing…")
                    : count === 1
                      ? mw(lang, "Import 1 tool from {source}", { source: SOURCE_NAME[source] })
                      : mw(lang, "Import {count} tools from {source}", {
                          count,
                          source: SOURCE_NAME[source],
                        })}
                </Button>
              ))}
            </div>
          )}
          {importFailed !== null && (
            <p className="set-note" role="status" data-testid="tools-import-failed">
              {say(importFailed)}
            </p>
          )}
          {importedLines !== null && importedLines.length > 0 && (
            <div className="set-tools-imported" role="status" data-testid="tools-imported">
              {importedLines.map((line, i) => (
                <p
                  key={i}
                  className="set-note"
                  data-testid={i === 0 ? "tools-imported-head" : "tools-skipped"}
                >
                  {say(line)}
                </p>
              ))}
            </div>
          )}
          <Group {...(firm ? { note: mw(lang, "Your firm set these tools.") } : {})}>
            {view.servers.length === 0 && (
              <PlateRow>
                <p className={settingNote} data-testid="tools-empty">
                  {say(mw(lang, "No tools yet."))}
                </p>
              </PlateRow>
            )}
            {view.servers.map((server) => (
              <PlateRow key={server.name} className="set-tools-row" disabled={firm}>
                <span className="set-tools-icon" aria-hidden="true">
                  <IconCommand size={16} />
                </span>
                <div className="min-w-0 flex-1" data-testid="tools-row">
                  <p className={settingLabel} lang="en">
                    <bdi>{server.name}</bdi>
                  </p>
                  <p className={settingNote} lang="en" data-testid="tools-detail">
                    <bdi>{detailOf(server)}</bdi>
                  </p>
                </div>
                {!firm && (
                  <>
                    <Button
                      kind="quiet"
                      size="small"
                      data-tool-edit={server.name}
                      aria-label={`${mt(lang, "Edit")} ${server.name}`}
                      onClick={() => {
                        setFailed(null);
                        setOpen({ editing: server });
                      }}
                    >
                      {mt(lang, "Edit")}
                    </Button>
                    <Button
                      kind="quiet"
                      size="small"
                      aria-label={`${mt(lang, "Remove")} ${server.name}`}
                      onClick={() => setRemoving(server.name)}
                    >
                      {mt(lang, "Remove")}
                    </Button>
                  </>
                )}
                <Switch
                  checked={server.enabled}
                  disabled={firm}
                  label={server.name}
                  onCheckedChange={(next) => void answered(commands.setEnabled(server.name, next))}
                />
              </PlateRow>
            ))}
          </Group>
          {failed !== null && (
            <p className="set-note" role="status" data-testid="tools-failed">
              {say(failed)}
            </p>
          )}
          {!firm && (
            <div>
              <Button
                kind={imports.length > 0 ? "quiet" : "tone"}
                size="small"
                data-testid="tools-add"
                onClick={() => {
                  setFailed(null);
                  setOpen({ editing: null });
                }}
              >
                {mw(lang, "Add a tool")}
              </Button>
            </div>
          )}
        </>
      )}
      <AlertDialog open={removing !== null} onOpenChange={(next) => !next && setRemoving(null)}>
        <AlertDialogPopup
          data-testid="tools-remove-confirm"
          undoable={false}
          title={say(mw(lang, "Remove {name}?", { name: removing ?? "" }))}
          actions={
            <>
              <AlertDialogClose render={<Button kind="quiet" data-testid="tools-remove-cancel" />}>
                {ui(lang, "Cancel")}
              </AlertDialogClose>
              <AlertDialogClose
                render={<Button kind="tone" data-testid="tools-remove-ok" />}
                onClick={() => {
                  const name = removing;
                  setRemoving(null);
                  if (name === null) return;
                  back.current = "";
                  void answered(commands.remove(name));
                }}
              >
                {mt(lang, "Remove")}
              </AlertDialogClose>
            </>
          }
        />
      </AlertDialog>
    </div>
  );
}
