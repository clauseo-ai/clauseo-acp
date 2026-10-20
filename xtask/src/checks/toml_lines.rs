//! Line numbers in Cargo manifests. `cargo metadata` gives the dependencies; these helpers parse
//! the manifest with the `toml` crate and read its spans, so a violation can point at the line
//! that declares a dependency (or names a feature), whatever the spelling: a comment after a
//! table header, a quoted key (`'tauri' = ...`), a `[dependencies.tauri]` table, a dotted
//! `tauri.workspace = true`, or a `[target.'cfg(...)'.dependencies]` table.

use crate::checks;
use toml::Spanned;
use toml::de::{DeString, DeTable, DeValue};

/// The tables a package declares dependencies in, each also under `[target.<cfg>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[build-dependencies]`.
    Build,
    /// `[dev-dependencies]`.
    Dev,
}

impl Kind {
    /// The table's name in the manifest.
    fn table(self) -> &'static str {
        match self {
            Kind::Normal => "dependencies",
            Kind::Build => "build-dependencies",
            Kind::Dev => "dev-dependencies",
        }
    }
}

/// `[dependencies]` only.
pub(crate) const NORMAL: &[Kind] = &[Kind::Normal];
/// `[build-dependencies]` only.
pub(crate) const BUILD: &[Kind] = &[Kind::Build];
/// Every dependency table: normal, build and dev.
pub(crate) const ANY: &[Kind] = &[Kind::Normal, Kind::Build, Kind::Dev];

/// A parsed manifest. Text that is not TOML locates nothing: every lookup gives `None`.
pub(crate) struct Manifest<'a> {
    text: &'a str,
    doc: Option<Spanned<DeTable<'a>>>,
}

impl<'a> Manifest<'a> {
    /// Parses `text`.
    pub(crate) fn parse(text: &'a str) -> Manifest<'a> {
        Manifest {
            text,
            doc: DeTable::parse(text).ok(),
        }
    }

    /// The first line that declares dependency `key` in a table of `kinds`, with or without a
    /// target: the line of the key itself (in a `[<table>.key]` header, that header's line).
    pub(crate) fn declaration(&self, key: &str, kinds: &[Kind]) -> Option<u32> {
        self.declarations(key, kinds).first().copied()
    }

    /// Every line that declares dependency `key` in a table of `kinds`, in order, each once.
    /// `declaration` is the first of them.
    pub(crate) fn declarations(&self, key: &str, kinds: &[Kind]) -> Vec<u32> {
        let mut lines: Vec<u32> = self
            .entries(key, kinds)
            .map(|(key, _)| self.line(key.span().start))
            .collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    /// The line where the entry for `key` (in `kinds`) lists `feature` in its `features`.
    pub(crate) fn feature_in_entry(&self, key: &str, kinds: &[Kind], feature: &str) -> Option<u32> {
        self.entries(key, kinds)
            .filter_map(|(_, value)| self.feature_line(value, feature))
            .min()
    }

    /// The line where the `[workspace.dependencies]` entry for `key` lists `feature`.
    pub(crate) fn workspace_feature(&self, key: &str, feature: &str) -> Option<u32> {
        let table = self.root().and_then(|root| table_at(root, "workspace"))?;
        let (_, value) = entry(table_at(table, "dependencies")?, key)?;
        self.feature_line(value, feature)
    }

    /// The line of the first `[features]` item that is exactly `item`.
    pub(crate) fn feature_item(&self, item: &str) -> Option<u32> {
        let features = self.root().and_then(|root| table_at(root, "features"))?;
        features
            .values()
            .filter_map(|value| value.get_ref().as_array())
            .flat_map(|items| items.iter())
            .find(|value| value.get_ref().as_str() == Some(item))
            .map(|value| self.line(value.span().start))
    }

    /// The line of the `[dependencies]` table's key: its header, or its first dotted entry.
    pub(crate) fn dependencies_header(&self) -> Option<u32> {
        let (key, _) = entry(self.root()?, Kind::Normal.table())?;
        Some(self.line(key.span().start))
    }

    /// The root table.
    fn root(&self) -> Option<&DeTable<'a>> {
        self.doc.as_ref().map(Spanned::get_ref)
    }

    /// Every entry for `key` in the tables of `kinds`, top-level and under each target.
    fn entries<'s>(
        &'s self,
        key: &'s str,
        kinds: &'s [Kind],
    ) -> impl Iterator<Item = Entry<'s, 'a>> + 's {
        let root = self.root();
        let targets = root
            .and_then(|root| table_at(root, "target"))
            .into_iter()
            .flat_map(|targets| targets.values())
            .filter_map(|target| target.get_ref().as_table());
        root.into_iter()
            .chain(targets)
            .flat_map(move |scope| {
                kinds
                    .iter()
                    .filter_map(move |kind| table_at(scope, kind.table()))
            })
            .filter_map(move |table| entry(table, key))
    }

    /// The line of `feature` in the `features` array of the dependency entry `value`.
    fn feature_line(&self, value: &Spanned<DeValue<'a>>, feature: &str) -> Option<u32> {
        let features = value
            .get_ref()
            .as_table()?
            .iter()
            .find_map(|(name, list)| {
                (name.get_ref() == "features")
                    .then(|| list.get_ref().as_array())
                    .flatten()
            })?;
        features
            .iter()
            .find(|item| item.get_ref().as_str() == Some(feature))
            .map(|item| self.line(item.span().start))
    }

    /// The 1-based line of byte `offset`.
    fn line(&self, offset: usize) -> u32 {
        let before = self.text.get(..offset).unwrap_or(self.text);
        checks::line_at(before.matches('\n').count())
    }
}

/// A key and its value, both with their spans.
type Entry<'s, 'a> = (&'s Spanned<DeString<'a>>, &'s Spanned<DeValue<'a>>);

/// The entry `key` of `table`.
fn entry<'s, 'a>(table: &'s DeTable<'a>, key: &str) -> Option<Entry<'s, 'a>> {
    table.iter().find(|(name, _)| name.get_ref() == key)
}

/// The table under `key` in `table`.
fn table_at<'s, 'a>(table: &'s DeTable<'a>, key: &str) -> Option<&'s DeTable<'a>> {
    entry(table, key).and_then(|(_, value)| value.get_ref().as_table())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "[dependencies] # runtime\ntauri-x = \"1\"\ntauri = { version = \"2\", features = [\n  \"devtools\",\n] }\n'quoted' = \"1\"\ndotted.workspace = true\n\n[target.'cfg(unix)'.dependencies.os]\nversion = \"2\"\n\n[build-dependencies]\nbuilt = \"1\"\n\n[workspace.dependencies]\n\"t\" = { version = \"1\", features = [\"x\"] }\n\n[features]\nall = [\"tauri/devtools\"]\n";

    #[test]
    fn a_key_is_found_by_its_own_line_or_its_table_header() {
        let manifest = Manifest::parse(TEXT);
        assert_eq!(manifest.declaration("tauri", NORMAL), Some(3));
        assert_eq!(manifest.declaration("quoted", NORMAL), Some(6));
        assert_eq!(manifest.declaration("dotted", NORMAL), Some(7));
        assert_eq!(manifest.declaration("os", NORMAL), Some(9));
        assert_eq!(manifest.declaration("built", NORMAL), None);
        assert_eq!(manifest.declaration("built", BUILD), Some(13));
        assert_eq!(manifest.declarations("built", BUILD), [13]);
        assert_eq!(manifest.declaration("t", ANY), None);
        assert_eq!(manifest.dependencies_header(), Some(1));
    }

    #[test]
    fn a_feature_is_found_inside_its_entry_or_the_features_table() {
        let manifest = Manifest::parse(TEXT);
        assert_eq!(
            manifest.feature_in_entry("tauri", NORMAL, "devtools"),
            Some(4)
        );
        assert_eq!(
            manifest.feature_in_entry("tauri", NORMAL, "protocol-asset"),
            None
        );
        assert_eq!(manifest.workspace_feature("t", "x"), Some(16));
        assert_eq!(manifest.feature_item("tauri/devtools"), Some(19));
        assert_eq!(manifest.feature_item("other"), None);
    }

    #[test]
    fn text_that_is_not_toml_locates_nothing() {
        let manifest = Manifest::parse("[dependencies\ntauri = ");
        assert_eq!(manifest.declaration("tauri", ANY), None);
        assert_eq!(manifest.dependencies_header(), None);
    }
}
