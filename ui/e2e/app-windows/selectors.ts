// The room's elements, in one place: every spec and the speed scoreboard find them through here,
// so a renamed test id changes this file only. The test ids are the ones the room
// (ui/app/src/room) sets, some through a part's optional `testIds` prop; every transcript row also
// carries `data-entry-id`. ui/app/src/perf/autoplay.ts takes the same steps on macOS.

import type { Locator, Page } from "@playwright/test";

/** The `data-testid` of each element the specs use. */
export const ids = {
  /** The New matter form's name field. */
  matterName: "matter-name",
  /** The rail's New matter, which opens the form. */
  newMatter: "new-matter",
  /** In the form: Change, pick_folder for an existing folder (`existing_folder`). */
  matterExisting: "matter-existing",
  /**
   * In the form: Create the matter. With a folder picked by Change, that folder; without,
   * pick_folder for the parent and the host makes `<parent>/<name>` (`new_folder`).
   */
  createMatter: "create-matter",
  /**
   * A matter's row button in the rail, with `data-state` (the MatterRow state) and
   * `aria-current="page"` on the open matter. The scratch matter's foot row is not one.
   */
  matterRow: "matter-row",
  /** The rail's find field (shown once there is a matter to find). */
  find: "find",
  /** Matters on the toolbar, which opens and folds the rail below 1200 px. */
  railToggle: "rail-toggle",
  /** The toolbar's title: the open matter's name. */
  matter: "matter",
  /** The window's error line, with `data-code` (the HostError code). */
  error: "error",
  /** The prompt `<textarea>`. */
  composer: "composer",
  /** Ask: sends the prompt; hidden while a turn runs, when Stop stands in its place. */
  send: "send",
  /** Stop: shown while a turn runs. */
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
  /**
   * The line a finished turn ends with, with `data-stop-reason` (the wire StopReason) and, on a
   * failed turn, `data-failure` (its ErrorCode).
   */
  turnStopped: "turn-stopped",
  /**
   * The working line once Claude stopped mid-answer (limits/stopped), with `data-code`: shown for
   * a break this window saw arrive, until the next question.
   */
  agentStopped: "agent-stopped",
  /** Restart on agentStopped, once the host restarts an agent (H10). */
  restart: "restart",
  /** The working line once the thread's stream failed, with `data-code`. */
  streamFailed: "stream-failed",
  /** Try again on streamFailed, for a retryable code only. */
  retry: "retry",
  /** A file the turn changed, in the turn's change note. */
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
export type StopReason = "end_turn" | "cancelled" | "at_limit" | "failed";

/** The window's elements on `page`. */
export function window(page: Page) {
  const id = (name: string): Locator => page.getByTestId(name);
  return {
    matterName: id(ids.matterName),
    newMatter: id(ids.newMatter),
    matterExisting: id(ids.matterExisting),
    /**
     * The New matter form's folder line: the path it names, once there is one (the matter form's
     * `.matter-form-path`; a part's class, not a test id, as ui/app/src/perf/autoplay.ts reads it).
     */
    folderLine: page.locator(".matter-form-path"),
    createMatter: id(ids.createMatter),
    matterRows: id(ids.matterRow),
    /** The rail row of the matter named `name` (its text also holds its status). */
    matterRow: (name: string): Locator => id(ids.matterRow).filter({ hasText: name }),
    /** The rail's find field. */
    findMatter: id(ids.find),
    /**
     * Matters on the toolbar, which unfolds the rail below 1200 px (the e2e window opens at
     * 1280 x 800, crates/desktop's e2e.rs, so the rail starts open).
     */
    railToggle: id(ids.railToggle),
    matter: id(ids.matter),
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
    agentStopped: id(ids.agentStopped),
    restart: id(ids.restart),
    streamFailed: id(ids.streamFailed),
    retry: id(ids.retry),
    fileChanged: id(ids.fileChanged),
    notices: id(ids.notice),
    limitStop: id(ids.limitStop),
    limitResetsAt: id(ids.limitResetsAt),
    undo: id(ids.undo),
  };
}

export type Window = ReturnType<typeof window>;
