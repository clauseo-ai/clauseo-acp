//! A `stetdoc` command's arguments, read with stetdoc v0.9.1's options and their arity (its
//! `cli.rs`), so a flag's value is never taken for the document. Both the pre-write guard
//! (`pre_write_guard::names::stetdoc`, which refuses a write to a held file) and the turn's
//! shell writes ([`crate::shell_writes`], which tell the agent's own Word edits from the
//! lawyer's) read a command through [`form`]; [`written`] gives the files a command writes.

/// What a `stetdoc` command's subcommand does ([`form`]).
#[derive(Debug)]
pub(crate) enum Form<'w> {
    /// No subcommand, or one that only reads: `view`, `inspect`, `check`, `lookup`,
    /// `capabilities`, `skills`.
    Reads,
    /// `mcp` or `init`: serve or install.
    Serves,
    /// A subcommand the list does not know.
    Unknown,
    /// A writing subcommand (`unpack`, `pack`, `edit`, `replace`, `comment`, `compare`) and
    /// its arguments.
    Writing { sub: &'w str, parsed: Parsed<'w> },
}

/// A writing subcommand's arguments, as clap reads them.
#[derive(Debug, Default)]
pub(crate) struct Parsed<'w> {
    pub(crate) positionals: Vec<&'w str>,
    pub(crate) outputs: Vec<&'w str>,
    pub(crate) in_place: bool,
    pub(crate) dry_run: bool,
    /// An option the subcommand does not have, a flag given a value, or a value that looks like
    /// an option (clap would refuse it; no reader guesses).
    pub(crate) unknown: bool,
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

/// `args` read against `valued` and `flags` ([`options`]): `--name value`, `--name=value`,
/// `-o value`, `-ovalue`, `-o=value`; `--` ends the options; `-` is a positional (stdin).
pub(crate) fn parse<'w>(args: &[&'w str], valued: &[&str], flags: &[&str]) -> Parsed<'w> {
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

/// The arguments after `stetdoc` without the global `--json`, which may appear anywhere.
pub(crate) fn args_of(words: &[String]) -> Vec<&str> {
    words
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--json")
        .collect()
}

/// What the subcommand of `args` ([`args_of`]) does, a writing one's arguments read with its
/// real options ([`options`], [`parse`]).
pub(crate) fn form<'w>(args: &[&'w str]) -> Form<'w> {
    let Some((sub, rest)) = args.split_first() else {
        return Form::Reads;
    };
    match options(sub) {
        Some((valued, flags)) => Form::Writing {
            sub,
            parsed: parse(rest, valued, flags),
        },
        None => match *sub {
            "view" | "inspect" | "check" | "lookup" | "capabilities" | "skills" => Form::Reads,
            "mcp" | "init" => Form::Serves,
            _ => Form::Unknown,
        },
    }
}

impl<'w> Parsed<'w> {
    /// The default output of `sub` when no output, `--in-place` or `--dry-run` is given:
    /// `edit`, `replace` and `comment` write `NAME.edited.docx` beside their document, and
    /// `pack` writes `NAME.edited.docx` beside its working copy `NAME.stet`. The source as
    /// given (a `pack` folder's trailing separators off) and the output's file name.
    pub(crate) fn default_output(&self, sub: &str) -> Option<(&'w str, String)> {
        let edited = |source: &str, strip: &str| {
            let last = source.rsplit(['/', '\\']).next().unwrap_or(source);
            let stem = last
                .strip_suffix(strip)
                .unwrap_or_else(|| last.rsplit_once('.').map_or(last, |(stem, _)| stem));
            format!("{stem}.edited.docx")
        };
        let source = *self.positionals.first()?;
        match sub {
            "edit" | "replace" | "comment" => Some((source, edited(source, ".docx"))),
            "pack" => {
                let dir = source.trim_end_matches(['/', '\\']);
                Some((dir, edited(dir, ".stet")))
            }
            _ => None,
        }
    }
}

/// The files the `stetdoc` command whose arguments (after the program) are `words` writes, as
/// spelled in it: each output (`-o`, `--output`, any spelling); else with `--in-place` the
/// document (the first positional); else the default output beside its source
/// ([`Parsed::default_output`]). Nothing for `--dry-run`, a subcommand that only reads or
/// serves, or anything clap would refuse (an unknown subcommand or option).
pub(crate) fn written(words: &[String]) -> Vec<String> {
    let args = args_of(words);
    let Form::Writing { sub, parsed } = form(&args) else {
        return Vec::new();
    };
    if parsed.unknown || parsed.dry_run {
        return Vec::new();
    }
    if !parsed.outputs.is_empty() {
        // `-o -` writes to stdout.
        return parsed
            .outputs
            .iter()
            .filter(|out| **out != "-")
            .map(|out| (*out).to_owned())
            .collect();
    }
    if parsed.in_place {
        // Only `edit`, `replace` and `comment` have the flag (any other is `unknown`).
        return parsed
            .positionals
            .first()
            .map(|doc| (*doc).to_owned())
            .into_iter()
            .collect();
    }
    let Some((source, name)) = parsed.default_output(sub) else {
        return Vec::new();
    };
    let folder = source
        .rfind(['/', '\\'])
        .and_then(|at| source.get(..=at))
        .unwrap_or("");
    vec![format!("{folder}{name}")]
}
