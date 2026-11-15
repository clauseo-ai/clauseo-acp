//! The registry check's code for each claim and nesting, the matter it names, and what it lets
//! through for an import ([`check_import`]).

use std::path::{Path, PathBuf};

use clauseo_protocol::{CommandId, ErrorCode, MatterId};
use uuid::Uuid;

use super::{Claim, Claimed, Held, check_import, check_registry};
use crate::error::Error;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A valid v7 id, distinct per `n`.
fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n)
}

fn claimed(n: u128, folder: &Path, claim: Claim, command: Option<CommandId>) -> Claimed {
    Claimed {
        matter: MatterId::new(id(n)).unwrap(),
        folder: folder.to_owned(),
        claim,
        command,
    }
}

/// The refusal's code and matter, or a panic when `result` is not a refusal.
fn refusal<T: std::fmt::Debug>(result: Result<T, Error>) -> (ErrorCode, Option<MatterId>) {
    match result {
        Err(Error::Refused(error)) => (error.code(), error.matter()),
        other => panic!("not a refusal: {other:?}"),
    }
}

/// `root/a`, `root/a/sub` and `root` itself, made on disk.
fn folders(dir: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
    let root = dir.path().join("work");
    let folder = root.join("a");
    let inside = folder.join("sub");
    write::create_dir_all(&inside).unwrap();
    (root, folder, inside)
}

#[test]
fn a_listed_matters_folder_is_named_by_its_nesting() {
    let dir = TempDir::new("claims-listed");
    let (root, folder, inside) = folders(&dir);
    let registry = [claimed(1, &folder, Claim::Listed, None)];
    let matter = Some(registry[0].matter);
    for (path, code) in [
        (&inside, ErrorCode::MatterFolderInsideMatter),
        (&root, ErrorCode::MatterFolderHoldsMatter),
        (&folder, ErrorCode::MatterFolderIsMatter),
    ] {
        let result = check_registry(path, &registry, false);
        assert_eq!(refusal(result), (code, matter), "{}", path.display());
    }
    // An import of the same folder is answered by the actor, which refuses it naming the
    // matter; off the actor (the folder query, the scan) it is refused at once.
    assert_eq!(
        check_registry(&folder, &registry, true).unwrap(),
        Held::Listed(registry[0].matter)
    );
    assert_eq!(
        refusal(check_import(&folder, &registry)),
        (ErrorCode::MatterFolderIsMatter, matter)
    );
}

#[test]
fn a_folder_being_made_and_a_forgotten_matters_have_codes_of_their_own() {
    let dir = TempDir::new("claims-making");
    let (root, folder, inside) = folders(&dir);
    let command = CommandId::new(id(9)).unwrap();
    for (claim, command, code) in [
        (
            Claim::Making,
            Some(command),
            ErrorCode::MatterFolderBeingMade,
        ),
        // A relocate's target: no command to join.
        (Claim::Making, None, ErrorCode::MatterFolderBeingMade),
        (
            Claim::Forgotten,
            None,
            ErrorCode::MatterFolderOfForgottenMatter,
        ),
    ] {
        let registry = [claimed(2, &folder, claim, command)];
        for path in [&inside, &root] {
            for import in [false, true] {
                let result = check_registry(path, &registry, import);
                let what = format!("{claim:?} {} import {import}", path.display());
                assert_eq!(refusal(result), (code, None), "{what}");
            }
        }
    }
    // The same folder: one being made is joined, a forgotten matter's own folder brings it back
    // on import; a relocate's target and, for a new folder, a forgotten matter's are refused.
    let making = [claimed(2, &folder, Claim::Making, Some(command))];
    assert_eq!(
        check_registry(&folder, &making, true).unwrap(),
        Held::Making(command)
    );
    let forgotten = [claimed(3, &folder, Claim::Forgotten, None)];
    assert_eq!(
        check_registry(&folder, &forgotten, true).unwrap(),
        Held::Free
    );
    assert_eq!(
        refusal(check_registry(&folder, &forgotten, false)),
        (ErrorCode::MatterFolderOfForgottenMatter, None)
    );
    let target = [claimed(4, &folder, Claim::Making, None)];
    assert_eq!(
        refusal(check_registry(&folder, &target, true)),
        (ErrorCode::MatterFolderBeingMade, None)
    );
}
