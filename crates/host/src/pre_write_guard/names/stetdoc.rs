//! What a `stetdoc` command writes, read with its real option arity (stetdoc v0.9.1's `cli.rs`).

use super::{Held, Named, named, names_word, program_name};
use crate::pre_write_guard::shell::command_words;

/// What a `stetdoc` segment does to held files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::pre_write_guard) enum Stetdoc<'a> {
    /// Not `stetdoc`, or a form that writes no held file.
    Fine,
    /// `stetdoc mcp` or `stetdoc init`: refused in the agent's shell outright.
    Serve,
    /// A form that writes this held file, or (`None`) a file the path layer found held
    /// (`hits`).
    Writes(Option<&'a Held>),
    /// A subcommand or option the list does not know, in a command naming a held file.
    Unknown,
}

/// Each writing subcommand's options that take a value (long names; `output` is also `-o`) and
/// its flags, from stetdoc v0.9.1's `cli.rs` (`StampArgs` gives `author` and `date`; `FindArgs`
/// gives `replace` and `comment` theirs).
fn options(sub: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    Some(match sub {
        "unpack" => (&["output"], &["refresh"]),
        "pack" => (
            &["output", "author", "date"],
            &["force", "no-track", "done"],
        ),
        "edit" => (
            &[
                "ops", "output", "scope", "quotes", "offsets", "author", "date",
            ],
            &["in-place", "no-track", "manual", "manual-color", "dry-run"],
        ),
        "replace" => (
            &[
                "expect", "range", "clause", "output", "quotes", "author", "date",
            ],
            &[
                "all",
                "manual",
                "manual-color",
                "in-place",
                "dry-run",
                "ignore-case",
            ],
        ),
        "comment" => (
            &[
                "expect", "range", "clause", "output", "quotes", "author", "date",
            ],
            &["all", "in-place", "dry-run", "ignore-case"],
        ),
        "compare" => (&["output", "author", "date"], &[]),
        _ => return None,
    })
}

/// A writing subcommand's arguments, as clap reads them.
#[derive(Debug, Default)]
struct Parsed<'w> {
    positionals: Vec<&'w str>,
    outputs: Vec<&'w str>,
    in_place: bool,
    dry_run: bool,
    /// An option the subcommand does not have, a flag given a value, or a value that looks like
    /// an option (clap would refuse it; the guard does not guess).
    unknown: bool,
}

/// `args` read against `valued` and `flags` ([`options`]): `--name value`, `--name=value`,
/// `-o value`, `-ovalue`, `-o=value`; `--` ends the options; `-` is a positional (stdin).
fn parse<'w>(args: &[&'w str], valued: &[&str], flags: &[&str]) -> Parsed<'w> {
    let mut parsed = Parsed::default();
    let mut iter = args.iter().copied().peekable();
    let mut ended = false;
    while let Some(arg) = iter.next() {
        if ended || arg == "-" || !arg.starts_with('-') {
            parsed.positionals.push(arg);
            continue;
        }
        if arg == "--" {
            ended = true;
            continue;
        }
        let (name, inline) = if let Some(long) = arg.strip_prefix("--") {
            match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            }
        } else if let Some(value) = arg.strip_prefix("-o") {
            let value = value.strip_prefix('=').unwrap_or(value);
            ("output", Some(value).filter(|value| !value.is_empty()))
        } else {
            parsed.unknown = true;
            continue;
        };
        if valued.contains(&name) {
            let value = inline.or_else(|| {
                let next = iter.next_if(|next| *next == "-" || !next.starts_with('-'));
                parsed.unknown |= next.is_none();
                next
            });
            if name == "output" {
                parsed.outputs.extend(value);
            }
        } else if flags.contains(&name) && inline.is_none() {
            parsed.in_place |= name == "in-place";
            parsed.dry_run |= name == "dry-run";
        } else {
            parsed.unknown = true;
        }
    }
    parsed
}

/// What the segment `words` does if it runs `stetdoc` ([`Stetdoc`]), with `hits` the words the
/// path layer found naming a held file:
///
/// - `view`, `inspect`, `check`, `lookup`, `capabilities`, `skills`: read only, whatever the flags.
/// - `mcp`, `init`: serve or install, refused.
/// - `unpack`, `pack`, `edit`, `replace`, `comment`, `compare`, read with their real options
///   ([`options`], [`parse`]; a flag's value is never the document): an output (`-o`,
///   `--output`, any spelling) naming a held file writes it, whatever else is given; with
///   `--in-place`, any word naming a held file is written; else `--dry-run` writes nothing;
///   else `edit`, `replace` and `comment` write `NAME.edited.docx` beside their document and
///   `pack` `NAME.edited.docx` from its working copy `NAME.stet`. An option the subcommand does
///   not have is [`Stetdoc::Unknown`] when a word names a held file.
/// - Any other subcommand naming a held file is [`Stetdoc::Unknown`].
///
/// `--json` (global) may appear anywhere.
pub(in crate::pre_write_guard) fn stetdoc<'a>(
    words: &[String],
    held: &'a [Held],
    hits: &[String],
) -> Stetdoc<'a> {
    let Some((program, args)) = command_words(words).split_first() else {
        return Stetdoc::Fine;
    };
    if program_name(program) != "stetdoc" {
        return Stetdoc::Fine;
    }
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json")
        .collect();
    let Some((sub, rest)) = args.split_first() else {
        return Stetdoc::Fine;
    };
    let names = |word: &str| names_word(word, held, hits).map(Named::file);
    let names_any = args.iter().find_map(|arg| names(arg));
    let Some((valued, flags)) = options(sub) else {
        return match *sub {
            "view" | "inspect" | "check" | "lookup" | "capabilities" | "skills" => Stetdoc::Fine,
            "mcp" | "init" => Stetdoc::Serve,
            _ if names_any.is_some() => Stetdoc::Unknown,
            _ => Stetdoc::Fine,
        };
    };
    let parsed = parse(rest, valued, flags);
    if parsed.unknown && names_any.is_some() {
        return Stetdoc::Unknown;
    }
    if let Some(file) = parsed.outputs.iter().find_map(|out| names(out)) {
        return Stetdoc::Writes(file);
    }
    if parsed.in_place
        && let Some(file) = names_any
    {
        return Stetdoc::Writes(file);
    }
    if parsed.dry_run || !parsed.outputs.is_empty() || parsed.in_place {
        return Stetdoc::Fine;
    }
    let edited = |source: &str, strip: &str| {
        let last = source.rsplit(['/', '\\']).next().unwrap_or(source);
        let stem = last
            .strip_suffix(strip)
            .unwrap_or_else(|| last.rsplit_once('.').map_or(last, |(stem, _)| stem));
        format!("{stem}.edited.docx")
    };
    let default = match *sub {
        "edit" | "replace" | "comment" => parsed
            .positionals
            .first()
            .map(|source| edited(source, ".docx")),
        "pack" => parsed
            .positionals
            .first()
            .map(|dir| edited(dir.trim_end_matches(['/', '\\']), ".stet")),
        _ => None,
    };
    default
        .and_then(|out| named(&out, held))
        .map_or(Stetdoc::Fine, |file| Stetdoc::Writes(Some(file)))
}

#[cfg(test)]
mod tests;
