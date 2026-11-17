//! The parameters of `Remove-Item`, `Move-Item` and `Rename-Item`, bound as PowerShell binds
//! them: a parameter named in full, by a unique prefix or by an alias, any case; its value the
//! next word, or what follows the name's colon (`-Path:a.docx`; `-Path: a.docx` takes the next
//! word); every word after `--` a position; and the positions in order, `-Path` first, then
//! `-Destination` or `-NewName`. `Remove-Item` reads every position as a path.

use crate::pre_write_guard::ARRAY_COMMA;

use super::{Act, Arg, Taken, To, ends_in_separator};

/// The cmdlet read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cmdlet {
    Remove,
    Move,
    Rename,
}

/// What a parameter's value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// A path the cmdlet takes.
    Path,
    /// Where a move or a rename puts it.
    To,
    /// A value that is no path (`-Filter`, `-ErrorAction`).
    Valued,
    /// No value (`-Force`, `-Recurse`).
    Switch,
}

/// The parameters all three cmdlets take, the common parameters among them, lower case.
const SHARED: [(&str, Slot); 19] = [
    ("path", Slot::Path),
    ("literalpath", Slot::Path),
    ("credential", Slot::Valued),
    ("force", Slot::Switch),
    ("whatif", Slot::Switch),
    ("confirm", Slot::Switch),
    ("usetransaction", Slot::Switch),
    ("verbose", Slot::Switch),
    ("debug", Slot::Switch),
    ("erroraction", Slot::Valued),
    ("warningaction", Slot::Valued),
    ("informationaction", Slot::Valued),
    ("errorvariable", Slot::Valued),
    ("warningvariable", Slot::Valued),
    ("informationvariable", Slot::Valued),
    ("outvariable", Slot::Valued),
    ("outbuffer", Slot::Valued),
    ("pipelinevariable", Slot::Valued),
    ("progressaction", Slot::Valued),
];

/// The parameters' aliases, matched whole.
const ALIASES: [(&str, Slot); 16] = [
    ("pspath", Slot::Path),
    ("lp", Slot::Path),
    ("ea", Slot::Valued),
    ("wa", Slot::Valued),
    ("infa", Slot::Valued),
    ("ev", Slot::Valued),
    ("wv", Slot::Valued),
    ("iv", Slot::Valued),
    ("ov", Slot::Valued),
    ("ob", Slot::Valued),
    ("pv", Slot::Valued),
    ("proga", Slot::Valued),
    ("vb", Slot::Switch),
    ("db", Slot::Switch),
    ("wi", Slot::Switch),
    ("cf", Slot::Switch),
];

impl Cmdlet {
    /// The parameters this cmdlet takes besides [`SHARED`].
    fn own(self) -> &'static [(&'static str, Slot)] {
        match self {
            Self::Remove => &[
                ("filter", Slot::Valued),
                ("include", Slot::Valued),
                ("exclude", Slot::Valued),
                ("stream", Slot::Valued),
                ("recurse", Slot::Switch),
            ],
            Self::Move => &[
                ("destination", Slot::To),
                ("filter", Slot::Valued),
                ("include", Slot::Valued),
                ("exclude", Slot::Valued),
                ("passthru", Slot::Switch),
            ],
            Self::Rename => &[("newname", Slot::To), ("passthru", Slot::Switch)],
        }
    }
}

/// The slot of the parameter `name` (no `-`): an alias or a whole name, else the one slot of
/// every parameter it is a prefix of; `None` when it is none of the cmdlet's.
fn slot_of(name: &str, cmdlet: Cmdlet) -> Option<Slot> {
    let name = name.to_ascii_lowercase();
    let all = || SHARED.iter().chain(cmdlet.own());
    if let Some((_, slot)) = ALIASES
        .iter()
        .chain(all())
        .find(|(whole, _)| *whole == name)
    {
        return Some(*slot);
    }
    let mut slots = all()
        .filter(|(whole, _)| whole.starts_with(name.as_str()))
        .map(|(_, slot)| *slot);
    let first = slots.next()?;
    slots.all(|slot| slot == first).then_some(first)
}

/// What `cmdlet` with `args` deletes or moves.
pub(super) fn taken(args: &[&str], cmdlet: Cmdlet) -> Taken {
    let mut paths = Vec::new();
    let mut to = None;
    let mut unclear = false;
    let (mut path_named, mut to_named) = (false, false);
    let mut positions = Vec::new();
    let mut parameters = true;
    let mut words = args.iter().enumerate();
    while let Some((i, arg)) = words.next() {
        let Some(name) = arg
            .strip_prefix('-')
            .filter(|name| parameters && !name.is_empty())
        else {
            positions.push(Arg::whole(i));
            continue;
        };
        if name == "-" {
            parameters = false;
            continue;
        }
        let (name, inline) = name
            .split_once(':')
            .map_or((name, None), |(name, value)| (name, Some(value)));
        let Some(slot @ (Slot::Path | Slot::To | Slot::Valued)) = slot_of(name, cmdlet) else {
            continue;
        };
        let value = match inline.filter(|value| !value.is_empty()) {
            Some(value) => Some(Arg {
                at: i,
                from: arg.len() - value.len(),
            }),
            None => words.next().map(|(at, _)| Arg::whole(at)),
        };
        unclear |= value.is_none() && slot != Slot::Valued;
        match slot {
            Slot::Path => {
                path_named = true;
                paths.extend(value);
            }
            Slot::To => {
                to_named = true;
                to = to.or(value);
            }
            Slot::Valued | Slot::Switch => {}
        }
    }
    let mut open = positions.into_iter();
    if cmdlet == Cmdlet::Remove {
        paths.extend(open);
        return Taken {
            act: Some(Act::Delete),
            input: paths.is_empty(),
            deleted: paths,
            unclear,
            ..Taken::default()
        };
    }
    if !path_named {
        paths.extend(open.next());
    }
    if !to_named {
        to = open.next();
    }
    let several = paths.len() > 1
        || paths.iter().any(|arg| {
            args.get(arg.at)
                .is_some_and(|text| text.contains(ARRAY_COMMA))
        });
    let to = to.map(|arg| {
        if cmdlet == Cmdlet::Rename {
            To::Beside(arg)
        } else if several || ends_in_separator(args, arg) {
            To::Folder(arg)
        } else {
            To::Path(arg)
        }
    });
    Taken {
        act: Some(Act::Move),
        input: paths.is_empty(),
        sources: paths,
        to,
        unclear,
        ..Taken::default()
    }
}
