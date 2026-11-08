// UC: the firm requires protection (ProtectionView.required) and the open matter has none: the
// composer's send is held (room.tsx's context) and this notice says why in its notice slot, saving
// the host's round trip; U10's error line stays the fallback. Where a set-up with no Windows prompt
// would fix it (needs_set_up, set_up_refused), "Set up the Safe workspace" sends it. Where the
// repair is the installer (design B, blocked.ts's needsInstaller), the notice says so and asks IT
// to install Clauseo again: a firm install's repair, with no button. A full pool says R17 PR-E's
// sandbox.pool_full words; a build without the Safe workspace says it comes in an update
// (not_built); every other cause says sandbox.required's, then a block's own line.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import {
  POOL_FULL_CODE,
  protectionErrorWords,
  pw,
} from "@clauseo/components/lib/words-protection.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { ProtectionStore } from "../../store/protection.ts";
import { useProtection } from "../../store/protection-run.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { Cause } from "./causes.ts";
import { blockLine, needsInstaller, repairLine, setUpFixes } from "./blocked.ts";
import { retryProtection } from "./commands.ts";
import { whyOf } from "./detail.ts";
import { failedWords, useSetUpPress, waitingWords } from "./setup.tsx";

export function RequiredNotice({
  transport,
  store,
  cause,
}: {
  transport: HostTransport;
  store: ProtectionStore;
  cause: Cause;
}) {
  const { lang } = useDisplay();
  const state = useProtection(store);
  const { press, confirm } = useSetUpPress(
    state,
    cause,
    () => void retryProtection(transport, store),
  );
  const repair = needsInstaller(cause);
  const fixable =
    !repair &&
    (cause.name === "needs_set_up" || cause.name === "set_up_refused" || setUpFixes(cause));
  const notBuilt = whyOf(cause) === "not_built";
  const block = repair ? null : blockLine(lang, cause);
  const waiting = fixable ? waitingWords(lang, state, cause) : null;
  const failed = fixable ? failedWords(lang, state) : null;
  return (
    <div
      className="room-required"
      role="status"
      data-testid="protection-required"
      data-cause={cause.name}
    >
      <p>
        <Say>
          {repair
            ? pw(
                lang,
                "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.",
              )
            : fixable
              ? pw(
                  lang,
                  "Your firm requires the Safe workspace on this computer. Set it up once, then ask here.",
                )
              : cause.group === "pool"
                ? protectionErrorWords(lang, POOL_FULL_CODE)
                : notBuilt
                  ? pw(
                      lang,
                      "Your firm requires the Safe workspace, and this version of Clauseo doesn't have it yet, so Clauseo can't answer here. The Safe workspace comes in an update.",
                    )
                  : errorWords(lang, "sandbox.required")}
        </Say>
      </p>
      {(repair || block !== null) && (
        <p data-testid={repair ? "protection-repair" : "protection-block"}>
          <Say>{(repair ? repairLine(lang, state.view, state.managedBy) : block) ?? ""}</Say>
        </p>
      )}
      {fixable &&
        (waiting !== null ? (
          <p className="room-required-wait" data-testid="protection-wait">
            <Say>{waiting}</Say>
          </p>
        ) : (
          <Button kind="tone" size="small" onClick={press} data-testid="protection-setup">
            <span className="deva-nudge">{pw(lang, "Set up the Safe workspace")}</span>
          </Button>
        ))}
      {failed !== null && (
        <p data-testid="protection-failed">
          <Say>{failed}</Say>
        </p>
      )}
      {confirm}
    </div>
  );
}
