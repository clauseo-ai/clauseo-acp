//! The rows on the runner job's UI limits: `clipboard` and `atoms`, and the check the
//! `whoami`, `node-pipes` and `codex` rows make of a probe's `ui-limits` and `ui-effect` lines
//! ([`ui_limits`]), which runs the harness's positive control (`checks_ctl.rs`) and the per-effect
//! gate (`gate.rs`), and the atom information.

use super::checks_ctl::{Live, controls};
use super::gate::Kind;
use super::launch::{Spec, deadline, launch, summary};
use super::manifest::Manifest;
use super::rows::Ctx;
use super::ui;
use super::win::UI_LIMITS;

/// A random hex text, for the clipboard's text and the atom's name.
pub(crate) fn random() -> Result<String, String> {
    let mut bits = [0u8; 8];
    getrandom::fill(&mut bits).map_err(|e| format!("getrandom: {e}"))?;
    Ok(bits.iter().fold(String::new(), |mut text, b| {
        text.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
        text
    }))
}

/// Checks the UI limits bind the probe that printed `lines` (`probe job-ui`'s, or
/// `probe token`'s): a direct read of exactly [`UI_LIMITS`] passes alone (not in `strict`
/// mode); a read of exactly `0x0` (libuv's nested job), or any of the two in `strict` mode, runs
/// the positive control (`checks_ctl.rs`) and `gate.rs` decides on the effects. A gate of
/// INFO calls [`Ctx::info`]; FAIL, or any other read, is the `Err`.
pub(crate) fn ui_limits(
    ctx: &Ctx<'_>,
    lines: &[String],
    live: Live<'_>,
    strict: bool,
) -> Result<String, String> {
    let limits = read_limits(lines)?;
    if limits == UI_LIMITS && !strict {
        return Ok(format!("the job's UI limits read exactly {UI_LIMITS:#x}"));
    }
    if limits != UI_LIMITS && limits != 0 {
        return Err(format!(
            "the job's UI limits read {limits:#x}, not exactly {UI_LIMITS:#x} (nor 0x0, \
             libuv's nested job)"
        ));
    }
    let judged = controls(ctx, lines, live, strict);
    let text = format!(
        "the innermost job reads {limits:#x}; the effects, per effect: {}",
        judged.text
    );
    match judged.kind {
        Kind::Pass => Ok(text),
        Kind::Fail => Err(text),
        Kind::Info => {
            ctx.info(&format!("no effect is discriminating: {text}"));
            Ok(text)
        }
    }
}

/// The `ui-limits <hex>` line's value; why when there is none or it isn't hex.
fn read_limits(lines: &[String]) -> Result<u32, String> {
    let line = lines
        .iter()
        .find_map(|l| l.strip_prefix("ui-limits "))
        .ok_or_else(|| format!("no ui-limits line in {lines:?}"))?;
    u32::from_str_radix(line.trim().trim_start_matches("0x"), 16)
        .map_err(|_not_hex| format!("the job's UI limits weren't read: ui-limits {line}"))
}

/// Adds a global atom as the lawyer (the harness), runs `body` with its name, then deletes it;
/// the manifest lists it meanwhile, so cleanup deletes one a crash left.
pub(crate) fn with_atom<T>(
    ctx: &Ctx<'_>,
    body: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, String> {
    let name = format!("ClauseoS15-{}", random()?);
    Manifest::update(&ctx.manifest.root, |m| m.atoms.push(name.clone()))?;
    let result = ui::add_atom(&name).and_then(|atom| {
        if ui::find_atom(&name) != atom {
            return Err(format!("the harness can't find its own atom {name}"));
        }
        ctx.note(&format!("added the global atom {name} ({atom:#x})"));
        body(&name)
    });
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

/// Notes, as information, the agent's lookup of the harness's atom `name` (its
/// `ui-info atom` line) and the same lookup outside the job; never the pass criterion.
pub(crate) fn atom_info(ctx: &Ctx<'_>, lines: &[String], name: &str) {
    let agent = lines
        .iter()
        .find_map(|l| l.strip_prefix(&format!("ui-info atom {name} ")))
        .unwrap_or("none printed");
    let outside = ui::find_atom(name);
    ctx.note(&format!(
        "information, not the pass criterion: the agent's GlobalFindAtomW({name}) gave {agent}; \
         the harness's outside the job gave {outside:#x} (the positive control for its own atom). \
         Atoms are per window station, so the agent's own ClauseoRun- station hides the lawyer's \
         atom whatever the job limits are; the desktop-create refusal is the discriminating effect"
    ));
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
    with_atom(ctx, |name| {
        let found = probe(ctx, &["find-atom", name])?;
        let answer: Vec<&str> = found
            .iter()
            .filter_map(|l| l.strip_prefix("atom "))
            .collect();
        if answer != ["0"] {
            return Err(format!("A's GlobalFindAtomW gave {answer:?}, not 0"));
        }
        Ok(format!("A's GlobalFindAtomW didn't find {name}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{UI_LIMITS, read_limits};

    #[test]
    fn the_limits_line_reads_as_hex() {
        let exact = vec![format!("ui-limits {UI_LIMITS:#x}")];
        assert_eq!(read_limits(&exact), Ok(UI_LIMITS));
        assert_eq!(read_limits(&["ui-limits 0x0".to_owned()]), Ok(0));
        assert!(read_limits(&["ui-limits error 5".to_owned()]).is_err());
        assert!(read_limits(&[]).is_err());
    }
}
