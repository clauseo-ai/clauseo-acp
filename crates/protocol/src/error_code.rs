//! `ErrorCode`, the frozen dotted codes a host call fails with (`HostError` is the failure).
//!
//! Codes are append-only and a code's meaning never changes. The UI picks its copy by code, never
//! from `message`. `docs/decisions/0003-error-codes.md` holds one row per code; a test checks it
//! against [`ErrorCode::ALL`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Error;
use crate::serde_util::BorrowedStr;

/// Writes `ErrorCode` and its methods from one table: variant, dotted string, retryable, and the
/// meaning, which is both the variant's doc comment and what `meaning()` returns. The record in
/// `docs/decisions/0003-error-codes.md` has the same text in its meaning column.
macro_rules! error_codes {
    ($($variant:ident = $code:literal, retryable: $retryable:literal, meaning: $meaning:literal;)+) => {
        /// Why a host call failed: a stable dotted string such as `store.disk_full`.
        ///
        /// A string this version does not know decodes to [`ErrorCode::Unknown`] (and
        /// [`HostError`] keeps the original string in its `message`). Not `non_exhaustive`:
        /// `Unknown` covers newer codes on the wire, so a match lists every code and a new code
        /// flags each match that should handle it. `Unknown` stays last; a test checks it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        pub enum ErrorCode {
            $(#[doc = $meaning] #[serde(rename = $code)] $variant,)+
        }

        impl ErrorCode {
            /// Every code, in table order.
            pub const ALL: [Self; $crate::open_enum::count_idents!($($variant)+)] =
                [$(Self::$variant),+];

            /// The Rust name of the variant, such as `"StoreDiskFull"`, for tests.
            #[doc(hidden)]
            pub const fn variant_name(&self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                }
            }

            /// The dotted string, such as `"store.disk_full"`.
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }

            /// What the code means: the same sentence as the variant's doc comment and its row in
            /// the record.
            pub const fn meaning(&self) -> &'static str {
                match self {
                    $(Self::$variant => $meaning,)+
                }
            }

            /// Whether the same call may succeed if made again later, with nothing else changed.
            pub const fn is_retryable(&self) -> bool {
                match self {
                    $(Self::$variant => $retryable,)+
                }
            }

            /// The code for `code`, or `None` when this version does not know it.
            pub(crate) fn lookup(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

error_codes! {
    Internal = "internal", retryable: false, meaning: "A bug or an unexpected failure in the \
        host; Tauri's own string rejections map here too.";
    CommandInvalid = "command.invalid", retryable: false, meaning: "The command failed shape \
        validation: a length, count or format. A query that fails to decode (a `PageLimit` or \
        `ReadLen` out of range) gets this code too.";
    CommandConflict = "command.conflict", retryable: false, meaning: "The command conflicts with \
        what the host holds, such as a reused `command_id` with a different body.";
    CommandUnknownMatter = "command.unknown_matter", retryable: false, meaning: "The command \
        names a matter the host does not know.";
    CommandUnknownThread = "command.unknown_thread", retryable: false, meaning: "The command \
        names a thread the host does not know.";
    ThreadTurnRunning = "thread.turn_running", retryable: true, meaning: "A turn is running, so \
        the thread refuses this command (archive, restart) for now.";
    ThreadInterrupted = "thread.interrupted", retryable: false, meaning: "The host stopped during \
        the turn; the turn ended without its answer.";
    ThreadTurnNotQueued = "thread.turn_not_queued", retryable: false, meaning: "The queued turn \
        an edit, remove or send-now names has already started or ended.";
    ThreadQueueFull = "thread.queue_full", retryable: true, meaning: "The thread already has \
        `MAX_QUEUED_TURNS` queued turns.";
    PermissionNotPending = "permission.not_pending", retryable: false, meaning: "The permission \
        request was already answered, expired or never existed.";
    PermissionUnknownOption = "permission.unknown_option", retryable: false, meaning: "The answer \
        names an option the permission request did not offer.";
    ElicitationNotPending = "elicitation.not_pending", retryable: false, meaning: "The \
        elicitation was already answered, expired or never existed.";
    ElicitationInvalidValues = "elicitation.invalid_values", retryable: false, meaning: "The \
        values do not match the elicitation's schema.";
    AgentLaunchFailed = "agent.launch_failed", retryable: true, meaning: "The agent's process did \
        not start or did not finish the ACP handshake.";
    AgentExited = "agent.exited", retryable: true, meaning: "The agent's process exited; a new \
        launch may work.";
    AgentProtocolError = "agent.protocol_error", retryable: false, meaning: "The agent sent \
        something that breaks ACP.";
    AgentAuthRequired = "agent.auth_required", retryable: false, meaning: "The agent needs the \
        lawyer to sign in first.";
    AgentUnsupportedVersion = "agent.unsupported_version", retryable: false, meaning: "The agent \
        speaks an ACP version Clauseo does not support.";
    StoreDiskFull = "store.disk_full", retryable: false, meaning: "The disk is full; space must \
        be freed first.";
    StoreCorrupt = "store.corrupt", retryable: false, meaning: "The data folder holds data that \
        does not read back, a stored value that does not parse included.";
    StoreIo = "store.io", retryable: true, meaning: "A read or write in the data folder failed, \
        possibly for a passing reason.";
    StoreLocked = "store.locked", retryable: false, meaning: "Another Clauseo process holds the \
        data folder.";
    StoreNewerVersion = "store.newer_version", retryable: false, meaning: "The data came from a \
        newer Clauseo.";
    MatterCheckpointFailed = "matter.checkpoint_failed", retryable: true, meaning: "Taking a \
        checkpoint of the matter's files failed.";
    MatterFolderMissing = "matter.folder_missing", retryable: true, meaning: "The matter's \
        folder is gone: moved, deleted, or its drive is not mounted or did not answer in time.";
    MatterFolderRefused = "matter.folder_refused", retryable: false, meaning: "The folder may not \
        be a matter: a drive root, the home folder or a system folder.";
    MatterCodexFolder = "matter.codex_folder", retryable: false, meaning: "The matter folder \
        holds a `.codex` folder, whose settings Codex would load.";
    MatterFileLocked = "matter.file_locked", retryable: true, meaning: "A file the operation \
        needs is locked by another program, such as an open document.";
    MatterTurnRunning = "matter.turn_running", retryable: true, meaning: "A turn is running in \
        the matter, so undo or restore waits.";
    MatterFolderIsDrive = "matter.folder_is_drive", retryable: false, meaning: "The folder may \
        not be a matter: it is the root of a drive or a mounted volume.";
    MatterFolderIsHome = "matter.folder_is_home", retryable: false, meaning: "The folder may not \
        be a matter: it is the home folder, or holds it.";
    MatterFolderIsSystem = "matter.folder_is_system", retryable: false, meaning: "The folder may \
        not be a matter: it is a system folder or a folder of app data or keys, or holds one.";
    MatterFolderUnreadable = "matter.folder_unreadable", retryable: false, meaning: "Clauseo may \
        not read the folder (a `.clauseo/matter.json` that does not read is set aside, not \
        refused, since 2026-11-11).";
    MatterFolderIsMatter = "matter.folder_is_matter", retryable: false, meaning: "The folder is \
        a matter's folder, or sits inside one or holds one; the error's `matter` names that \
        matter when it is on Clauseo's list.";
    MatterFolderInMattersRoot = "matter.folder_in_matters_root", retryable: false, meaning: "The \
        folder is or holds Clauseo's data folder, or is, holds or sits inside the matters' \
        history folder in it.";
    MatterFolderNetwork = "matter.folder_network", retryable: false, meaning: "The matter \
        folder is on a network drive, where Codex's sandbox can neither read nor write, so Codex \
        is not started there.";
    MatterFolderNoPermissions = "matter.folder_no_permissions", retryable: false,
        meaning: "The matter folder is on a drive that keeps no file permissions (FAT, exFAT), \
        which Codex's sandbox needs, so Codex is not started there.";
    TransportUnauthorized = "transport.unauthorized", retryable: false, meaning: "The client is \
        not allowed to make this call.";
    TransportTooLarge = "transport.too_large", retryable: false, meaning: "The message is larger \
        than the transport accepts.";
    SettingsLocked = "settings.locked", retryable: false, meaning: "The change names a setting \
        that the firm's policy sets, so the lawyer cannot change it; nothing was changed.";
    AgentInstallOffline = "agent.install_offline", retryable: true, meaning: "The agent's install \
        cannot reach its hosts: name resolution failed, connect failed with no route, or the \
        connection was reset, aborted or timed out. The install waits and tries again by itself; \
        the app says it is offline only while the window says so, else that the download cannot \
        be reached.";
    AgentInstallBlocked = "agent.install_blocked", retryable: false, meaning: "A proxy or \
        firewall blocked the agent's download: a 403 or 407 from a proxy, a TLS failure, or a web \
        page where a file should be.";
    AgentInstallDiskFull = "agent.install_disk_full", retryable: true, meaning: "Too little free \
        space for the agent's install; a retry succeeds once space is freed.";
    AgentInstallFailed = "agent.install_failed", retryable: true, meaning: "The agent's install \
        failed for another reason, after three tries.";
    SigninCancelled = "signin.cancelled", retryable: false, meaning: "The lawyer cancelled the \
        sign-in, in Clauseo or on the vendor's page.";
    SigninLinkExpired = "signin.link_expired", retryable: true, meaning: "The sign-in link or \
        device code expired before the sign-in finished; a new sign-in gets a new one.";
    SigninNoPlan = "signin.no_plan", retryable: false, meaning: "The account has no plan the \
        agent accepts.";
    SigninDeviceCodeOff = "signin.device_code_off", retryable: false, meaning: "Device-code \
        sign-in is turned off for the account.";
    SigninPortBusy = "signin.port_busy", retryable: true, meaning: "Another program holds the \
        port the agent's sign-in listens on (Codex's 1455).";
    SigninFailed = "signin.failed", retryable: true, meaning: "The sign-in ended before signing \
        in, for any other reason.";
    SigninPolicyUnreadable = "signin.policy_unreadable", retryable: false, meaning: "The firm's \
        policy cannot be read and once set this agent's firm plan, so sign-in and launch are \
        refused rather than sent to a plan login.";
    AgentGuardMissing = "agent.guard_missing", retryable: false, meaning: "A Claude launch that \
        must ask before every change could not set up the guard that makes it ask, so it was \
        not started.";
    UpdateFailed = "update.failed", retryable: true, meaning: "The update's check, download, \
        signature check or install failed; the version running keeps running, and a later check \
        tries again.";
    SettingsRefused = "settings.refused", retryable: false, meaning: "The change breaks a \
        settings rule, such as an agent entry that is not Clauseo's own install, a secret-shaped \
        variable or an address that is not https; the message names the key, never the value, \
        and nothing was changed.";
    SettingsKeyStored = "settings.key_stored", retryable: false, meaning: "A key is already \
        stored for this agent, so this window may not replace it; it is changed or removed in \
        Settings. No key text is in the message.";
    SigninLoginShared = "signin.login_shared", retryable: false, meaning: "A Codex login, logout \
        or authenticate was refused while Clauseo's Codex home shares the lawyer's ChatGPT login \
        file; the remedy is in the app, \"Stop using it here\".";
    SigninFirmManaged = "signin.firm_managed", retryable: false, meaning: "Sign-in is \
        refused: a firm endpoint is set by policy.";
    SigninKeyInUse = "signin.key_in_use", retryable: false, meaning: "Sign-in is refused: an \
        API key is stored.";
    SigninEndpointInUse = "signin.endpoint_in_use", retryable: false, meaning: "Sign-in is \
        refused: an endpoint is set in Settings.";
    MatterFolderTooDeep = "matter.folder_too_deep", retryable: false, meaning: "The matter \
        folder's path is too long for Windows to start programs in it: move the folder closer \
        to the top of its drive.";
    SigninAgentsStillRunning = "signin.agents_still_running", retryable: true, meaning: "An \
        agent was still running after Clauseo waited for it to stop, so the login was not \
        changed and nothing was removed; trying again once it has stopped can succeed.";
    AgentKeysLoading = "agent.keys_loading", retryable: true, meaning: "Clauseo has not \
        finished loading the stored API keys; no agent was started; trying again in a moment can \
        succeed.";
    AgentKeyUnreadable = "agent.key_unreadable", retryable: true, meaning: "A stored API key \
        could not be read from the system's key store in time: the read had not answered (often \
        a permission prompt not answered yet); no agent was started. Trying again once it can be \
        read can succeed.";
    MatterDocumentPathTooLong = "matter.document_path_too_long", retryable: false, meaning: "A \
        Word document's path is too long for Word to open, even with its folders' short names \
        (Windows): move the matter's folder nearer the top of its drive, or shorten its folder \
        names.";
    MatterProtectedLink = "matter.protected_link", retryable: false, meaning: "A protected \
        name in the matter folder (such as a nested CLAUDE.md, AGENTS.md or .claude) is a \
        shortcut (a junction or symbolic link), which the Windows sandbox can't protect, so the \
        agent is not started; the lawyer moves or deletes it.";
    SandboxSetupRefused = "sandbox.setup_refused", retryable: true, meaning: "The sandbox's set-up \
        was not done: the lawyer declined the administrator prompt, or Windows refused to raise \
        it; asking again can succeed.";
    SandboxSetupFailed = "sandbox.setup_failed", retryable: true, meaning: "The sandbox's set-up, \
        or the check that the sandbox works, failed; trying again can succeed.";
    SandboxSetupRequired = "sandbox.setup_required", retryable: false, meaning: "The firm \
        requires the sandbox (sandbox.required) and this computer needs its one-time set-up \
        first (not done, or its administrator prompt declined), so no agent was started; \
        running the set-up fixes it.";
    SandboxRequired = "sandbox.required", retryable: false, meaning: "The firm requires the \
        sandbox (sandbox.required) and this computer cannot give it for a reason the set-up does \
        not fix (no sandbox here, a failed set-up, a full pool, protection switched off, or a \
        newer cause), so no agent was started.";
    MatterFolderNotAMatter = "matter.folder_not_a_matter", retryable: false, meaning: "The \
        matter's folder is there, but its Clauseo file (`.clauseo/matter.json`) is missing, \
        unreadable or belongs to another matter: locate the matter's folder.";
    SandboxBlocked = "sandbox.blocked", retryable: false, meaning: "A Windows setting or \
        security tool blocks the sandbox (its set-up program is missing or was blocked, say), so \
        no agent was started; whoever looks after this computer can fix it.";
    BlocklistOverrideOff = "blocklist.override_off", retryable: false, meaning: "Allowing a \
        site on Clauseo's unsafe-site list is turned off (network.allow_unsafe_site_override is \
        false, usually by the firm's policy), so nothing was allowed.";
    BlocklistNoticeExpired = "blocklist.notice_expired", retryable: false, meaning: "The \
        blocked-site notice an allow names is one the host no longer keeps (too old, or from \
        before a restart), so nothing was allowed: allow the site from a new notice.";
    BlocklistAllowNotSaved = "blocklist.allow_not_saved", retryable: false, meaning: "The \
        allow applies until Clauseo closes, but Clauseo couldn't save it (the OS secret store \
        or the data folder failed), so the site is blocked again after a restart.";
    BlocklistRevokeNotSaved = "blocklist.revoke_not_saved", retryable: false, meaning: "The \
        site is blocked again until Clauseo closes, but Clauseo couldn't save that or void the \
        saved allows, so the site may be allowed again after a restart.";
    SandboxCleaningUp = "sandbox.cleaning_up", retryable: true, meaning: "An earlier Safe \
        workspace session's clean-up (its unclean-session marker's stop or sweep) wasn't finished \
        after Clauseo waited for it, so no agent was started; trying again in a moment can \
        succeed.";
    AgentInstallSetupFailed = "agent.install_setup_failed", retryable: true, meaning: "The \
        agent's files all downloaded and checked, but setting them up on this computer failed \
        after three tries (unpacking, `npm ci`, the move into place or its receipt), often \
        because security software held the fresh files. Trying again may work.";
    BlacklineNoWordTool = "blackline.no_word_tool", retryable: false, meaning: "The Word \
        blackline needs stetdoc, Clauseo's Word tool, and this Clauseo has none, so nothing was \
        compared.";
    BlacklineNotWord = "blackline.not_word", retryable: false, meaning: "The file is not a \
        Word document (.docx), so it has no Word blackline.";
    BlacklineFailed = "blackline.failed", retryable: true, meaning: "The Word blackline \
        failed: the compare or its view failed, took too long, or printed what Clauseo could not \
        read; nothing is shown and nothing was written in the matter. Trying again can \
        succeed.";
    WordStetdocMissing = "word.stetdoc_missing", retryable: false, meaning: "Clauseo has no \
        stetdoc program to read or edit Word files with, or could not start it.";
    WordRevisionsStale = "word.revisions_stale", retryable: false, meaning: "The Word file \
        changed since its tracked changes were listed, or a change named is no longer in it, so \
        nothing was written: list them again.";
    WordNotDocx = "word.not_docx", retryable: false, meaning: "The file is not a Word document \
        (.docx) that stetdoc can read.";
    WordEditFailed = "word.edit_failed", retryable: true, meaning: "stetdoc failed or did not \
        finish in time while reading or editing the Word file, or the file is not on this computer \
        or is too large to version, so nothing was written.";
    MatterFolderPermission = "matter.folder_permission", retryable: false, meaning: "The \
        computer's privacy settings keep Clauseo, or the agent it starts, from opening the \
        matter's folder (on macOS, Files and Folders in Privacy & Security), so no agent works \
        in it: allow Clauseo there, then ask again.";
    AgentOffline = "agent.offline", retryable: true, meaning: "The agent lost its connection \
        while answering and the computer is offline: the window said offline and the host's own \
        probe of the agent's service did not get through either. The answer so far is kept; \
        asking again can work once the computer is back online.";
    MatterFolderInClauseoData = "matter.folder_in_clauseo_data", retryable: false,
        meaning: "The folder is, holds or sits inside Clauseo's data folder; one that is or sits \
        inside the matters' history folder in it is `matter.folder_in_matters_root`.";
    MatterFolderInsideMatter = "matter.folder_inside_matter", retryable: false, meaning: "The \
        folder sits inside a matter's folder; the error's `matter` names that matter when it is \
        on Clauseo's list.";
    MatterFolderHoldsMatter = "matter.folder_holds_matter", retryable: false, meaning: "The \
        folder holds a matter's folder; the error's `matter` names that matter when it is on \
        Clauseo's list.";
    PromptTooLong = "prompt.too_long", retryable: false, meaning: "The prompt's text is over \
        `MAX_PROMPT_TEXT_BYTES` (6 MiB, counted as JSON-escaped bytes), in a `SendPrompt` or an \
        `EditQueuedPrompt`, so nothing was sent; the draft is kept. A `ChatGPT` prompt over Codex's \
        own input limit (1,048,576 characters, as the host counts them) gets it too: refused the \
        same way when sent or edited, or, when the undo note added as it goes out takes it over, \
        its turn fails with this code before anything is sent, the queue held. Only the prompt's \
        text gets this code: every other size limit stays `command.invalid`.";
    McpNameEmpty = "mcp.name_empty", retryable: false, meaning: "The MCP server has no name.";
    McpNameTaken = "mcp.name_taken", retryable: false, meaning: "Another of the lawyer's MCP \
        servers already has this name.";
    McpNameReserved = "mcp.name_reserved", retryable: false, meaning: "The MCP server name is \
        kept for one of Clauseo's own tools (`browser`, `case-law`) or Claude Code's built-ins \
        (`ide`, `claude-in-chrome`, `computer-use`), in any case.";
    McpNameInvalid = "mcp.name_invalid", retryable: false, meaning: "The MCP server name is not \
        1 to 64 letters, digits, `_` or `-`, or holds `__`, or ends in `_`.";
    McpCommandInvalid = "mcp.command_invalid", retryable: false, meaning: "The local MCP \
        server's program is neither a bare name (`npx`) nor an absolute path on this computer: a \
        relative path, a network path, or empty.";
    McpCommandNotFound = "mcp.command_not_found", retryable: false, meaning: "The local MCP \
        server's program is not on this computer: a bare name found on no search path, or an \
        absolute path with no file. The program is the one the command named.";
    McpUrlInvalid = "mcp.url_invalid", retryable: false, meaning: "The remote MCP server's URL \
        is not an `https` URL with a host (`http` only for `localhost` or `127.0.0.1`), or \
        holds a user name or password.";
    McpHeaderNameInvalid = "mcp.header_name_invalid", retryable: false, meaning: "A header name \
        is not an HTTP token, or appears twice in any case.";
    McpEnvNameInvalid = "mcp.env_name_invalid", retryable: false, meaning: "A variable name is \
        not a letter or `_` followed by letters, digits or `_`, or appears twice.";
    McpSecretStoreFailed = "mcp.secret_store_failed", retryable: true, meaning: "The OS \
        keychain refused or failed to save a value, so the MCP server was not changed.";
    McpServerUnknown = "mcp.server_unknown", retryable: false, meaning: "The lawyer has no MCP \
        server of that name.";
    McpSaveDeclined = "mcp.save_declined", retryable: false, meaning: "The lawyer declined the \
        native confirm before an MCP server was added or its program, arguments, variable names \
        or URL changed, so nothing was saved.";
    AgentServiceError = "agent.service_error", retryable: true, meaning: "The agent's service \
        had a problem on its side and the agent gave up on the turn: an AIR `sessionFailure` of \
        severity `error` offering `retry` or `new_session`, in a category no other code reads \
        (codex-acp's `internal_error`, `provider_error`, `overloaded`). The answer so far is \
        kept; asking again can work.";
    AgentQuiet = "agent.quiet", retryable: true, meaning: "The agent sent nothing for 5 minutes \
        while answering, with no tool running and no question waiting for the lawyer, so the \
        host ended the turn (D-050's backstop: a blocked network the host's probe could not see, \
        such as a proxy that asks for credentials or one that inspects TLS); or the agent's own \
        request to its service timed out while the host's probe reached that service (a slow \
        service). The answer so far is kept; asking again can work.";
    McpImportTooLarge = "mcp.import_too_large", retryable: false, meaning: "The servers an \
        import would add, with their programs, arguments, URLs and variable or header names, \
        are too long to show whole in one confirm (over 2,000 characters), so nothing was \
        imported.";
    MatterFolderBackslash = "matter.folder_backslash", retryable: false, meaning: "The \
        matter folder's path has a backslash (`\\`) in a folder's name, on macOS or Linux, where \
        the agent cannot start in such a folder (Claude Code exits with \"Can't access working \
        directory\"), so the host did not start it. Renaming the folder fixes it.";
    AgentFolderInaccessible = "agent.folder_inaccessible", retryable: true, meaning: "The \
        agent could not open the matter folder as its working folder when it started (Claude \
        Code's \"Can't access working directory\" in its answer to a startup request), though \
        the host found the folder there just before. Asking again can work once the folder is \
        back where the matter expects it.";
    BlacklineNestedTable = "blackline.nested_table", retryable: false, meaning: "stetdoc could \
        not compare the two versions because of a table inside a table cell (its compare \
        refused the redline it made, `validation_failed`, over a cell), so nothing is shown; \
        trying again gives the same answer.";
    BlacklineCannotCompare = "blackline.cannot_compare", retryable: false, meaning: "stetdoc \
        refused to compare the two versions (`validation_failed` or `unsupported`) for a reason \
        other than a table inside a table, so nothing is shown; trying again gives the same \
        answer.";
    BlacklineNotReadable = "blackline.not_readable", retryable: false, meaning: "stetdoc could \
        not read one of the two versions as a Word document (`malformed_input`, \
        `corrupt_complement` or `unsupported_version`), so nothing is shown.";
    BlacklineTooLarge = "blackline.too_large", retryable: false, meaning: "One of the two \
        versions is past a size limit of stetdoc's (`limit_exceeded`), so nothing is shown.";
    BlacklineFormatUnknown = "blackline.format_unknown", retryable: false, meaning: "stetdoc's \
        view of the blackline is in a format this Clauseo does not know (a header other than \
        `stet 1`, or none), so nothing is shown rather than a guess.";
    AgentUnreachable = "agent.unreachable", retryable: true, meaning: "The agent could not get \
        through to its service while answering, and the computer did not seem offline: the \
        service, or a proxy or firewall in front of it, refused, timed out or blocked the \
        connection, or its name did not resolve. The answer so far is kept; asking again can \
        work.";
    WordTooLarge = "word.too_large", retryable: false, meaning: "The Word file is over the \
        matters' size cap (512,000,000 bytes) or one of stetdoc's limits (its `limit_exceeded`: \
        the package's size, entry count or compression ratio, an XML part's size, depth or node \
        count), so its tracked changes were not read or decided.";
    AgentStartOffline = "agent.start_offline", retryable: true, meaning: "The agent did not start: \
        it answered a start request with an error naming a connection failure, and the host's \
        probe of its service did not get through. Asking again can work once back online.";
    AgentAnswerFailed = "agent.answer_failed", retryable: true, meaning: "The agent answered the \
        question with an error no other code reads, or Codex gave up on a connection the host does \
        not read as offline. The answer so far is kept; asking again can work.";
    AgentModeMissing = "agent.mode_missing", retryable: false, meaning: "The session offered no \
        mode Clauseo runs the agent in, so nothing was sent: an unsupported agent version.";
    AgentSettingRefused = "agent.setting_refused", retryable: true, meaning: "The agent refused \
        the lawyer's own mode or setting change on a running session; nothing changed.";
    AgentProgramBlocked = "agent.program_blocked", retryable: false, meaning: "The computer \
        refused to run the agent's program (permission, Windows policy or antivirus).";
    AgentProgramMissing = "agent.program_missing", retryable: false, meaning: "The OS found no \
        agent program to run (not found), so the agent did not start.";
    SkillNoSkillMd = "skill.no_skill_md", retryable: false, meaning: "The skill to add has no \
        `SKILL.md`: the path is neither a folder holding one nor a `SKILL.md` file; nothing was \
        added.";
    SkillInvalid = "skill.invalid", retryable: false, meaning: "The skill to add could not be \
        made a skill pack: it could not be read, it is over 5,000,000 bytes or 200 files, or it \
        breaks a skill pack rule (no name or description in its `SKILL.md`, a script while \
        scripts are off, and the like); nothing was added.";
    MatterFileFolderClash = "matter.file_folder_clash", retryable: false, meaning: "A restore \
        of chosen files names a file whose name is now a folder, or one of whose folders is now \
        a file, so nothing was restored or written.";
    AgentHomeFailed = "agent.home_failed", retryable: true, meaning: "Clauseo could not prepare \
        the agent's own folder under its data folder (its home, or a folder of its session), so \
        the agent was not started. Asking again can work.";
    MatterFileRefused = "matter.file_refused", retryable: true, meaning: "Files the operation \
        would change refused it (their permissions, a second name, or a saved copy that could \
        not be checked); everything else was done.";
    MatterFileWriteFailed = "matter.file_write_failed", retryable: true, meaning: "Writing a \
        file in place failed partway, so the operation stopped there; the file was put back as \
        it was, or may be damaged, and the version before the operation holds it.";
    AgentKeyDenied = "agent.key_denied", retryable: true, meaning: "A stored API key could not \
        be read: the system's key store answered the read with an error (often a permission \
        prompt that was denied or cancelled, or a locked key store); no agent was started. \
        Trying again, and allowing the read, can succeed.";
    AgentStartUnreachable = "agent.start_unreachable", retryable: true, meaning: "The agent did \
        not start: its start failed on its connection, and the host's probe could not reach its \
        service while the window says the computer is online (a slow network, a proxy or a \
        firewall). Asking again can work.";
    Unknown = "unknown", retryable: false, meaning: "A code from a newer Clauseo that this \
        version does not know. Never produced here.";
}

impl ErrorCode {
    /// The code for `code`, or [`ErrorCode::Unknown`] when this version does not know it.
    pub fn from_str_lossy(code: &str) -> Self {
        Self::lookup(code).unwrap_or(Self::Unknown)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ErrorCode {
    type Err = Error;

    /// Parses a dotted code; a string this version does not know is an error here (use
    /// [`ErrorCode::from_str_lossy`] or serde to get [`ErrorCode::Unknown`] instead).
    fn from_str(code: &str) -> Result<Self, Error> {
        Self::lookup(code).ok_or_else(|| Error::UnknownErrorCode {
            code: code.to_owned(),
        })
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = BorrowedStr::deserialize(deserializer)?;
        Ok(Self::from_str_lossy(code.as_str()))
    }
}
