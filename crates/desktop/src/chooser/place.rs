//! A folder the chooser stands on, as a Windows path held in parts ([`Place`]): a drive root
//! (`C:\`) or a share root (`\\server\share`), then the names below it. Plain strings, so every
//! rule here runs on every OS. A typed path becomes a place only through [`Place::typed`], which
//! refuses what could leave the folders the chooser offers.

/// The longest typed path the chooser takes, in characters.
pub(crate) const TYPED_MAX: usize = 1024;

/// A drive root or a share root, then the folder names below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    root: Root,
    parts: Vec<String>,
}

/// Where a place starts.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Root {
    /// A drive letter, upper case.
    Drive(char),
    /// `\\server\share`, as written.
    Share { server: String, share: String },
}

impl Place {
    /// A drive's root, `C:\`.
    pub(crate) fn drive(letter: char) -> Option<Self> {
        letter.is_ascii_alphabetic().then(|| Self {
            root: Root::Drive(letter.to_ascii_uppercase()),
            parts: Vec::new(),
        })
    }

    /// A path the OS gave (home, Desktop, the last pick): read by the same rules as a typed one.
    pub(crate) fn from_os(path: &str) -> Option<Self> {
        Self::typed(path)
    }

    /// A path the lawyer typed: `X:\...` or `\\server\share\...`, `/` read as `\`. Refused:
    /// more than [`TYPED_MAX`] characters, `\\?\` and `\\.\` paths, a relative or
    /// drive-relative path (`C:Cases`), a `.` or `..` part, a `:` anywhere but after the drive
    /// letter (alternate streams), the characters Windows forbids in a name, and a part ending in
    /// a dot or a space (which Windows trims, so it would name another folder).
    pub(crate) fn typed(path: &str) -> Option<Self> {
        if path.chars().count() > TYPED_MAX {
            return None;
        }
        let path = path.replace('/', "\\");
        let (root, rest) = if let Some(unc) = path.strip_prefix("\\\\") {
            let mut parts = unc.splitn(3, '\\');
            let server = parts.next().filter(|s| name_ok(s))?;
            let share = parts.next().filter(|s| name_ok(s))?;
            // `\\?\` and `\\.\`: the server would be `?` or `.`, which `name_ok` refuses.
            let root = Root::Share {
                server: server.to_owned(),
                share: share.to_owned(),
            };
            (root, parts.next().unwrap_or(""))
        } else {
            let mut chars = path.chars();
            let letter = chars.next().filter(char::is_ascii_alphabetic)?;
            let rest = chars.as_str().strip_prefix(":\\")?;
            (Root::Drive(letter.to_ascii_uppercase()), rest)
        };
        let mut parts = Vec::new();
        for part in rest.split('\\').filter(|part| !part.is_empty()) {
            if !name_ok(part) {
                return None;
            }
            parts.push(part.to_owned());
        }
        Some(Self { root, parts })
    }

    /// The folder `name` inside this one, when `name` is a plain folder name.
    pub(crate) fn child(&self, name: &str) -> Option<Self> {
        name_ok(name).then(|| {
            let mut parts = self.parts.clone();
            parts.push(name.to_owned());
            Self {
                root: self.root.clone(),
                parts,
            }
        })
    }

    /// The folder above, stopping at the drive or share root.
    pub(crate) fn up(&self) -> Self {
        let mut parts = self.parts.clone();
        parts.pop();
        Self {
            root: self.root.clone(),
            parts,
        }
    }

    /// This place's drive or share root.
    pub(crate) fn root(&self) -> Self {
        Self {
            root: self.root.clone(),
            parts: Vec::new(),
        }
    }

    /// The drive letter, for a place on a drive.
    pub(crate) fn letter(&self) -> Option<char> {
        match self.root {
            Root::Drive(letter) => Some(letter),
            Root::Share { .. } => None,
        }
    }

    /// Whether `self` and `other` name the same folder, as Windows compares names.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.text().to_lowercase() == other.text().to_lowercase()
    }

    /// Whether `self` is `other` or inside it.
    pub(crate) fn within(&self, other: &Self) -> bool {
        self.same_root(other)
            && self.parts.len() >= other.parts.len()
            && self
                .parts
                .iter()
                .zip(&other.parts)
                .all(|(ours, theirs)| ours.to_lowercase() == theirs.to_lowercase())
    }

    /// Whether both places start at the same drive or share.
    fn same_root(&self, other: &Self) -> bool {
        self.root().same(&other.root())
    }

    /// The full path: `C:\Cases\Acme`, `\\server\share\Acme`.
    pub(crate) fn text(&self) -> String {
        let mut text = match &self.root {
            Root::Drive(letter) => format!("{letter}:\\"),
            Root::Share { server, share } => format!("\\\\{server}\\{share}\\"),
        };
        text.push_str(&self.parts.join("\\"));
        if matches!(self.root, Root::Share { .. }) && self.parts.is_empty() {
            text.pop();
        }
        text
    }

    /// The root as a crumb shows it: `C:` or `\\server\share`.
    pub(crate) fn root_label(&self) -> String {
        match &self.root {
            Root::Drive(letter) => format!("{letter}:"),
            Root::Share { server, share } => format!("\\\\{server}\\{share}"),
        }
    }

    /// Whether this is a share path; a mapped network drive is the machine's question.
    pub(crate) fn is_share(&self) -> bool {
        matches!(self.root, Root::Share { .. })
    }

    /// The names below the root.
    pub(crate) fn parts(&self) -> &[String] {
        &self.parts
    }
}

/// Whether `name` is one folder name the chooser takes: not empty, not `.` or `..`, no
/// separator, no `:` and none of `*?"<>|` or a control character, and not ending in a dot or a
/// space.
pub(crate) fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.ends_with(['.', ' '])
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}
