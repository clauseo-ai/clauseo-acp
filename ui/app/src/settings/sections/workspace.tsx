// The Safe workspace (UC, settings.html#workspace): this computer's protection, as the design's
// status block says it (design/03-system/src/pages/settings/sections-b.tsx), then the firm's lines.
// On for every matter, with what macOS or Windows does (and "Remove set-up" where Windows'
// permission set it up, always through its confirm); or Not protected with why, and "Set up the
// Safe workspace" or "Set up again" where a set-up with no Windows prompt fixes it. Design B: a
// block says its reason's line (blocked.ts); where the repair is the installer, its line stands in
// the button's place, with "Get the installer" without a firm's policy (installer.ts). The waiting
// line stands in the button's place while a command runs, and a refusal's words under the block.
// While the firm's policy is active (firmPolicy, blocked.ts), a block only the firm can act on, or a
// sandbox.blocked refusal, adds "Copy details for IT" (it-details.ts) under them.
// Where the host doesn't answer the query yet (`command.invalid`, before R17 PR-B admits this
// window) the section says so quietly. A matter's own state (a full pool, protection switched off)
// is the room's, never shown here. No timers: the view is read when the window opens, after each
// command and on each protection_changed.
import { CopyForIt } from "@clauseo/components/clauseo/copy-for-it.tsx";
import { ProtectionConfirm } from "@clauseo/components/clauseo/protection-confirm.tsx";
import { IconSafeWorkspace } from "@clauseo/components/icons/icons.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { INSTALLER_CLOSE, pw } from "@clauseo/components/lib/words-protection.ts";
import { useSettingsWords, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useEffect, useId, useRef, useState } from "react";
import { protectionStoreOf } from "../../store/protection.ts";
import { useProtection } from "../../store/protection-run.ts";
import type { ProtectionView } from "../../transport/protocol.ts";
import type { Cause } from "../../room/protection/causes.ts";
import { failedWords, useSetUpPress, waitingWords } from "../../room/protection/setup.tsx";
import { computerCause, setUpAt } from "../../room/protection/view.ts";
import { named1385, whyOf } from "../../room/protection/detail.ts";
import {
  blockLine,
  blockOf,
  firmPolicy,
  needsInstaller,
  repairLine,
  setUpFixes,
} from "../../room/protection/blocked.ts";
import { openInstallerPage, repairPage } from "../../room/protection/installer.ts";
import { itDetails } from "../../room/protection/it-details.ts";
import type { SectionContext } from "../sections.tsx";
import { Managed } from "./parts.tsx";
import { removeSetUp, setUpAgain } from "./workspace-commands.ts";

/** Whether the host enforces the firm's sandbox.allow_unprotected_auto = false. It doesn't yet: the key only fills
 * ProtectionView.unprotected_auto_allowed, so UC-76's line would promise what nothing does. Set to true with the host
 * follow-up that keeps an unprotected agent in its ask modes under that policy. */
const FIRM_NO_AUTO_ENFORCED: boolean = false;

/** The block's state, as data-state says it. */
type Status =
  | "a-mac"
  | "a-win"
  | "b"
  | "needs_set_up"
  | "set_up_refused"
  | "set_up_failed"
  | "blocked"
  | "cant";

const statusOf = (view: ProtectionView, cause: Cause | null, platform: string): Status => {
  if (cause === null) return view.tier === "b" ? "b" : platform === "windows" ? "a-win" : "a-mac";
  if (blockOf(cause) !== null) return "blocked";
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
    return "The Safe workspace isn't available on Linux yet, so Clauseo asks before changing anything outside a matter's folder.";
  if (why === "windows_arm")
    return "The Safe workspace isn't available on Windows on ARM yet, so Clauseo asks before changing anything outside a matter's folder.";
  if (why === "not_built")
    return "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo asks before changing anything outside a matter's folder.";
  return "This computer can't run the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.";
}

export function Workspace({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const display = useDisplay();
  const { lang, platform } = display;
  const [store] = useState(() => protectionStoreOf(ctx.transport));
  const state = useProtection(store);
  const view = state.view;
  const cause = computerCause(view);
  const managed = useId();
  const [removing, setRemoving] = useState(false);
  // a press whose command puts the waiting line in its button's place: focus goes to that line (the window's frame
  // takes a focus lost any other way to the section's head)
  const moving = useRef(false);
  const waitLine = useRef<HTMLParagraphElement>(null);
  const { press, confirm } = useSetUpPress(state, cause, () => {
    moving.current = true;
    void setUpAgain(ctx.transport, store);
  });
  const waiting = view == null ? null : waitingWords(lang, state, cause);
  useEffect(() => {
    if (moving.current) waitLine.current?.focus();
    moving.current = false;
  }, [waiting]);
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
        {w.say(w.s("Clauseo can't show this computer's Safe workspace here yet."))}
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
          : status === "needs_set_up" || blockOf(cause) === "admin_copy_missing"
            ? w.s(
                "The Safe workspace isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.",
              )
            : status === "set_up_refused"
              ? w.s(
                  "Windows didn't allow the set-up, so Clauseo asks before changing anything outside a matter's folder.",
                )
              : status === "set_up_failed"
                ? w.s(
                    "The Safe workspace couldn't be set up on this computer, so Clauseo asks before changing anything outside a matter's folder.",
                  )
                : status === "blocked"
                  ? pw(
                      lang,
                      "Something on this computer stops the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.",
                    )
                  : w.s(cantKey(cause ?? { name: "unknown", group: "cant", raw: null }));
  const repair = needsInstaller(cause);
  const policy = firmPolicy(view, state.managedBy);
  const page = repair && !policy ? repairPage(display) : null;
  const block = blockLine(lang, cause);
  const failed = failedWords(lang, state);
  const forIt = policy
    ? (itDetails.blocked(lang, cause) ?? itDetails.refused(lang, state.failed, cause))
    : null;
  const act = repair ? (
    page === null ? null : (
      <Button
        kind="tone"
        size="small"
        onClick={() => void openInstallerPage(ctx.transport, page)}
        data-testid="protection-installer"
      >
        {w.say(pw(lang, "Get the installer"))}
      </Button>
    )
  ) : status === "b" ? (
    <Button
      kind="quiet"
      size="small"
      onClick={() => setRemoving(true)}
      data-testid="protection-remove"
    >
      {w.say(pw(lang, "Remove set-up"))}
    </Button>
  ) : status === "needs_set_up" ||
    status === "set_up_refused" ||
    status === "set_up_failed" ||
    setUpFixes(cause) ? (
    <Button kind="tone" size="small" onClick={press} data-testid="protection-setup">
      {w.say(
        status === "needs_set_up" || status === "blocked"
          ? pw(lang, "Set up the Safe workspace")
          : w.s("Set up again"),
      )}
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
          {block !== null && (
            <p className="set-note" data-testid="protection-block">
              {w.say(block)}
            </p>
          )}
          {repair && (
            <p className="set-note" data-testid="protection-repair">
              {w.say(repairLine(lang, view, state.managedBy))}
            </p>
          )}
          {page !== null && (
            <p className="set-note" data-testid="protection-close-line">
              {w.say(pw(lang, INSTALLER_CLOSE))}
            </p>
          )}
          {cause !== null && named1385(cause) && (
            <p className="set-note">
              {w.say(
                pw(
                  lang,
                  "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this.",
                ),
              )}
            </p>
          )}
          {view.required && <Managed id={managed} />}
        </div>
        {act !== null &&
          (waiting !== null ? (
            <p
              className="set-ws-wait"
              role="status"
              tabIndex={-1}
              ref={waitLine}
              data-testid="protection-wait"
            >
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
      {forIt !== null && (
        <div className="mt-s2">
          <CopyForIt lang={lang} text={forIt} testId="protection-it" />
        </div>
      )}
      {off && view.required && (
        <p className="set-note" data-testid="protection-required">
          {w.say(
            w.s(
              "Your firm requires the Safe workspace, so Clauseo doesn't answer on this computer until it's set up.",
            ),
          )}
        </p>
      )}
      {FIRM_NO_AUTO_ENFORCED && off && !view.unprotected_auto_allowed && (
        <p className="set-note" data-testid="protection-no-auto">
          {w.say(w.s("Your firm doesn't allow Clauseo to work without asking on this computer."))}
        </p>
      )}
      {confirm}
      <ProtectionConfirm
        act="remove"
        open={removing}
        required={view.required}
        onOpenChange={setRemoving}
        onConfirm={() => {
          setRemoving(false);
          moving.current = true;
          void removeSetUp(ctx.transport, store);
        }}
      />
    </div>
  );
}
