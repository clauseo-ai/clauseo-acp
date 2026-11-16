// UC's protection wire, scripted for FakeTransport's `protection`: the `protection` query and the two commands. Their
// events are the test's to play (`FakeTransport.protectionChanged`, `protectionViewChanged`, which need `host.next`'s
// app part). Also the scan the fake host says nothing of. Kept apart so fake-transport.ts stays under its size.
import type { CommandEnvelope, FolderRefId, FolderScanned, ProtectionView } from "./protocol.ts";

/** A protection command, by its wire name. */
export type FakeProtectionCommand = "retry_protection" | "remove_sandbox_setup";

export type FakeProtectionHost = {
  /** Answers each `protection` query; throwing refuses it (`command.invalid`, before PR-B admits it). */
  view: () => ProtectionView | Promise<ProtectionView>;
  /** Answers `retry_protection` and `remove_sandbox_setup`; throwing refuses. Done when left out. */
  command?: (command: FakeProtectionCommand) => void | Promise<void>;
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
