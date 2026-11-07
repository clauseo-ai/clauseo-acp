//! The rows on the runner job's UI limits: `clipboard` and `atoms`, and the check the
//! `whoami`, `node-pipes` and `codex` rows make of a probe's `ui-limits` line.

use super::launch::{Spec, deadline, launch, summary};
use super::manifest::Manifest;
use super::rows::Ctx;
use super::ui;
use super::win::UI_LIMITS;

/// A random hex text, for the clipboard's text and the atom's name.
fn random() -> Result<String, String> {
    let mut bits = [0u8; 8];
    getrandom::fill(&mut bits).map_err(|e| format!("getrandom: {e}"))?;
    Ok(bits.iter().fold(String::new(), |mut text, b| {
        text.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
        text
    }))
}

/// Checks the UI limits bind the probe that printed `lines`: its `ui-limits <hex>` line holds
/// every limit in [`UI_LIMITS`] (it sits in the runner's job itself), or, when that innermost job
/// is another's (libuv's, inside the runner's), its `ui-effect clipboard-write refused` line
/// shows the runner's limits still apply.
pub(crate) fn ui_limits(lines: &[String]) -> Result<String, String> {
    let refused = lines
        .iter()
        .any(|l| l.starts_with("ui-effect clipboard-write refused"));
    match direct_limits(lines) {
        Ok(found) => Ok(found),
        Err(_) if refused => Ok(
            "the innermost job is another's (libuv's), and its clipboard write \
                                was refused: the runner's UI limits bind it"
                .to_owned(),
        ),
        Err(why) => Err(format!("{why}, and its clipboard write wasn't refused")),
    }
}

/// [`ui_limits`]'s first test: the innermost job's own limits.
fn direct_limits(lines: &[String]) -> Result<String, String> {
    let line = lines
        .iter()
        .find_map(|l| l.strip_prefix("ui-limits "))
        .ok_or_else(|| format!("no ui-limits line in {lines:?}"))?;
    let limits = u32::from_str_radix(line.trim().trim_start_matches("0x"), 16)
        .map_err(|_not_hex| format!("the job's UI limits weren't read: ui-limits {line}"))?;
    if limits & UI_LIMITS != UI_LIMITS {
        return Err(format!(
            "the job's UI limits are {limits:#x}; {UI_LIMITS:#x} must all be set (missing {:#x})",
            UI_LIMITS & !limits
        ));
    }
    Ok(format!(
        "the job's UI limits {limits:#x} hold {UI_LIMITS:#x}"
    ))
}

/// Runs probe `args` as A to its end; its stdout.
fn probe(ctx: &Ctx<'_>, args: &[&str]) -> Result<Vec<String>, String> {
    let done =
        launch(ctx.manifest, "A", &Spec::probe(ctx.manifest, args))?.finish(deadline(ctx.limit))?;
    ctx.note(&format!("A: probe {}: {}", args.join(" "), summary(&done)));
    for line in done.stdout.iter().chain(&done.stderr) {
        ctx.note(&format!("A: {line}"));
    }
    Ok(done.stdout)
}

/// The `clipboard` row: A can't read the harness's clipboard text, nor replace it. `Ok(None)`
/// when this session has no clipboard the harness can use (the row's SKIP).
pub(crate) fn clipboard(ctx: &Ctx<'_>) -> Result<Option<String>, String> {
    let text = format!("ClauseoS15-clip-{}", random()?);
    if let Err(why) = ui::write_text(&text) {
        ctx.note(&format!("the harness can't write the clipboard: {why}"));
        return Ok(None);
    }
    match ui::read_text() {
        Ok(read) if read == text => {}
        Ok(read) => {
            ctx.note(&format!(
                "the clipboard gave back {read:?}, not the harness's text"
            ));
            return Ok(None);
        }
        Err(why) => {
            ctx.note(&format!("the harness can't read the clipboard back: {why}"));
            return Ok(None);
        }
    }
    let read = probe(ctx, &["clipboard-read"])?;
    let write = probe(ctx, &["clipboard-write", "ClauseoS15-from-A"])?;
    let after = ui::read_text();
    let read_line = read.iter().find(|l| l.starts_with("clipboard-read "));
    if !read_line.is_some_and(|l| l.starts_with("clipboard-read error")) {
        return Err(format!("A's clipboard read didn't fail: {read_line:?}"));
    }
    let write_line = write.iter().find(|l| l.starts_with("clipboard-write "));
    if !write_line.is_some_and(|l| l.starts_with("clipboard-write error")) {
        return Err(format!("A's clipboard write didn't fail: {write_line:?}"));
    }
    match after {
        Ok(after) if after == text => {}
        Ok(after) => return Err(format!("the harness's clipboard now holds {after:?}")),
        Err(why) => {
            return Err(format!(
                "the harness's clipboard can't be read after: {why}"
            ));
        }
    }
    Ok(Some(format!(
        "A's read ({}) and write ({}) failed; the harness's text stayed",
        read_line.map_or("", |l| l.trim_start_matches("clipboard-read error ")),
        write_line.map_or("", |l| l.trim_start_matches("clipboard-write error "))
    )))
}

/// The `atoms` row: A doesn't find the harness's global atom.
pub(crate) fn atoms(ctx: &Ctx<'_>) -> Result<String, String> {
    let name = format!("ClauseoS15-{}", random()?);
    Manifest::update(&ctx.manifest.root, |m| m.atoms.push(name.clone()))?;
    let result = atoms_with(ctx, &name);
    let atom = ui::find_atom(&name);
    let deleted = atom != 0 && ui::delete_atom(atom);
    let gone = ui::find_atom(&name) == 0;
    ctx.note(&format!(
        "deleted the atom {name}: {deleted}; none left: {gone}"
    ));
    if gone {
        Manifest::update(&ctx.manifest.root, |m| m.atoms.retain(|a| *a != name))?;
    }
    result
}

fn atoms_with(ctx: &Ctx<'_>, name: &str) -> Result<String, String> {
    let atom = ui::add_atom(name)?;
    if ui::find_atom(name) != atom {
        return Err(format!("the harness can't find its own atom {name}"));
    }
    ctx.note(&format!("added the global atom {name} ({atom:#x})"));
    let found = probe(ctx, &["find-atom", name])?;
    let answer: Vec<&str> = found
        .iter()
        .filter_map(|l| l.strip_prefix("atom "))
        .collect();
    if answer != ["0"] {
        return Err(format!("A's GlobalFindAtomW gave {answer:?}, not 0"));
    }
    Ok(format!("A's GlobalFindAtomW didn't find {name}"))
}
