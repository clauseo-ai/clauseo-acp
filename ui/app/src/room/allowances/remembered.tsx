// R18, in the room's protection note: the open matter's remembered answers ("Remembered for this
// matter"), read from its header on the matter's stream, each with Forget; and "Switch it back on"
// for a matter whose Safe workspace is off. The stream is held only while the note is open. A
// refusal's code comes back as the note's failed line.
import { RememberedRow } from "@clauseo/components/clauseo/protection-remembered.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useState, type ReactNode } from "react";
import { useHeader, useWatch } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterAllowance, MatterId } from "../../transport/protocol.ts";
import { matterStreamOf } from "../matter-stream.ts";
import { forget, switchOn } from "./commands.ts";

const NONE: readonly MatterAllowance[] = [];

export type Remembered = {
  /** The rows, one list item each; undefined while the matter has none (or the note is shut). */
  rows: ReactNode | undefined;
  /** Sends `switch_protection_on` for the matter. */
  switchOn: () => void;
  /** The last refused Forget or switch's words; null after one that went through. */
  failed: string | null;
};

export function useRemembered(
  transport: HostTransport,
  views: ViewStore,
  matter: MatterId,
  open: boolean,
  lang: Lang,
): Remembered {
  const stream = open ? matterStreamOf(matter) : null;
  useWatch(views, stream);
  const header = useHeader(views, stream);
  const kept =
    header !== null && "matter_header" in header
      ? (header.matter_header?.allowances ?? NONE)
      : NONE;
  const [busy, setBusy] = useState(false);
  const [gone, setGone] = useState<ReadonlySet<string>>(() => new Set());
  const [failed, setFailed] = useState<string | null>(null);
  const run = async (act: () => Promise<string | null>, row?: MatterAllowance) => {
    setBusy(true);
    const code = await act();
    setBusy(false);
    setFailed(code);
    // taken off at once once the host took it: the header's next frame says the same
    if (code === null && row !== undefined) setGone((was) => new Set(was).add(row.allowance));
  };
  const shown = kept.filter((row) => !gone.has(row.allowance));
  return {
    rows:
      shown.length === 0
        ? undefined
        : shown.map((row) => (
            <span key={row.allowance} role="listitem">
              <RememberedRow
                target={row.target}
                allowed={row.allowed}
                busy={busy}
                onForget={() => void run(() => forget(transport, matter, row.allowance), row)}
              />
            </span>
          )),
    switchOn: () => void run(() => switchOn(transport, matter)),
    failed: failed === null ? null : errorWords(lang, failed),
  };
}
