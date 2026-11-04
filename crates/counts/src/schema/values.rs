//! The schema's enums (research 37 §2), each value spelled as it travels.

use super::closed::closed_enum;

closed_enum! {
    "The release channel, a build constant."
    pub enum Channel {
        "Released builds."
        Stable = "stable",
        "Beta builds."
        Beta = "beta",
    }
}

closed_enum! {
    "The operating system. Its major version travels beside it as `os_major`."
    pub enum Os {
        "macOS, `os_major` 13 to 40."
        Macos = "macos",
        "Windows, `os_major` 10 or 11."
        Windows = "windows",
        "Linux, with no version."
        Linux = "linux",
    }
}

closed_enum! {
    "The CPU architecture."
    pub enum Arch {
        "Intel and AMD."
        X86_64 = "x86_64",
        "Apple silicon and ARM64 Windows."
        Aarch64 = "aarch64",
    }
}

closed_enum! {
    "The app's language (`ui.language`)."
    pub enum Language {
        "English."
        En = "en",
        "Hindi."
        Hi = "hi",
    }
}

closed_enum! {
    "Whether the app is installed for one user or for the machine. Tally only."
    pub enum InstallType {
        "Installed for the user alone."
        PerUser = "per_user",
        "Installed for every user, by IT."
        PerMachine = "per_machine",
    }
}

closed_enum! {
    "The lawyer's agent."
    pub enum Agent {
        "Claude Code."
        Claude = "claude",
        "Codex, with a `ChatGPT` account or key."
        Chatgpt = "chatgpt",
    }
}

closed_enum! {
    "`app_opened.launch`."
    pub enum Launch {
        "A plain start."
        Cold = "cold",
        "The first start after an update."
        AfterUpdate = "after_update",
    }
}

closed_enum! {
    "`onboarding_step.step`."
    pub enum OnboardingStep {
        "The welcome showed."
        WelcomeShown = "welcome_shown",
        "A folder was chosen."
        FolderChosen = "folder_chosen",
        "A new matter was made."
        NewMatter = "new_matter",
        "The sample matter opened."
        SampleOpened = "sample_opened",
        "The agent became ready."
        AgentReady = "agent_ready",
        "The lawyer signed in to the agent."
        SignedIn = "signed_in",
        "The first read finished."
        FirstReadDone = "first_read_done",
        "The first question went."
        FirstQuestion = "first_question",
    }
}

closed_enum! {
    "`agent_install.result`."
    pub enum InstallResult {
        "Installed."
        Done = "done",
        "No network."
        Offline = "offline",
        "The network refused the download."
        Blocked = "blocked",
        "The disk was full."
        DiskFull = "disk_full",
        "Any other failure."
        Failed = "failed",
    }
}

closed_enum! {
    "`sign_in.billing`."
    pub enum Billing {
        "The agent vendor's plan."
        Plan = "plan",
        "An API key."
        ApiKey = "api_key",
        "The firm's own address."
        FirmAddress = "firm_address",
    }
}

closed_enum! {
    "`sign_in.result`."
    pub enum SignInResult {
        "Signed in."
        Done = "done",
        "The lawyer cancelled."
        Cancelled = "cancelled",
        "It failed."
        Failed = "failed",
    }
}

closed_enum! {
    "`matter_opened.kind`."
    pub enum MatterKind {
        "An existing folder."
        Folder = "folder",
        "A new folder."
        NewFolder = "new_folder",
        "The sample matter."
        Sample = "sample",
    }
}

closed_enum! {
    "`matter_opened.place`."
    pub enum Place {
        "A local disk."
        Local = "local",
        "A network drive."
        Network = "network",
        "A cloud-sync folder."
        CloudSync = "cloud_sync",
    }
}

closed_enum! {
    "`matter_opened.protection`."
    pub enum Protection {
        "The sandbox runs."
        Protected = "protected",
        "It does not."
        NotProtected = "not_protected",
    }
}

closed_enum! {
    "`first_read.result`."
    pub enum FirstReadResult {
        "It finished."
        Done = "done",
        "The lawyer stopped it."
        Stopped = "stopped",
        "It failed."
        Failed = "failed",
    }
}

closed_enum! {
    "`question_sent.delivery`."
    pub enum Delivery {
        "Sent at once."
        Now = "now",
        "Queued behind an answer."
        Queued = "queued",
        "Sent into a running answer."
        Steer = "steer",
    }
}

closed_enum! {
    "`answer_ended.outcome`."
    pub enum AnswerOutcome {
        "The answer finished."
        Done = "done",
        "The lawyer stopped it."
        StoppedByLawyer = "stopped_by_lawyer",
        "The agent stopped."
        AgentStopped = "agent_stopped",
        "A plan limit stopped it."
        Limit = "limit",
        "The sign-in expired."
        SigninExpired = "signin_expired",
        "The network went."
        Offline = "offline",
        "Any other error."
        Error = "error",
    }
}

closed_enum! {
    "`feature_used.feature`."
    pub enum Feature {
        "Versions."
        Versions = "versions",
        "Compare."
        Compare = "compare",
        "Restore."
        Restore = "restore",
        "Undo."
        Undo = "undo",
        "Open in Word."
        OpenInWord = "open_in_word",
        "A citation."
        Citation = "citation",
        "A source."
        Source = "source",
        "An Indian Kanoon search."
        KanoonSearch = "kanoon_search",
        "Continue with the other agent."
        ContinueOtherAgent = "continue_other_agent",
        "A prepared answer was used."
        PreparedAnswerUsed = "prepared_answer_used",
        "The sample matter."
        SampleMatter = "sample_matter",
    }
}

closed_enum! {
    "`permission_asked.kind`."
    pub enum PermissionKind {
        "Outside the matter folder."
        OutsideFolder = "outside_folder",
        "A new site."
        NewSite = "new_site",
        "The sandbox off."
        SandboxOff = "sandbox_off",
        "Full access."
        FullAccess = "full_access",
    }
}

closed_enum! {
    "`permission_asked.answer`."
    pub enum PermissionAnswer {
        "Allowed once."
        AllowOnce = "allow_once",
        "Always allowed."
        Always = "always",
        "Denied."
        Deny = "deny",
        "Left unanswered."
        LeftOpen = "left_open",
    }
}

closed_enum! {
    "`limit.kind`."
    pub enum LimitKind {
        "75% of the plan's limit."
        Warned75 = "warned_75",
        "The limit reached."
        Reached = "reached",
        "Preparing ahead paused for the limit."
        PreparingPaused = "preparing_paused",
    }
}

closed_enum! {
    "`break_shown.kind`."
    pub enum BreakKind {
        "The agent stopped."
        AgentStopped = "agent_stopped",
        "The sign-in expired."
        SigninExpired = "signin_expired",
        "No network."
        Offline = "offline",
        "A terms notice."
        TermsNotice = "terms_notice",
        "The plan shut the lawyer out."
        PlanShutOut = "plan_shut_out",
    }
}

closed_enum! {
    "`break_shown.then`."
    pub enum BreakThen {
        "Restarted."
        Restarted = "restarted",
        "Signed in."
        SignedIn = "signed_in",
        "Waited."
        Waited = "waited",
        "Switched agent."
        SwitchedAgent = "switched_agent",
        "Left."
        Left = "left",
    }
}

closed_enum! {
    "`update.result`."
    pub enum UpdateResult {
        "Installed."
        Installed = "installed",
        "Failed."
        Failed = "failed",
        "Installed after an answer ended."
        AfterAnswer = "after_answer",
        "Managed by the firm."
        Managed = "managed",
    }
}

closed_enum! {
    "The colour theme (`ui.theme`)."
    pub enum Theme {
        "As the OS is set."
        System = "system",
        "Light."
        Light = "light",
        "Dark."
        Dark = "dark",
    }
}

closed_enum! {
    "Whether the app runs light (`ui.lite`)."
    pub enum LiteMode {
        "The app decides from the machine."
        Automatic = "automatic",
        "Always light."
        On = "on",
        "Never light."
        Off = "off",
    }
}

closed_enum! {
    "Whether the app animates (`ui.motion`)."
    pub enum ReducedMotion {
        "As the OS says."
        System = "system",
        "Little or no animation."
        Reduce = "reduce",
        "Full animation."
        Full = "full",
    }
}

closed_enum! {
    "How the app updates (`update.mode`, R23)."
    pub enum UpdateMode {
        "Installs updates itself."
        Auto = "auto",
        "Tells the lawyer, installs nothing."
        Notify = "notify",
        "Never checks."
        None = "none",
    }
}

closed_enum! {
    "`stall.where`."
    pub enum StallWhere {
        "The host."
        Host = "host",
        "The webview."
        Webview = "webview",
    }
}

closed_enum! {
    "`crash.process`."
    pub enum CrashProcess {
        "The host."
        Host = "host",
        "The webview."
        Webview = "webview",
    }
}

closed_enum! {
    "`crash.kind`."
    pub enum CrashKind {
        "A Rust panic."
        Panic = "panic",
        "A signal or exception."
        Signal = "signal",
        "The webview's renderer went."
        RendererGone = "renderer_gone",
        "A stall the app never came back from."
        Stall = "stall",
    }
}
