//! What the fence's walks need from around the matters, on a real Windows (S15 `PR4h`): the
//! folder check, the set-aside of a child the lawyer can't open, and no credential link (the
//! set-up's fence plans none). The setup exe's step 8 and `UserSetup::forget_matter` use it.
//! Both run it acting as the owner, so the set-aside lands in the owner's records as the
//! owner's own launch would put it there.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_protocol::MatterId;

use crate::windows::admit::{FolderList, ReadDir};
use crate::windows::apply::Surroundings;
use crate::windows::{FolderSupport, SetAsideReason, folder_support, set_aside_path};

/// Around the owner's matters in `data_dir`.
#[derive(Debug, Clone)]
pub struct Around {
    data_dir: PathBuf,
}

impl Around {
    /// Around the matters whose set-asides go under `data_dir`.
    pub fn new(data_dir: PathBuf) -> Around {
        Around { data_dir }
    }
}

impl FolderList for Around {
    fn subfolders(&self, parent: &Path) -> io::Result<Vec<PathBuf>> {
        ReadDir.subfolders(parent)
    }
}

impl Surroundings for Around {
    fn check(&self, folder: &Path) -> io::Result<FolderSupport> {
        folder_support(folder)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the set-aside's timestamp folder, the one clock this walk reads"
    )]
    fn set_aside(&mut self, matter: MatterId, path: &Path) -> io::Result<()> {
        let now = SystemTime::now();
        set_aside_path(
            &self.data_dir,
            &matter,
            path,
            SetAsideReason::LawyerCantOpen,
            now,
        )
        .map(drop)
        .map_err(io::Error::other)
    }

    fn credentials(&self) -> Option<PathBuf> {
        None
    }
}
