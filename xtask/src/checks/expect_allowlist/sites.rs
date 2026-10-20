//! The lint attribute sites of one Rust file: every `expect` and `allow`, found with syn.
//!
//! syn parses the file, and a visitor reaches every attribute in it: outer and inner, on items,
//! statements, expressions, fields and the rest. An attribute counts when its path is `expect` or
//! `allow`, or when it is a `cfg_attr` whose attributes (after the predicate, at any depth) are.
//! syn leaves the bodies of macros as tokens, so the tokens of every macro (a `macro_rules!`
//! definition and a call alike) are walked for `#`, an optional `!`, then a bracket group, which
//! is then read as an attribute. Doc comments are `doc` attributes and string literals are single
//! tokens, so text that mentions `#[expect]` is never a site.

use proc_macro2::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use std::path::Path;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

/// The item name of an inner attribute at the top of a file.
pub(super) const FILE_TOP: &str = "-";
/// The module name of a crate root: `lib.rs`, `main.rs` or `build.rs`.
pub(super) const CRATE_ROOT: &str = "crate";

/// One lint named by one `expect` or `allow` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Site {
    /// The 1-based line of the attribute's `expect` or `allow`.
    pub line: u32,
    /// The nearest enclosing function, impl method, const, static or module (see `module_name`),
    /// or `-` for an inner attribute at the top of the file.
    pub item: String,
    pub kind: Kind,
}

/// What the attribute is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Kind {
    /// `expect` of this lint; `inner` for `#![expect]`.
    Expect { lint: String, inner: bool },
    /// `allow` of this lint; `inner` for `#![allow]`.
    Allow { lint: String, inner: bool },
    /// `allow` that names no lint, such as `#[allow()]`.
    EmptyAllow,
}

/// The sites in `text`, the Rust file `path` (repo-relative), in source order, or syn's error.
pub(super) fn find(path: &str, text: &str) -> syn::Result<Vec<Site>> {
    let file = syn::parse_file(text)?;
    let mut finder = Finder {
        names: vec![module_name(path)],
        sites: Vec::new(),
    };
    for attr in &file.attrs {
        finder.attribute(attr, FILE_TOP);
    }
    for item in &file.items {
        finder.visit_item(item);
    }
    finder.sites.sort_by_key(|site| site.line);
    Ok(finder.sites)
}

/// The name an item at the top of the file `path` takes for its enclosing module: `crate` for a
/// crate root (`lib.rs`, `main.rs`, `build.rs`), the folder for `mod.rs`, else the file stem,
/// which is the module's name, and for an integration test or a `src/bin` file the name cargo
/// gives its crate.
pub(super) fn module_name(path: &str) -> String {
    let path = Path::new(path);
    let name = |part: Option<&std::ffi::OsStr>| {
        part.map_or_else(
            || CRATE_ROOT.to_owned(),
            |part| part.to_string_lossy().into_owned(),
        )
    };
    match name(path.file_stem()).as_str() {
        "lib" | "main" | "build" => CRATE_ROOT.to_owned(),
        "mod" => name(path.parent().and_then(Path::file_name)),
        stem => stem.to_owned(),
    }
}

/// Walks one file, keeping the names of the items it is inside.
struct Finder {
    /// The enclosing items that name a site, innermost last; never empty.
    names: Vec<String>,
    sites: Vec<Site>,
}

impl Finder {
    /// The nearest enclosing item's name.
    fn current(&self) -> String {
        self.names.last().cloned().unwrap_or_default()
    }

    /// Visits a node inside the item called `name`.
    fn named(&mut self, name: &syn::Ident, visit: impl FnOnce(&mut Self)) {
        self.names.push(name.unraw().to_string());
        visit(self);
        self.names.pop();
    }

    /// Records the sites of a syn attribute, naming them `item`.
    fn attribute(&mut self, attr: &syn::Attribute, item: &str) {
        let inner = matches!(attr.style, syn::AttrStyle::Inner(_));
        self.meta(&attr.meta, inner, item);
    }

    /// Records the sites of one attribute's meta: an `expect`, an `allow`, or the attributes of a
    /// `cfg_attr` after its predicate.
    fn meta(&mut self, meta: &syn::Meta, inner: bool, item: &str) {
        let Some(name) = meta
            .path()
            .get_ident()
            .map(|ident| ident.unraw().to_string())
        else {
            return;
        };
        let syn::Meta::List(list) = meta else {
            if name == "allow" {
                self.push(meta.path().get_ident(), item, Kind::EmptyAllow);
            }
            return;
        };
        let line = line_of(
            list.path
                .segments
                .first()
                .map(|segment| segment.ident.span()),
        );
        match name.as_str() {
            "expect" => {
                for lint in lints(list.tokens.clone()) {
                    self.push_at(line, item, Kind::Expect { lint, inner });
                }
            }
            "allow" => {
                let lints = lints(list.tokens.clone());
                if lints.is_empty() {
                    self.push_at(line, item, Kind::EmptyAllow);
                }
                for lint in lints {
                    self.push_at(line, item, Kind::Allow { lint, inner });
                }
            }
            "cfg_attr" => {
                for chunk in split_commas(list.tokens.clone()).into_iter().skip(1) {
                    if let Ok(meta) = syn::parse2::<syn::Meta>(chunk) {
                        self.meta(&meta, inner, item);
                    }
                }
            }
            _ => {}
        }
    }

    /// Records a site at the line of `ident`.
    fn push(&mut self, ident: Option<&syn::Ident>, item: &str, kind: Kind) {
        self.push_at(line_of(ident.map(syn::Ident::span)), item, kind);
    }

    fn push_at(&mut self, line: u32, item: &str, kind: Kind) {
        self.sites.push(Site {
            line,
            item: item.to_owned(),
            kind,
        });
    }

    /// Finds the attributes in the tokens of a macro: `#`, an optional `!`, then a bracket group.
    fn tokens(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut index = 0;
        while let Some(tree) = trees.get(index) {
            index += 1;
            if let TokenTree::Punct(punct) = tree
                && punct.as_char() == '#'
            {
                let bang =
                    matches!(trees.get(index), Some(TokenTree::Punct(p)) if p.as_char() == '!');
                let at = if bang { index + 1 } else { index };
                if let Some(TokenTree::Group(group)) = trees.get(at)
                    && group.delimiter() == Delimiter::Bracket
                {
                    self.token_attribute(group.stream(), bang);
                    index = at + 1;
                    continue;
                }
            }
            if let TokenTree::Group(group) = tree {
                self.tokens(group.stream());
            }
        }
    }

    /// Records the sites of the attribute whose bracket holds `inside`, found in a macro's tokens.
    /// Tokens that do not read as an attribute, such as `#[$meta]`, name no lint.
    fn token_attribute(&mut self, inside: TokenStream, inner: bool) {
        if let Ok(meta) = syn::parse2::<syn::Meta>(inside) {
            let item = self.current();
            self.meta(&meta, inner, &item);
        }
    }
}

impl<'ast> Visit<'ast> for Finder {
    fn visit_attribute(&mut self, i: &'ast syn::Attribute) {
        let item = self.current();
        self.attribute(i, &item);
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        self.named(&i.sig.ident, |finder| visit::visit_item_fn(finder, i));
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        self.named(&i.sig.ident, |finder| {
            visit::visit_impl_item_fn(finder, i);
        });
    }

    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        self.named(&i.ident, |finder| visit::visit_item_const(finder, i));
    }

    fn visit_impl_item_const(&mut self, i: &'ast syn::ImplItemConst) {
        self.named(&i.ident, |finder| {
            visit::visit_impl_item_const(finder, i);
        });
    }

    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        self.named(&i.ident, |finder| visit::visit_item_static(finder, i));
    }

    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.named(&i.ident, |finder| visit::visit_item_mod(finder, i));
    }

    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        self.tokens(i.tokens.clone());
        visit::visit_macro(self, i);
    }
}

/// The lints an `expect(..)` or `allow(..)` names, each written without spaces
/// (`clippy::unwrap_used`). A `key = value` argument such as `reason = ".."` is not a lint.
fn lints(args: TokenStream) -> Vec<String> {
    split_commas(args)
        .into_iter()
        .filter(|chunk| !is_key_value(chunk))
        .map(|chunk| chunk.to_string().split_whitespace().collect())
        .collect()
}

/// True for `name = value`: an identifier, then a lone `=`.
fn is_key_value(chunk: &TokenStream) -> bool {
    let mut trees = chunk.clone().into_iter();
    matches!(
        (trees.next(), trees.next()),
        (Some(TokenTree::Ident(_)), Some(TokenTree::Punct(p)))
            if p.as_char() == '=' && p.spacing() == Spacing::Alone
    )
}

/// `tokens` split at its top-level commas, empty pieces dropped.
fn split_commas(tokens: TokenStream) -> Vec<TokenStream> {
    let mut chunks = Vec::new();
    let mut chunk = Vec::new();
    for tree in tokens {
        if matches!(&tree, TokenTree::Punct(p) if p.as_char() == ',') {
            chunks.push(std::mem::take(&mut chunk));
        } else {
            chunk.push(tree);
        }
    }
    chunks.push(chunk);
    chunks
        .into_iter()
        .filter(|chunk| !chunk.is_empty())
        .map(TokenStream::from_iter)
        .collect()
}

/// The 1-based line where `span` starts, or 1 when there is none.
fn line_of(span: Option<Span>) -> u32 {
    span.map_or(1, |span| {
        u32::try_from(span.start().line).unwrap_or(u32::MAX).max(1)
    })
}

#[cfg(test)]
mod tests;
