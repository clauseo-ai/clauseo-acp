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
  /**
   * A conversation's row button under the open matter's rail row, with `data-state` (the
   * ConversationState), `data-conversation` (its key) and `aria-current="true"` on the open one.
   * Its text is the title ("New conversation" for a draft or a thread with no title yet), then,
   * on a row not open, its status words for assistive technology ("Answering.").
   */
  conversation: "conversation",
  /** New conversation, under the open matter's conversations: opens a draft, sending nothing. */
  newConversation: "new-conversation",
  /** The rail's find field (shown once there is a matter to find). */
  find: "find",
  /**
   * The rail's Needs you group (U16a): its `section`, under Pinned and above the date groups, holding the
   * matterRow of every matter a conversation of which waits on the lawyer, has an answer ready or failed.
   */
  railNeedsYou: "rail-needs-you",
  /** Matters / Recent under the find field; each side is `rail-switch-matters` or `rail-switch-recent`. */
  railSwitch: "rail-switch",
  /** Recent's list, and each of its rows: a conversation of any matter, with `data-conversation`. */
  railRecent: "rail-recent",
  recentRow: "recent-row",
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
  /** A tool call row, with `data-tool-call-id` and `data-status`, shown once its step group is opened. */
  toolCall: "tool-call",
  /** A step group: the tool calls and thoughts between two of the agent's texts, folded into one line. */
  stepGroup: "step-group",
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
  /**
   * The toolbar's plan line, with `data-tone` (the MeterTone: neutral, near, at_limit) and
   * `data-freshness` (the Freshness: fresh, stale, loading, unavailable, missing_scope,
   * not_applicable, live_only).
   */
  plan: "plan",
  /**
   * The hidden marker in the toolbar's plan slot (live-plan.tsx), present whenever the conversation's agent is
   * known: `data-freshness` is the entry's freshness, "none" before any view, and "live_only" while the line is
   * hidden. Attached, not visible.
   */
  planState: "plan-state",
  /** The plan card that the plan line opens, with the windows and their resets. */
  planCard: "plan-card",
  /** Try again in the plan card, which asks for a fresh read (`refresh_limits` with `try_again`). */
  planTryAgain: "plan-try-again",
  /** The warning in the composer's notice line under the field; never blocks sending. */
  limitWarning: "limit-warning",
  /** The Plan and usage section of the settings window. */
  usageSection: "usage-section",
  /** A finished turn's Undo. */
  undo: "undo",
  /** The Versions panel, which stands where the rail of matters stood while it is open. */
  versions: "versions",
  /** One version row of the panel, with `data-checkpoint` and `data-kind` (`VersionKind`). */
  version: "version",
  /** Undo in the band of the selected version. */
  versionUndo: "version-undo",
  /** Restore in the band of the selected version. */
  versionRestore: "version-restore",
  /** The line that says Restore and Undo wait for an answer to finish. */
  versionsWait: "versions-wait",
  /** The confirm button of the Restore or Undo dialog. */
  dialogConfirm: "dialog-confirm",
  /** The dialog's Try again, after a refusal such as Word holding a file. */
  dialogTryAgain: "dialog-try-again",
  /** The lock block under the change note, when a program holds a file the undo would write. */
  wordLock: "word-lock",
  /** Try again in the lock block, once the file is closed in Word. */
  wordLockRetry: "word-lock-retry",
  /** Open in Word: a .doc or .docx paper's tile on the wall, or the change note's button. */
  openInWord: "open-in-word",
  /**
   * The wait panel of a conversation whose turn waits behind another conversation of the matter,
   * which holds the matter's writer: it names that conversation by its title, with no clock time.
   */
  otherWait: "other-wait",
  /** In otherWait: stops the other conversation's turn, so this conversation's turn runs next. */
  stopOther: "stop-other",
  /** In otherWait: the panel's Queue button. */
  queue: "queue",
  /** The composer's working line while a turn runs or waits ("Answering.", or the wait behind another). */
  working: "working",
  /** Settings at the rail's foot: opens the settings window, or brings it forward when open. */
  openSettings: "open-settings",
  /** One message queued over the composer while a turn runs, with `data-turn` (the turn that answers it). */
  queued: "queued",
  /** In a queued row: edits its text. */
  queuedEdit: "queued-edit",
  /** In a queued row: removes it from the queue. */
  queuedRemove: "queued-remove",
  /** In a queued row: sends it now, ending the running turn when there is one. */
  queuedSendNow: "queued-send-now",
  /** The held strip's Send next: sends the next queued message while the queue is held. */
  sendNext: "send-next",
  /**
   * The queue strip's head line (`role="status"`): the held line after a stop or a failure, or
   * the count line otherwise.
   */
  queueSay: "queue-say",
  /** The line above the field while the lawyer types during an answer the agent can steer. */
  steerHint: "steer-hint",
  /** In steerHint: sends the typed words now, steering the answer or stopping it. */
  steerSendNow: "steer-send-now",
  /** The lawyer's words steered into an answer, standing in it where they arrived (`data-entry-id`). */
  steerMark: "steer-mark",
  /** Where a steered message went, with `data-outcome` ("requeued" or "sent_now"). */
  steerOutcome: "steer-outcome",
} as const;

/** The settings window's page: the last part of its URL path (crates/desktop's settings window). */
export const SETTINGS_PAGE = "settings.html";

/** The `data-kind` of a version row: the matter's CheckpointKind in snake_case. */
export type VersionKind =
  | "after_turn"
  | "outside_edits"
  | "undo"
  | "restore"
  | "import"
  | "before_turn";

/** The ACP option kinds a permission button is labelled by. */
export type OptionKind = "allow_once" | "allow_always" | "reject_once" | "reject_always";

/** The `data-outcome` of a steer outcome line; an injected steer has none, its words show as a steer mark. */
export type SteerOutcomeKind = "requeued" | "sent_now";

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
    /** The open matter's conversation rows, in the rail's order. */
    conversations: id(ids.conversation),
    /** The conversation row whose text contains `title` (its text also holds its status). */
    conversationRow: (title: string): Locator => id(ids.conversation).filter({ hasText: title }),
    /**
     * The conversation row whose own title contains `title`, its status words aside: a row that
     * waits for another holds the other's title in its words.
     */
    conversationTitled: (title: string): Locator =>
      id(ids.conversation).filter({ has: page.locator(".room-conv-title", { hasText: title }) }),
    /**
     * A conversation row's title alone, without its plan or status words (the row's
     * `.room-conv-title`; a part's class, not a test id, like folderLine).
     */
    conversationTitle: (row: Locator): Locator => row.locator(".room-conv-title"),
    /**
     * A conversation row's plan words (the row's `.room-conv-plan`; a part's class, not a test id).
     * Only a row whose plan differs from the matter's plan has one.
     */
    conversationPlan: (row: Locator): Locator => row.locator(".room-conv-plan"),
    newConversation: id(ids.newConversation),
    /** The rail's Needs you group. */
    needsYou: id(ids.railNeedsYou),
    /** The row of the matter named `name` in Needs you. */
    needsYouRow: (name: string): Locator =>
      id(ids.railNeedsYou).getByTestId(ids.matterRow).filter({ hasText: name }),
    /**
     * A Needs-you row's conversation line: the title of the conversation behind its status (the row's
     * `.room-matter-where`; a part's class, not a test id, like folderLine).
     */
    matterWhere: (row: Locator): Locator => row.locator(".room-matter-where"),
    /** Matters / Recent's Recent side. */
    railRecentSide: id(`${ids.railSwitch}-recent`),
    recentRows: id(ids.recentRow),
    /** The rail's find field. */
    findMatter: id(ids.find),
    /**
     * Matters on the toolbar, which unfolds the rail below 1200 px (the e2e window opens at
     * 1280 x 800, crates/desktop's e2e.rs, so the rail starts open).
     */
    railToggle: id(ids.railToggle),
    matter: id(ids.matter),
    /**
     * The plan to ask with, offered on a new conversation until its first question: a segmented
     * switch, a group named "Plan to ask with" (status-line.tsx's SegmentedSwitch, no test id).
     */
    planChoice: page.getByRole("group", { name: "Plan to ask with", exact: true }),
    /** One plan in the plan choice, "Claude" or "ChatGPT": a button, `aria-pressed` when chosen. */
    planOption: (plan: string): Locator =>
      page
        .getByRole("group", { name: "Plan to ask with", exact: true })
        .getByRole("button", { name: plan, exact: true }),
    /**
     * The status line's plan words, "Uses your ChatGPT plan", once the conversation has its plan (a
     * part's class, not a test id, like folderLine).
     */
    statusPlan: page.locator(".room-status-plan"),
    error: id(ids.error),
    composer: id(ids.composer),
    send: id(ids.send),
    cancel: id(ids.cancel),
    transcript: id(ids.transcript),
    userMessages: id(ids.userMessage),
    agentMessages: id(ids.agentMessage),
    thoughts: id(ids.thought),
    toolCalls: id(ids.toolCall),
    /** The step groups' lines: each a button that opens its rows (`aria-expanded`). */
    stepGroupLines: id(ids.stepGroup).locator("button"),
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
    /** The toolbar's plan line (`data-tone`, `data-freshness`). */
    plan: id(ids.plan),
    planState: id(ids.planState),
    planCard: id(ids.planCard),
    planTryAgain: id(ids.planTryAgain),
    limitWarning: id(ids.limitWarning),
    undo: id(ids.undo),
    /** The Versions button on the toolbar, which opens the panel. */
    versionsButton: page.getByRole("button", { name: "Versions", exact: true }),
    /**
     * The toolbar's Undo, beside Versions: it opens Versions on the newest turn's change, with its
     * undo dialog. The toolbar is named "Tools for this matter" (no test id).
     */
    toolbarUndo: page
      .getByRole("toolbar", { name: "Tools for this matter", exact: true })
      .getByRole("button", { name: "Undo", exact: true }),
    /**
     * The rail of matters, a navigation named "Matters" (rail.tsx's `aria-label`, no test id). It
     * is out of the page while the Versions panel stands in its place.
     */
    rail: page.getByRole("navigation", { name: "Matters", exact: true }),
    versionsPanel: id(ids.versions),
    /** The panel's Close, which puts the rail back (version-panel.tsx's `.ver-panel-close`). */
    versionsClose: id(ids.versions).getByRole("button", { name: "Close", exact: true }),
    /** The version rows of `kind` in the panel. */
    version: (kind: VersionKind): Locator =>
      page.locator(`[data-testid="${ids.version}"][data-kind="${kind}"]`),
    versionUndo: id(ids.versionUndo),
    versionRestore: id(ids.versionRestore),
    versionsWait: id(ids.versionsWait),
    /** The confirm dialog of Restore or Undo, its confirm and its Try again. */
    dialog: page.getByRole("dialog"),
    dialogConfirm: id(ids.dialogConfirm),
    dialogTryAgain: id(ids.dialogTryAgain),
    /** The lock block: the line that says a paper is open in another program, and its Try again. */
    wordLock: id(ids.wordLock),
    wordLockRetry: id(ids.wordLockRetry),
    /** Open in Word, on a Word paper's tile or the change note. */
    openInWord: id(ids.openInWord),
    /** The wait panel that names the other conversation holding the matter's writer. */
    otherWait: id(ids.otherWait),
    /** The Stop button inside the wait panel `panel`. */
    stopOther: (panel: Locator): Locator => panel.getByTestId(ids.stopOther),
    /** The Queue button inside the wait panel `panel`. */
    queue: (panel: Locator): Locator => panel.getByTestId(ids.queue),
    /** The composer's working line. */
    working: id(ids.working),
    openSettings: id(ids.openSettings),
    /** The queued rows of the strip, in order. */
    queued: id(ids.queued),
    /** The Edit button inside the queued row `row`. */
    queuedEdit: (row: Locator): Locator => row.getByTestId(ids.queuedEdit),
    /** The Remove button inside the queued row `row`. */
    queuedRemove: (row: Locator): Locator => row.getByTestId(ids.queuedRemove),
    /** The Send now button inside the queued row `row`. */
    queuedSendNow: (row: Locator): Locator => row.getByTestId(ids.queuedSendNow),
    /** The strip's Send next, shown while the queue is held. */
    sendNext: id(ids.sendNext),
    /** The strip's head line: the held line, or the count line. */
    queueSay: id(ids.queueSay),
    /** The steer hint above the field, shown while the lawyer types during a steerable answer. */
    steerHint: id(ids.steerHint),
    /** Send now in the steer hint. */
    steerSendNow: id(ids.steerSendNow),
    /** The steered words in the answer. */
    steerMark: id(ids.steerMark),
    /** The steer outcome line of `outcome`: `requeued` or `sent_now`. */
    steerOutcome: (outcome: SteerOutcomeKind): Locator =>
      page.locator(`[data-testid="${ids.steerOutcome}"][data-outcome="${outcome}"]`),
  };
}

export type Window = ReturnType<typeof window>;

/** The settings window's elements on its `page`. Its controls carry no test ids: they are found by role and their
 * English names, as the settings sections (ui/app/src/settings/sections/) label them. */
export function settingsWindow(page: Page) {
  return {
    /** The Plan and usage section, shown once its channel carries the limits (U12). */
    usage: page.getByTestId(ids.usageSection),
    /** Appearance in the section list. */
    appearance: page
      .getByRole("navigation", { name: "Sections", exact: true })
      .getByRole("button", { name: /^Appearance/ }),
    /** One side of Theme's segmented switch (a Base UI toggle group: a group of pressed buttons). */
    theme: (name: "Follow the computer" | "Day" | "Night"): Locator =>
      page
        .getByRole("group", { name: "Theme", exact: true })
        .getByRole("button", { name, exact: true }),
  };
}
