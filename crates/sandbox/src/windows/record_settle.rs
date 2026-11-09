//! A read of the machine's record taken by a reader that holds no set-up lock, settled. Set-up
//! makes `HKLM\SOFTWARE\Clauseo\Sandbox` again on every run (`setup/vendor_key`): it takes the
//! old key over, which shuts readers out (access denied) until it is deleted, writes the new
//! one whole as `Sandbox.new`, deletes the old one and renames the new one into place. So a
//! reader can catch the key denied, or missing for the moment between the delete and the
//! rename. Neither is an answer: [`settle`] reads again after a short pause, up to [`TRIES`]
//! reads, and only then hands the last answer back, which its caller judges as it always has.
//! A missing key is read again only while the new copy is there, or once the key is back.
//! Pure; `record.rs` is the registry. Tested on every OS.

/// The record's new copy, beside the record under `HKLM\SOFTWARE\Clauseo`.
pub(crate) const TEMP_NAME: &str = "Sandbox.new";

/// How many times [`settle`] reads before it gives the last answer back.
pub(crate) const TRIES: usize = 10;

/// The record as one reader sees it.
pub(crate) trait Record {
    /// What the reader reads.
    type Answer;
    /// Why a read failed.
    type Error;
    /// One read through a handle on the record key, so every value comes from one key:
    /// `Ok(None)` when the key isn't there.
    fn read(&mut self) -> Result<Option<Self::Answer>, Self::Error>;
    /// Whether `error` may come from a set-up making the key again: access denied, a key
    /// deleted under the read, or a value of the wrong type or that doesn't parse.
    fn unsteady(&self, error: &Self::Error) -> bool;
    /// Whether the new copy is there, or can't be told.
    fn copy_there(&mut self) -> bool;
    /// Whether the record key is there, or can't be told.
    fn key_there(&mut self) -> bool;
    /// Waits before the next read (`record.rs`: 50 ms, so half a second at most with
    /// [`TRIES`], which covers a set-up's flush of the new copy).
    fn pause(&mut self);
}

/// The record's answer, read again while it may be a set-up's moment rather than the record:
/// an unsteady error, or a missing key while the new copy is there or once the key is back
/// (renamed between the read and the look). `Ok(None)` only when neither was there.
pub(crate) fn settle<R: Record>(record: &mut R) -> Result<Option<R::Answer>, R::Error> {
    let mut reads = 1;
    loop {
        let answer = record.read();
        let again = match &answer {
            Ok(Some(_)) => false,
            Ok(None) => record.copy_there() || record.key_there(),
            Err(error) => record.unsteady(error),
        };
        if !again || reads >= TRIES {
            return answer;
        }
        reads += 1;
        record.pause();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{Record, TRIES, settle};

    /// A failed read: `true` when it is one a set-up may cause.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Failed(bool);

    /// Reads given in order (the last one repeats), and what the looks see.
    struct Script {
        reads: VecDeque<Result<Option<u32>, Failed>>,
        copy: VecDeque<bool>,
        key: VecDeque<bool>,
        done: usize,
        paused: usize,
    }

    impl Script {
        fn new(reads: Vec<Result<Option<u32>, Failed>>) -> Script {
            Script {
                reads: reads.into(),
                copy: VecDeque::new(),
                key: VecDeque::new(),
                done: 0,
                paused: 0,
            }
        }
    }

    fn next<T: Clone>(queue: &mut VecDeque<T>, default: T) -> T {
        if queue.len() > 1 {
            queue.pop_front().unwrap_or(default)
        } else {
            queue.front().cloned().unwrap_or(default)
        }
    }

    impl Record for Script {
        type Answer = u32;
        type Error = Failed;

        fn read(&mut self) -> Result<Option<u32>, Failed> {
            self.done += 1;
            next(&mut self.reads, Ok(None))
        }

        fn unsteady(&self, error: &Failed) -> bool {
            error.0
        }

        fn copy_there(&mut self) -> bool {
            next(&mut self.copy, false)
        }

        fn key_there(&mut self) -> bool {
            next(&mut self.key, false)
        }

        fn pause(&mut self) {
            self.paused += 1;
        }
    }

    #[test]
    fn a_denied_read_mid_set_up_is_read_again_until_the_record_answers() {
        let mut record = Script::new(vec![Err(Failed(true)), Err(Failed(true)), Ok(Some(7))]);
        assert_eq!(settle(&mut record), Ok(Some(7)));
        assert_eq!((record.done, record.paused), (3, 2));
    }

    #[test]
    fn a_read_still_denied_after_every_try_gives_the_denial_back() {
        let mut record = Script::new(vec![Err(Failed(true))]);
        assert_eq!(settle(&mut record), Err(Failed(true)));
        assert_eq!((record.done, record.paused), (TRIES, TRIES - 1));
    }

    #[test]
    fn a_missing_key_while_the_new_copy_is_there_is_read_again() {
        let mut record = Script::new(vec![Ok(None), Ok(Some(7))]);
        record.copy = VecDeque::from([true]);
        assert_eq!(settle(&mut record), Ok(Some(7)));
        assert_eq!(record.done, 2);
    }

    #[test]
    fn a_missing_key_renamed_into_place_before_the_look_is_read_again() {
        let mut record = Script::new(vec![Ok(None), Ok(Some(7))]);
        record.key = VecDeque::from([true]);
        assert_eq!(settle(&mut record), Ok(Some(7)));
        assert_eq!(record.done, 2);
    }

    #[test]
    fn a_missing_key_with_no_copy_is_missing_at_once() {
        let mut record = Script::new(vec![Ok(None)]);
        assert_eq!(settle(&mut record), Ok(None));
        assert_eq!((record.done, record.paused), (1, 0));
    }

    #[test]
    fn a_copy_left_for_good_gives_missing_after_every_try() {
        let mut record = Script::new(vec![Ok(None)]);
        record.copy = VecDeque::from([true]);
        assert_eq!(settle(&mut record), Ok(None));
        assert_eq!(record.done, TRIES);
    }

    #[test]
    fn an_unparsable_read_is_read_again_and_another_failure_is_not() {
        let mut record = Script::new(vec![Err(Failed(true)), Ok(Some(7))]);
        assert_eq!(settle(&mut record), Ok(Some(7)));
        let mut other = Script::new(vec![Err(Failed(false)), Ok(Some(7))]);
        assert_eq!(settle(&mut other), Err(Failed(false)));
        assert_eq!(other.done, 1);
    }
}
