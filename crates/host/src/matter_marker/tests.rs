//! `check_matter_folder`: each finding's code, and no path in the message.

use std::path::Path;

use clauseo_protocol::{ErrorCode, MatterId};

use super::check_matter_folder;
use crate::pipeline::tests::support::TempDir;
use crate::write;

fn matter(n: u128) -> MatterId {
    MatterId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
    .unwrap()
}

fn marker(folder: &Path, text: &str) {
    write::create_dir_all(&folder.join(".clauseo")).unwrap();
    write::write_file(&folder.join(".clauseo/matter.json"), text.as_bytes()).unwrap();
}

fn naming(id: MatterId) -> String {
    format!(r#"{{"version":1,"matter_id":"{id}","created_at":0}}"#)
}

#[test]
fn each_finding_gives_its_code_and_no_path() {
    let dir = TempDir::new("matter-marker");
    let folder = dir.path().join("Acme");
    let code = |folder: &Path| check_matter_folder(folder, matter(1)).map_err(|e| e.code());

    assert_eq!(code(&folder), Err(ErrorCode::MatterFolderMissing));
    write::create_dir_all(&folder).unwrap();
    let refused = check_matter_folder(&folder, matter(1)).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::MatterFolderNotAMatter);
    assert!(!refused.retryable());
    assert!(!refused.message().contains("Acme"), "{}", refused.message());
    marker(&folder, "{oops");
    assert_eq!(code(&folder), Err(ErrorCode::MatterFolderNotAMatter));
    marker(&folder, &naming(matter(2)));
    assert_eq!(code(&folder), Err(ErrorCode::MatterFolderNotAMatter));
    marker(&folder, &naming(matter(1)));
    assert_eq!(code(&folder), Ok(()));
}
