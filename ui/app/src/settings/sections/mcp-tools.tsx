// Settings › Tools (UC-169 to UC-189): the lawyer's own MCP servers, called tools. The section's lead says plainly what
// a tool can reach; under it, while SAFE_WORKSPACE_NOTE holds, that the Safe workspace doesn't use them. Then the list:
// each tool's name, a quiet detail (its program's file name, or its address's host), its switch, Edit and Remove
// (through a confirm); or, with none, one line and Add a tool. The form (mcp-tools-form.tsx) stands in the list's
// place while open, and on Save or Cancel focus goes back to the row or the button that opened it. A firm's policy list
// is read-only with the lock's treatment (its rows greyed, the switches off limits) and says the firm set it; the
// firm's own MCP shows one line and no list. The view is read as the section opens, after each command and when the
// window regains focus; every command goes through tools-model.ts's ToolsCommands.
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
import { ToolForm } from "./mcp-tools-form.tsx";
import { refusalOf } from "./mcp-tools-refusal.ts";
import { Group, PlateRow } from "./parts.tsx";
import {
  fileName,
  hostOf,
  remoteOffered,
  SAFE_WORKSPACE_NOTE,
  toolsCommandsOf,
  type McpServerView,
  type McpToolsView,
  type ToolsAnswer,
  type ToolsCommands,
} from "./tools-model.ts";

/** The form's state: closed, adding, or editing a tool. */
type Open = null | { editing: McpServerView | null };

/** A server's quiet detail: its program's file name, or its address's host. */
const detailOf = (server: McpServerView): string =>
  server.kind === "remote" ? hostOf(server.url ?? "") : fileName(server.command ?? "");

/** The view over the commands: null before the first answer; an answer stands only while no later read was asked. */
function viewOver(commands: ToolsCommands) {
  let view: McpToolsView | null = null;
  let asked = 0;
  const listeners = new Set<() => void>();
  const read = async (): Promise<void> => {
    const n = (asked += 1);
    try {
      const next = await commands.read();
      if (n !== asked) return;
      view = next;
      for (const listener of listeners) listener();
    } catch {
      // the view stays as last read
    }
  };
  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    view: () => view,
    read,
  };
}

/** The view, read now, after each command and whenever the window regains focus. */
function useToolsView(commands: ToolsCommands) {
  const [over] = useState(() => viewOver(commands));
  const view = useSyncExternalStore(over.subscribe, over.view, over.view);
  useEffect(() => {
    void over.read();
    const focused = () => void over.read();
    window.addEventListener("focus", focused);
    return () => window.removeEventListener("focus", focused);
  }, [over]);
  return { view, read: over.read };
}

export function McpTools({ ctx }: { ctx: SectionContext }) {
  const commands = toolsCommandsOf(ctx.transport);
  if (commands === null) return null;
  return <Tools commands={commands} />;
}

function Tools({ commands }: { commands: ToolsCommands }) {
  const { lang, platform } = useDisplay();
  const { view, read } = useToolsView(commands);
  const [open, setOpen] = useState<Open>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
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
    const refusal = await answer.catch(() => "secretStoreFailed" as const);
    setFailed(refusal === null ? null : refusalOf(lang, platform, refusal).words);
    await read();
  };

  if (view === null) return null;
  if (view.control === "firmManaged")
    return (
      <p className="set-note" data-testid="tools-section" data-control="firmManaged">
        {say(mw(lang, "Your firm manages tools on this computer."))}
      </p>
    );
  const firm = view.control === "firmList";
  const close = (name: string | null) => {
    back.current = name ?? "";
    setOpen(null);
  };
  return (
    <div ref={root} className="set-tools" data-testid="tools-section" data-control={view.control}>
      {SAFE_WORKSPACE_NOTE && (
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
              save={async (previousName, draft) => {
                const answer = await commands.save(previousName, draft);
                if (answer === null) await read();
                return answer;
              }}
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
                kind="tone"
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
