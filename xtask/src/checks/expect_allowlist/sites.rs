//! The sites of one Rust file, found with syn: every `expect`, every `allow` and `warn` (which
//! lower a lint), and every form that could hide one of them from this check.
//!
//! syn parses the file, and a visitor (`visit`) reaches every attribute in it: outer and inner, on
//! items, statements, expressions, fields and the rest. An attribute counts when its path is
//! `expect`, `allow` or `warn`, or when it is a `cfg_attr` whose attributes (after the predicate,
//! at any depth) are. syn leaves the bodies of macros as tokens, so the tokens of every macro (a
//! `macro_rules!` definition and a call alike) are walked for `#`, an optional `!`, then a bracket
//! group, which is then read as an attribute. Doc comments are `doc` attributes and string
//! literals are single tokens, so text that mentions `#[expect]` is never a site.
//!
//! What the check could not read is a site too: an attribute in a macro body whose name or lint
//! is a metavariable (`#[$m]`, `#[cfg_attr(.., $m)]`, `#[expect($lint)]`), a `#` in a macro body
//! with no brackets after it, `#[path]` and `include!`, which make rustc compile a file this
//! check may not read. The value of a `name = value` attribute is an expression, and the sites in
//! it (an `include!`, an attribute in a block) are found too.

mod visit;

use super::fix::Level;
use proc_macro2::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::visit::Visit;

/// The item name of an attribute at the top level of a file that sits on no named item: an inner
/// attribute of the file, or one on a `use`, an `extern` block or a macro call.
pub(super) const FILE_TOP: &str = "-";

/// One finding of the file, at one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Site {
    /// The 1-based line of the attribute's name, or of the `#`, `path` or `include`.
    pub line: u32,
    /// The path of the item the attribute sits on, or of the nearest item around it for a
    /// statement or an expression; `-` at the top level of the file (see `FILE_TOP`).
    pub item: String,
    pub kind: Kind,
}

/// What was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Kind {
    /// `expect` of this lint; `inner` for `#![expect]`.
    Expect { lint: String, inner: bool },
    /// `allow` or `warn` of this lint; `inner` for `#![..]`.
    Lower {
        level: Level,
        lint: String,
        inner: bool,
    },
    /// `allow` or `warn` that names no lint, such as `#[allow()]`.
    EmptyLower { level: Level },
    /// An attribute in a macro body whose name, or one of whose lints, is a metavariable.
    Metavariable,
    /// A `#` in a macro body that is not followed by `[..]` or `![..]`.
    HashWithoutBrackets,
    /// `#[path = ..]`: the string literal, or `None` for any other value. `resolve` is true when
    /// the attribute sits on an item at the top level of the file, outside any macro body, so
    /// the path is relative to the file's folder.
    Path {
        value: Option<String>,
        resolve: bool,
    },
    /// `include!(..)`: the string literal, or `None` for any other argument.
    Include { target: Option<String> },
}

/// The sites in `text`, in source order, or syn's error.
pub(super) fn find(text: &str) -> syn::Result<Vec<Site>> {
    let file = syn::parse_file(text)?;
    let mut finder = Finder::default();
    finder.file(&file);
    let mut sites = finder.sites;
    sites.sort_by_key(|site| site.line);
    Ok(sites)
}

/// Where an attribute is read, which decides what it may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// In the syntax tree syn built; `resolve` as in `Kind::Path`.
    Tree { resolve: bool },
    /// In the tokens of a macro.
    Macro,
}

/// Walks one file, keeping the path of the items it is inside.
#[derive(Default)]
struct Finder {
    /// The names of the items around the walk, outermost first.
    path: Vec<String>,
    sites: Vec<Site>,
}

impl Finder {
    /// Walks `file`: its inner attributes, then its items.
    fn file(&mut self, file: &syn::File) {
        for attr in &file.attrs {
            self.attribute(attr, Origin::Tree { resolve: false });
        }
        for item in &file.items {
            self.visit_item(item);
        }
    }

    /// The item a site found now names.
    fn current(&self) -> String {
        if self.path.is_empty() {
            FILE_TOP.to_owned()
        } else {
            self.path.join("::")
        }
    }

    /// Records a site at `line`, naming the current item.
    fn push(&mut self, line: u32, kind: Kind) {
        let item = self.current();
        self.sites.push(Site { line, item, kind });
    }

    /// Visits a node inside the item called `name`.
    fn named(&mut self, name: String, visit: impl FnOnce(&mut Self)) {
        self.path.push(name);
        visit(self);
        self.path.pop();
    }

    /// Records the sites of a syn attribute.
    fn attribute(&mut self, attr: &syn::Attribute, origin: Origin) {
        let inner = matches!(attr.style, syn::AttrStyle::Inner(_));
        self.meta(&attr.meta, inner, origin);
    }

    /// Records the sites of one attribute's meta: an `expect`, `allow` or `warn`, a `path`, or
    /// the attributes of a `cfg_attr` after its predicate.
    fn meta(&mut self, meta: &syn::Meta, inner: bool, origin: Origin) {
        let Some(ident) = meta.path().get_ident() else {
            return;
        };
        let line = line_of(ident.span());
        let name = ident.unraw().to_string();
        let level = match name.as_str() {
            "allow" => Some(Level::Allow),
            "warn" => Some(Level::Warn),
            _ => None,
        };
        match meta {
            syn::Meta::NameValue(pair) if name == "path" => {
                let value = match &pair.value {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(text),
                        ..
                    }) => Some(text.value()),
                    _ => None,
                };
                let resolve = matches!(origin, Origin::Tree { resolve: true });
                self.push(line, Kind::Path { value, resolve });
            }
            syn::Meta::Path(_) | syn::Meta::NameValue(_) => {
                if let Some(level) = level {
                    self.push(line, Kind::EmptyLower { level });
                }
            }
            syn::Meta::List(list) => match name.as_str() {
                "expect" | "allow" | "warn" => {
                    let lints = lints(list.tokens.clone());
                    if lints.iter().any(|lint| lint.contains('$')) {
                        self.push(line, Kind::Metavariable);
                        return;
                    }
                    if lints.is_empty()
                        && let Some(level) = level
                    {
                        self.push(line, Kind::EmptyLower { level });
                    }
                    for lint in lints {
                        let kind = match level {
                            Some(level) => Kind::Lower { level, lint, inner },
                            None => Kind::Expect { lint, inner },
                        };
                        self.push(line, kind);
                    }
                }
                "cfg_attr" => self.cfg_attr(list.tokens.clone(), inner, origin, line),
                _ => {}
            },
        }
        // The value of `name = value` is an expression: an `include!` in it, or an attribute on
        // a statement of a block in it (`#[doc = { #[allow(dead_code)] let _a = 1; "" }]`), is a
        // site.
        if let syn::Meta::NameValue(pair) = meta {
            Visit::visit_expr(self, &pair.value);
        }
    }

    /// Records the sites of the attributes of a `cfg_attr` after its predicate. One that starts
    /// with a metavariable is a `Metavariable` site at `line`.
    fn cfg_attr(&mut self, tokens: TokenStream, inner: bool, origin: Origin, line: u32) {
        for chunk in split_commas(tokens).into_iter().skip(1) {
            if starts_with_dollar(&chunk) {
                self.push(line, Kind::Metavariable);
            } else if let Ok(meta) = syn::parse2::<syn::Meta>(chunk) {
                self.meta(&meta, inner, origin);
            }
        }
    }

    /// Finds the attributes and `include!` calls in the tokens of a macro.
    fn tokens(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut index = 0;
        while let Some(tree) = trees.get(index) {
            index += 1;
            match tree {
                TokenTree::Punct(punct) if punct.as_char() == '#' => {
                    let bang = is_punct(trees.get(index), '!');
                    let at = if bang { index + 1 } else { index };
                    match trees.get(at) {
                        Some(TokenTree::Group(group))
                            if group.delimiter() == Delimiter::Bracket =>
                        {
                            self.token_attribute(group.stream(), bang, line_of(punct.span()));
                            index = at + 1;
                        }
                        _ => self.push(line_of(punct.span()), Kind::HashWithoutBrackets),
                    }
                }
                TokenTree::Ident(ident)
                    if ident == "include" && is_punct(trees.get(index), '!') =>
                {
                    if let Some(TokenTree::Group(group)) = trees.get(index + 1) {
                        self.include(group.stream(), ident.span());
                        index += 2;
                    }
                }
                TokenTree::Group(group) => self.tokens(group.stream()),
                _ => {}
            }
        }
    }

    /// Records the sites of the attribute whose bracket holds `inside`, found in a macro's tokens
    /// after a `#` at `line`. A name that is a metavariable (`#[$m]`, `#![$($t)*]`) is a site.
    fn token_attribute(&mut self, inside: TokenStream, inner: bool, line: u32) {
        if starts_with_dollar(&inside) {
            self.push(line, Kind::Metavariable);
        } else if let Ok(meta) = syn::parse2::<syn::Meta>(inside) {
            self.meta(&meta, inner, Origin::Macro);
        }
    }

    /// Records an `include!` whose argument tokens are `args`, at the line of `at`.
    fn include(&mut self, args: TokenStream, at: Span) {
        let target = syn::parse2::<syn::LitStr>(args)
            .ok()
            .map(|text| text.value());
        self.push(line_of(at), Kind::Include { target });
    }
}

/// True when `tree` is the punctuation `ch`.
fn is_punct(tree: Option<&TokenTree>, ch: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(p)) if p.as_char() == ch)
}

/// True when the first token of `tokens` is `$`.
fn starts_with_dollar(tokens: &TokenStream) -> bool {
    is_punct(tokens.clone().into_iter().next().as_ref(), '$')
}

/// The lints an `expect(..)`, `allow(..)` or `warn(..)` names, each written without spaces
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

/// The 1-based line where `span` starts.
fn line_of(span: Span) -> u32 {
    u32::try_from(span.start().line).unwrap_or(u32::MAX).max(1)
}

#[cfg(test)]
mod tests;
