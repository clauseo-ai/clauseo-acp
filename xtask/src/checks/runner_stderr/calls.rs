//! What `runner-stderr` judges in one file: each macro that writes to the console or to a stderr
//! handle, each call of a path ending in `stderr`, each error formatted outside `Detail::new`
//! (in a format macro's arguments or as `.to_string()`), each `use` that renames `Detail` or
//! `stderr`, and each `macro_rules!` whose body calls a console macro or names `stderr`.
//!
//! A macro's arguments are tokens to syn, so they are judged by scanning them: a call inside
//! `format!(...)` is not an `ExprCall`. The scan finds `stderr(` and `error.to_string()` in them,
//! and it reads the macros nested in them the same way, so a call in a nested macro counts too.

use super::Fix;
use crate::checks;
use crate::violation::Violation;
use proc_macro2::{Delimiter, Group, Ident, Span, TokenStream, TokenTree};
use syn::visit::Visit;

/// The macros that write to the console.
const CONSOLE: [&str; 5] = ["eprintln", "eprint", "println", "print", "dbg"];
/// The macros whose first argument is the writer they write to.
const WRITES: [&str; 2] = ["write", "writeln"];
/// The macros whose arguments are judged for an error's text: their format string, and the
/// arguments the format string places.
const FORMATS: [&str; 10] = [
    "format",
    "format_args",
    "write",
    "writeln",
    "panic",
    "unreachable",
    "todo",
    "assert",
    "assert_eq",
    "debug_assert",
];
/// The names a `use` may not rename, since the check finds them by name.
const RENAMED: [&str; 2] = ["Detail", "stderr"];
/// The message for a call that opens stderr.
const STDERR_CALL: &str =
    "stderr() opens stderr; the runner says things only through runner::stderr";

/// One violation before its file is known: where it is, what it says and its fix.
#[derive(Debug)]
pub(super) struct Hit {
    pub(super) at: Span,
    pub(super) message: String,
    pub(super) fix: Fix,
}

/// The calls of one file, and the violations among them.
pub(super) struct Calls<'a> {
    path: &'a str,
    /// How many `Detail::new(...)` argument lists the visit is inside: an error may be formatted
    /// in one.
    detail: usize,
    pub(super) found: Vec<Violation>,
}

impl<'a> Calls<'a> {
    pub(super) fn new(path: &'a str) -> Calls<'a> {
        Calls {
            path,
            detail: 0,
            found: Vec::new(),
        }
    }

    fn push(&mut self, hit: Hit) {
        let line = u32::try_from(hit.at.start().line).unwrap_or(1).max(1);
        self.found
            .push(checks::violation(self.path, line, hit.message, hit.fix));
    }
}

impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        let at = i
            .path
            .segments
            .first()
            .map_or_else(|| i.bang_token.span, |segment| segment.ident.span());
        let name = last_name(&i.path);
        let mut hits = Vec::new();
        if name != "macro_rules" {
            check_macro(&name, at, &i.tokens, self.detail > 0, &mut hits);
        } else if let Some(problem) = body_problem(&i.tokens) {
            let message =
                format!("a macro_rules! body {problem}, which runner-stderr can't follow");
            hits.push(Hit {
                at,
                message,
                fix: Fix::Wrapper,
            });
        }
        for hit in hits {
            self.push(hit);
        }
        syn::visit::visit_macro(self, i);
    }

    fn visit_expr_call(&mut self, i: &'ast syn::ExprCall) {
        let path = match &*i.func {
            syn::Expr::Path(callee) => Some(&callee.path),
            _ => None,
        };
        let is_detail = path.is_some_and(is_detail_new);
        if let Some(last) = path
            .and_then(|path| path.segments.last())
            .filter(|last| last.ident == "stderr")
        {
            self.push(Hit {
                at: last.ident.span(),
                message: STDERR_CALL.to_owned(),
                fix: Fix::StderrCall,
            });
        }
        self.detail += usize::from(is_detail);
        syn::visit::visit_expr_call(self, i);
        self.detail -= usize::from(is_detail);
    }

    fn visit_expr_method_call(&mut self, i: &'ast syn::ExprMethodCall) {
        if self.detail == 0
            && i.method == "to_string"
            && i.args.is_empty()
            && let syn::Expr::Path(receiver) = &*i.receiver
            && let Some(name) = receiver.path.get_ident()
            && is_errorish(&name.to_string())
        {
            self.push(Hit {
                at: name.span(),
                message: to_string_message(&name.to_string()),
                fix: Fix::ErrorText,
            });
        }
        syn::visit::visit_expr_method_call(self, i);
    }

    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if !is_cfg_test(&i.attrs) {
            syn::visit::visit_item_mod(self, i);
        }
    }

    fn visit_use_rename(&mut self, i: &'ast syn::UseRename) {
        let name = i.ident.to_string();
        if RENAMED.contains(&name.as_str()) {
            let message = format!(
                "`{name}` is renamed `{}`, which runner-stderr can't follow",
                i.rename
            );
            self.push(Hit {
                at: i.ident.span(),
                message,
                fix: Fix::Alias,
            });
        }
        syn::visit::visit_use_rename(self, i);
    }
}

/// The violations of one macro call named `name` at `at`, whose arguments are `tokens`. `exempt`
/// is true inside a `Detail::new(...)`, where an error may be formatted.
fn check_macro(name: &str, at: Span, tokens: &TokenStream, exempt: bool, hits: &mut Vec<Hit>) {
    if CONSOLE.contains(&name) {
        hits.push(Hit {
            at,
            message: format!(
                "{name}! writes to the console; the runner says things only through runner::stderr"
            ),
            fix: Fix::Console,
        });
    }
    let parts = split(tokens);
    if WRITES.contains(&name) && parts.first().is_some_and(|part| mentions_stderr(part)) {
        hits.push(Hit {
            at,
            message: format!(
                "{name}! writes to a stderr handle; the runner says things only through \
                 runner::stderr"
            ),
            fix: Fix::WriteStderr,
        });
    }
    if !exempt && FORMATS.contains(&name) {
        for (error, span) in formatted_errors(&parts) {
            hits.push(Hit {
                at: span,
                message: format!(
                    "{name}! formats the error `{error}`; error text can hold a path, so format it \
                     inside Detail::new"
                ),
                fix: Fix::ErrorText,
            });
        }
    }
    scan(tokens, exempt, hits);
}

/// The violations in a macro's `tokens`, at any depth: each `stderr(` call, each error's
/// `.to_string()` outside a `Detail::new(...)`, and each macro call nested in them, judged as
/// [`check_macro`] judges one. A `Detail::new(...)` argument list is scanned with `exempt` set.
pub(super) fn scan(tokens: &TokenStream, exempt: bool, hits: &mut Vec<Hit>) {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let mut at = 0;
    while let Some(tree) = trees.get(at) {
        at += match tree {
            TokenTree::Ident(ident) => scan_ident(&trees, at, ident, exempt, hits),
            TokenTree::Group(group) => {
                scan(&group.stream(), exempt, hits);
                1
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => 1,
        };
    }
}

/// Scans from the identifier `ident` at `at` in `trees`, and returns how many trees it used.
fn scan_ident(
    trees: &[TokenTree],
    at: usize,
    ident: &Ident,
    exempt: bool,
    hits: &mut Vec<Hit>,
) -> usize {
    if ident == "Detail"
        && punct_is(trees.get(at + 1), ':')
        && punct_is(trees.get(at + 2), ':')
        && is_ident(trees.get(at + 3), "new")
        && let Some(TokenTree::Group(args)) = trees.get(at + 4)
    {
        scan(&args.stream(), true, hits);
        return 5;
    }
    if punct_is(trees.get(at + 1), '!')
        && let Some(TokenTree::Group(args)) = trees.get(at + 2)
    {
        check_macro(
            &ident.to_string(),
            ident.span(),
            &args.stream(),
            exempt,
            hits,
        );
        return 3;
    }
    if ident == "stderr" && is_call_group(trees.get(at + 1)) {
        hits.push(Hit {
            at: ident.span(),
            message: STDERR_CALL.to_owned(),
            fix: Fix::StderrCall,
        });
    } else if !exempt
        && is_errorish(&ident.to_string())
        && !at
            .checked_sub(1)
            .is_some_and(|prev| punct_is(trees.get(prev), '.'))
        && punct_is(trees.get(at + 1), '.')
        && is_ident(trees.get(at + 2), "to_string")
        && matches!(trees.get(at + 3), Some(TokenTree::Group(args))
            if is_paren(args) && args.stream().is_empty())
    {
        hits.push(Hit {
            at: ident.span(),
            message: to_string_message(&ident.to_string()),
            fix: Fix::ErrorText,
        });
    }
    1
}

/// The problem a `macro_rules!` body with `tokens` has, if it calls a console macro or names
/// `stderr`, at any depth: `calls eprintln!` or `names stderr`.
pub(super) fn body_problem(tokens: &TokenStream) -> Option<String> {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(ident) if ident == "stderr" => return Some("names stderr".to_owned()),
            TokenTree::Ident(ident)
                if CONSOLE.contains(&ident.to_string().as_str())
                    && punct_is(trees.get(at + 1), '!') =>
            {
                return Some(format!("calls {ident}!"));
            }
            TokenTree::Group(group) => {
                if let Some(problem) = body_problem(&group.stream()) {
                    return Some(problem);
                }
            }
            _ => {}
        }
    }
    None
}

/// The error-ish names a format macro formats in `parts` (its arguments split at top-level
/// commas), with the span each is written at. The format string is the first part that is one
/// string literal. Its placeholders name the errors it formats, and so do the parts after it
/// that are such an identifier (see [`errorish_argument`]).
pub(super) fn formatted_errors(parts: &[Vec<TokenTree>]) -> Vec<(String, Span)> {
    let format = parts
        .iter()
        .enumerate()
        .find_map(|(at, part)| match part.as_slice() {
            [TokenTree::Literal(literal)] => syn::parse_str::<syn::LitStr>(&literal.to_string())
                .ok()
                .map(|lit| (at, literal.span(), lit.value())),
            _ => None,
        });
    let Some((at, span, text)) = format else {
        return Vec::new();
    };
    let mut found: Vec<(String, Span)> = placeholders(&text)
        .into_iter()
        .filter(|name| is_errorish(name))
        .map(|name| (name.to_owned(), span))
        .collect();
    for part in &parts[at + 1..] {
        if let Some(argument) = errorish_argument(part) {
            found.push(argument);
        }
    }
    found
}

/// The error-ish identifier an argument `part` is: just such a name, behind an `&`, or the value
/// of a `name = value` argument.
fn errorish_argument(part: &[TokenTree]) -> Option<(String, Span)> {
    let value = match part {
        [TokenTree::Ident(_), TokenTree::Punct(eq), value @ ..] if eq.as_char() == '=' => value,
        _ => part,
    };
    let value = match value {
        [TokenTree::Punct(and), rest @ ..] if and.as_char() == '&' => rest,
        _ => value,
    };
    match value {
        [TokenTree::Ident(ident)] if is_errorish(&ident.to_string()) => {
            Some((ident.to_string(), ident.span()))
        }
        _ => None,
    }
}

/// The names of the placeholders in a format string: `{name}` and `{name:...}` give `name`, and
/// `{}` or `{0}` give an empty name or a number. `{{` is an escaped brace, not a placeholder.
pub(super) fn placeholders(text: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('{') {
        let after = &rest[at + 1..];
        if let Some(escaped) = after.strip_prefix('{') {
            rest = escaped;
            continue;
        }
        let end = after.find([':', '}']).unwrap_or(after.len());
        names.push(after[..end].trim());
        rest = &after[end..];
    }
    names
}

/// True for an error's name: `e`, `err`, `error`, or one ending in `_err` or `_error`.
pub(super) fn is_errorish(name: &str) -> bool {
    matches!(name, "e" | "err" | "error") || name.ends_with("_err") || name.ends_with("_error")
}

/// True when the attributes include exactly `#[cfg(test)]`: not `cfg(any(test, ..))` nor
/// `cfg(not(test))`, which are read.
pub(super) fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string() == "test")
    })
}

/// True when `path` ends with the two segments `Detail::new`.
fn is_detail_new(path: &syn::Path) -> bool {
    let mut segments = path.segments.iter().rev();
    matches!(
        (segments.next(), segments.next()),
        (Some(new), Some(detail)) if new.ident == "new" && detail.ident == "Detail"
    )
}

/// The last segment's name of a macro's path, or empty.
fn last_name(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

/// The message for `name.to_string()` on an error outside `Detail::new`.
fn to_string_message(name: &str) -> String {
    format!(
        "`{name}.to_string()` formats the error; error text can hold a path, so format it inside \
         Detail::new"
    )
}

/// True when the token is the punctuation `c`.
fn punct_is(tree: Option<&TokenTree>, c: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == c)
}

/// True when the token is the identifier `name`.
fn is_ident(tree: Option<&TokenTree>, name: &str) -> bool {
    matches!(tree, Some(TokenTree::Ident(ident)) if ident == name)
}

/// True for a group in parentheses.
fn is_paren(group: &Group) -> bool {
    group.delimiter() == Delimiter::Parenthesis
}

/// True for the parenthesised group that follows a callee: a call.
fn is_call_group(tree: Option<&TokenTree>) -> bool {
    matches!(tree, Some(TokenTree::Group(group)) if is_paren(group))
}

/// True when the trees name an identifier that contains `stderr`, at any depth.
fn mentions_stderr(trees: &[TokenTree]) -> bool {
    trees.iter().any(|tree| match tree {
        TokenTree::Ident(ident) => ident.to_string().contains("stderr"),
        TokenTree::Group(group) => {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            mentions_stderr(&inner)
        }
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

/// `tokens` split at its top-level commas.
pub(super) fn split(tokens: &TokenStream) -> Vec<Vec<TokenTree>> {
    let mut parts: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tree in tokens.clone() {
        match &tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => parts.push(Vec::new()),
            _ => {
                if let Some(part) = parts.last_mut() {
                    part.push(tree);
                }
            }
        }
    }
    parts
}
