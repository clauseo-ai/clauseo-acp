//! Pins the text of each `Fix` variant, for every check's `fix/tests.rs`. `fix_texts!` writes one
//! exhaustive `match` whose arms the macro numbers in order, and `assert_texts` requires one
//! sample per arm, each printing the text its arm gives. A new variant fails to compile until it
//! has an arm; after that the test fails, naming the arm, until the arm has a sample.

use std::fmt;

/// Writes `fn NAME(fix: &TYPE) -> (usize, String)`: a `match` with one arm per variant, which
/// returns the arm's index beside its expected text. It also writes `const ARMS`, the pattern of
/// each arm as text, so `ARMS.len()` is the count of arms. Write each arm as `Type::Variant`, with
/// any fields: `assert_texts` fails on an arm that is not a path, such as `_` or a binding.
macro_rules! fix_texts {
    (fn $name:ident for $ty:ty { $($arms:tt)* }) => {
        fix_texts!(@arm $name $ty [] [] [] ; $($arms)*);
    };
    // An arm with a comma after it. `$mark` holds one `()` for each arm before it, so the count
    // of `$mark` is this arm's index.
    (@arm $name:ident $ty:ty [$($done:tt)*] [$($mark:tt)*] [$($names:tt)*] ;
        $pat:pat_param => $text:expr, $($rest:tt)*) => {
        fix_texts!(@arm $name $ty
            [$($done)* $pat => (<[()]>::len(&[$($mark)*]), String::from($text)),]
            [$($mark)* (),]
            [$($names)* stringify!($pat),]
            ; $($rest)*);
    };
    // The last arm, with or without its comma.
    (@arm $name:ident $ty:ty [$($done:tt)*] [$($mark:tt)*] [$($names:tt)*] ;
        $pat:pat_param => $text:expr $(,)?) => {
        fix_texts!(@arm $name $ty
            [$($done)* $pat => (<[()]>::len(&[$($mark)*]), String::from($text)),]
            [$($mark)* (),]
            [$($names)* stringify!($pat),]
            ; );
    };
    (@arm $name:ident $ty:ty [$($done:tt)*] [$($mark:tt)*] [$($names:tt)*] ; ) => {
        fn $name(fix: &$ty) -> (usize, String) {
            match fix {
                $($done)*
            }
        }
        const ARMS: &[&str] = &[$($names)*];
    };
}
pub(crate) use fix_texts;

/// Checks `samples` against the arms of a `fix_texts!` function, `text`, whose patterns are
/// `arms`. Each arm must name a variant by its path, must have exactly one sample, and each
/// sample must print the text its arm gives.
pub(crate) fn assert_texts<F: fmt::Display>(
    samples: &[F],
    text: fn(&F) -> (usize, String),
    arms: &[&str],
) {
    for arm in arms {
        assert!(
            arm.contains("::"),
            "arm `{arm}` names no variant by its path: write the arm as `Type::Variant`"
        );
    }
    let mut count = vec![0_usize; arms.len()];
    for sample in samples {
        let (index, expected) = text(sample);
        assert!(
            index < arms.len(),
            "{sample} is past the last arm, {}",
            arms.len()
        );
        assert_eq!(
            sample.to_string(),
            expected,
            "arm {index} (`{}`) prints the wrong text",
            arms[index]
        );
        count[index] += 1;
    }
    for (index, (arm, count)) in arms.iter().zip(count).enumerate() {
        match count {
            1 => {}
            0 => panic!(
                "arm {index} (`{arm}`) has no sample, so its text is never checked: add a \
                 sample of that variant"
            ),
            _ => panic!("arm {index} (`{arm}`) has {count} samples: each arm needs exactly one"),
        }
    }
}

/// A two-variant enum, with the macro and the assertion run on it, to pin their failures.
#[derive(Debug)]
enum Toy {
    Alpha,
    Beta { n: u8 },
}

impl fmt::Display for Toy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Toy::Alpha => f.write_str("alpha"),
            Toy::Beta { n } => write!(f, "beta {n}"),
        }
    }
}

fix_texts! {
    fn toy_text for Toy {
        Toy::Alpha => "alpha",
        Toy::Beta { n } => format!("beta {n}"),
    }
}

#[test]
fn a_sample_of_every_arm_prints_its_text() {
    assert_texts(&[Toy::Alpha, Toy::Beta { n: 2 }], toy_text, ARMS);
}

#[test]
#[should_panic(expected = "arm 1 (`Toy")]
fn an_arm_with_no_sample_fails_and_names_it() {
    assert_texts(&[Toy::Alpha], toy_text, ARMS);
}

#[test]
#[should_panic(expected = "arm 0 (`Toy::Alpha`) has 2 samples")]
fn two_samples_of_one_arm_fail() {
    assert_texts(
        &[Toy::Alpha, Toy::Alpha, Toy::Beta { n: 1 }],
        toy_text,
        ARMS,
    );
}

#[test]
#[should_panic(expected = "arm 0 (`Toy::Alpha`) prints the wrong text")]
fn a_sample_that_prints_another_text_fails() {
    fix_texts! {
        fn wrong_text for Toy {
            Toy::Alpha => "ALPHA",
            Toy::Beta { n } => format!("beta {n}"),
        }
    }
    assert_texts(&[Toy::Alpha, Toy::Beta { n: 2 }], wrong_text, ARMS);
}

#[test]
#[should_panic(expected = "names no variant by its path")]
fn an_arm_that_is_not_a_variant_path_fails() {
    fix_texts! {
        fn catch_all for Toy {
            _ => "any",
        }
    }
    assert_texts(&[Toy::Alpha], catch_all, ARMS);
}
