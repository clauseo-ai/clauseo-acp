//! What a `stetdoc` command writes to held files, its arguments read by the reader the turn's
//! shell writes share ([`crate::stetdoc_args`]).

use super::{Held, Named, named, names_word, program_name};
use crate::pre_write_guard::shell::command_words;
#[cfg(test)]
use crate::stetdoc_args::parse;
use crate::stetdoc_args::{Form, args_of, form};

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

/// What the segment `words` does if it runs `stetdoc` ([`Stetdoc`]), with `hits` the words the
/// path layer found naming a held file:
///
/// - `view`, `inspect`, `check`, `lookup`, `capabilities`, `skills`: read only, whatever the flags.
/// - `mcp`, `init`: serve or install, refused.
/// - `unpack`, `pack`, `edit`, `replace`, `comment`, `compare`, read with their real options
///   ([`form`]; a flag's value is never the document): an output (`-o`,
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
    let args = args_of(args);
    let names = |word: &str| names_word(word, held, hits).map(Named::file);
    let names_any = args.iter().find_map(|arg| names(arg));
    let (sub, parsed) = match form(&args) {
        Form::Serves => return Stetdoc::Serve,
        Form::Unknown if names_any.is_some() => return Stetdoc::Unknown,
        Form::Reads | Form::Unknown => return Stetdoc::Fine,
        Form::Writing { sub, parsed } => (sub, parsed),
    };
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
    let default = parsed.default_output(sub).map(|(_, name)| name);
    default
        .and_then(|out| named(&out, held))
        .map_or(Stetdoc::Fine, |file| Stetdoc::Writes(Some(file)))
}

#[cfg(test)]
mod tests;
