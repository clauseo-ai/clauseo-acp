//! The readers of [`super::read_only`] whose options can write: what each writes, so the
//! exemption can check it names no held file.

use super::long_prefix;

/// The pandoc options that write a file or run code; Haskell's `GetOpt` takes any prefix of a
/// long option.
const PANDOC_REFUSED: [&str; 8] = [
    "extract-media",
    "filter",
    "lua-filter",
    "defaults",
    "data-dir",
    "log",
    "pdf-engine",
    "pdf-engine-opt",
];

/// Whether pandoc with `args` only reads and writes stdout or a file `writable` allows: no
/// option ([`PANDOC_REFUSED`], any prefix) that writes or runs code, no short group with `F`,
/// `L` or `d`; each output (`--output`, any prefix, `=value` or the next word; a short group
/// with `o`, the rest of the group or the next word) `writable`. `--` ends the options.
pub(super) fn pandoc_reads(args: &[String], writable: impl Fn(&str) -> bool) -> bool {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        let output = if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = long.split_once('=').unwrap_or((long, ""));
            if PANDOC_REFUSED.iter().any(|option| long_prefix(arg, option)) {
                return false;
            }
            "output".starts_with(name).then_some(value)
        } else if let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) {
            if flags.contains(['F', 'L', 'd']) {
                return false;
            }
            flags.split_once('o').map(|(_, value)| value)
        } else {
            None
        };
        let Some(value) = output else {
            continue;
        };
        let value = if value.is_empty() {
            iter.next().map_or("", String::as_str)
        } else {
            value
        };
        if !value.is_empty() && !writable(value) {
            return false;
        }
    }
    true
}

/// textutil's options that take a value (its man page); `-stdout`, `-noload`, `-nostore`,
/// `-strip`, `-info` and `-help` take none.
const TEXTUTIL_VALUED: [&str; 23] = [
    "-convert",
    "-cat",
    "-extension",
    "-output",
    "-font",
    "-fontsize",
    "-encoding",
    "-inputencoding",
    "-format",
    "-title",
    "-author",
    "-subject",
    "-keywords",
    "-comment",
    "-editor",
    "-company",
    "-creationtime",
    "-modificationtime",
    "-baseurl",
    "-timeout",
    "-textsizemultiplier",
    "-excludedelements",
    "-prefixspaces",
];

/// The files textutil with `args` writes, `None` for an option it may not know (the guard does
/// not guess its arity): its `-output`; else with `-stdout` nothing; else with `-convert FMT`
/// each input's name with its extension replaced by `FMT` (or `-extension EXT`), beside it,
/// and with `-cat FMT` `out.FMT`; else (`-info`) nothing. Each is `(path, given)`: `given`
/// when the command spelled it (`-output`), not derived from an input.
pub(super) fn textutil_outputs(args: &[String]) -> Option<Vec<(String, bool)>> {
    let (mut convert, mut cat, mut extension, mut output) = (None, None, None, None);
    let mut stdout = false;
    let mut inputs = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_str();
        if TEXTUTIL_VALUED.contains(&arg) {
            let value = iter.next().map(String::as_str);
            match arg {
                "-convert" => convert = value,
                "-cat" => cat = value,
                "-extension" => extension = value,
                "-output" => output = value,
                _ => {}
            }
        } else if arg == "-stdout" {
            stdout = true;
        } else if matches!(arg, "-noload" | "-nostore" | "-strip" | "-info" | "-help") {
        } else if arg.starts_with('-') && arg != "-" {
            return None;
        } else {
            inputs.push(arg);
        }
    }
    if let Some(output) = output {
        return Some(vec![(output.to_owned(), true)]);
    }
    let (Some(format), false) = (convert.or(cat), stdout) else {
        return Some(Vec::new());
    };
    let extension = extension.unwrap_or(format);
    if cat.is_some() && convert.is_none() {
        return Some(vec![(format!("out.{extension}"), false)]);
    }
    Some(
        inputs
            .into_iter()
            .map(|input| {
                let split = input.rfind(['/', '\\']).map_or(0, |at| at + 1);
                let stem = input
                    .rfind('.')
                    .filter(|dot| *dot > split)
                    .and_then(|dot| input.get(..dot))
                    .unwrap_or(input);
                (format!("{stem}.{extension}"), false)
            })
            .collect(),
    )
}

/// Whether `cp` with `args` writes no held file: no option but `-p`, `-v`, `-n` (`--` ends
/// them); either exactly a source and a destination that reads as a file (not `.` or `..`, no
/// trailing separator, a `.` inside its last part, since `cp Revised/Contract.docx .` writes
/// `./Contract.docx`) that `writable` allows, or sources and a last word that is a folder
/// `outside` the matter (`cp *.docx /tmp`).
pub(super) fn cp_reads(
    args: &[String],
    writable: impl Fn(&str) -> bool,
    outside: impl Fn(&str) -> bool,
) -> bool {
    let mut positionals = Vec::new();
    let mut ended = false;
    for arg in args {
        if ended || arg == "-" || !arg.starts_with('-') {
            positionals.push(arg.as_str());
        } else if arg == "--" {
            ended = true;
        } else if !arg
            .strip_prefix('-')
            .is_some_and(|flags| !flags.is_empty() && flags.chars().all(|c| "pvn".contains(c)))
        {
            return false;
        }
    }
    let Some((destination, sources)) = positionals.split_last() else {
        return false;
    };
    if sources.is_empty() {
        return false;
    }
    if outside(destination) {
        return true;
    }
    let last = destination
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(destination);
    sources.len() == 1 && writable(destination) && last.trim_start_matches('.').contains('.')
}

/// Whether `unzip` with `args` only lists or prints (`-l`, `-p`, `-v`, `-lv`, with `-q`,
/// `-qq`), or extracts into a folder given by `-d DIR` (or `-dDIR`) that `writable` allows
/// and that is not `.` or `..`, with no option but `-o`, `-n`, `-q`, `-qq`.
pub(super) fn unzip_reads(args: &[String], writable: impl Fn(&str) -> bool) -> bool {
    let options: Vec<&String> = args.iter().filter(|arg| arg.starts_with('-')).collect();
    let lists = options
        .iter()
        .any(|arg| matches!(arg.as_str(), "-l" | "-p" | "-v" | "-lv"))
        && options
            .iter()
            .all(|arg| matches!(arg.as_str(), "-l" | "-p" | "-v" | "-lv" | "-q" | "-qq"));
    if lists {
        return true;
    }
    let mut folder = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-d" => folder = iter.next().map(String::as_str),
            "-o" | "-n" | "-q" | "-qq" => {}
            other if other.starts_with("-d") => folder = other.get(2..),
            other if other.starts_with('-') => return false,
            _ => {}
        }
    }
    folder.is_some_and(|folder| {
        let trimmed = folder.trim_end_matches(['/', '\\']);
        !matches!(trimmed, "" | "." | "..") && writable(folder)
    })
}
