//! The 18 events of research 37 §2.2: one struct per event holding its fields (a cell's
//! combination), and [`Event`], what the app passes to `Counts::bump`.

use super::closed::seal;
use super::{
    Agent, AnswerOutcome, Billing, BreakKind, BreakThen, CrashKind, CrashProcess, Delivery,
    Feature, FirstReadResult, InstallResult, Language, Launch, LimitKind, LiteMode, MatterKind,
    OnboardingStep, Papers, PermissionAnswer, PermissionKind, Place, Protection, ReducedMotion,
    SignInResult, StallTook, StallWhere, StrictErrorCode, TextSize, Theme, TimeOpenBucket, Took,
    Tries, UpdateMode, UpdateResult,
};

/// `app_opened`: each start.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct AppOpened {
    /// A plain start or the first after an update.
    pub launch: Launch,
}

seal!(AppOpened { launch });

/// `time_open`: one cell a day, made at the day's close from the foreground time. Not an
/// [`Event`]: `Counts` adds the foreground time up itself.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct TimeOpen {
    /// The day's foreground time.
    pub bucket: TimeOpenBucket,
}

seal!(TimeOpen { bucket });

/// `onboarding_step`: once per step per install.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Onboarding {
    /// The step.
    pub step: OnboardingStep,
}

seal!(Onboarding { step });

/// `agent_install`: each download ends.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct AgentInstall {
    /// The agent.
    pub agent: Agent,
    /// How it ended.
    pub result: InstallResult,
    /// How many tries it took.
    pub tries: Tries,
    /// How long it took.
    pub took: Took,
}

seal!(AgentInstall {
    agent,
    result,
    tries,
    took
});

/// `sign_in`: each sign-in run ends.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct SignIn {
    /// The agent.
    pub agent: Agent,
    /// How the lawyer pays.
    pub billing: Billing,
    /// How it ended.
    pub result: SignInResult,
}

seal!(SignIn {
    agent,
    billing,
    result
});

/// `matter_opened`: a matter opens.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct MatterOpened {
    /// An existing folder, a new one or the sample.
    pub kind: MatterKind,
    /// How many papers, as a bucket.
    pub papers: Papers,
    /// Where the folder is; left out until stage D's `matters::place` exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    /// Whether the sandbox runs.
    pub protection: Protection,
}

seal!(MatterOpened {
    kind,
    papers,
    place,
    protection
});

/// `first_read`: a first read ends.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct FirstRead {
    /// How it ended.
    pub result: FirstReadResult,
    /// How many papers, as a bucket.
    pub papers: Papers,
    /// How long it took.
    pub took: Took,
}

seal!(FirstRead {
    result,
    papers,
    took
});

/// `question_sent`: the lawyer sends.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct QuestionSent {
    /// The agent.
    pub agent: Agent,
    /// How it went.
    pub delivery: Delivery,
}

seal!(QuestionSent { agent, delivery });

/// `answer_ended`: an answer ends.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct AnswerEnded {
    /// The agent.
    pub agent: Agent,
    /// How it ended.
    pub outcome: AnswerOutcome,
    /// How long the whole answer took.
    pub took: Took,
    /// How long until the first words; left out when none came.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_words: Option<Took>,
}

seal!(AnswerEnded {
    agent,
    outcome,
    took,
    first_words
});

/// `feature_used`: each use.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct FeatureUsed {
    /// The feature.
    pub feature: Feature,
}

seal!(FeatureUsed { feature });

/// `permission_asked`: an escalation is answered.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct PermissionAsked {
    /// What was asked.
    pub kind: PermissionKind,
    /// The answer.
    pub answer: PermissionAnswer,
}

seal!(PermissionAsked { kind, answer });

/// `limit`: the limits crate reports it.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Limit {
    /// The agent.
    pub agent: Agent,
    /// What happened.
    pub kind: LimitKind,
}

seal!(Limit { agent, kind });

/// `break_shown`: one of Item 16's screens.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct BreakShown {
    /// Which break.
    pub kind: BreakKind,
    /// What the lawyer did next.
    pub then: BreakThen,
}

seal!(BreakShown { kind, then });

/// `error_shown`: the UI shows an error's copy.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct ErrorShown {
    /// The code, never `unknown`.
    pub code: StrictErrorCode,
}

seal!(ErrorShown { code });

/// `update`: the updater ends a run.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Update {
    /// How it ended.
    pub result: UpdateResult,
}

seal!(Update { result });

/// `stall`: the hang monitor fires.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Stall {
    /// Which side hung.
    pub r#where: StallWhere,
    /// How long.
    pub took: StallTook,
}

seal!(Stall { r#where, took });

/// `crash` (weekly tally only): the next start after a crash.
#[derive(
    Debug, Clone, PartialEq, Eq, Copy, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Crash {
    /// Which process.
    pub process: CrashProcess,
    /// What kind.
    pub kind: CrashKind,
    /// Whether it came within a short time of the start.
    pub after_launch: bool,
}

seal!(Crash {
    process,
    kind,
    after_launch
});

/// `setting_changed`: a setting changed, with its new value in that setting's own enum.
/// Settings with free values (the matters folder, keys, addresses) have no variant.
/// `preparing_ahead` and `keep_versions` wait for their settings to exist (a new variant is
/// added before the release that sends it, as for error codes).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(
    tag = "setting",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub enum SettingChanged {
    /// `ui.language`.
    Language(Language),
    /// `ui.theme`.
    Theme(Theme),
    /// `ui.font_size`.
    TextSize(TextSize),
    /// `ui.lite`.
    LiteMode(LiteMode),
    /// `ui.motion`.
    ReducedMotion(ReducedMotion),
    /// `update.mode` (R23).
    UpdateMode(UpdateMode),
    /// `privacy.crash_reports` (S22). Counts' own switch is never counted: off sends nothing.
    CrashReports(bool),
    /// The default agent.
    DefaultAgent(Agent),
}

impl super::closed::sealed::Sealed for SettingChanged {}
impl super::Closed for SettingChanged {}

const _: fn() = || {
    super::require_closed::<Language>();
    super::require_closed::<Theme>();
    super::require_closed::<TextSize>();
    super::require_closed::<LiteMode>();
    super::require_closed::<ReducedMotion>();
    super::require_closed::<UpdateMode>();
    super::require_closed::<bool>();
    super::require_closed::<Agent>();
};

/// One thing to count. Each variant holds its event's fields, all closed types, so a call site
/// cannot pass text. `time_open` is not here (the foreground time makes it), nor `active_week`
/// (the tally marks each week the app ran).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `app_opened`.
    AppOpened(AppOpened),
    /// `onboarding_step`, counted once per step per install.
    Onboarding(Onboarding),
    /// `agent_install`.
    AgentInstall(AgentInstall),
    /// `sign_in`.
    SignIn(SignIn),
    /// `matter_opened`.
    MatterOpened(MatterOpened),
    /// `first_read`.
    FirstRead(FirstRead),
    /// `question_sent`.
    QuestionSent(QuestionSent),
    /// `answer_ended`.
    AnswerEnded(AnswerEnded),
    /// `feature_used`.
    FeatureUsed(FeatureUsed),
    /// `permission_asked`.
    PermissionAsked(PermissionAsked),
    /// `limit`.
    Limit(Limit),
    /// `break_shown`.
    BreakShown(BreakShown),
    /// `error_shown`.
    ErrorShown(ErrorShown),
    /// `update`.
    Update(Update),
    /// `setting_changed`.
    SettingChanged(SettingChanged),
    /// `stall`.
    Stall(Stall),
    /// `crash`, which goes in the weekly tally, never the day report.
    Crash(Crash),
}
