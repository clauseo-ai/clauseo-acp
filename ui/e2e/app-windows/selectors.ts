// The internal window's elements, in one place: every spec finds them through here, so a
// renamed test id changes this file only. The test ids are the ones ui/app/src/internal sets;
// every transcript row also carries `data-entry-id`.

import type { Locator, Page } from "@playwright/test";

/** The `data-testid` of each element the specs use. */
export const ids = {
  /** The new matter's name (prefilled "Matter"; New matter is disabled while it is empty). */
  matterName: "matter-name",
  /** Button: pick_folder (E2E_MATTER_DIR in the e2e build), then CreateMatter and a thread. */
  newMatter: "new-matter",
  /** The open matter's name, "Opening…" or "No matter". */
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
