use std::path::Path;

use super::warnings;
use crate::{ImportWarning, Limits};

#[test]
fn warnings_in_variant_order() {
    let limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 100,
    };
    assert_eq!(warnings(Path::new("/m"), 100, &limits), []);
    assert_eq!(
        warnings(Path::new("/m"), 101, &limits),
        [ImportWarning::OverThreshold { bytes: 101 }]
    );
    assert_eq!(
        warnings(Path::new(r"\\server\share\m"), 101, &limits),
        [
            ImportWarning::UncShare,
            ImportWarning::OverThreshold { bytes: 101 }
        ]
    );
}
