// Settings › Skills (UC-970 to UC-978): the skill packs Claude and ChatGPT may use, as `list_skill_packs` lists them.
// Each row: the pack's name, then where it came from ("Added on this computer" for one the lawyer added, else its source
// address's host), one line when it can't load as it stands (with Turn on again for one changed since it was turned
// on), Remove for one added here (through the same confirm as Tools' Remove), and its switch. Under the list, Add a
// skill (RC A1, library/add-skill.tsx): a GitHub link, a skills.sh name or a zip, previewed and then added with
// `add_skills`, or Choose a folder: the native picker, then `add_skill` at once, then the list again; one line under
// it when the host refuses a folder's command. A firm that locks `skills.enabled` holds the switches. Every command goes through skills-commands.ts.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { mw } from "@clauseo/components/lib/words-mcp-tools.ts";
import { sk } from "@clauseo/components/lib/words-skills.ts";
import { say, ui } from "@clauseo/components/lib/words-ui.ts";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogPopup,
} from "@clauseo/components/ui/alert-dialog.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { Switch } from "@clauseo/components/ui/switch.tsx";
import { useState } from "react";
import type { PackStatus } from "../../transport/protocol.ts";
import type { SectionContext } from "../sections.tsx";
import { Group, PlateRow } from "./parts.tsx";
import { useSkills } from "./skills-commands.ts";
import { AddSkillPanel } from "../../library/add-skill.tsx";
import { hostOf } from "./tools-model.ts";

const nameOf = (pack: PackStatus): string => pack.name ?? pack.id;

export function SkillsSection({ ctx }: { ctx: SectionContext }) {
  const { lang } = useDisplay();
  const { packs, failed, busy, add, remove, turn, read } = useSkills(ctx.transport);
  const [removing, setRemoving] = useState<PackStatus | null>(null);
  if (packs === null) return null;
  const locked = ctx.settings.locked.includes("skills.enabled");
  return (
    <div className="set-tools" data-testid="skills-section">
      <Group>
        {packs.length === 0 && (
          <PlateRow>
            <p className={settingNote} data-testid="skills-empty">
              {say(sk(lang, "No skills added yet."))}
            </p>
          </PlateRow>
        )}
        {packs.map((pack) => {
          const name = nameOf(pack);
          const host = pack.source_url === undefined ? "" : hostOf(pack.source_url);
          const loads = pack.state === "on" || pack.state === "off";
          return (
            <PlateRow key={pack.id} className="set-tools-row">
              <div className="min-w-0 flex-1" data-testid="skills-row">
                <p className={settingLabel}>
                  <bdi>{name}</bdi>
                </p>
                {pack.added_here === true ? (
                  <p className={settingNote} data-testid="skills-source">
                    {say(sk(lang, "Added on this computer"))}
                  </p>
                ) : (
                  host !== "" && (
                    <p className={settingNote} lang="en" data-testid="skills-source">
                      <bdi>{host}</bdi>
                    </p>
                  )
                )}
                {!loads && (
                  <p className={settingNote} data-testid="skills-state">
                    {say(sk(lang, "Clauseo can't use this skill as it is."))}
                  </p>
                )}
              </div>
              {pack.state === "changed" && (
                <Button
                  kind="quiet"
                  size="small"
                  data-testid="skills-turn-on-again"
                  disabled={busy || locked}
                  onClick={() => turn(pack.id, true)}
                >
                  {sk(lang, "Turn on again")}
                </Button>
              )}
              {pack.added_here === true && (
                <Button
                  kind="quiet"
                  size="small"
                  data-testid="skills-remove"
                  aria-label={`${mt(lang, "Remove")} ${name}`}
                  disabled={busy}
                  onClick={() => setRemoving(pack)}
                >
                  {mt(lang, "Remove")}
                </Button>
              )}
              <Switch
                checked={pack.state === "on"}
                disabled={busy || locked || !loads}
                label={name}
                onCheckedChange={(next) => turn(pack.id, next)}
              />
            </PlateRow>
          );
        })}
      </Group>
      <section data-testid="skills-add-block">
        <h3 className={settingLabel}>{sk(lang, "Add a skill")}</h3>
        <AddSkillPanel
          transport={ctx.transport}
          matter={null}
          scope0="all"
          onAdded={read}
          onFolder={add}
        />
      </section>
      {failed !== null && (
        <p className="set-note" role="status" data-testid="skills-failed">
          {say(
            failed === "skill.no_skill_md"
              ? sk(lang, "That folder has no SKILL.md file. Choose the folder that holds one.")
              : sk(lang, "Clauseo couldn't make that change. Your skills are as they were."),
          )}
        </p>
      )}
      <AlertDialog open={removing !== null} onOpenChange={(next) => !next && setRemoving(null)}>
        <AlertDialogPopup
          data-testid="skills-remove-confirm"
          undoable={false}
          title={say(
            mw(lang, "Remove {name}?", { name: removing === null ? "" : nameOf(removing) }),
          )}
          actions={
            <>
              <AlertDialogClose render={<Button kind="quiet" data-testid="skills-remove-cancel" />}>
                {ui(lang, "Cancel")}
              </AlertDialogClose>
              <AlertDialogClose
                render={<Button kind="tone" data-testid="skills-remove-ok" />}
                onClick={() => {
                  const pack = removing;
                  setRemoving(null);
                  if (pack !== null) remove(pack.id);
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
