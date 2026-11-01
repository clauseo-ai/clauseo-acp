// The windows' elements, in one place: every spec finds them through here, so a renamed test id
// changes this file only. The test ids are the ones ui/app/src/internal (R12's window) and
// ui/app/src/room (the room, `E2E_WINDOW=room`) set; every transcript row also carries
// `data-entry-id`. The two windows share `matter-name`, `new-matter` and `matter`.

import type { Locator, Page } from "@playwright/test";

/** The `data-testid` of each element the specs use. */
export const ids = {
  /**
   * The new matter's name. Internal window: prefilled "Matter", New matter is disabled while it
   * is empty. Room: the New matter form's name field.
   */
  matterName: "matter-name",
  /**
   * Internal window: pick_folder (E2E_MATTER_DIR in the e2e build), then CreateMatter and a
   * thread. Room: the rail's New matter, which opens the form.
   */
  newMatter: "new-matter",
  /** Room, in the form: Change, pick_folder for an existing folder (`existing_folder`). */
  matterExisting: "matter-existing",
  /**
   * Room, in the form: Create the matter. With a folder picked by Change, that folder; without,
   * pick_folder for the parent and the host makes `<parent>/<name>` (`new_folder`).
   */
  createMatter: "create-matter",
  /**
   * Room: a matter's row button in the rail, with `data-state` (the MatterRow state) and
   * `aria-current="page"` on the open matter. The scratch matter's foot row is not one.
   */
  matterRow: "matter-row",
  /** Room: the rail's find field (shown once there is a matter to find). */
  find: "find",
  /** Room: Matters on the toolbar, which opens and folds the rail below 1200 px. */
  railToggle: "rail-toggle",
  /** The open matter's name; in the internal window also "Opening…" or "No matter". */
  matter: "matter",
  /** The open matter's threads. */
  threadSelect: "thread-select",
  /** Button: a new thread in the open matter. */
  newThread: "new-thread",
  /** The window's error line, with `data-code` (the HostError code). */
  error: "error",
  /** The prompt `<textarea>`. */
  composer: "composer",
  /** Sends the prompt; disabled while a turn runs. */
  send: "send",
  /** Shown while a turn runs. */
  cancel: "cancel",
  transcript: "transcript",
  userMessage: "message-user",
  agentMessage: "message-agent",
  thought: "thought",
  /** A tool call row, with `data-tool-call-id` and `data-status`. */
  toolCall: "tool-call",
  /**
   * A permission request, with `data-state` (pending, resolved, expired) and, while it can be
   * answered, one button per offered option kind. An answered card stays, without buttons.
   */
  permission: "permission",
  /** The line a finished turn ends with, with `data-stop-reason` (the wire StopReason). */
  turnStopped: "turn-stopped",
  /** Inside turnStopped: the files the turn changed, one fileChanged each. */
  filesChanged: "files-changed",
  fileChanged: "file-changed",
  notice: "notice",
  /** A turn stopped at a usage limit; holds `resetsAt`. */
  limitStop: "limit-stop",
  /** `<time dateTime="<ISO, UTC>">` inside limitStop. */
  limitResetsAt: "limit-resets-at",
  /** A finished turn's Undo. */
  undo: "undo",
} as const;

/** The ACP option kinds a permission button is labelled by. */
export type OptionKind = "allow_once" | "allow_always" | "reject_once" | "reject_always";

/** The `data-testid` of the permission button for `kind`. */
export const permissionButton = (kind: OptionKind): string => `permission-${kind}`;

/** The `data-stop-reason` values the specs expect: protocol StopReason wire values. */
export type StopReason = "end_turn" | "cancelled" | "at_limit";

/** The window's elements on `page`. */
export function window(page: Page) {
  const id = (name: string): Locator => page.getByTestId(name);
  return {
    matterName: id(ids.matterName),
    newMatter: id(ids.newMatter),
    matterExisting: id(ids.matterExisting),
    createMatter: id(ids.createMatter),
    matterRows: id(ids.matterRow),
    /** Room: the rail row of the matter named `name` (its text also holds its status). */
    matterRow: (name: string): Locator => id(ids.matterRow).filter({ hasText: name }),
    /** Room: the rail's find field. */
    findMatter: id(ids.find),
    /**
     * Room: Matters on the toolbar, which unfolds the rail below 1200 px (the e2e window opens at
     * Tauri's default 800 x 600, so the rail starts folded).
     */
    railToggle: id(ids.railToggle),
    matter: id(ids.matter),
    threadSelect: id(ids.threadSelect),
    newThread: id(ids.newThread),
    error: id(ids.error),
    composer: id(ids.composer),
    send: id(ids.send),
    cancel: id(ids.cancel),
    transcript: id(ids.transcript),
    userMessages: id(ids.userMessage),
    agentMessages: id(ids.agentMessage),
    thoughts: id(ids.thought),
    toolCalls: id(ids.toolCall),
    toolCall: (toolCallId: string): Locator =>
      page.locator(`[data-testid="${ids.toolCall}"][data-tool-call-id="${toolCallId}"]`),
    /** The tool call rows whose text contains `title`. */
    toolCallTitled: (title: string): Locator => id(ids.toolCall).filter({ hasText: title }),
    permissions: id(ids.permission),
    permissionButton: (card: Locator, kind: OptionKind): Locator =>
      card.getByTestId(permissionButton(kind)),
    turnStopped: id(ids.turnStopped),
    filesChanged: id(ids.filesChanged),
    fileChanged: id(ids.fileChanged),
    notices: id(ids.notice),
    limitStop: id(ids.limitStop),
    limitResetsAt: id(ids.limitResetsAt),
    undo: id(ids.undo),
  };
}

export type Window = ReturnType<typeof window>;
