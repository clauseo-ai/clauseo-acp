//! What a delete or a move takes from the matter: the paths `rm`, `unlink` and `mv` (both
//! sides) name, and PowerShell's `Remove-Item` and `Move-Item` with their aliases (`del`,
//! `erase`, `ri`, `rd`, `mi`, `move`; `-Path`, `-LiteralPath` and `-Destination` values
//! included). A file a turn's shell deletes or moves away is the agent's change, as one it writes
//! is (`clauseo_matters::Matter::after_turn_named` credits a deleted or renamed file named here,
//! or under a folder named here). One reader, so whatever else judges a shell delete reads the
//! same forms.
//!
//! Options are left out (`-rf`, `-Force`, `-Recurse`), and so is the value of an option that
//! takes a pattern or a setting rather than a path (`-Filter`, `-Include`, `-Exclude`,
//! `-Credential`, `-Stream`; `mv -S`/`--suffix`); after `--` every word is a path. A target
//! that is unclear (an expansion, a glob) is the caller's to drop, as for any write.

/// The deleting programs, lower case, without `.exe`: every path they name goes.
const DELETERS: [&str; 7] = ["rm", "unlink", "remove-item", "del", "erase", "ri", "rd"];

/// The moving programs: every path they name, source or destination, changes.
const MOVERS: [&str; 4] = ["mv", "move-item", "mi", "move"];

/// Options whose value is no path.
const VALUED: [&str; 7] = [
    "-filter",
    "-include",
    "-exclude",
    "-credential",
    "-stream",
    "-s",
    "--suffix",
];

/// The indexes in `args` of the paths `program` (as written: a folder, any case, `.exe` or not)
/// deletes or moves; empty for any other program.
pub(crate) fn removed<W: AsRef<str>>(program: &str, args: &[W]) -> Vec<usize> {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    if !DELETERS.contains(&name) && !MOVERS.contains(&name) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut options = true;
    let mut skip = false;
    for (i, arg) in args.iter().enumerate() {
        let arg = arg.as_ref();
        if std::mem::take(&mut skip) {
            continue;
        }
        if options && arg == "--" {
            options = false;
        } else if options && arg.starts_with('-') && arg.len() > 1 {
            skip = VALUED.contains(&arg.to_ascii_lowercase().as_str());
        } else {
            out.push(i);
        }
    }
    out
}
