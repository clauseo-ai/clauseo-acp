// The Safe workspace (UC, settings.html#workspace): this computer's protection, as the design's status block says it
// (design/03-system/src/pages/settings/sections-b.tsx), then the firm's lines. On for every matter, with what macOS or
// Windows does (and "Remove set-up" where Windows' permission set it up, always through its confirm); or Not
// protected with why, and "Set up protection" or "Set up again" where the set-up fixes it, through the "Unknown
// publisher" confirm where Windows' prompt follows. The waiting line stands in the button's place while a command
// runs, and a refusal's words under the block. Where the host doesn't answer the query yet (`command.invalid`, before
// R17 PR-B admits this window) the section says so quietly. A matter's own state (a full pool, protection switched
// off) is the room's, never shown here. No timers: the view is read when the window opens, after each command and on
// each protection_changed.
import { ProtectionConfirm } from "@clauseo/components/clauseo/protection-confirm.tsx";
import { IconSafeWorkspace } from "@clauseo/components/icons/icons.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { pw } from "@clauseo/components/lib/words-protection.ts";
import { useSettingsWords, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useId, useState } from "react";
import { protectionStoreOf } from "../../store/protection.ts";
import { useProtection } from "../../store/protection-run.ts";
import type { ProtectionView } from "../../transport/protocol.ts";
import type { Cause } from "../../room/protection/causes.ts";
import { failedWords, useSetUpPress, waitingWords } from "../../room/protection/setup.tsx";
import { computerCause, setUpAt } from "../../room/protection/view.ts";
import { named1385, whyOf } from "../../room/protection/detail.ts";
import type { SectionContext } from "../sections.tsx";
import { Managed } from "./parts.tsx";
import { removeSetUp, setUpAgain } from "./workspace-commands.ts";

/** The block's state, as data-state says it. */
type Status =
  | "a-mac"
  | "a-win"
  | "b"
  | "needs_set_up"
  | "set_up_refused"
  | "set_up_failed"
  | "cant";

const statusOf = (view: ProtectionView, cause: Cause | null, platform: string): Status => {
  if (cause === null) return view.tier === "b" ? "b" : platform === "windows" ? "a-win" : "a-mac";
  if (cause.group === "setup") return cause.name as Status;
  return "cant";
};

/** "3 November 2026", Latin digits in both languages. */
const dayOf = (at: number, lang: Lang): string =>
  new Intl.DateTimeFormat(lang === "hi" ? "hi-IN" : "en-IN", {
    day: "numeric",
    month: "long",
    year: "numeric",
    numberingSystem: "latn",
  }).format(at);

/** Why a computer can't give protection, by R17 PR-B's `unsupported { why }`; an older host's plain cause, or a newer
 * one, takes the generic line. */
function cantKey(cause: Cause): SettingsKey {
  const why = whyOf(cause);
  if (why === "os")
    return "Clauseo's protection isn't available on Linux yet, so Clauseo asks before changing anything outside a matter's folder.";
  if (why === "windows_arm")
    return "Clauseo's protection isn't available on Windows on ARM yet, so Clauseo asks before changing anything outside a matter's folder.";
  if (why === "not_built")
    return "This build of Clauseo comes without protection, so Clauseo asks before changing anything outside a matter's folder.";
  return "This computer can't give Clauseo's protection, so Clauseo asks before changing anything outside a matter's folder.";
}

export function Workspace({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { lang, platform } = useDisplay();
  const [store] = useState(() => protectionStoreOf(ctx.transport));
  const state = useProtection(store);
  const view = state.view;
  const cause = computerCause(view);
  const managed = useId();
  const [removing, setRemoving] = useState(false);
  const { press, confirm } = useSetUpPress(
    state,
    cause,
    () => void setUpAgain(ctx.transport, store),
  );
  // not read yet: nothing, so the block never flashes a state it doesn't have
  if (view === undefined) return null;
  if (view === null)
    return (
      <p
        className="set-note"
        role="status"
        data-testid="settings-workspace"
        data-state="unavailable"
      >
        {w.say(w.s("Clauseo can't show this computer's protection here yet."))}
      </p>
    );
  const status = statusOf(view, cause, platform);
  const off = cause !== null;
  const at = setUpAt(view);
  const note: string =
    status === "a-mac"
      ? w.s("macOS keeps Clauseo inside each matter's folder.")
      : status === "a-win"
        ? w.s("Windows keeps Clauseo inside each matter's folder.")
        : status === "b"
          ? at === null
            ? w.s("Set up with Windows' permission.")
            : w.s("Set up on {date}, with Windows' permission.", { date: dayOf(at, lang) })
          : status === "needs_set_up"
            ? w.s(
                "Protection isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.",
              )
            : status === "set_up_refused"
              ? w.s(
                  "Windows didn't allow the set-up, so Clauseo asks before changing anything outside a matter's folder.",
                )
              : status === "set_up_failed"
                ? w.s(
                    "Protection couldn't be set up on this computer, so Clauseo asks before changing anything outside a matter's folder.",
                  )
                : w.s(cantKey(cause ?? { name: "unknown", group: "cant", raw: null }));
  const waiting = waitingWords(lang, state, cause);
  const failed = failedWords(lang, state);
  const act =
    status === "b" ? (
      <Button
        kind="quiet"
        size="small"
        onClick={() => setRemoving(true)}
        data-testid="protection-remove"
      >
        {w.say(pw(lang, "Remove set-up"))}
      </Button>
    ) : status === "needs_set_up" || status === "set_up_refused" || status === "set_up_failed" ? (
      <Button kind="tone" size="small" onClick={press} data-testid="protection-setup">
        {w.say(status === "needs_set_up" ? pw(lang, "Set up protection") : w.s("Set up again"))}
      </Button>
    ) : null;
  return (
    // id "protection": R17 PR-B1's SettingsSection::Protection opens settings.html#protection
    <div id="protection" className="set-ws" data-testid="settings-workspace" data-state={status}>
      <div className="set-status" data-off={off ? "" : undefined}>
        <span className="set-status-icon" aria-hidden="true">
          <IconSafeWorkspace size={18} />
        </span>
        <div className="set-status-words">
          <p className="set-status-title">
            {w.say(off ? w.s("Not protected") : w.s("On for every matter"))}
          </p>
          <p className="set-note">{w.say(note)}</p>
          {cause !== null && named1385(cause) && (
            <p className="set-note">
              {w.say(
                pw(
                  lang,
                  "Windows policy on this computer doesn't let Clauseo's protection accounts sign in. Ask your IT team to add them to the Allow log on locally policy.",
                ),
              )}
            </p>
          )}
          {view.required && <Managed id={managed} />}
        </div>
        {act !== null &&
          (waiting !== null ? (
            <p className="set-ws-wait" role="status" data-testid="protection-wait">
              {w.say(waiting)}
            </p>
          ) : (
            act
          ))}
      </div>
      {failed !== null && (
        <p className="set-ws-failed" role="status" data-testid="protection-failed">
          {w.say(failed)}
        </p>
      )}
      {off && view.required && (
        <p className="set-note" data-testid="protection-required">
          {w.say(
            w.s(
              "Your firm requires protection, so Clauseo doesn't answer on this computer until it's protected.",
            ),
          )}
        </p>
      )}
      {off && !view.unprotected_auto_allowed && (
        <p className="set-note" data-testid="protection-no-auto">
          {w.say(w.s("Your firm doesn't allow Clauseo to work without asking on this computer."))}
        </p>
      )}
      {confirm}
      <ProtectionConfirm
        act="remove"
        open={removing}
        onOpenChange={setRemoving}
        onConfirm={() => {
          setRemoving(false);
          void removeSetUp(ctx.transport, store);
        }}
      />
    </div>
  );
}
