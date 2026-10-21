//! Pins the text of every `file_size::Fix` variant with `fix_texts!`: one arm per variant, and
//! `assert_texts` fails until each arm has a sample, so a new variant needs both.

use super::Fix;
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::SplitOrExempt => {
            "split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a \
             reason"
        },
        Fix::WrapComment => "wrap the comment at 100 columns",
        Fix::DeleteException => "delete this line from xtask/file-size-exceptions.txt",
    }
}

#[test]
fn every_fix_prints_its_pinned_text() {
    let samples = [Fix::SplitOrExempt, Fix::WrapComment, Fix::DeleteException];
    assert_texts(&samples, pinned, ARMS);
}
