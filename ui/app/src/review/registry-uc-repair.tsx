// UC's copy review entries for design B's in-app repair ("Download and run the installer again",
// room/protection/repair.ts): the room's Not protected note over a missing admin copy on a personal
// install where the repair runs, its two waiting lines after the press, and the line after a start
// that follows an installer which didn't set the Safe workspace up (the note opens by itself). Kept
// apart so registry-uc.tsx stays under its size. Provisional, with provisional Hindi.
import type { ProtectionKey } from "@clauseo/components/lib/words-protection.ts";
import type { FakeRepairHost } from "../transport/fake-protection.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  MatterSummary,
  Protection,
  ProtectionView,
  RepairStep,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { PressInOrder } from "./press-words.tsx";
import { RoomOver } from "./states.tsx";
import { protectionWords } from "./words.ts";

const THREAD = "00000000-0000-4000-8000-0000000000c1";
const MISSING = {
  not_protected: { cause: { blocked: { reason: "admin_copy_missing" } } },
} as Protection;
const LINE = '[data-testid="workspace-line"]';
const AGAIN = '[data-testid="protection-repair-again"]';

const viewOf = (computer: Protection): ProtectionView => ({
  tier: "c",
  computer,
  required: false,
  unprotected_auto_allowed: true,
});

/** The room over a matter whose computer misses the admin copy, with `repair` scripted; every note
 * explained, so only a press or the repair's return opens one. */
const repairHost = (repair: FakeRepairHost) => () => {
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER.matter] }),
  );
  const matter: MatterSummary = { ...MATTER, protection: MISSING };
  return new FakeTransport({
    platform: "windows",
    query: (query) => {
      if (query === "list_matters") return { matters: [matter] };
      if (typeof query === "object" && "list_threads" in query)
        return {
          threads: [{ thread: THREAD, matter: MATTER.matter, agent: "claude-acp", status: "idle" }],
        };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    protection: { view: () => viewOf(MISSING), repair },
  });
};

const entry = (
  id: string,
  key: ProtectionKey,
  repair: FakeRepairHost,
  press: string[] = [],
): Entry => ({
  id,
  words: protectionWords(key),
  screen: "room/np-note",
  render: () =>
    press.length === 0 ? (
      <RoomOver host={repairHost(repair)} />
    ) : (
      <PressInOrder press={press}>
        <RoomOver host={repairHost(repair)} />
      </PressInOrder>
    ),
});

/** A repair that says `steps`, then never answers, as the desktop does once its window is gone. */
const holding =
  (...steps: RepairStep[]): NonNullable<FakeRepairHost["run"]> =>
  (onStep) => {
    for (const step of steps) onStep(step);
    return new Promise<void>(() => {});
  };

export const UC_REPAIR: Entry[] = [
  entry("UC-97", "Download and run the installer again", { available: true }, [LINE]),
  entry("UC-98", "Downloading the installer…", { available: true, run: holding("downloading") }, [
    LINE,
    AGAIN,
  ]),
  entry(
    "UC-99",
    "Windows will ask for permission. Choose Yes.",
    { available: true, run: holding("downloading", "launching") },
    [LINE, AGAIN],
  ),
  entry(
    "UC-100",
    "The installer didn't finish setting up the Safe workspace. Try again, or choose Get the installer.",
    { available: true, note: "returned" },
  ),
];
