//! Arbitrary closed values for the property tests: every type draws from a [`Tape`] of numbers
//! that proptest supplies and shrinks.

use crate::schema::{Cell, Count, Event, SettingChanged};

/// A run of numbers to draw values from. Past its end it keeps going with mixed values, so a
/// short tape still makes whole values.
pub(crate) struct Tape {
    values: Vec<u64>,
    at: u64,
}

impl Tape {
    pub(crate) fn new(values: Vec<u64>) -> Tape {
        Tape { values, at: 0 }
    }

    pub(crate) fn next(&mut self) -> u64 {
        let index = usize::try_from(self.at).unwrap();
        let value = match self.values.get(index) {
            Some(&value) => value,
            None => crate::schema::tests::mix(self.at),
        };
        self.at += 1;
        value
    }

    /// A number below `n`.
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// One of `all`.
    pub(crate) fn pick<T: Copy>(&mut self, all: &[T]) -> T {
        let len = u64::try_from(all.len()).unwrap();
        all[usize::try_from(self.below(len)).unwrap()]
    }
}

/// A type the property tests can draw.
pub(crate) trait Sample: Sized {
    fn sample(tape: &mut Tape) -> Self;
}

impl Sample for bool {
    fn sample(tape: &mut Tape) -> Self {
        tape.below(2) == 1
    }
}

impl<T: Sample> Sample for Option<T> {
    fn sample(tape: &mut Tape) -> Self {
        bool::sample(tape).then(|| T::sample(tape))
    }
}

impl<T: Sample> Sample for Vec<T> {
    fn sample(tape: &mut Tape) -> Self {
        (0..tape.below(4)).map(|_| T::sample(tape)).collect()
    }
}

impl<K: Sample> Sample for Cell<K> {
    fn sample(tape: &mut Tape) -> Self {
        Cell {
            key: K::sample(tape),
            n: Count::sample(tape),
        }
    }
}

impl Sample for SettingChanged {
    fn sample(tape: &mut Tape) -> Self {
        match tape.below(8) {
            0 => SettingChanged::Language(Sample::sample(tape)),
            1 => SettingChanged::Theme(Sample::sample(tape)),
            2 => SettingChanged::TextSize(Sample::sample(tape)),
            3 => SettingChanged::LiteMode(Sample::sample(tape)),
            4 => SettingChanged::ReducedMotion(Sample::sample(tape)),
            5 => SettingChanged::UpdateMode(Sample::sample(tape)),
            6 => SettingChanged::CrashReports(Sample::sample(tape)),
            _ => SettingChanged::DefaultAgent(Sample::sample(tape)),
        }
    }
}

impl Sample for Event {
    fn sample(tape: &mut Tape) -> Self {
        match tape.below(17) {
            0 => Event::AppOpened(Sample::sample(tape)),
            1 => Event::Onboarding(Sample::sample(tape)),
            2 => Event::AgentInstall(Sample::sample(tape)),
            3 => Event::SignIn(Sample::sample(tape)),
            4 => Event::MatterOpened(Sample::sample(tape)),
            5 => Event::FirstRead(Sample::sample(tape)),
            6 => Event::QuestionSent(Sample::sample(tape)),
            7 => Event::AnswerEnded(Sample::sample(tape)),
            8 => Event::FeatureUsed(Sample::sample(tape)),
            9 => Event::PermissionAsked(Sample::sample(tape)),
            10 => Event::Limit(Sample::sample(tape)),
            11 => Event::BreakShown(Sample::sample(tape)),
            12 => Event::ErrorShown(Sample::sample(tape)),
            13 => Event::Update(Sample::sample(tape)),
            14 => Event::SettingChanged(Sample::sample(tape)),
            15 => Event::Stall(Sample::sample(tape)),
            _ => Event::Crash(Sample::sample(tape)),
        }
    }
}
