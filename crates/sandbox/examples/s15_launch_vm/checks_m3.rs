//! The `m3-hardlink` row: M3's refusal of a credential link that reaches the lawyer's
//! `~\.codex\auth.json`.
//!
//! The real `auth.json` can't be faked unelevated, so the harness makes a stand-in profile,
//! `<root>\m3\profile\.codex\auth.json` (a fixture), and links to it from A's state folder:
//!
//! - the library's `codex_link_in_policy`, given the stand-in profile, refuses a policy whose
//!   credential link is a hard link to the fixture (`LinkedLogin`) or a symbolic link to it
//!   (`ReparseLink`);
//! - the outer runner, which compares with the real profile, runs a launch whose credential link
//!   is the hard link (it isn't the real `auth.json`), and refuses one whose link is a symbolic
//!   link (`RunExit::CodexReuse`) or lies in the real profile's `.codex` folder. That last path
//!   is never made: the outer refuses it by its text, before any file call.
//!
//! The symbolic link needs developer mode; without it that half is skipped and says so. The
//! fixture and the links are removed by full path, and recorded in the manifest until then.

use std::path::{Path, PathBuf};

use clauseo_sandbox::Policy;
use clauseo_sandbox::windows::runner::paths::CodexReuse;
use clauseo_sandbox::windows::runner::{RunExit, WinPath, codex_link_in_policy};

use super::checks::run;
use super::launch::{Spec, describe, text};
use super::manifest::Manifest;
use super::rows::Ctx;
use super::sys::{hard_link, make_dir, remove_file, remove_tree, symlink_file, write_file};

/// The fixture's text: no secret, only a marker.
const FIXTURE: &[u8] = b"{\"clauseo_s15_fixture\":\"not a login\"}\n";

/// The row's paths.
struct Paths {
    /// `<root>\m3`, the folder the row makes.
    base: PathBuf,
    /// `<root>\m3\profile`, standing in for the lawyer's profile.
    profile: PathBuf,
    /// `<profile>\.codex\auth.json`.
    auth: PathBuf,
    /// The hard link in A's state folder.
    hard: PathBuf,
    /// The symbolic link in A's state folder.
    soft: PathBuf,
}

/// The row.
pub(crate) fn m3_hardlink(ctx: &Ctx<'_>) -> Result<String, String> {
    let m = ctx.manifest;
    let base = m.root.join("m3");
    let profile = base.join("profile");
    let paths = Paths {
        auth: profile.join(".codex").join("auth.json"),
        profile,
        hard: m.state("A").join("m3-auth-hardlink.json"),
        soft: m.state("A").join("m3-auth-symlink.json"),
        base,
    };
    for path in [&paths.base, &paths.hard, &paths.soft] {
        if path.symlink_metadata().is_ok() {
            return Err(format!(
                "{} exists from an earlier run: run cleanup",
                path.display()
            ));
        }
    }
    Manifest::update(&m.root, |on_disk| {
        on_disk.dirs.push(paths.base.clone());
        on_disk.files.push(paths.hard.clone());
        on_disk.files.push(paths.soft.clone());
    })?;
    let result = checks(ctx, &paths);
    let cleaned = clean(ctx, &paths);
    match (result, cleaned) {
        (Ok(note), Ok(())) => Ok(note),
        (Ok(_), Err(why)) | (Err(why), Ok(())) => Err(why),
        (Err(why), Err(also)) => Err(format!("{why}; and {also}")),
    }
}

fn checks(ctx: &Ctx<'_>, paths: &Paths) -> Result<String, String> {
    let m = ctx.manifest;
    let codex = paths.auth.parent().ok_or("auth.json has no folder")?;
    make_dir(codex).map_err(|e| format!("making {}: {e}", codex.display()))?;
    write_file(&paths.auth, FIXTURE).map_err(|e| format!("writing the fixture: {e}"))?;
    hard_link(&paths.auth, &paths.hard).map_err(|e| format!("the hard link: {e}"))?;
    ctx.note(&format!(
        "fixture {}; hard link {}",
        paths.auth.display(),
        paths.hard.display()
    ));
    let profile = WinPath::parse(&text(&paths.profile))
        .ok_or_else(|| format!("{} isn't a full local path", paths.profile.display()))?;
    let mut notes = Vec::new();

    let verdict = library(m, &paths.hard, &profile)?;
    if verdict != Err(CodexReuse::LinkedLogin) {
        return Err(format!(
            "codex_link_in_policy on the hard link gave {verdict:?}, not LinkedLogin"
        ));
    }
    notes.push("codex_link_in_policy refused the hard link (LinkedLogin)".to_owned());

    let done = run(ctx, "A", &link_spec(m, &paths.hard))?;
    if done.code != 0 {
        return Err(format!(
            "the outer refused a hard link to a fixture that isn't the real auth.json: exit {}",
            describe(done.code)
        ));
    }
    notes.push(
        "the outer ran the hard link to the fixture (its file-ID check compares with the real \
         auth.json)"
            .to_owned(),
    );

    if let Err(error) = symlink_file(&paths.auth, &paths.soft) {
        ctx.note(&format!(
            "SKIP the symbolic link half: CreateSymbolicLinkW failed ({error}): developer mode \
             is off"
        ));
        notes.push("symbolic link skipped (no developer mode)".to_owned());
    } else {
        let verdict = library(m, &paths.soft, &profile)?;
        if verdict != Err(CodexReuse::ReparseLink) {
            return Err(format!(
                "codex_link_in_policy on the symbolic link gave {verdict:?}, not ReparseLink"
            ));
        }
        refused(ctx, &paths.soft, "the symbolic link")?;
        notes.push("the symbolic link: refused by the library and the outer".to_owned());
    }

    let home = std::env::home_dir().ok_or("no home folder")?;
    // Never made: the outer refuses it by its text before it opens anything.
    let in_codex = home.join(".codex").join("clauseo-s15-never-made.json");
    if in_codex.symlink_metadata().is_ok() {
        return Err(format!("{} exists; it must not", in_codex.display()));
    }
    refused(ctx, &in_codex, "a link in the real profile's .codex")?;
    notes.push(format!(
        "a link in {} (never made): refused",
        home.join(".codex").display()
    ));
    Ok(notes.join("; "))
}

/// `codex_link_in_policy` on a policy for A whose credential link is `link`, with `profile`.
fn library(m: &Manifest, link: &Path, profile: &WinPath) -> Result<Result<(), CodexReuse>, String> {
    let policy = Policy::builder(m.matter("A"), m.state("A"))
        .credential_link(link)
        .build()
        .map_err(|e| {
            format!(
                "the policy with the credential link {}: {e}",
                link.display()
            )
        })?;
    Ok(codex_link_in_policy(&policy, profile))
}

/// `probe exit 0` as A with the credential link `link`.
fn link_spec(m: &Manifest, link: &Path) -> Spec {
    Spec {
        credential_link: Some(link.to_path_buf()),
        ..Spec::probe(m, &["exit", "0"])
    }
}

/// Runs a launch with the credential link `link` and requires the outer's M3 refusal.
fn refused(ctx: &Ctx<'_>, link: &Path, what: &str) -> Result<(), String> {
    let done = run(ctx, "A", &link_spec(ctx.manifest, link))?;
    let want = RunExit::CodexReuse.code();
    if done.code != want {
        return Err(format!(
            "the outer ran {what} ({}): exit {}, not {}",
            link.display(),
            describe(done.code),
            describe(want)
        ));
    }
    ctx.note(&format!("the outer refused {what}: {:?}", done.stderr));
    Ok(())
}

/// Removes the links, the fixture and its folder, each by full path, and drops them from the
/// manifest once gone.
fn clean(ctx: &Ctx<'_>, paths: &Paths) -> Result<(), String> {
    let mut left = Vec::new();
    for file in [&paths.soft, &paths.hard, &paths.auth] {
        match remove_file(file) {
            Ok(()) => ctx.note(&format!("removed {}", file.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => left.push(format!("{}: {e}", file.display())),
        }
    }
    match remove_tree(&paths.base) {
        Ok(()) => ctx.note(&format!("removed {}", paths.base.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => left.push(format!("{}: {e}", paths.base.display())),
    }
    if !left.is_empty() {
        return Err(format!("left behind (cleanup tries again): {left:?}"));
    }
    Manifest::update(&ctx.manifest.root, |on_disk| {
        on_disk.dirs.retain(|d| *d != paths.base);
        on_disk
            .files
            .retain(|f| *f != paths.hard && *f != paths.soft);
    })
}
