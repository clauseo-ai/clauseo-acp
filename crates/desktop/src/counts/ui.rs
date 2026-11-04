//! `count_ui`'s argument: the events only the webview sees (decision record 0036). A closed
//! enum of closed enums, each refusing an unknown field or value, so the webview can name only
//! these counts and never send text: `error_shown`, `break_shown`, the webview's `feature_used`
//! values and `onboarding_step`. The host counts `undo` and `restore` itself, and stage E's
//! tools their own features, so the webview cannot claim those.
//!
//! The enums mirror `clauseo_counts::schema`'s, value for value and name for name (a test checks
//! the names), so the TypeScript bindings can name them; `clauseo-counts` carries no specta.

use clauseo_counts::schema::{
    BreakKind, BreakShown, ErrorShown, Event, Feature, FeatureUsed, Onboarding, OnboardingStep,
    StrictErrorCode,
};
use serde::Deserialize;

/// One count from the webview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize, specta::Type))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum UiCount {
    /// The UI showed an error's copy. `unknown`, or a code this build does not know, is refused
    /// as the argument is read (`StrictErrorCode`).
    ErrorShown {
        /// The error's code. The bindings name it `ErrorCode`, whose strings it reads.
        #[cfg_attr(test, specta(type = clauseo_protocol::ErrorCode))]
        code: StrictErrorCode,
    },
    /// A break screen showed, and what the lawyer did next.
    BreakShown {
        /// Which break.
        kind: UiBreakKind,
        /// What the lawyer did.
        then: UiBreakThen,
    },
    /// A feature the webview alone sees was used.
    FeatureUsed {
        /// The feature.
        feature: UiFeature,
    },
    /// An onboarding screen's step; each counts once per install.
    OnboardingStep {
        /// The step.
        step: UiOnboardingStep,
    },
}

/// `break_shown.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize, specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiBreakKind {
    /// The agent stopped.
    AgentStopped,
    /// The sign-in expired.
    SigninExpired,
    /// No network.
    Offline,
    /// A terms notice.
    TermsNotice,
    /// The plan shut the lawyer out.
    PlanShutOut,
}

/// `break_shown.then`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize, specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiBreakThen {
    /// Restarted.
    Restarted,
    /// Signed in.
    SignedIn,
    /// Waited.
    Waited,
    /// Switched agent.
    SwitchedAgent,
    /// Left.
    Left,
}

/// The `feature_used` values the webview alone sees. `versions` and `compare` are the Versions
/// panel's (0036: the host would count each refresh, not each use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize, specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiFeature {
    /// Versions opened.
    Versions,
    /// Compare.
    Compare,
    /// Open in Word.
    OpenInWord,
    /// Continue with the other agent.
    ContinueOtherAgent,
    /// A prepared answer was used.
    PreparedAnswerUsed,
    /// The sample matter.
    SampleMatter,
}

/// `onboarding_step.step`: the onboarding screens' steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize, specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiOnboardingStep {
    /// The welcome showed.
    WelcomeShown,
    /// A folder was chosen.
    FolderChosen,
    /// A new matter was made.
    NewMatter,
    /// The sample matter opened.
    SampleOpened,
    /// The agent became ready.
    AgentReady,
    /// The lawyer signed in to the agent.
    SignedIn,
    /// The first read finished.
    FirstReadDone,
    /// The first question went.
    FirstQuestion,
}

impl UiCount {
    /// The counts' event.
    pub(crate) fn event(self) -> Event {
        match self {
            UiCount::ErrorShown { code } => Event::ErrorShown(ErrorShown { code }),
            UiCount::BreakShown { kind, then } => Event::BreakShown(BreakShown {
                kind: kind.into(),
                then: then.into(),
            }),
            UiCount::FeatureUsed { feature } => Event::FeatureUsed(FeatureUsed {
                feature: feature.into(),
            }),
            UiCount::OnboardingStep { step } => Event::Onboarding(Onboarding { step: step.into() }),
        }
    }
}

impl From<UiBreakKind> for BreakKind {
    fn from(kind: UiBreakKind) -> BreakKind {
        match kind {
            UiBreakKind::AgentStopped => BreakKind::AgentStopped,
            UiBreakKind::SigninExpired => BreakKind::SigninExpired,
            UiBreakKind::Offline => BreakKind::Offline,
            UiBreakKind::TermsNotice => BreakKind::TermsNotice,
            UiBreakKind::PlanShutOut => BreakKind::PlanShutOut,
        }
    }
}

impl From<UiBreakThen> for clauseo_counts::schema::BreakThen {
    fn from(then: UiBreakThen) -> clauseo_counts::schema::BreakThen {
        use clauseo_counts::schema::BreakThen;
        match then {
            UiBreakThen::Restarted => BreakThen::Restarted,
            UiBreakThen::SignedIn => BreakThen::SignedIn,
            UiBreakThen::Waited => BreakThen::Waited,
            UiBreakThen::SwitchedAgent => BreakThen::SwitchedAgent,
            UiBreakThen::Left => BreakThen::Left,
        }
    }
}

impl From<UiFeature> for Feature {
    fn from(feature: UiFeature) -> Feature {
        match feature {
            UiFeature::Versions => Feature::Versions,
            UiFeature::Compare => Feature::Compare,
            UiFeature::OpenInWord => Feature::OpenInWord,
            UiFeature::ContinueOtherAgent => Feature::ContinueOtherAgent,
            UiFeature::PreparedAnswerUsed => Feature::PreparedAnswerUsed,
            UiFeature::SampleMatter => Feature::SampleMatter,
        }
    }
}

impl From<UiOnboardingStep> for OnboardingStep {
    fn from(step: UiOnboardingStep) -> OnboardingStep {
        match step {
            UiOnboardingStep::WelcomeShown => OnboardingStep::WelcomeShown,
            UiOnboardingStep::FolderChosen => OnboardingStep::FolderChosen,
            UiOnboardingStep::NewMatter => OnboardingStep::NewMatter,
            UiOnboardingStep::SampleOpened => OnboardingStep::SampleOpened,
            UiOnboardingStep::AgentReady => OnboardingStep::AgentReady,
            UiOnboardingStep::SignedIn => OnboardingStep::SignedIn,
            UiOnboardingStep::FirstReadDone => OnboardingStep::FirstReadDone,
            UiOnboardingStep::FirstQuestion => OnboardingStep::FirstQuestion,
        }
    }
}
