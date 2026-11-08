// Settings, "Remembered answers" (R18): each matter's kept answers to the native dialogs, grouped by
// matter (its name from the app feed), each row the folder, file or site, isolated, what kind it
// is, and "Allowed" or "Not allowed", with Forget, which sends `forget_allowance`. An empty list says
// how a row gets here; a line under the list says when forgetting a folder takes effect; a refusal
// says its code's words. Read as the section opens, after each Forget, and when the window gains
// focus or becomes visible (allowances-commands.ts). Allowing and refusing are the room's, through the host's dialogs;
// switching the Safe workspace off is the main window's alone, so nothing here offers it.
import { RememberedRow } from "@clauseo/components/clauseo/protection-remembered.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { isolate } from "@clauseo/components/lib/isolate.ts";
import { aw } from "@clauseo/components/lib/words-allowances.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import type { SectionContext } from "../sections.tsx";
import { useMatterNames } from "./allowed-sites-commands.ts";
import { useAllowances } from "./allowances-commands.ts";
import { Group, PlateRow } from "./parts.tsx";

export function Allowances({ ctx }: { ctx: SectionContext }) {
  const { lang } = useDisplay();
  const names = useMatterNames(ctx.transport);
  const { byMatter, failed, forgetting, forget } = useAllowances(ctx.transport);
  if (byMatter === null && failed === null) return null;
  // a matter's name comes from outside Clauseo: sanitised and isolated (isolate.ts)
  const nameOf = (matter: string) => {
    const name = names.get(matter);
    return name === undefined ? aw(lang, "A matter not on your list") : isolate(name);
  };
  const groups = byMatter === null ? [] : [...byMatter.entries()];
  return (
    <div data-testid="allowances" className="contents">
      {byMatter !== null && groups.length === 0 && (
        <Group>
          <PlateRow>
            <p className={settingNote} data-testid="allowances-empty">
              {say(
                aw(
                  lang,
                  "No remembered answers. When you allow or refuse a folder or site for a matter and keep the answer, it shows here.",
                ),
              )}
            </p>
          </PlateRow>
        </Group>
      )}
      {groups.map(([matter, rows]) => (
        <div key={matter} data-testid="allowances-matter" className="contents">
          <Group title={nameOf(matter)}>
            {rows.map((row) => (
              <PlateRow key={row.allowance} className="flex items-center justify-between gap-3">
                <RememberedRow
                  target={row.target}
                  allowed={row.allowed}
                  busy={forgetting !== null}
                  onForget={() => forget(row)}
                />
              </PlateRow>
            ))}
          </Group>
        </div>
      ))}
      {failed !== null && (
        <p className={settingNote} role="status" data-testid="allowances-failed">
          {say(errorWords(lang, failed))}
        </p>
      )}
      {groups.length > 0 && (
        <p className="set-group-note" data-testid="allowances-later">
          {say(
            aw(
              lang,
              "Forgetting a folder takes effect the next time Clauseo starts work in that matter.",
            ),
          )}
        </p>
      )}
    </div>
  );
}
