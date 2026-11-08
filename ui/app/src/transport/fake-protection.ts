// UC's protection wire, scripted for FakeTransport's `protection`: the `protection` query and the two commands. Their
// events are the test's to play (`FakeTransport.protectionChanged`, `protectionViewChanged`, which need `host.next`'s
// app part). Also the scan the fake host says nothing of. Kept apart so fake-transport.ts stays under its size.
import type {
  CommandEnvelope,
  FolderRefId,
  FolderScanned,
  ProtectionView,
  RepairNote,
  RepairStep,
} from "./protocol.ts";

/** A protection command, by its wire name. */
export type FakeProtectionCommand = "retry_protection" | "remove_sandbox_setup";

export type FakeProtectionHost = {
  /** Answers each `protection` query; throwing refuses it (`command.invalid`, before PR-B admits it). */
  view: () => ProtectionView | Promise<ProtectionView>;
  /** Answers `retry_protection` and `remove_sandbox_setup`; throwing refuses. Done when left out. */
  command?: (command: FakeProtectionCommand) => void | Promise<void>;
  /** Design B's in-app repair: display_info's word on it, and repair_install. None when left out. */
  repair?: FakeRepairHost;
};

/** The protection command an envelope carries, by its wire name; null for any other. */
export const protectionCommandOf = (envelope: CommandEnvelope): FakeProtectionCommand | null =>
  envelope.command.retry_protection !== undefined
    ? "retry_protection"
    : envelope.command.remove_sandbox_setup !== undefined
      ? "remove_sandbox_setup"
      : null;

/** A scan the fake host says nothing of: cancelled, nothing counted. */
export const quietScan = (folder: FolderRefId): FolderScanned => ({
  folder,
  files: 0,
  bytes: 0,
  place: "local",
  cloud_only: 0,
  done: true,
  cancelled: true,
});

/** Design B's in-app repair, scripted: display_info's word on it, and each repair_install. */
export type FakeRepairHost = {
  /** display_info's repair_available; false when left out. */
  available?: boolean;
  /** display_info's repair; null when left out. */
  note?: RepairNote | null;
  /** Answers each repair_install, saying its steps through `onStep`; throwing refuses. Left out, it
   * says both steps and never answers, as the desktop does once its window is gone. */
  run?: (onStep: (step: RepairStep) => void) => void | Promise<void>;
};

/** display_info's repair fields for a scripted repair. */
export const repairInfoOf = (repair: FakeRepairHost | undefined) => ({
  repair_available: repair?.available ?? false,
  repair: repair?.note ?? null,
});

/** One repair_install, as the script answers it. */
export async function runFakeRepair(
  repair: FakeRepairHost | undefined,
  onStep: (step: RepairStep) => void,
): Promise<void> {
  if (repair?.run !== undefined) return repair.run(onStep);
  onStep("downloading");
  onStep("launching");
  return new Promise<void>(() => {});
}
