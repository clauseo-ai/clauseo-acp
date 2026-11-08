// Settings, Updates (R23, decision record 0037): "Clauseo 0.4.1", when the last check ended, and where the updater
// stands now in words (checking, downloading, ready for the main window's Restart to update, up to date after a
// check, a failure); Check now, from idle or failed; and how updates install, auto or Tell me first, each choice one
// set_update_mode. A firm's policy lock (`update.mode` in the locked keys, or the host's settings.locked) greys the
// choice with "Managed by firm", as the other locked settings say it. Where someone else manages updates (managed:
// policy, a per-machine install, a read-only location, a package manager) one line says who, with no choice and no
// Check now. Shown only where the transport carries the updater (sections.tsx).
import { useId } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { uw } from "@clauseo/components/lib/words-update.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { settingLabel, settingNote } from "@clauseo/components/ui/recipes-controls.ts";
import { RadioSetting } from "@clauseo/components/ui/settings-row.tsx";
import { stamp } from "../../room/thread/clock.ts";
import { updaterOf, updateStoreOf, useUpdate } from "../../store/update.ts";
import type { SectionContext } from "../sections.tsx";
import { Group, Managed, PlateRow } from "./parts.tsx";
import { checkForUpdate, setUpdateMode } from "./update-commands.ts";
import { MANAGED_WORDS, MODE_KEY, standingWords } from "./updates-words.ts";
import { useNow } from "../../store/limits.ts";

export function Updates({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { lang } = w;
  const store = updateStoreOf(ctx.transport);
  const { view, sending, refused } = useUpdate(store);
  const managedId = useId();
  // times against this render's clock: the host sends a new view at each check, nothing ticks here
  const now = useNow();
  if (view == null) return null;
  const s = view.state;
  const managed = typeof s === "object" ? s.managed?.by : undefined;
  const { name } = updaterOf(s);
  const canCheck = name === "idle" || name === "failed";
  const locked =
    ctx.settings.locked.includes(MODE_KEY) ||
    (refused?.act === "mode" && refused.code === "settings.locked");
  const standing = managed === undefined ? standingWords(lang, view) : null;
  // a refusal other than the lock says its code's words under the row it came from
  const refusal =
    refused === null || (refused.act === "mode" && refused.code === "settings.locked")
      ? null
      : errorWords(lang, refused.code);
  return (
    <Group>
      <PlateRow className="set-choice">
        <div className="set-choice-words">
          <p className={settingLabel} data-testid="update-version">
            <Say>{uw(lang, "Clauseo {version}", { version: view.current })}</Say>
          </p>
          {view.checked_at !== undefined && managed === undefined && (
            <p className={settingNote} data-testid="update-checked">
              <Say>
                {uw(lang, "Last checked {time}", {
                  time: stamp(lang, view.checked_at, now),
                })}
              </Say>
            </p>
          )}
          <p className={settingNote} role="status" data-testid="update-standing">
            {managed !== undefined ? (
              <Say>{uw(lang, MANAGED_WORDS[managed])}</Say>
            ) : (
              standing !== null && <Say>{standing}</Say>
            )}
          </p>
          {refusal !== null && refused?.act === "check" && (
            <p className={settingNote} data-testid="update-refused">
              <Say>{refusal}</Say>
            </p>
          )}
        </div>
        {managed === undefined && (
          <Button
            kind="tone"
            size="small"
            disabled={!canCheck || sending !== null}
            data-testid="update-check"
            onClick={() => void checkForUpdate(ctx.transport, store)}
          >
            <span className="deva-nudge">{uw(lang, "Check now")}</span>
          </Button>
        )}
      </PlateRow>
      {managed === undefined && view.mode !== "none" && (
        <PlateRow disabled={locked}>
          <RadioSetting<"auto" | "notify">
            className="py-0"
            label={uw(lang, "When an update is ready")}
            note=""
            value={view.mode}
            disabled={locked}
            describedBy={locked ? managedId : undefined}
            onValueChange={(mode) => {
              if (!locked && mode !== view.mode) void setUpdateMode(ctx.transport, store, mode);
            }}
            choices={[
              { value: "auto", label: uw(lang, "Install updates when Clauseo next opens") },
              { value: "notify", label: uw(lang, "Tell me first") },
            ]}
          />
          {locked && <Managed id={managedId} className="mt-s3" />}
          {refusal !== null && refused?.act === "mode" && (
            <p className={settingNote} data-testid="update-refused">
              <Say>{refusal}</Say>
            </p>
          )}
        </PlateRow>
      )}
    </Group>
  );
}
