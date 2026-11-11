//! The bare-name rule on a Mac's rules over a fake filesystem, with a canonical form that maps
//! the shims and symlinks each test names.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clauseo_settings::mcp_import::{Env, ImportFs, Os};

use super::bare_name;

#[derive(Default)]
struct Files(BTreeSet<PathBuf>);

impl ImportFs for Files {
    fn read_to_string(&self, _: &Path, _: u64) -> Option<String> {
        None
    }

    fn is_file(&self, p: &Path) -> bool {
        self.0.contains(p)
    }

    fn list_dir(&self, _: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
}

const NVM_BIN: &str = "/Users/ana/.nvm/versions/node/v22.1.0/bin";

/// `bare_name` for `command` with `files` on disk, `path` as PATH, and `links` mapping a path
/// to its canonical form (any other path is its own; a path in `broken` has none).
fn rule(
    files: &[&str],
    path: &str,
    links: &[(&str, &str)],
    broken: &[&str],
    command: &str,
) -> Option<String> {
    let fs = Files(files.iter().map(PathBuf::from).collect());
    let path = path.to_owned();
    let vars = move |name: &str| (name == "PATH").then(|| path.clone());
    let env = Env {
        os: Os::Mac,
        home: Path::new("/Users/ana"),
        vars: &vars,
        fs: &fs,
    };
    let links: BTreeMap<PathBuf, PathBuf> = links
        .iter()
        .map(|(from, to)| (PathBuf::from(from), PathBuf::from(to)))
        .collect();
    let canonical = |p: &Path| {
        if broken.iter().any(|one| Path::new(one) == p) {
            return None;
        }
        Some(links.get(p).cloned().unwrap_or_else(|| p.to_owned()))
    };
    bare_name(&env, &canonical, Path::new(command))
}

#[test]
fn a_program_under_nvm_found_again_by_its_name_is_stored_bare() {
    let npx = format!("{NVM_BIN}/npx");
    assert_eq!(
        rule(&[&npx], NVM_BIN, &[], &[], &npx).as_deref(),
        Some("npx")
    );
}

#[test]
fn a_homebrew_link_into_the_cellar_is_stored_bare() {
    let link = "/usr/local/bin/node";
    let cellar = "/usr/local/Cellar/node/22.1.0/bin/node";
    let got = rule(&[link], "/usr/local/bin", &[(link, cellar)], &[], link);
    assert_eq!(got.as_deref(), Some("node"));
}

#[test]
fn a_program_in_a_user_folder_keeps_its_absolute_path() {
    // `~/bin` and `~/.local/bin` are user-writable and no version manager's: a bare name there
    // would follow anything put earlier on the PATH.
    for dir in ["/Users/ana/bin", "/Users/ana/.local/bin"] {
        let tool = format!("{dir}/tool");
        assert_eq!(rule(&[&tool], dir, &[], &[], &tool), None, "{dir}");
    }
}

#[test]
fn another_program_earlier_on_the_path_keeps_the_absolute_path() {
    let npx = format!("{NVM_BIN}/npx");
    let path = format!("/Users/ana/bin:{NVM_BIN}");
    let got = rule(&["/Users/ana/bin/npx", &npx], &path, &[], &[], &npx);
    assert_eq!(got, None);
}

#[test]
fn the_same_program_by_another_path_counts_as_the_same() {
    // The PATH holds a symlinked folder; both canonical forms are the nvm file.
    let npx = format!("{NVM_BIN}/npx");
    let via = "/Users/ana/.nvm/current/bin/npx";
    let got = rule(
        &[via, &npx],
        "/Users/ana/.nvm/current/bin",
        &[(via, &npx)],
        &[],
        &npx,
    );
    assert_eq!(got.as_deref(), Some("npx"));
}

#[test]
fn a_canonical_form_that_cannot_be_made_keeps_the_absolute_path() {
    let npx = format!("{NVM_BIN}/npx");
    assert_eq!(rule(&[&npx], NVM_BIN, &[], &[&npx], &npx), None);
    assert_eq!(
        rule(&[&npx], NVM_BIN, &[], &["/Users/ana/.nvm"], &npx),
        None
    );
}

#[test]
fn a_program_the_name_does_not_find_keeps_the_absolute_path() {
    let npx = format!("{NVM_BIN}/npx");
    assert_eq!(rule(&[&npx], "/usr/bin", &[], &[], &npx), None);
}
